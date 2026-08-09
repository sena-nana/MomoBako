//! Filesystem watcher lifecycle and repository watch-set synchronization.

use crate::services::repository::{
    backend_summary_supports_local_root_access, RepositoryState, RepositoryStructureRefreshRequest,
    RepositoryStructureUpdatedEvent, RepositorySummary,
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

fn log_watcher_error(action: &str, message: &str, context: serde_json::Value) {
    crate::app_log!("warn", "runtime.watcher", action, message, context);
}

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
                if tx.send(result).is_err() {
                    log_watcher_error(
                        "eventDispatchFailed",
                        "文件监听事件投递失败。",
                        serde_json::json!({}),
                    );
                }
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
                match event {
                    Ok(event) => handle_fs_event(&repository_state_for_thread, event),
                    Err(error) => {
                        log_watcher_error(
                            "eventReadFailed",
                            "文件监听事件读取失败。",
                            serde_json::json!({ "error": error.to_string() }),
                        );
                    }
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

/// 读取测试运行时的监听集合，用于核对取消后 watcher 未发生漂移。
#[cfg(test)]
pub(crate) fn watched_paths_for_test(
    watcher_handle: &Arc<Mutex<RepositoryWatcher>>,
) -> Result<BTreeSet<PathBuf>, String> {
    watcher_handle
        .lock()
        .map(|watcher| watcher.watched_paths.clone())
        .map_err(|_| "watcher state lock poisoned".to_string())
}

fn handle_fs_event(repository_state: &Arc<RepositoryState>, event: Event) {
    let repositories = match repository_state.list_repositories() {
        Ok(repositories) => repositories,
        Err(error) => {
            log_watcher_error(
                "listRepositoriesFailed",
                "处理文件监听事件时读取资源库列表失败。",
                serde_json::json!({ "error": error }),
            );
            return;
        }
    };

    if event.paths.is_empty() {
        return;
    };

    let mut changed_paths_by_repo = BTreeMap::<String, BTreeSet<String>>::new();
    for path in event.paths {
        for repository in repositories
            .iter()
            .filter(|repo| repository_event_path_is_inside(Path::new(&repo.path), &path))
        {
            let changed_paths = changed_paths_by_repo
                .entry(repository.repo_id.clone())
                .or_default();
            if let Some(relative_path) =
                repository_relative_event_path(Path::new(&repository.path), &path)
            {
                changed_paths.insert(relative_path);
            }
        }
    }

    for (repo_id, paths) in changed_paths_by_repo {
        repository_state.queue_repository_structure_refresh_with_paths(repo_id, "watcher", paths);
    }
}

fn repository_relative_event_path(repo_root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(repo_root).ok()?;
    let raw = relative.to_string_lossy().replace('\\', "/");
    let mut parts = Vec::new();
    for part in raw.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." || matches!(part, ".momo" | ".meta") {
            return None;
        }
        parts.push(part);
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

fn run_structure_refresh_worker(
    repository_state: Arc<RepositoryState>,
    write_lock: Arc<Mutex<()>>,
    rx: Receiver<RepositoryStructureRefreshRequest>,
) {
    let mut pending = BTreeMap::<String, (String, Instant, BTreeSet<String>)>::new();
    loop {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(request) => {
                pending
                    .entry(request.repo_id)
                    .and_modify(|entry| {
                        entry.0 = request.reason.clone();
                        entry.1 = Instant::now();
                        entry.2.extend(request.paths.clone());
                    })
                    .or_insert((request.reason, Instant::now(), request.paths));
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }

        let now = Instant::now();
        let ready = pending
            .iter()
            .filter_map(|(repo_id, (reason, queued_at, paths))| {
                (now.duration_since(*queued_at).as_millis()
                    >= u128::from(STRUCTURE_REFRESH_DEBOUNCE_MS))
                .then_some((repo_id.clone(), reason.clone(), paths.clone()))
            })
            .collect::<Vec<_>>();

        for (repo_id, reason, paths) in ready {
            pending.remove(&repo_id);
            let Ok(_guard) = write_lock.lock() else {
                log_watcher_error(
                    "writeLockFailed",
                    "结构刷新工作线程获取写锁失败。",
                    serde_json::json!({ "repoId": repo_id }),
                );
                return;
            };
            if let Err(error) = repository_state.set_repository_structure_refreshing(&repo_id, true)
            {
                log_watcher_error(
                    "refreshStateSetFailed",
                    "标记资源库结构刷新状态失败。",
                    serde_json::json!({ "repoId": repo_id, "refreshing": true, "error": error }),
                );
            }
            let sync_result = repository_state.sync_repository_changed_paths(&repo_id, &paths);
            let indexed_at = match repository_state.repository_structure_indexed_at(&repo_id) {
                Ok(value) => value,
                Err(error) => {
                    log_watcher_error(
                        "indexedAtReadFailed",
                        "读取资源库结构索引时间失败。",
                        serde_json::json!({ "repoId": repo_id, "error": error }),
                    );
                    None
                }
            };
            if let Err(error) =
                repository_state.set_repository_structure_refreshing(&repo_id, false)
            {
                log_watcher_error(
                    "refreshStateSetFailed",
                    "标记资源库结构刷新状态失败。",
                    serde_json::json!({ "repoId": repo_id, "refreshing": false, "error": error }),
                );
            }
            match sync_result {
                Ok(result) => {
                    crate::app_log!(
                        "info",
                        "runtime.watcher",
                        "structureRefreshed",
                        "资源库结构刷新完成。",
                        serde_json::json!({
                            "repoId": repo_id,
                            "reason": reason,
                            "changedPathCount": paths.len(),
                            "scannedFiles": result.scanned_files,
                            "createdAssets": result.created_assets,
                            "updatedAssets": result.updated_assets,
                            "deletedAssets": result.deleted_assets,
                        })
                    );
                    repository_state.emit_repository_structure_updated(
                        RepositoryStructureUpdatedEvent {
                            repo_id,
                            reason,
                            indexed_at,
                        },
                    );
                }
                Err(error) => {
                    log_watcher_error(
                        "structureRefreshFailed",
                        "资源库结构刷新失败。",
                        serde_json::json!({
                            "repoId": repo_id,
                            "reason": reason,
                            "changedPathCount": paths.len(),
                            "error": error,
                        }),
                    );
                }
            }
        }
    }
}

fn normalize_path(path: &Path) -> String {
    let value = path.to_string_lossy().replace('\\', "/");
    #[cfg(windows)]
    {
        value.to_lowercase()
    }
    #[cfg(not(windows))]
    {
        value
    }
}

fn repository_event_path_is_inside(repo_root: &Path, path: &Path) -> bool {
    let root = normalize_path(repo_root).trim_end_matches('/').to_string();
    let path = normalize_path(path);
    path == root
        || path
            .strip_prefix(&root)
            .is_some_and(|remaining| remaining.starts_with('/'))
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

    #[test]
    fn repository_relative_event_path_returns_normalized_file_path() {
        let repo_root = unique_test_path("relative-file");
        let file_path = repo_root.join("Artist").join("track.mp3");

        let relative = repository_relative_event_path(&repo_root, &file_path);

        assert_eq!(relative.as_deref(), Some("Artist/track.mp3"));
    }

    #[test]
    fn repository_relative_event_path_skips_internal_metadata() {
        let repo_root = unique_test_path("relative-meta");
        let metadata_path = repo_root.join(".momo").join("repo.db");

        let relative = repository_relative_event_path(&repo_root, &metadata_path);

        assert_eq!(relative, None);
    }

    #[test]
    fn repository_event_path_matches_only_repository_boundary() {
        let repo_root = unique_test_path("boundary");
        let sibling_root = PathBuf::from(format!("{}-other", repo_root.to_string_lossy()));
        let inside_path = repo_root.join("folder").join("asset.png");
        let sibling_path = sibling_root.join("asset.png");

        assert!(repository_event_path_is_inside(&repo_root, &inside_path));
        assert!(!repository_event_path_is_inside(&repo_root, &sibling_path));
    }
}
