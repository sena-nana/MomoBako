//! File browser reads and mutations for repository entries.

use super::*;

fn local_repository_cache_snapshot(
    connection: &Connection,
    repo_id: &str,
    refreshing: bool,
) -> Result<RepositoryStructureCacheSnapshot, String> {
    let has_cache = has_directory_cache(connection, repo_id).map_err(db_error)?;
    let indexed_at = latest_directory_indexed_at(connection, repo_id).map_err(db_error)?;
    Ok(RepositoryStructureCacheSnapshot {
        cache_state: if !has_cache {
            RepositoryStructureCacheState::Warming
        } else if refreshing {
            RepositoryStructureCacheState::Refreshing
        } else {
            RepositoryStructureCacheState::Ready
        },
        indexed_at,
    })
}

fn build_tree_from_directory_records(records: Vec<DirectoryRecord>) -> Vec<FileTreeNode> {
    let mut grouped = BTreeMap::<String, Vec<DirectoryRecord>>::new();
    for record in records.into_iter().filter(|record| !record.path.is_empty()) {
        grouped
            .entry(record.parent_path.clone())
            .or_default()
            .push(record);
    }

    fn build_nodes(
        parent_path: &str,
        grouped: &mut BTreeMap<String, Vec<DirectoryRecord>>,
    ) -> Vec<FileTreeNode> {
        let mut children = grouped.remove(parent_path).unwrap_or_default();
        children.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
        children
            .into_iter()
            .map(|record| FileTreeNode {
                path: record.path.clone(),
                label: record.name.clone(),
                children: build_nodes(&record.path, grouped),
            })
            .collect()
    }

    build_nodes("", &mut grouped)
}

fn load_cached_directory_entries(
    connection: &Connection,
    repo_id: &str,
    current_path: &str,
) -> Result<Vec<FileSystemEntry>, String> {
    let directory_entries = load_directory_records_for_parent(connection, repo_id, current_path)
        .map_err(db_error)?
        .into_iter()
        .map(|record| FileSystemEntry {
            path: record.path,
            name: record.name,
            kind: FileSystemEntryKind::Directory,
            extension: None,
            size_bytes: None,
            modified_at: Some(record.updated_at),
            is_virtual: false,
            provider_id: None,
            provider_item_id: None,
            source_payload: None,
            local_absolute_path: None,
        })
        .collect::<Vec<_>>();

    let mut stmt = connection.prepare(
        r#"
        SELECT
          path,
          filename,
          extension,
          size_bytes,
          modified_at,
          is_virtual,
          provider_id,
          provider_item_id,
          source_payload_json,
          local_absolute_path
        FROM assets
        WHERE repo_id = ?1
          AND status != 'deleted'
          AND (
            (?2 = '' AND INSTR(path, '/') = 0)
            OR (?2 != '' AND path LIKE (?2 || '/%') AND SUBSTR(path, LENGTH(?2) + 2) NOT LIKE '%/%')
          )
        ORDER BY filename COLLATE NOCASE
        "#,
    ).map_err(db_error)?;
    let file_entries = stmt
        .query_map(params![repo_id, current_path], |row| {
            Ok(FileSystemEntry {
                path: row.get(0)?,
                name: row.get(1)?,
                kind: FileSystemEntryKind::File,
                extension: row.get(2)?,
                size_bytes: row.get(3)?,
                modified_at: row.get(4)?,
                is_virtual: row.get::<_, i64>(5)? != 0,
                provider_id: row.get(6)?,
                provider_item_id: row.get(7)?,
                source_payload: parse_json_column_nullable(row.get::<_, Option<String>>(8)?)?,
                local_absolute_path: row.get(9)?,
            })
        })
        .map_err(db_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_error)?;

    let mut entries = directory_entries;
    entries.extend(file_entries);
    Ok(entries)
}

pub(super) fn load_file_browser(
    state: &RepositoryState,
    request: FileBrowserRequest,
) -> Result<FileBrowserSnapshot, String> {
    state.ensure_initialized()?;

    let repo = state.load_repository_record(&request.repo_id)?;
    let repo_root = PathBuf::from(&repo.summary.path);
    let connection = state.open_repository_connection(
        &repo.summary.repo_id,
        &repo.summary.path,
        &repo.backend_record,
    )?;
    let thumbnail_root = state.repository_thumbnail_root(&repo)?;
    let special_location = normalize_special_location(request.special_location.as_deref())?;
    if special_location.is_some() && repo.backend_record.plugin_id != LOCAL_FILESYSTEM_PLUGIN_ID {
        return Err(format!(
            "trash browser is only supported for local filesystem repositories, got: {}",
            repo.backend_record.plugin_id
        ));
    }
    let current_path =
        normalize_directory_path(request.directory_path.as_deref().unwrap_or_default())?;
    let cache_snapshot = if repo.backend_record.plugin_id == LOCAL_FILESYSTEM_PLUGIN_ID {
        let snapshot = local_repository_cache_snapshot(
            &connection,
            &request.repo_id,
            state.repository_structure_refresh_in_progress(&request.repo_id),
        )?;
        if matches!(snapshot.cache_state, RepositoryStructureCacheState::Warming) {
            state.queue_repository_structure_refresh(request.repo_id.clone(), "cache-miss");
        }
        snapshot
    } else {
        RepositoryStructureCacheSnapshot {
            cache_state: RepositoryStructureCacheState::Ready,
            indexed_at: None,
        }
    };
    let tree = if special_location.is_some() {
        None
    } else if request.include_tree.unwrap_or(true) {
        Some(if repo.backend_record.plugin_id == LOCAL_FILESYSTEM_PLUGIN_ID {
            build_tree_from_directory_records(
                load_directory_records(&connection, &request.repo_id).map_err(db_error)?,
            )
        } else {
            list_backend_tree(&state.root, &repo, &repo_root)?
        })
    } else {
        None
    };
    let offset = request.offset.unwrap_or(0);
    let limit = request.limit.filter(|value| *value > 0);
    let raw_entries = if special_location.as_deref() == Some("trash") {
        let asset_map = normalize_asset_thumbnail_map(
            &connection,
            &repo,
            &thumbnail_root,
            load_asset_path_map(&connection, &request.repo_id).map_err(db_error)?,
        )?;
        let thumbnail_map = normalize_entry_thumbnail_map(
            &connection,
            &repo,
            &thumbnail_root,
            load_entry_thumbnail_map(&connection, &request.repo_id).map_err(db_error)?,
        )?;
        list_trash_directory_entries(&repo_root, &current_path, &asset_map, &thumbnail_map)?
    } else {
        let listed_entries = if repo.backend_record.plugin_id == LOCAL_FILESYSTEM_PLUGIN_ID {
            load_cached_directory_entries(&connection, &request.repo_id, &current_path)?
        } else {
            backend_adapter(&state.root, &repo).list_directory_entries(
                &repo_root,
                &current_path,
                &repo.backend_record.config,
            )?
        };
        let entry_paths = listed_entries
            .iter()
            .map(|entry| entry.path.clone())
            .collect::<Vec<_>>();
        let entry_thumbnail_keys = listed_entries
            .iter()
            .map(|entry| {
                let kind = match entry.kind {
                    FileSystemEntryKind::Directory => "directory".to_string(),
                    FileSystemEntryKind::File => "file".to_string(),
                };
                (entry.path.clone(), kind)
            })
            .collect::<Vec<_>>();
        let directory_paths = listed_entries
            .iter()
            .filter(|entry| matches!(entry.kind, FileSystemEntryKind::Directory))
            .map(|entry| entry.path.clone())
            .collect::<Vec<_>>();
        let asset_map = normalize_asset_thumbnail_map(
            &connection,
            &repo,
            &thumbnail_root,
            load_asset_path_map_for_paths(&connection, &request.repo_id, &entry_paths)
                .map_err(db_error)?,
        )?;
        let thumbnail_map = normalize_entry_thumbnail_map(
            &connection,
            &repo,
            &thumbnail_root,
            load_entry_thumbnail_map_for_paths(&connection, &request.repo_id, &entry_thumbnail_keys)
                .map_err(db_error)?,
        )?;
        let folder_metadata = load_folder_metadata_map_for_paths(
            &connection,
            &request.repo_id,
            &directory_paths,
        )
        .map_err(db_error)?;
        map_file_browser_entries(
            listed_entries,
            &asset_map,
            &thumbnail_map,
            &folder_metadata,
        )
    };
    let (entries, total_entries, loaded_count, next_offset, has_more) =
        paginate_file_browser_entries(raw_entries, offset, limit);
    let entries =
        attach_browser_entry_metadata(&connection, &request.repo_id, entries).map_err(db_error)?;

    Ok(FileBrowserSnapshot {
        repo_id: request.repo_id,
        root_path: repo.summary.path,
        backend_plugin_id: repo.backend_record.plugin_id.clone(),
        backend_kind: repo.summary.backend.kind,
        cache_state: cache_snapshot.cache_state,
        indexed_at: cache_snapshot.indexed_at,
        current_path,
        total_entries,
        loaded_count,
        next_offset,
        has_more,
        special_location,
        tree,
        entries,
    })
}

pub(super) fn load_repository_tree(
    state: &RepositoryState,
    repo_id: &str,
) -> Result<RepositoryTreeSnapshot, String> {
    state.ensure_initialized()?;

    let repo = state.load_repository_record(repo_id)?;
    let repo_root = PathBuf::from(&repo.summary.path);
    let connection = state.open_repository_connection(
        &repo.summary.repo_id,
        &repo.summary.path,
        &repo.backend_record,
    )?;
    let cache_snapshot = if repo.backend_record.plugin_id == LOCAL_FILESYSTEM_PLUGIN_ID {
        let snapshot = local_repository_cache_snapshot(
            &connection,
            repo_id,
            state.repository_structure_refresh_in_progress(repo_id),
        )?;
        if matches!(snapshot.cache_state, RepositoryStructureCacheState::Warming) {
            state.queue_repository_structure_refresh(repo_id.to_string(), "cache-miss");
        }
        snapshot
    } else {
        RepositoryStructureCacheSnapshot {
            cache_state: RepositoryStructureCacheState::Ready,
            indexed_at: None,
        }
    };
    let tree = if repo.backend_record.plugin_id == LOCAL_FILESYSTEM_PLUGIN_ID {
        build_tree_from_directory_records(load_directory_records(&connection, repo_id).map_err(db_error)?)
    } else {
        list_backend_tree(&state.root, &repo, &repo_root)?
    };

    Ok(RepositoryTreeSnapshot {
        repo_id: repo.summary.repo_id.clone(),
        root_path: repo.summary.path.clone(),
        backend_plugin_id: repo.backend_record.plugin_id.clone(),
        backend_kind: repo.summary.backend.kind.clone(),
        cache_state: cache_snapshot.cache_state,
        indexed_at: cache_snapshot.indexed_at,
        tree,
    })
}

pub(super) fn create_directory(
    state: &RepositoryState,
    request: FileCreateRequest,
) -> Result<FileBrowserSnapshot, String> {
    state.ensure_initialized()?;
    let repo = state.load_repository_record(&request.repo_id)?;
    let repo_root = PathBuf::from(&repo.summary.path);
    let parent_path = normalize_directory_path(request.parent_path.as_deref().unwrap_or_default())?;
    let name = validate_new_entry_name(&request.name)?;
    create_backend_directory(&state.root, &repo, &repo_root, &parent_path, &name)?;
    if repo.backend_record.plugin_id == LOCAL_FILESYSTEM_PLUGIN_ID {
        let connection = state.open_repository_connection(
            &repo.summary.repo_id,
            &repo.summary.path,
            &repo.backend_record,
        )?;
        let path = join_relative_path(&parent_path, &name);
        upsert_directory_record(&connection, &request.repo_id, &path, &parent_path, &name)
            .map_err(db_error)?;
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

pub(super) fn create_file(
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

pub(super) fn import_entries(
    state: &RepositoryState,
    request: FileImportRequest,
) -> Result<FileBrowserSnapshot, String> {
    state.ensure_initialized()?;
    let repo = state.load_repository_record(&request.repo_id)?;
    ensure_local_filesystem_repository(&repo, "importing files")?;

    let repo_root = PathBuf::from(&repo.summary.path);
    let (parent_path, target_dir) = resolve_file_copy_target(
        &repo_root,
        request.parent_path.as_deref(),
        &request.source_paths,
    )?;

    let import_plan =
        validate_external_import_entries(&request.source_paths, &repo_root, &target_dir)?;
    let include_tree = import_plan.iter().any(|entry| entry.is_directory);
    let outcomes = copy_external_entries_parallel(import_plan, true)?;
    state.finish_file_copy_operation(&request.repo_id, parent_path, include_tree, outcomes)
}

pub(super) fn copy_entries(
    state: &RepositoryState,
    request: FileCopyRequest,
) -> Result<FileBrowserSnapshot, String> {
    state.ensure_initialized()?;
    let repo = state.load_repository_record(&request.repo_id)?;
    ensure_local_filesystem_repository(&repo, "copying files")?;

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
    let outcomes = copy_external_entries_parallel(copy_plan, hardlink_preferred)?;
    state.finish_file_copy_operation(&request.repo_id, parent_path, include_tree, outcomes)
}

pub(super) fn move_entries(
    state: &RepositoryState,
    request: FileMoveRequest,
) -> Result<FileBrowserSnapshot, String> {
    state.ensure_initialized()?;
    let repo = state.load_repository_record(&request.repo_id)?;
    ensure_local_filesystem_repository(&repo, "moving files")?;

    let repo_root = PathBuf::from(&repo.summary.path);
    let parent_path = normalize_directory_path(&request.parent_path)?;
    let target_dir = resolve_repository_relative_path(&repo_root, &parent_path)?;
    if !target_dir.exists() || !target_dir.is_dir() {
        return Err(format!("directory not found: {parent_path}"));
    }
    if request.source_paths.is_empty() {
        return Err("no source files were provided".to_string());
    }

    let move_plan =
        validate_repository_move_entries(&request.source_paths, &repo_root, &target_dir)?;
    let include_tree = move_plan.iter().any(|entry| entry.is_directory);
    let mut connection = state.open_repository_connection(
        &repo.summary.repo_id,
        &repo.summary.path,
        &repo.backend_record,
    )?;
    let tx = connection.transaction().map_err(db_error)?;

    for entry in &move_plan {
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
    }
    tx.commit().map_err(db_error)?;
    if include_tree {
        let _ = state.sync_repository(SyncRequest {
            repo_id: request.repo_id.clone(),
        })?;
    }

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

pub(super) fn rename_entry(
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

pub(super) fn delete_entry(
    state: &RepositoryState,
    request: FileDeleteRequest,
) -> Result<FileBrowserSnapshot, String> {
    state.ensure_initialized()?;
    let repo = state.load_repository_record(&request.repo_id)?;
    let repo_root = PathBuf::from(&repo.summary.path);
    let delete_mode = request.mode.as_deref().unwrap_or("delete");

    if delete_mode == "permanentDelete" {
        if repo.backend_record.plugin_id != LOCAL_FILESYSTEM_PLUGIN_ID {
            return Err(format!(
                "permanent trash delete is only supported for local filesystem repositories, got: {}",
                repo.backend_record.plugin_id
            ));
        }
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

    let entry_path = normalize_entry_path(&request.path)?;
    let parent_path = parent_relative_path(&entry_path);
    let entry = stat_backend_entry(&state.root, &repo, &repo_root, &entry_path)?;

    let is_directory = matches!(entry.kind, FileSystemEntryKind::Directory);
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
            if repo.backend_record.plugin_id != LOCAL_FILESYSTEM_PLUGIN_ID {
                return Err(format!(
                    "trash delete is only supported for local filesystem repositories, got: {}",
                    repo.backend_record.plugin_id
                ));
            }
            move_entry_to_trash(&repo_root, &entry_path, is_directory)?;
            let mut connection = state.open_repository_connection(
                &repo.summary.repo_id,
                &repo.summary.path,
                &repo.backend_record,
            )?;
            let tx = connection.transaction().map_err(db_error)?;
            mark_directory_assets_deleted(&tx, &request.repo_id, &entry_path).map_err(db_error)?;
            tx.commit().map_err(db_error)?;
            let _ = state.sync_repository(SyncRequest {
                repo_id: request.repo_id.clone(),
            })?;
        }
    } else {
        if repo.backend_record.plugin_id != LOCAL_FILESYSTEM_PLUGIN_ID {
            return Err(format!(
                "trash delete is only supported for local filesystem repositories, got: {}",
                repo.backend_record.plugin_id
            ));
        }
        move_entry_to_trash(&repo_root, &entry_path, is_directory)?;
        let mut connection = state.open_repository_connection(
            &repo.summary.repo_id,
            &repo.summary.path,
            &repo.backend_record,
        )?;
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

pub(super) fn mutate_trash(
    state: &RepositoryState,
    request: TrashMutationRequest,
) -> Result<FileBrowserSnapshot, String> {
    state.ensure_initialized()?;
    let repo = state.load_repository_record(&request.repo_id)?;
    if repo.backend_record.plugin_id != LOCAL_FILESYSTEM_PLUGIN_ID {
        return Err(format!(
            "trash operations are only supported for local filesystem repositories, got: {}",
            repo.backend_record.plugin_id
        ));
    }
    let repo_root = PathBuf::from(&repo.summary.path);

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
