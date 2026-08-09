//! Momo task runtime 的有界队列、终态与 ABI 回归测试。

use super::*;
use crate::services::mutsuki_runner::protocols::{PROTOCOL_ENTRY_DELETE, PROTOCOL_REPOSITORY_SYNC};
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
        let watcher_handle = RepositoryWatcher::start(repository_state.clone(), write_lock.clone())
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

fn repository_sync_request(repo_id: &str, batch_id: &str, task_id: &str) -> SubmitTaskBatchRequest {
    SubmitTaskBatchRequest {
        batch: TaskBatch::one(
            batch_id,
            Task::new(
                task_id,
                PROTOCOL_REPOSITORY_SYNC,
                serde_json::json!({ "repoId": repo_id }),
            ),
        ),
    }
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
    assert_eq!(outcomes.terminal.len(), 4);
    assert_eq!(outcomes.expired.len(), 4);
    assert!(matches!(
        outcomes.lookup_at("capacity-0", started_at + Duration::from_secs(1)),
        OutcomeLookup::Missing
    ));
    assert!(matches!(
        outcomes.lookup_at("capacity-95", started_at + Duration::from_secs(1)),
        OutcomeLookup::Expired
    ));
    assert!(matches!(
        outcomes.lookup_at("capacity-99", started_at + Duration::from_secs(1)),
        OutcomeLookup::Retained(_)
    ));
    assert!(matches!(
        outcomes.lookup_at("capacity-99", started_at + Duration::from_secs(31)),
        OutcomeLookup::Expired
    ));
    assert_eq!(outcomes.terminal.len(), 0);
    assert_eq!(outcomes.expired.len(), 4);
    assert!(matches!(
        outcomes.lookup_at("capacity-99", started_at + Duration::from_secs(62)),
        OutcomeLookup::Missing
    ));
    assert_eq!(outcomes.expired.len(), 0);
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
        &repository_sync_request(&fixture.repo_id, "abi-outcome-batch", "abi-outcome-task"),
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
fn abi_dispatch_distinguishes_evicted_and_unknown_task_handles() {
    let fixture = RuntimeFixture::new("issue-13-abi-outcome-state");
    let runtime = Arc::new(MomoTaskRuntime::with_outcome_policy(
        fixture.runtime.clone(),
        1,
        Duration::from_secs(30),
    ));
    let context = PluginHostContext::default().with_task_gateway(runtime);

    let first_handles = dispatch(
        &context,
        20,
        &repository_sync_request(&fixture.repo_id, "abi-first-batch", "abi-first-task"),
    )
    .expect("first ABI submit should succeed");
    let [first_handle] = first_handles.as_slice() else {
        panic!("first ABI submit should return one handle")
    };
    wait_for_outcome(&context, first_handle);

    let second_handles = dispatch(
        &context,
        21,
        &repository_sync_request(&fixture.repo_id, "abi-second-batch", "abi-second-task"),
    )
    .expect("second ABI submit should succeed");
    let [second_handle] = second_handles.as_slice() else {
        panic!("second ABI submit should return one handle")
    };
    wait_for_outcome(&context, second_handle);

    let expired = dispatch(
        &context,
        22,
        &TaskOutcomeRequest {
            handle: first_handle.clone(),
        },
    )
    .expect_err("evicted outcome should be expired");
    assert_eq!(expired.code, ERR_TASK_EXPIRED);

    let duplicate = dispatch(
        &context,
        23,
        &repository_sync_request(&fixture.repo_id, "abi-duplicate-batch", "abi-first-task"),
    )
    .expect_err("expired tombstone should keep the task id reserved");
    assert_eq!(duplicate.code, "plugin.task.duplicate");

    let mut unknown_handle = first_handle.clone();
    unknown_handle.task_id = "abi-unknown-task".to_string();
    let unknown = dispatch(
        &context,
        24,
        &TaskOutcomeRequest {
            handle: unknown_handle,
        },
    )
    .expect_err("unregistered handle should be unknown");
    assert_eq!(unknown.code, ERR_TASK_NOT_FOUND);
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
