//! 新建、导入、复制、移动、重命名、删除条目与回收站操作。

use super::*;

pub(in super::super) fn create_directory(
    state: &RepositoryState,
    request: FileCreateRequest,
) -> Result<FileBrowserSnapshot, String> {
    state.ensure_initialized()?;
    let repo = state.load_repository_record(&request.repo_id)?;
    let repo_root = PathBuf::from(&repo.summary.path);
    let parent_path = normalize_directory_path(request.parent_path.as_deref().unwrap_or_default())?;
    let name = validate_new_entry_name(&request.name)?;
    create_backend_directory(&state.root, &repo, &repo_root, &parent_path, &name)?;
    if repository_supports_local_root_access(&repo) {
        let connection = state.open_repository_connection(
            &repo.summary.repo_id,
            &repo.summary.path,
            &repo.backend_record,
        )?;
        let path = join_relative_path(&parent_path, &name);
        upsert_directory_record(&connection, &request.repo_id, &path, &parent_path, &name)
            .map_err(db_error)?;
    } else {
        let _ = state.sync_repository(SyncRequest {
            repo_id: request.repo_id.clone(),
        })?;
    }
    load_file_browser(
        state,
        FileBrowserRequest {
            repo_id: request.repo_id,
            directory_path: Some(parent_path),
            include_tree: Some(true),
            special_location: None,
            offset: None,
            limit: None,
        },
    )
}

pub(in super::super) fn create_file(
    state: &RepositoryState,
    request: FileCreateRequest,
) -> Result<FileBrowserSnapshot, String> {
    state.ensure_initialized()?;
    let repo = state.load_repository_record(&request.repo_id)?;
    let repo_root = PathBuf::from(&repo.summary.path);
    let parent_path = normalize_directory_path(request.parent_path.as_deref().unwrap_or_default())?;
    let name = validate_new_entry_name(&request.name)?;
    create_backend_file(&state.root, &repo, &repo_root, &parent_path, &name)?;
    let _ = state.sync_repository(SyncRequest {
        repo_id: request.repo_id.clone(),
    })?;

    load_file_browser(
        state,
        FileBrowserRequest {
            repo_id: request.repo_id,
            directory_path: Some(parent_path),
            include_tree: Some(false),
            special_location: None,
            offset: None,
            limit: None,
        },
    )
}

/// 把外部文件或目录导入资源库的一个文件夹。目标里已有同名条目时按 `skip_existing` 整批拒绝或跳过它，
/// 跳过的写进结果的 `skipped_entries`；已有的文件都不覆盖。
pub(in super::super) fn import_entries_cancellable(
    state: &RepositoryState,
    request: FileImportRequest,
    cancellation: &dyn CancellationCheck,
) -> Result<FileImportResponse, String> {
    cancellation.checkpoint()?;
    state.ensure_initialized()?;
    let repo = state.load_repository_record(&request.repo_id)?;
    ensure_repository_supports_local_write_access(&repo, "importing files")?;

    let repo_root = PathBuf::from(&repo.summary.path);
    let (parent_path, target_dir) = resolve_file_copy_target(
        &repo_root,
        request.parent_path.as_deref(),
        &request.source_paths,
    )?;

    let ExternalImportPlan {
        entries: import_plan,
        skipped,
    } = validate_external_import_entries(
        &request.source_paths,
        &repo_root,
        &target_dir,
        request.skip_existing,
    )?;
    let include_tree = import_plan.iter().any(|entry| entry.is_directory);
    let outcomes = match copy_external_entries_parallel(import_plan, true, cancellation) {
        Ok(outcomes) => outcomes,
        Err(error) => {
            let _ = state.sync_repository(SyncRequest {
                repo_id: request.repo_id.clone(),
            })?;
            return Err(error);
        }
    };
    let snapshot =
        state.finish_file_copy_operation(&request.repo_id, parent_path, include_tree, outcomes)?;
    cancellation.checkpoint()?;
    Ok(FileImportResponse {
        snapshot,
        skipped_entries: skipped,
    })
}

pub(in super::super) fn copy_entries_cancellable(
    state: &RepositoryState,
    request: FileCopyRequest,
    cancellation: &dyn CancellationCheck,
) -> Result<FileBrowserSnapshot, String> {
    cancellation.checkpoint()?;
    state.ensure_initialized()?;
    let repo = state.load_repository_record(&request.repo_id)?;
    ensure_repository_supports_local_write_access(&repo, "copying files")?;

    let repo_root = PathBuf::from(&repo.summary.path);
    let (parent_path, target_dir) = resolve_file_copy_target(
        &repo_root,
        request.parent_path.as_deref(),
        &request.source_paths,
    )?;

    let copy_plan =
        validate_repository_copy_entries(&request.source_paths, &repo_root, &target_dir)?;
    let include_tree = copy_plan.iter().any(|entry| entry.is_directory);
    let hardlink_preferred =
        request.mode.as_deref().unwrap_or("hardlinkPreferred") == "hardlinkPreferred";
    let outcomes = match copy_external_entries_parallel(copy_plan, hardlink_preferred, cancellation)
    {
        Ok(outcomes) => outcomes,
        Err(error) => {
            let _ = state.sync_repository(SyncRequest {
                repo_id: request.repo_id.clone(),
            })?;
            return Err(error);
        }
    };
    let snapshot =
        state.finish_file_copy_operation(&request.repo_id, parent_path, include_tree, outcomes)?;
    cancellation.checkpoint()?;
    Ok(snapshot)
}

pub(in super::super) fn move_entries_cancellable(
    state: &RepositoryState,
    request: FileMoveRequest,
    cancellation: &dyn CancellationCheck,
) -> Result<FileBrowserSnapshot, String> {
    cancellation.checkpoint()?;
    state.ensure_initialized()?;
    let repo = state.load_repository_record(&request.repo_id)?;
    if !backend_has_capability(&repo.summary.backend, "write") {
        return Err("moving files is not available for this repository backend".to_string());
    }

    let repo_root = PathBuf::from(&repo.summary.path);
    let parent_path = normalize_directory_path(&request.parent_path)?;
    if request.source_paths.is_empty() {
        return Err("no source files were provided".to_string());
    }
    if !repository_supports_local_root_access(&repo) {
        let mut include_tree = false;
        for source_path in &request.source_paths {
            cancellation.checkpoint()?;
            let source_path = normalize_entry_path(source_path)?;
            let entry = stat_backend_entry(&state.root, &repo, &repo_root, &source_path)?;
            include_tree |= matches!(entry.kind, FileSystemEntryKind::Directory);
            move_backend_entry(&state.root, &repo, &repo_root, &source_path, &parent_path)?;
            let _ = state.sync_repository(SyncRequest {
                repo_id: request.repo_id.clone(),
            })?;
            cancellation.checkpoint()?;
        }
        return load_file_browser(
            state,
            FileBrowserRequest {
                repo_id: request.repo_id,
                directory_path: Some(parent_path),
                include_tree: Some(include_tree),
                special_location: None,
                offset: None,
                limit: None,
            },
        );
    }

    let target_dir = resolve_repository_relative_path(&repo_root, &parent_path)?;
    if !target_dir.exists() || !target_dir.is_dir() {
        return Err(format!("directory not found: {parent_path}"));
    }

    let move_plan =
        validate_repository_move_entries(&request.source_paths, &repo_root, &target_dir)?;
    let include_tree = move_plan.iter().any(|entry| entry.is_directory);
    let mut connection = state.open_repository_connection(
        &repo.summary.repo_id,
        &repo.summary.path,
        &repo.backend_record,
    )?;
    for entry in &move_plan {
        cancellation.checkpoint()?;
        let tx = connection.transaction().map_err(db_error)?;
        let moved = move_backend_entry(
            &state.root,
            &repo,
            &repo_root,
            &entry.source_relative_path,
            &parent_path,
        )?;
        if entry.is_directory {
            rename_directory_asset_records(
                &tx,
                &request.repo_id,
                &entry.source_relative_path,
                &entry.target_relative_path,
            )
            .map_err(db_error)?;
        } else {
            let extension = moved.extension.unwrap_or_default();
            let modified_at = moved.modified_at.unwrap_or_else(now_rfc3339);
            rename_file_asset_record(
                &tx,
                &request.repo_id,
                &entry.source_relative_path,
                &entry.target_relative_path,
                &entry.target_name,
                &extension,
                &modified_at,
            )
            .map_err(db_error)?;
        }
        tx.commit().map_err(db_error)?;
        cancellation.checkpoint()?;
    }
    if include_tree {
        let _ = state.sync_repository(SyncRequest {
            repo_id: request.repo_id.clone(),
        })?;
    }
    cancellation.checkpoint()?;

    load_file_browser(
        state,
        FileBrowserRequest {
            repo_id: request.repo_id,
            directory_path: Some(parent_path),
            include_tree: Some(include_tree),
            special_location: None,
            offset: None,
            limit: None,
        },
    )
}

pub(in super::super) fn rename_entry(
    state: &RepositoryState,
    request: FileRenameRequest,
) -> Result<FileBrowserSnapshot, String> {
    state.ensure_initialized()?;
    let repo = state.load_repository_record(&request.repo_id)?;
    let repo_root = PathBuf::from(&repo.summary.path);
    let source_path = normalize_entry_path(&request.path)?;
    let new_name = validate_new_entry_name(&request.new_name)?;
    let parent_path = parent_relative_path(&source_path);
    let target_path = join_relative_path(&parent_path, &new_name);
    let renamed = rename_backend_entry(&state.root, &repo, &repo_root, &source_path, &new_name)?;

    let is_directory = matches!(renamed.kind, FileSystemEntryKind::Directory);
    if !repository_supports_local_root_access(&repo) {
        let _ = state.sync_repository(SyncRequest {
            repo_id: request.repo_id.clone(),
        })?;
        return load_file_browser(
            state,
            FileBrowserRequest {
                repo_id: request.repo_id,
                directory_path: Some(parent_path),
                include_tree: Some(is_directory),
                special_location: None,
                offset: None,
                limit: None,
            },
        );
    }
    if !is_directory {
        let extension = renamed.extension.unwrap_or_default();
        let modified_at = renamed.modified_at.unwrap_or_else(now_rfc3339);
        let mut connection = state.open_repository_connection(
            &repo.summary.repo_id,
            &repo.summary.path,
            &repo.backend_record,
        )?;
        let tx = connection.transaction().map_err(db_error)?;
        rename_file_asset_record(
            &tx,
            &request.repo_id,
            &source_path,
            &target_path,
            &new_name,
            &extension,
            &modified_at,
        )
        .map_err(db_error)?;
        tx.commit().map_err(db_error)?;
    } else {
        let mut connection = state.open_repository_connection(
            &repo.summary.repo_id,
            &repo.summary.path,
            &repo.backend_record,
        )?;
        let tx = connection.transaction().map_err(db_error)?;
        rename_directory_asset_records(&tx, &request.repo_id, &source_path, &target_path)
            .map_err(db_error)?;
        tx.commit().map_err(db_error)?;
        let _ = state.sync_repository(SyncRequest {
            repo_id: request.repo_id.clone(),
        })?;
    }

    load_file_browser(
        state,
        FileBrowserRequest {
            repo_id: request.repo_id,
            directory_path: Some(parent_path),
            include_tree: Some(is_directory),
            special_location: None,
            offset: None,
            limit: None,
        },
    )
}

pub(in super::super) fn delete_entry(
    state: &RepositoryState,
    request: FileDeleteRequest,
) -> Result<FileBrowserSnapshot, String> {
    state.ensure_initialized()?;
    let repo = state.load_repository_record(&request.repo_id)?;
    let repo_root = PathBuf::from(&repo.summary.path);
    let delete_mode = request.mode.as_deref().unwrap_or("delete");
    let mut connection = state.open_repository_connection(
        &repo.summary.repo_id,
        &repo.summary.path,
        &repo.backend_record,
    )?;
    let source_trash_supported = if repository_supports_local_root_access(&repo) {
        false
    } else {
        repository_has_source_trash_entries(&connection, &request.repo_id).map_err(db_error)?
    };

    if delete_mode == "permanentDelete" {
        if repository_supports_local_root_access(&repo) {
            let trash_path = normalize_trash_relative_path(&request.path, false)?;
            let parent_path = parent_relative_path(&trash_path);
            delete_trash_entry(&repo_root, &trash_path)?;
            return load_file_browser(
                state,
                FileBrowserRequest {
                    repo_id: request.repo_id,
                    directory_path: Some(parent_path),
                    include_tree: Some(false),
                    special_location: Some("trash".to_string()),
                    offset: None,
                    limit: None,
                },
            );
        }
        if !source_trash_supported {
            return Err(format!(
                "permanent trash delete is only supported for repositories with local write access, got: {}",
                repo.summary.backend.plugin_id
            ));
        }
        let trash_path = normalize_trash_relative_path(&request.path, false)?;
        let trash_record = load_source_trash_entry(&connection, &request.repo_id, &trash_path)
            .map_err(db_error)?
            .ok_or_else(|| format!("trash entry not found: {trash_path}"))?;
        let parent_path = parent_relative_path(&trash_path);
        delete_backend_entry(
            &state.root,
            &repo,
            &repo_root,
            &trash_path,
            trash_record.kind == "directory",
        )?;
        let _ = state.sync_repository(SyncRequest {
            repo_id: request.repo_id.clone(),
        })?;
        return load_file_browser(
            state,
            FileBrowserRequest {
                repo_id: request.repo_id,
                directory_path: Some(parent_path),
                include_tree: Some(false),
                special_location: Some("trash".to_string()),
                offset: None,
                limit: None,
            },
        );
    }

    let entry_path = normalize_entry_path(&request.path)?;
    let parent_path = parent_relative_path(&entry_path);
    let entry = stat_backend_entry(&state.root, &repo, &repo_root, &entry_path)?;

    let is_directory = matches!(entry.kind, FileSystemEntryKind::Directory);
    if !repository_supports_local_root_access(&repo) {
        if delete_mode == "moveToParent" {
            return Err("moveToParent 仅支持本地目录资源库".to_string());
        }
        delete_backend_entry(&state.root, &repo, &repo_root, &entry_path, is_directory)?;
        let _ = state.sync_repository(SyncRequest {
            repo_id: request.repo_id.clone(),
        })?;
        return load_file_browser(
            state,
            FileBrowserRequest {
                repo_id: request.repo_id,
                directory_path: Some(parent_path),
                include_tree: Some(is_directory),
                special_location: None,
                offset: None,
                limit: None,
            },
        );
    }
    if is_directory {
        if delete_mode == "moveToParent" {
            move_directory_contents_to_parent(
                &state.root,
                &repo,
                &repo_root,
                &request.repo_id,
                &entry_path,
            )?;
        } else {
            move_entry_to_trash(&repo_root, &entry_path, is_directory)?;
            let tx = connection.transaction().map_err(db_error)?;
            mark_directory_assets_deleted(&tx, &request.repo_id, &entry_path).map_err(db_error)?;
            tx.commit().map_err(db_error)?;
            let _ = state.sync_repository(SyncRequest {
                repo_id: request.repo_id.clone(),
            })?;
        }
    } else {
        move_entry_to_trash(&repo_root, &entry_path, is_directory)?;
        let tx = connection.transaction().map_err(db_error)?;
        mark_file_asset_deleted(&tx, &request.repo_id, &entry_path).map_err(db_error)?;
        tx.commit().map_err(db_error)?;
    }

    load_file_browser(
        state,
        FileBrowserRequest {
            repo_id: request.repo_id,
            directory_path: Some(parent_path),
            include_tree: Some(is_directory),
            special_location: None,
            offset: None,
            limit: None,
        },
    )
}

pub(in super::super) fn mutate_trash(
    state: &RepositoryState,
    request: TrashMutationRequest,
) -> Result<FileBrowserSnapshot, String> {
    state.ensure_initialized()?;
    let repo = state.load_repository_record(&request.repo_id)?;
    let repo_root = PathBuf::from(&repo.summary.path);
    let connection = state.open_repository_connection(
        &repo.summary.repo_id,
        &repo.summary.path,
        &repo.backend_record,
    )?;
    let source_trash_supported = if repository_supports_local_root_access(&repo) {
        false
    } else {
        repository_has_source_trash_entries(&connection, &request.repo_id).map_err(db_error)?
    };

    if repository_supports_local_root_access(&repo) {
        match request.action.as_str() {
            "restore" => {
                let trash_path = request
                    .path
                    .as_deref()
                    .ok_or_else(|| "trash restore requires a path".to_string())
                    .and_then(|path| normalize_trash_relative_path(path, false))?;
                restore_trash_entry(&repo_root, &trash_path)?;
            }
            "restoreAll" => {
                restore_all_trash_entries(&repo_root)?;
            }
            "empty" => {
                empty_trash(&repo_root)?;
            }
            value => return Err(format!("unsupported trash action: {value}")),
        }
    } else {
        if !source_trash_supported {
            return Err(format!(
                "trash operations are not available for backend: {}",
                repo.summary.backend.plugin_id
            ));
        }
        let source_entries =
            load_source_trash_entries(&connection, &request.repo_id).map_err(db_error)?;
        match request.action.as_str() {
            "restore" => {
                let trash_path = request
                    .path
                    .as_deref()
                    .ok_or_else(|| "trash restore requires a path".to_string())
                    .and_then(|path| normalize_trash_relative_path(path, false))?;
                let record = source_entries
                    .iter()
                    .find(|entry| entry.trash_path == trash_path)
                    .ok_or_else(|| format!("trash entry not found: {trash_path}"))?;
                move_backend_entry(
                    &state.root,
                    &repo,
                    &repo_root,
                    &trash_path,
                    &parent_relative_path(&record.original_path),
                )?;
            }
            "restoreAll" => {
                let mut restore_entries = source_entries.clone();
                restore_entries.sort_by(|left, right| left.original_path.cmp(&right.original_path));
                for entry in restore_entries {
                    move_backend_entry(
                        &state.root,
                        &repo,
                        &repo_root,
                        &entry.trash_path,
                        &parent_relative_path(&entry.original_path),
                    )?;
                }
            }
            "empty" => {
                let mut delete_entries = source_entries.clone();
                delete_entries.sort_by(|left, right| right.trash_path.cmp(&left.trash_path));
                for entry in delete_entries {
                    delete_backend_entry(
                        &state.root,
                        &repo,
                        &repo_root,
                        &entry.trash_path,
                        entry.kind == "directory",
                    )?;
                }
            }
            value => return Err(format!("unsupported trash action: {value}")),
        }
    }

    let _ = state.sync_repository(SyncRequest {
        repo_id: request.repo_id.clone(),
    })?;

    load_file_browser(
        state,
        FileBrowserRequest {
            repo_id: request.repo_id,
            directory_path: Some(String::new()),
            include_tree: Some(false),
            special_location: Some("trash".to_string()),
            offset: None,
            limit: None,
        },
    )
}
