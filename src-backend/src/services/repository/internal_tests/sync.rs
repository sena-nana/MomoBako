//! 目录树、增量同步、发现文件补全与文件浏览缓存的测试。

use super::*;

#[test]
fn repository_tree_reports_direct_file_counts_per_directory() {
    let (state, root, repo_root, _thumbnail_root) = create_test_state("repository-tree-file-count");
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

#[test]
fn repository_tree_syncs_empty_directory_changes_from_filesystem_plugin() {
    let (state, root, repo_root, _thumbnail_root) =
        create_test_state("repository-tree-empty-directory-sync");
    fs::create_dir_all(repo_root.join("Empty/Child"))
        .expect("empty child directory should be created");
    let repo_id = create_repository_without_initial_sync(&state, &repo_root);

    state
        .sync_repository(SyncRequest {
            repo_id: repo_id.clone(),
        })
        .expect("repository should sync");

    let snapshot = state
        .load_repository_tree(&repo_id)
        .expect("repository tree should load");
    let empty = snapshot
        .tree
        .iter()
        .find(|node| node.path == "Empty")
        .expect("empty directory should be indexed");
    assert!(empty.children.iter().any(|node| node.path == "Empty/Child"));

    fs::remove_dir_all(repo_root.join("Empty")).expect("empty directory should be removed");
    state
        .sync_repository(SyncRequest {
            repo_id: repo_id.clone(),
        })
        .expect("repository should sync after directory removal");

    let snapshot = state
        .load_repository_tree(&repo_id)
        .expect("repository tree should load after directory removal");
    assert!(snapshot.tree.iter().all(|node| node.path != "Empty"));
    fs::remove_dir_all(root).expect("test temp root should be removed");
}

#[test]
fn changed_path_sync_creates_updates_and_deletes_file() {
    let (state, root, repo_root, _thumbnail_root) = create_test_state("changed-path-file-sync");
    let repo_id = create_repository_without_initial_sync(&state, &repo_root);

    fs::write(repo_root.join("alpha.txt"), "one").expect("test file should be written");
    let created = state
        .sync_repository_changed_paths(&repo_id, &changed_paths(&["alpha.txt"]))
        .expect("changed file should sync");
    assert_eq!(created.created_assets, 1);
    assert_eq!(
        asset_status_for_path(&state, &repo_id, "alpha.txt").as_deref(),
        Some("synced")
    );

    fs::write(repo_root.join("alpha.txt"), "updated file body")
        .expect("test file should be updated");
    let updated = state
        .sync_repository_changed_paths(&repo_id, &changed_paths(&["alpha.txt"]))
        .expect("updated file should sync");
    assert_eq!(updated.updated_assets, 1);
    assert_eq!(
        asset_size_for_path(&state, &repo_id, "alpha.txt"),
        Some("updated file body".len() as i64)
    );

    fs::remove_file(repo_root.join("alpha.txt")).expect("test file should be removed");
    let deleted = state
        .sync_repository_changed_paths(&repo_id, &changed_paths(&["alpha.txt"]))
        .expect("deleted file should sync");
    assert_eq!(deleted.deleted_assets, 1);
    assert_eq!(
        asset_status_for_path(&state, &repo_id, "alpha.txt").as_deref(),
        Some("deleted")
    );

    fs::remove_dir_all(root).expect("test temp root should be removed");
}

#[test]
fn changed_path_sync_expands_and_deletes_directory_prefix() {
    let (state, root, repo_root, _thumbnail_root) =
        create_test_state("changed-path-directory-sync");
    let repo_id = create_repository_without_initial_sync(&state, &repo_root);
    fs::create_dir_all(repo_root.join("Scenes/Act1")).expect("nested directory should be created");
    fs::write(repo_root.join("Scenes/Act1/shot-a.txt"), "shot a")
        .expect("first nested file should be written");
    fs::write(repo_root.join("Scenes/Act1/shot-b.txt"), "shot b")
        .expect("second nested file should be written");

    let created = state
        .sync_repository_changed_paths(&repo_id, &changed_paths(&["Scenes"]))
        .expect("changed directory should sync");
    assert_eq!(created.created_assets, 2);
    let tree = state
        .load_repository_tree(&repo_id)
        .expect("repository tree should load after changed sync");
    assert!(tree.tree.iter().any(|node| node.path == "Scenes"));

    fs::remove_dir_all(repo_root.join("Scenes")).expect("nested directory should be removed");
    let deleted = state
        .sync_repository_changed_paths(&repo_id, &changed_paths(&["Scenes"]))
        .expect("deleted directory should sync");
    assert_eq!(deleted.deleted_assets, 2);
    assert_eq!(
        asset_status_for_path(&state, &repo_id, "Scenes/Act1/shot-a.txt").as_deref(),
        Some("deleted")
    );
    assert_eq!(
        asset_status_for_path(&state, &repo_id, "Scenes/Act1/shot-b.txt").as_deref(),
        Some("deleted")
    );
    let tree = state
        .load_repository_tree(&repo_id)
        .expect("repository tree should load after directory deletion");
    assert!(tree.tree.iter().all(|node| node.path != "Scenes"));

    fs::remove_dir_all(root).expect("test temp root should be removed");
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
fn supplement_discovered_files_with_hint_paths_adds_missing_watched_file() {
    let _lock = playback_test_lock();
    fn stat_hook(
        _repo: &RepositoryRecord,
        repo_root: &Path,
        entry_path: &str,
    ) -> Option<Result<FileSystemEntry, String>> {
        if entry_path != "fresh.txt" {
            return None;
        }
        let absolute_path = resolve_repository_relative_path(repo_root, entry_path)
            .expect("fresh file path should resolve");
        let metadata = fs::metadata(&absolute_path).expect("fresh file should exist");
        Some(Ok(FileSystemEntry {
            path: entry_path.to_string(),
            name: "fresh.txt".to_string(),
            kind: FileSystemEntryKind::File,
            extension: Some("txt".to_string()),
            size_bytes: Some(metadata.len() as i64),
            modified_at: metadata
                .modified()
                .ok()
                .map(system_time_to_rfc3339)
                .transpose()
                .expect("modified time should serialize"),
            is_virtual: false,
            provider_id: None,
            provider_item_id: None,
            source_payload: None,
            local_absolute_path: Some(absolute_path.to_string_lossy().to_string()),
            status: None,
            shared_asset_id: None,
            tags: None,
            thumbnail_local_absolute_path: None,
        }))
    }

    let (state, root, repo_root, _thumbnail_root) = create_test_state("sync-hint-path");
    let repo_id = create_repository_without_initial_sync(&state, &repo_root);
    fs::write(repo_root.join("fresh.txt"), "fresh").expect("fresh file should be written");
    let repo = state
        .load_repository_record(&repo_id)
        .expect("repository record should load");
    let hint_paths = ["fresh.txt".to_string()]
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();

    set_test_backend_stat_entry_hook(Some(stat_hook));
    let files = supplement_discovered_files_with_hint_paths(
        &state.root,
        &repo,
        &repo_root,
        Vec::new(),
        &hint_paths,
    )
    .expect("hint paths should be supplemented from backend stat");
    set_test_backend_stat_entry_hook(None);

    assert_eq!(files.len(), 1);
    assert_eq!(files[0].relative_path, "fresh.txt");
    assert_eq!(files[0].size_bytes, 5);

    fs::remove_dir_all(root).expect("test temp root should be removed");
}

#[test]
fn supplement_discovered_files_with_hint_paths_recurses_directory_hints() {
    let (state, root, repo_root, _thumbnail_root) = create_test_state("sync-hint-directory");
    let repo_id = create_repository_without_initial_sync(&state, &repo_root);
    fs::create_dir_all(repo_root.join("incoming/nested"))
        .expect("hint directory should be created");
    fs::write(repo_root.join("incoming/fresh.txt"), "fresh")
        .expect("direct child file should be written");
    fs::write(repo_root.join("incoming/nested/deep.txt"), "deep")
        .expect("nested child file should be written");
    let repo = state
        .load_repository_record(&repo_id)
        .expect("repository record should load");
    let hint_paths = ["incoming".to_string()]
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();

    let files = supplement_discovered_files_with_hint_paths(
        &state.root,
        &repo,
        &repo_root,
        Vec::new(),
        &hint_paths,
    )
    .expect("directory hint paths should be expanded recursively");

    let discovered_paths = files
        .into_iter()
        .map(|file| file.relative_path)
        .collect::<Vec<_>>();
    assert_eq!(
        discovered_paths,
        vec![
            "incoming/fresh.txt".to_string(),
            "incoming/nested/deep.txt".to_string()
        ]
    );

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
    let (state, root, repo_root, _thumbnail_root) = create_test_state("netease-system-metadata");
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
            })
            .to_string()
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
    let metadata_keys = source_metadata_mirror_keys(&state.root, &repo.backend_record.plugin_id);
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

    drop(connection);
    fs::remove_dir_all(root).expect("test temp root should be removed");
}

#[test]
fn metadata_parser_source_projection_only_contains_manifest_keys() {
    let projected = project_source_metadata(
        Some(&serde_json::json!({
            "songId": 3301,
            "songName": "公开歌曲",
            "accountCookie": "MUSIC_U=secret",
            "privateSession": { "token": "secret" }
        })),
        &["songId".to_string(), "songName".to_string()],
    )
    .expect("whitelisted source metadata should be projected");

    assert_eq!(projected.get("songId"), Some(&serde_json::json!(3301)));
    assert_eq!(
        projected.get("songName"),
        Some(&serde_json::json!("公开歌曲"))
    );
    assert!(!projected.contains_key("accountCookie"));
    assert!(!projected.contains_key("privateSession"));
}

#[test]
fn netease_page_asset_hydration_applies_parser_defaults_without_overwriting_user_metadata() {
    let (state, root, repo_root, _thumbnail_root) =
        create_test_state("netease-page-parser-defaults");
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
    let path = "创建的歌单/分页歌单/歌手 A - 第一页.mp3";
    let entry = FileSystemEntry {
        path: path.to_string(),
        name: "歌手 A - 第一页.mp3".to_string(),
        kind: FileSystemEntryKind::File,
        extension: Some("mp3".to_string()),
        size_bytes: Some(0),
        modified_at: Some(now_rfc3339()),
        is_virtual: true,
        provider_id: Some(NETEASE_CLOUD_MUSIC_PROVIDER_ID.to_string()),
        provider_item_id: Some("3301".to_string()),
        source_payload: Some(serde_json::json!({
            "songId": 3301,
            "songName": "第一页"
        })),
        local_absolute_path: None,
        status: None,
        shared_asset_id: None,
        tags: None,
        thumbnail_local_absolute_path: None,
    };
    let parser_defaults = BTreeMap::from([(
        path.to_string(),
        BTreeMap::from([
            ("title".to_string(), serde_json::json!("第一页")),
            ("artist".to_string(), serde_json::json!("歌手 A")),
        ]),
    )]);
    let source_keys = vec!["songId".to_string(), "songName".to_string()];

    let tx = connection.transaction().expect("transaction should start");
    mirror_netease_entries_to_assets(
        &tx,
        &repo_id,
        std::slice::from_ref(&entry),
        &now_rfc3339(),
        &source_keys,
        &parser_defaults,
    )
    .expect("paged asset metadata should hydrate");
    tx.commit().expect("transaction should commit");
    let metadata = metadata_for_asset_path(&state, &repo_id, path);
    assert_eq!(metadata.get("title"), Some(&serde_json::json!("第一页")));
    assert_eq!(metadata.get("artist"), Some(&serde_json::json!("歌手 A")));

    let tx = connection
        .transaction()
        .expect("second transaction should start");
    let asset_id = asset_id_for_path(&repo_id, path);
    upsert_metadata_value(&tx, &asset_id, "title", &serde_json::json!("用户标题"))
        .expect("user title should update");
    mirror_netease_entries_to_assets(
        &tx,
        &repo_id,
        &[entry],
        &now_rfc3339(),
        &source_keys,
        &parser_defaults,
    )
    .expect("paged asset metadata should hydrate again");
    tx.commit().expect("second transaction should commit");
    let metadata = metadata_for_asset_path(&state, &repo_id, path);
    assert_eq!(metadata.get("title"), Some(&serde_json::json!("用户标题")));
    assert_eq!(metadata.get("artist"), Some(&serde_json::json!("歌手 A")));

    drop(connection);
    fs::remove_dir_all(root).expect("test temp root should be removed");
}

#[test]
fn load_file_browser_uses_netease_cached_page_without_backend_roundtrip() {
    let (state, root, repo_root, _thumbnail_root) = create_test_state("netease-browser-cache-hit");
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
            status: None,
            shared_asset_id: None,
            tags: None,
            thumbnail_local_absolute_path: None,
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
            status: None,
            shared_asset_id: None,
            tags: None,
            thumbnail_local_absolute_path: None,
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

    drop(connection);
    fs::remove_dir_all(root).expect("test temp root should be removed");
}
