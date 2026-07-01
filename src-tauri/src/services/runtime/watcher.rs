//! Filesystem watcher lifecycle and repository watch-set synchronization.

use crate::services::repository::{
    backend_summary_supports_local_root_access, RepositoryState,
    RepositoryStructureRefreshRequest, RepositoryStructureUpdatedEvent, RepositorySummary,
    SyncRequest,
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

fn repository_supports_local_watch(summary: &RepositorySummary) -> bool {
    backend_summary_supports_local_root_access(&summary.backend)
        && summary
            .backend
            .capabilities
            .iter()
            .any(|value| value == "watch")
        && Path::new(&summary.path).is_absolute()
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
        .filter(repository_supports_local_watch)
        .map(|repository| PathBuf::from(repository.path))
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
