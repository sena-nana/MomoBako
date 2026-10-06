//! Test-only hooks and fixtures for repository backend seams and split-out tests.

#[cfg(test)]
use super::*;
#[cfg(test)]
use std::{
    io::{Read, Write},
    net::TcpListener,
    path::Path,
    sync::{Mutex, MutexGuard, OnceLock},
    thread,
    time::SystemTime,
};

#[cfg(test)]
type TestDownloaderPlaybackHook = fn(serde_json::Value) -> Result<serde_json::Value, String>;
#[cfg(test)]
type TestDownloaderTrackPackageHook = fn(serde_json::Value) -> Result<serde_json::Value, String>;
#[cfg(test)]
type TestBackendStatEntryHook =
    fn(&RepositoryRecord, &Path, &str) -> Option<Result<FileSystemEntry, String>>;

#[cfg(test)]
static TEST_DOWNLOADER_PLAYBACK_HOOK: OnceLock<Mutex<Option<TestDownloaderPlaybackHook>>> =
    OnceLock::new();
#[cfg(test)]
static TEST_DOWNLOADER_TRACK_PACKAGE_HOOK: OnceLock<Mutex<Option<TestDownloaderTrackPackageHook>>> =
    OnceLock::new();
#[cfg(test)]
static TEST_BACKEND_STAT_ENTRY_HOOK: OnceLock<Mutex<Option<TestBackendStatEntryHook>>> =
    OnceLock::new();
#[cfg(test)]
static PLAYBACK_TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[cfg(test)]
pub(crate) fn downloader_playback_hook() -> Result<Option<TestDownloaderPlaybackHook>, String> {
    TEST_DOWNLOADER_PLAYBACK_HOOK
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map(|guard| guard.as_ref().copied())
        .map_err(|_| "test downloader playback hook lock poisoned".to_string())
}

#[cfg(test)]
pub(crate) fn downloader_track_package_hook(
) -> Result<Option<TestDownloaderTrackPackageHook>, String> {
    TEST_DOWNLOADER_TRACK_PACKAGE_HOOK
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map(|guard| guard.as_ref().copied())
        .map_err(|_| "test downloader track package hook lock poisoned".to_string())
}

#[cfg(test)]
pub(super) fn backend_stat_entry_hook(
    repo: &RepositoryRecord,
    repo_root: &Path,
    entry_path: &str,
) -> Result<Option<Result<FileSystemEntry, String>>, String> {
    TEST_BACKEND_STAT_ENTRY_HOOK
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map(|guard| {
            guard
                .as_ref()
                .and_then(|hook| hook(repo, repo_root, entry_path))
        })
        .map_err(|_| "test backend stat entry hook lock poisoned".to_string())
}

#[cfg(test)]
pub(crate) fn set_test_downloader_playback_hook(hook: Option<TestDownloaderPlaybackHook>) {
    *TEST_DOWNLOADER_PLAYBACK_HOOK
        .get_or_init(|| Mutex::new(None))
        .lock()
        .expect("test downloader playback hook lock should succeed") = hook;
}

#[cfg(test)]
pub(crate) fn set_test_downloader_track_package_hook(hook: Option<TestDownloaderTrackPackageHook>) {
    *TEST_DOWNLOADER_TRACK_PACKAGE_HOOK
        .get_or_init(|| Mutex::new(None))
        .lock()
        .expect("test downloader track package hook lock should succeed") = hook;
}

#[cfg(test)]
pub(super) fn set_test_backend_stat_entry_hook(hook: Option<TestBackendStatEntryHook>) {
    *TEST_BACKEND_STAT_ENTRY_HOOK
        .get_or_init(|| Mutex::new(None))
        .lock()
        .expect("test backend stat entry hook lock should succeed") = hook;
}

#[cfg(test)]
pub(crate) fn playback_test_lock() -> MutexGuard<'static, ()> {
    PLAYBACK_TEST_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .expect("playback test lock should succeed")
}

#[cfg(test)]
pub(crate) fn create_test_state(label: &str) -> (RepositoryState, PathBuf, PathBuf, PathBuf) {
    let root = unique_temp_dir(label);
    let repo_root = root.join("repo");
    fs::create_dir_all(&repo_root).expect("repo root should be created");
    let root = canonicalize_local_path(&root).expect("test root should canonicalize");
    let repo_root = canonicalize_local_path(&repo_root).expect("repo root should canonicalize");
    let state_root = root.join("state");
    let thumbnail_root = repo_root.join(REPO_META_DIR).join("thumbnails");
    (
        RepositoryState::from_root(state_root),
        root,
        repo_root,
        thumbnail_root,
    )
}

#[cfg(test)]
pub(crate) fn create_repository_without_initial_sync(
    state: &RepositoryState,
    repo_root: &Path,
) -> String {
    let repo_id = format!(
        "repo-{}",
        slugify_repo_id("test", &repo_root.to_string_lossy())
    );
    state
        .ensure_initialized()
        .expect("repository state should initialize");
    install_local_filesystem_test_plugin_archive(&state.root);
    let repo_path = repo_root.to_string_lossy().to_string();
    let backend = RepositoryBackendRecord {
        plugin_id: LOCAL_FILESYSTEM_PLUGIN_ID.to_string(),
        config: serde_json::json!({}),
    };
    let seed = RepositorySeed {
        repo_id: &repo_id,
        name: "Test Repo",
        root_path: "",
        status: "ready",
        assets: &[],
    };
    initialize_repository_directory(&state.root, repo_root, &seed, &backend)
        .expect("repository files should be prepared");
    let registry = Connection::open(&state.registry_path).expect("registry should open");
    registry
        .execute(
            r#"
            INSERT OR REPLACE INTO repositories (
              repo_id, name, path, backend_plugin_id, backend_config_json, status, created_at, updated_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, 'ready', ?6, ?6)
            "#,
            params![
                &repo_id,
                "Test Repo",
                &repo_path,
                LOCAL_FILESYSTEM_PLUGIN_ID,
                "{}",
                now_rfc3339()
            ],
        )
        .expect("repository should be registered");
    repo_id
}

#[cfg(test)]
pub(crate) fn create_local_repository_record_for_external_tests(
    state: &RepositoryState,
    repo_root: &Path,
) -> String {
    let repo_id = format!(
        "repo-{}",
        slugify_repo_id("external", &repo_root.to_string_lossy())
    );
    state
        .ensure_initialized()
        .expect("repository state should initialize");
    let runtime_plugin_root = plugin::runtime_plugins_dir(&state.root);
    write_test_plugin_archive_with_manifest(
        &runtime_plugin_root.join("local-filesystem.momoplug"),
        test_plugin_manifest_json(
            LOCAL_FILESYSTEM_PLUGIN_ID,
            "Local Filesystem",
            serde_json::json!({
                "kind": "filesystem",
                "category": "source",
                "type": {
                    "layer": "source",
                    "kind": "filesystem"
                },
                "capabilities": ["browse", "read", "write", "watch", "sync", "localRootPath"],
                "runtime": "manifest-only",
                "source": "system",
                "contributes": {
                    "settings": {
                        "schemaVersion": 1,
                        "settingsPage": {
                            "label": "本地文件系统",
                            "description": "配置本地资源库的文件检索方式。",
                            "order": 10
                        },
                        "fields": [
                            {
                                "key": "fileSearchMode",
                                "label": "文件检索方式",
                                "type": "select",
                                "description": "NTFS 与 Everything 不可用时会自动回退到现有扫描。",
                                "default": "recursive",
                                "options": [
                                    { "label": "现有扫描", "value": "recursive" },
                                    { "label": "NTFS 索引", "value": "ntfs" },
                                    { "label": "Everything 索引", "value": "everything" }
                                ]
                            }
                        ]
                    }
                }
            }),
        ),
    );
    let metadata_dir = repo_root.join(REPO_META_DIR);
    fs::create_dir_all(&metadata_dir).expect("metadata dir should be created");
    let now = now_rfc3339();
    let metadata = RepositoryMetadataFile {
        repo_id: repo_id.clone(),
        name: "External Test Repo".to_string(),
        root_path: repo_root.to_string_lossy().to_string(),
        backend_plugin_id: LOCAL_FILESYSTEM_PLUGIN_ID.to_string(),
        backend_config: serde_json::json!({}),
        created_at: now.clone(),
        schema_version: REPO_SCHEMA_VERSION,
    };
    fs::write(
        metadata_dir.join(REPO_METADATA_FILE_NAME),
        serde_json::to_string_pretty(&metadata).expect("metadata should encode"),
    )
    .expect("metadata should be written");
    let connection =
        Connection::open(metadata_dir.join(REPO_DB_FILE_NAME)).expect("repository db should open");
    migrate_repository_schema(&connection).expect("repository schema should initialize");
    seed_repository_data(
        &connection,
        &RepositorySeed {
            repo_id: &repo_id,
            name: "External Test Repo",
            root_path: "",
            status: "ready",
            assets: &[],
        },
        &now,
    )
    .expect("repository data should seed");

    let registry = Connection::open(&state.registry_path).expect("registry should open");
    registry
        .execute(
            r#"
            INSERT OR REPLACE INTO repositories (
              repo_id, name, path, backend_plugin_id, backend_config_json, status, created_at, updated_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, 'ready', ?6, ?6)
            "#,
            params![
                &repo_id,
                "External Test Repo",
                repo_root.to_string_lossy().to_string(),
                LOCAL_FILESYSTEM_PLUGIN_ID,
                "{}",
                now
            ],
        )
        .expect("repository should be registered");
    repo_id
}

#[cfg(test)]
pub(crate) fn serve_test_http_body(body: impl AsRef<[u8]> + Send + 'static) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("test HTTP server should bind");
    let addr = listener
        .local_addr()
        .expect("test HTTP server address should resolve");
    thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request);
            let body = body.as_ref();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.write_all(body);
        }
    });
    format!("http://{addr}/asset.txt")
}

#[cfg(test)]
fn unique_temp_dir(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("momobako-{label}-{}-{unique}", std::process::id()))
}

#[cfg(test)]
fn test_plugin_manifest_json(
    plugin_id: &str,
    name: &str,
    overrides: serde_json::Value,
) -> serde_json::Value {
    let mut manifest = serde_json::json!({
        "pluginId": plugin_id,
        "legacyPluginIds": [],
        "name": name,
        "version": "0.1.0",
        "kind": "metadata",
        "description": "Test plugin.",
        "capabilities": ["metadata"],
        "enabled": true,
        "sdk": "backend",
        "entry": {},
        "source": "user",
        "runtime": "manifest-only",
        "permissions": [],
        "compat": {
            "sdkVersion": "1",
            "legacyPluginIds": []
        },
        "status": "ready"
    });
    if let (Some(target), Some(extra_fields)) = (manifest.as_object_mut(), overrides.as_object()) {
        for (key, value) in extra_fields {
            target.insert(key.clone(), value.clone());
        }
    }
    manifest
}

#[cfg(test)]
fn write_test_plugin_archive_with_manifest(path: &Path, manifest: serde_json::Value) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("plugin archive parent should be created");
    }
    let plugin_id = manifest
        .get("pluginId")
        .and_then(|value| value.as_str())
        .unwrap_or("user.sample-metadata");
    let file = File::create(path).expect("plugin archive should be created");
    let mut archive = zip::ZipWriter::new(file);
    let root_dir = format!("{plugin_id}-0.1.0");
    archive
        .start_file(
            format!("{root_dir}/manifest.json"),
            zip::write::SimpleFileOptions::default(),
        )
        .expect("plugin manifest entry should be created");
    archive
        .write_all(
            serde_json::to_vec_pretty(&manifest)
                .expect("plugin manifest should encode")
                .as_slice(),
        )
        .expect("plugin manifest should write");
    archive.finish().expect("plugin archive should finish");
}

#[cfg(test)]
pub(crate) fn insert_virtual_asset(
    state: &RepositoryState,
    repo_id: &str,
    path: &str,
    filename: &str,
    provider_item_id: &str,
    source_payload: serde_json::Value,
) {
    let connection = Connection::open(
        Path::new(&path_from_repo(state, repo_id))
            .join(REPO_META_DIR)
            .join(REPO_DB_FILE_NAME),
    )
    .expect("repository db should open");
    connection
        .execute(
            r#"
            INSERT INTO assets (
              asset_id, repo_id, path, filename, extension, size_bytes, created_at, modified_at,
              hash, status, version, updated_at, thumbnail_path, is_virtual, provider_id,
              provider_item_id, source_payload_json, local_absolute_path
            )
            VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, ?6, NULL, 'synced', 1, ?6, NULL, 1, ?7, ?8, ?9, NULL)
            "#,
            params![
                asset_id_for_path(repo_id, path),
                repo_id,
                path,
                filename,
                "mp3",
                "2026-06-14T00:00:00Z",
                "netease-cloud-music",
                provider_item_id,
                source_payload.to_string(),
            ],
        )
        .expect("virtual asset should be inserted");
}

#[cfg(test)]
pub(crate) fn insert_asset_metadata_number(
    state: &RepositoryState,
    repo_id: &str,
    path: &str,
    key: &str,
    value_json: &str,
) {
    let connection = Connection::open(
        Path::new(&path_from_repo(state, repo_id))
            .join(REPO_META_DIR)
            .join(REPO_DB_FILE_NAME),
    )
    .expect("repository db should open");
    connection
        .execute(
            r#"
            INSERT INTO metadata (
              asset_id, key, value_type, value_json, version, updated_at
            )
            VALUES (?1, ?2, 'number', ?3, 1, ?4)
            "#,
            params![
                asset_id_for_path(repo_id, path),
                key,
                value_json,
                "2026-06-14T00:00:00Z"
            ],
        )
        .expect("metadata should be inserted");
}

#[cfg(test)]
pub(crate) fn update_repository_backend_config(
    state: &RepositoryState,
    repo_id: &str,
    backend_config: serde_json::Value,
) {
    let registry = Connection::open(&state.registry_path).expect("registry should open");
    registry
        .execute(
            "UPDATE repositories SET backend_config_json = ?2 WHERE repo_id = ?1",
            params![repo_id, backend_config.to_string()],
        )
        .expect("repository backend config should update");
}

#[cfg(test)]
fn path_from_repo(state: &RepositoryState, repo_id: &str) -> String {
    let registry = Connection::open(&state.registry_path).expect("registry should open");
    registry
        .query_row(
            "SELECT path FROM repositories WHERE repo_id = ?1",
            [repo_id],
            |row| row.get::<_, String>(0),
        )
        .expect("repository path should load")
}
