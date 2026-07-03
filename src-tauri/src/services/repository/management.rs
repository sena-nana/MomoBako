//! Repository management workflows such as create, import, relocate, export, and sync.

use super::*;

pub(super) fn create_repository(
    state: &RepositoryState,
    request: RepositoryMutationRequest,
) -> Result<RepositoryMutationResponse, String> {
    state.ensure_initialized()?;

    let backend = parse_backend_request(&state.root, &request)?;
    if let Some(repository) = state.find_existing_repository_for_backend(&backend)? {
        return Ok(RepositoryMutationResponse { repository });
    }
    let repo_id = request
        .repo_id
        .unwrap_or_else(|| slugify_repo_id(&request.name, &request.path));
    let repo_root =
        normalize_repository_root_for_backend(&state.root, &request.path, &backend, false)?;
    let seed = RepositorySeed {
        repo_id: &repo_id,
        name: &request.name,
        root_path: "",
        status: "ready",
        assets: &[],
    };
    initialize_repository_directory(&state.root, &repo_root, &seed, &backend)?;

    let registry = Connection::open(&state.registry_path).map_err(db_error)?;
    upsert_registry_entry(&registry, &repo_root, &seed, &backend)?;
    if !request.skip_initial_sync {
        sync_repository(
            state,
            SyncRequest {
                repo_id: repo_id.clone(),
            },
        )?;
    }

    let repository = state.load_repository_record(&repo_id)?.summary;
    Ok(RepositoryMutationResponse { repository })
}

pub(super) fn import_repository(
    state: &RepositoryState,
    request: RepositoryMutationRequest,
) -> Result<RepositoryMutationResponse, String> {
    state.ensure_initialized()?;

    let requested_backend = parse_backend_request(&state.root, &request)?;
    if let Some(repository) = state.find_existing_repository_for_backend(&requested_backend)? {
        return Ok(RepositoryMutationResponse { repository });
    }
    let repo_root = normalize_repository_root_for_backend(
        &state.root,
        &request.path,
        &requested_backend,
        true,
    )?;
    migrate_legacy_meta_dir_if_needed(&repo_root, &requested_backend.plugin_id)?;
    let metadata_path = repository_meta_dir(&repo_root).join(REPO_METADATA_FILE_NAME);
    let imported_metadata = if metadata_path.exists() {
        let raw = fs::read_to_string(&metadata_path).map_err(io_error)?;
        let metadata =
            serde_json::from_str::<RepositoryMetadataFileImport>(&raw).map_err(json_error)?;
        rewrite_repository_metadata_if_needed(
            &state.root,
            &metadata_path,
            &metadata,
            &repo_root,
            None,
        )?;
        Some(metadata)
    } else {
        None
    };
    let repo_id = imported_metadata
        .as_ref()
        .map(|metadata| metadata.repo_id.clone())
        .unwrap_or_else(|| slugify_repo_id(&request.name, &request.path));
    let repo_name = imported_metadata
        .as_ref()
        .and_then(|metadata| metadata.name.as_ref())
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or(request.name.as_str())
        .to_string();
    let backend = imported_metadata
        .as_ref()
        .and_then(|metadata| import_backend_record(&state.root, metadata, &repo_root))
        .unwrap_or(requested_backend);

    let seed = RepositorySeed {
        repo_id: &repo_id,
        name: &repo_name,
        root_path: "",
        status: "ready",
        assets: &[],
    };

    if !repository_meta_dir(&repo_root).exists() && !legacy_repository_meta_dir(&repo_root).exists()
    {
        initialize_repository_directory(&state.root, &repo_root, &seed, &backend)?;
    }

    let registry = Connection::open(&state.registry_path).map_err(db_error)?;
    upsert_registry_entry(&registry, &repo_root, &seed, &backend)?;
    sync_repository(
        state,
        SyncRequest {
            repo_id: repo_id.clone(),
        },
    )?;

    let repository = state.load_repository_record(&repo_id)?.summary;
    Ok(RepositoryMutationResponse { repository })
}

pub(super) fn attach_repository_folder(
    state: &RepositoryState,
    request: RepositoryFolderRequest,
) -> Result<RepositoryMutationResponse, String> {
    state.ensure_initialized()?;

    let path = request.path.trim();
    if path.is_empty() {
        return Err("repository path cannot be empty".to_string());
    }

    let backend = parse_backend_request(
        &state.root,
        &RepositoryMutationRequest {
            repo_id: None,
            name: String::new(),
            path: path.to_string(),
            backend_plugin_id: None,
            backend_config: None,
            skip_initial_sync: false,
        },
    )?;
    let repo_root = normalize_repository_root_for_backend(&state.root, path, &backend, true)?;
    ensure_backend_path_is_attachable(&state.root, &backend, &repo_root)?;
    let name = infer_repository_name(&repo_root);
    let metadata_path = if repository_meta_dir(&repo_root).exists() {
        repository_meta_dir(&repo_root).join(REPO_METADATA_FILE_NAME)
    } else {
        legacy_repository_meta_dir(&repo_root).join(REPO_METADATA_FILE_NAME)
    };
    let mutation = RepositoryMutationRequest {
        repo_id: None,
        name,
        path: path.to_string(),
        backend_plugin_id: Some(backend.plugin_id.clone()),
        backend_config: Some(backend.config.clone()),
        skip_initial_sync: false,
    };

    if metadata_path.exists() {
        import_repository(state, mutation)
    } else {
        create_repository(state, mutation)
    }
}

pub(super) fn delete_repository(state: &RepositoryState, repo_id: &str) -> Result<(), String> {
    state.ensure_initialized()?;
    let registry = Connection::open(&state.registry_path).map_err(db_error)?;
    registry
        .execute("DELETE FROM repositories WHERE repo_id = ?1", [repo_id])
        .map_err(db_error)?;
    let storage_dir = repository_state_storage_dir(&state.root, repo_id);
    if storage_dir.exists() {
        fs::remove_dir_all(storage_dir).map_err(io_error)?;
    }
    Ok(())
}

pub(super) fn relocate_repository(
    state: &RepositoryState,
    request: RepositoryRelocateRequest,
) -> Result<RepositoryMutationResponse, String> {
    state.ensure_initialized()?;

    let repo = state.load_repository_record(&request.repo_id)?;
    if !repository_supports_local_write_access(&repo) {
        return Err("only repositories with local write access can be relocated".to_string());
    }

    let next_path = request.path.trim();
    if next_path.is_empty() {
        return Err("repository path cannot be empty".to_string());
    }

    let repo_root =
        normalize_repository_root_for_backend(&state.root, next_path, &repo.backend_record, true)?;
    if !repo_root.is_dir() {
        return Err("repository path is not a directory".to_string());
    }
    ensure_backend_path_is_attachable(&state.root, &repo.backend_record, &repo_root)?;

    let metadata_path = repository_meta_dir(&repo_root).join(REPO_METADATA_FILE_NAME);
    if !metadata_path.exists() {
        return Err("repository metadata not found in selected folder".to_string());
    }
    let raw = fs::read_to_string(&metadata_path).map_err(io_error)?;
    let metadata =
        serde_json::from_str::<RepositoryMetadataFileImport>(&raw).map_err(json_error)?;
    if metadata.repo_id != request.repo_id {
        return Err("selected folder belongs to a different repository".to_string());
    }
    rewrite_repository_metadata_if_needed(
        &state.root,
        &metadata_path,
        &metadata,
        &repo_root,
        Some(&repo_root),
    )?;

    let registry = Connection::open(&state.registry_path).map_err(db_error)?;
    registry
        .execute(
            r#"
                UPDATE repositories
                SET path = ?2, status = 'ready', updated_at = ?3
                WHERE repo_id = ?1
                "#,
            params![
                request.repo_id.as_str(),
                repo_root.to_string_lossy().to_string(),
                now_rfc3339()
            ],
        )
        .map_err(db_error)?;

    sync_repository(
        state,
        SyncRequest {
            repo_id: request.repo_id.clone(),
        },
    )?;

    let repository = state.load_repository_record(&request.repo_id)?.summary;
    Ok(RepositoryMutationResponse { repository })
}

pub(super) fn update_repository_backend_config(
    state: &RepositoryState,
    request: RepositoryBackendConfigUpdateRequest,
) -> Result<RepositoryMutationResponse, String> {
    state.ensure_initialized()?;

    if !request.backend_config.is_object() {
        return Err("backend config must be a JSON object".to_string());
    }

    let repo = state.load_repository_record(&request.repo_id)?;
    let mut next_backend_config = request.backend_config.clone();
    preserve_netease_cache_config(&repo.backend_record, &mut next_backend_config);
    let repo_root = normalize_repository_root_for_backend(
        &state.root,
        &repo.summary.path,
        &repo.backend_record,
        true,
    )?;
    let metadata_path = repository_meta_dir(&repo_root).join(REPO_METADATA_FILE_NAME);
    if metadata_path.exists() {
        let raw = fs::read_to_string(&metadata_path).map_err(io_error)?;
        let metadata =
            serde_json::from_str::<RepositoryMetadataFileImport>(&raw).map_err(json_error)?;
        let rewritten = RepositoryMetadataFile {
            repo_id: metadata.repo_id,
            name: metadata
                .name
                .clone()
                .unwrap_or_else(|| infer_repository_name(&repo_root)),
            root_path: metadata
                .root_path
                .clone()
                .unwrap_or_else(|| repo_root.to_string_lossy().to_string()),
            backend_plugin_id: metadata
                .backend_plugin_id
                .clone()
                .unwrap_or_else(|| repo.backend_record.plugin_id.clone()),
            backend_config: next_backend_config.clone(),
            created_at: metadata.created_at.clone().unwrap_or_else(now_rfc3339),
            schema_version: metadata.schema_version.unwrap_or(REPO_SCHEMA_VERSION),
        };
        let metadata_json = serde_json::to_string_pretty(&rewritten).map_err(json_error)?;
        fs::write(&metadata_path, metadata_json).map_err(io_error)?;
    }

    let registry = Connection::open(&state.registry_path).map_err(db_error)?;
    registry
        .execute(
            r#"
                UPDATE repositories
                SET backend_config_json = ?2, updated_at = ?3
                WHERE repo_id = ?1
                "#,
            params![
                request.repo_id.as_str(),
                next_backend_config.to_string(),
                now_rfc3339()
            ],
        )
        .map_err(db_error)?;

    let repository = state.load_repository_record(&request.repo_id)?.summary;
    Ok(RepositoryMutationResponse { repository })
}

pub(super) fn configure_netease_repository_cache(
    state: &RepositoryState,
    request: NeteaseRepositoryCacheConfigureRequest,
) -> Result<NeteaseRepositoryCacheConfigureResponse, String> {
    state.ensure_initialized()?;

    let repo = state.load_repository_record(&request.repo_id)?;
    if repo.backend_record.plugin_id != NETEASE_CLOUD_MUSIC_PLUGIN_ID {
        return Err(
            "only netease cloud music repositories support cache configuration".to_string(),
        );
    }

    let selected = request.path.trim();
    if selected.is_empty() {
        return Err("cache directory cannot be empty".to_string());
    }
    let cache_root = normalize_external_cache_root(selected)?;
    fs::create_dir_all(&cache_root).map_err(io_error)?;
    let storage_paths = ensure_repository_storage_paths(
        &state.root,
        &repo.summary.repo_id,
        &cache_root,
        &repo.backend_record.plugin_id,
    )?;
    hide_repository_meta_dir(&storage_paths.metadata_dir);

    let old_state_dir = repository_state_storage_dir(&state.root, &repo.summary.repo_id);
    let old_metadata_dir = old_state_dir.join(REPO_META_DIR);
    let mut migration = NeteaseRepositoryCacheMigrationSummary::empty();
    if old_metadata_dir.exists() && old_metadata_dir != storage_paths.metadata_dir {
        migration.moved_state_files +=
            merge_netease_cache_state_contents(&old_metadata_dir, &storage_paths.metadata_dir)?;
    }

    let mut backend_config = repo.backend_record.config.clone();
    if !backend_config.is_object() {
        backend_config = serde_json::json!({});
    }
    if let Some(object) = backend_config.as_object_mut() {
        object
            .entry("sourceUri".to_string())
            .or_insert_with(|| serde_json::json!(netease_source_uri_for_repo(&repo)));
        object.insert(
            "localCachePath".to_string(),
            serde_json::json!(cache_root.to_string_lossy().to_string()),
        );
    }

    write_repository_metadata(
        &storage_paths.metadata_dir,
        &repo.summary.repo_id,
        &repo.summary.name,
        &cache_root,
        &repo.backend_record.plugin_id,
        &backend_config,
        None,
    )?;

    let registry = Connection::open(&state.registry_path).map_err(db_error)?;
    registry
        .execute(
            r#"
                UPDATE repositories
                SET path = ?2, backend_config_json = ?3, status = 'ready', updated_at = ?4
                WHERE repo_id = ?1
                "#,
            params![
                repo.summary.repo_id.as_str(),
                cache_root.to_string_lossy().to_string(),
                backend_config.to_string(),
                now_rfc3339(),
            ],
        )
        .map_err(db_error)?;

    if request.migrate_legacy_cache {
        let updated_repo = state.load_repository_record(&request.repo_id)?;
        migrate_netease_playback_cache(&state.root, &updated_repo, &cache_root, &mut migration)?;
    }

    let repository = state.load_repository_record(&request.repo_id)?.summary;
    Ok(NeteaseRepositoryCacheConfigureResponse {
        repository,
        migration,
    })
}

pub(super) fn export_repository(
    state: &RepositoryState,
    request: RepositoryExportRequest,
) -> Result<RepositoryExportResponse, String> {
    state.ensure_initialized()?;
    let repository = state.load_repository_record(&request.repo_id)?.summary;
    let repo_root = PathBuf::from(&repository.path);

    match request.target.as_str() {
        "archive" => {
            let archive = request
                .archive
                .ok_or_else(|| "archive export options are required".to_string())?;
            export_repository_archive(&repo_root, &archive)?;
            Ok(RepositoryExportResponse {
                repository,
                target: "archive".to_string(),
                output_path: Some(archive.output_path),
                format: Some(archive.format),
                encrypted: Some(archive.encrypt),
                remote: None,
                branch: None,
                message: "资源库压缩包已导出".to_string(),
            })
        }
        "git" => {
            let git = request.git.unwrap_or(RepositoryGitExportOptions {
                remote: None,
                branch: None,
                message: None,
            });
            let result = export_repository_to_git(&repo_root, &git)?;
            Ok(RepositoryExportResponse {
                repository,
                target: "git".to_string(),
                output_path: None,
                format: None,
                encrypted: None,
                remote: Some(result.remote),
                branch: Some(result.branch),
                message: result.message,
            })
        }
        value => Err(format!("unsupported repository export target: {value}")),
    }
}

pub(super) fn sync_repository(
    state: &RepositoryState,
    request: SyncRequest,
) -> Result<SyncResult, String> {
    sync_repository_with_candidate_skips_and_hint_paths(
        state,
        &request.repo_id,
        &HashSet::new(),
        &std::collections::BTreeSet::new(),
    )
}

pub(super) fn sync_repository_with_hint_paths(
    state: &RepositoryState,
    repo_id: &str,
    hint_paths: &std::collections::BTreeSet<String>,
) -> Result<SyncResult, String> {
    sync_repository_with_candidate_skips_and_hint_paths(state, repo_id, &HashSet::new(), hint_paths)
}

pub(super) fn sync_repository_with_candidate_skips(
    state: &RepositoryState,
    repo_id: &str,
    skip_hardlink_candidate_paths: &HashSet<String>,
) -> Result<SyncResult, String> {
    sync_repository_with_candidate_skips_and_hint_paths(
        state,
        repo_id,
        skip_hardlink_candidate_paths,
        &std::collections::BTreeSet::new(),
    )
}

fn sync_repository_with_candidate_skips_and_hint_paths(
    state: &RepositoryState,
    repo_id: &str,
    skip_hardlink_candidate_paths: &HashSet<String>,
    hint_paths: &std::collections::BTreeSet<String>,
) -> Result<SyncResult, String> {
    state.ensure_initialized()?;
    let repo = state.load_repository_record(repo_id)?;
    let mut connection = state.open_repository_connection(
        &repo.summary.repo_id,
        &repo.summary.path,
        &repo.backend_record,
    )?;
    let tx = connection.transaction().map_err(db_error)?;

    let scan = sync_repository_files(
        &state.root,
        &tx,
        &repo,
        skip_hardlink_candidate_paths,
        hint_paths,
    )
    .map_err(db_error)?;
    tx.commit().map_err(db_error)?;
    Ok(scan)
}
