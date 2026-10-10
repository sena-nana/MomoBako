//! 插件调用交回的事件进系统日志。从插件交回的调用结果出发：照插件 SDK（`External/Plugins/_sdk/mutsuki-rust`）
//! 的样子排布，经 ABI 线上编码走一遍（宿主收到的就是解开以后的），再交给宿主收下；断言日志中心里的记录和交回给
//! 调用方的结果。

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use mutsuki_runtime_contracts::{
    CompletionBatch, DomainEvent, EntryCompletion, RunnerResult, RunnerStatus, RuntimeError,
};
use mutsuki_runtime_wire::{
    decode_binary_response, encode_binary_response, Opcode, RunBatchRequest, DEFAULT_WIRE_LIMITS,
};
use serde_json::{json, Value};

use super::{settle_call, PLUGIN_LOG_EVENT};
use crate::services::logging::AppLogger;
use crate::services::repository::{SystemLogQuery, SystemLogRecord};

const NETEASE: &str = "momobako.netease.source";

static NEXT_ROOT: AtomicU64 = AtomicU64::new(1);

/// 测试独占的日志中心（落盘目录在临时目录里），测试结束时删掉。
struct LogCenter {
    root: PathBuf,
    logger: AppLogger,
}

impl LogCenter {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "momobako-plugin-events-{name}-{}-{}",
            std::process::id(),
            NEXT_ROOT.fetch_add(1, Ordering::Relaxed)
        ));
        let logger = AppLogger::new(root.clone()).expect("日志中心");
        Self { root, logger }
    }

    /// 宿主收下一次调用的结果，日志写进这个日志中心。
    fn settle(&self, plugin_id: &str, repo_id: Option<&str>, entry: EntryCompletion) -> Result<Value, String> {
        settle_call(plugin_id, repo_id, over_the_wire(entry), &mut |request| {
            self.logger.write(request).expect("写进日志中心");
        })
    }

    /// 日志中心里的记录，按写下的先后。
    fn records(&self) -> Vec<SystemLogRecord> {
        let mut records = self
            .logger
            .list(Some(SystemLogQuery { limit: Some(50), ..SystemLogQuery::default() }))
            .expect("读日志")
            .records;
        records.reverse();
        records
    }
}

impl Drop for LogCenter {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// 插件 SDK `write_host_log_silently` 写下的事件。
fn sdk_log(plugin_id: &str, level: &str, action: &str, message: &str, context: Value) -> DomainEvent {
    DomainEvent {
        event_id: format!("{plugin_id}:1"),
        kind: PLUGIN_LOG_EVENT.into(),
        payload: json!({
            "level": level, "action": action, "message": message, "pluginId": plugin_id, "context": context
        }),
    }
}

/// 插件 SDK 里 handler 写过日志、交回 `Err` 时的那一条结果：错误是插件写的原因，日志在状态为失败的结果里。
fn failed_with(plugin_id: &str, events: Vec<DomainEvent>, reason: &str) -> EntryCompletion {
    let mut result = RunnerResult::completed("call.task");
    result.status = RunnerStatus::Failed;
    result.events = events;
    EntryCompletion {
        entry_id: "call.entry".into(),
        task_id: "call.task".into(),
        result: Some(result),
        error: Some(RuntimeError::new("momobako.plugin_call_failed", plugin_id, reason)),
    }
}

/// 插件 SDK 里 handler 交回 `Ok` 时的那一条结果。
fn succeeded_with(events: Vec<DomainEvent>, output: Value) -> EntryCompletion {
    let mut result = RunnerResult::completed("call.task");
    result.output = Some(output);
    result.events = events;
    EntryCompletion { entry_id: "call.entry".into(), task_id: "call.task".into(), result: Some(result), error: None }
}

/// 经 ABI 线上编码走一遍：插件交回的批次编码成响应帧，宿主再解开。
fn over_the_wire(entry: EntryCompletion) -> EntryCompletion {
    let batch = CompletionBatch {
        batch_id: "call.batch".into(),
        tick_id: "call.tick".into(),
        results: vec![entry],
        metadata: Vec::new(),
    };
    let bytes = encode_binary_response(7, Opcode::RunnerRunBatch, Ok(&batch), DEFAULT_WIRE_LIMITS).expect("编码响应帧");
    decode_binary_response::<RunBatchRequest>(&bytes, 7, DEFAULT_WIRE_LIMITS)
        .expect("解开响应帧")
        .results
        .remove(0)
}

/// 网易云下载时落盘失败：插件先写了带目标路径的日志再交回错误。错误照旧交回调用方，日志写进系统日志，来源是这个
/// 插件，记在这次调用的资源库名下。
#[test]
fn logs_written_before_a_failed_call_reach_the_system_log() {
    let center = LogCenter::new("failed");
    let log = sdk_log(NETEASE, "error", "downloadMoveFailed", "网易云下载落盘失败。", json!({ "target": "D:/Music/晴天.mp3" }));
    let result = center.settle(NETEASE, Some("netease-cloud-music:42"), failed_with(NETEASE, vec![log], "文件被占用"));

    assert_eq!(result.expect_err("调用失败"), "文件被占用", "插件写的原因原样交回调用方，界面上直接看得懂");
    let records = center.records();
    assert_eq!(records.len(), 1);
    let record = &records[0];
    assert_eq!(
        (record.level.as_str(), record.category.as_str(), record.action.as_str(), record.message.as_str()),
        ("error", "plugin.backend", "downloadMoveFailed", "网易云下载落盘失败。")
    );
    assert_eq!(record.source.kind, "backend-plugin");
    assert_eq!(record.source.plugin_id.as_deref(), Some(NETEASE));
    assert_eq!(record.source.label.as_deref(), Some(NETEASE));
    assert_eq!(record.source.repo_id.as_deref(), Some("netease-cloud-music:42"));
    assert_eq!(record.location.module_path.as_deref(), Some(NETEASE));
    assert_eq!(record.context["target"], "D:/Music/晴天.mp3");
}

/// 成功的调用：输出照旧交回，日志照样写进系统日志。
#[test]
fn a_successful_call_keeps_its_output_and_its_logs() {
    let center = LogCenter::new("succeeded");
    let log = sdk_log(NETEASE, "warn", "tempCleanupFailed", "网易云临时文件清理失败。", json!({ "path": "a.mp3" }));
    let output = center.settle(NETEASE, None, succeeded_with(vec![log], json!({ "songId": 1 }))).expect("调用成功");
    assert_eq!(output, json!({ "songId": 1 }));
    let records = center.records();
    assert_eq!(records.iter().map(|record| record.action.as_str()).collect::<Vec<_>>(), ["tempCleanupFailed"]);
    assert_eq!((records[0].level.as_str(), records[0].source.repo_id.as_deref()), ("warn", None));
}

/// 来源记成宿主加载的这个插件，不信载荷里自报的插件标识；级别只认四种，warning 当 warn、别的当 info；没写动作和
/// 消息时用占位，空的上下文不写。
#[test]
fn the_host_decides_the_source_and_normalizes_the_record() {
    let center = LogCenter::new("normalize");
    let spoofed = sdk_log("momobako.local-filesystem", "WARNING", "spoof", "自报别的插件。", json!({ "k": 1 }));
    let fatal = sdk_log(NETEASE, "fatal", "", "  ", Value::Null);
    center
        .settle(NETEASE, None, succeeded_with(vec![spoofed, fatal], Value::Null))
        .expect("调用成功");
    let records = center.records();
    assert_eq!(records.len(), 2);
    assert_eq!((records[0].level.as_str(), records[0].source.plugin_id.as_deref()), ("warn", Some(NETEASE)));
    assert_eq!(
        (records[1].level.as_str(), records[1].action.as_str(), records[1].message.as_str()),
        ("info", "pluginLog", "插件日志")
    );
    assert_eq!(records[1].context, json!({}), "没有上下文时日志中心按空对象记");
}

/// 别的种类的事件宿主不处理：丢弃，同一个插件的同一种事件只记一次原因（来源是宿主）；日志事件照常写。
#[test]
fn other_events_are_dropped_and_reported_once_per_kind() {
    let center = LogCenter::new("dropped");
    let plugin_id = "momobako.test.events";
    let event = |kind: &str| DomainEvent { event_id: format!("{kind}:1"), kind: kind.into(), payload: json!({}) };
    let first = vec![event("momobako.task.progress"), sdk_log(plugin_id, "info", "kept", "日志照写。", json!({}))];
    center.settle(plugin_id, None, succeeded_with(first, Value::Null)).expect("第一次");
    let second = vec![event("momobako.task.progress"), event("custom.kind")];
    center.settle(plugin_id, None, succeeded_with(second, Value::Null)).expect("第二次");

    let records = center.records();
    let summary: Vec<(String, String, Value)> = records
        .iter()
        .map(|record| (record.source.kind.clone(), record.action.clone(), record.context["kind"].clone()))
        .collect();
    assert_eq!(
        summary,
        [
            ("host".to_string(), "pluginEventDropped".to_string(), json!("momobako.task.progress")),
            ("backend-plugin".into(), "kept".into(), Value::Null),
            ("host".into(), "pluginEventDropped".into(), json!("custom.kind")),
        ]
    );
    assert!(records.iter().filter(|record| record.source.kind == "host").all(|record| record.source.plugin_id.as_deref() == Some(plugin_id)));
}

/// 没有事件的调用不写日志；没写日志就失败的调用照旧只交回错误。
#[test]
fn calls_without_events_write_nothing() {
    let center = LogCenter::new("quiet");
    let quiet_failure = EntryCompletion {
        entry_id: "call.entry".into(),
        task_id: "call.task".into(),
        result: None,
        error: Some(RuntimeError::new("momobako.plugin_call_failed", NETEASE, "网易云登录已失效，请重新登录")),
    };
    assert_eq!(center.settle(NETEASE, None, quiet_failure).expect_err("失败"), "网易云登录已失效，请重新登录");
    center.settle(NETEASE, None, succeeded_with(Vec::new(), Value::Null)).expect("成功");
    assert!(center.records().is_empty());
}

/// 不是插件自己交回的错误（运行时、宿主）带上错误码和现场，和插件写的原因区分开。
#[test]
fn runtime_failures_name_their_code() {
    let center = LogCenter::new("runtime-error");
    let mut error = RuntimeError::new("abi.runner.claim_conflict", NETEASE, "runner.run_batch.call");
    let conflict = EntryCompletion { entry_id: "call.entry".into(), task_id: "call.task".into(), result: None, error: Some(error.clone()) };
    assert_eq!(
        center.settle(NETEASE, None, conflict).expect_err("失败"),
        "插件调用失败（abi.runner.claim_conflict）：runner.run_batch.call"
    );
    error.evidence.insert("lease".into(), mutsuki_runtime_contracts::ScalarValue::String("call.lease".into()));
    let with_evidence = EntryCompletion { entry_id: "call.entry".into(), task_id: "call.task".into(), result: None, error: Some(error) };
    let text = center.settle(NETEASE, None, with_evidence).expect_err("失败");
    assert!(text.starts_with("插件调用失败（abi.runner.claim_conflict）：runner.run_batch.call ") && text.contains("call.lease"), "{text}");
}
