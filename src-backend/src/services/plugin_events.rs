//! 插件调用交回的事件。
//!
//! 插件 SDK（`External/Plugins/_sdk/mutsuki-rust`）把 `write_host_log_silently` 写的日志作为 `momobako.plugin.log`
//! 事件放进这次调用的结果（`RunnerResult.events`），载荷是 `{ level, action, message, pluginId, context }`；调用失败
//! 时也带着。宿主收下结果时先把它们写进系统日志，再交回输出或错误。日志的来源记成这个插件（`backend-plugin`、插件
//! 标识、分类 `plugin.backend`），和迁到 Mutsuki 以前的 `system.log.write` 宿主桥一样；插件标识取宿主加载的这个
//! 插件，不信载荷里自报的 `pluginId`。调用载荷里有 `repoId`（宿主按 `repositoryId` 调用时自己写进去的）时，日志也记
//! 在这个资源库名下。
//!
//! 插件调用是一次性的请求，宿主没有别的事件的消费方：别的种类的事件丢弃，同一个插件的同一种事件只记一次原因。

use std::collections::BTreeSet;
use std::sync::{Mutex, OnceLock};

use mutsuki_runtime_contracts::{DomainEvent, EntryCompletion, RunnerStatus};
use serde_json::Value;

use crate::services::logging;
use crate::services::repository::{SystemLogLocationInput, SystemLogWriteRequest};

/// 插件日志事件的种类（插件 SDK 的 `PLUGIN_LOG_EVENT`）。
pub(crate) const PLUGIN_LOG_EVENT: &str = "momobako.plugin.log";

/// 已经记过「丢弃」的插件和事件种类，每一对只记一次。
static DROPPED_KINDS: OnceLock<Mutex<BTreeSet<(String, String)>>> = OnceLock::new();

/// 收下插件一次调用的结果：先把交回的事件交给 `write`（日志写进系统日志），再按成败交回输出或错误。
pub(crate) fn settle_call(
    plugin_id: &str,
    repo_id: Option<&str>,
    entry: EntryCompletion,
    write: &mut dyn FnMut(SystemLogWriteRequest),
) -> Result<Value, String> {
    if let Some(result) = entry.result.as_ref() {
        forward_events(plugin_id, repo_id, &result.events, write);
    }
    if let Some(error) = entry.error {
        return Err(format!(
            "plugin task failed: {} {:?}",
            error.route, error.evidence
        ));
    }
    let result = entry
        .result
        .ok_or_else(|| "plugin returned no runner result".to_string())?;
    if result.status != RunnerStatus::Completed {
        return Err(format!("plugin task did not complete: {:?}", result.status));
    }
    Ok(result.output.unwrap_or(Value::Null))
}

/// 产品里的 `write`：写进系统日志。日志器不在时只记标准错误，不影响插件调用的结果。
pub(crate) fn write_system_log(request: SystemLogWriteRequest) {
    let action = request.action.clone();
    if let Err(error) = logging::write_log(request) {
        eprintln!("[momobako] 插件日志写不进系统日志：{action}：{error}");
    }
}

/// 日志事件换成系统日志的写入请求；别的事件丢弃，第一次见到这个插件的这种事件时记一条原因。
fn forward_events(
    plugin_id: &str,
    repo_id: Option<&str>,
    events: &[DomainEvent],
    write: &mut dyn FnMut(SystemLogWriteRequest),
) {
    for event in events {
        if event.kind == PLUGIN_LOG_EVENT {
            write(plugin_log(plugin_id, repo_id, &event.payload));
        } else if first_drop(plugin_id, &event.kind) {
            write(dropped_event(plugin_id, &event.kind));
        }
    }
}

/// 一条插件日志：级别只认 debug、info、warn、error（warning 当 warn，别的当 info），没写动作和消息时用占位。
fn plugin_log(plugin_id: &str, repo_id: Option<&str>, payload: &Value) -> SystemLogWriteRequest {
    let text = |key: &str| {
        payload
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    };
    let level = match text("level").map(str::to_ascii_lowercase).as_deref() {
        Some(level @ ("debug" | "info" | "warn" | "error")) => level.to_string(),
        Some("warning") => "warn".to_string(),
        _ => "info".to_string(),
    };
    SystemLogWriteRequest {
        level,
        category: "plugin.backend".to_string(),
        action: text("action").unwrap_or("pluginLog").to_string(),
        message: text("message").unwrap_or("插件日志").to_string(),
        context: payload.get("context").filter(|value| !value.is_null()).cloned(),
        repo_id: repo_id.map(str::to_string),
        plugin_id: Some(plugin_id.to_string()),
        source_kind: Some("backend-plugin".to_string()),
        source_label: Some(plugin_id.to_string()),
        location: Some(SystemLogLocationInput {
            module_path: Some(plugin_id.to_string()),
            file: None,
            line: None,
        }),
    }
}

/// 这个插件的这种事件是不是第一次被丢弃。登记表的锁坏了时按第一次算，宁可多记一条。
fn first_drop(plugin_id: &str, kind: &str) -> bool {
    let registry = DROPPED_KINDS.get_or_init(|| Mutex::new(BTreeSet::new()));
    match registry.lock() {
        Ok(mut seen) => seen.insert((plugin_id.to_string(), kind.to_string())),
        Err(_) => true,
    }
}

/// 丢弃事件时记的原因，来源是宿主。
fn dropped_event(plugin_id: &str, kind: &str) -> SystemLogWriteRequest {
    SystemLogWriteRequest {
        level: "warn".to_string(),
        category: "plugin.runtime".to_string(),
        action: "pluginEventDropped".to_string(),
        message: "插件调用交回了宿主不处理的事件，已丢弃；同一插件的同一种事件只记这一次。".to_string(),
        context: Some(serde_json::json!({ "pluginId": plugin_id, "kind": kind })),
        repo_id: None,
        plugin_id: Some(plugin_id.to_string()),
        source_kind: Some("host".to_string()),
        source_label: None,
        location: Some(SystemLogLocationInput {
            module_path: Some(module_path!().to_string()),
            file: Some(file!().to_string()),
            line: Some(line!()),
        }),
    }
}

#[cfg(test)]
#[path = "plugin_events_tests.rs"]
mod tests;
