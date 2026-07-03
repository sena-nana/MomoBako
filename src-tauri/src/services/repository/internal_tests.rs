//! Legacy repository service tests kept near the service internals while migration continues.

use super::*;

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::sync::MutexGuard;
    use std::time::{SystemTime, UNIX_EPOCH};

    const TEST_WEBDAV_PLUGIN_ID: &str = "momobako.webdav";
    static PLAYBACK_TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    struct TestWorkspace {
        root: PathBuf,
    }

    impl TestWorkspace {
        fn new(name: &str) -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock must be after unix epoch")
                .as_nanos();
            let root = std::env::temp_dir()
                .join(format!("momobako-{name}-{}-{unique}", std::process::id()));
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

    fn playback_test_lock() -> MutexGuard<'static, ()> {
        PLAYBACK_TEST_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .expect("playback test lock should succeed")
    }

    #[test]
    fn create_local_repository_creates_metadata_storage_dirs() {
        let workspace = TestWorkspace::new("local-repository-create");
        let service_root = workspace.path("service");
        let repo_root = workspace.path("repo");
        install_local_filesystem_test_plugin_archive(&service_root);
        let state = RepositoryState::from_root(service_root);

        state
            .create_repository(RepositoryMutationRequest {
                repo_id: Some("repo-test".to_string()),
                name: "测试资源库".to_string(),
                path: repo_root.to_string_lossy().to_string(),
                backend_plugin_id: Some(LOCAL_FILESYSTEM_PLUGIN_ID.to_string()),
                backend_config: None,
                skip_initial_sync: false,
            })
            .expect("local repository should be created");

        let metadata_dir = repo_root.join(REPO_META_DIR);
        assert!(metadata_dir.is_dir());
        assert!(metadata_dir.join(REPO_METADATA_FILE_NAME).is_file());
        assert!(metadata_dir.join(REPO_DB_FILE_NAME).is_file());
        for subdir in ["cache", "thumbnails", "logs", "indexes"] {
            assert!(metadata_dir.join(subdir).is_dir());
        }
    }

    #[test]
    fn create_repository_can_skip_initial_sync() {
        let workspace = TestWorkspace::new("repository-create-skip-sync");
        let service_root = workspace.path("service");
        let repo_root = workspace.path("repo");
        install_local_filesystem_test_plugin_archive(&service_root);
        fs::create_dir_all(&repo_root).expect("repository root should exist");
        fs::write(repo_root.join("track.mp3"), b"demo").expect("test file should be written");
        let state = RepositoryState::from_root(service_root);

        let repo_id = state
            .create_repository(RepositoryMutationRequest {
                repo_id: Some("repo-skip-sync".to_string()),
                name: "Skip Sync Repo".to_string(),
                path: repo_root.to_string_lossy().to_string(),
                backend_plugin_id: Some(LOCAL_FILESYSTEM_PLUGIN_ID.to_string()),
                backend_config: None,
                skip_initial_sync: true,
            })
            .expect("repository should be created without inline sync")
            .repository
            .repo_id;

        let snapshot_before_sync = state
            .load_snapshot(&repo_id)
            .expect("snapshot should load before sync");
        assert!(snapshot_before_sync.assets.is_empty());

        state
            .sync_repository(SyncRequest {
                repo_id: repo_id.clone(),
            })
            .expect("repository should sync when triggered later");

        let snapshot_after_sync = state
            .load_snapshot(&repo_id)
            .expect("snapshot should load after sync");
        assert_eq!(snapshot_after_sync.assets.len(), 1);
        assert_eq!(snapshot_after_sync.assets[0].path, "track.mp3");
    }

    #[test]
    fn load_snapshot_rebuilds_missing_repository_storage() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("snapshot-rebuild-meta");
        fs::write(repo_root.join("track.mp3"), b"demo").expect("test file should be written");
        let repo_id = create_repository_for_path(&state, &repo_root);

        fs::remove_dir_all(repo_root.join(REPO_META_DIR)).expect("metadata dir should be removed");

        let snapshot = state
            .load_snapshot(&repo_id)
            .expect("snapshot should rebuild missing repository storage");

        assert_eq!(snapshot.assets.len(), 1);
        assert_eq!(snapshot.assets[0].path, "track.mp3");
        assert!(repo_root
            .join(REPO_META_DIR)
            .join(REPO_METADATA_FILE_NAME)
            .is_file());
        assert!(repo_root
            .join(REPO_META_DIR)
            .join(REPO_DB_FILE_NAME)
            .is_file());

        fs::remove_dir_all(root).expect("test workspace should be removed");
    }

    #[test]
    fn load_snapshot_recovers_from_corrupted_repository_database() {
        let (state, root, repo_root, _thumbnail_root) =
            create_test_state("snapshot-rebuild-corrupted-db");
        fs::write(repo_root.join("track.mp3"), b"demo").expect("test file should be written");
        let repo_id = create_repository_for_path(&state, &repo_root);

        fs::write(
            repo_root.join(REPO_META_DIR).join(REPO_DB_FILE_NAME),
            b"broken",
        )
        .expect("database file should be corrupted");

        let snapshot = state
            .load_snapshot(&repo_id)
            .expect("snapshot should recover from corrupted database");

        assert_eq!(snapshot.assets.len(), 1);
        assert_eq!(snapshot.assets[0].path, "track.mp3");

        fs::remove_dir_all(root).expect("test workspace should be removed");
    }

    #[test]
    fn repository_tree_rebuilds_directory_cache_after_storage_loss_is_observed_by_listing() {
        let (state, root, repo_root, _thumbnail_root) =
            create_test_state("repository-tree-rebuild-after-listing");
        fs::create_dir_all(repo_root.join("Campaigns/Summer"))
            .expect("summer directory should be created");
        fs::write(repo_root.join("Campaigns/brief.txt"), "brief")
            .expect("campaign file should be written");
        fs::write(repo_root.join("Campaigns/Summer/cover.psd"), "cover")
            .expect("summer cover should be written");
        let repo_id = create_repository_for_path(&state, &repo_root);

        fs::remove_dir_all(repo_root.join(REPO_META_DIR)).expect("metadata dir should be removed");

        let repositories = state
            .list_repositories()
            .expect("repository summaries should load");
        let rebuilt_summary = repositories
            .iter()
            .find(|repository| repository.repo_id == repo_id)
            .expect("repository summary should exist");
        assert_eq!(rebuilt_summary.asset_count, 0);
        assert!(repo_root.join(REPO_META_DIR).join(REPO_DB_FILE_NAME).exists());

        let snapshot = state
            .load_repository_tree(&repo_id)
            .expect("repository tree should rebuild after storage loss");
        let campaigns = snapshot
            .tree
            .iter()
            .find(|node| node.path == "Campaigns")
            .expect("campaigns node should exist");
        let summer = campaigns
            .children
            .iter()
            .find(|node| node.path == "Campaigns/Summer")
            .expect("summer node should exist");

        assert_eq!(campaigns.file_count, 1);
        assert_eq!(summer.file_count, 1);

        let repo = state
            .load_repository_record(&repo_id)
            .expect("repository record should load");
        let connection = state
            .open_repository_connection(
                &repo.summary.repo_id,
                &repo.summary.path,
                &repo.backend_record,
            )
            .expect("repository connection should open");
        let directory_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM directories WHERE repo_id = ?1",
                params![&repo_id],
                |row| row.get(0),
            )
            .expect("directory count should query");

        assert!(directory_count >= 2);
        fs::remove_dir_all(root).expect("test workspace should be removed");
    }

    #[test]
    fn find_existing_repository_for_backend_matches_netease_account_id() {
        let workspace = TestWorkspace::new("netease-repository-dedupe");
        let service_root = workspace.path("service");
        install_local_filesystem_test_plugin_archive(&service_root);
        let state = RepositoryState::from_root(service_root.clone());
        state
            .ensure_initialized()
            .expect("state should initialize registry");
        let registry =
            Connection::open(service_root.join(REGISTRY_FILE_NAME)).expect("registry should open");
        let backend = RepositoryBackendRecord {
            plugin_id: NETEASE_CLOUD_MUSIC_PLUGIN_ID.to_string(),
            config: serde_json::json!({
                "accountId": "123",
                "cookie": "MUSIC_U=test"
            }),
        };
        let seed = RepositorySeed {
            repo_id: "netease-one",
            name: "网易云 A",
            root_path: "",
            status: "ready",
            assets: &[],
        };
        upsert_registry_entry(
            &registry,
            Path::new("netease-cloud-music://account/123"),
            &seed,
            &backend,
        )
        .expect("registry entry should be stored");

        let existing = state
            .find_existing_repository_for_backend(&RepositoryBackendRecord {
                plugin_id: NETEASE_CLOUD_MUSIC_PLUGIN_ID.to_string(),
                config: serde_json::json!({
                    "accountId": "123",
                    "cookie": "MUSIC_U=other"
                }),
            })
            .expect("lookup should succeed")
            .expect("existing repository should be found");

        assert_eq!(existing.repo_id, "netease-one");
    }

    #[test]
    fn find_existing_repository_for_backend_matches_numeric_netease_account_id() {
        let workspace = TestWorkspace::new("netease-repository-dedupe-numeric");
        let service_root = workspace.path("service");
        install_local_filesystem_test_plugin_archive(&service_root);
        let state = RepositoryState::from_root(service_root.clone());
        state
            .ensure_initialized()
            .expect("state should initialize registry");
        let registry =
            Connection::open(service_root.join(REGISTRY_FILE_NAME)).expect("registry should open");
        let backend = RepositoryBackendRecord {
            plugin_id: NETEASE_CLOUD_MUSIC_PLUGIN_ID.to_string(),
            config: serde_json::json!({
                "accountId": 123,
                "cookie": "MUSIC_U=test"
            }),
        };
        let seed = RepositorySeed {
            repo_id: "netease-one",
            name: "网易云 A",
            root_path: "",
            status: "ready",
            assets: &[],
        };
        upsert_registry_entry(
            &registry,
            Path::new("netease-cloud-music://account/123"),
            &seed,
            &backend,
        )
        .expect("registry entry should be stored");

        let existing = state
            .find_existing_repository_for_backend(&RepositoryBackendRecord {
                plugin_id: NETEASE_CLOUD_MUSIC_PLUGIN_ID.to_string(),
                config: serde_json::json!({
                    "accountId": "123",
                    "cookie": "MUSIC_U=other"
                }),
            })
            .expect("lookup should succeed")
            .expect("existing repository should be found");

        assert_eq!(existing.repo_id, "netease-one");
    }

    #[test]
    fn list_repositories_reports_netease_local_cache_statuses() {
        let workspace = TestWorkspace::new("netease-cache-status");
        let service_root = workspace.path("service");
        let ready_cache = workspace.path("ready-cache");
        let missing_cache = workspace.path("missing-cache");
        fs::create_dir_all(&ready_cache).expect("ready cache should be created");
        install_local_filesystem_test_plugin_archive(&service_root);
        let state = RepositoryState::from_root(service_root.clone());
        state
            .ensure_initialized()
            .expect("state should initialize registry");
        let registry =
            Connection::open(service_root.join(REGISTRY_FILE_NAME)).expect("registry should open");
        let backend = RepositoryBackendRecord {
            plugin_id: NETEASE_CLOUD_MUSIC_PLUGIN_ID.to_string(),
            config: serde_json::json!({
                "accountId": "123",
                "cookie": "MUSIC_U=test"
            }),
        };
        for (repo_id, name, path) in [
            (
                "netease-ready",
                "网易云 Ready",
                ready_cache.to_string_lossy().to_string(),
            ),
            (
                "netease-missing",
                "网易云 Missing",
                missing_cache.to_string_lossy().to_string(),
            ),
            (
                "netease-unconfigured",
                "网易云 Legacy",
                "netease-cloud-music://account/123".to_string(),
            ),
        ] {
            let seed = RepositorySeed {
                repo_id,
                name,
                root_path: "",
                status: "ready",
                assets: &[],
            };
            upsert_registry_entry(&registry, Path::new(&path), &seed, &backend)
                .expect("registry entry should be stored");
        }

        let repositories = state.list_repositories().expect("repositories should list");
        let ready = repositories
            .iter()
            .find(|repo| repo.repo_id == "netease-ready")
            .expect("ready repo should exist");
        assert_eq!(ready.status, "ready");
        assert_eq!(
            ready
                .local_cache
                .as_ref()
                .map(|cache| cache.status.as_str()),
            Some("ready")
        );
        let missing = repositories
            .iter()
            .find(|repo| repo.repo_id == "netease-missing")
            .expect("missing repo should exist");
        assert_eq!(missing.status, "missing");
        assert_eq!(
            missing
                .local_cache
                .as_ref()
                .map(|cache| cache.status.as_str()),
            Some("missing")
        );
        let unconfigured = repositories
            .iter()
            .find(|repo| repo.repo_id == "netease-unconfigured")
            .expect("legacy repo should exist");
        assert_eq!(unconfigured.status, "missing");
        assert_eq!(
            unconfigured
                .local_cache
                .as_ref()
                .map(|cache| cache.status.as_str()),
            Some("unconfigured")
        );
        assert_eq!(
            unconfigured
                .local_cache
                .as_ref()
                .and_then(|cache| cache.path.as_deref()),
            None
        );
    }

    #[test]
    fn configure_netease_repository_cache_updates_registry_metadata_and_moves_state() {
        let workspace = TestWorkspace::new("netease-cache-configure");
        let service_root = workspace.path("service");
        let cache_root = workspace.path("netease-cache");
        install_local_filesystem_test_plugin_archive(&service_root);
        let state = RepositoryState::from_root(service_root.clone());
        state
            .ensure_initialized()
            .expect("state should initialize registry");
        let registry =
            Connection::open(service_root.join(REGISTRY_FILE_NAME)).expect("registry should open");
        let backend = RepositoryBackendRecord {
            plugin_id: NETEASE_CLOUD_MUSIC_PLUGIN_ID.to_string(),
            config: serde_json::json!({
                "accountId": "123",
                "cookie": "MUSIC_U=test-cookie"
            }),
        };
        let seed = RepositorySeed {
            repo_id: "netease-one",
            name: "网易云 A",
            root_path: "",
            status: "ready",
            assets: &[],
        };
        upsert_registry_entry(
            &registry,
            Path::new("netease-cloud-music://account/123"),
            &seed,
            &backend,
        )
        .expect("registry entry should be stored");
        let old_meta_dir =
            repository_state_storage_dir(&service_root, "netease-one").join(REPO_META_DIR);
        fs::create_dir_all(old_meta_dir.join("indexes")).expect("old index dir should be created");
        fs::write(old_meta_dir.join("indexes").join("legacy.json"), "{}")
            .expect("old index should be written");

        let response = state
            .configure_netease_repository_cache(NeteaseRepositoryCacheConfigureRequest {
                repo_id: "netease-one".to_string(),
                path: cache_root.to_string_lossy().to_string(),
                migrate_legacy_cache: true,
            })
            .expect("cache should configure");

        assert_eq!(response.repository.path, cache_root.to_string_lossy());
        assert_eq!(response.repository.status, "ready");
        assert_eq!(
            response
                .repository
                .local_cache
                .as_ref()
                .map(|cache| cache.status.as_str()),
            Some("ready")
        );
        assert!(response.migration.moved_state_files >= 1);
        let metadata_path = cache_root.join(REPO_META_DIR).join(REPO_METADATA_FILE_NAME);
        let metadata_raw = fs::read_to_string(metadata_path).expect("metadata should be written");
        let metadata: RepositoryMetadataFileImport =
            serde_json::from_str(&metadata_raw).expect("metadata should parse");
        assert_eq!(metadata.repo_id, "netease-one");
        assert_eq!(
            metadata
                .backend_config
                .as_ref()
                .and_then(|config| config.get("sourceUri"))
                .and_then(serde_json::Value::as_str),
            Some("netease-cloud-music://account/123")
        );
        assert_eq!(
            metadata
                .backend_config
                .as_ref()
                .and_then(|config| config.get("localCachePath"))
                .and_then(serde_json::Value::as_str),
            Some(cache_root.to_string_lossy().as_ref())
        );
        assert!(cache_root
            .join(REPO_META_DIR)
            .join("indexes")
            .join("legacy.json")
            .is_file());
    }

    #[test]
    fn add_playlist_items_by_paths_expands_directories_and_deduplicates_files() {
        let (state, root, repo_root, _thumbnail_root) =
            create_test_state("playlist-items-by-paths");
        fs::create_dir_all(repo_root.join("Albums/Disc 1"))
            .expect("album directory should be created");
        fs::create_dir_all(repo_root.join("Singles")).expect("singles directory should be created");
        fs::write(repo_root.join("Albums/Disc 1/track-01.mp3"), b"track one")
            .expect("first track should be written");
        fs::write(repo_root.join("Albums/Disc 1/track-02.mp3"), b"track two")
            .expect("second track should be written");
        fs::write(repo_root.join("Singles/track-03.mp3"), b"track three")
            .expect("third track should be written");
        let repo_id = create_repository_without_initial_sync(&state, &repo_root);
        state
            .sync_repository(SyncRequest {
                repo_id: repo_id.clone(),
            })
            .expect("repository should sync");

        let repo = state
            .load_repository_record(&repo_id)
            .expect("repository record should load");
        let connection = state
            .open_repository_connection(
                &repo.summary.repo_id,
                &repo.summary.path,
                &repo.backend_record,
            )
            .expect("repository connection should open");
        let playlist_id = "playlist-by-paths";
        let now = now_rfc3339();
        connection
            .execute(
                r#"
                INSERT INTO playlists (
                  playlist_id, repo_id, name, player_type_id, player_plugin_id,
                  player_label, file_class, sort_order, created_at, updated_at
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8, ?8)
                "#,
                params![
                    playlist_id,
                    repo_id,
                    "路径展开测试歌单",
                    "momobako.playlist.audio-sequence",
                    "momobako.preview.media",
                    "音频顺序播放",
                    "audio",
                    now,
                ],
            )
            .expect("playlist should be inserted");

        {
            let detail = state
                .add_playlist_items_by_paths(PlaylistItemsByPathsAddRequest {
                    repo_id: repo_id.clone(),
                    playlist_id: playlist_id.to_string(),
                    paths: vec![
                        "Albums".to_string(),
                        "Albums/Disc 1/track-01.mp3".to_string(),
                        "Singles/track-03.mp3".to_string(),
                    ],
                })
                .expect("playlist items should be added by paths");

            let mut actual_paths = detail
                .items
                .iter()
                .map(|item| item.path.clone())
                .collect::<Vec<_>>();
            actual_paths.sort();
            assert_eq!(
                actual_paths,
                vec![
                    "Albums/Disc 1/track-01.mp3".to_string(),
                    "Albums/Disc 1/track-02.mp3".to_string(),
                    "Singles/track-03.mp3".to_string(),
                ]
            );
        }
        drop(connection);

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn add_playlist_items_by_paths_expands_virtual_playlist_folders() {
        let (state, root, repo_root, _thumbnail_root) =
            create_test_state("playlist-items-by-paths-virtual");
        let repo_id = create_repository_without_initial_sync(&state, &repo_root);

        let repo = state
            .load_repository_record(&repo_id)
            .expect("repository record should load");
        let connection = state
            .open_repository_connection(
                &repo.summary.repo_id,
                &repo.summary.path,
                &repo.backend_record,
            )
            .expect("repository connection should open");
        let playlist_id = "playlist-virtual-by-paths";
        let now = now_rfc3339();
        connection
            .execute(
                r#"
                INSERT INTO playlists (
                  playlist_id, repo_id, name, player_type_id, player_plugin_id,
                  player_label, file_class, sort_order, created_at, updated_at
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8, ?8)
                "#,
                params![
                    playlist_id,
                    repo_id,
                    "网易云虚拟歌单",
                    "momobako.playlist.audio-sequence",
                    "momobako.preview.media",
                    "音频顺序播放",
                    "audio",
                    now,
                ],
            )
            .expect("playlist should be inserted");

        for (asset_id, path, song_id, song_name) in [
            (
                asset_id_for_path(&repo_id, "创建的歌单/夜跑歌单/周杰伦 - 稻香.mp3"),
                "创建的歌单/夜跑歌单/周杰伦 - 稻香.mp3",
                2001_i64,
                "稻香",
            ),
            (
                asset_id_for_path(&repo_id, "创建的歌单/夜跑歌单/陈奕迅 - 孤勇者.mp3"),
                "创建的歌单/夜跑歌单/陈奕迅 - 孤勇者.mp3",
                2002_i64,
                "孤勇者",
            ),
        ] {
            connection
                .execute(
                    r#"
                    INSERT INTO assets (
                      asset_id, repo_id, path, filename, extension, size_bytes, created_at, modified_at,
                      hash, status, version, updated_at, thumbnail_path, is_virtual, provider_id,
                      provider_item_id, source_payload_json, local_absolute_path
                    )
                    VALUES (?1, ?2, ?3, ?4, 'mp3', 0, ?5, ?5, NULL, 'synced', 1, ?5, NULL, 1, ?6, ?7, ?8, NULL)
                    "#,
                    params![
                        asset_id,
                        repo_id,
                        path,
                        Path::new(path)
                            .file_name()
                            .expect("virtual asset path should contain a filename")
                            .to_string_lossy()
                            .to_string(),
                        now,
                        "netease-cloud-music",
                        song_id.to_string(),
                        serde_json::json!({
                            "provider": "netease-cloud-music",
                            "playlistId": 9001,
                            "playlistName": "夜跑歌单",
                            "playlistCategory": "created",
                            "songId": song_id,
                            "songName": song_name,
                            "virtualEntry": true
                        })
                        .to_string(),
                    ],
                )
                .expect("virtual asset should be inserted");
        }

        let detail = state
            .add_playlist_items_by_paths(PlaylistItemsByPathsAddRequest {
                repo_id: repo_id.clone(),
                playlist_id: playlist_id.to_string(),
                paths: vec!["创建的歌单/夜跑歌单".to_string()],
            })
            .expect("virtual playlist folder should expand into playable tracks");

        let mut actual_paths = detail
            .items
            .iter()
            .map(|item| item.path.clone())
            .collect::<Vec<_>>();
        actual_paths.sort();
        assert_eq!(
            actual_paths,
            vec![
                "创建的歌单/夜跑歌单/周杰伦 - 稻香.mp3".to_string(),
                "创建的歌单/夜跑歌单/陈奕迅 - 孤勇者.mp3".to_string(),
            ]
        );
        assert!(detail.items.iter().all(|item| item.is_virtual));

        drop(connection);
        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn plugin_registry_discovers_runtime_manifests() {
        let workspace = TestWorkspace::new("plugin-registry");
        let service_root = workspace.path("service");
        seed_standard_test_plugins(&service_root);
        let registry = backend_plugin_registry(&service_root);
        let manifests = registry.list_manifests();

        assert!(manifests.iter().any(|manifest| {
            manifest.plugin_id == LOCAL_FILESYSTEM_PLUGIN_ID
                && manifest.category == "source"
                && manifest.runtime == "native-dylib"
                && manifest
                    .legacy_plugin_ids
                    .contains(&LEGACY_LOCAL_FILESYSTEM_PLUGIN_ID.to_string())
        }));
        assert!(manifests.iter().any(|manifest| {
            manifest.plugin_id == "momobako.preview.media"
                && manifest.category == "preview"
                && manifest.sdk == "frontend"
                && manifest.hooks.iter().any(|hook| hook.slot == "playlist")
        }));
        assert!(manifests.iter().any(|manifest| {
            manifest.plugin_id == "momobako.library.audio"
                && manifest.category == "library-kind"
                && manifest
                    .optional
                    .contains(&"momobako.parser.audio".to_string())
        }));
        assert!(manifests.iter().any(|manifest| {
            manifest.plugin_id == "momobako.parser.audio" && manifest.category == "parser"
        }));
        assert!(manifests.iter().any(|manifest| {
            manifest.plugin_id == "momobako.service.network-search"
                && manifest.category == "service"
        }));
        assert_eq!(
            registry.normalize_plugin_id(LOCAL_FILESYSTEM_PLUGIN_ID),
            LOCAL_FILESYSTEM_PLUGIN_ID
        );
    }

    #[test]
    fn plugin_registry_resolves_dependencies_and_degraded_state() {
        let workspace = TestWorkspace::new("plugin-dependency-state");
        let plugin_root = workspace.path("service/plugins");
        fs::create_dir_all(&plugin_root).expect("runtime plugin dir should be created");
        write_test_plugin_archive_with_manifest(
            &plugin_root.join("required-provider.momoplug"),
            test_plugin_manifest_json("user.provider", "Provider", serde_json::json!({})),
        );
        write_test_plugin_archive_with_manifest(
            &plugin_root.join("optional-helper.momoplug"),
            test_plugin_manifest_json(
                "user.optional-helper",
                "Optional Helper",
                serde_json::json!({}),
            ),
        );
        write_test_plugin_archive_with_manifest(
            &plugin_root.join("dependent-plugin.momoplug"),
            test_plugin_manifest_json(
                "user.dependent",
                "Dependent Plugin",
                serde_json::json!({
                    "permissions": ["readMetadata"],
                    "requires": ["user.provider"],
                    "optional": ["user.optional-helper"]
                }),
            ),
        );
        let state = RepositoryState::from_root(workspace.path("service"));

        state
            .set_plugin_enabled(PluginEnabledRequest {
                plugin_id: "user.optional-helper".to_string(),
                enabled: false,
            })
            .expect("optional helper should be disabled");
        let plugins = state.list_plugins().expect("plugins should load");
        let dependent = plugins
            .iter()
            .find(|manifest| manifest.plugin_id == "user.dependent")
            .expect("dependent plugin should exist");

        assert_eq!(dependent.status, "ready");
        assert!(dependent.enabled);
        assert!(dependent.degraded);
        assert_eq!(
            dependent.dependency_status.optional[0].plugin_id,
            "user.optional-helper"
        );
        assert_eq!(dependent.dependency_status.optional[0].status, "disabled");
        assert!(dependent
            .degradation_reason
            .as_deref()
            .unwrap_or_default()
            .contains("Optional Helper"));

        fs::remove_file(plugin_root.join("required-provider.momoplug"))
            .expect("required provider archive should be removable");
        let plugins = state.list_plugins().expect("plugins should reload");
        let dependent = plugins
            .iter()
            .find(|manifest| manifest.plugin_id == "user.dependent")
            .expect("dependent plugin should remain listed");

        assert_eq!(dependent.status, "unavailable");
        assert!(!dependent.enabled);
        assert_eq!(dependent.dependency_status.required[0].status, "missing");
        assert!(dependent
            .disable_reason
            .as_deref()
            .unwrap_or_default()
            .contains("user.provider"));
    }

    #[test]
    fn plugin_data_directory_uses_service_plugin_data_root_and_legacy_ids() {
        let workspace = TestWorkspace::new("plugin-data-directory");
        let service_root = workspace.path("service");
        seed_standard_test_plugins(&service_root);
        let state = RepositoryState::from_root(service_root.clone());

        let response = state
            .get_plugin_data_directory(LEGACY_LOCAL_FILESYSTEM_PLUGIN_ID.to_string())
            .expect("plugin data directory should be returned");
        let expected_path = plugin_data_dir(&service_root, LOCAL_FILESYSTEM_PLUGIN_ID);

        assert_eq!(response.plugin_id, LOCAL_FILESYSTEM_PLUGIN_ID);
        assert_eq!(PathBuf::from(response.path), expected_path);
        assert!(expected_path.is_dir());
        assert!(expected_path.starts_with(service_root.join("plugin-data")));
    }

    #[test]
    fn plugin_data_file_preview_source_is_limited_to_plugin_data_dir() {
        let workspace = TestWorkspace::new("plugin-data-preview-source");
        let service_root = workspace.path("service");
        seed_standard_test_plugins(&service_root);
        let state = RepositoryState::from_root(service_root.clone());
        let data_dir = plugin_data_dir(&service_root, LOCAL_FILESYSTEM_PLUGIN_ID);
        fs::create_dir_all(&data_dir).expect("plugin data directory should be created");
        let preview_file = data_dir.join("preview.txt");
        fs::write(&preview_file, b"hello").expect("preview file should be written");
        let outside_file = service_root.join("outside.txt");
        fs::write(&outside_file, b"outside").expect("outside file should be written");

        let response = state
            .prepare_plugin_data_file_preview_source(PluginDataFilePreviewSourceRequest {
                plugin_id: LEGACY_LOCAL_FILESYSTEM_PLUGIN_ID.to_string(),
                path: preview_file.to_string_lossy().to_string(),
                media_type: "text/plain; charset=utf-8".to_string(),
            })
            .expect("plugin data preview source should register");

        assert_eq!(response.plugin_id, LOCAL_FILESYSTEM_PLUGIN_ID);
        assert_eq!(response.media_type, "text/plain; charset=utf-8");
        assert_eq!(response.size_bytes, 5);
        assert!(state.open_preview_file_source(&response.token).is_ok());

        let error = state
            .prepare_plugin_data_file_preview_source(PluginDataFilePreviewSourceRequest {
                plugin_id: LOCAL_FILESYSTEM_PLUGIN_ID.to_string(),
                path: outside_file.to_string_lossy().to_string(),
                media_type: "text/plain".to_string(),
            })
            .expect_err("outside plugin data files should be rejected");

        assert!(error.contains("outside plugin data directory"));
    }

    #[test]
    fn repository_cache_file_preview_source_is_limited_to_repository_cache_dir() {
        let workspace = TestWorkspace::new("repository-cache-preview-source");
        let service_root = workspace.path("service");
        let repo_root = workspace.path("repo");
        install_local_filesystem_test_plugin_archive(&service_root);
        let state = RepositoryState::from_root(service_root);

        state
            .create_repository(RepositoryMutationRequest {
                repo_id: Some("repo-preview-cache".to_string()),
                name: "缓存预览测试".to_string(),
                path: repo_root.to_string_lossy().to_string(),
                backend_plugin_id: Some(LOCAL_FILESYSTEM_PLUGIN_ID.to_string()),
                backend_config: None,
                skip_initial_sync: true,
            })
            .expect("repository should be created");

        let cache_dir = repo_root.join(REPO_META_DIR).join("cache").join("office-preview");
        fs::create_dir_all(&cache_dir).expect("repository cache dir should be created");
        let preview_file = cache_dir.join("preview.pdf");
        fs::write(&preview_file, b"%PDF-1.4").expect("preview cache file should be written");
        let outside_file = repo_root.join("outside.pdf");
        fs::write(&outside_file, b"%PDF-1.4").expect("outside file should be written");

        let response = state
            .prepare_repository_cache_file_preview_source(RepositoryCacheFilePreviewSourceRequest {
                repo_id: "repo-preview-cache".to_string(),
                path: preview_file.to_string_lossy().to_string(),
                media_type: "application/pdf".to_string(),
            })
            .expect("repository cache preview source should register");

        assert_eq!(response.repo_id, "repo-preview-cache");
        assert_eq!(response.media_type, "application/pdf");
        assert_eq!(response.size_bytes, 8);
        assert!(state.open_preview_file_source(&response.token).is_ok());

        let error = state
            .prepare_repository_cache_file_preview_source(RepositoryCacheFilePreviewSourceRequest {
                repo_id: "repo-preview-cache".to_string(),
                path: outside_file.to_string_lossy().to_string(),
                media_type: "application/pdf".to_string(),
            })
            .expect_err("outside repository cache files should be rejected");

        assert!(error.contains("outside repository cache directory"));
    }

    #[test]
    fn shutdown_runtime_helpers_cleans_office_helper_state_files() {
        let workspace = TestWorkspace::new("shutdown-office-helper-state");
        let service_root = workspace.path("service");
        seed_standard_test_plugins(&service_root);
        let state = RepositoryState::from_root(service_root.clone());

        let plugin_dir = plugin_data_dir(&service_root, "momobako.service.office-convert");
        let helper_dir = plugin_dir.join("helpers").join("libreoffice");
        fs::create_dir_all(&helper_dir).expect("office helper dir should be created");
        fs::write(helper_dir.join("pid.txt"), "invalid").expect("pid state should be written");
        fs::write(helper_dir.join("status.json"), "{}").expect("status state should be written");
        fs::write(helper_dir.join("port.txt"), "23119").expect("port state should be written");
        fs::write(helper_dir.join("session.txt"), "office").expect("session state should be written");
        fs::write(helper_dir.join("office-convert-helper.ps1"), "Write-Host helper")
            .expect("helper script should be written");

        state
            .shutdown_runtime_helpers()
            .expect("helper shutdown should clean stale office state");

        assert!(!helper_dir.join("pid.txt").exists());
        assert!(!helper_dir.join("status.json").exists());
        assert!(!helper_dir.join("port.txt").exists());
        assert!(!helper_dir.join("session.txt").exists());
        assert!(!helper_dir.join("office-convert-helper.ps1").exists());
    }

    #[test]
    fn shutdown_runtime_helpers_cleans_aria2_helper_state_files() {
        let workspace = TestWorkspace::new("shutdown-aria2-helper-state");
        let service_root = workspace.path("service");
        seed_standard_test_plugins(&service_root);
        let state = RepositoryState::from_root(service_root.clone());

        let plugin_dir = plugin_data_dir(&service_root, "momobako.service.downloader");
        let helper_dir = plugin_dir.join("helpers").join("aria2");
        fs::create_dir_all(&helper_dir).expect("aria2 helper dir should be created");
        fs::write(helper_dir.join("pid.txt"), "invalid").expect("pid state should be written");
        fs::write(helper_dir.join("status.json"), "{}").expect("status state should be written");
        fs::write(helper_dir.join("session.txt"), "aria2-session")
            .expect("session state should be written");

        state
            .shutdown_runtime_helpers()
            .expect("helper shutdown should clean stale aria2 state");

        assert!(!helper_dir.join("pid.txt").exists());
        assert!(!helper_dir.join("status.json").exists());
        assert!(!helper_dir.join("session.txt").exists());
    }

    #[test]
    fn plugin_config_api_persists_values_in_plugin_data_dir() {
        let workspace = TestWorkspace::new("plugin-config");
        let service_root = workspace.path("service");
        seed_standard_test_plugins(&service_root);
        let state = RepositoryState::from_root(service_root.clone());

        let updated = state
            .set_plugin_config_value(PluginConfigSetRequest {
                plugin_id: LEGACY_LOCAL_FILESYSTEM_PLUGIN_ID.to_string(),
                key: "apiKey".to_string(),
                value: serde_json::json!("secret"),
            })
            .expect("plugin config value should be written");

        assert_eq!(updated.plugin_id, LOCAL_FILESYSTEM_PLUGIN_ID);
        assert_eq!(
            updated.values.get("apiKey"),
            Some(&serde_json::json!("secret"))
        );
        let data_dir = plugin_data_dir(&service_root, LOCAL_FILESYSTEM_PLUGIN_ID);
        assert!(data_dir.join("config.json").is_file());

        let loaded = state
            .get_plugin_config(LOCAL_FILESYSTEM_PLUGIN_ID.to_string())
            .expect("plugin config should be loaded");
        assert_eq!(
            loaded.values.get("apiKey"),
            Some(&serde_json::json!("secret"))
        );

        let deleted = state
            .delete_plugin_config_value(PluginConfigDeleteRequest {
                plugin_id: LOCAL_FILESYSTEM_PLUGIN_ID.to_string(),
                key: "apiKey".to_string(),
            })
            .expect("plugin config value should be deleted");
        assert!(!deleted.values.contains_key("apiKey"));
    }

    #[test]
    fn plugin_config_api_includes_schema_and_rejects_mismatched_values() {
        let workspace = TestWorkspace::new("plugin-config-schema");
        let plugin_root = workspace.path("service/plugins");
        fs::create_dir_all(&plugin_root).expect("runtime plugin dir should be created");
        write_test_plugin_archive_with_manifest(
            &plugin_root.join("configurable.momoplug"),
            test_plugin_manifest_json(
                "user.configurable",
                "Configurable Plugin",
                serde_json::json!({
                    "contributes": {
                        "settings": {
                            "schemaVersion": 1,
                            "fields": [
                                { "key": "enabled", "label": "Enabled", "type": "boolean" },
                                {
                                    "key": "mode",
                                    "label": "Mode",
                                    "type": "select",
                                    "options": [
                                        { "label": "Fast", "value": "fast" },
                                        { "label": "Careful", "value": "careful" }
                                    ]
                                }
                            ]
                        }
                    }
                }),
            ),
        );
        let state = RepositoryState::from_root(workspace.path("service"));

        let snapshot = state
            .get_plugin_config("user.configurable".to_string())
            .expect("plugin config schema should load");
        assert_eq!(
            snapshot.schema["fields"][0]["key"],
            serde_json::json!("enabled")
        );

        let error = state
            .set_plugin_config_value(PluginConfigSetRequest {
                plugin_id: "user.configurable".to_string(),
                key: "enabled".to_string(),
                value: serde_json::json!("yes"),
            })
            .expect_err("schema mismatch should be rejected");
        assert!(error.contains("boolean"));

        state
            .set_plugin_config_value(PluginConfigSetRequest {
                plugin_id: "user.configurable".to_string(),
                key: "mode".to_string(),
                value: serde_json::json!("fast"),
            })
            .expect("schema option should be accepted");
    }

    #[test]
    fn plugin_call_envelope_serializes_runtime_config_snapshot() {
        let envelope = PluginCallEnvelope {
            method: "provider.lookupMetadataCandidate".to_string(),
            payload: serde_json::json!({ "id": "sample-123456" }),
            runtime: PluginCallHostRuntime {
                plugin_id: "user.provider".to_string(),
                plugin_data_dir: "C:/MomoBako/.service-data/plugin-data/user-provider".to_string(),
                service_root_dir: "C:/MomoBako/.service-data".to_string(),
                plugin_runtime_dir: "C:/MomoBako/.service-data/runtime-cache/user-provider"
                    .to_string(),
                plugin_config: BTreeMap::from([(
                    "apiKey".to_string(),
                    serde_json::json!("secret"),
                )]),
            },
        };

        let value = serde_json::to_value(envelope).expect("envelope should serialize");

        assert_eq!(
            value["runtime"]["pluginId"],
            serde_json::json!("user.provider")
        );
        assert_eq!(
            value["runtime"]["serviceRootDir"],
            serde_json::json!("C:/MomoBako/.service-data")
        );
        assert_eq!(
            value["runtime"]["pluginRuntimeDir"],
            serde_json::json!("C:/MomoBako/.service-data/runtime-cache/user-provider")
        );
        assert_eq!(
            value["runtime"]["pluginConfig"]["apiKey"],
            serde_json::json!("secret")
        );
    }

    #[test]
    fn local_file_search_mode_defaults_to_recursive() {
        assert_eq!(
            local_file_search_mode_from_config(&serde_json::json!({})),
            LocalFileSearchMode::Recursive
        );
        assert_eq!(
            local_file_search_mode_from_config(&serde_json::json!({
                LOCAL_FILESYSTEM_FILE_SEARCH_MODE_KEY: "unknown"
            })),
            LocalFileSearchMode::Recursive
        );
        assert_eq!(
            local_file_search_mode_from_config(&serde_json::json!({
                LOCAL_FILESYSTEM_FILE_SEARCH_MODE_KEY: "ntfs"
            })),
            LocalFileSearchMode::Ntfs
        );
        assert_eq!(
            local_file_search_mode_from_config(&serde_json::json!({
                LOCAL_FILESYSTEM_FILE_SEARCH_MODE_KEY: "everything"
            })),
            LocalFileSearchMode::Everything
        );
    }

    #[test]
    fn recursive_local_file_search_preserves_file_metadata_semantics() {
        let workspace = TestWorkspace::new("recursive-local-file-search");
        let repo_root = workspace.path("repo");
        fs::create_dir_all(repo_root.join("music")).expect("music dir should be created");
        fs::create_dir_all(repo_root.join(".momo")).expect("meta dir should be created");
        fs::create_dir_all(repo_root.join(".meta")).expect("legacy meta dir should be created");
        fs::write(repo_root.join("music").join("demo.flac"), b"audio")
            .expect("file should be written");
        fs::write(repo_root.join(".momo").join("hidden.flac"), b"skip")
            .expect("hidden file should be written");
        fs::write(repo_root.join(".meta").join("legacy.flac"), b"skip")
            .expect("legacy file should be written");

        let files = collect_repository_files_with_mode(
            &repo_root,
            &serde_json::json!({
                LOCAL_FILESYSTEM_FILE_SEARCH_MODE_KEY: "recursive"
            }),
        )
        .expect("recursive scan should succeed");

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].relative_path, "music/demo.flac");
        assert_eq!(files[0].filename, "demo.flac");
        assert_eq!(files[0].extension, "flac");
        assert_eq!(files[0].size_bytes, 5);
        assert!(files[0].modified_at.contains('T'));
    }

    #[test]
    fn unavailable_index_local_file_search_falls_back_to_recursive_scan() {
        fn unavailable(_repo_root: &Path) -> Result<Vec<DiscoveredFile>, String> {
            Err("not available".to_string())
        }

        let workspace = TestWorkspace::new("unavailable-index-local-file-search");
        let repo_root = workspace.path("repo");
        fs::create_dir_all(&repo_root).expect("repo root should be created");
        fs::write(repo_root.join("demo.mp3"), b"audio").expect("file should be written");

        let files = collect_repository_files_with_fallback(&repo_root, "Test", unavailable)
            .expect("fallback scan should succeed");

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].relative_path, "demo.mp3");
    }

    #[test]
    fn native_dylib_source_plugin_without_library_is_marked_unavailable() {
        let workspace = TestWorkspace::new("native-dylib-source-unavailable");
        let service_root = workspace.path("service");
        let plugin_root = runtime_plugins_dir(&service_root);
        write_test_local_filesystem_plugin_archive(&plugin_root, serde_json::json!({}));
        let state = RepositoryState::from_root(service_root);

        let plugin = state
            .list_plugins()
            .expect("plugins should load")
            .into_iter()
            .find(|manifest| manifest.plugin_id == LOCAL_FILESYSTEM_PLUGIN_ID)
            .expect("local filesystem plugin should exist");

        assert_eq!(plugin.status, "unavailable");
        assert!(
            plugin
                .disable_reason
                .as_deref()
                .unwrap_or_default()
                .contains("原生运行时不可用")
        );
    }

    #[test]
    fn plugin_dependency_resolution_accepts_legacy_ids() {
        let workspace = TestWorkspace::new("plugin-legacy-dependency");
        let plugin_root = workspace.path("service/plugins");
        fs::create_dir_all(&plugin_root).expect("runtime plugin dir should be created");
        write_test_plugin_archive_with_manifest(
            &plugin_root.join("provider.momoplug"),
            test_plugin_manifest_json(
                "user.provider",
                "Provider",
                serde_json::json!({
                    "legacyPluginIds": ["legacy.provider"],
                    "compat": {
                        "sdkVersion": "1",
                        "legacyPluginIds": []
                    }
                }),
            ),
        );
        write_test_plugin_archive_with_manifest(
            &plugin_root.join("dependent.momoplug"),
            test_plugin_manifest_json(
                "user.dependent",
                "Dependent",
                serde_json::json!({
                    "requires": ["legacy.provider"]
                }),
            ),
        );
        let state = RepositoryState::from_root(workspace.path("service"));
        let plugins = state.list_plugins().expect("plugins should load");
        let dependent = plugins
            .iter()
            .find(|manifest| manifest.plugin_id == "user.dependent")
            .expect("dependent plugin should exist");

        assert_eq!(dependent.status, "ready");
        assert_eq!(
            dependent.dependency_status.required[0].plugin_id,
            "user.provider"
        );
        assert!(dependent.dependency_status.missing_required.is_empty());
    }

    #[test]
    fn plugin_call_blocks_missing_required_dependency() {
        let workspace = TestWorkspace::new("plugin-call-required-missing");
        let service_root = workspace.path("service");
        let plugin_root = runtime_plugins_dir(&service_root);
        write_test_local_filesystem_plugin_archive(
            &plugin_root,
            serde_json::json!({ "requires": ["user.required-provider"] }),
        );
        let state = RepositoryState::from_root(service_root.clone());
        let error = state
            .call_plugin(test_list_files_plugin_call(
                LEGACY_LOCAL_FILESYSTEM_PLUGIN_ID,
                workspace.path("repo"),
            ))
            .expect_err("missing required dependency should block plugin call");

        assert!(error.contains("plugin call blocked by dependency status"));
        assert!(error.contains(LOCAL_FILESYSTEM_PLUGIN_ID));
        assert!(error.contains("filesystem.listFiles"));
        assert!(error.contains("缺少必需依赖"));
    }

    #[test]
    fn plugin_call_blocks_disabled_required_dependency() {
        let workspace = TestWorkspace::new("plugin-call-required-disabled");
        let service_root = workspace.path("service");
        let plugin_root = runtime_plugins_dir(&service_root);
        write_test_plugin_archive_with_manifest(
            &plugin_root.join("required-provider.momoplug"),
            test_plugin_manifest_json(
                "user.required-provider",
                "Required Provider",
                serde_json::json!({}),
            ),
        );
        write_test_local_filesystem_plugin_archive(
            &plugin_root,
            serde_json::json!({ "requires": ["user.required-provider"] }),
        );
        let state = RepositoryState::from_root(service_root);
        state
            .set_plugin_enabled(PluginEnabledRequest {
                plugin_id: "user.required-provider".to_string(),
                enabled: false,
            })
            .expect("required provider should be disabled");

        let error = state
            .call_plugin(test_list_files_plugin_call(
                LOCAL_FILESYSTEM_PLUGIN_ID,
                workspace.path("repo"),
            ))
            .expect_err("disabled required dependency should block plugin call");

        assert!(error.contains("plugin call blocked by dependency status"));
        assert!(error.contains("必需依赖不可用"));
        assert!(error.contains("Required Provider"));
    }

    #[test]
    fn plugin_call_returns_degraded_runtime_for_disabled_optional_dependency() {
        let workspace = TestWorkspace::new("plugin-call-optional-disabled");
        let service_root = workspace.path("service");
        let repo_root = workspace.path("repo");
        fs::create_dir_all(&repo_root).expect("repo root should be created");
        fs::write(repo_root.join("track.mp3"), b"audio").expect("test file should be written");
        let plugin_root = runtime_plugins_dir(&service_root);
        write_test_plugin_archive_with_manifest(
            &plugin_root.join("optional-helper.momoplug"),
            test_plugin_manifest_json(
                "user.optional-helper",
                "Optional Helper",
                serde_json::json!({}),
            ),
        );
        write_test_local_filesystem_plugin_archive(
            &plugin_root,
            serde_json::json!({ "optional": ["user.optional-helper"] }),
        );
        let state = RepositoryState::from_root(service_root);
        state
            .set_plugin_enabled(PluginEnabledRequest {
                plugin_id: "user.optional-helper".to_string(),
                enabled: false,
            })
            .expect("optional helper should be disabled");

        let response = state
            .call_plugin(test_list_files_plugin_call(
                LEGACY_LOCAL_FILESYSTEM_PLUGIN_ID,
                repo_root,
            ))
            .expect("optional dependency should not block plugin call");

        assert_eq!(response.plugin_id, LOCAL_FILESYSTEM_PLUGIN_ID);
        assert!(response.payload.is_array());
        let runtime = response
            .runtime
            .expect("degraded runtime context should be returned");
        assert!(runtime.degraded);
        assert!(runtime
            .degradation_reason
            .as_deref()
            .unwrap_or_default()
            .contains("Optional Helper"));
        assert_eq!(
            runtime.dependency_status.optional[0].plugin_id,
            "user.optional-helper"
        );
        assert_eq!(runtime.dependency_status.optional[0].status, "disabled");
    }

    #[test]
    fn plugin_call_accepts_legacy_required_dependency_id() {
        let workspace = TestWorkspace::new("plugin-call-legacy-required");
        let service_root = workspace.path("service");
        let repo_root = workspace.path("repo");
        fs::create_dir_all(&repo_root).expect("repo root should be created");
        let plugin_root = runtime_plugins_dir(&service_root);
        write_test_plugin_archive_with_manifest(
            &plugin_root.join("provider.momoplug"),
            test_plugin_manifest_json(
                "user.provider",
                "Provider",
                serde_json::json!({
                    "legacyPluginIds": ["legacy.provider"],
                    "compat": {
                        "sdkVersion": "1",
                        "legacyPluginIds": []
                    }
                }),
            ),
        );
        write_test_local_filesystem_plugin_archive(
            &plugin_root,
            serde_json::json!({ "requires": ["legacy.provider"] }),
        );
        let state = RepositoryState::from_root(service_root);

        let response = state
            .call_plugin(test_list_files_plugin_call(
                LEGACY_LOCAL_FILESYSTEM_PLUGIN_ID,
                repo_root,
            ))
            .expect("legacy dependency id should resolve before plugin call");

        assert_eq!(response.plugin_id, LOCAL_FILESYSTEM_PLUGIN_ID);
        assert!(response.runtime.is_none());
    }

    #[test]
    fn plugin_hook_execution_records_declared_hook_calls() {
        let workspace = TestWorkspace::new("plugin-hook-execution-records");
        let service_root = workspace.path("service");
        let repo_root = workspace.path("repo");
        fs::create_dir_all(&repo_root).expect("repo root should be created");
        fs::write(repo_root.join("note.txt"), b"note").expect("test file should be written");
        let plugin_root = runtime_plugins_dir(&service_root);
        write_test_local_filesystem_plugin_archive(
            &plugin_root,
            serde_json::json!({
                "hooks": [
                    {
                        "slot": "auditLog",
                        "action": "filesystem.listFiles",
                        "label": "记录文件列表"
                    }
                ]
            }),
        );
        let state = RepositoryState::from_root(service_root);

        state
            .call_plugin(test_list_files_plugin_call(
                LEGACY_LOCAL_FILESYSTEM_PLUGIN_ID,
                repo_root,
            ))
            .expect("declared hook plugin call should succeed");
        let all_records = state
            .list_plugin_hook_executions(PluginHookExecutionListRequest::default())
            .expect("hook execution records should load");

        assert_eq!(all_records.records.len(), 1);
        let record = &all_records.records[0];
        assert_eq!(record.plugin_id, LOCAL_FILESYSTEM_PLUGIN_ID);
        assert_eq!(record.hook_slot, "auditLog");
        assert_eq!(record.hook_action, "filesystem.listFiles");
        assert_eq!(record.hook_label.as_deref(), Some("记录文件列表"));
        assert_eq!(record.status, "success");
        assert_eq!(record.target.get("repoRoot"), None);
        assert_eq!(record.target.get("config"), None);

        state
            .call_plugin(PluginCallRequest {
                plugin_id: LOCAL_FILESYSTEM_PLUGIN_ID.to_string(),
                method: "filesystem.listTree".to_string(),
                payload: serde_json::json!({
                    "repoRoot": workspace.path("repo"),
                    "path": "note.txt"
                }),
            })
            .expect("non-hook plugin call should succeed");
        let filtered = state
            .list_plugin_hook_executions(PluginHookExecutionListRequest {
                plugin_id: Some(LEGACY_LOCAL_FILESYSTEM_PLUGIN_ID.to_string()),
                limit: Some(1),
            })
            .expect("filtered hook execution records should load");

        assert_eq!(filtered.records.len(), 1);
        assert_eq!(filtered.records[0].hook_action, "filesystem.listFiles");
    }

    #[test]
    fn release_plugin_manifest_loading_returns_empty_when_runtime_dir_is_empty() {
        let workspace = TestWorkspace::new("runtime-plugin-empty");
        let manifests = load_plugin_manifests_from_runtime(workspace.path("plugins"));

        assert!(manifests.is_empty());
    }

    #[test]
    fn runtime_manifest_scan_reflects_deleted_plugin_archives() {
        let workspace = TestWorkspace::new("runtime-plugin-scan");
        let plugin_root = workspace.path("plugins");
        fs::create_dir_all(&plugin_root).expect("runtime plugin dir should be created");
        let plugin_archive = plugin_root.join("sample-plugin.momoplug");
        write_test_plugin_archive(&plugin_archive, "user.sample-runtime");

        let manifests = load_plugin_manifests_from_runtime(plugin_root.clone());
        assert_eq!(manifests.len(), 1);
        assert_eq!(manifests[0].manifest.plugin_id, "user.sample-runtime");

        fs::remove_file(plugin_archive).expect("runtime plugin archive should be removable");
        let manifests = load_plugin_manifests_from_runtime(plugin_root);
        assert!(manifests.is_empty());
    }

    #[test]
    fn set_plugin_enabled_persists_plugin_state() {
        let workspace = TestWorkspace::new("plugin-enabled-state");
        let service_root = workspace.path("service");
        seed_standard_test_plugins(&service_root);
        let state = RepositoryState::from_root(service_root.clone());

        let response = state
            .set_plugin_enabled(PluginEnabledRequest {
                plugin_id: "momobako.preview.media".to_string(),
                enabled: false,
            })
            .expect("plugin should be disabled");
        let disabled = response
            .plugins
            .iter()
            .find(|plugin| plugin.plugin_id == "momobako.preview.media")
            .expect("media plugin should be listed");
        assert!(!disabled.enabled);
        assert_eq!(disabled.status, "disabled");

        let reloaded_state = RepositoryState::from_root(service_root);
        let plugins = reloaded_state
            .list_plugins()
            .expect("plugins should reload from persisted state");
        let disabled = plugins
            .iter()
            .find(|plugin| plugin.plugin_id == "momobako.preview.media")
            .expect("media plugin should be listed after reload");
        assert!(!disabled.enabled);
    }

    #[test]
    fn delete_plugin_rejects_builtin_plugins() {
        let workspace = TestWorkspace::new("builtin-plugin-delete");
        let service_root = workspace.path("service");
        seed_standard_test_plugins(&service_root);
        let state = RepositoryState::from_root(service_root);

        let error = state
            .delete_plugin("momobako.preview.media".to_string())
            .expect_err("built-in plugins should not be deleted");

        assert!(error.contains("built-in plugins cannot be deleted"));
    }

    #[test]
    fn install_plugin_from_archive_loads_and_deletes_user_plugin() {
        let workspace = TestWorkspace::new("plugin-archive-install");
        let service_root = workspace.path("service");
        let archive_path = workspace.path("sample-plugin.momoplug");
        write_test_plugin_archive(&archive_path, "user.sample-metadata");
        let state = RepositoryState::from_root(service_root.clone());

        let response = state
            .install_plugin_from_archive(PluginInstallRequest {
                package_path: archive_path.to_string_lossy().to_string(),
            })
            .expect("plugin archive should install");
        let installed = response
            .plugins
            .iter()
            .find(|plugin| plugin.plugin_id == "user.sample-metadata")
            .expect("installed plugin should be listed");
        assert_eq!(installed.source, "user");
        assert!(installed.enabled);
        assert!(runtime_plugins_dir(&service_root)
            .join("user-sample-metadata-0.1.0.momoplug")
            .is_file());

        let response = state
            .delete_plugin("user.sample-metadata".to_string())
            .expect("user plugin should be deleted");
        assert!(!response
            .plugins
            .iter()
            .any(|plugin| plugin.plugin_id == "user.sample-metadata"));
        assert!(!runtime_plugins_dir(&service_root)
            .join("user-sample-metadata-0.1.0.momoplug")
            .exists());
    }

    #[test]
    fn read_plugin_archive_text_supports_single_root_directory_packages() {
        let workspace = TestWorkspace::new("plugin-archive-read-text");
        let service_root = workspace.path("service");
        let runtime_root = runtime_plugins_dir(&service_root);
        fs::create_dir_all(&runtime_root).expect("runtime plugin dir should be created");
        write_test_plugin_archive_with_options(
            &runtime_root.join("example-text-preview-0.1.0.momoplug"),
            TestPluginArchiveOptions {
                plugin_id: "momobako.example.text-preview",
                name: "Example Text Preview",
                source: "user",
                runtime: "vue-module",
                sdk: "frontend",
                kind: "preview",
                plugin_type_layer: "library-kind",
                plugin_type_kind: "preview",
                entry: serde_json::json!({
                    "frontend": {
                        "module": "dist/register.js",
                        "export": "register"
                    }
                }),
                extra_files: vec![(
                    "dist/register.js".to_string(),
                    "export function register(){ return 'ok'; }".to_string(),
                )],
            },
        );
        let state = RepositoryState::from_root(service_root);

        let response = state
            .read_plugin_archive_text(PluginArchiveReadRequest {
                plugin_id: "momobako.example.text-preview".to_string(),
                path: "dist/register.js".to_string(),
            })
            .expect("archive text should load from single-root package");

        assert_eq!(
            response.path,
            "momobako-example-text-preview-0.1.0/dist/register.js"
        );
        assert!(response.text.contains("register"));
    }

    #[test]
    fn runtime_builtin_plugins_keep_manifest_source_value() {
        let workspace = TestWorkspace::new("runtime-builtin-source");
        let service_root = workspace.path("service");
        let runtime_root = runtime_plugins_dir(&service_root);
        fs::create_dir_all(&runtime_root).expect("runtime plugin dir should be created");
        write_test_plugin_archive_with_options(
            &runtime_root.join("media-preview-1.0.0.momoplug"),
            TestPluginArchiveOptions {
                plugin_id: "momobako.preview.media",
                name: "Media Preview",
                source: "builtin",
                runtime: "manifest-only",
                sdk: "frontend",
                kind: "preview",
                plugin_type_layer: "library-kind",
                plugin_type_kind: "preview",
                entry: serde_json::json!({
                    "frontend": {
                        "module": "dist/register.js",
                        "export": "register"
                    }
                }),
                extra_files: vec![(
                    "dist/register.js".to_string(),
                    "export function register() {}".to_string(),
                )],
            },
        );

        let state = RepositoryState::from_root(service_root);
        let plugins = state.list_plugins().expect("plugins should load");
        let plugin = plugins
            .iter()
            .find(|item| item.plugin_id == "momobako.preview.media")
            .expect("media plugin should be listed");

        assert_eq!(plugin.source, "builtin");
    }

    #[test]
    fn install_plugin_from_archive_rejects_zip_extension() {
        let workspace = TestWorkspace::new("plugin-archive-zip");
        let service_root = workspace.path("service");
        let archive_path = workspace.path("sample-plugin.zip");
        write_test_plugin_archive(&archive_path, "user.sample-metadata");
        let state = RepositoryState::from_root(service_root);

        let error = state
            .install_plugin_from_archive(PluginInstallRequest {
                package_path: archive_path.to_string_lossy().to_string(),
            })
            .expect_err("zip extension should be rejected");

        assert!(error.contains(".momoplug extension"));
    }

    #[test]
    fn install_plugin_from_archive_rejects_root_level_manifest_packages() {
        let workspace = TestWorkspace::new("plugin-archive-root-manifest");
        let service_root = workspace.path("service");
        let archive_path = workspace.path("sample-plugin.momoplug");
        write_test_plugin_archive_without_root_dir(&archive_path, "user.sample-rootless");
        let state = RepositoryState::from_root(service_root);

        let error = state
            .install_plugin_from_archive(PluginInstallRequest {
                package_path: archive_path.to_string_lossy().to_string(),
            })
            .expect_err("root-level manifest package should be rejected");

        assert!(error.contains("exactly one root directory with manifest.json"));
    }

    #[test]
    fn install_plugin_from_archive_rejects_duplicate_plugin_id() {
        let workspace = TestWorkspace::new("plugin-archive-duplicate-id");
        let service_root = workspace.path("service");
        let first_archive = workspace.path("sample-plugin-a.momoplug");
        let second_archive = workspace.path("sample-plugin-b.momoplug");
        write_test_plugin_archive(&first_archive, "user.sample-duplicate");
        write_test_plugin_archive(&second_archive, "user.sample-duplicate");
        let state = RepositoryState::from_root(service_root);

        state
            .install_plugin_from_archive(PluginInstallRequest {
                package_path: first_archive.to_string_lossy().to_string(),
            })
            .expect("first plugin archive should install");

        let error = state
            .install_plugin_from_archive(PluginInstallRequest {
                package_path: second_archive.to_string_lossy().to_string(),
            })
            .expect_err("duplicate plugin id should be rejected");

        assert!(error.contains("plugin already exists: user.sample-duplicate"));
    }

    #[test]
    fn broken_plugin_archives_do_not_hide_other_runtime_plugins() {
        let workspace = TestWorkspace::new("broken-plugin-archive");
        let service_root = workspace.path("service");
        let runtime_root = runtime_plugins_dir(&service_root);
        fs::create_dir_all(&runtime_root).expect("runtime plugin dir should be created");
        write_test_plugin_archive(
            &runtime_root.join("good-plugin.momoplug"),
            "user.good-plugin",
        );
        fs::write(runtime_root.join("broken-plugin.momoplug"), b"not-a-zip")
            .expect("broken plugin archive should be written");

        let state = RepositoryState::from_root(service_root);
        let plugins = state.list_plugins().expect("plugins should load");

        let good = plugins
            .iter()
            .find(|plugin| plugin.plugin_id == "user.good-plugin")
            .expect("good plugin should still be listed");
        assert_eq!(good.status, "ready");

        let broken = plugins
            .iter()
            .find(|plugin| plugin.plugin_id == "broken.broken-plugin")
            .expect("broken plugin placeholder should be listed");
        assert!(!broken.enabled);
        assert!(matches!(broken.status.as_str(), "error" | "disabled"));
        assert!(broken.description.contains("Failed to read plugin archive"));
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
                    "sdk": "frontend",
                    "runtime": "vue-module",
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
                    "sdk": "frontend",
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
        if let (Some(base), Some(extra)) =
            (manifest.as_object_mut(), dependency_overrides.as_object())
        {
            for (key, value) in extra {
                base.insert(key.clone(), value.clone());
            }
        }
        write_test_plugin_archive_with_manifest(
            &plugin_root.join("local-filesystem.momoplug"),
            manifest,
        );
    }

    fn test_list_files_plugin_call(plugin_id: &str, repo_root: PathBuf) -> PluginCallRequest {
        PluginCallRequest {
            plugin_id: plugin_id.to_string(),
            method: "filesystem.listFiles".to_string(),
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

    #[test]
    fn disabled_manifest_only_backend_is_not_attachable() {
        let workspace = TestWorkspace::new("disabled-backend");
        let service_root = workspace.path("service");
        let runtime_root = runtime_plugins_dir(&service_root);
        write_test_plugin_archive_with_manifest(
            &runtime_root.join("webdav.momoplug"),
            test_plugin_manifest_json(
                TEST_WEBDAV_PLUGIN_ID,
                "WebDAV",
                serde_json::json!({
                    "kind": "webdav",
                    "category": "source",
                    "type": {
                        "layer": "source",
                        "kind": "webdav"
                    },
                    "capabilities": ["listFiles", "readFile", "writeFile"],
                    "enabled": false,
                    "sdk": "backend",
                    "runtime": "manifest-only",
                    "source": "system"
                }),
            ),
        );
        let state = RepositoryState::from_root(service_root);
        let repo_root = workspace.path("repo");

        let error = state
            .create_repository(RepositoryMutationRequest {
                repo_id: Some("repo-webdav".to_string()),
                name: "WebDAV Repo".to_string(),
                path: repo_root.to_string_lossy().to_string(),
                backend_plugin_id: Some(TEST_WEBDAV_PLUGIN_ID.to_string()),
                backend_config: None,
                skip_initial_sync: false,
            })
            .expect_err("disabled manifest-only backend should not create a repository");

        assert!(
            error.contains("plugin is disabled")
                || error.contains("plugin runtime is not available")
        );
    }

    #[test]
    fn local_filesystem_backend_without_native_runtime_is_rejected() {
        let workspace = TestWorkspace::new("runtime-local-backend");
        let service_root = workspace.path("service");
        let repo_root = workspace.path("repo");
        install_local_filesystem_test_plugin_archive(&service_root);
        let state = RepositoryState::from_root(service_root);

        let error = state
            .create_repository(RepositoryMutationRequest {
                repo_id: Some("repo-runtime".to_string()),
                name: "Runtime Repo".to_string(),
                path: repo_root.to_string_lossy().to_string(),
                backend_plugin_id: Some(LOCAL_FILESYSTEM_PLUGIN_ID.to_string()),
                backend_config: None,
                skip_initial_sync: false,
            })
            .expect_err("local filesystem backend should require native runtime");

        assert!(error.contains(LOCAL_FILESYSTEM_PLUGIN_ID));
        assert!(error.contains("plugin runtime is not available"));
    }

    #[test]
    fn custom_system_filesystem_backend_without_native_runtime_is_rejected() {
        let workspace = TestWorkspace::new("runtime-custom-local-backend");
        let service_root = workspace.path("service");
        let repo_root = workspace.path("repo");
        let plugin_id = "user.custom-local-filesystem";
        let runtime_root = runtime_plugins_dir(&service_root);
        write_test_plugin_archive_with_manifest(
            &runtime_root.join("custom-local-filesystem.momoplug"),
            test_plugin_manifest_json(
                plugin_id,
                "Custom Local Filesystem",
                serde_json::json!({
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
        let state = RepositoryState::from_root(service_root);

        let error = state
            .create_repository(RepositoryMutationRequest {
                repo_id: Some("repo-runtime-custom".to_string()),
                name: "Runtime Custom Repo".to_string(),
                path: repo_root.to_string_lossy().to_string(),
                backend_plugin_id: Some(plugin_id.to_string()),
                backend_config: None,
                skip_initial_sync: false,
            })
            .expect_err("custom filesystem backend should require native runtime");

        assert!(error.contains(plugin_id));
        assert!(error.contains("plugin runtime is not available"));
    }

    #[test]
    fn create_repository_without_backend_plugin_id_infers_custom_local_root_backend() {
        let workspace = TestWorkspace::new("infer-custom-local-backend");
        let service_root = workspace.path("service");
        let repo_root = workspace.path("repo");
        let runtime_root = runtime_plugins_dir(&service_root);
        let plugin_id = "user.custom-local-filesystem";
        write_test_plugin_archive_with_manifest(
            &runtime_root.join("custom-local-filesystem.momoplug"),
            test_plugin_manifest_json(
                plugin_id,
                "Custom Local Filesystem",
                serde_json::json!({
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
        let state = RepositoryState::from_root(service_root);

        let error = state
            .create_repository(RepositoryMutationRequest {
                repo_id: Some("repo-infer-custom".to_string()),
                name: "Infer Custom Repo".to_string(),
                path: repo_root.to_string_lossy().to_string(),
                backend_plugin_id: None,
                backend_config: None,
                skip_initial_sync: false,
            })
            .expect_err("custom local backend should be inferred before runtime validation");

        assert!(error.contains(plugin_id));
        assert!(error.contains("plugin runtime is not available"));
    }

    #[test]
    fn create_repository_without_backend_plugin_id_errors_without_local_root_backend() {
        let workspace = TestWorkspace::new("infer-local-backend-missing");
        let service_root = workspace.path("service");
        let repo_root = workspace.path("repo");
        install_netease_source_test_plugin_archive(&service_root);
        let state = RepositoryState::from_root(service_root);

        let error = state
            .create_repository(RepositoryMutationRequest {
                repo_id: Some("repo-missing-local-backend".to_string()),
                name: "Missing Local Backend".to_string(),
                path: repo_root.to_string_lossy().to_string(),
                backend_plugin_id: None,
                backend_config: None,
                skip_initial_sync: false,
            })
            .expect_err("missing local-root backend should fail clearly");

        assert!(error.contains("supports local repository roots"));
    }

    #[test]
    fn update_repository_backend_config_persists_registry_and_repository_metadata() {
        let (state, root, repo_root, _thumbnail_root) =
            create_test_state("update-repo-backend-config");
        let repo_id = create_repository_for_path(&state, &repo_root);

        let response = state
            .update_repository_backend_config(RepositoryBackendConfigUpdateRequest {
                repo_id: repo_id.clone(),
                backend_config: serde_json::json!({
                    "cookie": "MUSIC_U=updated-cookie",
                    "accountId": "123456"
                }),
            })
            .expect("repository backend config should update");

        assert_eq!(response.repository.repo_id, repo_id);

        let repo = state
            .load_repository_record(&repo_id)
            .expect("repository record should load");
        assert_eq!(
            repo.backend_record.config,
            serde_json::json!({
                "cookie": "MUSIC_U=updated-cookie",
                "accountId": "123456"
            })
        );

        let metadata_path = repository_meta_dir(&repo_root).join(REPO_METADATA_FILE_NAME);
        let metadata_raw =
            fs::read_to_string(metadata_path).expect("repository metadata should exist");
        let metadata: RepositoryMetadataFileImport =
            serde_json::from_str(&metadata_raw).expect("repository metadata should decode");
        assert_eq!(
            metadata.backend_config,
            Some(serde_json::json!({
                "cookie": "MUSIC_U=updated-cookie",
                "accountId": "123456"
            }))
        );

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn list_repositories_marks_missing_local_paths() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("missing-repo-list");
        let repo_id = create_repository_for_path(&state, &repo_root);
        fs::remove_dir_all(&repo_root).expect("repo root should be removed");

        let repositories = state
            .list_repositories()
            .expect("repositories should list even when path is missing");
        let repository = repositories
            .iter()
            .find(|item| item.repo_id == repo_id)
            .expect("missing repository should stay registered");

        assert_eq!(repository.status, "missing");
        assert_eq!(repository.asset_count, 0);
        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn relocate_repository_requires_matching_metadata_repo_id() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("relocate-mismatch");
        let repo_id = create_repository_for_path(&state, &repo_root);
        let other_root = root.join("other-repo");
        let other_meta_dir = other_root.join(REPO_META_DIR);
        fs::create_dir_all(&other_meta_dir).expect("other metadata dir should be created");
        fs::write(
            other_meta_dir.join(REPO_METADATA_FILE_NAME),
            serde_json::to_string_pretty(&serde_json::json!({
                "repoId": "repo-other",
                "name": "Other Repo",
                "rootPath": other_root.to_string_lossy(),
                "backendPluginId": LOCAL_FILESYSTEM_PLUGIN_ID,
                "backendConfig": {},
                "createdAt": now_rfc3339(),
                "schemaVersion": REPO_SCHEMA_VERSION,
            }))
            .expect("metadata json should encode"),
        )
        .expect("other metadata should be written");

        let error = state
            .relocate_repository(RepositoryRelocateRequest {
                repo_id,
                path: other_root.to_string_lossy().to_string(),
            })
            .expect_err("mismatched metadata repo id should fail");

        assert!(error.contains("different repository"));
        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn relocate_repository_updates_path_and_preserves_repo_id() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("relocate-success");
        let repo_id = create_repository_for_path(&state, &repo_root);
        state
            .create_smart_folder(SmartFolderMutationRequest {
                repo_id: repo_id.clone(),
                smart_folder_id: Some("sf-reference".to_string()),
                parent_id: None,
                name: "Reference".to_string(),
                filter: SmartFolderFilter {
                    path_prefix: Some("Reference".to_string()),
                    ..SmartFolderFilter::default()
                },
            })
            .expect("smart folder should be created before relocation");
        let relocated_root = root.join("relocated-repo");
        fs::rename(&repo_root, &relocated_root).expect("repo root should move");

        let missing = state
            .list_repositories()
            .expect("repositories should list")
            .into_iter()
            .find(|item| item.repo_id == repo_id)
            .expect("repository should stay registered");
        assert_eq!(missing.status, "missing");

        let response = state
            .relocate_repository(RepositoryRelocateRequest {
                repo_id: repo_id.clone(),
                path: relocated_root.to_string_lossy().to_string(),
            })
            .expect("relocation should succeed");

        assert_eq!(response.repository.repo_id, repo_id);
        assert_eq!(
            PathBuf::from(&response.repository.path),
            canonicalize_local_path(&relocated_root).expect("relocated root should canonicalize")
        );
        let ready = state
            .list_repositories()
            .expect("repositories should list after relocation")
            .into_iter()
            .find(|item| item.repo_id == response.repository.repo_id)
            .expect("repository should still be registered");
        assert_eq!(ready.status, "ready");
        let raw_metadata = fs::read_to_string(
            relocated_root
                .join(REPO_META_DIR)
                .join(REPO_METADATA_FILE_NAME),
        )
        .expect("relocated metadata should read");
        let metadata: RepositoryMetadataFileImport =
            serde_json::from_str(&raw_metadata).expect("relocated metadata should parse");
        let expected_root_path = relocated_root.to_string_lossy().to_string();
        assert_eq!(metadata.repo_id, repo_id);
        assert_eq!(
            metadata.root_path.as_deref(),
            Some(expected_root_path.as_str())
        );
        let smart_folders = state
            .list_smart_folders(&response.repository.repo_id)
            .expect("smart folders should load after relocation");
        assert_eq!(smart_folders.len(), 1);
        assert_eq!(smart_folders[0].folder.smart_folder_id, "sf-reference");
        assert_eq!(
            smart_folders[0].folder.filter.path_prefix.as_deref(),
            Some("Reference")
        );
        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn delete_repository_removes_registry_and_managed_state_dir() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("delete-repo-state");
        let repo_id = create_repository_for_path(&state, &repo_root);
        let managed_state_dir = repository_state_storage_dir(&state.root, &repo_id);
        fs::create_dir_all(managed_state_dir.join("cache"))
            .expect("managed state dir should be created");
        fs::write(managed_state_dir.join("cache/index.json"), "{}")
            .expect("managed cache file should be written");

        state
            .delete_repository(&repo_id)
            .expect("repository should delete");

        assert!(!managed_state_dir.exists());
        assert!(repo_root.exists());
        assert!(state
            .list_repositories()
            .expect("repositories should list after delete")
            .iter()
            .all(|item| item.repo_id != repo_id));
        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn record_entry_access_keeps_only_latest_50_entries() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("recent-access-cap");
        let repo_id = create_repository_without_initial_sync(&state, &repo_root);
        for index in 0..55 {
            fs::write(repo_root.join(format!("recent-{index:02}.txt")), format!("entry-{index}"))
                .expect("test file should be written");
        }
        state
            .sync_repository(SyncRequest {
                repo_id: repo_id.clone(),
            })
            .expect("repository should sync");

        for index in 0..55 {
            state
                .record_entry_access(EntryAccessRecordRequest {
                    repo_id: repo_id.clone(),
                    path: format!("recent-{index:02}.txt"),
                })
                .expect("entry access should record");
        }

        let repo = state
            .load_repository_record(&repo_id)
            .expect("repository record should load");
        let connection = state
            .open_repository_connection(
                &repo.summary.repo_id,
                &repo.summary.path,
                &repo.backend_record,
            )
            .expect("repository connection should open");
        let recent_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM assets WHERE repo_id = ?1 AND last_accessed_at IS NOT NULL",
                params![&repo_id],
                |row| row.get(0),
            )
            .expect("recent access count should query");
        let trimmed_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM assets WHERE repo_id = ?1 AND last_accessed_at IS NULL",
                params![&repo_id],
                |row| row.get(0),
            )
            .expect("trimmed access count should query");
        let latest_access: Option<String> = connection
            .query_row(
                "SELECT last_accessed_at FROM assets WHERE repo_id = ?1 AND path = ?2",
                params![&repo_id, "recent-54.txt"],
                |row| row.get(0),
            )
            .expect("latest access should query");

        assert_eq!(recent_count, 50);
        assert_eq!(trimmed_count, 5);
        assert!(latest_access.is_some());
        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn clear_recent_access_history_resets_all_last_accessed_at() {
        let (state, root, repo_root, _thumbnail_root) =
            create_test_state("recent-access-clear");
        let repo_id = create_repository_without_initial_sync(&state, &repo_root);
        for path in ["alpha.txt", "beta.txt"] {
            fs::write(repo_root.join(path), path).expect("test file should be written");
        }
        state
            .sync_repository(SyncRequest {
                repo_id: repo_id.clone(),
            })
            .expect("repository should sync");
        for path in ["alpha.txt", "beta.txt"] {
            state
                .record_entry_access(EntryAccessRecordRequest {
                    repo_id: repo_id.clone(),
                    path: path.to_string(),
                })
                .expect("entry access should record");
        }

        let response = state
            .clear_recent_access_history(RecentAccessHistoryClearRequest {
                repo_id: repo_id.clone(),
            })
            .expect("recent access history should clear");

        let repo = state
            .load_repository_record(&repo_id)
            .expect("repository record should load");
        let connection = state
            .open_repository_connection(
                &repo.summary.repo_id,
                &repo.summary.path,
                &repo.backend_record,
            )
            .expect("repository connection should open");
        let remaining_recent_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM assets WHERE repo_id = ?1 AND last_accessed_at IS NOT NULL",
                params![&repo_id],
                |row| row.get(0),
            )
            .expect("remaining recent access count should query");

        assert_eq!(response.repo_id, repo_id);
        assert_eq!(response.cleared_count, 2);
        assert_eq!(remaining_recent_count, 0);
        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn repository_tree_reports_direct_file_counts_per_directory() {
        let (state, root, repo_root, _thumbnail_root) =
            create_test_state("repository-tree-file-count");
        fs::create_dir_all(repo_root.join("Campaigns/Summer"))
            .expect("summer directory should be created");
        fs::create_dir_all(repo_root.join("Campaigns/Winter"))
            .expect("winter directory should be created");
        fs::write(repo_root.join("Campaigns/brief.txt"), "brief")
            .expect("campaign file should be written");
        fs::write(repo_root.join("Campaigns/Summer/cover.psd"), "cover")
            .expect("summer cover should be written");
        fs::write(repo_root.join("Campaigns/Summer/thumb.png"), "thumb")
            .expect("summer thumbnail should be written");
        fs::write(repo_root.join("Campaigns/Winter/scene.png"), "scene")
            .expect("winter scene should be written");
        let repo_id = create_repository_without_initial_sync(&state, &repo_root);
        state
            .sync_repository(SyncRequest {
                repo_id: repo_id.clone(),
            })
            .expect("repository should sync");

        let snapshot = state
            .load_repository_tree(&repo_id)
            .expect("repository tree should load");
        let campaigns = snapshot
            .tree
            .iter()
            .find(|node| node.path == "Campaigns")
            .expect("campaigns node should exist");
        let summer = campaigns
            .children
            .iter()
            .find(|node| node.path == "Campaigns/Summer")
            .expect("summer node should exist");
        let winter = campaigns
            .children
            .iter()
            .find(|node| node.path == "Campaigns/Winter")
            .expect("winter node should exist");

        assert_eq!(campaigns.file_count, 1);
        assert_eq!(summer.file_count, 2);
        assert_eq!(winter.file_count, 1);
        fs::remove_dir_all(root).expect("test temp root should be removed");
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

    fn create_local_repository_record_for_external_tests(
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
        let runtime_plugin_root = runtime_plugins_dir(&state.root);
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
                "source": "system"
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
        let connection = Connection::open(metadata_dir.join(REPO_DB_FILE_NAME))
            .expect("repository db should open");
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

    #[test]
    fn backend_discovered_file_falls_back_to_repository_path_when_absolute_path_is_missing() {
        let workspace = TestWorkspace::new("backend-discovered-file-compat");
        let repo_root = workspace.path("repo");
        fs::create_dir_all(repo_root.join("notes")).expect("repo directory should be created");

        let raw = serde_json::json!({
            "relativePath": "notes/today.txt",
            "filename": "today.txt",
            "extension": "txt",
            "sizeBytes": 12,
            "modifiedAt": "2026-06-09T00:00:00Z"
        });
        let file = serde_json::from_value::<BackendDiscoveredFile>(raw)
            .expect("legacy plugin file payload should decode")
            .into_discovered_file(&repo_root)
            .expect("legacy plugin file payload should normalize");

        assert_eq!(file.relative_path, "notes/today.txt");
        assert_eq!(
            file.absolute_path,
            Some(repo_root.join("notes").join("today.txt"))
        );
    }

    #[test]
    fn filesystem_entry_decodes_legacy_payload_without_virtual_fields() {
        let raw = serde_json::json!({
            "path": "Albums",
            "name": "Albums",
            "kind": "directory",
            "modifiedAt": "2026-06-09T00:00:00Z"
        });
        let entry = serde_json::from_value::<FileSystemEntry>(raw)
            .expect("legacy filesystem entry payload should decode");

        assert!(matches!(entry.kind, FileSystemEntryKind::Directory));
        assert!(!entry.is_virtual);
        assert_eq!(entry.provider_id, None);
        assert_eq!(entry.provider_item_id, None);
        assert_eq!(entry.source_payload, None);
        assert_eq!(entry.local_absolute_path, None);
    }

    #[test]
    fn sync_repository_indexes_assets_without_generating_thumbnails() {
        let (state, root, repo_root, thumbnail_root) = create_test_state("sync-no-thumb");
        write_test_image(&repo_root.join("cover.png"));

        let repo_id = create_repository_for_path(&state, &repo_root);
        let snapshot = state
            .load_snapshot(&repo_id)
            .expect("snapshot should load after sync");

        assert_eq!(snapshot.assets.len(), 1);
        assert_eq!(snapshot.assets[0].thumbnail_path, None);
        assert_eq!(count_files(&thumbnail_root), 0);

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn load_file_browser_returns_generic_file_metadata() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("browser-metadata");
        fs::write(repo_root.join("note.txt"), "plain text").expect("test file should be written");
        let repo_id = create_repository_for_path(&state, &repo_root);

        let snapshot = state
            .load_file_browser(FileBrowserRequest {
                repo_id,
                directory_path: Some(String::new()),
                include_tree: Some(false),
                special_location: None,
                offset: None,
                limit: None,
            })
            .expect("file browser should load");
        let entry = snapshot
            .entries
            .iter()
            .find(|item| item.path == "note.txt")
            .expect("file entry should be listed");

        assert_eq!(entry.metadata.get("rating"), Some(&serde_json::json!(0)));
        assert_eq!(entry.metadata.get("comment"), Some(&serde_json::json!("")));
        assert_eq!(entry.metadata.get("link"), Some(&serde_json::json!("")));
        assert_eq!(
            entry.metadata.get("tagGroups"),
            Some(&serde_json::json!([]))
        );
        assert!(entry
            .metadata
            .get("addedToLibraryAt")
            .and_then(serde_json::Value::as_str)
            .is_some());

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn sync_mirrored_source_metadata_uses_plugin_manifest_rules_without_touching_user_fields() {
        let (state, root, repo_root, _thumbnail_root) =
            create_test_state("netease-system-metadata");
        let repo_id = create_netease_repository_without_initial_sync(
            &state,
            &repo_root,
            serde_json::json!({
                "accountId": "123456",
                "cookie": "MUSIC_U=test"
            }),
        );
        let repo = state
            .load_repository_record(&repo_id)
            .expect("repository record should load");
        let mut connection = state
            .open_repository_connection(
                &repo.summary.repo_id,
                &repo.summary.path,
                &repo.backend_record,
            )
            .expect("repository connection should open");
        let tx = connection.transaction().expect("transaction should start");
        let asset_id = asset_id_for_path(&repo_id, "创建的歌单/分页歌单/歌手 A - 第一页.mp3");
        tx.execute(
            r#"
            INSERT INTO assets (
              asset_id, repo_id, path, filename, extension, size_bytes, created_at, modified_at,
              hash, status, version, updated_at, thumbnail_path, is_virtual, provider_id,
              provider_item_id, source_payload_json, local_absolute_path
            )
            VALUES (?1, ?2, ?3, ?4, 'mp3', 0, ?5, ?5, NULL, 'synced', 1, ?5, NULL, 1, ?6, ?7, ?8, NULL)
            "#,
            params![
                &asset_id,
                &repo_id,
                "创建的歌单/分页歌单/歌手 A - 第一页.mp3",
                "歌手 A - 第一页.mp3",
                now_rfc3339(),
                NETEASE_CLOUD_MUSIC_PROVIDER_ID,
                "3301",
                serde_json::json!({
                    "provider": "netease-cloud-music",
                    "accountId": "123456",
                    "playlistId": 9201,
                    "playlistName": "分页歌单",
                    "playlistCategory": "created",
                    "songId": 3301,
                    "songName": "第一页",
                    "artists": ["歌手 A"],
                    "albumName": "专辑 A",
                    "coverUrl": "https://example.test/cover-3301.jpg",
                    "durationMs": 180000
                }).to_string()
            ],
        )
        .expect("virtual asset should insert");
        ensure_default_metadata(
            &tx,
            &asset_id,
            "创建的歌单/分页歌单/歌手 A - 第一页.mp3",
            "歌手 A - 第一页.mp3",
            "mp3",
            &now_rfc3339(),
            None,
            &[],
            None,
            false,
        )
        .expect("default metadata should seed");
        upsert_metadata_value(&tx, &asset_id, "rating", &serde_json::json!(4))
            .expect("user rating should update");
        let metadata_keys =
            source_metadata_mirror_keys(&state.root, &repo.backend_record.plugin_id);
        sync_mirrored_source_metadata(
            &tx,
            &asset_id,
            Some(&serde_json::json!({
                "provider": "netease-cloud-music",
                "accountId": "123456",
                "playlistId": 9201,
                "playlistName": "分页歌单",
                "playlistCategory": "created",
                "songId": 3301,
                "songName": "第一页",
                "artists": ["歌手 A"],
                "albumName": "专辑 A",
                "coverUrl": "https://example.test/cover-3301.jpg",
                "durationMs": 180000
            })),
            &metadata_keys,
        )
        .expect("mirrored metadata should sync");
        tx.commit().expect("transaction should commit");

        let metadata =
            metadata_for_asset_path(&state, &repo_id, "创建的歌单/分页歌单/歌手 A - 第一页.mp3");
        assert_eq!(metadata.get("rating"), Some(&serde_json::json!(4)));
        assert_eq!(metadata.get("songId"), Some(&serde_json::json!(3301)));
        assert_eq!(metadata.get("songName"), Some(&serde_json::json!("第一页")));
        assert_eq!(
            metadata.get("artists"),
            Some(&serde_json::json!(["歌手 A"]))
        );
        assert_eq!(
            metadata.get("coverUrl"),
            Some(&serde_json::json!("https://example.test/cover-3301.jpg"))
        );

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn load_file_browser_uses_netease_cached_page_without_backend_roundtrip() {
        let (state, root, repo_root, _thumbnail_root) =
            create_test_state("netease-browser-cache-hit");
        let repo_id = create_netease_repository_without_initial_sync(
            &state,
            &repo_root,
            serde_json::json!({
                "accountId": "123456",
                "cookie": "MUSIC_U=test"
            }),
        );
        let repo = state
            .load_repository_record(&repo_id)
            .expect("repository record should load");
        let connection = state
            .open_repository_connection(
                &repo.summary.repo_id,
                &repo.summary.path,
                &repo.backend_record,
            )
            .expect("repository connection should open");
        let refreshed_at = now_rfc3339();
        let entries = vec![
            FileSystemEntry {
                path: "创建的歌单/分页歌单/歌手 A - 第一页.mp3".to_string(),
                name: "歌手 A - 第一页.mp3".to_string(),
                kind: FileSystemEntryKind::File,
                extension: Some("mp3".to_string()),
                size_bytes: Some(0),
                modified_at: Some(refreshed_at.clone()),
                is_virtual: true,
                provider_id: Some(NETEASE_CLOUD_MUSIC_PROVIDER_ID.to_string()),
                provider_item_id: Some("3301".to_string()),
                source_payload: Some(serde_json::json!({
                    "provider": "netease-cloud-music",
                    "accountId": "123456",
                    "playlistId": 9201,
                    "playlistName": "分页歌单",
                    "playlistCategory": "created",
                    "songId": 3301,
                    "songName": "第一页",
                    "artists": ["歌手 A"],
                    "albumName": "专辑 A",
                    "coverUrl": "https://example.test/cover-3301.jpg",
                    "durationMs": 180000
                })),
                local_absolute_path: None,
            },
            FileSystemEntry {
                path: "创建的歌单/分页歌单/歌手 B - 第二页前.mp3".to_string(),
                name: "歌手 B - 第二页前.mp3".to_string(),
                kind: FileSystemEntryKind::File,
                extension: Some("mp3".to_string()),
                size_bytes: Some(0),
                modified_at: Some(refreshed_at.clone()),
                is_virtual: true,
                provider_id: Some(NETEASE_CLOUD_MUSIC_PROVIDER_ID.to_string()),
                provider_item_id: Some("3302".to_string()),
                source_payload: Some(serde_json::json!({
                    "provider": "netease-cloud-music",
                    "accountId": "123456",
                    "playlistId": 9201,
                    "playlistName": "分页歌单",
                    "playlistCategory": "created",
                    "songId": 3302,
                    "songName": "第二页前",
                    "artists": ["歌手 B"],
                    "albumName": "专辑 B",
                    "coverUrl": "https://example.test/cover-3302.jpg",
                    "durationMs": 190000
                })),
                local_absolute_path: None,
            },
        ];
        replace_netease_directory_cache_page(
            &connection,
            &repo_id,
            "创建的歌单/分页歌单",
            0,
            &entries,
            3,
            &refreshed_at,
        )
        .expect("directory cache page should persist");

        let snapshot = state
            .load_file_browser(FileBrowserRequest {
                repo_id,
                directory_path: Some("创建的歌单/分页歌单".to_string()),
                include_tree: Some(false),
                special_location: None,
                offset: Some(0),
                limit: Some(2),
            })
            .expect("netease cached browser page should load");

        assert_eq!(snapshot.entries.len(), 2);
        assert_eq!(snapshot.total_entries, 3);
        assert_eq!(snapshot.loaded_count, 2);
        assert_eq!(snapshot.next_offset, Some(2));
        assert!(snapshot.has_more);
        assert_eq!(
            snapshot.entries[0]
                .source_payload
                .as_ref()
                .and_then(|value| value.get("songId")),
            Some(&serde_json::json!(3301))
        );
        assert!(snapshot.entries[0].asset_id.is_some());

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn plugin_metadata_defaults_preserve_existing_values() {
        let (state, root, repo_root, _thumbnail_root) =
            create_test_state("plugin-metadata-defaults");
        fs::write(repo_root.join("note.txt"), "hello").expect("test file should be written");

        let repo_id = create_repository_for_path(&state, &repo_root);
        let repo = state
            .load_repository_record(&repo_id)
            .expect("repository record should load");
        let mut connection = state
            .open_repository_connection(
                &repo.summary.repo_id,
                &repo.summary.path,
                &repo.backend_record,
            )
            .expect("repository connection should open");
        let tx = connection
            .transaction()
            .expect("metadata transaction should start");
        let asset_id = tx
            .query_row(
                "SELECT asset_id FROM assets WHERE repo_id = ?1 AND path = 'note.txt'",
                [&repo_id],
                |row| row.get::<_, String>(0),
            )
            .expect("asset should exist");
        upsert_metadata_value(&tx, &asset_id, "title", &serde_json::json!("User Title"))
            .expect("existing title should update");
        let plugin_defaults = BTreeMap::from([
            ("title".to_string(), serde_json::json!("Plugin Title")),
            (
                "pluginDefault".to_string(),
                serde_json::json!("Plugin Value"),
            ),
        ]);
        ensure_default_metadata(
            &tx,
            &asset_id,
            "note.txt",
            "note.txt",
            "txt",
            &now_rfc3339(),
            None,
            &[],
            Some(&plugin_defaults),
            false,
        )
        .expect("plugin defaults should merge");
        tx.commit().expect("metadata transaction should commit");

        let metadata = metadata_for_asset_path(&state, &repo_id, "note.txt");
        assert_eq!(
            metadata.get("title"),
            Some(&serde_json::json!("User Title"))
        );
        assert_eq!(
            metadata.get("pluginDefault"),
            Some(&serde_json::json!("Plugin Value"))
        );
        drop(connection);

        fs::write(repo_root.join("second.txt"), "second")
            .expect("second test file should be written");
        state
            .sync_repository(SyncRequest {
                repo_id: repo_id.clone(),
            })
            .expect("repository should resync");
        let repo = state
            .load_repository_record(&repo_id)
            .expect("repository record should reload");
        let mut second_connection = state
            .open_repository_connection(
                &repo.summary.repo_id,
                &repo.summary.path,
                &repo.backend_record,
            )
            .expect("repository connection should reopen");
        let tx = second_connection
            .transaction()
            .expect("second metadata transaction should start");
        let second_asset_id = tx
            .query_row(
                "SELECT asset_id FROM assets WHERE repo_id = ?1 AND path = 'second.txt'",
                [&repo_id],
                |row| row.get::<_, String>(0),
            )
            .expect("second asset should exist");
        ensure_default_metadata(
            &tx,
            &second_asset_id,
            "second.txt",
            "second.txt",
            "txt",
            &now_rfc3339(),
            None,
            &[],
            Some(&plugin_defaults),
            true,
        )
        .expect("new asset plugin defaults should merge without replacing host defaults");
        tx.commit()
            .expect("second metadata transaction should commit");
        let second_metadata = metadata_for_asset_path(&state, &repo_id, "second.txt");
        assert_eq!(
            second_metadata.get("title"),
            Some(&serde_json::json!("second.txt"))
        );
        assert_eq!(
            second_metadata.get("pluginDefault"),
            Some(&serde_json::json!("Plugin Value"))
        );

        drop(second_connection);
        drop(state);
        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn sync_repository_extracts_palette_metadata_for_new_images() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("sync-palette");
        write_test_palette_image(&repo_root.join("cover.png"));

        let repo_id = create_repository_for_path(&state, &repo_root);
        let metadata = metadata_for_asset_path(&state, &repo_id, "cover.png");

        assert_eq!(
            metadata.get("color"),
            Some(&serde_json::Value::String("#D2281E".to_string()))
        );
        assert_eq!(
            metadata.get("palette"),
            Some(&serde_json::json!(["#D2281E", "#28B45A", "#1450C8"]))
        );

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn sync_repository_skips_palette_metadata_for_non_images() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("sync-no-palette");
        fs::write(repo_root.join("note.txt"), "plain text").expect("text file should be written");

        let repo_id = create_repository_for_path(&state, &repo_root);
        let metadata = metadata_for_asset_path(&state, &repo_id, "note.txt");

        assert_eq!(metadata.get("color"), None);
        assert_eq!(metadata.get("palette"), None);

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn sync_repository_ignores_broken_images_when_extracting_palette() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("sync-broken-palette");
        fs::write(repo_root.join("broken.png"), b"not an image")
            .expect("broken image should be written");

        let repo_id = create_repository_for_path(&state, &repo_root);
        let metadata = metadata_for_asset_path(&state, &repo_id, "broken.png");

        assert_eq!(metadata.get("color"), None);
        assert_eq!(metadata.get("palette"), None);

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn sync_repository_generates_unique_asset_ids_for_slug_collisions() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("sync-asset-id");
        fs::create_dir_all(repo_root.join("A B")).expect("spaced directory should be created");
        fs::create_dir_all(repo_root.join("A-B")).expect("hyphen directory should be created");
        fs::write(repo_root.join("A B").join("cover.png"), "first")
            .expect("first file should be written");
        fs::write(repo_root.join("A-B").join("cover.png"), "second")
            .expect("second file should be written");

        let repo_id = create_repository_for_path(&state, &repo_root);
        let snapshot = state
            .load_snapshot(&repo_id)
            .expect("snapshot should load after sync");
        let asset_ids = snapshot
            .assets
            .iter()
            .map(|asset| asset.asset_id.as_str())
            .collect::<std::collections::BTreeSet<_>>();

        assert_eq!(snapshot.assets.len(), 2);
        assert_eq!(asset_ids.len(), 2);
        assert!(asset_ids
            .iter()
            .all(|asset_id| asset_id.starts_with("asset-") && asset_id.len() == 70));

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn sync_repository_stores_real_content_hash_and_candidates() {
        let (state, root, repo_root, _thumbnail_root) =
            create_test_state("sync-hardlink-candidate");
        fs::write(repo_root.join("source.txt"), b"same bytes")
            .expect("source file should be written");
        let repo_id = create_repository_for_path(&state, &repo_root);
        fs::write(repo_root.join("copy.txt"), b"same bytes").expect("copy file should be written");

        let result = state
            .sync_repository(SyncRequest {
                repo_id: repo_id.clone(),
            })
            .expect("sync should complete");
        assert_eq!(result.hardlink_candidates, 1);

        let connection = state
            .open_repository_connection(
                &repo_id,
                &repo_root.to_string_lossy(),
                &RepositoryBackendRecord {
                    plugin_id: LOCAL_FILESYSTEM_PLUGIN_ID.to_string(),
                    config: serde_json::json!({}),
                },
            )
            .expect("repository connection should open");
        let hash: String = connection
            .query_row(
                "SELECT hash FROM assets WHERE repo_id = ?1 AND path = 'source.txt'",
                [repo_id.as_str()],
                |row| row.get(0),
            )
            .expect("hash should load");
        assert!(is_content_hash(&hash));

        let candidates = state
            .list_hardlink_candidates(&repo_id)
            .expect("candidates should load");
        assert_eq!(candidates.candidates.len(), 1);

        drop(connection);
        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn search_assets_filters_current_repository_metadata_and_formats() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("search-filters");
        fs::write(repo_root.join("cover.psd"), b"cover").expect("cover file should be written");
        fs::write(repo_root.join("alt.psd"), b"alternate").expect("alt file should be written");
        fs::write(repo_root.join("icon.png"), b"icon").expect("icon file should be written");
        fs::write(repo_root.join("deleted.psd"), b"deleted")
            .expect("deleted file should be written");
        let repo_id = create_repository_for_path(&state, &repo_root);

        let repo = state
            .load_repository_record(&repo_id)
            .expect("repository record should load");
        let connection = state
            .open_repository_connection(
                &repo.summary.repo_id,
                &repo.summary.path,
                &repo.backend_record,
            )
            .expect("repository connection should open");
        let now = now_rfc3339();
        let cover_asset_id: String = connection
            .query_row(
                "SELECT asset_id FROM assets WHERE repo_id = ?1 AND path = 'cover.psd'",
                [repo_id.as_str()],
                |row| row.get(0),
            )
            .expect("cover asset id should load");
        let alt_asset_id: String = connection
            .query_row(
                "SELECT asset_id FROM assets WHERE repo_id = ?1 AND path = 'alt.psd'",
                [repo_id.as_str()],
                |row| row.get(0),
            )
            .expect("alt asset id should load");
        let deleted_asset_id: String = connection
            .query_row(
                "SELECT asset_id FROM assets WHERE repo_id = ?1 AND path = 'deleted.psd'",
                [repo_id.as_str()],
                |row| row.get(0),
            )
            .expect("deleted asset id should load");

        for (asset_id, key, value) in [
            (&cover_asset_id, "color", serde_json::json!("红色")),
            (&cover_asset_id, "shape", serde_json::json!("方形")),
            (&cover_asset_id, "rating", serde_json::json!(5)),
            (&alt_asset_id, "color", serde_json::json!("蓝色")),
            (&alt_asset_id, "shape", serde_json::json!("圆形")),
            (&alt_asset_id, "rating", serde_json::json!(2)),
            (&deleted_asset_id, "color", serde_json::json!("红色")),
            (&deleted_asset_id, "shape", serde_json::json!("方形")),
            (&deleted_asset_id, "rating", serde_json::json!(5)),
        ] {
            connection
                .execute(
                    r#"
                    INSERT OR REPLACE INTO metadata (asset_id, key, value_type, value_json, version, updated_at)
                    VALUES (?1, ?2, ?3, ?4, 1, ?5)
                    "#,
                    params![asset_id, key, infer_value_type(&value), value.to_string(), now],
                )
                .expect("metadata should be written");
        }
        for (asset_id, tag) in [(&cover_asset_id, "封面"), (&alt_asset_id, "草稿")] {
            connection
                .execute(
                    r#"
                    INSERT OR REPLACE INTO tags (asset_id, tag, normalized_tag)
                    VALUES (?1, ?2, ?3)
                    "#,
                    params![asset_id, tag, tag.to_lowercase()],
                )
                .expect("tag should be written");
        }
        connection
            .execute(
                "UPDATE assets SET status = 'deleted' WHERE repo_id = ?1 AND path = 'deleted.psd'",
                [repo_id.as_str()],
            )
            .expect("deleted asset should be marked");
        drop(connection);

        let response = state
            .search_assets(SearchRequest {
                query: String::new(),
                repo_id: Some(repo_id.clone()),
                exclude_query: None,
                metadata_key: None,
                metadata_value: None,
                tag: None,
                tags: Some(vec!["封面".to_string()]),
                metadata_filters: Some(vec![
                    SearchMetadataFilter {
                        key: "color".to_string(),
                        value: "红色".to_string(),
                    },
                    SearchMetadataFilter {
                        key: "shape".to_string(),
                        value: "方形".to_string(),
                    },
                ]),
                formats: Some(vec!["psd".to_string(), "jpg".to_string()]),
                min_rating: Some(4.0),
                exclude_tags: None,
                exclude_formats: None,
                exclude_metadata_filters: None,
                exclude_path_prefixes: None,
                exclude_number_filters: None,
                exclude_date_filters: None,
                number_filters: None,
                date_filters: None,
                match_mode: None,
                sort: None,
                limit: None,
            })
            .expect("filtered search should complete");

        assert_eq!(response.results.len(), 1);
        assert_eq!(response.results[0].path, "cover.psd");

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn search_assets_preserves_virtual_entry_markers() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("search-virtual");
        fs::write(repo_root.join("virtual-track.mp3"), b"track")
            .expect("virtual track file should be written");
        let repo_id = create_repository_for_path(&state, &repo_root);

        let repo = state
            .load_repository_record(&repo_id)
            .expect("repository record should load");
        let connection = state
            .open_repository_connection(
                &repo.summary.repo_id,
                &repo.summary.path,
                &repo.backend_record,
            )
            .expect("repository connection should open");
        connection
            .execute(
                r#"
                UPDATE assets
                SET is_virtual = 1,
                    provider_id = ?2,
                    provider_item_id = ?3,
                    source_payload_json = ?4,
                    local_absolute_path = ?5
                WHERE repo_id = ?1 AND path = 'virtual-track.mp3'
                "#,
                params![
                    repo_id,
                    "netease-cloud-music",
                    "123456",
                    serde_json::json!({
                        "provider": "netease-cloud-music",
                        "songId": 123456,
                        "playlistId": 42,
                    })
                    .to_string(),
                    Option::<String>::None,
                ],
            )
            .expect("asset should be marked virtual");
        drop(connection);

        let response = state
            .search_assets(SearchRequest {
                query: "virtual-track".to_string(),
                repo_id: Some(repo_id.clone()),
                exclude_query: None,
                metadata_key: None,
                metadata_value: None,
                tag: None,
                tags: None,
                metadata_filters: None,
                formats: Some(vec!["mp3".to_string()]),
                min_rating: None,
                exclude_tags: None,
                exclude_formats: None,
                exclude_metadata_filters: None,
                exclude_path_prefixes: None,
                exclude_number_filters: None,
                exclude_date_filters: None,
                number_filters: None,
                date_filters: None,
                match_mode: None,
                sort: None,
                limit: None,
            })
            .expect("virtual search should complete");

        assert_eq!(response.results.len(), 1);
        assert_eq!(response.results[0].path, "virtual-track.mp3");
        assert!(response.results[0].is_virtual);
        assert_eq!(
            response.results[0].provider_id.as_deref(),
            Some("netease-cloud-music")
        );
        assert_eq!(
            response.results[0].provider_item_id.as_deref(),
            Some("123456")
        );
        assert_eq!(
            response.results[0]
                .source_payload
                .as_ref()
                .and_then(|value| value.get("songId"))
                .and_then(serde_json::Value::as_i64),
            Some(123456)
        );
        assert_eq!(response.results[0].local_absolute_path, None);

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn repository_actions_list_run_and_reject_unsafe_states() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("repository-actions");
        fs::write(repo_root.join("cover.png"), b"cover").expect("cover file should be written");
        let repo_id = create_repository_for_path(&state, &repo_root);
        let asset_id = asset_id_for_test_path(&state, &repo_id, "cover.png");

        let repo = state
            .load_repository_record(&repo_id)
            .expect("repository record should load");
        let connection = state
            .open_repository_connection(
                &repo.summary.repo_id,
                &repo.summary.path,
                &repo.backend_record,
            )
            .expect("repository connection should open");
        let now = now_rfc3339();
        connection
            .execute(
                r#"
                INSERT INTO repository_actions (
                  action_id, repo_id, source, source_action_id, name, status, enabled,
                  raw_json, unsupported_reason, sort_order, created_at, updated_at
                )
                VALUES
                  ('action-ready', ?1, 'eagle-importer', 'source-ready', '标记精选', 'ready', 1, '{}', NULL, 0, ?2, ?2),
                  ('action-disabled', ?1, 'eagle-importer', 'source-disabled', '停用动作', 'ready', 0, '{}', NULL, 1, ?2, ?2),
                  ('action-unsupported', ?1, 'eagle-importer', 'source-unsupported', '未知动作', 'unsupported', 0, '{}', 'unsupported action step: shell', 2, ?2, ?2)
                "#,
                params![repo_id, now],
            )
            .expect("actions should be inserted");
        connection
            .execute(
                r#"
                INSERT INTO repository_action_steps (
                  step_id, action_id, repo_id, step_kind, label, status,
                  config_json, raw_json, unsupported_reason, sort_order
                )
                VALUES
                  ('step-ready-1', 'action-ready', ?1, 'metadata.update', '更新评分', 'ready', '{"metadata":{"rating":5,"comment":"Action run"}}', '{"type":"rating"}', NULL, 0),
                  ('step-ready-2', 'action-ready', ?1, 'tagGroups.set', '设置标签', 'ready', '{"tags":["精选"]}', '{"type":"tags"}', NULL, 1),
                  ('step-disabled-1', 'action-disabled', ?1, 'metadata.update', '更新评分', 'ready', '{"metadata":{"rating":4}}', '{"type":"rating"}', NULL, 0),
                  ('step-unsupported-1', 'action-unsupported', ?1, 'unsupported', '外部脚本', 'unsupported', '{}', '{"type":"shell"}', 'unsupported action step: shell', 0)
                "#,
                [repo_id.as_str()],
            )
            .expect("action steps should be inserted");
        drop(connection);

        let actions = state
            .list_repository_actions(&repo_id)
            .expect("actions should list");
        assert_eq!(
            actions
                .iter()
                .map(|action| action.name.as_str())
                .collect::<Vec<_>>(),
            vec!["标记精选", "停用动作", "未知动作"]
        );
        assert_eq!(actions[0].steps.len(), 2);

        let disabled_error = state
            .run_repository_action(RepositoryActionRunRequest {
                repo_id: repo_id.clone(),
                action_id: "action-disabled".to_string(),
                target_paths: Some(vec!["cover.png".to_string()]),
                asset_ids: None,
            })
            .expect_err("disabled action should be rejected");
        assert!(disabled_error.contains("disabled"));

        let unsupported_error = state
            .set_repository_action_enabled(RepositoryActionEnabledRequest {
                repo_id: repo_id.clone(),
                action_id: "action-unsupported".to_string(),
                enabled: true,
            })
            .expect_err("unsupported action cannot be enabled");
        assert!(unsupported_error.contains("unsupported"));

        let missing_target_error = state
            .run_repository_action(RepositoryActionRunRequest {
                repo_id: repo_id.clone(),
                action_id: "action-ready".to_string(),
                target_paths: None,
                asset_ids: None,
            })
            .expect_err("targetless action should be rejected");
        assert!(missing_target_error.contains("at least one target"));

        let response = state
            .run_repository_action(RepositoryActionRunRequest {
                repo_id: repo_id.clone(),
                action_id: "action-ready".to_string(),
                target_paths: Some(vec!["cover.png".to_string()]),
                asset_ids: None,
            })
            .expect("ready action should run");
        assert_eq!(response.run.status, "success");
        assert_eq!(
            response
                .action
                .last_run
                .as_ref()
                .map(|run| run.status.as_str()),
            Some("success")
        );

        let detail = state
            .load_asset_detail(&repo_id, &asset_id)
            .expect("asset detail should load after action");
        let metadata = detail
            .metadata
            .iter()
            .map(|entry| (entry.key.as_str(), entry.value.clone()))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(metadata.get("rating"), Some(&serde_json::json!(5)));
        assert_eq!(
            metadata.get("comment"),
            Some(&serde_json::json!("Action run"))
        );
        assert_eq!(
            metadata.get("tagGroups"),
            Some(&serde_json::json!(["精选"]))
        );
        assert!(detail
            .revisions
            .iter()
            .any(|revision| revision.source == "repository-action:action-ready"));

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn search_and_smart_folders_apply_exclude_filters_after_include_filters() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("search-excludes");
        fs::create_dir_all(repo_root.join("Archive")).expect("archive directory should be created");
        fs::write(repo_root.join("hero.png"), b"hero").expect("hero file should be written");
        fs::write(repo_root.join("draft.png"), b"draft").expect("draft file should be written");
        fs::write(repo_root.join("Archive/old.png"), b"old")
            .expect("archived file should be written");
        let repo_id = create_repository_for_path(&state, &repo_root);

        let repo = state
            .load_repository_record(&repo_id)
            .expect("repository record should load");
        let connection = state
            .open_repository_connection(
                &repo.summary.repo_id,
                &repo.summary.path,
                &repo.backend_record,
            )
            .expect("repository connection should open");
        let now = now_rfc3339();
        let ids = ["hero.png", "draft.png", "Archive/old.png"]
            .into_iter()
            .map(|path| {
                let asset_id: String = connection
                    .query_row(
                        "SELECT asset_id FROM assets WHERE repo_id = ?1 AND path = ?2",
                        params![repo_id.as_str(), path],
                        |row| row.get(0),
                    )
                    .expect("asset id should load");
                (path.to_string(), asset_id)
            })
            .collect::<BTreeMap<_, _>>();
        for (path, width, created_at, note) in [
            (
                "hero.png",
                serde_json::json!(1920),
                serde_json::json!("2024-02-02T00:00:00Z"),
                serde_json::json!("final hero"),
            ),
            (
                "draft.png",
                serde_json::json!(480),
                serde_json::json!("2024-02-02T00:00:00Z"),
                serde_json::json!("draft hero"),
            ),
            (
                "Archive/old.png",
                serde_json::json!(1920),
                serde_json::json!("2024-01-10T00:00:00Z"),
                serde_json::json!("old hero"),
            ),
        ] {
            for (key, value) in [
                ("width", width),
                ("fileCreatedAt", created_at),
                ("note", note),
            ] {
                connection
                    .execute(
                        r#"
                        INSERT OR REPLACE INTO metadata (asset_id, key, value_type, value_json, version, updated_at)
                        VALUES (?1, ?2, ?3, ?4, 1, ?5)
                        "#,
                        params![
                            ids[path].as_str(),
                            key,
                            infer_value_type(&value),
                            value.to_string(),
                            now
                        ],
                    )
                    .expect("metadata should be written");
            }
        }
        drop(connection);

        let response = state
            .search_assets(SearchRequest {
                query: "hero".to_string(),
                repo_id: Some(repo_id.clone()),
                exclude_query: Some("draft".to_string()),
                metadata_key: None,
                metadata_value: None,
                tag: None,
                tags: None,
                metadata_filters: None,
                exclude_tags: None,
                exclude_formats: None,
                exclude_metadata_filters: None,
                exclude_path_prefixes: Some(vec!["Archive".to_string()]),
                exclude_number_filters: Some(vec![SearchNumberFilter {
                    key: "width".to_string(),
                    min: None,
                    max: Some(640.0),
                }]),
                exclude_date_filters: Some(vec![SearchDateFilter {
                    key: "fileCreatedAt".to_string(),
                    from: Some("2024-01-01T00:00:00Z".to_string()),
                    to: Some("2024-01-31T00:00:00Z".to_string()),
                }]),
                number_filters: None,
                date_filters: None,
                formats: Some(vec!["png".to_string()]),
                min_rating: None,
                match_mode: None,
                sort: None,
                limit: None,
            })
            .expect("exclude search should complete");
        assert_eq!(
            response
                .results
                .iter()
                .map(|item| item.path.as_str())
                .collect::<Vec<_>>(),
            vec!["hero.png"]
        );

        state
            .create_smart_folder(SmartFolderMutationRequest {
                repo_id: repo_id.clone(),
                smart_folder_id: Some("smart-hero".to_string()),
                parent_id: None,
                name: "Hero".to_string(),
                filter: SmartFolderFilter {
                    query: Some("hero".to_string()),
                    formats: Some(vec!["png".to_string()]),
                    exclude_query: Some("draft".to_string()),
                    exclude_path_prefixes: Some(vec!["Archive".to_string()]),
                    exclude_number_filters: Some(vec![SearchNumberFilter {
                        key: "width".to_string(),
                        min: None,
                        max: Some(640.0),
                    }]),
                    exclude_date_filters: Some(vec![SearchDateFilter {
                        key: "fileCreatedAt".to_string(),
                        from: Some("2024-01-01T00:00:00Z".to_string()),
                        to: Some("2024-01-31T00:00:00Z".to_string()),
                    }]),
                    ..SmartFolderFilter::default()
                },
            })
            .expect("smart folder should be created");
        let smart_result = state
            .query_smart_folder(&repo_id, "smart-hero")
            .expect("smart folder should query");
        assert_eq!(
            smart_result
                .results
                .iter()
                .map(|item| item.path.as_str())
                .collect::<Vec<_>>(),
            vec!["hero.png"]
        );

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn plugin_manifest_infers_category_for_legacy_kind() {
        let manifest = parse_plugin_manifest(
            r#"{
              "pluginId": "user.legacy-webdav",
              "legacyPluginIds": [],
              "name": "Legacy WebDAV",
              "version": "0.1.0",
              "kind": "webdav",
              "description": "Legacy source plugin.",
              "capabilities": ["browse"],
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
            }"#,
        )
        .expect("legacy manifest should parse");

        assert_eq!(manifest.category, "source");
        assert!(is_repository_backend_plugin(&manifest));
        assert!(manifest.requires.is_empty());
        assert!(manifest.optional.is_empty());
        assert!(manifest.hooks.is_empty());
        assert!(manifest.contributes.is_object());
    }

    #[test]
    fn search_assets_match_mode_or_spans_filter_families_and_metadata_sort_is_typed() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("search-or-sort");
        fs::write(repo_root.join("tag-only.png"), b"tag").expect("tag-only file should be written");
        fs::write(repo_root.join("metadata-only.png"), b"metadata")
            .expect("metadata-only file should be written");
        fs::write(repo_root.join("small.png"), b"small").expect("small file should be written");
        fs::write(repo_root.join("large.png"), b"large").expect("large file should be written");
        let repo_id = create_repository_for_path(&state, &repo_root);

        let repo = state
            .load_repository_record(&repo_id)
            .expect("repository record should load");
        let connection = state
            .open_repository_connection(
                &repo.summary.repo_id,
                &repo.summary.path,
                &repo.backend_record,
            )
            .expect("repository connection should open");
        let now = now_rfc3339();
        let ids = [
            "tag-only.png",
            "metadata-only.png",
            "small.png",
            "large.png",
        ]
        .into_iter()
        .map(|path| {
            let asset_id: String = connection
                .query_row(
                    "SELECT asset_id FROM assets WHERE repo_id = ?1 AND path = ?2",
                    params![repo_id.as_str(), path],
                    |row| row.get(0),
                )
                .expect("asset id should load");
            (path.to_string(), asset_id)
        })
        .collect::<BTreeMap<_, _>>();

        connection
            .execute(
                "INSERT OR REPLACE INTO tags (asset_id, tag, normalized_tag) VALUES (?1, ?2, ?3)",
                params![ids["tag-only.png"].as_str(), "Poster", "poster"],
            )
            .expect("tag should be written");
        for (path, width, created_at) in [
            (
                "metadata-only.png",
                serde_json::json!(1920),
                serde_json::json!("2024-01-04T00:00:00Z"),
            ),
            (
                "small.png",
                serde_json::json!(800),
                serde_json::json!("2024-01-01T00:00:00Z"),
            ),
            (
                "large.png",
                serde_json::json!(1920),
                serde_json::json!("2024-01-03T00:00:00Z"),
            ),
        ] {
            for (key, value) in [("width", width), ("fileCreatedAt", created_at)] {
                connection
                    .execute(
                        r#"
                        INSERT OR REPLACE INTO metadata (asset_id, key, value_type, value_json, version, updated_at)
                        VALUES (?1, ?2, ?3, ?4, 1, ?5)
                        "#,
                        params![
                            ids[path].as_str(),
                            key,
                            infer_value_type(&value),
                            value.to_string(),
                            now
                        ],
                    )
                    .expect("metadata should be written");
            }
        }
        drop(connection);

        let or_response = state
            .search_assets(SearchRequest {
                query: String::new(),
                repo_id: Some(repo_id.clone()),
                exclude_query: None,
                metadata_key: None,
                metadata_value: None,
                tag: None,
                tags: Some(vec!["Poster".to_string()]),
                metadata_filters: Some(vec![SearchMetadataFilter {
                    key: "width".to_string(),
                    value: "1920".to_string(),
                }]),
                exclude_tags: None,
                exclude_formats: None,
                exclude_metadata_filters: None,
                exclude_path_prefixes: None,
                exclude_number_filters: None,
                exclude_date_filters: None,
                number_filters: None,
                date_filters: None,
                formats: None,
                min_rating: None,
                match_mode: Some("or".to_string()),
                sort: Some(SearchSort {
                    field: "metadata.fileCreatedAt".to_string(),
                    direction: "asc".to_string(),
                }),
                limit: None,
            })
            .expect("or search should complete");

        let or_paths = or_response
            .results
            .iter()
            .map(|item| item.path.as_str())
            .collect::<Vec<_>>();
        assert_eq!(or_paths.len(), 3);
        assert!(or_paths.contains(&"tag-only.png"));
        assert!(or_paths.contains(&"metadata-only.png"));
        assert!(or_paths.contains(&"large.png"));

        let width_sorted = state
            .search_assets(SearchRequest {
                query: String::new(),
                repo_id: Some(repo_id.clone()),
                exclude_query: None,
                metadata_key: None,
                metadata_value: None,
                tag: None,
                tags: None,
                metadata_filters: None,
                exclude_tags: None,
                exclude_formats: None,
                exclude_metadata_filters: None,
                exclude_path_prefixes: None,
                exclude_number_filters: None,
                exclude_date_filters: None,
                number_filters: None,
                date_filters: None,
                formats: Some(vec!["png".to_string()]),
                min_rating: None,
                match_mode: None,
                sort: Some(SearchSort {
                    field: "metadata.width".to_string(),
                    direction: "asc".to_string(),
                }),
                limit: None,
            })
            .expect("metadata width sort should complete");

        let sorted_paths = width_sorted
            .results
            .iter()
            .map(|item| item.path.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            sorted_paths,
            vec![
                "small.png",
                "large.png",
                "metadata-only.png",
                "tag-only.png"
            ]
        );

        let date_sorted = state
            .search_assets(SearchRequest {
                query: String::new(),
                repo_id: Some(repo_id.clone()),
                exclude_query: None,
                metadata_key: None,
                metadata_value: None,
                tag: None,
                tags: None,
                metadata_filters: Some(vec![SearchMetadataFilter {
                    key: "width".to_string(),
                    value: "1920".to_string(),
                }]),
                exclude_tags: None,
                exclude_formats: None,
                exclude_metadata_filters: None,
                exclude_path_prefixes: None,
                exclude_number_filters: None,
                exclude_date_filters: None,
                number_filters: None,
                date_filters: None,
                formats: None,
                min_rating: None,
                match_mode: None,
                sort: Some(SearchSort {
                    field: "metadata.fileCreatedAt".to_string(),
                    direction: "asc".to_string(),
                }),
                limit: None,
            })
            .expect("metadata date sort should complete");
        let date_paths = date_sorted
            .results
            .iter()
            .map(|item| item.path.as_str())
            .collect::<Vec<_>>();
        assert_eq!(date_paths, vec!["large.png", "metadata-only.png"]);

        let random_sorted = state
            .search_assets(SearchRequest {
                query: String::new(),
                repo_id: Some(repo_id),
                exclude_query: None,
                metadata_key: None,
                metadata_value: None,
                tag: None,
                tags: None,
                metadata_filters: None,
                exclude_tags: None,
                exclude_formats: None,
                exclude_metadata_filters: None,
                exclude_path_prefixes: None,
                exclude_number_filters: None,
                exclude_date_filters: None,
                number_filters: None,
                date_filters: None,
                formats: Some(vec!["png".to_string()]),
                min_rating: None,
                match_mode: None,
                sort: Some(SearchSort {
                    field: "random".to_string(),
                    direction: "asc".to_string(),
                }),
                limit: None,
            })
            .expect("core random sort should complete");
        let random_paths = random_sorted
            .results
            .iter()
            .map(|item| item.path.as_str())
            .collect::<HashSet<_>>();
        assert_eq!(
            random_paths,
            HashSet::from([
                "large.png",
                "metadata-only.png",
                "small.png",
                "tag-only.png",
            ])
        );

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn copy_entries_records_linked_hardlink_member() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("copy-hardlink");
        fs::create_dir_all(repo_root.join("Copies")).expect("copies folder should be created");
        fs::write(repo_root.join("source.txt"), b"copy me").expect("source file should be written");
        let repo_id = create_repository_for_path(&state, &repo_root);

        state
            .copy_entries(FileCopyRequest {
                repo_id: repo_id.clone(),
                source_paths: vec!["source.txt".to_string()],
                parent_path: Some("Copies".to_string()),
                mode: None,
            })
            .expect("copy should complete");

        let snapshot = state
            .load_file_browser(FileBrowserRequest {
                repo_id: repo_id.clone(),
                directory_path: Some("Copies".to_string()),
                include_tree: Some(false),
                special_location: None,
                offset: None,
                limit: None,
            })
            .expect("browser should load");
        let copied = snapshot
            .entries
            .iter()
            .find(|entry| entry.path == "Copies/source.txt")
            .expect("copied entry should exist");
        assert_eq!(copied.hardlink_state.as_deref(), Some("linked"));
        assert!(copied.hardlink_group_id.is_some());
        let root_snapshot = state
            .load_file_browser(FileBrowserRequest {
                repo_id: repo_id.clone(),
                directory_path: Some(String::new()),
                include_tree: Some(false),
                special_location: None,
                offset: None,
                limit: None,
            })
            .expect("root browser should load");
        let source = root_snapshot
            .entries
            .iter()
            .find(|entry| entry.path == "source.txt")
            .expect("source entry should exist");
        assert_eq!(source.hardlink_state.as_deref(), Some("linked"));
        assert_eq!(source.hardlink_group_id, copied.hardlink_group_id);
        let candidates = state
            .list_hardlink_candidates(&repo_id)
            .expect("candidates should load");
        assert!(candidates.candidates.is_empty());

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn copy_entries_rejects_same_directory_target() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("copy-same-directory");
        fs::write(repo_root.join("source.txt"), b"copy me").expect("source file should be written");
        let repo_id = create_repository_for_path(&state, &repo_root);

        let error = state
            .copy_entries(FileCopyRequest {
                repo_id,
                source_paths: vec!["source.txt".to_string()],
                parent_path: Some("".to_string()),
                mode: None,
            })
            .expect_err("same-directory copy should fail");
        assert!(error.contains("不能复制到原目录"));

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn move_entries_updates_filesystem_and_asset_paths() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("move-file");
        fs::create_dir_all(repo_root.join("Archive")).expect("archive folder should be created");
        fs::write(repo_root.join("note.txt"), b"move me").expect("source file should be written");
        let repo_id = create_repository_without_initial_sync(&state, &repo_root);
        state
            .sync_repository(SyncRequest {
                repo_id: repo_id.clone(),
            })
            .expect("repository should sync");

        let snapshot = state
            .move_entries(FileMoveRequest {
                repo_id: repo_id.clone(),
                source_paths: vec!["note.txt".to_string()],
                parent_path: "Archive".to_string(),
            })
            .expect("move should complete");

        assert!(!repo_root.join("note.txt").exists());
        assert!(repo_root.join("Archive/note.txt").is_file());
        assert!(snapshot
            .entries
            .iter()
            .any(|entry| entry.path == "Archive/note.txt"));

        let repository_snapshot = state
            .load_snapshot(&repo_id)
            .expect("repository snapshot should load");
        assert!(repository_snapshot
            .assets
            .iter()
            .any(|asset| asset.path == "Archive/note.txt"));

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn move_entries_reject_same_directory_target() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("move-same-directory");
        fs::write(repo_root.join("source.txt"), b"move me").expect("source file should be written");
        let repo_id = create_repository_for_path(&state, &repo_root);

        let error = state
            .move_entries(FileMoveRequest {
                repo_id,
                source_paths: vec!["source.txt".to_string()],
                parent_path: String::new(),
            })
            .expect_err("same-directory move should fail");
        assert!(error.contains("不能移动到原目录"));

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn move_entries_reject_folder_cycle_nesting() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("move-folder-cycle");
        fs::create_dir_all(repo_root.join("Scenes/Act1")).expect("nested folder should be created");
        fs::write(repo_root.join("Scenes/Act1/shot.txt"), b"scene")
            .expect("nested file should be written");
        let repo_id = create_repository_without_initial_sync(&state, &repo_root);
        state
            .sync_repository(SyncRequest {
                repo_id: repo_id.clone(),
            })
            .expect("repository should sync");

        let error = state
            .move_entries(FileMoveRequest {
                repo_id,
                source_paths: vec!["Scenes".to_string()],
                parent_path: "Scenes/Act1".to_string(),
            })
            .expect_err("cyclic folder move should fail");
        assert!(error.contains("文件夹不能移动到自身或其子文件夹内"));

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn copy_entries_copy_mode_records_fallback_without_candidate() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("copy-fallback");
        fs::create_dir_all(repo_root.join("Copies")).expect("copies folder should be created");
        fs::write(repo_root.join("source.txt"), b"copy me").expect("source file should be written");
        let repo_id = create_repository_for_path(&state, &repo_root);

        state
            .copy_entries(FileCopyRequest {
                repo_id: repo_id.clone(),
                source_paths: vec!["source.txt".to_string()],
                parent_path: Some("Copies".to_string()),
                mode: Some("copy".to_string()),
            })
            .expect("copy should complete");

        let snapshot = state
            .load_file_browser(FileBrowserRequest {
                repo_id: repo_id.clone(),
                directory_path: Some("Copies".to_string()),
                include_tree: Some(false),
                special_location: None,
                offset: None,
                limit: None,
            })
            .expect("browser should load");
        let copied = snapshot
            .entries
            .iter()
            .find(|entry| entry.path == "Copies/source.txt")
            .expect("copied entry should exist");
        assert_eq!(copied.hardlink_state.as_deref(), Some("copiedFallback"));
        state
            .sync_repository(SyncRequest {
                repo_id: repo_id.clone(),
            })
            .expect("sync should preserve fallback state");
        let synced_snapshot = state
            .load_file_browser(FileBrowserRequest {
                repo_id: repo_id.clone(),
                directory_path: Some("Copies".to_string()),
                include_tree: Some(false),
                special_location: None,
                offset: None,
                limit: None,
            })
            .expect("browser should load after sync");
        let synced_copy = synced_snapshot
            .entries
            .iter()
            .find(|entry| entry.path == "Copies/source.txt")
            .expect("copied entry should exist after sync");
        assert_eq!(
            synced_copy.hardlink_state.as_deref(),
            Some("copiedFallback")
        );
        let candidates = state
            .list_hardlink_candidates(&repo_id)
            .expect("candidates should load");
        assert!(candidates.candidates.is_empty());

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn confirm_hardlink_candidate_rejects_changed_file() {
        let (state, root, repo_root, _thumbnail_root) =
            create_test_state("confirm-hardlink-changed");
        fs::write(repo_root.join("source.txt"), b"same bytes")
            .expect("source file should be written");
        let repo_id = create_repository_for_path(&state, &repo_root);
        fs::write(repo_root.join("copy.txt"), b"same bytes").expect("copy file should be written");
        state
            .sync_repository(SyncRequest {
                repo_id: repo_id.clone(),
            })
            .expect("sync should create candidate");
        let candidate_id = state
            .list_hardlink_candidates(&repo_id)
            .expect("candidates should load")
            .candidates
            .first()
            .expect("candidate should exist")
            .candidate_id
            .clone();

        fs::write(repo_root.join("copy.txt"), b"changed bytes")
            .expect("copy file should be modified");
        let error = state
            .confirm_hardlink_candidate(HardlinkConfirmRequest {
                repo_id: repo_id.clone(),
                candidate_id,
            })
            .expect_err("changed candidate should fail");
        assert!(error.contains("no longer valid"));
        let bytes = fs::read(repo_root.join("copy.txt")).expect("copy file should still exist");
        assert_eq!(bytes, b"changed bytes");
        let candidates = state
            .list_hardlink_candidates(&repo_id)
            .expect("candidates should load after rejection");
        assert!(candidates.candidates.is_empty());

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn hardlink_candidate_list_filters_stale_records() {
        let (state, root, repo_root, _thumbnail_root) =
            create_test_state("hardlink-stale-candidate");
        fs::write(repo_root.join("source.txt"), b"same bytes")
            .expect("source file should be written");
        let repo_id = create_repository_for_path(&state, &repo_root);
        fs::write(repo_root.join("copy.txt"), b"same bytes").expect("copy file should be written");
        state
            .sync_repository(SyncRequest {
                repo_id: repo_id.clone(),
            })
            .expect("sync should create candidate");
        assert_eq!(
            state
                .list_hardlink_candidates(&repo_id)
                .expect("candidate should load")
                .candidates
                .len(),
            1
        );

        fs::write(repo_root.join("copy.txt"), b"different bytes")
            .expect("copy file should be modified");
        state
            .sync_repository(SyncRequest {
                repo_id: repo_id.clone(),
            })
            .expect("sync should mark candidate stale");
        assert!(state
            .list_hardlink_candidates(&repo_id)
            .expect("stale candidates should be filtered")
            .candidates
            .is_empty());

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn ensure_thumbnail_reuses_existing_cache_path() {
        let (state, root, repo_root, thumbnail_root) = create_test_state("thumb-reuse");
        write_test_image(&repo_root.join("cover.png"));
        let repo_id = create_repository_for_path(&state, &repo_root);
        let snapshot = state
            .load_snapshot(&repo_id)
            .expect("snapshot should load after sync");
        let asset_id = snapshot.assets[0].asset_id.clone();
        let thumbnail_dir = thumbnail_root.join(thumbnail_repository_dir_name(
            &repo_id,
            &repo_root.to_string_lossy(),
        ));
        fs::create_dir_all(&thumbnail_dir).expect("thumbnail dir should be created");
        let thumbnail_path = thumbnail_dir.join(thumbnail_file_name(
            &repo_id,
            &repo_root.to_string_lossy(),
            "cover.png",
            "file",
            "generated",
        ));
        write_test_image(&thumbnail_path);

        let storage_paths = ensure_repository_storage_paths(
            &state.root,
            &repo_id,
            &repo_root,
            LOCAL_FILESYSTEM_PLUGIN_ID,
        )
        .expect("storage paths should resolve");
        let connection =
            Connection::open(storage_paths.database_path).expect("repository db should open");
        connection
            .execute(
                "UPDATE assets SET thumbnail_path = ?3 WHERE repo_id = ?1 AND asset_id = ?2",
                params![
                    repo_id,
                    asset_id,
                    thumbnail_path.to_string_lossy().to_string()
                ],
            )
            .expect("asset thumbnail path should update");

        let response = state
            .ensure_thumbnail(ThumbnailRequest {
                repo_id,
                path: "cover.png".to_string(),
                action: None,
                source_path: None,
                source_url: None,
                image_bytes: None,
                media_type: None,
            })
            .expect("thumbnail should be ensured");

        assert_eq!(
            response.thumbnail_path,
            Some(thumbnail_path.to_string_lossy().to_string())
        );

        drop(connection);
        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn ensure_repository_storage_paths_use_local_root_capability_for_custom_filesystem_plugin() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("custom-local-root");
        let runtime_root = runtime_plugins_dir(&state.root);
        write_test_plugin_archive_with_manifest(
            &runtime_root.join("custom-local-root.momoplug"),
            test_plugin_manifest_json(
                "user.custom-local-root",
                "Custom Local Root",
                serde_json::json!({
                    "kind": "filesystem",
                    "category": "source",
                    "type": {
                        "layer": "source",
                        "kind": "filesystem"
                    },
                    "capabilities": ["browse", "read", "write", "localRootPath"],
                    "runtime": "manifest-only",
                    "source": "user"
                }),
            ),
        );

        let storage_paths = ensure_repository_storage_paths(
            &state.root,
            "repo-custom-local-root",
            &repo_root,
            "user.custom-local-root",
        )
        .expect("storage paths should resolve");

        assert_eq!(storage_paths.metadata_dir, repo_root.join(REPO_META_DIR));

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn ensure_thumbnail_migrates_existing_cache_path_to_repository_metadata_dir() {
        let (state, root, repo_root, thumbnail_root) = create_test_state("thumb-migrate");
        write_test_image(&repo_root.join("cover.png"));
        let repo_id = create_repository_for_path(&state, &repo_root);
        let snapshot = state
            .load_snapshot(&repo_id)
            .expect("snapshot should load after sync");
        let asset_id = snapshot.assets[0].asset_id.clone();
        let legacy_root = root.join("legacy-thumbnails");
        let legacy_dir = legacy_root.join(thumbnail_repository_dir_name(
            &repo_id,
            &repo_root.to_string_lossy(),
        ));
        fs::create_dir_all(&legacy_dir).expect("legacy thumbnail dir should be created");
        let legacy_thumbnail_path = legacy_dir.join(thumbnail_file_name(
            &repo_id,
            &repo_root.to_string_lossy(),
            "cover.png",
            "file",
            "generated",
        ));
        write_test_image(&legacy_thumbnail_path);

        let storage_paths = ensure_repository_storage_paths(
            &state.root,
            &repo_id,
            &repo_root,
            LOCAL_FILESYSTEM_PLUGIN_ID,
        )
        .expect("storage paths should resolve");
        let connection =
            Connection::open(storage_paths.database_path).expect("repository db should open");
        connection
            .execute(
                "UPDATE assets SET thumbnail_path = ?3 WHERE repo_id = ?1 AND asset_id = ?2",
                params![
                    repo_id,
                    asset_id,
                    legacy_thumbnail_path.to_string_lossy().to_string()
                ],
            )
            .expect("asset thumbnail path should update");

        let response = state
            .ensure_thumbnail(ThumbnailRequest {
                repo_id: repo_id.clone(),
                path: "cover.png".to_string(),
                action: None,
                source_path: None,
                source_url: None,
                image_bytes: None,
                media_type: None,
            })
            .expect("thumbnail should be ensured");
        let thumbnail_path = response
            .thumbnail_path
            .as_deref()
            .map(Path::new)
            .expect("thumbnail path should be returned");

        assert!(thumbnail_path.starts_with(&thumbnail_root));
        assert!(thumbnail_path.is_file());
        assert_ne!(thumbnail_path, legacy_thumbnail_path.as_path());
        let stored_path: String = connection
            .query_row(
                "SELECT thumbnail_path FROM assets WHERE repo_id = ?1 AND asset_id = ?2",
                params![repo_id, asset_id],
                |row| row.get(0),
            )
            .expect("asset thumbnail path should load");
        assert_eq!(stored_path, thumbnail_path.to_string_lossy());

        drop(connection);
        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn ensure_thumbnail_migrates_custom_entry_cache_path_to_repository_metadata_dir() {
        let (state, root, repo_root, thumbnail_root) = create_test_state("thumb-custom-migrate");
        fs::create_dir_all(repo_root.join("Shots")).expect("directory should be created");
        let repo_id = create_repository_for_path(&state, &repo_root);
        let legacy_root = root.join("legacy-thumbnails");
        let legacy_dir = legacy_root.join(thumbnail_repository_dir_name(
            &repo_id,
            &repo_root.to_string_lossy(),
        ));
        fs::create_dir_all(&legacy_dir).expect("legacy thumbnail dir should be created");
        let legacy_thumbnail_path = legacy_dir.join(thumbnail_file_name(
            &repo_id,
            &repo_root.to_string_lossy(),
            "Shots",
            "directory",
            "custom",
        ));
        write_test_image(&legacy_thumbnail_path);

        let storage_paths = ensure_repository_storage_paths(
            &state.root,
            &repo_id,
            &repo_root,
            LOCAL_FILESYSTEM_PLUGIN_ID,
        )
        .expect("storage paths should resolve");
        let connection =
            Connection::open(storage_paths.database_path).expect("repository db should open");
        upsert_entry_thumbnail_record(
            &connection,
            &repo_id,
            "Shots",
            "directory",
            &legacy_thumbnail_path.to_string_lossy(),
            true,
        )
        .expect("entry thumbnail should be seeded");

        let response = state
            .ensure_thumbnail(ThumbnailRequest {
                repo_id: repo_id.clone(),
                path: "Shots".to_string(),
                action: None,
                source_path: None,
                source_url: None,
                image_bytes: None,
                media_type: None,
            })
            .expect("thumbnail should be ensured");
        let thumbnail_path = response
            .thumbnail_path
            .as_deref()
            .map(Path::new)
            .expect("thumbnail path should be returned");

        assert!(response.thumbnail_custom);
        assert!(thumbnail_path.starts_with(&thumbnail_root));
        assert!(thumbnail_path.is_file());
        assert_ne!(thumbnail_path, legacy_thumbnail_path.as_path());
        let stored_path: String = connection
            .query_row(
                "SELECT thumbnail_path FROM entry_thumbnails WHERE repo_id = ?1 AND path = ?2 AND kind = ?3",
                params![repo_id, "Shots", "directory"],
                |row| row.get(0),
            )
            .expect("entry thumbnail path should load");
        assert_eq!(stored_path, thumbnail_path.to_string_lossy());

        drop(connection);
        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn ensure_thumbnail_writes_cache_under_repository_metadata_dir() {
        let (state, root, repo_root, thumbnail_root) = create_test_state("thumb-repo-meta");
        write_test_image(&repo_root.join("cover.png"));
        let repo_id = create_repository_for_path(&state, &repo_root);

        let response = state
            .ensure_thumbnail(ThumbnailRequest {
                repo_id,
                path: "cover.png".to_string(),
                action: None,
                source_path: None,
                source_url: None,
                image_bytes: None,
                media_type: None,
            })
            .expect("thumbnail should be generated");
        let thumbnail_path = response
            .thumbnail_path
            .as_deref()
            .map(Path::new)
            .expect("thumbnail path should be returned");

        assert!(thumbnail_path.starts_with(&thumbnail_root));
        assert!(thumbnail_path.is_file());
        assert_eq!(count_files(&thumbnail_root), 1);

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn ensure_thumbnail_saves_remote_source_url_as_custom_thumbnail() {
        let (state, root, repo_root, thumbnail_root) = create_test_state("thumb-remote-source");
        fs::write(repo_root.join("track.mp3"), b"fake audio")
            .expect("track file should be written");
        let repo_id = create_repository_for_path(&state, &repo_root);
        let mut body = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            2,
            2,
            image::Rgb([220, 80, 40]),
        ))
        .write_to(&mut body, image::ImageFormat::Png)
        .expect("test thumbnail image should encode");
        let source_url = serve_test_http_body(body.into_inner());

        let response = state
            .ensure_thumbnail(ThumbnailRequest {
                repo_id: repo_id.clone(),
                path: "track.mp3".to_string(),
                action: Some("save".to_string()),
                source_path: None,
                source_url: Some(source_url),
                image_bytes: None,
                media_type: None,
            })
            .expect("remote thumbnail should be saved");
        let thumbnail_path = response
            .thumbnail_path
            .as_deref()
            .map(Path::new)
            .expect("thumbnail path should be returned");

        assert!(response.thumbnail_custom);
        assert!(thumbnail_path.starts_with(&thumbnail_root));
        assert!(thumbnail_path.is_file());
        assert!(response
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("thumbnailPalette"))
            .is_some());

        let snapshot = state
            .load_file_browser(FileBrowserRequest {
                repo_id,
                directory_path: Some(String::new()),
                include_tree: Some(false),
                special_location: None,
                offset: None,
                limit: None,
            })
            .expect("file browser should load");
        let entry = snapshot
            .entries
            .iter()
            .find(|item| item.path == "track.mp3")
            .expect("track entry should be listed");
        assert!(entry.thumbnail_custom);
        assert_eq!(
            entry.thumbnail_path.as_deref(),
            Some(thumbnail_path.to_string_lossy().as_ref())
        );

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn ensure_thumbnail_extracts_palette_metadata() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("thumb-palette");
        write_test_image(&repo_root.join("cover.png"));
        let repo_id = create_repository_for_path(&state, &repo_root);

        let response = state
            .ensure_thumbnail(ThumbnailRequest {
                repo_id: repo_id.clone(),
                path: "cover.png".to_string(),
                action: None,
                source_path: None,
                source_url: None,
                image_bytes: None,
                media_type: None,
            })
            .expect("thumbnail should be generated");
        let palette = response
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("thumbnailPalette"))
            .and_then(serde_json::Value::as_array)
            .cloned()
            .expect("thumbnail palette should be returned");

        assert!(!palette.is_empty());
        assert!(palette
            .iter()
            .all(|item| item.as_str().is_some_and(|value| value.starts_with('#'))));

        let snapshot = state
            .load_file_browser(FileBrowserRequest {
                repo_id,
                directory_path: Some(String::new()),
                include_tree: Some(false),
                special_location: None,
                offset: None,
                limit: None,
            })
            .expect("file browser should load");
        let entry = snapshot
            .entries
            .iter()
            .find(|item| item.path == "cover.png")
            .expect("cover entry should be listed");
        assert_eq!(
            entry.metadata.get("thumbnailPalette"),
            Some(&serde_json::Value::Array(palette))
        );

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn ensure_thumbnail_returns_null_for_unsupported_types() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("thumb-unsupported");
        fs::write(repo_root.join("note.txt"), "plain text").expect("text file should be written");
        let repo_id = create_repository_for_path(&state, &repo_root);

        let response = state
            .ensure_thumbnail(ThumbnailRequest {
                repo_id,
                path: "note.txt".to_string(),
                action: None,
                source_path: None,
                source_url: None,
                image_bytes: None,
                media_type: None,
            })
            .expect("unsupported thumbnail request should succeed");

        assert_eq!(response.thumbnail_path, None);

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn open_preview_file_source_returns_registered_file() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("preview-open");
        let source_path = repo_root.join("model.glb");
        fs::write(&source_path, b"glb").expect("preview source should be written");
        let repo_id = create_repository_for_path(&state, &repo_root);
        let response = state
            .prepare_preview_file_source(FileReadRequest {
                repo_id,
                path: "model.glb".to_string(),
            })
            .expect("preview source should be prepared");

        let (mut file, media_type) = state
            .open_preview_file_source(&response.token)
            .expect("registered preview token should open");
        let mut body = Vec::new();
        use std::io::Read;
        file.read_to_end(&mut body)
            .expect("preview file should be readable");

        assert_eq!(body, b"glb");
        assert_eq!(media_type, "model/gltf-binary");
        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn register_preview_source_path_serves_temporary_playback_files() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("preview-register-temp");
        let audio_path = repo_root.join("temp-track.mp3");
        let lyric_path = repo_root.join("temp-track.lrc");
        fs::write(&audio_path, b"audio").expect("temporary audio should be written");
        fs::write(&lyric_path, "[00:01.00]line").expect("temporary lyric should be written");

        let audio_token = state
            .register_preview_source_path(audio_path, "audio/mpeg")
            .expect("audio source should register");
        let lyric_token = state
            .register_preview_source_path(lyric_path, "text/plain; charset=utf-8")
            .expect("lyric source should register");

        let (mut audio_file, audio_media_type) = state
            .open_preview_file_source(&audio_token)
            .expect("registered audio source should open");
        let mut audio_body = Vec::new();
        use std::io::Read;
        audio_file
            .read_to_end(&mut audio_body)
            .expect("audio source should read");
        assert_eq!(audio_body, b"audio");
        assert_eq!(audio_media_type, "audio/mpeg");

        let (mut lyric_file, lyric_media_type) = state
            .open_preview_file_source(&lyric_token)
            .expect("registered lyric source should open");
        let mut lyric_text = String::new();
        lyric_file
            .read_to_string(&mut lyric_text)
            .expect("lyric source should read");
        assert_eq!(lyric_text, "[00:01.00]line");
        assert_eq!(lyric_media_type, "text/plain; charset=utf-8");

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn prepare_entry_playback_source_uses_backend_stat_for_unindexed_virtual_tracks() {
        let _lock = playback_test_lock();
        let (state, root, repo_root, _thumbnail_root) =
            create_test_state("prepare-entry-playback-unindexed-virtual");
        let repo_id = create_repository_without_initial_sync(&state, &repo_root);
        let expected_repo_id = repo_id.clone();

        fn stat_hook(
            _repo: &RepositoryRecord,
            _repo_root: &Path,
            entry_path: &str,
        ) -> Option<Result<FileSystemEntry, String>> {
            if entry_path != "Created/lazy-track.mp3" {
                return None;
            }
            Some(Ok(FileSystemEntry {
                path: entry_path.to_string(),
                name: "lazy-track.mp3".to_string(),
                kind: FileSystemEntryKind::File,
                extension: Some("mp3".to_string()),
                size_bytes: None,
                modified_at: Some("2026-06-14T00:00:00Z".to_string()),
                is_virtual: true,
                provider_id: Some("netease-cloud-music".to_string()),
                provider_item_id: Some("3001".to_string()),
                source_payload: Some(serde_json::json!({
                    "provider": "netease-cloud-music",
                    "songId": "3001",
                    "accountCookie": "MUSIC_U=lazy-cookie",
                    "accountId": "42",
                    "level": "exhigh"
                })),
                local_absolute_path: None,
            }))
        }

        fn playback_hook(payload: serde_json::Value) -> Result<serde_json::Value, String> {
            let expected_repo_id = std::env::var("MOMOBKO_TEST_EXPECTED_REPO_ID")
                .expect("expected repo id should be provided");
            assert_eq!(payload["songId"], serde_json::json!(3001));
            assert_eq!(
                payload["accountCookie"],
                serde_json::json!("MUSIC_U=lazy-cookie")
            );
            assert_eq!(payload["level"], serde_json::json!("exhigh"));
            assert_eq!(payload["repoId"], serde_json::json!(expected_repo_id));
            assert_eq!(
                payload["entryPath"],
                serde_json::json!("Created/lazy-track.mp3")
            );
            assert_eq!(
                payload["sourcePayload"]["provider"],
                serde_json::json!("netease-cloud-music")
            );
            Ok(serde_json::json!({
                "localPath": "C:/Mock/Temp/lazy-track.mp3",
                "tempFilePath": "C:/Mock/Temp/lazy-track.mp3",
                "mediaType": "audio/mpeg"
            }))
        }

        std::env::set_var("MOMOBKO_TEST_EXPECTED_REPO_ID", &expected_repo_id);
        set_test_backend_stat_entry_hook(Some(stat_hook));
        set_test_downloader_playback_hook(Some(playback_hook));
        let response = state
            .prepare_entry_playback_source(EntryPlaybackRequest {
                repo_id: repo_id.clone(),
                path: "Created/lazy-track.mp3".to_string(),
            })
            .expect("unindexed virtual playback source should resolve from backend stat");
        set_test_downloader_playback_hook(None);
        set_test_backend_stat_entry_hook(None);
        std::env::remove_var("MOMOBKO_TEST_EXPECTED_REPO_ID");

        assert_eq!(response.repo_id, repo_id);
        assert_eq!(response.path, "Created/lazy-track.mp3");
        assert_eq!(response.media_type, "audio/mpeg");
        assert_eq!(
            response.local_path.as_deref(),
            Some("C:/Mock/Temp/lazy-track.mp3")
        );

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn open_preview_file_source_rejects_unknown_token() {
        let (state, root, _repo_root, _thumbnail_root) = create_test_state("preview-unknown");

        let error = state
            .open_preview_file_source(&"0".repeat(64))
            .expect_err("unknown preview token should fail");

        assert!(error.contains("preview source not found"));
        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn open_preview_file_source_rejects_deleted_source_file() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("preview-deleted");
        let source_path = repo_root.join("model.glb");
        fs::write(&source_path, b"glb").expect("preview source should be written");
        let repo_id = create_repository_for_path(&state, &repo_root);
        let response = state
            .prepare_preview_file_source(FileReadRequest {
                repo_id,
                path: "model.glb".to_string(),
            })
            .expect("preview source should be prepared");
        fs::remove_file(source_path).expect("preview source should be removed");

        let error = state
            .open_preview_file_source(&response.token)
            .expect_err("deleted preview source should fail");

        assert!(error.contains("preview source file is no longer available"));
        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn delete_entry_moves_to_trash_then_permanently_deletes() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("trash-delete");
        let repo_id = create_repository_without_initial_sync(&state, &repo_root);
        fs::write(repo_root.join("note.txt"), "plain text").expect("test file should be written");
        state
            .sync_repository(SyncRequest {
                repo_id: repo_id.clone(),
            })
            .expect("repository should sync");

        state
            .delete_entry(FileDeleteRequest {
                repo_id: repo_id.clone(),
                path: "note.txt".to_string(),
                mode: None,
            })
            .expect("file should move to trash");

        assert!(!repo_root.join("note.txt").exists());
        let trash_dir = repository_trash_dir(&repo_root);
        let trash_entries = fs::read_dir(&trash_dir)
            .expect("trash directory should be readable")
            .collect::<Result<Vec<_>, _>>()
            .expect("trash entries should be readable");
        assert_eq!(trash_entries.len(), 1);
        let trash_path = trash_entries[0].file_name().to_string_lossy().to_string();

        let trash_snapshot = state
            .load_file_browser(FileBrowserRequest {
                repo_id: repo_id.clone(),
                directory_path: Some(String::new()),
                include_tree: Some(false),
                special_location: Some("trash".to_string()),
                offset: None,
                limit: None,
            })
            .expect("trash browser should load");
        let trash_entry = trash_snapshot
            .entries
            .iter()
            .find(|entry| entry.path == trash_path)
            .expect("trash entry should be listed");
        assert_eq!(
            trash_entry.metadata.get("originalPath"),
            Some(&serde_json::Value::String("note.txt".to_string()))
        );
        assert!(trash_entry
            .metadata
            .get("deletedAt")
            .and_then(serde_json::Value::as_str)
            .is_some());

        let snapshot = state
            .load_snapshot(&repo_id)
            .expect("snapshot should load after trash delete");
        assert!(snapshot.assets.is_empty());

        state
            .delete_entry(FileDeleteRequest {
                repo_id,
                path: trash_path,
                mode: Some("permanentDelete".to_string()),
            })
            .expect("trash entry should be permanently deleted");

        assert_eq!(
            fs::read_dir(trash_dir)
                .expect("trash directory should still exist")
                .count(),
            0
        );

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn trash_restore_all_and_empty_keep_directory_metadata() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("trash-restore");
        let repo_id = create_repository_without_initial_sync(&state, &repo_root);
        fs::create_dir_all(repo_root.join("Scenes/Act1"))
            .expect("test directory should be written");
        fs::write(repo_root.join("Scenes/Act1/shot.txt"), "plain text")
            .expect("test nested file should be written");
        fs::write(repo_root.join("loose.txt"), "plain text").expect("test file should be written");
        state
            .sync_repository(SyncRequest {
                repo_id: repo_id.clone(),
            })
            .expect("repository should sync");

        state
            .delete_entry(FileDeleteRequest {
                repo_id: repo_id.clone(),
                path: "loose.txt".to_string(),
                mode: None,
            })
            .expect("file should move to trash");
        assert!(!repo_root.join("loose.txt").exists());

        state
            .mutate_trash(TrashMutationRequest {
                repo_id: repo_id.clone(),
                action: "restore".to_string(),
                path: Some("loose.txt".to_string()),
            })
            .expect("file should restore from trash");
        assert!(repo_root.join("loose.txt").exists());

        state
            .delete_entry(FileDeleteRequest {
                repo_id: repo_id.clone(),
                path: "Scenes".to_string(),
                mode: None,
            })
            .expect("directory should move to trash");
        assert!(!repo_root.join("Scenes").exists());
        assert!(repository_trash_dir(&repo_root)
            .join("Scenes/Act1/shot.txt")
            .exists());

        let nested_trash_snapshot = state
            .load_file_browser(FileBrowserRequest {
                repo_id: repo_id.clone(),
                directory_path: Some("Scenes/Act1".to_string()),
                include_tree: Some(false),
                special_location: Some("trash".to_string()),
                offset: None,
                limit: None,
            })
            .expect("nested trash browser should load");
        let nested_entry = nested_trash_snapshot
            .entries
            .iter()
            .find(|entry| entry.path == "Scenes/Act1/shot.txt")
            .expect("nested trash entry should be listed");
        assert_eq!(
            nested_entry.metadata.get("originalPath"),
            Some(&serde_json::Value::String(
                "Scenes/Act1/shot.txt".to_string()
            ))
        );
        assert!(nested_entry
            .metadata
            .get("deletedAt")
            .and_then(serde_json::Value::as_str)
            .is_some());

        state
            .mutate_trash(TrashMutationRequest {
                repo_id: repo_id.clone(),
                action: "restoreAll".to_string(),
                path: None,
            })
            .expect("all trash entries should restore");
        assert!(repo_root.join("Scenes/Act1/shot.txt").exists());
        assert_eq!(
            fs::read_dir(repository_trash_dir(&repo_root))
                .expect("trash directory should exist")
                .count(),
            0
        );

        state
            .delete_entry(FileDeleteRequest {
                repo_id: repo_id.clone(),
                path: "loose.txt".to_string(),
                mode: None,
            })
            .expect("file should move to trash again");
        state
            .mutate_trash(TrashMutationRequest {
                repo_id,
                action: "empty".to_string(),
                path: None,
            })
            .expect("trash should empty");
        assert!(!repo_root.join("loose.txt").exists());
        assert_eq!(
            fs::read_dir(repository_trash_dir(&repo_root))
                .expect("trash directory should exist")
                .count(),
            0
        );

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn thumbnail_file_name_stays_within_windows_component_limit() {
        let asset_id = format!(
            "asset-{}",
            slugify_repo_id("startIcon.png", LONG_RELATIVE_PATH)
        );
        let repo_dir = thumbnail_repository_dir_name(&asset_id, LONG_RELATIVE_PATH);
        let file_name = thumbnail_file_name(
            &asset_id,
            LONG_RELATIVE_PATH,
            LONG_RELATIVE_PATH,
            "file",
            "generated",
        );

        assert!(repo_dir.len() <= 255);
        assert!(file_name.len() <= 255);
        assert!(file_name.ends_with(".jpg"));
        assert_eq!(file_name.len(), 68);
    }

    #[test]
    fn thumbnail_cache_names_are_stable() {
        let repo_dir = thumbnail_repository_dir_name("repo-cubism", "C:/Assets/Cubism");
        let file_name = thumbnail_file_name(
            "repo-cubism",
            "C:/Assets/Cubism",
            LONG_RELATIVE_PATH,
            "file",
            "generated",
        );

        assert_eq!(
            repo_dir,
            thumbnail_repository_dir_name("repo-cubism", "C:/Assets/Cubism")
        );
        assert_eq!(
            file_name,
            thumbnail_file_name(
                "repo-cubism",
                "C:/Assets/Cubism",
                LONG_RELATIVE_PATH,
                "file",
                "generated",
            )
        );
    }

    #[test]
    fn thumbnail_cache_names_differ_for_different_paths() {
        let first = thumbnail_file_name(
            "repo-cubism",
            "C:/Assets/Cubism",
            LONG_RELATIVE_PATH,
            "file",
            "generated",
        );
        let second = thumbnail_file_name(
            "repo-cubism",
            "C:/Assets/Cubism",
            "CubismSdkForNative-5-r.5/Samples/OpenGL/Demo/proj.harmonyos.cmake/Full/entry/src/ohosTest/resources/base/media/icon.png",
            "file",
            "generated",
        );

        assert_ne!(first, second);
    }

    #[test]
    fn video_thumbnail_ffmpeg_args_write_a_single_image() {
        let source_path = Path::new("C:/Assets/video.mp4");
        let thumbnail_path = Path::new("C:/Cache/thumbnail.jpg");
        let args = video_thumbnail_ffmpeg_args(source_path, thumbnail_path);

        assert!(args.windows(2).any(|items| items == ["-frames:v", "1"]));
        assert!(args.windows(2).any(|items| items == ["-update", "1"]));

        let update_index = args
            .iter()
            .position(|item| item == "-update")
            .expect("missing -update");
        let output_index = args
            .iter()
            .position(|item| item == thumbnail_path.as_os_str())
            .expect("missing output path");
        assert!(update_index < output_index);
    }

    #[test]
    fn audio_thumbnail_ffmpeg_args_extract_the_first_embedded_cover_stream() {
        let source_path = Path::new("C:/Assets/track.flac");
        let thumbnail_path = Path::new("C:/Cache/thumbnail.jpg");
        let args = audio_thumbnail_ffmpeg_args(source_path, thumbnail_path);

        assert!(args.windows(2).any(|items| items == ["-map", "0:v:0"]));
        assert!(args.windows(2).any(|items| items == ["-frames:v", "1"]));
        assert!(args.windows(2).any(|items| items == ["-update", "1"]));

        let map_index = args
            .iter()
            .position(|item| item == "-map")
            .expect("missing -map");
        let output_index = args
            .iter()
            .position(|item| item == thumbnail_path.as_os_str())
            .expect("missing output path");
        assert!(map_index < output_index);
    }

    #[test]
    fn audio_cover_probe_args_select_video_streams_as_json() {
        let source_path = Path::new("C:/Assets/track.mp3");
        let args = audio_cover_probe_args(source_path);

        assert!(args
            .windows(2)
            .any(|items| items == ["-select_streams", "v"]));
        assert!(args
            .windows(2)
            .any(|items| items == ["-show_entries", "stream=index"]));
        assert!(args.windows(2).any(|items| items == ["-of", "json"]));
        assert_eq!(args.last(), Some(&source_path.as_os_str().to_os_string()));
    }

    #[test]
    fn audio_cover_probe_output_reports_missing_streams() {
        assert!(!audio_cover_probe_output_has_stream(br#"{"streams":[]}"#)
            .expect("probe output should parse"));
        assert!(!audio_cover_probe_output_has_stream(br#"{}"#).expect("probe output should parse"));
    }

    #[test]
    fn audio_cover_probe_output_reports_present_streams() {
        assert!(
            audio_cover_probe_output_has_stream(br#"{"streams":[{"index":1}]}"#)
                .expect("probe output should parse")
        );
    }

    #[test]
    fn repository_journal_mode_falls_back_for_locking_protocol_errors() {
        let error = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_PROTOCOL),
            Some("locking protocol".to_string()),
        );

        assert!(should_fallback_repository_journal_mode(&error));
    }

    #[test]
    fn repository_journal_mode_keeps_original_error_for_other_sqlite_failures() {
        let error = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
            Some("database is locked".to_string()),
        );

        assert!(!should_fallback_repository_journal_mode(&error));
    }
}
