//! Mutsuki 桌面宿主的进程级接线与 Momo 兼容调用适配。

use mutsuki_runtime_contracts::TaskOutcome;
use mutsuki_tauri_bridge::{FrontendContext, FrontendTaskRequest};
use mutsuki_tauri_host::{MutsukiTauriHost, PluginSelection};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, OnceLock, RwLock},
    time::Duration,
};

const PLUGIN_RELOAD_DRAIN_TIMEOUT: Duration = Duration::from_secs(30);

static HOST: OnceLock<RwLock<Option<Arc<MutsukiTauriHost>>>> = OnceLock::new();

fn host_slot() -> &'static RwLock<Option<Arc<MutsukiTauriHost>>> {
    HOST.get_or_init(|| RwLock::new(None))
}

/// 让 RepositoryService 内部的插件调用复用 Tauri 管理的唯一 Host。
pub fn install_host(host: Arc<MutsukiTauriHost>) -> Result<(), String> {
    let mut slot = host_slot()
        .write()
        .map_err(|_| "Mutsuki Host 状态锁已损坏。".to_string())?;
    *slot = Some(host);
    Ok(())
}

fn active_host() -> Result<Arc<MutsukiTauriHost>, String> {
    host_slot()
        .read()
        .map_err(|_| "Mutsuki Host 状态锁已损坏。".to_string())?
        .clone()
        .ok_or_else(|| "Mutsuki Host 尚未启动。".to_string())
}

/// 将旧的 `callPlugin` 方法名映射到声明式 `momobako.*` protocol。
pub fn call_plugin(plugin_id: &str, method: &str, payload: Value) -> Result<Value, String> {
    let protocol_id = format!("momobako.{}", method.trim().trim_start_matches("momobako."));
    let (binding_id, runner_id) = plugin_target(plugin_id, &protocol_id);
    execute_task(
        &protocol_id,
        payload,
        Some((binding_id.as_str(), runner_id.as_str())),
    )
    .map(|(output, _)| output)
    .map_err(|error| format!("{error} [plugin={plugin_id}, method={method}]"))
}

fn plugin_target(plugin_id: &str, protocol_id: &str) -> (String, String) {
    (
        format!("binding:{plugin_id}:{protocol_id}"),
        format!("{plugin_id}.runner"),
    )
}

fn execute_task(
    protocol_id: &str,
    payload: Value,
    target: Option<(&str, &str)>,
) -> Result<(Value, Vec<Value>), String> {
    let (target_binding_id, runner_hint) = target
        .map(|(binding, runner)| (Some(binding.to_string()), Some(runner.to_string())))
        .unwrap_or_default();
    let result = active_host()?
        .call(FrontendTaskRequest {
            protocol_id: protocol_id.to_string(),
            payload,
            task_id: None,
            trace_id: None,
            correlation_id: None,
            idempotency_key: None,
            target_binding_id,
            runner_hint,
            input_refs: Vec::new(),
            priority: 0,
            context: FrontendContext::default(),
        })
        .map_err(|error| error.to_string())?;
    let progress = result
        .events
        .iter()
        .filter(|event| event.name == "momobako.task.progress")
        .filter_map(|event| event.attributes.get("payload"))
        .filter_map(|value| match value {
            mutsuki_runtime_contracts::ScalarValue::String(value) => {
                serde_json::from_str::<Value>(value).ok()
            }
            _ => None,
        })
        .filter_map(|payload| payload.get("progress").cloned())
        .collect();
    match result.outcome {
        Some(TaskOutcome::Completed { output, .. }) => {
            Ok((output.unwrap_or(Value::Null), progress))
        }
        Some(TaskOutcome::Failed { error, .. }) => Err(format!(
            "plugin task failed: {:?} ({})",
            error.evidence, error.route
        )),
        Some(TaskOutcome::Cancelled { reason, .. }) => Err(format!(
            "plugin task cancelled: {}",
            reason.unwrap_or_else(|| protocol_id.to_string())
        )),
        Some(TaskOutcome::Expired { reason, .. }) => Err(format!(
            "plugin task expired: {}",
            reason.unwrap_or_else(|| protocol_id.to_string())
        )),
        Some(TaskOutcome::DeadLetter { reason, .. }) => Err(format!(
            "plugin task entered dead letter: {}",
            reason.unwrap_or_else(|| protocol_id.to_string())
        )),
        None => Err(format!("task finished without outcome: {protocol_id}")),
    }
}

/// 在 blocking pool 中执行内建长任务，并返回可供旧 Channel 回放的进度。
pub async fn call_long_task<Request>(
    protocol_id: &'static str,
    request: Request,
) -> Result<(Value, Vec<Value>), String>
where
    Request: Serialize + Send + 'static,
{
    let payload = serde_json::to_value(request).map_err(|error| error.to_string())?;
    tauri::async_runtime::spawn_blocking(move || execute_task(protocol_id, payload, None))
        .await
        .map_err(|error| error.to_string())?
}

/// 若 Host 已启动，返回执行型插件不可用的精确原因。
pub fn plugin_unavailable_reason(plugin_id: &str) -> Option<String> {
    let host = host_slot().read().ok()?.clone()?;
    let summary = host
        .plugins()
        .into_iter()
        .find(|plugin| plugin.plugin_id == plugin_id);
    match summary {
        Some(plugin) if plugin.status == "loaded" => None,
        Some(plugin) if plugin.status == "disabled" => Some("插件已停用。".to_string()),
        Some(plugin) => Some(
            plugin
                .error
                .unwrap_or_else(|| format!("需要 ABI v2 版本（状态：{}）。", plugin.status)),
        ),
        None => Some("需要 ABI v2 版本。".to_string()),
    }
}

/// 原子重扫插件；失败时 TauriHost 保留旧 generation。
pub fn reload_plugins(selection: PluginSelection) -> Result<(), String> {
    active_host()?
        .reload_plugins(selection, PLUGIN_RELOAD_DRAIN_TIMEOUT)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// 构造 Momo 配置目录保持不变时需要传给 ABI guest 的初始化快照。
pub fn plugin_selection(
    enabled_plugin_ids: BTreeSet<String>,
    configs: BTreeMap<String, Value>,
) -> PluginSelection {
    PluginSelection {
        enabled_plugin_ids: Some(enabled_plugin_ids),
        configs,
    }
}

#[cfg(test)]
mod tests {
    use super::plugin_target;

    #[test]
    fn plugin_target_scopes_shared_protocol_to_plugin() {
        let protocol = "momobako.filesystem.listFiles";
        let local = plugin_target("momobako.local-filesystem", protocol);
        let eagle = plugin_target("momobako.source.eagle-library", protocol);

        assert_eq!(
            local,
            (
                "binding:momobako.local-filesystem:momobako.filesystem.listFiles".to_string(),
                "momobako.local-filesystem.runner".to_string(),
            )
        );
        assert_ne!(local.0, eagle.0);
    }
}
