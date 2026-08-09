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
    outcome: TaskOutcome,
    completed_at: Instant,
}

/// 终态按完成顺序保留固定容量，并在 TTL 后惰性回收。
struct OutcomeStore {
    entries: BTreeMap<String, StoredOutcome>,
    completion_order: VecDeque<String>,
    capacity: usize,
    retention: Duration,
}

impl OutcomeStore {
    fn new(capacity: usize, retention: Duration) -> Self {
        Self {
            entries: BTreeMap::new(),
            completion_order: VecDeque::new(),
            capacity: capacity.max(1),
            retention,
        }
    }

    fn publish(&mut self, task_id: String, outcome: TaskOutcome) {
        self.publish_at(task_id, outcome, Instant::now());
    }

    fn publish_at(&mut self, task_id: String, outcome: TaskOutcome, completed_at: Instant) {
        self.prune_expired(completed_at);
        if self.entries.contains_key(&task_id) {
            self.completion_order.retain(|queued| queued != &task_id);
        }
        self.entries.insert(
            task_id.clone(),
            StoredOutcome {
                outcome,
                completed_at,
            },
        );
        self.completion_order.push_back(task_id);
        while self.entries.len() > self.capacity {
            self.evict_oldest();
        }
    }

    fn lookup(&mut self, task_id: &str) -> Option<TaskOutcome> {
        self.lookup_at(task_id, Instant::now())
    }

    fn lookup_at(&mut self, task_id: &str, now: Instant) -> Option<TaskOutcome> {
        self.prune_expired(now);
        self.entries
            .get(task_id)
            .map(|stored| stored.outcome.clone())
    }

    fn contains(&self, task_id: &str) -> bool {
        self.entries.contains_key(task_id)
    }

    fn prune_expired(&mut self, now: Instant) {
        loop {
            let Some(task_id) = self.completion_order.front() else {
                break;
            };
            let Some(stored) = self.entries.get(task_id) else {
                self.completion_order.pop_front();
                continue;
            };
            if now.saturating_duration_since(stored.completed_at) < self.retention {
                break;
            }
            self.evict_oldest();
        }
    }

    fn evict_oldest(&mut self) {
        if let Some(task_id) = self.completion_order.pop_front() {
            self.entries.remove(&task_id);
        }
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.len()
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
        let (interactive_tx, interactive_rx) = mpsc::channel(INTERACTIVE_QUEUE_LIMIT);
        let (background_tx, background_rx) = mpsc::channel(BACKGROUND_QUEUE_LIMIT);
        let state = Arc::new(RuntimeState {
            runtime,
            interactive_tx,
            background_tx,
            next_task_id: AtomicU64::new(1),
            cancellations: Mutex::new(BTreeMap::new()),
            outcomes: Mutex::new(OutcomeStore::new(OUTCOME_CAPACITY, OUTCOME_RETENTION)),
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
        if let Some(outcome) = outcomes.lookup(&handle.task_id) {
            return Ok(Some(outcome));
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
                "plugin.task.outcome_not_found",
                "plugin.task.outcome",
                format!(
                    "task outcome expired or handle is unknown: {}",
                    handle.task_id
                ),
                "submit a new task and retain its current handle",
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
mod tests {
    use super::*;
    use crate::services::mutsuki_runner::protocols::{
        PROTOCOL_ENTRY_DELETE, PROTOCOL_REPOSITORY_SYNC,
    };
    use crate::services::repository::{
        test_support::{create_repository_without_initial_sync, create_test_state},
        SyncRequest,
    };
    use crate::services::runtime::{
        external_api::build_external_connection_status, watcher::RepositoryWatcher,
    };
    use mutsuki_plugin_api::PluginHostContext;
    use mutsuki_runtime_wire::{
        decode_binary_response, encode_binary_request, CancelTaskRequest, SubmitTaskBatchRequest,
        TaskOutcomeRequest, WireRequest, DEFAULT_WIRE_LIMITS,
    };
    use std::fs;
    use std::path::PathBuf;
    use std::thread;

    const TEST_TIMEOUT: Duration = Duration::from_secs(5);

    /// 为 ABI dispatch 测试提供真实 RepositoryRuntime 与 watcher。
    struct RuntimeFixture {
        runtime: RepositoryRuntime,
        root: PathBuf,
        repo_root: PathBuf,
        repo_id: String,
    }

    impl RuntimeFixture {
        fn new(label: &str) -> Self {
            let (state, root, repo_root, _) = create_test_state(label);
            let repo_id = create_repository_without_initial_sync(&state, &repo_root);
            state
                .sync_repository(SyncRequest {
                    repo_id: repo_id.clone(),
                })
                .expect("initial repository index should be created");
            let repository_state = Arc::new(state);
            let write_lock = Arc::new(Mutex::new(()));
            let watcher_handle =
                RepositoryWatcher::start(repository_state.clone(), write_lock.clone())
                    .expect("test repository watcher should start");
            let runtime = RepositoryRuntime {
                repository_state,
                watcher_handle,
                write_lock,
                preview_addr: "127.0.0.1:0".to_string(),
                external_connection: build_external_connection_status(
                    &root.join("state"),
                    "127.0.0.1:0",
                    "test-token",
                    "0",
                ),
            };
            Self {
                runtime,
                root,
                repo_root,
                repo_id,
            }
        }
    }

    impl Drop for RuntimeFixture {
        fn drop(&mut self) {
            for _ in 0..20 {
                if fs::remove_dir_all(&self.root).is_ok() || !self.root.exists() {
                    break;
                }
                thread::sleep(Duration::from_millis(10));
            }
        }
    }

    fn dispatch<R: WireRequest>(
        context: &PluginHostContext,
        request_id: u64,
        request: &R,
    ) -> Result<R::Response, RuntimeError> {
        let frame = encode_binary_request(request_id, request, DEFAULT_WIRE_LIMITS)
            .expect("ABI request should encode");
        let response = context.dispatch_binary_request(&frame);
        decode_binary_response::<R>(&response, request_id, DEFAULT_WIRE_LIMITS)
    }

    fn wait_for_outcome(context: &PluginHostContext, handle: &TaskHandle) -> TaskOutcome {
        let deadline = Instant::now() + TEST_TIMEOUT;
        loop {
            let outcome = dispatch(
                context,
                900,
                &TaskOutcomeRequest {
                    handle: handle.clone(),
                },
            )
            .expect("task outcome dispatch should succeed");
            if let Some(outcome) = outcome {
                return outcome;
            }
            assert!(
                Instant::now() < deadline,
                "task did not publish a terminal outcome"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn completed_outcome(task_id: &str) -> TaskOutcome {
        TaskOutcome::Completed {
            task_id: task_id.to_string(),
            output: Some(Value::Null),
            output_ref: None,
        }
    }

    #[test]
    fn outcome_store_applies_ttl_and_fifo_capacity() {
        let started_at = Instant::now();
        let mut outcomes = OutcomeStore::new(4, Duration::from_secs(30));
        for index in 0..100 {
            let task_id = format!("capacity-{index}");
            outcomes.publish_at(
                task_id.clone(),
                completed_outcome(&task_id),
                started_at + Duration::from_millis(index),
            );
        }
        assert_eq!(outcomes.len(), 4);
        assert!(outcomes
            .lookup_at("capacity-0", started_at + Duration::from_secs(1))
            .is_none());
        assert!(outcomes
            .lookup_at("capacity-99", started_at + Duration::from_secs(31))
            .is_none());
        assert_eq!(outcomes.len(), 0);
    }

    #[test]
    fn mixed_batch_capacity_failure_enqueues_nothing() {
        let fixture = RuntimeFixture::new("issue-13-batch-atomic");
        let runtime = MomoTaskRuntime::new(fixture.runtime.clone());
        let mut tasks = vec![Task::new(
            "interactive-task",
            PROTOCOL_THUMBNAIL_REQUEST,
            serde_json::json!({}),
        )];
        tasks.extend((0..=BACKGROUND_QUEUE_LIMIT).map(|index| {
            Task::new(
                format!("background-task-{index}"),
                PROTOCOL_REPOSITORY_SYNC,
                serde_json::json!({}),
            )
        }));
        let error = runtime
            .submit_batch(TaskBatch {
                batch_id: "capacity-failure".to_string(),
                tick_id: None,
                tasks,
                resource_plan: None,
            })
            .expect_err("oversized background lane reservation should fail");
        assert_eq!(error.error.code, "plugin.task.queue_full");
        assert!(!runtime.cancel("interactive-task"));
        assert!(!runtime.cancel("background-task-0"));
    }

    #[test]
    fn abi_dispatch_submit_then_outcome_returns_completed_terminal_state() {
        let fixture = RuntimeFixture::new("issue-13-abi-outcome");
        let runtime = Arc::new(MomoTaskRuntime::new(fixture.runtime.clone()));
        let context = PluginHostContext::default().with_task_gateway(runtime);
        let handles = dispatch(
            &context,
            1,
            &SubmitTaskBatchRequest {
                batch: TaskBatch::one(
                    "abi-outcome-batch",
                    Task::new(
                        "abi-outcome-task",
                        PROTOCOL_REPOSITORY_SYNC,
                        serde_json::json!({ "repoId": fixture.repo_id }),
                    ),
                ),
            },
        )
        .expect("ABI submit dispatch should succeed");
        let [handle] = handles.as_slice() else {
            panic!("ABI submit should return one handle")
        };

        let outcome = wait_for_outcome(&context, handle);
        assert!(matches!(
            outcome,
            TaskOutcome::Completed { ref task_id, .. } if task_id == "abi-outcome-task"
        ));
    }

    #[test]
    fn abi_dispatch_submit_returns_handle_before_cancelled_task_finishes() {
        let fixture = RuntimeFixture::new("issue-13-abi-cancel");
        fs::write(fixture.repo_root.join("keep.txt"), b"keep")
            .expect("delete fixture should be written");
        fixture
            .runtime
            .repository_state
            .sync_repository(SyncRequest {
                repo_id: fixture.repo_id.clone(),
            })
            .expect("delete fixture should be indexed");
        let runtime = Arc::new(MomoTaskRuntime::new(fixture.runtime.clone()));
        let context = PluginHostContext::default().with_task_gateway(runtime);
        let held = fixture
            .runtime
            .write_lock
            .lock()
            .expect("test should hold the repository write lock");

        let handles = dispatch(
            &context,
            10,
            &SubmitTaskBatchRequest {
                batch: TaskBatch::one(
                    "abi-cancel-batch",
                    Task::new(
                        "abi-cancel-task",
                        PROTOCOL_ENTRY_DELETE,
                        serde_json::json!({
                            "repoId": fixture.repo_id,
                            "paths": ["keep.txt"],
                            "mode": "delete"
                        }),
                    ),
                ),
            },
        )
        .expect("ABI submit must return while the task is blocked");
        let [handle] = handles.as_slice() else {
            panic!("ABI submit should return one handle")
        };
        assert!(dispatch(
            &context,
            11,
            &TaskOutcomeRequest {
                handle: handle.clone(),
            },
        )
        .expect("running outcome dispatch should succeed")
        .is_none());
        dispatch(
            &context,
            12,
            &CancelTaskRequest {
                handle: handle.clone(),
            },
        )
        .expect("ABI cancel dispatch should succeed");
        drop(held);

        let outcome = wait_for_outcome(&context, handle);
        assert!(matches!(
            outcome,
            TaskOutcome::Cancelled { ref task_id, .. } if task_id == "abi-cancel-task"
        ));
        assert!(fixture.repo_root.join("keep.txt").is_file());
    }
}
