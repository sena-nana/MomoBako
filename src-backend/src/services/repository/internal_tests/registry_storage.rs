//! 仓库注册表迁移、元数据存储重建与 SQLite 连接配置的测试。

use super::*;

#[test]
fn registry_migration_rewrites_legacy_netease_source_id_idempotently() {
    let connection = Connection::open_in_memory().expect("registry should open");
    connection
        .execute_batch(REGISTRY_SCHEMA_SQL)
        .expect("registry schema should initialize");
    let now = now_rfc3339();
    connection
        .execute(
            r#"
            INSERT INTO repositories (
              repo_id, name, path, backend_plugin_id, backend_config_json,
              status, created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, '{}', 'ready', ?5, ?5)
            "#,
            params![
                "legacy-netease",
                "Legacy Netease",
                "netease-cloud-music://account/42",
                LEGACY_NETEASE_CLOUD_MUSIC_PLUGIN_ID,
                now,
            ],
        )
        .expect("legacy registry record should insert");

    migrate_registry_schema(&connection).expect("source id migration should succeed");
    migrate_registry_schema(&connection).expect("source id migration should be idempotent");

    let plugin_id: String = connection
        .query_row(
            "SELECT backend_plugin_id FROM repositories WHERE repo_id = 'legacy-netease'",
            [],
            |row| row.get(0),
        )
        .expect("migrated source id should load");
    assert_eq!(plugin_id, NETEASE_CLOUD_MUSIC_PLUGIN_ID);
}

#[test]
fn repository_migration_moves_audio_playlists_to_audio_player_idempotently() {
    let connection = Connection::open_in_memory().expect("repository should open");
    connection
        .execute_batch(REPOSITORY_SCHEMA_SQL)
        .expect("repository schema should initialize");
    let now = now_rfc3339();
    connection
        .execute(
            r#"
            INSERT INTO repositories (
              repo_id, name, root_path, schema_version, created_at, updated_at
            ) VALUES (?1, ?2, ?3, 3, ?4, ?4)
            "#,
            params!["repo-audio-migration", "Audio Repo", "C:/Mock/Audio", now],
        )
        .expect("repository row should insert");
    connection
        .execute(
            r#"
            INSERT INTO playlists (
              playlist_id, repo_id, name, player_type_id, player_plugin_id,
              player_label, file_class, sort_order, created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'audio', 0, ?7, ?7)
            "#,
            params![
                "legacy-audio-playlist",
                "repo-audio-migration",
                "Legacy Audio",
                "momobako.playlist.audio-sequence",
                LEGACY_AUDIO_PLAYER_PLUGIN_ID,
                "Audio",
                now,
            ],
        )
        .expect("legacy playlist should insert");

    ensure_repository_schema_current(&connection).expect("audio player migration should succeed");
    ensure_repository_schema_current(&connection)
        .expect("audio player migration should be idempotent");

    let plugin_id: String = connection
        .query_row(
            "SELECT player_plugin_id FROM playlists WHERE playlist_id = 'legacy-audio-playlist'",
            [],
            |row| row.get(0),
        )
        .expect("migrated playlist should load");
    assert_eq!(plugin_id, AUDIO_PLAYER_PLUGIN_ID);
}

#[test]
fn source_data_directory_migration_merges_missing_files_idempotently() {
    let workspace = TestWorkspace::new("source-data-directory-migration");
    let service_root = workspace.path("service");
    let legacy = plugin_data_dir(&service_root, LEGACY_NETEASE_CLOUD_MUSIC_PLUGIN_ID);
    let current = plugin_data_dir(&service_root, NETEASE_CLOUD_MUSIC_PLUGIN_ID);
    fs::create_dir_all(legacy.join("cache")).expect("legacy data dir should exist");
    fs::create_dir_all(&current).expect("current data dir should exist");
    fs::write(legacy.join("last-session.json"), b"legacy-session")
        .expect("legacy session should write");
    fs::write(legacy.join("cache/index.json"), b"legacy-index")
        .expect("legacy nested data should write");
    fs::write(current.join("last-session.json"), b"current-session")
        .expect("current session should write");

    migrate_builtin_plugin_data_dirs(&service_root).expect("source data migration should succeed");
    migrate_builtin_plugin_data_dirs(&service_root)
        .expect("source data migration should be idempotent");

    assert_eq!(
        fs::read(current.join("last-session.json")).expect("current session should read"),
        b"current-session"
    );
    assert_eq!(
        fs::read(current.join("cache/index.json")).expect("nested data should migrate"),
        b"legacy-index"
    );
    assert!(
        legacy.is_dir(),
        "legacy Source data remains recoverable after merge"
    );
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
fn sync_repository_recovers_missing_database_before_writing_assets() {
    let (state, root, repo_root, _thumbnail_root) = create_test_state("sync-rebuild-missing-db");
    fs::write(repo_root.join("track.mp3"), b"demo").expect("test file should be written");
    let repo_id = create_repository_for_path(&state, &repo_root);

    fs::remove_file(repo_root.join(REPO_META_DIR).join(REPO_DB_FILE_NAME))
        .expect("repository database should be removed");

    let result = state
        .sync_repository(SyncRequest {
            repo_id: repo_id.clone(),
        })
        .expect("sync should rebuild repository identity before indexing assets");

    assert_eq!(result.scanned_files, 1);
    assert_eq!(
        asset_status_for_path(&state, &repo_id, "track.mp3").as_deref(),
        Some("synced")
    );

    fs::remove_dir_all(root).expect("test workspace should be removed");
}

#[test]
fn list_repositories_recovers_asset_count_after_metadata_loss() {
    let (state, root, repo_root, _thumbnail_root) =
        create_test_state("list-rebuild-meta-asset-count");
    fs::write(repo_root.join("track.mp3"), b"demo").expect("test file should be written");
    let repo_id = create_repository_for_path(&state, &repo_root);

    fs::remove_dir_all(repo_root.join(REPO_META_DIR)).expect("metadata dir should be removed");

    let repositories = state
        .list_repositories()
        .expect("repository summaries should recover missing storage");
    let rebuilt_summary = repositories
        .iter()
        .find(|repository| repository.repo_id == repo_id)
        .expect("repository summary should exist");

    assert_eq!(rebuilt_summary.asset_count, 1);
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
fn list_repositories_reads_existing_database_during_active_writer() {
    let (state, root, repo_root, _thumbnail_root) = create_test_state("list-during-active-writer");
    fs::write(repo_root.join("track.mp3"), b"demo").expect("test file should be written");
    let repo_id = create_repository_for_path(&state, &repo_root);
    let database_path = repo_root.join(REPO_META_DIR).join(REPO_DB_FILE_NAME);
    let writer = Connection::open(&database_path).expect("writer connection should open");
    configure_repository_connection(&writer).expect("writer connection should be configured");
    writer
        .execute("BEGIN IMMEDIATE", [])
        .expect("writer transaction should start");
    writer
        .execute(
            "UPDATE repositories SET updated_at = updated_at WHERE repo_id = ?1",
            [&repo_id],
        )
        .expect("writer should hold an uncommitted update");

    let repositories = state
        .list_repositories()
        .expect("repository summaries should load while writer is active");
    let summary = repositories
        .iter()
        .find(|repository| repository.repo_id == repo_id)
        .expect("repository summary should exist");

    assert_eq!(summary.asset_count, 1);
    writer
        .execute("ROLLBACK", [])
        .expect("writer transaction should roll back");
    drop(writer);
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
    assert_eq!(rebuilt_summary.asset_count, 2);
    assert!(repo_root
        .join(REPO_META_DIR)
        .join(REPO_DB_FILE_NAME)
        .exists());

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
    drop(connection);
    fs::remove_dir_all(root).expect("test workspace should be removed");
}

#[test]
fn registry_connection_enables_wal_and_busy_timeout() {
    let workspace = TestWorkspace::new("registry-connection-config");
    let registry_path = workspace.path("registry.db");
    fs::create_dir_all(workspace.path("")).expect("workspace root should be created");

    let connection =
        open_registry_connection(&registry_path).expect("registry connection should open");
    let journal_mode: String = connection
        .pragma_query_value(None, "journal_mode", |row| row.get(0))
        .expect("journal mode pragma should succeed");
    let busy_timeout: i64 = connection
        .pragma_query_value(None, "busy_timeout", |row| row.get(0))
        .expect("busy timeout pragma should succeed");

    assert_eq!(journal_mode.to_ascii_lowercase(), "wal");
    assert_eq!(busy_timeout, 5_000);
}

#[test]
fn repository_connection_configuration_skips_reapplying_wal_during_active_writer() {
    let workspace = TestWorkspace::new("repository-connection-lock");
    let database_path = workspace.path("metadata.db");
    fs::create_dir_all(workspace.path("")).expect("workspace root should be created");

    let seed_connection = open_repository_database_connection(&database_path)
        .expect("repository database should initialize");
    drop(seed_connection);

    let writer = Connection::open(&database_path).expect("writer connection should open");
    configure_repository_connection(&writer).expect("writer connection should be configured");
    writer
        .execute("BEGIN IMMEDIATE", [])
        .expect("writer transaction should start");

    let contender = Connection::open(&database_path).expect("contender connection should open");
    configure_repository_connection(&contender)
        .expect("contender should reuse wal mode without lock error");
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
    assert!(is_database_locked_error(&error));
}
