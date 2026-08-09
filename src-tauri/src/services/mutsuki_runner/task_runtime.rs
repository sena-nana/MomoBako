//! MomoBako 自有的有界任务运行时。
//!
//! 交互和后台任务使用独立队列与固定 worker 数，领域操作仍复用 RepositoryTaskExecutor 的
//! 写锁、取消检查和错误语义，但不再经过 CoreRuntime、CoreActor 或无界阻塞线程池。

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use mutsuki_plugin_api::{plugin_error, PluginHostError, PluginResult, PluginTaskGateway};
use mutsuki_runtime_contracts::{
    CancelPolicy, RuntimeError, ScalarValue, Task, TaskBatch, TaskHandle, TaskOutcome,
    ERR_TASK_EXPIRED, ERR_TASK_NOT_FOUND,
};
use serde::Serialize;
use serde_json::Value;
use tokio::sync::mpsc::error::TrySendError;
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
const OUTCOME_CAPACITY: usize = 256;
const OUTCOME_RETENTION: Duration = Duration::from_secs(15 * 60);

type TaskResult = Result<(Value, Vec<Value>), String>;

enum TaskCompletion {
    Awaited(oneshot::Sender<TaskResult>),
    Gateway,
}

struct TaskRequest {
    task_id: String,
    protocol_id: String,
    payload: Value,
    cancellation: Arc<MomoCancellation>,
    completion: TaskCompletion,
}

struct PreparedGatewayTask {
    handle: TaskHandle,
    request: TaskRequest,
    interactive: bool,
}

struct StoredOutcome {
    task_id: String,
    outcome: TaskOutcome,
    completed_at: Instant,
}

enum OutcomeLookup {
    Retained(TaskOutcome),
    Expired,
    Missing,
}

/// 终态与过期标记分别按 FIFO 有界保留，并在 TTL 后惰性回收。
struct OutcomeStore {
    terminal: VecDeque<StoredOutcome>,
    expired: VecDeque<(String, Instant)>,
    capacity: usize,
    retention: Duration,
}

impl OutcomeStore {
    fn new(capacity: usize, retention: Duration) -> Self {
        Self {
            terminal: VecDeque::new(),
            expired: VecDeque::new(),
            capacity: capacity.max(1),
            retention,
        }
    }

    fn publish(&mut self, task_id: String, outcome: TaskOutcome) {
        self.publish_at(task_id, outcome, Instant::now());
    }

    fn publish_at(&mut self, task_id: String, outcome: TaskOutcome, completed_at: Instant) {
        self.prune_expired(completed_at);
        self.terminal.push_back(StoredOutcome {
            task_id,
            outcome,
            completed_at,
        });
        if self.terminal.len() > self.capacity {
            self.evict_oldest(completed_at);
        }
    }

    fn lookup(&mut self, task_id: &str) -> OutcomeLookup {
        self.lookup_at(task_id, Instant::now())
    }

    fn lookup_at(&mut self, task_id: &str, now: Instant) -> OutcomeLookup {
        self.prune_expired(now);
        if let Some(stored) = self
            .terminal
            .iter()
            .find(|stored| stored.task_id == task_id)
        {
            OutcomeLookup::Retained(stored.outcome.clone())
        } else if self.expired.iter().any(|(expired, _)| expired == task_id) {
            OutcomeLookup::Expired
        } else {
            OutcomeLookup::Missing
        }
    }

    fn contains(&self, task_id: &str) -> bool {
        self.terminal.iter().any(|stored| stored.task_id == task_id)
            || self.expired.iter().any(|(expired, _)| expired == task_id)
    }

    fn prune_expired(&mut self, now: Instant) {
        while self.terminal.front().is_some_and(|stored| {
            now.saturating_duration_since(stored.completed_at) >= self.retention
        }) {
            self.evict_oldest(now);
        }
        while self.expired.front().is_some_and(|(_, expired_at)| {
            now.saturating_duration_since(*expired_at) >= self.retention
        }) {
            self.expired.pop_front();
        }
    }

    fn evict_oldest(&mut self, expired_at: Instant) {
        if let Some(stored) = self.terminal.pop_front() {
            self.expired.push_back((stored.task_id, expired_at));
            if self.expired.len() > self.capacity {
                self.expired.pop_front();
            }
        }
    }
}

struct RuntimeState {
    runtime: RepositoryRuntime,
    interactive_tx: mpsc::Sender<TaskRequest>,
    background_tx: mpsc::Sender<TaskRequest>,
    next_task_id: AtomicU64,
    cancellations: Mutex<BTreeMap<String, Arc<MomoCancellation>>>,
    outcomes: Mutex<OutcomeStore>,
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
        Self::with_outcome_policy(runtime, OUTCOME_CAPACITY, OUTCOME_RETENTION)
    }

    /// 创建使用指定终态留存策略的双 lane 运行时。
    fn with_outcome_policy(
        runtime: RepositoryRuntime,
        outcome_capacity: usize,
        outcome_retention: Duration,
    ) -> Self {
        let (interactive_tx, interactive_rx) = mpsc::channel(INTERACTIVE_QUEUE_LIMIT);
        let (background_tx, background_rx) = mpsc::channel(BACKGROUND_QUEUE_LIMIT);
        let state = Arc::new(RuntimeState {
            runtime,
            interactive_tx,
            background_tx,
            next_task_id: AtomicU64::new(1),
            cancellations: Mutex::new(BTreeMap::new()),
            outcomes: Mutex::new(OutcomeStore::new(outcome_capacity, outcome_retention)),
        });
        let task_runtime = Self { state };
        spawn_lane_workers(
            Arc::downgrade(&task_runtime.state),
            interactive_rx,
            INTERACTIVE_WORKERS,
            "interactive",
        );
        spawn_lane_workers(
            Arc::downgrade(&task_runtime.state),
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
            completion: TaskCompletion::Awaited(result_tx),
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
        match result_rx.await {
            Ok(result) => result,
            Err(_) => {
                self.remove_task(&task_id);
                Err("Momo task worker dropped its result".to_string())
            }
        }
    }

    fn run_blocking(&self, request: TaskRequest) {
        let TaskRequest {
            task_id,
            protocol_id,
            payload,
            cancellation,
            completion,
        } = request;
        let task = Task::new(task_id.clone(), protocol_id.clone(), payload);
        let (result, cancelled) = catch_unwind(AssertUnwindSafe(|| {
            let mut events = Vec::new();
            let executor = RepositoryTaskExecutor::new(&self.state.runtime, cancellation.as_ref());
            match executor.execute(&task, &mut events) {
                Ok(result) => {
                    let output = result.output.unwrap_or(Value::Null);
                    let events = result
                        .events
                        .into_iter()
                        .filter(|event| event.kind == "momobako.task.progress")
                        .filter_map(|event| event.payload.get("progress").cloned())
                        .collect();
                    (Ok((output, events)), false)
                }
                Err(error) => {
                    let cancelled = runtime_error_is_cancelled(&error);
                    (
                        Err(format!(
                            "{} [{}] {:?}",
                            protocol_id, error.route, error.evidence
                        )),
                        cancelled,
                    )
                }
            }
        }))
        .unwrap_or_else(|_| {
            crate::app_log!(
                "error",
                "momo.taskRuntime",
                "taskPanicked",
                "Momo 任务执行发生 panic。",
                serde_json::json!({ "taskId": task_id, "protocolId": protocol_id })
            );
            (
                Err(format!(
                    "{protocol_id} [momobako.task_runtime] task panicked"
                )),
                false,
            )
        });
        match completion {
            TaskCompletion::Awaited(result_sender) => {
                if result_sender.send(result).is_err() {
                    crate::app_log!(
                        "warn",
                        "momo.taskRuntime",
                        "resultReceiverDropped",
                        "Momo 任务结果接收端已关闭。",
                        serde_json::json!({ "taskId": task_id, "protocolId": protocol_id })
                    );
                }
            }
            TaskCompletion::Gateway => {
                self.publish_gateway_outcome(&task_id, result, cancelled);
            }
        }
        self.remove_task(&task_id);
    }

    fn remove_task(&self, task_id: &str) {
        if let Ok(mut tasks) = self.state.cancellations.lock() {
            tasks.remove(task_id);
        }
    }

    fn prepare_gateway_task(task: Task) -> PreparedGatewayTask {
        let handle = TaskHandle {
            task_id: task.task_id.clone(),
            protocol_id: task.protocol_id.clone(),
            target_binding_id: task.target_binding_id,
            cancel_policy: CancelPolicy::Cascade,
            trace_id: task.trace_id,
            correlation_id: task.correlation_id,
        };
        let cancellation = Arc::new(MomoCancellation::new());
        PreparedGatewayTask {
            interactive: is_interactive(&handle.protocol_id),
            request: TaskRequest {
                task_id: handle.task_id.clone(),
                protocol_id: handle.protocol_id.clone(),
                payload: task.payload.to_value(),
                cancellation,
                completion: TaskCompletion::Gateway,
            },
            handle,
        }
    }

    fn publish_gateway_outcome(&self, task_id: &str, result: TaskResult, cancelled: bool) {
        let outcome = match result {
            Ok((output, _)) => TaskOutcome::Completed {
                task_id: task_id.to_string(),
                output: Some(output),
                output_ref: None,
            },
            Err(error) if cancelled => TaskOutcome::Cancelled {
                task_id: task_id.to_string(),
                reason: Some(error),
            },
            Err(error) => TaskOutcome::Failed {
                task_id: task_id.to_string(),
                error: RuntimeError::new("momobako.task_failed", "momobako.task_runtime", error),
            },
        };
        match self.state.outcomes.lock() {
            Ok(mut outcomes) => outcomes.publish(task_id.to_string(), outcome),
            Err(_) => crate::app_log!(
                "error",
                "momo.taskRuntime",
                "outcomePublishFailed",
                "Momo 任务终态存储已损坏。",
                serde_json::json!({ "taskId": task_id })
            ),
        }
    }

    fn lookup_task_outcome(&self, handle: &TaskHandle) -> PluginResult<Option<TaskOutcome>> {
        let mut outcomes = self.state.outcomes.lock().map_err(|_| {
            gateway_error(
                "plugin.task.state_poisoned",
                "plugin.task.outcome",
                "outcome state poisoned",
                "restart the plugin host",
            )
        })?;
        match outcomes.lookup(&handle.task_id) {
            OutcomeLookup::Retained(outcome) => return Ok(Some(outcome)),
            OutcomeLookup::Expired => {
                return Err(gateway_error(
                    ERR_TASK_EXPIRED,
                    "plugin.task.outcome",
                    format!("task outcome was evicted: {}", handle.task_id),
                    "submit a new task with a fresh unique task id",
                ));
            }
            OutcomeLookup::Missing => {}
        }
        let active = self
            .state
            .cancellations
            .lock()
            .map_err(|_| {
                gateway_error(
                    "plugin.task.state_poisoned",
                    "plugin.task.outcome",
                    "cancellation state poisoned",
                    "restart the plugin host",
                )
            })?
            .contains_key(&handle.task_id);
        if active {
            Ok(None)
        } else {
            Err(gateway_error(
                ERR_TASK_NOT_FOUND,
                "plugin.task.outcome",
                format!("task handle is unknown: {}", handle.task_id),
                "submit the task before querying its outcome",
            ))
        }
    }
}

impl PluginTaskGateway for MomoTaskRuntime {
    fn submit_batch(&self, batch: TaskBatch) -> PluginResult<Vec<TaskHandle>> {
        let batch_id = batch.batch_id.clone();
        let mut task_ids = BTreeSet::new();
        for task in &batch.tasks {
            if task.task_id.trim().is_empty() {
                return Err(gateway_error(
                    "plugin.task.invalid_id",
                    "plugin.task.submit",
                    format!("batch {batch_id} contains an empty task id"),
                    "provide a non-empty unique task id",
                ));
            }
            if !task_ids.insert(task.task_id.clone()) {
                return Err(gateway_error(
                    "plugin.task.duplicate",
                    "plugin.task.submit",
                    format!("batch {batch_id} repeats task id: {}", task.task_id),
                    "provide a unique task id for every submission",
                ));
            }
        }

        let interactive_count = batch
            .tasks
            .iter()
            .filter(|task| is_interactive(&task.protocol_id))
            .count();
        let background_count = batch.tasks.len() - interactive_count;

        // 两条 lane 都预留成功后才注册任务，任一失败时批次保持零入队。
        let mut interactive_permits = reserve_lane(
            &self.state.interactive_tx,
            interactive_count,
            &batch_id,
            "interactive",
        )?;
        let mut background_permits = reserve_lane(
            &self.state.background_tx,
            background_count,
            &batch_id,
            "background",
        )?;
        let prepared = batch
            .tasks
            .into_iter()
            .map(Self::prepare_gateway_task)
            .collect::<Vec<_>>();

        {
            let mut outcomes = self.state.outcomes.lock().map_err(|_| {
                gateway_error(
                    "plugin.task.state_poisoned",
                    "plugin.task.submit",
                    "outcome state poisoned",
                    "restart the plugin host",
                )
            })?;
            outcomes.prune_expired(Instant::now());
            let mut cancellations = self.state.cancellations.lock().map_err(|_| {
                gateway_error(
                    "plugin.task.state_poisoned",
                    "plugin.task.submit",
                    "cancellation state poisoned",
                    "restart the plugin host",
                )
            })?;
            for item in &prepared {
                if cancellations.contains_key(&item.handle.task_id)
                    || outcomes.contains(&item.handle.task_id)
                {
                    return Err(gateway_error(
                        "plugin.task.duplicate",
                        "plugin.task.submit",
                        format!("task id is already registered: {}", item.handle.task_id),
                        "provide a unique task id for every submission",
                    ));
                }
            }
            for item in &prepared {
                cancellations.insert(
                    item.handle.task_id.clone(),
                    item.request.cancellation.clone(),
                );
            }
        }

        let mut handles = Vec::with_capacity(prepared.len());
        for item in prepared {
            let permit = if item.interactive {
                interactive_permits
                    .as_mut()
                    .and_then(Iterator::next)
                    .expect("interactive lane reservation count must match prepared tasks")
            } else {
                background_permits
                    .as_mut()
                    .and_then(Iterator::next)
                    .expect("background lane reservation count must match prepared tasks")
            };
            handles.push(item.handle);
            permit.send(item.request);
        }
        Ok(handles)
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

fn reserve_lane<'a>(
    sender: &'a mpsc::Sender<TaskRequest>,
    count: usize,
    batch_id: &str,
    lane: &str,
) -> PluginResult<Option<mpsc::PermitIterator<'a, TaskRequest>>> {
    if count == 0 {
        return Ok(None);
    }
    sender
        .try_reserve_many(count)
        .map(Some)
        .map_err(|error| match error {
            TrySendError::Full(()) => gateway_error(
                "plugin.task.queue_full",
                "plugin.task.submit",
                format!("batch {batch_id} needs {count} slots in the {lane} lane"),
                "retry the complete batch after lane capacity becomes available",
            ),
            TrySendError::Closed(()) => gateway_error(
                "plugin.task.queue_closed",
                "plugin.task.submit",
                format!("the {lane} lane is closed for batch {batch_id}"),
                "reload or restart the plugin host before retrying",
            ),
        })
}

fn gateway_error(
    code: &str,
    route: &str,
    detail: impl Into<String>,
    recovery: &str,
) -> PluginHostError {
    let mut error = plugin_error(route, detail);
    error.error.code = code.to_string();
    error.error.recovery = Some(recovery.to_string());
    error
}

fn runtime_error_is_cancelled(error: &RuntimeError) -> bool {
    error.evidence.values().any(|value| {
        matches!(value, ScalarValue::String(message) if message.contains("repository operation cancelled"))
    })
}

fn spawn_lane_workers(
    state: Weak<RuntimeState>,
    receiver: mpsc::Receiver<TaskRequest>,
    count: usize,
    lane: &'static str,
) {
    let receiver = Arc::new(AsyncMutex::new(receiver));
    for worker_id in 0..count {
        let state = state.clone();
        let receiver = receiver.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                let request = receiver.lock().await.recv().await;
                let Some(request) = request else { break };
                let Some(state) = state.upgrade() else { break };
                let result_runtime = MomoTaskRuntime { state };
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

#[cfg(test)]
#[path = "task_runtime_tests.rs"]
mod tests;
