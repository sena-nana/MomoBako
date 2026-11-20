//! 网易云仓库识别、本地缓存配置与播放列表按路径添加的测试。

use super::*;

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
    install_netease_source_test_plugin_archive(&service_root);
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

    let cache_root = canonicalize_local_path(&cache_root).unwrap_or(cache_root);
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
    let (state, root, repo_root, _thumbnail_root) = create_test_state("playlist-items-by-paths");
    fs::create_dir_all(repo_root.join("Albums/Disc 1")).expect("album directory should be created");
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
