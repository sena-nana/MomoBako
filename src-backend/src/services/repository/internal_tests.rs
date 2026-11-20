//! 仓库服务内部测试：共享的测试工作区、插件包构造与仓库夹具，按主题拆到子模块。

use super::browser::mirror_netease_entries_to_assets;
use super::test_support::playback_test_lock;
use super::*;
use std::net::TcpListener;
use std::time::{SystemTime, UNIX_EPOCH};

mod entries;
mod lifecycle;
mod metadata;
mod netease_playlists;
mod plugins;
mod registry_storage;
mod search;
mod sync;
mod thumbnails;

const TEST_WEBDAV_PLUGIN_ID: &str = "momobako.webdav";

struct TestWorkspace {
    root: PathBuf,
}

impl TestWorkspace {
    fn new(name: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock must be after unix epoch")
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("momobako-{name}-{}-{unique}", std::process::id()));
        Self { root }
    }

    fn path(&self, child: &str) -> PathBuf {
        self.root.join(child)
    }
}

impl Drop for TestWorkspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn write_test_plugin_archive(path: &Path, plugin_id: &str) {
    write_test_plugin_archive_with_options(
        path,
        TestPluginArchiveOptions {
            plugin_id,
            ..TestPluginArchiveOptions::default()
        },
    );
}

fn write_test_v2_plugin_archive(path: &Path, plugin_id: &str, claimed_source: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("plugin archive parent should be created");
    }
    let root_dir = format!("{}-0.1.0", slugify_ascii_component(plugin_id));
    let manifest = test_plugin_manifest_json(
        plugin_id,
        "Sample Metadata",
        serde_json::json!({ "source": claimed_source }),
    );
    let envelope = serde_json::json!({
        "formatVersion": 2,
        "pluginId": plugin_id,
        "version": "0.1.0",
        "targetTriple": "any",
        "deployment": "manifest",
        "productManifest": "manifest.json",
        "artifacts": []
    });
    let file = File::create(path).expect("plugin archive should be created");
    let mut archive = zip::ZipWriter::new(file);
    for (name, value) in [
        ("manifest.json", manifest),
        ("momobako.package.json", envelope),
    ] {
        archive
            .start_file(
                format!("{root_dir}/{name}"),
                zip::write::SimpleFileOptions::default(),
            )
            .expect("plugin entry should start");
        archive
            .write_all(
                serde_json::to_string_pretty(&value)
                    .expect("plugin json should encode")
                    .as_bytes(),
            )
            .expect("plugin entry should write");
    }
    archive.finish().expect("plugin archive should finish");
}

fn seed_standard_test_plugins(service_root: &Path) {
    let runtime_root = runtime_plugins_dir(service_root);
    write_test_plugin_archive_with_manifest(
        &runtime_root.join("local-filesystem.momoplug"),
        test_plugin_manifest_json(
            LOCAL_FILESYSTEM_PLUGIN_ID,
            "Local Filesystem",
            serde_json::json!({
                "legacyPluginIds": [LEGACY_LOCAL_FILESYSTEM_PLUGIN_ID],
                "kind": "filesystem",
                "category": "source",
                "type": {
                    "layer": "source",
                    "kind": "filesystem"
                },
                "capabilities": ["browse", "read", "write", "watch", "sync", "localRootPath"],
                "sdk": "backend",
                "runtime": "native-dylib",
                "source": "system"
            }),
        ),
    );
    install_netease_source_test_plugin_archive(service_root);
    write_test_plugin_archive_with_manifest(
        &runtime_root.join("media-preview.momoplug"),
        test_plugin_manifest_json(
            "momobako.preview.media",
            "Media Preview",
            serde_json::json!({
                "kind": "preview",
                "category": "preview",
                "type": {
                    "layer": "library-kind",
                    "kind": "preview"
                },
                "capabilities": ["preview", "playlist", "media"],
                "sdk": "backend",
                "runtime": "manifest-only",
                "source": "builtin",
                "hooks": [
                    { "slot": "playlist", "action": "preview.media.enqueue", "label": "加入播放列表" }
                ]
            }),
        ),
    );
    write_test_plugin_archive_with_manifest(
        &runtime_root.join("library-audio.momoplug"),
        test_plugin_manifest_json(
            "momobako.library.audio",
            "Audio Library",
            serde_json::json!({
                "kind": "audio",
                "category": "library-kind",
                "type": {
                    "layer": "library-kind",
                    "kind": "audio"
                },
                "capabilities": ["library", "audio"],
                "sdk": "backend",
                "runtime": "manifest-only",
                "source": "builtin",
                "optional": ["momobako.parser.audio"]
            }),
        ),
    );
    write_test_plugin_archive_with_manifest(
        &runtime_root.join("parser-audio.momoplug"),
        test_plugin_manifest_json(
            "momobako.parser.audio",
            "Audio Parser",
            serde_json::json!({
                "kind": "parser",
                "category": "parser",
                "type": {
                    "layer": "parser",
                    "kind": "audio"
                },
                "capabilities": ["parse", "audio"],
                "sdk": "backend",
                "runtime": "manifest-only",
                "source": "builtin"
            }),
        ),
    );
    write_test_plugin_archive_with_manifest(
        &runtime_root.join("service-network-search.momoplug"),
        test_plugin_manifest_json(
            "momobako.service.network-search",
            "Network Search",
            serde_json::json!({
                "kind": "search",
                "category": "service",
                "type": {
                    "layer": "provider-service",
                    "kind": "search"
                },
                "capabilities": ["network", "search"],
                "sdk": "backend",
                "runtime": "manifest-only",
                "source": "builtin"
            }),
        ),
    );
}

fn install_netease_source_test_plugin_archive(service_root: &Path) {
    let runtime_root = runtime_plugins_dir(service_root);
    write_test_plugin_archive_with_manifest(
        &runtime_root.join("source-netease-cloud-music.momoplug"),
        test_plugin_manifest_json(
            NETEASE_CLOUD_MUSIC_PLUGIN_ID,
            "Netease Cloud Music Source",
            serde_json::json!({
                "kind": "netease-cloud-music",
                "category": "source",
                "type": {
                    "layer": "source",
                    "kind": "netease-cloud-music"
                },
                "capabilities": ["browse", "read", "sync", "virtual-entries", "login"],
                "sdk": "backend",
                "runtime": "manifest-only",
                "source": "builtin",
                "contributes": {
                    "source": {
                        "operations": ["list", "read", "sync"],
                        "dangerousOperations": [],
                        "metadataMirrorKeys": [
                            "songId",
                            "songName",
                            "artists",
                            "albumName",
                            "coverUrl",
                            "durationMs",
                            "playlistId",
                            "playlistName",
                            "playlistCategory",
                            "provider",
                            "accountId"
                        ]
                    }
                }
            }),
        ),
    );
}

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
    if let (Some(base), Some(extra)) = (manifest.as_object_mut(), overrides.as_object()) {
        for (key, value) in extra {
            base.insert(key.clone(), value.clone());
        }
    }
    manifest
}

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
        .expect("manifest entry should start");
    archive
        .write_all(
            serde_json::to_string_pretty(&manifest)
                .expect("manifest should encode")
                .as_bytes(),
        )
        .expect("manifest should write");
    archive.finish().expect("plugin archive should finish");
}

#[derive(Clone)]
struct TestPluginArchiveOptions<'a> {
    plugin_id: &'a str,
    name: &'a str,
    source: &'a str,
    runtime: &'a str,
    sdk: &'a str,
    kind: &'a str,
    plugin_type_layer: &'a str,
    plugin_type_kind: &'a str,
    entry: serde_json::Value,
    extra_files: Vec<(String, String)>,
}

impl Default for TestPluginArchiveOptions<'_> {
    fn default() -> Self {
        Self {
            plugin_id: "user.sample-metadata",
            name: "Sample Metadata",
            source: "user",
            runtime: "manifest-only",
            sdk: "backend",
            kind: "metadata",
            plugin_type_layer: "provider-service",
            plugin_type_kind: "metadata",
            entry: serde_json::json!({}),
            extra_files: Vec::new(),
        }
    }
}

fn write_test_plugin_archive_with_options(path: &Path, options: TestPluginArchiveOptions<'_>) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("plugin archive parent should be created");
    }
    let file = File::create(path).expect("plugin archive should be created");
    let mut archive = zip::ZipWriter::new(file);
    let root_dir = format!("{}-0.1.0", slugify_ascii_component(options.plugin_id));
    let manifest_path = format!("{root_dir}/manifest.json");
    archive
        .start_file(manifest_path, zip::write::SimpleFileOptions::default())
        .expect("manifest entry should start");
    archive
        .write_all(
            serde_json::to_string_pretty(&serde_json::json!({
                "pluginId": options.plugin_id,
                "legacyPluginIds": [],
                "name": options.name,
                "version": "0.1.0",
                "type": {
                    "layer": options.plugin_type_layer,
                    "kind": options.plugin_type_kind
                },
                "kind": options.kind,
                "description": "Test plugin installed from archive.",
                "capabilities": [options.kind],
                "enabled": true,
                "sdk": options.sdk,
                "entry": options.entry,
                "source": options.source,
                "runtime": options.runtime,
                "permissions": [],
                "compat": {
                    "sdkVersion": "1",
                    "legacyPluginIds": []
                },
                "status": "ready"
            }))
            .expect("manifest should encode")
            .as_bytes(),
        )
        .expect("manifest should write");
    let envelope = serde_json::json!({
        "formatVersion": 2,
        "pluginId": options.plugin_id,
        "version": "0.1.0",
        "targetTriple": "any",
        "deployment": "manifest",
        "productManifest": "manifest.json",
        "artifacts": []
    });
    archive
        .start_file(
            format!("{root_dir}/momobako.package.json"),
            zip::write::SimpleFileOptions::default(),
        )
        .expect("package envelope entry should start");
    archive
        .write_all(
            serde_json::to_string_pretty(&envelope)
                .expect("package envelope should encode")
                .as_bytes(),
        )
        .expect("package envelope should write");
    for (relative_path, content) in options.extra_files {
        archive
            .start_file(
                format!("{root_dir}/{relative_path}"),
                zip::write::SimpleFileOptions::default(),
            )
            .expect("extra entry should start");
        archive
            .write_all(content.as_bytes())
            .expect("extra entry should write");
    }
    archive.finish().expect("plugin archive should finish");
}

fn write_test_local_filesystem_plugin_archive(
    plugin_root: &Path,
    dependency_overrides: serde_json::Value,
) {
    let mut manifest = test_plugin_manifest_json(
        LOCAL_FILESYSTEM_PLUGIN_ID,
        "Local Filesystem",
        serde_json::json!({
            "legacyPluginIds": [LEGACY_LOCAL_FILESYSTEM_PLUGIN_ID],
            "kind": "filesystem",
            "category": "source",
            "type": {
                "layer": "source",
                "kind": "filesystem"
            },
            "capabilities": ["browse", "read", "write", "watch", "sync", "localRootPath"],
            "sdk": "backend",
            "runtime": "native-dylib",
            "source": "system"
        }),
    );
    if let (Some(base), Some(extra)) = (manifest.as_object_mut(), dependency_overrides.as_object())
    {
        for (key, value) in extra {
            base.insert(key.clone(), value.clone());
        }
    }
    write_test_plugin_archive_with_manifest(
        &plugin_root.join("local-filesystem.momoplug"),
        manifest,
    );
    enable_local_filesystem_adapter();
}

fn test_list_files_plugin_call(plugin_id: &str, repo_root: PathBuf) -> PluginCallRequest {
    PluginCallRequest {
        plugin_id: plugin_id.to_string(),
        method: "filesystem.listFiles".to_string(),
        repository_id: None,
        payload: serde_json::json!({
            "repoRoot": repo_root,
            "config": {}
        }),
    }
}

fn write_test_plugin_archive_without_root_dir(path: &Path, plugin_id: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("plugin archive parent should be created");
    }
    let file = File::create(path).expect("plugin archive should be created");
    let mut archive = zip::ZipWriter::new(file);
    archive
        .start_file("manifest.json", zip::write::SimpleFileOptions::default())
        .expect("manifest entry should start");
    archive
        .write_all(
            serde_json::to_string_pretty(&serde_json::json!({
                "pluginId": plugin_id,
                "legacyPluginIds": [],
                "name": "Rootless Plugin",
                "version": "0.1.0",
                "type": {
                    "layer": "provider-service",
                    "kind": "metadata"
                },
                "kind": "metadata",
                "description": "Invalid root-level manifest package.",
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
            }))
            .expect("manifest should encode")
            .as_bytes(),
        )
        .expect("manifest should write");
    archive.finish().expect("plugin archive should finish");
}

const LONG_RELATIVE_PATH: &str = "CubismSdkForNative-5-r.5/Samples/OpenGL/Demo/proj.harmonyos.cmake/Full/entry/src/main/resources/base/media/startIcon.png";

fn unique_temp_dir(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("momobako-{label}-{}-{unique}", std::process::id()))
}

fn create_test_state(label: &str) -> (RepositoryState, PathBuf, PathBuf, PathBuf) {
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

fn create_repository_for_path(state: &RepositoryState, repo_root: &Path) -> String {
    install_local_filesystem_test_plugin(state);
    let response = state
        .create_repository(RepositoryMutationRequest {
            repo_id: None,
            name: "Test Repo".to_string(),
            path: repo_root.to_string_lossy().to_string(),
            backend_plugin_id: None,
            backend_config: None,
            skip_initial_sync: false,
        })
        .expect("repository should be created");
    response.repository.repo_id
}

fn install_local_filesystem_test_plugin(state: &RepositoryState) {
    state
        .ensure_initialized()
        .expect("repository state should initialize");
    install_local_filesystem_test_plugin_archive(&state.root);
}

fn create_repository_without_initial_sync(state: &RepositoryState, repo_root: &Path) -> String {
    let repo_id = format!(
        "repo-{}",
        slugify_repo_id("test", &repo_root.to_string_lossy())
    );
    state
        .ensure_initialized()
        .expect("repository state should initialize");
    install_local_filesystem_test_plugin_archive(&state.root);
    install_netease_source_test_plugin_archive(&state.root);
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

fn changed_paths(paths: &[&str]) -> std::collections::BTreeSet<String> {
    paths.iter().map(|path| path.to_string()).collect()
}

fn asset_status_for_path(state: &RepositoryState, repo_id: &str, path: &str) -> Option<String> {
    let repo = state
        .load_repository_record(repo_id)
        .expect("repository record should load");
    let connection = state
        .open_repository_connection(
            &repo.summary.repo_id,
            &repo.summary.path,
            &repo.backend_record,
        )
        .expect("repository connection should open");
    connection
        .query_row(
            "SELECT status FROM assets WHERE repo_id = ?1 AND path = ?2",
            params![repo_id, path],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .expect("asset status should query")
}

fn asset_size_for_path(state: &RepositoryState, repo_id: &str, path: &str) -> Option<i64> {
    let repo = state
        .load_repository_record(repo_id)
        .expect("repository record should load");
    let connection = state
        .open_repository_connection(
            &repo.summary.repo_id,
            &repo.summary.path,
            &repo.backend_record,
        )
        .expect("repository connection should open");
    connection
        .query_row(
            "SELECT size_bytes FROM assets WHERE repo_id = ?1 AND path = ?2",
            params![repo_id, path],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .expect("asset size should query")
}

fn create_netease_repository_without_initial_sync(
    state: &RepositoryState,
    repo_root: &Path,
    backend_config: serde_json::Value,
) -> String {
    let repo_id = format!(
        "netease-{}",
        slugify_repo_id("netease", &repo_root.to_string_lossy())
    );
    state
        .ensure_initialized()
        .expect("repository state should initialize");
    install_local_filesystem_test_plugin_archive(&state.root);
    install_netease_source_test_plugin_archive(&state.root);
    let repo_path = repo_root.to_string_lossy().to_string();
    let backend = RepositoryBackendRecord {
        plugin_id: NETEASE_CLOUD_MUSIC_PLUGIN_ID.to_string(),
        config: backend_config.clone(),
    };
    let seed = RepositorySeed {
        repo_id: &repo_id,
        name: "Netease Test Repo",
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
                "Netease Test Repo",
                &repo_path,
                NETEASE_CLOUD_MUSIC_PLUGIN_ID,
                backend_config.to_string(),
                now_rfc3339()
            ],
        )
        .expect("repository should be registered");
    repo_id
}

fn write_test_image(path: &Path) {
    let image = image::RgbImage::from_pixel(2, 2, image::Rgb([120, 120, 120]));
    image.save(path).expect("test image should be saved");
}

fn serve_test_http_body(body: impl AsRef<[u8]> + Send + 'static) -> String {
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

fn write_test_palette_image(path: &Path) {
    let mut image = image::RgbImage::new(100, 10);
    for x in 0..100 {
        let color = if x < 60 {
            image::Rgb([210, 40, 30])
        } else if x < 85 {
            image::Rgb([40, 180, 90])
        } else {
            image::Rgb([20, 80, 200])
        };
        for y in 0..10 {
            image.put_pixel(x, y, color);
        }
    }
    image
        .save(path)
        .expect("test palette image should be saved");
}

fn metadata_for_asset_path(
    state: &RepositoryState,
    repo_id: &str,
    path: &str,
) -> BTreeMap<String, serde_json::Value> {
    let snapshot = state
        .load_snapshot(repo_id)
        .expect("snapshot should load after sync");
    let asset_id = snapshot
        .assets
        .iter()
        .find(|asset| asset.path == path)
        .expect("asset should exist")
        .asset_id
        .clone();
    state
        .load_asset_detail(repo_id, &asset_id)
        .expect("asset detail should load")
        .metadata
        .into_iter()
        .map(|entry| (entry.key, entry.value))
        .collect()
}

fn asset_id_for_test_path(state: &RepositoryState, repo_id: &str, path: &str) -> String {
    let snapshot = state
        .load_snapshot(repo_id)
        .expect("snapshot should load after sync");
    snapshot
        .assets
        .iter()
        .find(|asset| asset.path == path)
        .expect("asset should exist")
        .asset_id
        .clone()
}

fn count_files(path: &Path) -> usize {
    if !path.exists() {
        return 0;
    }
    fs::read_dir(path)
        .expect("path should be readable")
        .map(|entry| {
            let path = entry.expect("dir entry should be readable").path();
            if path.is_dir() {
                count_files(&path)
            } else {
                1
            }
        })
        .sum()
}
