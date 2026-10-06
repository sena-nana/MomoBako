//! 内建长任务运行中取消的跨领域验收测试。

use super::{
    operations::RepositoryTaskExecutor, PROTOCOL_ARCHIVE_IMPORT, PROTOCOL_EAGLE_IMPORT,
    PROTOCOL_ENTRY_COPY, PROTOCOL_ENTRY_DELETE, PROTOCOL_ENTRY_MOVE, PROTOCOL_PLAYLIST_DOWNLOAD,
};
use crate::services::{
    repository::{
        set_test_downloader_track_package_hook,
        test_support::{
            create_repository_without_initial_sync, create_test_state, playback_test_lock,
        },
        CancellationCheck, SyncRequest,
    },
    runtime::{
        external_api::build_external_connection_status,
        watcher::{watched_paths_for_test, RepositoryWatcher},
        RepositoryRuntime,
    },
};
use mutsuki_runtime_contracts::{DomainEvent, RunnerResult, RuntimeError, Task};
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Condvar, Mutex, OnceLock,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const WAIT_TIMEOUT: Duration = Duration::from_secs(5);

type ExecutorOutcome = (Result<RunnerResult, RuntimeError>, Vec<DomainEvent>);

/// 持有真实 RepositoryRuntime，统一核对 watcher、索引和临时文件状态。
struct RuntimeFixture {
    runtime: Option<RepositoryRuntime>,
    root: PathBuf,
    repo_root: PathBuf,
    repo_id: String,
}

impl RuntimeFixture {
    fn new(label: &str) -> Self {
        let (state, root, repo_root, _) = create_test_state(label);
        let repo_id = create_repository_without_initial_sync(&state, &repo_root);
        let repository_state = Arc::new(state);
        repository_state
            .sync_repository(SyncRequest {
                repo_id: repo_id.clone(),
            })
            .expect("initial repository index should be created");
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
            runtime: Some(runtime),
            root,
            repo_root,
            repo_id,
        }
    }

    fn runtime(&self) -> &RepositoryRuntime {
        self.runtime
            .as_ref()
            .expect("test runtime should be active")
    }

    fn sync(&self) {
        self.runtime()
            .repository_state
            .sync_repository(SyncRequest {
                repo_id: self.repo_id.clone(),
            })
            .expect("repository index should synchronize");
    }

    fn assert_consistent(&self, expected_active_paths: &[&str]) {
        let watched_paths = watched_paths_for_test(&self.runtime().watcher_handle)
            .expect("watcher paths should be readable");
        assert_eq!(
            watched_paths,
            BTreeSet::from([self.repo_root.clone()]),
            "cancellation must not change the repository watch set"
        );

        let snapshot = self
            .runtime()
            .repository_state
            .load_snapshot(&self.repo_id)
            .expect("repository snapshot should remain readable");
        let active_paths = snapshot
            .assets
            .iter()
            .filter(|asset| asset.status != "deleted")
            .map(|asset| asset.path.clone())
            .collect::<BTreeSet<_>>();
        let expected = expected_active_paths
            .iter()
            .map(|path| (*path).to_string())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            active_paths, expected,
            "filesystem and active index diverged"
        );
        assert!(
            !contains_partial_file(&self.repo_root),
            "cancelled operation left a .momobako-part temporary file"
        );
    }
}

impl Drop for RuntimeFixture {
    fn drop(&mut self) {
        self.runtime.take();
        for _ in 0..20 {
            if fs::remove_dir_all(&self.root).is_ok() || !self.root.exists() {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}

/// 在领域检查点满足条件时阻塞，直到测试线程发出取消。
struct CancellationGate {
    predicate: Box<dyn Fn() -> bool + Send + Sync>,
    state: Mutex<CancellationGateState>,
    changed: Condvar,
}

#[derive(Default)]
struct CancellationGateState {
    reached: bool,
    cancelled: bool,
}

impl CancellationGate {
    fn after_checkpoints(limit: usize) -> Arc<Self> {
        let checkpoints = AtomicUsize::new(0);
        Self::when(move || checkpoints.fetch_add(1, Ordering::AcqRel) + 1 >= limit)
    }

    fn when(predicate: impl Fn() -> bool + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(Self {
            predicate: Box::new(predicate),
            state: Mutex::new(CancellationGateState::default()),
            changed: Condvar::new(),
        })
    }

    fn wait_until_reached(&self) {
        let deadline = Instant::now() + WAIT_TIMEOUT;
        let mut state = self.state.lock().expect("cancellation gate should lock");
        while !state.reached {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                state.cancelled = true;
                self.changed.notify_all();
                panic!("operation did not reach the expected cancellation latch");
            }
            let (next, wait) = self
                .changed
                .wait_timeout(state, remaining)
                .expect("cancellation gate should wait");
            state = next;
            if wait.timed_out() && !state.reached {
                state.cancelled = true;
                self.changed.notify_all();
                panic!("operation timed out before the cancellation latch");
            }
        }
    }

    fn cancel(&self) {
        let mut state = self.state.lock().expect("cancellation gate should lock");
        state.cancelled = true;
        self.changed.notify_all();
    }
}

impl CancellationCheck for CancellationGate {
    fn is_cancelled(&self) -> bool {
        let mut state = self.state.lock().expect("cancellation gate should lock");
        if state.cancelled {
            return true;
        }
        if !state.reached && (self.predicate)() {
            state.reached = true;
            self.changed.notify_all();
            while !state.cancelled {
                state = self
                    .changed
                    .wait(state)
                    .expect("cancellation gate should wait");
            }
            return true;
        }
        false
    }
}

struct ManualCancellation(AtomicBool);

impl ManualCancellation {
    fn new() -> Arc<Self> {
        Arc::new(Self(AtomicBool::new(false)))
    }

    fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
}

impl CancellationCheck for ManualCancellation {
    fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

fn spawn_executor(
    runtime: RepositoryRuntime,
    cancellation: Arc<dyn CancellationCheck>,
    protocol_id: &'static str,
    payload: Value,
) -> JoinHandle<ExecutorOutcome> {
    thread::spawn(move || {
        let task = Task::new("issue-11-cancellation", protocol_id, payload);
        let mut events = Vec::new();
        let result = RepositoryTaskExecutor::new(&runtime, cancellation.as_ref())
            .execute(&task, &mut events);
        (result, events)
    })
}

fn assert_cancelled_without_result(outcome: ExecutorOutcome) -> Vec<DomainEvent> {
    let (result, events) = outcome;
    let error = match result {
        Ok(_) => panic!("cancelled task must not publish a result"),
        Err(error) => error,
    };
    assert!(
        format!("{error:?}").contains("repository operation cancelled"),
        "unexpected cancellation error: {error:?}"
    );
    events
}

fn contains_partial_file(root: &Path) -> bool {
    let Ok(entries) = fs::read_dir(root) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let path = entry.path();
        if path.is_dir() {
            contains_partial_file(&path)
        } else {
            entry
                .file_name()
                .to_string_lossy()
                .contains(".momobako-part-")
        }
    })
}

fn write_large_zip(path: &Path) {
    let file = File::create(path).expect("test archive should be created");
    let mut archive = zip::ZipWriter::new(file);
    archive
        .start_file(
            "large.bin",
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored),
        )
        .expect("archive entry should start");
    archive
        .write_all(&vec![7_u8; 2 * 1024 * 1024])
        .expect("archive entry should be written");
    archive.finish().expect("test archive should finish");
}

#[test]
fn cancellation_while_waiting_for_repository_write_lock_has_no_side_effect() {
    let fixture = RuntimeFixture::new("issue-11-write-lock");
    fs::write(fixture.repo_root.join("locked.txt"), b"keep")
        .expect("locked source should be written");
    fixture.sync();

    let held = fixture
        .runtime()
        .write_lock
        .lock()
        .expect("test should hold repository write lock");
    let cancellation = CancellationGate::after_checkpoints(2);
    let worker = spawn_executor(
        fixture.runtime().clone(),
        cancellation.clone(),
        PROTOCOL_ENTRY_DELETE,
        json!({
            "repoId": fixture.repo_id,
            "paths": ["locked.txt"],
            "mode": "delete"
        }),
    );

    cancellation.wait_until_reached();
    cancellation.cancel();
    drop(held);

    let events = assert_cancelled_without_result(worker.join().expect("worker should exit"));
    assert!(
        events.is_empty(),
        "lock cancellation must not emit progress"
    );
    assert!(fixture.repo_root.join("locked.txt").is_file());
    fixture.assert_consistent(&["locked.txt"]);
}

struct DownloadHookGate {
    state: Mutex<DownloadHookState>,
    changed: Condvar,
    calls: AtomicUsize,
}

#[derive(Default)]
struct DownloadHookState {
    entered: bool,
    released: bool,
}

impl DownloadHookGate {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(DownloadHookState::default()),
            changed: Condvar::new(),
            calls: AtomicUsize::new(0),
        })
    }

    fn enter_and_wait(&self) {
        self.calls.fetch_add(1, Ordering::AcqRel);
        let mut state = self.state.lock().expect("download hook gate should lock");
        state.entered = true;
        self.changed.notify_all();
        while !state.released {
            state = self
                .changed
                .wait(state)
                .expect("download hook gate should wait");
        }
    }

    fn wait_until_entered(&self) {
        let deadline = Instant::now() + WAIT_TIMEOUT;
        let mut state = self.state.lock().expect("download hook gate should lock");
        while !state.entered {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                state.released = true;
                self.changed.notify_all();
                panic!("download hook was not entered");
            }
            let (next, wait) = self
                .changed
                .wait_timeout(state, remaining)
                .expect("download hook gate should wait");
            state = next;
            if wait.timed_out() && !state.entered {
                state.released = true;
                self.changed.notify_all();
                panic!("download hook entry timed out");
            }
        }
    }

    fn release(&self) {
        let mut state = self.state.lock().expect("download hook gate should lock");
        state.released = true;
        self.changed.notify_all();
    }
}

static DOWNLOAD_HOOK_GATE: OnceLock<Mutex<Option<Arc<DownloadHookGate>>>> = OnceLock::new();

fn blocking_download_hook(payload: Value) -> Result<Value, String> {
    let gate = DOWNLOAD_HOOK_GATE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .expect("download hook state should lock")
        .clone()
        .expect("download hook gate should be installed");
    gate.enter_and_wait();
    Ok(json!({
        "songId": payload.get("songId").cloned().unwrap_or(Value::Null),
        "paths": ["C:/Mock/cancelled-track.mp3"]
    }))
}

struct DownloadHookReset;

impl Drop for DownloadHookReset {
    fn drop(&mut self) {
        set_test_downloader_track_package_hook(None);
        if let Ok(mut gate) = DOWNLOAD_HOOK_GATE.get_or_init(|| Mutex::new(None)).lock() {
            *gate = None;
        }
    }
}

#[test]
fn cancellation_during_download_suppresses_later_progress_and_result() {
    let _lock = playback_test_lock();
    let fixture = RuntimeFixture::new("issue-11-download");
    let hook_gate = DownloadHookGate::new();
    *DOWNLOAD_HOOK_GATE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .expect("download hook state should lock") = Some(hook_gate.clone());
    set_test_downloader_track_package_hook(Some(blocking_download_hook));
    let _reset = DownloadHookReset;
    let cancellation = ManualCancellation::new();
    let worker = spawn_executor(
        fixture.runtime().clone(),
        cancellation.clone(),
        PROTOCOL_PLAYLIST_DOWNLOAD,
        json!({
            "playlistId": 11,
            "playlistName": "取消验收",
            "tracks": [
                { "songId": 101, "songName": "first" },
                { "songId": 102, "songName": "second" }
            ],
            "destination": { "kind": "localFolder", "path": "C:/Mock/Issue11" }
        }),
    );

    hook_gate.wait_until_entered();
    cancellation.cancel();
    hook_gate.release();

    let events = assert_cancelled_without_result(worker.join().expect("worker should exit"));
    assert_eq!(hook_gate.calls.load(Ordering::Acquire), 1);
    assert_eq!(
        events.len(),
        1,
        "no progress may be appended after cancellation"
    );
    assert_eq!(events[0].payload["progress"]["phase"], json!("start"));
    fixture.assert_consistent(&[]);
}

#[test]
fn cancellation_during_archive_import_removes_partial_file_and_index_entry() {
    let fixture = RuntimeFixture::new("issue-11-archive");
    let archive_path = fixture.root.join("large.zip");
    write_large_zip(&archive_path);
    let target_root = fixture.repo_root.clone();
    let cancellation = CancellationGate::when(move || contains_partial_file(&target_root));
    let worker = spawn_executor(
        fixture.runtime().clone(),
        cancellation.clone(),
        PROTOCOL_ARCHIVE_IMPORT,
        json!({
            "repoId": fixture.repo_id,
            "parentPath": "",
            "archivePath": archive_path
        }),
    );

    cancellation.wait_until_reached();
    cancellation.cancel();

    let events = assert_cancelled_without_result(worker.join().expect("worker should exit"));
    assert!(events.is_empty());
    assert!(!fixture.repo_root.join("large.bin").exists());
    fixture.assert_consistent(&[]);
}

#[test]
fn cancellation_before_eagle_importer_call_preserves_watcher_and_index() {
    let fixture = RuntimeFixture::new("issue-11-eagle");
    let library_path = fixture.root.join("Example.library");
    fs::create_dir_all(&library_path).expect("Eagle library fixture should be created");
    let cancellation = CancellationGate::after_checkpoints(4);
    let worker = spawn_executor(
        fixture.runtime().clone(),
        cancellation.clone(),
        PROTOCOL_EAGLE_IMPORT,
        json!({
            "repoId": fixture.repo_id,
            "parentPath": "",
            "libraryPath": library_path,
            "mode": "copy"
        }),
    );

    cancellation.wait_until_reached();
    cancellation.cancel();

    let events = assert_cancelled_without_result(worker.join().expect("worker should exit"));
    assert!(events.is_empty());
    fixture.assert_consistent(&[]);
}

#[test]
fn cancellation_during_copy_removes_atomic_temporary_file() {
    let fixture = RuntimeFixture::new("issue-11-copy");
    fs::create_dir_all(fixture.repo_root.join("Copies"))
        .expect("copy target directory should be created");
    fs::write(
        fixture.repo_root.join("source.bin"),
        vec![9_u8; 2 * 1024 * 1024],
    )
    .expect("copy source should be written");
    fixture.sync();

    let target_root = fixture.repo_root.join("Copies");
    let cancellation = CancellationGate::when(move || contains_partial_file(&target_root));
    let worker = spawn_executor(
        fixture.runtime().clone(),
        cancellation.clone(),
        PROTOCOL_ENTRY_COPY,
        json!({
            "repoId": fixture.repo_id,
            "sourcePaths": ["source.bin"],
            "parentPath": "Copies",
            "mode": "copy"
        }),
    );

    cancellation.wait_until_reached();
    cancellation.cancel();

    let events = assert_cancelled_without_result(worker.join().expect("worker should exit"));
    assert!(events.is_empty());
    assert!(!fixture.repo_root.join("Copies/source.bin").exists());
    fixture.assert_consistent(&["source.bin"]);
}

#[test]
fn cancellation_between_move_items_stops_later_side_effects() {
    let fixture = RuntimeFixture::new("issue-11-move");
    fs::create_dir_all(fixture.repo_root.join("Archive"))
        .expect("move target directory should be created");
    fs::write(fixture.repo_root.join("first.txt"), b"first")
        .expect("first move source should be written");
    fs::write(fixture.repo_root.join("second.txt"), b"second")
        .expect("second move source should be written");
    fixture.sync();

    let first_target = fixture.repo_root.join("Archive/first.txt");
    let cancellation = CancellationGate::when(move || first_target.is_file());
    let worker = spawn_executor(
        fixture.runtime().clone(),
        cancellation.clone(),
        PROTOCOL_ENTRY_MOVE,
        json!({
            "repoId": fixture.repo_id,
            "sourcePaths": ["first.txt", "second.txt"],
            "parentPath": "Archive"
        }),
    );

    cancellation.wait_until_reached();
    cancellation.cancel();

    let events = assert_cancelled_without_result(worker.join().expect("worker should exit"));
    assert!(events.is_empty());
    assert!(fixture.repo_root.join("Archive/first.txt").is_file());
    assert!(!fixture.repo_root.join("first.txt").exists());
    assert!(fixture.repo_root.join("second.txt").is_file());
    assert!(!fixture.repo_root.join("Archive/second.txt").exists());
    fixture.assert_consistent(&["Archive/first.txt", "second.txt"]);
}

#[test]
fn cancellation_between_batch_delete_items_stops_later_side_effects() {
    let fixture = RuntimeFixture::new("issue-11-batch-delete");
    fs::write(fixture.repo_root.join("first.txt"), b"first")
        .expect("first delete target should be written");
    fs::write(fixture.repo_root.join("second.txt"), b"second")
        .expect("second delete target should be written");
    fixture.sync();

    let first_source = fixture.repo_root.join("first.txt");
    let cancellation = CancellationGate::when(move || !first_source.exists());
    let worker = spawn_executor(
        fixture.runtime().clone(),
        cancellation.clone(),
        PROTOCOL_ENTRY_DELETE,
        json!({
            "repoId": fixture.repo_id,
            "paths": ["first.txt", "second.txt"],
            "mode": "delete"
        }),
    );

    cancellation.wait_until_reached();
    cancellation.cancel();

    let events = assert_cancelled_without_result(worker.join().expect("worker should exit"));
    assert!(events.is_empty());
    assert!(!fixture.repo_root.join("first.txt").exists());
    assert!(fixture.repo_root.join("second.txt").is_file());
    fixture.assert_consistent(&["second.txt"]);
}
