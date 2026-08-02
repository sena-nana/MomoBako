//! MomoBako 的独立 ABI 插件宿主与显式插件路由。
//!
//! 包发现、依赖解析、artifact 校验和 staging 由 Momo 负责；ABI 生命周期、有限队列和
//! wire 请求由 mutsuki-plugin-host 负责。这里仅维护产品 generation、调用计数和协议路由。

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock, RwLock};
use std::thread;
use std::time::{Duration, Instant};

use mutsuki_plugin_api::PluginHostContext;
use mutsuki_plugin_host::{PluginLoadRequest, PluginSession};
use mutsuki_runtime_contracts::{
    BatchEntry, BatchPayload, DispatchLane, OrderingRequirement, PluginManifest, RowPayload,
    RunnerContext, RunnerStatus, Task, TaskLease, WorkBatch, WorkResourcePlan,
};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::services::mutsuki_runner::MomoTaskRuntime;
use crate::services::repository::{
    extract_zip_plugin, native_plugin_specs, plugin_data_dir, NativePluginSpec,
};
use crate::services::runtime::RepositoryRuntime;

const PLUGIN_RELOAD_DRAIN_TIMEOUT: Duration = Duration::from_secs(30);

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
    session: Option<Arc<PluginSession>>,
    generation: u64,
    active_calls: AtomicUsize,
    draining: AtomicBool,
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
        let next = self.load_slots(generation);
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

    fn load_slots(&self, generation: u64) -> BTreeMap<String, Arc<PluginSlot>> {
        native_plugin_specs(&self.runtime.service_root())
            .into_iter()
            .map(|spec| {
                let plugin_id = spec.manifest.plugin_id.clone();
                let slot = match load_plugin_session(
                    &self.runtime,
                    &self.task_runtime,
                    &spec,
                ) {
                    Ok(session) => PluginSlot {
                        session: Some(Arc::new(session)),
                        generation,
                        active_calls: AtomicUsize::new(0),
                        draining: AtomicBool::new(false),
                        error: None,
                    },
                    Err(error) => {
                        crate::app_log!(
                            "error",
                            "plugin.runtime",
                            "loadFailed",
                            "独立 ABI 插件加载失败。",
                            serde_json::json!({ "pluginId": plugin_id, "generation": generation, "error": error })
                        );
                        PluginSlot {
                            session: None,
                            generation,
                            active_calls: AtomicUsize::new(0),
                            draining: AtomicBool::new(false),
                            error: Some(error),
                        }
                    }
                };
                (plugin_id, Arc::new(slot))
            })
            .collect()
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
            call_session(
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

    fn unavailable_reason(&self, plugin_id: &str) -> Option<String> {
        let slot = self.slots.read().ok()?.get(plugin_id).cloned()?;
        if slot.draining.load(Ordering::Acquire) {
            return Some("插件正在重新加载。".to_string());
        }
        slot.error.clone()
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
    active_host()
        .ok()
        .and_then(|host| host.unavailable_reason(plugin_id))
        .or_else(|| Some("需要独立 ABI v2 插件 Host。".to_string()))
}

pub fn reload_plugins() -> Result<(), String> {
    active_host()?.reload()
}

fn load_plugin_session(
    runtime: &RepositoryRuntime,
    task_runtime: &Arc<MomoTaskRuntime>,
    spec: &NativePluginSpec,
) -> Result<PluginSession, String> {
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
    let host_context = PluginHostContext::default().with_task_gateway(task_runtime.clone());
    PluginSession::load(PluginLoadRequest {
        library_path,
        expected_manifest,
        config: Some(config),
        host_context,
        host_config: Default::default(),
    })
    .map_err(|error| error.to_string())
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
            .join(cache_name(&spec.manifest.plugin_id, &spec.manifest.version));
        fs::create_dir_all(&cache_root).map_err(|error| error.to_string())?;
        let file = File::open(&spec.archive_path).map_err(|error| error.to_string())?;
        let mut archive = zip::ZipArchive::new(file).map_err(|error| error.to_string())?;
        extract_zip_plugin(&mut archive, &spec.manifest_prefix, &cache_root)?;
        cache_root
    };
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
    Ok((stage_root, expected_manifest))
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

fn cache_name(plugin_id: &str, version: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(plugin_id.as_bytes());
    hasher.update(b"\n");
    hasher.update(version.as_bytes());
    format!("plugin-{}", hex::encode(hasher.finalize()))
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
            let _ = session.dispose();
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn plugin_target_is_explicitly_plugin_scoped() {
        let plugin_id = "momobako.source.eagle-library";
        let method = "filesystem.listFiles";
        assert_eq!(
            format!("binding:{plugin_id}:momobako.{method}"),
            "binding:momobako.source.eagle-library:momobako.filesystem.listFiles"
        );
    }
}
