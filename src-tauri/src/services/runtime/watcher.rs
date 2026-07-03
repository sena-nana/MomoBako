//! Filesystem watcher lifecycle and repository watch-set synchronization.

use crate::services::repository::{
    backend_summary_supports_local_root_access, RepositoryState, RepositoryStructureRefreshRequest,
    RepositoryStructureUpdatedEvent, RepositorySummary, SyncRequest,
};
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::{
        mpsc::{channel, Receiver, Sender},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

const STRUCTURE_REFRESH_DEBOUNCE_MS: u64 = 400;

/// 仅为状态正常且路径真实存在的本地仓库建立监听，避免缺失路径阻塞应用启动。
fn repository_watch_path(summary: &RepositorySummary) -> Option<PathBuf> {
    let path = PathBuf::from(&summary.path);
    (summary.status == "ready"
        && backend_summary_supports_local_root_access(&summary.backend)
        && summary
            .backend
            .capabilities
            .iter()
            .any(|value| value == "watch")
        && path.is_absolute()
        && (path.is_dir() || path.is_file()))
    .then_some(path)
}

#[derive(Debug)]
pub(crate) struct RepositoryWatcher {
    watcher: RecommendedWatcher,
    watched_paths: BTreeSet<PathBuf>,
}

impl RepositoryWatcher {
    /// Starts the shared filesystem watcher and synchronizes the initial watch-set.
    pub(crate) fn start(
        repository_state: Arc<RepositoryState>,
        _write_lock: Arc<Mutex<()>>,
    ) -> Result<Arc<Mutex<Self>>, String> {
        let (tx, rx) = channel::<notify::Result<Event>>();
        let watcher = RecommendedWatcher::new(
            move |result| {
                let _ = tx.send(result);
            },
            Config::default(),
        )
        .map_err(|error| error.to_string())?;

        let handle = Arc::new(Mutex::new(Self {
            watcher,
            watched_paths: BTreeSet::new(),
        }));
        let repository_state_for_thread = repository_state.clone();

        thread::spawn(move || {
            while let Ok(event) = rx.recv() {
                if let Ok(event) = event {
                    handle_fs_event(&repository_state_for_thread, event);
                }
            }
        });

        sync_watched_paths(&repository_state, &handle)?;
        Ok(handle)
    }
}

pub(crate) fn start_structure_refresh_worker(
    repository_state: Arc<RepositoryState>,
    write_lock: Arc<Mutex<()>>,
) -> Result<Sender<RepositoryStructureRefreshRequest>, String> {
    let (tx, rx) = channel::<RepositoryStructureRefreshRequest>();
    thread::spawn(move || {
        run_structure_refresh_worker(repository_state, write_lock, rx);
    });
    Ok(tx)
}

/// Reconciles the runtime watch-set with currently attached local repositories.
pub(crate) fn sync_watched_paths(
    repository_state: &Arc<RepositoryState>,
    watcher_handle: &Arc<Mutex<RepositoryWatcher>>,
) -> Result<(), String> {
    let repositories = repository_state.list_repositories()?;
    let desired_paths = repositories
        .into_iter()
        .filter_map(|repository| repository_watch_path(&repository))
        .collect::<BTreeSet<_>>();

    let mut watcher = watcher_handle
        .lock()
        .map_err(|_| "watcher state lock poisoned".to_string())?;
    let current_paths = watcher.watched_paths.clone();

    for path in current_paths.difference(&desired_paths) {
        watcher
            .watcher
            .unwatch(path)
            .map_err(|error| error.to_string())?;
    }

    for path in desired_paths.difference(&current_paths) {
        watcher
            .watcher
            .watch(path, RecursiveMode::Recursive)
            .map_err(|error| error.to_string())?;
    }

    watcher.watched_paths = desired_paths;
    Ok(())
}

fn handle_fs_event(repository_state: &Arc<RepositoryState>, event: Event) {
    let Ok(repositories) = repository_state.list_repositories() else {
        return;
    };

    for path in event.paths {
        let normalized_path = normalize_path(&path);
        if let Some(repository) = repositories
            .iter()
            .find(|repo| normalized_path.starts_with(&normalize_path(Path::new(&repo.path))))
        {
            repository_state
                .queue_repository_structure_refresh(repository.repo_id.clone(), "watcher");
        }
    }
}

fn run_structure_refresh_worker(
    repository_state: Arc<RepositoryState>,
    write_lock: Arc<Mutex<()>>,
    rx: Receiver<RepositoryStructureRefreshRequest>,
) {
    let mut pending = BTreeMap::<String, (String, Instant)>::new();
    loop {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(request) => {
                pending.insert(request.repo_id, (request.reason, Instant::now()));
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }

        let now = Instant::now();
        let ready = pending
            .iter()
            .filter_map(|(repo_id, (reason, queued_at))| {
                (now.duration_since(*queued_at).as_millis()
                    >= u128::from(STRUCTURE_REFRESH_DEBOUNCE_MS))
                .then_some((repo_id.clone(), reason.clone()))
            })
            .collect::<Vec<_>>();

        for (repo_id, reason) in ready {
            pending.remove(&repo_id);
            let Ok(_guard) = write_lock.lock() else {
                return;
            };
            let _ = repository_state.set_repository_structure_refreshing(&repo_id, true);
            let sync_result = repository_state.sync_repository(SyncRequest {
                repo_id: repo_id.clone(),
            });
            let indexed_at = repository_state
                .repository_structure_indexed_at(&repo_id)
                .ok()
                .flatten();
            let _ = repository_state.set_repository_structure_refreshing(&repo_id, false);
            if sync_result.is_ok() {
                repository_state.emit_repository_structure_updated(
                    RepositoryStructureUpdatedEvent {
                        repo_id,
                        reason,
                        indexed_at,
                    },
                );
            }
        }
    }
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::repository::RepositoryBackendSummary;
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn build_repository_summary(path: &Path, status: &str) -> RepositorySummary {
        RepositorySummary {
            repo_id: "repo-test".to_string(),
            name: "Test Repo".to_string(),
            path: path.to_string_lossy().to_string(),
            backend: RepositoryBackendSummary {
                plugin_id: "local-filesystem".to_string(),
                kind: "filesystem".to_string(),
                name: "Local Filesystem".to_string(),
                capabilities: vec![
                    "browse".to_string(),
                    "read".to_string(),
                    "write".to_string(),
                    "watch".to_string(),
                    "sync".to_string(),
                    "localRootPath".to_string(),
                ],
            },
            status: status.to_string(),
            asset_count: 0,
            updated_at: "2026-07-03T00:00:00Z".to_string(),
            local_cache: None,
        }
    }

    fn unique_test_path(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock must be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "momobako-watcher-{label}-{}-{unique}",
            std::process::id()
        ))
    }

    #[test]
    fn repository_watch_path_returns_existing_ready_path() {
        let repo_root = unique_test_path("existing");
        fs::create_dir_all(&repo_root).expect("test repository root should be created");

        let summary = build_repository_summary(&repo_root, "ready");
        let watch_path = repository_watch_path(&summary);

        assert_eq!(watch_path, Some(repo_root.clone()));
        fs::remove_dir_all(&repo_root).expect("test repository root should be removed");
    }

    #[test]
    fn repository_watch_path_skips_missing_path() {
        let repo_root = unique_test_path("missing");
        let summary = build_repository_summary(&repo_root, "ready");

        assert_eq!(repository_watch_path(&summary), None);
    }
}
