//! 仓库后端校验、创建、迁移位置、删除与最近访问记录的测试。

use super::*;

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
        error.contains("plugin is disabled") || error.contains("plugin runtime is not available")
    );
}

#[test]
fn local_filesystem_backend_without_native_runtime_is_rejected() {
    let workspace = TestWorkspace::new("runtime-local-backend");
    let service_root = workspace.path("service");
    let repo_root = workspace.path("repo");
    install_local_filesystem_test_plugin_archive(&service_root);
    disable_local_filesystem_adapter();
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
    let (state, root, repo_root, _thumbnail_root) = create_test_state("update-repo-backend-config");
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
    let metadata_raw = fs::read_to_string(metadata_path).expect("repository metadata should exist");
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
fn delete_repository_record_only_removes_registry_and_keeps_disk_data() {
    let (state, root, repo_root, _thumbnail_root) = create_test_state("delete-repo-record-only");
    let repo_id = create_repository_for_path(&state, &repo_root);
    let metadata_dir = repo_root.join(REPO_META_DIR);
    let managed_state_dir = repository_state_storage_dir(&state.root, &repo_id);
    fs::create_dir_all(managed_state_dir.join("cache"))
        .expect("managed state dir should be created");
    fs::write(managed_state_dir.join("cache/index.json"), "{}")
        .expect("managed cache file should be written");

    state
        .delete_repository(RepositoryDeleteRequest {
            repo_id: repo_id.clone(),
            mode: RepositoryDeleteMode::RecordOnly,
        })
        .expect("repository should delete");

    assert!(managed_state_dir.exists());
    assert!(metadata_dir.exists());
    assert!(repo_root.exists());
    assert!(state
        .list_repositories()
        .expect("repositories should list after delete")
        .iter()
        .all(|item| item.repo_id != repo_id));
    fs::remove_dir_all(root).expect("test temp root should be removed");
}

#[test]
fn delete_repository_delete_metadata_removes_local_metadata_dirs() {
    let (state, root, repo_root, _thumbnail_root) = create_test_state("delete-repo-local-metadata");
    let repo_id = create_repository_for_path(&state, &repo_root);
    let metadata_dir = repo_root.join(REPO_META_DIR);
    let legacy_metadata_dir = repo_root.join(LEGACY_REPO_META_DIR);
    fs::create_dir_all(legacy_metadata_dir.join("cache"))
        .expect("legacy metadata dir should be created");
    fs::write(legacy_metadata_dir.join("cache").join("legacy.json"), "{}")
        .expect("legacy metadata file should be written");

    state
        .delete_repository(RepositoryDeleteRequest {
            repo_id: repo_id.clone(),
            mode: RepositoryDeleteMode::DeleteMetadata,
        })
        .expect("repository metadata should delete");

    assert!(!metadata_dir.exists());
    assert!(!legacy_metadata_dir.exists());
    assert!(repo_root.exists());
    assert!(state
        .list_repositories()
        .expect("repositories should list after delete")
        .iter()
        .all(|item| item.repo_id != repo_id));
    fs::remove_dir_all(root).expect("test temp root should be removed");
}

#[test]
fn delete_repository_delete_metadata_removes_managed_state_dir_for_uri_repo() {
    let workspace = TestWorkspace::new("delete-repo-managed-metadata");
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
            "accountId": "456",
            "cookie": "MUSIC_U=test-cookie"
        }),
    };
    let seed = RepositorySeed {
        repo_id: "netease-delete-metadata",
        name: "网易云删除元数据",
        root_path: "",
        status: "ready",
        assets: &[],
    };
    upsert_registry_entry(
        &registry,
        Path::new("netease-cloud-music://account/456"),
        &seed,
        &backend,
    )
    .expect("registry entry should be stored");
    let managed_state_dir = repository_state_storage_dir(&service_root, "netease-delete-metadata");
    let metadata_dir = managed_state_dir.join(REPO_META_DIR);
    fs::create_dir_all(metadata_dir.join("indexes"))
        .expect("managed metadata indexes should be created");
    fs::write(metadata_dir.join("indexes").join("legacy.json"), "{}")
        .expect("managed metadata file should be written");

    state
        .delete_repository(RepositoryDeleteRequest {
            repo_id: "netease-delete-metadata".to_string(),
            mode: RepositoryDeleteMode::DeleteMetadata,
        })
        .expect("managed repository metadata should delete");

    assert!(!metadata_dir.exists());
    assert!(!managed_state_dir.exists());
    assert!(state
        .list_repositories()
        .expect("repositories should list after delete")
        .iter()
        .all(|item| item.repo_id != "netease-delete-metadata"));
}

#[test]
fn delete_repository_delete_folder_removes_repository_root_and_managed_state() {
    let (state, root, repo_root, _thumbnail_root) = create_test_state("delete-repo-folder");
    let repo_id = create_repository_for_path(&state, &repo_root);
    let managed_state_dir = repository_state_storage_dir(&state.root, &repo_id);
    fs::create_dir_all(managed_state_dir.join("cache"))
        .expect("managed state dir should be created");
    fs::write(managed_state_dir.join("cache/index.json"), "{}")
        .expect("managed cache file should be written");

    state
        .delete_repository(RepositoryDeleteRequest {
            repo_id: repo_id.clone(),
            mode: RepositoryDeleteMode::DeleteFolder,
        })
        .expect("repository folder should delete");

    assert!(!repo_root.exists());
    assert!(!managed_state_dir.exists());
    assert!(state
        .list_repositories()
        .expect("repositories should list after delete")
        .iter()
        .all(|item| item.repo_id != repo_id));
    fs::remove_dir_all(root).expect("test temp root should be removed");
}

#[test]
fn delete_repository_delete_folder_rejects_missing_repository_path() {
    let (state, root, repo_root, _thumbnail_root) = create_test_state("delete-repo-folder-missing");
    let repo_id = create_repository_for_path(&state, &repo_root);
    fs::remove_dir_all(&repo_root).expect("repository root should be removed");

    let error = state
        .delete_repository(RepositoryDeleteRequest {
            repo_id: repo_id.clone(),
            mode: RepositoryDeleteMode::DeleteFolder,
        })
        .expect_err("missing repository folder should reject deletion");

    assert!(error.contains("repository folder is not available for deletion"));
    assert!(state
        .list_repositories()
        .expect("repositories should still list after failed delete")
        .iter()
        .any(|item| item.repo_id == repo_id));
    fs::remove_dir_all(root).expect("test temp root should be removed");
}

#[test]
fn delete_repository_delete_folder_removes_netease_cache_root() {
    let workspace = TestWorkspace::new("delete-repo-netease-folder");
    let service_root = workspace.path("service");
    let cache_root = workspace.path("netease-cache");
    fs::create_dir_all(&cache_root).expect("netease cache root should be created");
    let state = RepositoryState::from_root(service_root.clone());
    let repo_id = create_netease_repository_without_initial_sync(
        &state,
        &cache_root,
        serde_json::json!({
            "accountId": "789",
            "cookie": "MUSIC_U=test"
        }),
    );

    state
        .delete_repository(RepositoryDeleteRequest {
            repo_id: repo_id.clone(),
            mode: RepositoryDeleteMode::DeleteFolder,
        })
        .expect("netease cache root should delete");

    assert!(!cache_root.exists());
    assert!(state
        .list_repositories()
        .expect("repositories should list after delete")
        .iter()
        .all(|item| item.repo_id != repo_id));
}

#[test]
fn record_entry_access_keeps_only_latest_50_entries() {
    let (state, root, repo_root, _thumbnail_root) = create_test_state("recent-access-cap");
    let repo_id = create_repository_without_initial_sync(&state, &repo_root);
    for index in 0..55 {
        fs::write(
            repo_root.join(format!("recent-{index:02}.txt")),
            format!("entry-{index}"),
        )
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
    drop(connection);
    fs::remove_dir_all(root).expect("test temp root should be removed");
}

#[test]
fn clear_recent_access_history_resets_all_last_accessed_at() {
    let (state, root, repo_root, _thumbnail_root) = create_test_state("recent-access-clear");
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
    drop(connection);
    fs::remove_dir_all(root).expect("test temp root should be removed");
}
