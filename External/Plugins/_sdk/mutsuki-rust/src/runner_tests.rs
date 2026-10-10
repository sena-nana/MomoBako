//! Momo 插件 runner 交回的调用结果：handler 经 [`write_host_log_silently`] 写的日志随结果交给宿主，handler 失败时
//! 也不丢；一次调用的日志不会混进下一次。批次照宿主 `mutsuki_host::call_session` 的样子排布。

use mutsuki_runtime_contracts::{
    BatchEntry, BatchPayload, DispatchLane, OrderingRequirement, RowPayload, RunnerStatus, Task,
    TaskLease, WorkBatch, WorkResourcePlan,
};
use mutsuki_runtime_core::{Runner, RunnerContext};
use mutsuki_runtime_sdk::RunnerDescriptorBuilder;
use serde_json::json;

use super::{
    EntryCompletion, MomoPluginRunner, PluginCallEnvelope, PluginHandler, PluginRuntimeContext,
    PLUGIN_CALL_FAILED, PLUGIN_LOG_EVENT,
};

const PLUGIN_ID: &str = "momobako.test";

fn runner(handler: PluginHandler) -> MomoPluginRunner {
    MomoPluginRunner {
        descriptor: RunnerDescriptorBuilder::new("momobako.test.runner", PLUGIN_ID).build(),
        runtime: PluginRuntimeContext {
            plugin_id: PLUGIN_ID.into(),
            ..PluginRuntimeContext::default()
        },
        handler,
    }
}

/// 调用一次 `method`，交回这一条的结果。
fn call(runner: &mut MomoPluginRunner, method: &str) -> EntryCompletion {
    let task_id = "call.task".to_string();
    let task = Task::new(task_id.clone(), format!("momobako.{method}"), json!({}));
    let lease = TaskLease {
        lease_id: "call.lease".into(),
        task_id: task_id.clone(),
        attempt_generation: 1,
        runner_id: "momobako.test.runner".into(),
        executor_id: "test".into(),
        registry_generation: 1,
        acquired_at_step: 0,
        expires_at_step: None,
    };
    let entry = BatchEntry {
        entry_id: "call.entry".into(),
        task_id,
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
        batch_id: "call.batch".into(),
        tick_id: "call.tick".into(),
        batch_key: format!("plugin:{PLUGIN_ID}"),
        entries: vec![entry],
        payload: BatchPayload::Row(RowPayload {
            rows: vec![serde_json::to_value(&task).expect("任务可以序列化")],
        }),
        resource_plan: WorkResourcePlan::empty(),
        task_leases: vec![lease],
    };
    let context = RunnerContext::new(1, 0, "test", vec!["call.lease".to_string()], "call")
        .with_batch("call.batch", 1);
    runner
        .run_batch(context, batch)
        .expect("批次跑完")
        .results
        .remove(0)
}

fn logs_then_fails(request: PluginCallEnvelope) -> Result<serde_json::Value, String> {
    super::write_host_log_silently(
        &request.runtime,
        "error",
        "downloadMoveFailed",
        "落盘失败。",
        json!({ "target": "D:/Music/a.mp3" }),
    );
    Err("文件被占用".into())
}

fn logs_then_succeeds(request: PluginCallEnvelope) -> Result<serde_json::Value, String> {
    super::write_host_log_silently(&request.runtime, "info", "cached", "已缓存。", json!({}));
    Ok(json!({ "ok": true }))
}

fn fails_quietly(_request: PluginCallEnvelope) -> Result<serde_json::Value, String> {
    Err("网络超时".into())
}

/// handler 写了日志再失败：错误照旧是插件写的原因，日志放在状态为失败的结果里一起交回。
#[test]
fn a_failed_call_still_hands_back_the_logs_written_before_it_failed() {
    let completion = call(&mut runner(logs_then_fails), "media.download");
    let error = completion.error.expect("调用失败");
    assert_eq!((error.code.as_str(), error.route.as_str()), (PLUGIN_CALL_FAILED, "文件被占用"));
    let result = completion.result.expect("日志随结果交回");
    assert_eq!(result.status, RunnerStatus::Failed);
    assert_eq!(result.events.len(), 1);
    let event = &result.events[0];
    assert_eq!(event.kind, PLUGIN_LOG_EVENT);
    assert_eq!(
        (event.payload["level"].clone(), event.payload["action"].clone(), event.payload["pluginId"].clone()),
        (json!("error"), json!("downloadMoveFailed"), json!(PLUGIN_ID))
    );
    assert_eq!(event.payload["context"]["target"], "D:/Music/a.mp3");
}

/// 成功的调用：输出和日志都在结果里。
#[test]
fn a_successful_call_hands_back_its_output_and_logs() {
    let completion = call(&mut runner(logs_then_succeeds), "media.cache");
    assert!(completion.error.is_none());
    let result = completion.result.expect("调用成功");
    assert_eq!(result.status, RunnerStatus::Completed);
    assert_eq!(result.output, Some(json!({ "ok": true })));
    assert_eq!(result.events.iter().map(|event| event.kind.as_str()).collect::<Vec<_>>(), [PLUGIN_LOG_EVENT]);
}

/// 没写日志的失败照旧只有错误；上一次调用的日志不会混进这一次。
#[test]
fn a_quiet_failure_carries_no_result_and_no_earlier_logs() {
    let mut runner = runner(logs_then_fails);
    let first = call(&mut runner, "media.download");
    assert!(first.result.is_some());
    runner.handler = fails_quietly;
    let second = call(&mut runner, "media.download");
    assert!(second.result.is_none(), "这一次没有写日志：{second:?}");
    assert_eq!(second.error.map(|error| error.route), Some("网络超时".to_string()));
}
