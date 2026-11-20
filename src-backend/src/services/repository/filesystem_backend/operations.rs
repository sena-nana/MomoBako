//! 文件系统后端的列举、分页、增删改与回收站操作入口。

use super::*;

pub(in super::super) fn backend_adapter<'a>(
    service_root: &'a Path,
    repo: &'a RepositoryRecord,
) -> Box<dyn FileSystemBackendAdapter + 'a> {
    Box::new(RuntimeFileSystemBackendAdapter {
        service_root: service_root.to_path_buf(),
        plugin_id: repo.backend_record.plugin_id.clone(),
    })
}

pub(in super::super) fn list_backend_files(
    service_root: &Path,
    repo: &RepositoryRecord,
    repo_root: &Path,
) -> Result<Vec<DiscoveredFile>, String> {
    backend_adapter(service_root, repo).list_files(repo_root, &repo.backend_record.config)
}

pub(in super::super) fn list_backend_directory_entries(
    service_root: &Path,
    repo: &RepositoryRecord,
    repo_root: &Path,
    directory_path: &str,
) -> Result<Vec<FileSystemEntry>, String> {
    backend_adapter(service_root, repo).list_directory_entries(
        repo_root,
        directory_path,
        &repo.backend_record.config,
    )
}

pub(in super::super) fn list_backend_tree(
    service_root: &Path,
    repo: &RepositoryRecord,
    repo_root: &Path,
) -> Result<Vec<FileTreeNode>, String> {
    backend_adapter(service_root, repo).list_tree(repo_root, &repo.backend_record.config)
}

pub(in super::super) fn list_backend_directory_entries_page(
    service_root: &Path,
    repo: &RepositoryRecord,
    repo_root: &Path,
    directory_path: &str,
    offset: usize,
    limit: usize,
) -> Result<DirectoryPageResult, String> {
    backend_adapter(service_root, repo).list_directory_entries_page(
        repo_root,
        directory_path,
        offset,
        limit,
        &repo.backend_record.config,
    )
}

pub(in super::super) fn paginate_file_browser_entries(
    mut entries: Vec<FileBrowserEntry>,
    offset: usize,
    limit: Option<usize>,
) -> (Vec<FileBrowserEntry>, usize, usize, Option<usize>, bool) {
    let total_entries = entries.len();
    let start = offset.min(total_entries);
    let limit = limit.unwrap_or(total_entries.saturating_sub(start));
    let end = start.saturating_add(limit).min(total_entries);
    let paged_entries = if start == 0 && end == total_entries {
        entries
    } else {
        entries.drain(start..end).collect()
    };
    let loaded_count = end;
    let has_more = loaded_count < total_entries;
    let next_offset = has_more.then_some(loaded_count);
    (
        paged_entries,
        total_entries,
        loaded_count,
        next_offset,
        has_more,
    )
}

pub(super) fn is_unsupported_filesystem_backend_method(error: &str) -> bool {
    error.contains("unsupported method") || error.contains("unsupported filesystem plugin method")
}

pub(in super::super) fn list_trash_directory_entries(
    repo_root: &Path,
    current_path: &str,
    asset_map: &BTreeMap<String, AssetPathRecord>,
    thumbnail_map: &BTreeMap<(String, String), ThumbnailRecord>,
) -> Result<Vec<FileBrowserEntry>, String> {
    let trash_root = repository_trash_dir(repo_root);
    fs::create_dir_all(&trash_root).map_err(io_error)?;
    let manifest = load_trash_manifest(repo_root)?;
    let current_dir = resolve_trash_relative_path(&trash_root, current_path)?;
    if !current_dir.exists() || !current_dir.is_dir() {
        return Err(format!("trash directory not found: {current_path}"));
    }

    let entries = local_directory_entries(&trash_root, &current_dir)?;
    Ok(map_trash_browser_entries(
        entries,
        asset_map,
        thumbnail_map,
        &manifest,
    ))
}

pub(in super::super) fn create_backend_directory(
    service_root: &Path,
    repo: &RepositoryRecord,
    repo_root: &Path,
    parent_path: &str,
    name: &str,
) -> Result<(), String> {
    backend_adapter(service_root, repo).create_directory(
        repo_root,
        parent_path,
        name,
        &repo.backend_record.config,
    )
}

pub(in super::super) fn create_backend_file(
    service_root: &Path,
    repo: &RepositoryRecord,
    repo_root: &Path,
    parent_path: &str,
    name: &str,
) -> Result<(), String> {
    backend_adapter(service_root, repo).create_file(
        repo_root,
        parent_path,
        name,
        &repo.backend_record.config,
    )
}

#[allow(clippy::too_many_arguments)]
pub(in super::super) fn emit_entry_playback_progress(
    emit: &mut Option<&mut dyn FnMut(EntryPlaybackProgressEvent) -> Result<(), String>>,
    phase: &str,
    repo_id: &str,
    path: &str,
    value: u8,
    detail: &str,
    indeterminate: bool,
    cached: Option<bool>,
    error: Option<String>,
) -> Result<(), String> {
    if let Some(emit) = emit.as_deref_mut() {
        emit(EntryPlaybackProgressEvent {
            phase: phase.to_string(),
            repo_id: repo_id.to_string(),
            path: path.to_string(),
            value: value.min(100),
            detail: detail.to_string(),
            indeterminate,
            cached,
            error,
        })?;
    }
    Ok(())
}

pub(in super::super) fn stat_backend_entry(
    service_root: &Path,
    repo: &RepositoryRecord,
    repo_root: &Path,
    entry_path: &str,
) -> Result<FileSystemEntry, String> {
    #[cfg(test)]
    if let Some(result) = test_support::backend_stat_entry_hook(repo, repo_root, entry_path)? {
        return result;
    }

    backend_adapter(service_root, repo).stat_entry(
        repo_root,
        entry_path,
        &repo.backend_record.config,
    )
}

pub(in super::super) fn rename_backend_entry(
    service_root: &Path,
    repo: &RepositoryRecord,
    repo_root: &Path,
    source_path: &str,
    new_name: &str,
) -> Result<FileSystemEntry, String> {
    backend_adapter(service_root, repo).rename_entry(
        repo_root,
        source_path,
        new_name,
        &repo.backend_record.config,
    )
}

pub(in super::super) fn move_backend_entry(
    service_root: &Path,
    repo: &RepositoryRecord,
    repo_root: &Path,
    source_path: &str,
    target_parent_path: &str,
) -> Result<FileSystemEntry, String> {
    backend_adapter(service_root, repo).move_entry(
        repo_root,
        source_path,
        target_parent_path,
        &repo.backend_record.config,
    )
}

pub(in super::super) fn delete_backend_entry(
    service_root: &Path,
    repo: &RepositoryRecord,
    repo_root: &Path,
    entry_path: &str,
    recursive: bool,
) -> Result<(), String> {
    backend_adapter(service_root, repo).delete_entry(
        repo_root,
        entry_path,
        recursive,
        &repo.backend_record.config,
    )
}

pub(in super::super) fn describe_backend_repository_state(
    service_root: &Path,
    repo: &RepositoryRecord,
    repo_root: &Path,
) -> Result<Option<SourceRepositoryStateSnapshot>, String> {
    match backend_adapter(service_root, repo)
        .describe_repository_state(repo_root, &repo.backend_record.config)
    {
        Ok(state) => Ok(Some(state)),
        Err(error) if is_unsupported_filesystem_backend_method(&error) => Ok(None),
        Err(error) => Err(error),
    }
}

pub(in super::super) fn write_backend_asset_metadata(
    service_root: &Path,
    repo: &RepositoryRecord,
    repo_root: &Path,
    path: &str,
    shared_asset_id: Option<&str>,
    metadata: &BTreeMap<String, serde_json::Value>,
    previous_metadata: &BTreeMap<String, serde_json::Value>,
    operation: &str,
) -> Result<bool, String> {
    match backend_adapter(service_root, repo).write_asset_metadata(
        repo_root,
        path,
        shared_asset_id,
        metadata,
        previous_metadata,
        operation,
        &repo.backend_record.config,
    ) {
        Ok(()) => Ok(true),
        Err(error) if is_unsupported_filesystem_backend_method(&error) => Ok(false),
        Err(error) => Err(error),
    }
}

pub(in super::super) fn write_backend_repository_state(
    service_root: &Path,
    repo: &RepositoryRecord,
    repo_root: &Path,
    state: &SourceRepositoryStateSnapshot,
) -> Result<bool, String> {
    match backend_adapter(service_root, repo).write_repository_state(
        repo_root,
        state,
        &repo.backend_record.config,
    ) {
        Ok(()) => Ok(true),
        Err(error) if is_unsupported_filesystem_backend_method(&error) => Ok(false),
        Err(error) => Err(error),
    }
}

pub(in super::super) fn move_entry_to_trash(
    repo_root: &Path,
    entry_path: &str,
    is_directory: bool,
) -> Result<(), String> {
    let source_abs = resolve_repository_relative_path(repo_root, entry_path)?;
    if !source_abs.exists() {
        return Err(format!("entry not found: {entry_path}"));
    }

    let trash_root = repository_trash_dir(repo_root);
    fs::create_dir_all(&trash_root).map_err(io_error)?;
    let target_abs = unique_trash_target_path(&trash_root, entry_path)?;
    fs::rename(source_abs, &target_abs).map_err(io_error)?;

    let trash_path = trash_relative_path_for_target(&trash_root, &target_abs)?;
    let mut manifest = load_trash_manifest(repo_root)?;
    remove_manifest_paths(&mut manifest, &trash_path);
    manifest.entries.push(TrashManifestEntry {
        original_path: entry_path.to_string(),
        trash_path,
        deleted_at: now_rfc3339(),
        kind: if is_directory { "directory" } else { "file" }.to_string(),
    });
    save_trash_manifest(repo_root, &manifest)
}

pub(in super::super) fn delete_trash_entry(
    repo_root: &Path,
    trash_path: &str,
) -> Result<(), String> {
    let trash_root = repository_trash_dir(repo_root);
    let entry_abs = resolve_trash_relative_path(&trash_root, trash_path)?;
    if !entry_abs.exists() {
        return Err(format!("trash entry not found: {trash_path}"));
    }

    let metadata = entry_abs.metadata().map_err(io_error)?;
    if metadata.is_dir() {
        fs::remove_dir_all(entry_abs).map_err(io_error)?;
    } else {
        fs::remove_file(entry_abs).map_err(io_error)?;
    }

    let mut manifest = load_trash_manifest(repo_root)?;
    remove_manifest_paths(&mut manifest, trash_path);
    save_trash_manifest(repo_root, &manifest)
}

pub(in super::super) fn restore_trash_entry(
    repo_root: &Path,
    trash_path: &str,
) -> Result<(), String> {
    let trash_root = repository_trash_dir(repo_root);
    let entry_abs = resolve_trash_relative_path(&trash_root, trash_path)?;
    if !entry_abs.exists() {
        return Err(format!("trash entry not found: {trash_path}"));
    }

    let mut manifest = load_trash_manifest(repo_root)?;
    let manifest_entry = find_trash_manifest_entry(&manifest, trash_path)
        .cloned()
        .ok_or_else(|| format!("trash metadata not found: {trash_path}"))?;
    let original_path = original_path_for_trash_path(&manifest_entry, trash_path);
    let target_abs = resolve_repository_relative_path(repo_root, &original_path)?;

    restore_path_to_target(&entry_abs, &target_abs, &original_path)?;
    remove_manifest_paths(&mut manifest, trash_path);
    save_trash_manifest(repo_root, &manifest)?;
    prune_empty_trash_parents(&trash_root, trash_path)
}

pub(in super::super) fn restore_all_trash_entries(repo_root: &Path) -> Result<(), String> {
    let trash_root = repository_trash_dir(repo_root);
    fs::create_dir_all(&trash_root).map_err(io_error)?;
    let mut manifest = load_trash_manifest(repo_root)?;
    manifest
        .entries
        .sort_by(|left, right| left.trash_path.cmp(&right.trash_path));

    for entry in &manifest.entries {
        let entry_abs = resolve_trash_relative_path(&trash_root, &entry.trash_path)?;
        if !entry_abs.exists() {
            continue;
        }
        let target_abs = resolve_repository_relative_path(repo_root, &entry.original_path)?;
        ensure_restore_target_available(&entry_abs, &target_abs, &entry.original_path)?;
    }

    for entry in &manifest.entries {
        let entry_abs = resolve_trash_relative_path(&trash_root, &entry.trash_path)?;
        if !entry_abs.exists() {
            continue;
        }
        let target_abs = resolve_repository_relative_path(repo_root, &entry.original_path)?;
        restore_path_to_target(&entry_abs, &target_abs, &entry.original_path)?;
    }

    save_trash_manifest(repo_root, &TrashManifest::default())?;
    clean_empty_trash_directories(&trash_root)
}

pub(in super::super) fn empty_trash(repo_root: &Path) -> Result<(), String> {
    let trash_root = repository_trash_dir(repo_root);
    fs::create_dir_all(&trash_root).map_err(io_error)?;
    for entry in fs::read_dir(&trash_root).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let metadata = entry.metadata().map_err(io_error)?;
        if metadata.is_dir() {
            fs::remove_dir_all(entry.path()).map_err(io_error)?;
        } else {
            fs::remove_file(entry.path()).map_err(io_error)?;
        }
    }
    save_trash_manifest(repo_root, &TrashManifest::default())
}

pub(in super::super) fn clean_empty_trash_directories(trash_root: &Path) -> Result<(), String> {
    if !trash_root.exists() {
        return Ok(());
    }

    let mut directories = Vec::new();
    collect_trash_directories(trash_root, trash_root, &mut directories)?;
    directories.sort_by(|left, right| right.components().count().cmp(&left.components().count()));
    for directory in directories {
        if directory == trash_root {
            continue;
        }
        if directory.exists()
            && directory.is_dir()
            && fs::read_dir(&directory).map_err(io_error)?.next().is_none()
        {
            fs::remove_dir(directory).map_err(io_error)?;
        }
    }
    Ok(())
}

pub(in super::super) fn collect_trash_directories(
    trash_root: &Path,
    current_dir: &Path,
    directories: &mut Vec<PathBuf>,
) -> Result<(), String> {
    for entry in fs::read_dir(current_dir).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let path = entry.path();
        if path.is_dir() {
            collect_trash_directories(trash_root, &path, directories)?;
        }
    }
    directories.push(current_dir.to_path_buf());
    Ok(())
}
