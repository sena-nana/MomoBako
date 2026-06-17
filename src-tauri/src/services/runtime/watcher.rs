//! Filesystem watcher lifecycle and repository watch-set synchronization.

use crate::services::repository::{RepositoryState, SyncRequest};
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::{mpsc::channel, Arc, Mutex},
    thread,
};

const LOCAL_FILESYSTEM_PLUGIN_ID: &str = "momobako.local-filesystem";

#[derive(Debug)]
pub(crate) struct RepositoryWatcher {
    watcher: RecommendedWatcher,
    watched_paths: BTreeSet<PathBuf>,
}

impl RepositoryWatcher {
    /// Starts the shared filesystem watcher and synchronizes the initial watch-set.
    pub(crate) fn start(
        repository_state: Arc<RepositoryState>,
        write_lock: Arc<Mutex<()>>,
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
                    handle_fs_event(&repository_state_for_thread, &write_lock, event);
                }
            }
        });

        sync_watched_paths(&repository_state, &handle)?;
        Ok(handle)
    }
}

/// Reconciles the runtime watch-set with currently attached local repositories.
pub(crate) fn sync_watched_paths(
    repository_state: &Arc<RepositoryState>,
    watcher_handle: &Arc<Mutex<RepositoryWatcher>>,
) -> Result<(), String> {
    let repositories = repository_state.list_repositories()?;
    let desired_paths = repositories
        .into_iter()
        .filter(|repository| repository.backend.plugin_id == LOCAL_FILESYSTEM_PLUGIN_ID)
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

fn handle_fs_event(
    repository_state: &Arc<RepositoryState>,
    write_lock: &Arc<Mutex<()>>,
    event: Event,
) {
    let Ok(repositories) = repository_state.list_repositories() else {
        return;
    };

    for path in event.paths {
        let normalized_path = normalize_path(&path);
        if let Some(repository) = repositories
            .iter()
            .find(|repo| normalized_path.starts_with(&normalize_path(Path::new(&repo.path))))
        {
            let Ok(_guard) = write_lock.lock() else {
                return;
            };
            let _ = repository_state.sync_repository(SyncRequest {
                repo_id: repository.repo_id.clone(),
            });
        }
    }
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
