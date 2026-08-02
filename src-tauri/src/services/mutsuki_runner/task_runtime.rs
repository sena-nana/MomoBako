//! MomoBako 自有的有界任务运行时。
//!
//! 交互和后台任务使用独立队列与固定 worker 数，领域操作仍复用 RepositoryTaskExecutor 的
//! 写锁、取消检查和错误语义，但不再经过 CoreRuntime、CoreActor 或无界阻塞线程池。

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use mutsuki_plugin_api::{PluginResult, PluginTaskGateway};
use mutsuki_runtime_contracts::{
    CancelPolicy, RuntimeError, Task, TaskBatch, TaskHandle, TaskOutcome,
};
use serde::Serialize;
use serde_json::Value;
use tokio::sync::{mpsc, oneshot, Mutex as AsyncMutex};

use super::operations::RepositoryTaskExecutor;
use super::protocols::{
    PROTOCOL_PLAYBACK_PREPARE, PROTOCOL_REPOSITORY_ACTION_RUN, PROTOCOL_THUMBNAIL_REQUEST,
};
use crate::services::repository::CancellationCheck;
use crate::services::runtime::RepositoryRuntime;

const INTERACTIVE_QUEUE_LIMIT: usize = 32;
const BACKGROUND_QUEUE_LIMIT: usize = 16;
const INTERACTIVE_WORKERS: usize = 2;
const BACKGROUND_WORKERS: usize = 2;

type TaskResult = Result<(Value, Vec<Value>), String>;

struct TaskRequest {
    task_id: String,
    protocol_id: String,
    payload: Value,
    cancellation: Arc<MomoCancellation>,
    result: oneshot::Sender<TaskResult>,
}

struct RuntimeState {
    runtime: RepositoryRuntime,
    interactive_tx: mpsc::Sender<TaskRequest>,
    background_tx: mpsc::Sender<TaskRequest>,
    next_task_id: AtomicU64,
    cancellations: Mutex<BTreeMap<String, Arc<MomoCancellation>>>,
    outcomes: Mutex<BTreeMap<String, TaskOutcome>>,
}

/// MomoBako 的双 lane 有界任务运行时。
#[derive(Clone)]
pub struct MomoTaskRuntime {
    state: Arc<RuntimeState>,
}

/// 领域操作依赖的轻量取消 token，不暴露上游运行时类型。
pub(crate) struct MomoCancellation {
    cancelled: AtomicBool,
}

impl MomoCancellation {
    fn new() -> Self {
        Self {
            cancelled: AtomicBool::new(false),
        }
    }

    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
}

impl CancellationCheck for MomoCancellation {
    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

impl MomoTaskRuntime {
    /// 创建固定 worker 的交互/后台双 lane。
    pub fn new(runtime: RepositoryRuntime) -> Self {
        let (interactive_tx, interactive_rx) = mpsc::channel(INTERACTIVE_QUEUE_LIMIT);
        let (background_tx, background_rx) = mpsc::channel(BACKGROUND_QUEUE_LIMIT);
        let state = Arc::new(RuntimeState {
            runtime,
            interactive_tx,
            background_tx,
            next_task_id: AtomicU64::new(1),
            cancellations: Mutex::new(BTreeMap::new()),
            outcomes: Mutex::new(BTreeMap::new()),
        });
        let task_runtime = Self { state };
        spawn_lane_workers(
            task_runtime.clone(),
            interactive_rx,
            INTERACTIVE_WORKERS,
            "interactive",
        );
        spawn_lane_workers(
            task_runtime.clone(),
            background_rx,
            BACKGROUND_WORKERS,
            "background",
        );
        task_runtime
    }

    /// 将序列化后的 Tauri 请求提交到正确 lane，并保留旧的输出/进度契约。
    pub async fn execute<Request>(&self, protocol_id: &'static str, request: Request) -> TaskResult
    where
        Request: Serialize + Send + 'static,
    {
        let payload = serde_json::to_value(request).map_err(|error| error.to_string())?;
        let task_id = format!(
            "momobako.task.{}",
            self.state.next_task_id.fetch_add(1, Ordering::Relaxed)
        );
        self.execute_value(task_id, protocol_id.to_string(), payload)
            .await
    }

    /// 取消一个已提交或正在等待资源库写锁的任务。
    pub fn cancel(&self, task_id: &str) -> bool {
        let cancellation = self
            .state
            .cancellations
            .lock()
            .ok()
            .and_then(|tasks| tasks.get(task_id).cloned());
        if let Some(cancellation) = cancellation {
            cancellation.cancel();
            true
        } else {
            false
        }
    }

    async fn execute_value(
        &self,
        task_id: String,
        protocol_id: String,
        payload: Value,
    ) -> TaskResult {
        let cancellation = Arc::new(MomoCancellation::new());
        self.state
            .cancellations
            .lock()
            .map_err(|_| "Momo task cancellation state poisoned".to_string())?
            .insert(task_id.clone(), cancellation.clone());
        let (result_tx, result_rx) = oneshot::channel();
        let request = TaskRequest {
            task_id: task_id.clone(),
            protocol_id: protocol_id.clone(),
            payload,
            cancellation,
            result: result_tx,
        };
        let sender = if is_interactive(&protocol_id) {
            &self.state.interactive_tx
        } else {
            &self.state.background_tx
        };
        if let Err(error) = sender.send(request).await {
            self.remove_task(&task_id);
            return Err(format!("Momo task lane closed: {error}"));
        }
        let result = result_rx
            .await
            .map_err(|_| "Momo task worker dropped its result".to_string())?;
        self.remove_task(&task_id);
        result
    }

    fn run_blocking(&self, request: TaskRequest) {
        let TaskRequest {
            task_id,
            protocol_id,
            payload,
            cancellation,
            result: result_sender,
        } = request;
        let task = Task::new(task_id, protocol_id.clone(), payload);
        let mut events = Vec::new();
        let executor = RepositoryTaskExecutor::new(&self.state.runtime, cancellation.as_ref());
        let result = executor
            .execute(&task, &mut events)
            .map_err(|error| format!("{} [{}] {:?}", protocol_id, error.route, error.evidence));
        let result = result.map(|result| {
            let output = result.output.unwrap_or(Value::Null);
            let events = result
                .events
                .into_iter()
                .filter(|event| event.kind == "momobako.task.progress")
                .filter_map(|event| event.payload.get("progress").cloned())
                .collect();
            (output, events)
        });
        let _ = result_sender.send(result);
    }

    fn remove_task(&self, task_id: &str) {
        if let Ok(mut tasks) = self.state.cancellations.lock() {
            tasks.remove(task_id);
        }
    }

    fn execute_contract_task(&self, task: Task) -> PluginResult<TaskHandle> {
        let task_id = task.task_id.clone();
        let protocol_id = task.protocol_id.clone();
        let payload = task.payload.to_value();
        let result = tauri::async_runtime::block_on(self.execute_value(
            task_id.clone(),
            protocol_id.clone(),
            payload,
        ));
        let outcome = match result {
            Ok((output, _)) => TaskOutcome::Completed {
                task_id: task_id.clone(),
                output: Some(output),
                output_ref: None,
            },
            Err(error) => TaskOutcome::Failed {
                task_id: task_id.clone(),
                error: RuntimeError::new("momobako.task_failed", "momobako.task_runtime", error),
            },
        };
        self.state
            .outcomes
            .lock()
            .map_err(|_| {
                mutsuki_plugin_api::plugin_error("plugin.task.outcome", "outcome state poisoned")
            })?
            .insert(task_id.clone(), outcome);
        Ok(TaskHandle {
            task_id,
            protocol_id,
            target_binding_id: task.target_binding_id,
            cancel_policy: CancelPolicy::Cascade,
            trace_id: task.trace_id,
            correlation_id: task.correlation_id,
        })
    }

    fn lookup_task_outcome(&self, handle: &TaskHandle) -> PluginResult<Option<TaskOutcome>> {
        self.state
            .outcomes
            .lock()
            .map_err(|_| {
                mutsuki_plugin_api::plugin_error("plugin.task.outcome", "outcome state poisoned")
            })
            .map(|outcomes| outcomes.get(&handle.task_id).cloned())
    }
}

impl PluginTaskGateway for MomoTaskRuntime {
    fn submit_batch(&self, batch: TaskBatch) -> PluginResult<Vec<TaskHandle>> {
        batch
            .tasks
            .into_iter()
            .map(|task| self.execute_contract_task(task))
            .collect()
    }

    fn cancel_task(&self, handle: &TaskHandle) -> PluginResult<()> {
        if self.cancel(&handle.task_id) {
            Ok(())
        } else {
            Err(mutsuki_plugin_api::plugin_error(
                "plugin.task.cancel",
                format!("task is no longer active: {}", handle.task_id),
            ))
        }
    }

    fn task_outcome(&self, handle: &TaskHandle) -> PluginResult<Option<TaskOutcome>> {
        self.lookup_task_outcome(handle)
    }
}

fn spawn_lane_workers(
    runtime: MomoTaskRuntime,
    receiver: mpsc::Receiver<TaskRequest>,
    count: usize,
    lane: &'static str,
) {
    let receiver = Arc::new(AsyncMutex::new(receiver));
    for worker_id in 0..count {
        let runtime = runtime.clone();
        let receiver = receiver.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                let request = receiver.lock().await.recv().await;
                let Some(request) = request else { break };
                let result_runtime = runtime.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    result_runtime.run_blocking(request);
                })
                .await
                .unwrap_or_else(|error| {
                    crate::app_log!(
                        "error",
                        "momo.taskRuntime",
                        "workerStopped",
                        "Momo 任务 worker 异常停止。",
                        serde_json::json!({ "lane": lane, "workerId": worker_id, "error": error.to_string() })
                    );
                });
            }
        });
    }
}

fn is_interactive(protocol_id: &str) -> bool {
    matches!(
        protocol_id,
        PROTOCOL_REPOSITORY_ACTION_RUN | PROTOCOL_PLAYBACK_PREPARE | PROTOCOL_THUMBNAIL_REQUEST
    )
}
