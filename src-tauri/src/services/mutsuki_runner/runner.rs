//! MomoBako 内建 Mutsuki 长任务 runner。

use mutsuki_runtime_contracts::{
    DomainEvent, ExecutionClass, RunnerBatchCapability, RunnerDescriptor, RunnerPurity,
    RunnerResourceCapability, RunnerSideEffect,
};
use mutsuki_runtime_core::{Runner, RuntimeFailure};
use mutsuki_runtime_host::NativeRunner;
use mutsuki_runtime_sdk::RunnerDescriptorBuilder;

use super::{operations::RepositoryTaskExecutor, protocols};
use crate::services::runtime::RepositoryRuntime;

const PLUGIN_ID: &str = "momobako.builtin";
const RUNNER_ID: &str = "momobako.builtin.long-task";

/// 构造供 MutsukiTauriHost 注册的 boxed runner。
pub fn build_momo_long_task_runner(
    runtime: RepositoryRuntime,
    plugin_generation: u64,
) -> Box<dyn Runner> {
    let descriptor = build_descriptor(plugin_generation);
    Box::new(NativeRunner::new_borrowed_cancellable(
        descriptor,
        move |ctx, task, cancellation| {
            crate::app_log!(
                "info",
                "mutsuki.longTask",
                "start",
                "开始执行 MomoBako 内建长任务。",
                serde_json::json!({
                    "taskId": task.task_id,
                    "protocolId": task.protocol_id,
                    "invocationId": ctx.invocation_id,
                })
            );
            let executor = RepositoryTaskExecutor::new(&runtime, cancellation);
            let mut events = Vec::<DomainEvent>::new();
            let result = executor.execute(task, &mut events);
            match &result {
                Ok(result) => crate::app_log!(
                    "info",
                    "mutsuki.longTask",
                    "success",
                    "MomoBako 内建长任务 worker 已完成。",
                    serde_json::json!({
                        "taskId": task.task_id,
                        "protocolId": task.protocol_id,
                        "progressEventCount": result.events.len(),
                        "cancelRequested": cancellation.is_cancelled(),
                    })
                ),
                Err(error) if cancellation.is_cancelled() => crate::app_log!(
                    "info",
                    "mutsuki.longTask",
                    "workerStoppedAfterCancellation",
                    "取消请求后，MomoBako 内建长任务 worker 已停止。",
                    serde_json::json!({
                        "taskId": task.task_id,
                        "protocolId": task.protocol_id,
                        "code": error.code.as_str(),
                        "route": error.route.as_str(),
                    })
                ),
                Err(error) => crate::app_log!(
                    "error",
                    "mutsuki.longTask",
                    "failed",
                    "MomoBako 内建长任务 worker 执行失败。",
                    serde_json::json!({
                        "taskId": task.task_id,
                        "protocolId": task.protocol_id,
                        "code": error.code.as_str(),
                        "route": error.route.as_str(),
                        "evidence": &error.evidence,
                        "cancelRequested": cancellation.is_cancelled(),
                    })
                ),
            }
            result.map_err(RuntimeFailure::new)
        },
    ))
}

fn build_descriptor(plugin_generation: u64) -> RunnerDescriptor {
    let mut descriptor = RunnerDescriptorBuilder::new(RUNNER_ID, PLUGIN_ID)
        .plugin_generation(plugin_generation)
        .purity(RunnerPurity::Effectful)
        .execution_class(ExecutionClass::Blocking)
        .input_schema(serde_json::json!({ "type": "object" }))
        .output_schema(serde_json::json!({ "type": ["object", "array", "null"] }))
        .batch_capability(RunnerBatchCapability {
            side_effect: RunnerSideEffect::External,
            ..RunnerBatchCapability::default()
        })
        .resource_capability(RunnerResourceCapability {
            requires_resource_plan: false,
            ..RunnerResourceCapability::default()
        });
    for protocol_id in protocols::all() {
        descriptor = descriptor.accepted_protocol(protocol_id);
    }
    descriptor.build()
}
