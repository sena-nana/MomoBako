//! 条目复制移动、硬链接候选与回收站流程的测试。

use super::*;

#[test]
fn copy_entries_records_linked_hardlink_member() {
    let (state, root, repo_root, _thumbnail_root) = create_test_state("copy-hardlink");
    fs::create_dir_all(repo_root.join("Copies")).expect("copies folder should be created");
    fs::write(repo_root.join("source.txt"), b"copy me").expect("source file should be written");
    let repo_id = create_repository_for_path(&state, &repo_root);

    state
        .copy_entries_cancellable(
            FileCopyRequest {
                repo_id: repo_id.clone(),
                source_paths: vec!["source.txt".to_string()],
                parent_path: Some("Copies".to_string()),
                mode: None,
            },
            &NeverCancelled,
        )
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
        .copy_entries_cancellable(
            FileCopyRequest {
                repo_id,
                source_paths: vec!["source.txt".to_string()],
                parent_path: Some("".to_string()),
                mode: None,
            },
            &NeverCancelled,
        )
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
        .move_entries_cancellable(
            FileMoveRequest {
                repo_id: repo_id.clone(),
                source_paths: vec!["note.txt".to_string()],
                parent_path: "Archive".to_string(),
            },
            &NeverCancelled,
        )
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
        .move_entries_cancellable(
            FileMoveRequest {
                repo_id,
                source_paths: vec!["source.txt".to_string()],
                parent_path: String::new(),
            },
            &NeverCancelled,
        )
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
        .move_entries_cancellable(
            FileMoveRequest {
                repo_id,
                source_paths: vec!["Scenes".to_string()],
                parent_path: "Scenes/Act1".to_string(),
            },
            &NeverCancelled,
        )
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
        .copy_entries_cancellable(
            FileCopyRequest {
                repo_id: repo_id.clone(),
                source_paths: vec!["source.txt".to_string()],
                parent_path: Some("Copies".to_string()),
                mode: Some("copy".to_string()),
            },
            &NeverCancelled,
        )
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
    let (state, root, repo_root, _thumbnail_root) = create_test_state("confirm-hardlink-changed");
    fs::write(repo_root.join("source.txt"), b"same bytes").expect("source file should be written");
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

    fs::write(repo_root.join("copy.txt"), b"changed bytes").expect("copy file should be modified");
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
    let (state, root, repo_root, _thumbnail_root) = create_test_state("hardlink-stale-candidate");
    fs::write(repo_root.join("source.txt"), b"same bytes").expect("source file should be written");
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
    fs::create_dir_all(repo_root.join("Scenes/Act1")).expect("test directory should be written");
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
