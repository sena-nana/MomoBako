//! 撤销重做、插件元数据默认值与同步时色板、哈希提取的测试。

use super::*;

#[test]
fn undo_and_redo_follow_revision_cursor_without_replaying_action_rows() {
    let (state, root, repo_root, _thumbnail_root) = create_test_state("revision-cursor");
    fs::write(repo_root.join("note.txt"), "hello").expect("test file should be written");
    let repo_id = create_repository_for_path(&state, &repo_root);
    let asset_id = asset_id_for_test_path(&state, &repo_id, "note.txt");
    let initial = state
        .load_asset_detail(&repo_id, &asset_id)
        .expect("initial asset detail should load");

    let first = state
        .update_asset_metadata(MetadataUpdateRequest {
            repo_id: repo_id.clone(),
            asset_id: asset_id.clone(),
            expected_version: initial.summary.version,
            metadata: BTreeMap::from([("rating".to_string(), serde_json::json!(1))]),
            source: Some("test".to_string()),
        })
        .expect("first metadata update should succeed");
    let second = state
        .update_asset_metadata(MetadataUpdateRequest {
            repo_id: repo_id.clone(),
            asset_id: asset_id.clone(),
            expected_version: first.asset.summary.version,
            metadata: BTreeMap::from([("rating".to_string(), serde_json::json!(2))]),
            source: Some("test".to_string()),
        })
        .expect("second metadata update should succeed");
    assert_eq!(
        second
            .asset
            .metadata
            .iter()
            .find(|entry| entry.key == "rating")
            .map(|entry| &entry.value),
        Some(&serde_json::json!(2))
    );

    let undone = state
        .undo_last_revision(RevisionActionRequest {
            repo_id: repo_id.clone(),
            asset_id: asset_id.clone(),
        })
        .expect("undo should restore the first value");
    assert_eq!(
        undone
            .asset
            .metadata
            .iter()
            .find(|entry| entry.key == "rating")
            .map(|entry| &entry.value),
        Some(&serde_json::json!(1))
    );

    let redone = state
        .redo_last_revision(RevisionActionRequest {
            repo_id: repo_id.clone(),
            asset_id: asset_id.clone(),
        })
        .expect("redo should restore the second value");
    assert_eq!(
        redone
            .asset
            .metadata
            .iter()
            .find(|entry| entry.key == "rating")
            .map(|entry| &entry.value),
        Some(&serde_json::json!(2))
    );
    assert!(state
        .redo_last_revision(RevisionActionRequest {
            repo_id: repo_id.clone(),
            asset_id: asset_id.clone(),
        })
        .is_err());

    drop(state);
    fs::remove_dir_all(root).expect("test root should be removed");
}

#[test]
fn plugin_metadata_defaults_preserve_existing_values() {
    let (state, root, repo_root, _thumbnail_root) = create_test_state("plugin-metadata-defaults");
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

    fs::write(repo_root.join("second.txt"), "second").expect("second test file should be written");
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
    let (state, root, repo_root, _thumbnail_root) = create_test_state("sync-hardlink-candidate");
    fs::write(repo_root.join("source.txt"), b"same bytes").expect("source file should be written");
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
