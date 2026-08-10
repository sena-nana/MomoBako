//! MomoBako 的独立 ABI 插件宿主与显式插件路由。
//!
//! 包发现、依赖解析、artifact 校验和 staging 由 Momo 负责；ABI 生命周期、有限队列和
//! wire 请求由 mutsuki-plugin-host 负责。这里仅维护产品 generation、调用计数和协议路由。

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::thread;
use std::time::{Duration, Instant};

use mutsuki_plugin_api::PluginHostContext;
use mutsuki_plugin_host::{PluginLoadRequest, PluginSession};
use mutsuki_runtime_contracts::{
    BatchEntry, BatchPayload, DispatchLane, OrderingRequirement, PluginManifest, RowPayload,
    RunnerContext, RunnerStatus, Task, TaskLease, WorkBatch, WorkResourcePlan,
};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::services::mutsuki_runner::MomoTaskRuntime;
use crate::services::repository::{
    extract_zip_plugin, native_plugin_specs, plugin_data_dir, NativePluginSpec,
};
use crate::services::runtime::RepositoryRuntime;

const PLUGIN_RELOAD_DRAIN_TIMEOUT: Duration = Duration::from_secs(30);
const PROCESS_CALL_TIMEOUT: Duration = Duration::from_secs(30);

static HOST: OnceLock<RwLock<Option<Arc<MomoPluginRuntime>>>> = OnceLock::new();

fn host_slot() -> &'static RwLock<Option<Arc<MomoPluginRuntime>>> {
    HOST.get_or_init(|| RwLock::new(None))
}

/// 独立插件运行时的一个已加载 generation。
pub struct MomoPluginRuntime {
    runtime: RepositoryRuntime,
    task_runtime: Arc<MomoTaskRuntime>,
    slots: RwLock<BTreeMap<String, Arc<PluginSlot>>>,
    generation: AtomicU64,
    next_invocation: AtomicU64,
}

struct PluginSlot {
    session: Option<PluginBackendSession>,
    generation: u64,
    active_calls: AtomicUsize,
    draining: AtomicBool,
    error: Option<String>,
}

#[derive(Clone)]
enum PluginBackendSession {
    Abi(Arc<PluginSession>),
    Process(Arc<ProcessPluginSession>),
}

struct ProcessPluginSession {
    plugin_id: String,
    runtime: Value,
    stdin: Mutex<ChildStdin>,
    responses: Mutex<Receiver<Result<String, String>>>,
    child: Mutex<Child>,
    call_lock: Mutex<()>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProcessPluginResponse {
    id: u64,
    ok: bool,
    output: Option<Value>,
    error: Option<String>,
}

impl MomoPluginRuntime {
    pub fn new(runtime: RepositoryRuntime, task_runtime: Arc<MomoTaskRuntime>) -> Self {
        Self {
            runtime,
            task_runtime,
            slots: RwLock::new(BTreeMap::new()),
            generation: AtomicU64::new(0),
            next_invocation: AtomicU64::new(1),
        }
    }

    /// 加载新的插件 generation，并对旧 session 执行 drain-and-swap。
    pub fn reload(&self) -> Result<(), String> {
        let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        let next = self.load_slots(generation)?;
        let previous = {
            let mut slots = self
                .slots
                .write()
                .map_err(|_| "Momo 插件 generation 状态锁已损坏。".to_string())?;
            let previous = std::mem::replace(&mut *slots, next);
            for slot in previous.values() {
                slot.draining.store(true, Ordering::Release);
            }
            previous
        };
        drain_slots(previous);
        Ok(())
    }

    fn load_slots(&self, generation: u64) -> Result<BTreeMap<String, Arc<PluginSlot>>, String> {
        let mut slots = BTreeMap::new();
        for spec in native_plugin_specs(&self.runtime.service_root()) {
            let plugin_id = spec.manifest.plugin_id.clone();
            let session = match load_plugin_session(&self.runtime, &self.task_runtime, &spec) {
                Ok(session) => session,
                Err(error) => {
                    crate::app_log!(
                        "error",
                        "plugin.runtime",
                        "candidateLoadFailed",
                        "候选插件 generation 加载失败，继续保留当前 generation。",
                        serde_json::json!({ "pluginId": plugin_id, "generation": generation, "error": error })
                    );
                    drain_slots(slots);
                    return Err(format!(
                        "plugin generation {generation} rejected by {plugin_id}: {error}"
                    ));
                }
            };
            slots.insert(
                plugin_id,
                Arc::new(PluginSlot {
                    session: Some(session),
                    generation,
                    active_calls: AtomicUsize::new(0),
                    draining: AtomicBool::new(false),
                    error: None,
                }),
            );
        }
        Ok(slots)
    }

    fn call(&self, plugin_id: &str, method: &str, payload: Value) -> Result<Value, String> {
        let slot = self
            .slots
            .read()
            .map_err(|_| "Momo 插件 generation 状态锁已损坏。".to_string())?
            .get(plugin_id)
            .cloned()
            .ok_or_else(|| format!("plugin is unavailable: {plugin_id}"))?;
        if slot.draining.load(Ordering::Acquire) {
            return Err(format!("plugin is reloading: {plugin_id}"));
        }
        let session = slot.session.as_ref().cloned().ok_or_else(|| {
            slot.error
                .clone()
                .unwrap_or_else(|| "插件未加载。".to_string())
        })?;
        slot.active_calls.fetch_add(1, Ordering::AcqRel);
        let result = if slot.draining.load(Ordering::Acquire) {
            Err(format!("plugin is reloading: {plugin_id}"))
        } else {
            call_backend_session(
                &session,
                slot.generation,
                &self.next_invocation,
                method,
                payload,
            )
        };
        slot.active_calls.fetch_sub(1, Ordering::AcqRel);
        result
    }

    fn unavailable_reason(&self, plugin_id: &str) -> Result<Option<String>, String> {
        let slots = self
            .slots
            .read()
            .map_err(|_| "Momo 插件 generation 状态锁已损坏。".to_string())?;
        plugin_slot_unavailable_reason(plugin_id, slots.get(plugin_id).map(Arc::as_ref))
    }
}

/// 将当前 generation 的插件槽位收窄为可用、不可用或状态错误三种结果。
fn plugin_slot_unavailable_reason(
    plugin_id: &str,
    slot: Option<&PluginSlot>,
) -> Result<Option<String>, String> {
    let slot = slot.ok_or_else(|| format!("插件未加载到当前 Host generation：{plugin_id}"))?;
    if slot.draining.load(Ordering::Acquire) {
        return Ok(Some("插件正在重新加载。".to_string()));
    }
    if let Some(error) = slot.error.as_ref() {
        return Ok(Some(error.clone()));
    }
    if slot.session.is_none() {
        return Ok(Some("插件未加载。".to_string()));
    }
    Ok(None)
}

/// 保留健康插件的 None，同时将 Host 或插件状态错误转为不可用原因。
fn resolve_plugin_unavailable_reason(reason: Result<Option<String>, String>) -> Option<String> {
    match reason {
        Ok(reason) => reason,
        Err(error) => Some(error),
    }
}

/// 让 RepositoryService 内部的插件调用复用 Momo 管理的唯一独立 Host。
pub fn install_host(host: Arc<MomoPluginRuntime>) -> Result<(), String> {
    let mut slot = host_slot()
        .write()
        .map_err(|_| "Momo 插件 Host 状态锁已损坏。".to_string())?;
    *slot = Some(host);
    Ok(())
}

fn active_host() -> Result<Arc<MomoPluginRuntime>, String> {
    host_slot()
        .read()
        .map_err(|_| "Momo 插件 Host 状态锁已损坏。".to_string())?
        .clone()
        .ok_or_else(|| "Momo 独立插件 Host 尚未启动。".to_string())
}

pub fn call_plugin(plugin_id: &str, method: &str, payload: Value) -> Result<Value, String> {
    let protocol_id = format!("momobako.{}", method.trim().trim_start_matches("momobako."));
    active_host()?.call(plugin_id, &protocol_id, payload)
}

pub fn plugin_unavailable_reason(plugin_id: &str) -> Option<String> {
    resolve_plugin_unavailable_reason(
        active_host().and_then(|host| host.unavailable_reason(plugin_id)),
    )
}

pub fn reload_plugins() -> Result<(), String> {
    active_host()?.reload()
}

fn load_plugin_session(
    runtime: &RepositoryRuntime,
    task_runtime: &Arc<MomoTaskRuntime>,
    spec: &NativePluginSpec,
) -> Result<PluginBackendSession, String> {
    let (stage_root, expected_manifest) = stage_plugin(spec, &runtime.service_root())?;
    let data_dir = plugin_data_dir(&runtime.service_root(), &spec.manifest.plugin_id);
    fs::create_dir_all(&data_dir).map_err(|error| error.to_string())?;
    let config_values = runtime
        .repository_state
        .get_plugin_config(spec.manifest.plugin_id.clone())
        .map(|snapshot| snapshot.values)
        .unwrap_or_default();
    let config = serde_json::json!({
        "pluginId": spec.manifest.plugin_id,
        "pluginDataDir": data_dir,
        "serviceRootDir": runtime.service_root(),
        "pluginConfig": config_values,
        "_mutsuki": { "runtime_dir": stage_root },
    });
    let library_path = artifact_path(&stage_root, &expected_manifest.artifact.path)?;
    if spec.manifest.runtime == "process" {
        return spawn_process_plugin_session(
            spec.manifest.plugin_id.clone(),
            library_path,
            stage_root,
            config,
        )
        .map(|session| PluginBackendSession::Process(Arc::new(session)));
    }
    let host_context = PluginHostContext::default().with_task_gateway(task_runtime.clone());
    PluginSession::load(PluginLoadRequest {
        library_path,
        expected_manifest,
        config: Some(config),
        host_context,
        host_config: Default::default(),
    })
    .map(|session| PluginBackendSession::Abi(Arc::new(session)))
    .map_err(|error| error.to_string())
}

/// 启动逐行 JSON 进程 runner；崩溃或硬超时只终止当前插件进程。
fn spawn_process_plugin_session(
    plugin_id: String,
    executable: PathBuf,
    runtime_dir: PathBuf,
    runtime: Value,
) -> Result<ProcessPluginSession, String> {
    let mut child = Command::new(&executable)
        .current_dir(runtime_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|error| format!("failed to start process plugin {plugin_id}: {error}"))?;
    let Some(stdin) = child.stdin.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(format!("process plugin stdin unavailable: {plugin_id}"));
    };
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(format!("process plugin stdout unavailable: {plugin_id}"));
    };
    let (sender, receiver) = mpsc::channel();
    if let Err(error) = thread::Builder::new()
        .name(format!("momo-plugin-{plugin_id}"))
        .spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let response = line.map_err(|error| error.to_string());
                if sender.send(response).is_err() {
                    break;
                }
            }
        })
    {
        let _ = child.kill();
        let _ = child.wait();
        return Err(format!(
            "failed to start process reader {plugin_id}: {error}"
        ));
    }
    Ok(ProcessPluginSession {
        plugin_id,
        runtime,
        stdin: Mutex::new(stdin),
        responses: Mutex::new(receiver),
        child: Mutex::new(child),
        call_lock: Mutex::new(()),
    })
}

fn stage_plugin(
    spec: &NativePluginSpec,
    service_root: &Path,
) -> Result<(PathBuf, PluginManifest), String> {
    let stage_root = if spec.archive_path.is_dir() {
        spec.archive_path.clone()
    } else {
        let cache_root = service_root
            .join("mutsuki-plugin-cache")
            .join(cache_name(&spec.package_hash)?);
        if !cache_root.is_dir() {
            let cache_parent = cache_root
                .parent()
                .ok_or_else(|| "invalid plugin cache path".to_string())?;
            fs::create_dir_all(cache_parent).map_err(|error| error.to_string())?;
            let temporary_root = cache_parent.join(format!(
                ".staging-{}-{}",
                std::process::id(),
                spec.package_hash.trim_start_matches("sha256:")
            ));
            if temporary_root.exists() {
                fs::remove_dir_all(&temporary_root).map_err(|error| error.to_string())?;
            }
            fs::create_dir_all(&temporary_root).map_err(|error| error.to_string())?;
            let file = File::open(&spec.archive_path).map_err(|error| error.to_string())?;
            let mut archive = zip::ZipArchive::new(file).map_err(|error| error.to_string())?;
            if let Err(error) =
                extract_zip_plugin(&mut archive, &spec.manifest_prefix, &temporary_root)
            {
                let _ = fs::remove_dir_all(&temporary_root);
                return Err(error);
            }
            if let Err(error) = read_staged_plugin_manifest(&temporary_root, spec) {
                let _ = fs::remove_dir_all(&temporary_root);
                return Err(error);
            }
            match fs::rename(&temporary_root, &cache_root) {
                Ok(()) => {}
                Err(_) if cache_root.is_dir() => {
                    let _ = fs::remove_dir_all(&temporary_root);
                }
                Err(error) => {
                    let _ = fs::remove_dir_all(&temporary_root);
                    return Err(error.to_string());
                }
            }
        }
        cache_root
    };
    let expected_manifest = read_staged_plugin_manifest(&stage_root, spec)?;
    Ok((stage_root, expected_manifest))
}

fn read_staged_plugin_manifest(
    stage_root: &Path,
    spec: &NativePluginSpec,
) -> Result<PluginManifest, String> {
    let manifest_path = stage_root.join("plugin.toml");
    let manifest_raw = fs::read_to_string(&manifest_path).map_err(|error| {
        format!(
            "plugin.toml unavailable for {}: {error}",
            spec.manifest.plugin_id
        )
    })?;
    let expected_manifest = toml::from_str::<PluginManifest>(&manifest_raw).map_err(|error| {
        format!(
            "invalid plugin.toml for {}: {error}",
            spec.manifest.plugin_id
        )
    })?;
    if expected_manifest.plugin_id != spec.manifest.plugin_id
        || expected_manifest.version != spec.manifest.version
    {
        return Err(format!(
            "Momo/Mutsuki manifest mismatch for {}",
            spec.manifest.plugin_id
        ));
    }
    let library_path = artifact_path(&stage_root, &expected_manifest.artifact.path)?;
    verify_artifact_hash(&library_path, &expected_manifest.artifact.sha256)?;
    Ok(expected_manifest)
}

fn artifact_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let relative_path = Path::new(relative);
    if relative_path.as_os_str().is_empty()
        || relative_path.is_absolute()
        || relative_path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::Prefix(_)))
    {
        return Err(format!("invalid plugin artifact path: {relative}"));
    }
    let path = root.join(relative_path);
    if !path.is_file() {
        return Err(format!("plugin artifact is missing: {}", path.display()));
    }
    Ok(path)
}

fn verify_artifact_hash(path: &Path, expected: &str) -> Result<(), String> {
    let expected = expected
        .strip_prefix("sha256:")
        .ok_or_else(|| format!("plugin artifact hash is invalid: {expected}"))?;
    let mut hasher = Sha256::new();
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let actual = hex::encode(hasher.finalize());
    if actual != expected {
        return Err(format!(
            "plugin artifact hash mismatch: expected sha256:{expected}, got sha256:{actual}"
        ));
    }
    Ok(())
}

fn cache_name(package_hash: &str) -> Result<String, String> {
    let digest = package_hash
        .strip_prefix("sha256:")
        .filter(|value| {
            value.len() == 64
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        })
        .ok_or_else(|| format!("invalid plugin package hash: {package_hash}"))?;
    Ok(format!("plugin-{digest}"))
}

fn call_backend_session(
    session: &PluginBackendSession,
    generation: u64,
    next_invocation: &AtomicU64,
    protocol_id: &str,
    payload: Value,
) -> Result<Value, String> {
    match session {
        PluginBackendSession::Abi(session) => {
            call_session(session, generation, next_invocation, protocol_id, payload)
        }
        PluginBackendSession::Process(session) => {
            call_process_session(session, next_invocation, protocol_id, payload)
        }
    }
}

fn call_process_session(
    session: &ProcessPluginSession,
    next_invocation: &AtomicU64,
    protocol_id: &str,
    payload: Value,
) -> Result<Value, String> {
    let _call = session
        .call_lock
        .lock()
        .map_err(|_| format!("process plugin call lock poisoned: {}", session.plugin_id))?;
    let request_id = next_invocation.fetch_add(1, Ordering::Relaxed);
    let method = protocol_id.strip_prefix("momobako.").unwrap_or(protocol_id);
    let request = serde_json::json!({
        "id": request_id,
        "method": method,
        "payload": payload,
        "runtime": session.runtime,
    });
    {
        let mut stdin = session
            .stdin
            .lock()
            .map_err(|_| format!("process plugin stdin lock poisoned: {}", session.plugin_id))?;
        serde_json::to_writer(&mut *stdin, &request).map_err(|error| error.to_string())?;
        stdin.write_all(b"\n").map_err(|error| error.to_string())?;
        stdin.flush().map_err(|error| error.to_string())?;
    }
    let response_line = session
        .responses
        .lock()
        .map_err(|_| {
            format!(
                "process plugin response lock poisoned: {}",
                session.plugin_id
            )
        })?
        .recv_timeout(PROCESS_CALL_TIMEOUT)
        .map_err(|error| {
            terminate_process_session(session);
            format!(
                "process plugin call timed out or exited: {} ({error})",
                session.plugin_id
            )
        })??;
    let response = serde_json::from_str::<ProcessPluginResponse>(&response_line)
        .map_err(|error| format!("invalid process plugin response: {error}"))?;
    if response.id != request_id {
        terminate_process_session(session);
        return Err(format!(
            "process plugin response id mismatch: expected {request_id}, got {}",
            response.id
        ));
    }
    if response.ok {
        Ok(response.output.unwrap_or(Value::Null))
    } else {
        Err(response
            .error
            .unwrap_or_else(|| "process plugin returned an unspecified error".to_string()))
    }
}

fn terminate_process_session(session: &ProcessPluginSession) {
    if let Ok(mut child) = session.child.lock() {
        let _ = child.kill();
        let _ = child.wait();
    }
}

fn call_session(
    session: &PluginSession,
    generation: u64,
    next_invocation: &AtomicU64,
    protocol_id: &str,
    payload: Value,
) -> Result<Value, String> {
    let runner_id = format!("{}.runner", session.manifest().plugin_id);
    let runner = session
        .runner(&runner_id)
        .or_else(|| session.runners().next().cloned())
        .ok_or_else(|| format!("plugin runner is unavailable: {runner_id}"))?;
    let invocation_id = format!(
        "momobako.plugin.{}",
        next_invocation.fetch_add(1, Ordering::Relaxed)
    );
    let task_id = format!("{invocation_id}.task");
    let batch_id = format!("{invocation_id}.batch");
    let lease_id = format!("{invocation_id}.lease");
    let binding_id = format!("binding:{}:{protocol_id}", session.manifest().plugin_id);
    let mut task = Task::new(task_id.clone(), protocol_id, payload);
    task.target_binding_id = Some(binding_id);
    task.runner_hint = Some(runner.descriptor().runner_id.clone());
    let lease = TaskLease {
        lease_id: lease_id.clone(),
        task_id: task_id.clone(),
        attempt_generation: 1,
        runner_id: runner.descriptor().runner_id.clone(),
        executor_id: "momobako.plugin-host".to_string(),
        registry_generation: generation,
        acquired_at_step: 0,
        expires_at_step: None,
    };
    let entry = BatchEntry {
        entry_id: format!("{invocation_id}.entry"),
        task_id: task_id.clone(),
        trace_id: task.trace_id.clone(),
        parent_id: None,
        payload_index: 0,
        resource_requirement_indices: Vec::new(),
        cancel_index: None,
        deadline_tick: None,
        priority: task.priority,
        lane: DispatchLane::Normal,
        ordering: OrderingRequirement::None,
    };
    let batch = WorkBatch {
        batch_id: batch_id.clone(),
        tick_id: format!("{invocation_id}.tick"),
        batch_key: format!("plugin:{}", session.manifest().plugin_id),
        entries: vec![entry],
        payload: BatchPayload::Row(RowPayload {
            rows: vec![serde_json::to_value(&task).map_err(|error| error.to_string())?],
        }),
        resource_plan: WorkResourcePlan::empty(),
        task_leases: vec![lease],
    };
    let context = RunnerContext::new(
        generation,
        0,
        "momobako.plugin-host",
        vec![lease_id],
        invocation_id,
    )
    .with_batch(batch_id, 1);
    let completion = runner
        .run_batch(context, batch)
        .map_err(|error| format!("{}: {:?}", error.error.route, error.error.evidence))?;
    let result = completion
        .results
        .into_iter()
        .next()
        .ok_or_else(|| "plugin returned an empty completion batch".to_string())?;
    if let Some(error) = result.error {
        return Err(format!(
            "plugin task failed: {} {:?}",
            error.route, error.evidence
        ));
    }
    let result = result
        .result
        .ok_or_else(|| "plugin returned no runner result".to_string())?;
    if result.status != RunnerStatus::Completed {
        return Err(format!("plugin task did not complete: {:?}", result.status));
    }
    Ok(result.output.unwrap_or(Value::Null))
}

fn drain_slots(slots: BTreeMap<String, Arc<PluginSlot>>) {
    let started = Instant::now();
    for (plugin_id, slot) in slots.into_iter() {
        while slot.active_calls.load(Ordering::Acquire) != 0
            && started.elapsed() < PLUGIN_RELOAD_DRAIN_TIMEOUT
        {
            thread::sleep(Duration::from_millis(10));
        }
        if slot.active_calls.load(Ordering::Acquire) != 0 {
            crate::app_log!(
                "error",
                "plugin.runtime",
                "drainTimeout",
                "独立 ABI 插件 reload drain 超时。",
                serde_json::json!({ "pluginId": plugin_id, "activeCalls": slot.active_calls.load(Ordering::Acquire) })
            );
        }
        if let Some(session) = slot.session.as_ref() {
            match session {
                PluginBackendSession::Abi(session) => {
                    let _ = session.dispose();
                }
                PluginBackendSession::Process(session) => terminate_process_session(session),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_target_is_explicitly_plugin_scoped() {
        let plugin_id = "momobako.source.eagle-library";
        let method = "filesystem.listFiles";
        assert_eq!(
            format!("binding:{plugin_id}:momobako.{method}"),
            "binding:momobako.source.eagle-library:momobako.filesystem.listFiles"
        );
    }

    #[test]
    fn healthy_plugin_reason_remains_none() {
        assert_eq!(resolve_plugin_unavailable_reason(Ok(None)), None);
    }

    #[test]
    fn plugin_and_host_errors_are_preserved() {
        assert_eq!(
            resolve_plugin_unavailable_reason(Ok(Some("插件加载失败。".to_string()))),
            Some("插件加载失败。".to_string())
        );
        assert_eq!(
            resolve_plugin_unavailable_reason(Err("Momo 独立插件 Host 尚未启动。".to_string())),
            Some("Momo 独立插件 Host 尚未启动。".to_string())
        );
    }

    #[test]
    fn missing_plugin_generation_is_unavailable() {
        let error = plugin_slot_unavailable_reason("momobako.local-filesystem", None)
            .expect_err("missing generation slot should be unavailable");

        assert!(error.contains("momobako.local-filesystem"));
        assert!(error.contains("未加载到当前 Host generation"));
    }

    #[test]
    fn unloaded_and_draining_slots_remain_unavailable() {
        let unloaded = PluginSlot {
            session: None,
            generation: 1,
            active_calls: AtomicUsize::new(0),
            draining: AtomicBool::new(false),
            error: None,
        };
        assert_eq!(
            plugin_slot_unavailable_reason("momobako.local-filesystem", Some(&unloaded)),
            Ok(Some("插件未加载。".to_string()))
        );

        let draining = PluginSlot {
            draining: AtomicBool::new(true),
            ..unloaded
        };
        assert_eq!(
            plugin_slot_unavailable_reason("momobako.local-filesystem", Some(&draining)),
            Ok(Some("插件正在重新加载。".to_string()))
        );
    }
}
