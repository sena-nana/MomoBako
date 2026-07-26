//! MomoBako 内建 Mutsuki 长任务 runner。

use mutsuki_runtime_contracts::{
    CompletionBatch, DomainEvent, ExecutionClass, RunnerBatchCapability, RunnerDescriptor,
    RunnerPurity, RunnerResourceCapability, RunnerResult, RunnerSideEffect, RunnerStatus,
    WorkBatch,
};
use mutsuki_runtime_core::{Runner, RunnerContext, RuntimeResult};
use mutsuki_runtime_sdk::{map_work_batch_entries, RunnerDescriptorBuilder};

use super::{operations::RepositoryTaskExecutor, protocols};
use crate::services::runtime::RepositoryRuntime;

const PLUGIN_ID: &str = "momobako.builtin";
const RUNNER_ID: &str = "momobako.builtin.long-task";

/// 承载 MomoBako 产品领域长任务，Core 只负责编排和生命周期。
pub struct MomoLongTaskRunner {
    runtime: RepositoryRuntime,
    descriptor: RunnerDescriptor,
}

impl MomoLongTaskRunner {
    /// 创建指定插件 generation 的内建 runner。
    pub fn new(runtime: RepositoryRuntime, plugin_generation: u64) -> Self {
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
        Self {
            runtime,
            descriptor: descriptor.build(),
        }
    }
}

/// 构造供 MutsukiTauriHost 注册的 boxed runner。
pub fn build_momo_long_task_runner(
    runtime: RepositoryRuntime,
    plugin_generation: u64,
) -> Box<dyn Runner> {
    Box::new(MomoLongTaskRunner::new(runtime, plugin_generation))
}

impl Runner for MomoLongTaskRunner {
    fn descriptor(&self) -> &RunnerDescriptor {
        &self.descriptor
    }

    /// 按协议调用现有 RepositoryService，保持所有写操作串行。
    fn run_batch(
        &mut self,
        ctx: RunnerContext,
        batch: WorkBatch,
    ) -> RuntimeResult<CompletionBatch> {
        let executor = RepositoryTaskExecutor::new(&self.runtime);
        map_work_batch_entries(&batch, |task| {
            if ctx.cancel_requested {
                crate::app_log!(
                    "info",
                    "mutsuki.longTask",
                    "cancelledBeforeStart",
                    "长任务在进入 RepositoryService 前已取消。",
                    serde_json::json!({
                        "taskId": task.task_id,
                        "protocolId": task.protocol_id,
                        "invocationId": ctx.invocation_id,
                    })
                );
                return Ok(cancelled_result(task.task_id.as_str()));
            }

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
            let mut events = Vec::<DomainEvent>::new();
            let result = executor.execute(task, &mut events);
            match &result {
                Ok(result) => crate::app_log!(
                    "info",
                    "mutsuki.longTask",
                    "success",
                    "MomoBako 内建长任务执行完成。",
                    serde_json::json!({
                        "taskId": task.task_id,
                        "protocolId": task.protocol_id,
                        "progressEventCount": result.events.len(),
                    })
                ),
                Err(error) => crate::app_log!(
                    "error",
                    "mutsuki.longTask",
                    "failed",
                    "MomoBako 内建长任务执行失败。",
                    serde_json::json!({
                        "taskId": task.task_id,
                        "protocolId": task.protocol_id,
                        "code": error.code.as_str(),
                        "route": error.route.as_str(),
                        "evidence": &error.evidence,
                    })
                ),
            }
            result
        })
    }
}

fn cancelled_result(task_id: &str) -> RunnerResult {
    let mut result = RunnerResult::completed(task_id);
    result.status = RunnerStatus::Cancelled;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancelled_result_is_terminal_without_output() {
        let result = cancelled_result("cancelled-task");
        assert_eq!(result.task_id, "cancelled-task");
        assert_eq!(result.status, RunnerStatus::Cancelled);
        assert!(result.output.is_none());
    }
}
