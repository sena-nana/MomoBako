//! Filesystem backend adapter abstraction and browser entry mapping.

use super::*;

pub(super) trait FileSystemBackendAdapter {
    fn ensure_attachable(&self, repo_root: &Path, config: &serde_json::Value)
        -> Result<(), String>;

    fn prepare_repository_root(
        &self,
        repo_root: &Path,
        config: &serde_json::Value,
    ) -> Result<(), String>;

    fn list_files(
        &self,
        repo_root: &Path,
        config: &serde_json::Value,
    ) -> Result<Vec<DiscoveredFile>, String>;

    fn list_tree(
        &self,
        repo_root: &Path,
        config: &serde_json::Value,
    ) -> Result<Vec<FileTreeNode>, String>;

    fn list_directory_entries(
        &self,
        repo_root: &Path,
        directory_path: &str,
        config: &serde_json::Value,
    ) -> Result<Vec<FileSystemEntry>, String>;

    fn list_directory_entries_page(
        &self,
        repo_root: &Path,
        directory_path: &str,
        offset: usize,
        limit: usize,
        config: &serde_json::Value,
    ) -> Result<DirectoryPageResult, String>;

    fn create_directory(
        &self,
        repo_root: &Path,
        parent_path: &str,
        name: &str,
        config: &serde_json::Value,
    ) -> Result<(), String>;

    fn create_file(
        &self,
        repo_root: &Path,
        parent_path: &str,
        name: &str,
        config: &serde_json::Value,
    ) -> Result<(), String>;

    fn stat_entry(
        &self,
        repo_root: &Path,
        entry_path: &str,
        config: &serde_json::Value,
    ) -> Result<FileSystemEntry, String>;

    fn rename_entry(
        &self,
        repo_root: &Path,
        source_path: &str,
        new_name: &str,
        config: &serde_json::Value,
    ) -> Result<FileSystemEntry, String>;

    fn move_entry(
        &self,
        repo_root: &Path,
        source_path: &str,
        target_parent_path: &str,
        config: &serde_json::Value,
    ) -> Result<FileSystemEntry, String>;

    fn delete_entry(
        &self,
        repo_root: &Path,
        entry_path: &str,
        recursive: bool,
        config: &serde_json::Value,
    ) -> Result<(), String>;

    fn describe_repository_state(
        &self,
        repo_root: &Path,
        config: &serde_json::Value,
    ) -> Result<SourceRepositoryStateSnapshot, String>;

    fn write_asset_metadata(
        &self,
        repo_root: &Path,
        path: &str,
        shared_asset_id: Option<&str>,
        metadata: &BTreeMap<String, serde_json::Value>,
        previous_metadata: &BTreeMap<String, serde_json::Value>,
        operation: &str,
        config: &serde_json::Value,
    ) -> Result<(), String>;

    fn write_repository_state(
        &self,
        repo_root: &Path,
        state: &SourceRepositoryStateSnapshot,
        config: &serde_json::Value,
    ) -> Result<(), String>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum FileSystemEntryKind {
    Directory,
    File,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct FileSystemEntry {
    pub(super) path: String,
    pub(super) name: String,
    pub(super) kind: FileSystemEntryKind,
    pub(super) extension: Option<String>,
    pub(super) size_bytes: Option<i64>,
    pub(super) modified_at: Option<String>,
    #[serde(default)]
    pub(super) is_virtual: bool,
    #[serde(default)]
    pub(super) provider_id: Option<String>,
    #[serde(default)]
    pub(super) provider_item_id: Option<String>,
    #[serde(default)]
    pub(super) source_payload: Option<serde_json::Value>,
    #[serde(default)]
    pub(super) local_absolute_path: Option<String>,
    #[serde(default)]
    pub(super) status: Option<String>,
    #[serde(default)]
    pub(super) shared_asset_id: Option<String>,
    #[serde(default)]
    pub(super) tags: Option<Vec<String>>,
    #[serde(default)]
    pub(super) thumbnail_local_absolute_path: Option<String>,
}

pub(super) struct RuntimeFileSystemBackendAdapter {
    pub(super) service_root: PathBuf,
    pub(super) plugin_id: String,
}

pub(super) fn backend_adapter<'a>(
    service_root: &'a Path,
    repo: &'a RepositoryRecord,
) -> Box<dyn FileSystemBackendAdapter + 'a> {
    Box::new(RuntimeFileSystemBackendAdapter {
        service_root: service_root.to_path_buf(),
        plugin_id: repo.backend_record.plugin_id.clone(),
    })
}

pub(super) fn list_backend_files(
    service_root: &Path,
    repo: &RepositoryRecord,
    repo_root: &Path,
) -> Result<Vec<DiscoveredFile>, String> {
    backend_adapter(service_root, repo).list_files(repo_root, &repo.backend_record.config)
}

pub(super) fn list_backend_directory_entries(
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

pub(super) fn list_backend_tree(
    service_root: &Path,
    repo: &RepositoryRecord,
    repo_root: &Path,
) -> Result<Vec<FileTreeNode>, String> {
    backend_adapter(service_root, repo).list_tree(repo_root, &repo.backend_record.config)
}

pub(super) fn list_backend_directory_entries_page(
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

pub(super) fn paginate_file_browser_entries(
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

fn is_unsupported_filesystem_backend_method(error: &str) -> bool {
    error.contains("unsupported method") || error.contains("unsupported filesystem plugin method")
}

pub(super) fn list_trash_directory_entries(
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

pub(super) fn create_backend_directory(
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

pub(super) fn create_backend_file(
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
pub(super) fn emit_entry_playback_progress(
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

pub(super) fn stat_backend_entry(
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

pub(super) fn rename_backend_entry(
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

pub(super) fn move_backend_entry(
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

pub(super) fn delete_backend_entry(
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

pub(super) fn describe_backend_repository_state(
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

pub(super) fn write_backend_asset_metadata(
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

pub(super) fn write_backend_repository_state(
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

pub(super) fn move_entry_to_trash(
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

pub(super) fn delete_trash_entry(repo_root: &Path, trash_path: &str) -> Result<(), String> {
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

pub(super) fn restore_trash_entry(repo_root: &Path, trash_path: &str) -> Result<(), String> {
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

pub(super) fn restore_all_trash_entries(repo_root: &Path) -> Result<(), String> {
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

pub(super) fn empty_trash(repo_root: &Path) -> Result<(), String> {
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

pub(super) fn clean_empty_trash_directories(trash_root: &Path) -> Result<(), String> {
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

pub(super) fn collect_trash_directories(
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

pub(super) fn build_directory_tree(repo_root: &Path) -> Result<Vec<FileTreeNode>, String> {
    let mut children = Vec::new();
    let entries = fs::read_dir(repo_root).map_err(io_error)?;
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) if is_skippable_filesystem_error(&error) => continue,
            Err(error) => return Err(io_error(error)),
        };
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(error) if is_skippable_filesystem_error(&error) => continue,
            Err(error) => return Err(io_error(error)),
        };
        if !metadata.is_dir() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().to_string();
        if is_internal_repository_dir(&name) {
            continue;
        }

        let path = name.clone();
        if let Some(node) = build_directory_node(repo_root, &path)? {
            children.push(node);
        }
    }

    children.sort_by(|left, right| left.label.to_lowercase().cmp(&right.label.to_lowercase()));
    Ok(children)
}

pub(super) fn build_directory_node(
    repo_root: &Path,
    relative_path: &str,
) -> Result<Option<FileTreeNode>, String> {
    let abs_path = resolve_repository_relative_path(repo_root, relative_path)?;
    let mut children = Vec::new();
    let mut file_count = 0;

    let entries = match fs::read_dir(&abs_path) {
        Ok(entries) => entries,
        Err(error) if is_skippable_filesystem_error(&error) => return Ok(None),
        Err(error) => return Err(io_error(error)),
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) if is_skippable_filesystem_error(&error) => continue,
            Err(error) => return Err(io_error(error)),
        };
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(error) if is_skippable_filesystem_error(&error) => continue,
            Err(error) => return Err(io_error(error)),
        };
        if !metadata.is_dir() {
            file_count += 1;
            continue;
        }

        let name = entry.file_name().to_string_lossy().to_string();
        if is_internal_repository_dir(&name) {
            continue;
        }

        let child_path = join_relative_path(relative_path, &name);
        if let Some(node) = build_directory_node(repo_root, &child_path)? {
            children.push(node);
        }
    }

    children.sort_by(|left, right| left.label.to_lowercase().cmp(&right.label.to_lowercase()));
    Ok(Some(FileTreeNode {
        path: relative_path.to_string(),
        label: Path::new(relative_path)
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| relative_path.to_string()),
        file_count,
        children,
    }))
}

pub(super) fn map_file_browser_entries(
    mut entries: Vec<FileSystemEntry>,
    asset_map: &BTreeMap<String, AssetPathRecord>,
    thumbnail_map: &BTreeMap<(String, String), ThumbnailRecord>,
    folder_metadata: &BTreeMap<String, FolderMetadata>,
) -> Vec<FileBrowserEntry> {
    entries.sort_by(|left, right| match (&left.kind, &right.kind) {
        (FileSystemEntryKind::Directory, FileSystemEntryKind::File) => std::cmp::Ordering::Less,
        (FileSystemEntryKind::File, FileSystemEntryKind::Directory) => std::cmp::Ordering::Greater,
        _ => left.path.to_lowercase().cmp(&right.path.to_lowercase()),
    });

    entries
        .into_iter()
        .map(|entry| {
            let kind = match entry.kind {
                FileSystemEntryKind::Directory => "directory",
                FileSystemEntryKind::File => "file",
            };
            let asset_record = asset_map.get(&entry.path);
            let asset_id = asset_record.map(|record| record.asset_id.clone());
            let status = asset_record.map(|record| record.status.clone());
            let asset_thumbnail_path =
                asset_record.and_then(|record| record.thumbnail_path.clone());
            let hardlink_group_id =
                asset_record.and_then(|record| record.hardlink_group_id.clone());
            let hardlink_state = asset_record.and_then(|record| record.hardlink_state.clone());
            let is_virtual = asset_record
                .map(|record| record.is_virtual)
                .unwrap_or(entry.is_virtual);
            let provider_id = asset_record
                .and_then(|record| record.provider_id.clone())
                .or(entry.provider_id.clone());
            let provider_item_id = asset_record
                .and_then(|record| record.provider_item_id.clone())
                .or(entry.provider_item_id.clone());
            let source_payload = asset_record
                .and_then(|record| record.source_payload.clone())
                .or(entry.source_payload.clone());
            let local_absolute_path = asset_record
                .and_then(|record| record.local_absolute_path.clone())
                .or(entry.local_absolute_path.clone());
            let entry_thumbnail = thumbnail_map.get(&(entry.path.clone(), kind.to_string()));
            let thumbnail_path = entry_thumbnail
                .map(|record| record.path.clone())
                .or(asset_thumbnail_path);
            let thumbnail_custom = entry_thumbnail.map(|record| record.custom).unwrap_or(false);
            let size_bytes = entry.size_bytes;
            FileBrowserEntry {
                path: entry.path.clone(),
                name: entry.name,
                kind: kind.to_string(),
                extension: entry.extension,
                size_bytes,
                size_label: size_bytes.map(format_size_label),
                modified_at: entry.modified_at,
                asset_id,
                status,
                thumbnail_path,
                thumbnail_custom,
                hardlink_group_id,
                hardlink_state,
                tags: Vec::new(),
                alias_paths: Vec::new(),
                folder_metadata: folder_metadata.get(&entry.path).cloned(),
                metadata: BTreeMap::new(),
                is_virtual,
                provider_id,
                provider_item_id,
                source_payload,
                local_absolute_path,
            }
        })
        .collect()
}

pub(super) fn map_trash_browser_entries(
    mut entries: Vec<FileSystemEntry>,
    asset_map: &BTreeMap<String, AssetPathRecord>,
    thumbnail_map: &BTreeMap<(String, String), ThumbnailRecord>,
    manifest: &TrashManifest,
) -> Vec<FileBrowserEntry> {
    entries.sort_by(|left, right| match (&left.kind, &right.kind) {
        (FileSystemEntryKind::Directory, FileSystemEntryKind::File) => std::cmp::Ordering::Less,
        (FileSystemEntryKind::File, FileSystemEntryKind::Directory) => std::cmp::Ordering::Greater,
        _ => left.name.to_lowercase().cmp(&right.name.to_lowercase()),
    });

    entries
        .into_iter()
        .map(|entry| {
            let trash_path = entry.path.clone();
            let manifest_entry = find_trash_manifest_entry(manifest, &trash_path);
            let original_path = manifest_entry
                .map(|item| original_path_for_trash_path(item, &trash_path))
                .unwrap_or_else(|| trash_path.clone());
            let kind = match entry.kind {
                FileSystemEntryKind::Directory => "directory",
                FileSystemEntryKind::File => "file",
            };
            let asset_record = asset_map.get(&original_path);
            let asset_id = asset_record.map(|record| record.asset_id.clone());
            let status = asset_record
                .map(|record| record.status.clone())
                .or_else(|| Some("deleted".to_string()));
            let asset_thumbnail_path =
                asset_record.and_then(|record| record.thumbnail_path.clone());
            let hardlink_group_id =
                asset_record.and_then(|record| record.hardlink_group_id.clone());
            let hardlink_state = asset_record.and_then(|record| record.hardlink_state.clone());
            let is_virtual = asset_record
                .map(|record| record.is_virtual)
                .unwrap_or(false);
            let provider_id = asset_record.and_then(|record| record.provider_id.clone());
            let provider_item_id = asset_record.and_then(|record| record.provider_item_id.clone());
            let source_payload = asset_record.and_then(|record| record.source_payload.clone());
            let local_absolute_path =
                asset_record.and_then(|record| record.local_absolute_path.clone());
            let entry_thumbnail = thumbnail_map.get(&(original_path.clone(), kind.to_string()));
            let thumbnail_path = entry_thumbnail
                .map(|record| record.path.clone())
                .or(asset_thumbnail_path);
            let thumbnail_custom = entry_thumbnail.map(|record| record.custom).unwrap_or(false);
            let mut metadata = BTreeMap::new();
            if let Some(item) = manifest_entry {
                metadata.insert(
                    "deletedAt".to_string(),
                    serde_json::Value::String(item.deleted_at.clone()),
                );
                metadata.insert(
                    "originalPath".to_string(),
                    serde_json::Value::String(original_path),
                );
            }
            let size_bytes = entry.size_bytes;
            FileBrowserEntry {
                path: trash_path,
                name: entry.name,
                kind: kind.to_string(),
                extension: entry.extension,
                size_bytes,
                size_label: size_bytes.map(format_size_label),
                modified_at: entry.modified_at,
                asset_id,
                status,
                thumbnail_path,
                thumbnail_custom,
                hardlink_group_id,
                hardlink_state,
                tags: Vec::new(),
                alias_paths: Vec::new(),
                folder_metadata: None,
                metadata,
                is_virtual,
                provider_id,
                provider_item_id,
                source_payload,
                local_absolute_path,
            }
        })
        .collect()
}

pub(super) fn attach_browser_entry_metadata(
    connection: &Connection,
    repo_id: &str,
    mut entries: Vec<FileBrowserEntry>,
) -> Result<Vec<FileBrowserEntry>, rusqlite::Error> {
    let asset_ids = entries
        .iter()
        .filter_map(|entry| entry.asset_id.clone())
        .collect::<Vec<_>>();
    let metadata_by_asset = load_metadata_maps_for_assets(connection, &asset_ids)?;
    let alias_paths_by_asset = load_alias_paths_for_assets(connection, repo_id, &asset_ids)?;
    let tags_by_asset = load_tags_for_assets(connection, &asset_ids)?;

    for entry in &mut entries {
        let Some(asset_id) = &entry.asset_id else {
            continue;
        };
        if let Some(metadata) = metadata_by_asset.get(asset_id) {
            let mut merged = metadata.clone();
            merged.extend(entry.metadata.clone());
            normalize_loaded_metadata(&mut merged);
            entry.metadata = merged;
        }
        entry.tags = tags_by_asset.get(asset_id).cloned().unwrap_or_default();
        entry.alias_paths = alias_paths_by_asset
            .get(asset_id)
            .cloned()
            .unwrap_or_default();
    }

    Ok(entries)
}

pub(super) fn local_directory_entries(
    repo_root: &Path,
    current_dir: &Path,
) -> Result<Vec<FileSystemEntry>, String> {
    let mut entries = Vec::new();

    for entry in fs::read_dir(current_dir).map_err(io_error)? {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) if is_skippable_filesystem_error(&error) => continue,
            Err(error) => return Err(io_error(error)),
        };
        let name = entry.file_name().to_string_lossy().to_string();
        if is_internal_repository_dir(&name) {
            continue;
        }

        let path = entry.path();
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(error) if is_skippable_filesystem_error(&error) => continue,
            Err(error) => return Err(io_error(error)),
        };
        let relative_path = path
            .strip_prefix(repo_root)
            .map_err(path_error)?
            .to_string_lossy()
            .replace('\\', "/");

        entries.push(FileSystemEntry {
            path: relative_path,
            name,
            kind: if metadata.is_dir() {
                FileSystemEntryKind::Directory
            } else {
                FileSystemEntryKind::File
            },
            extension: if metadata.is_file() {
                path.extension()
                    .map(|value| value.to_string_lossy().to_string())
            } else {
                None
            },
            size_bytes: if metadata.is_file() {
                Some(metadata.len() as i64)
            } else {
                None
            },
            modified_at: metadata
                .modified()
                .ok()
                .map(system_time_to_rfc3339)
                .transpose()
                .map_err(time_error)?,
            is_virtual: false,
            provider_id: None,
            provider_item_id: None,
            source_payload: None,
            local_absolute_path: Some(path.to_string_lossy().to_string()),
            status: None,
            shared_asset_id: None,
            tags: None,
            thumbnail_local_absolute_path: None,
        });
    }

    Ok(entries)
}

impl FileSystemBackendAdapter for RuntimeFileSystemBackendAdapter {
    fn ensure_attachable(
        &self,
        repo_root: &Path,
        config: &serde_json::Value,
    ) -> Result<(), String> {
        plugin_catalog(&self.service_root).call(
            &self.plugin_id,
            "filesystem.ensureAttachable",
            serde_json::json!({
                "repoRoot": repo_root,
                "config": config,
            }),
        )?;
        Ok(())
    }

    fn prepare_repository_root(
        &self,
        repo_root: &Path,
        config: &serde_json::Value,
    ) -> Result<(), String> {
        plugin_catalog(&self.service_root).call(
            &self.plugin_id,
            "filesystem.prepareRepositoryRoot",
            serde_json::json!({
                "repoRoot": repo_root,
                "config": config,
            }),
        )?;
        Ok(())
    }

    fn list_files(
        &self,
        repo_root: &Path,
        config: &serde_json::Value,
    ) -> Result<Vec<DiscoveredFile>, String> {
        let response = plugin_catalog(&self.service_root).call(
            &self.plugin_id,
            "filesystem.listFiles",
            serde_json::json!({
                "repoRoot": repo_root,
                "config": config,
            }),
        )?;
        serde_json::from_value::<Vec<BackendDiscoveredFile>>(response)
            .map_err(json_error)?
            .into_iter()
            .map(|file| file.into_discovered_file(repo_root))
            .collect()
    }

    fn list_tree(
        &self,
        repo_root: &Path,
        config: &serde_json::Value,
    ) -> Result<Vec<FileTreeNode>, String> {
        let response = plugin_catalog(&self.service_root).call(
            &self.plugin_id,
            "filesystem.listTree",
            serde_json::json!({
                "repoRoot": repo_root,
                "config": config,
            }),
        )?;
        serde_json::from_value(response).map_err(json_error)
    }

    fn list_directory_entries(
        &self,
        repo_root: &Path,
        directory_path: &str,
        config: &serde_json::Value,
    ) -> Result<Vec<FileSystemEntry>, String> {
        let response = plugin_catalog(&self.service_root).call(
            &self.plugin_id,
            "filesystem.listDirectory",
            serde_json::json!({
                "repoRoot": repo_root,
                "directoryPath": directory_path,
                "config": config,
            }),
        )?;
        serde_json::from_value(response).map_err(json_error)
    }

    fn list_directory_entries_page(
        &self,
        repo_root: &Path,
        directory_path: &str,
        offset: usize,
        limit: usize,
        config: &serde_json::Value,
    ) -> Result<DirectoryPageResult, String> {
        let response = plugin_catalog(&self.service_root).call(
            &self.plugin_id,
            "filesystem.listDirectoryPage",
            serde_json::json!({
                "repoRoot": repo_root,
                "directoryPath": directory_path,
                "offset": offset,
                "limit": limit,
                "config": config,
            }),
        );
        match response {
            Ok(value) => serde_json::from_value(value).map_err(json_error),
            Err(error) if is_unsupported_filesystem_backend_method(&error) => {
                let entries = self.list_directory_entries(repo_root, directory_path, config)?;
                let total_entries = entries.len();
                Ok(DirectoryPageResult {
                    entries: entries.into_iter().skip(offset).take(limit).collect(),
                    total_entries,
                })
            }
            Err(error) => Err(error),
        }
    }

    fn create_directory(
        &self,
        repo_root: &Path,
        parent_path: &str,
        name: &str,
        config: &serde_json::Value,
    ) -> Result<(), String> {
        plugin_catalog(&self.service_root).call(
            &self.plugin_id,
            "filesystem.createDirectory",
            serde_json::json!({
                "repoRoot": repo_root,
                "parentPath": parent_path,
                "name": name,
                "config": config,
            }),
        )?;
        Ok(())
    }

    fn create_file(
        &self,
        repo_root: &Path,
        parent_path: &str,
        name: &str,
        config: &serde_json::Value,
    ) -> Result<(), String> {
        plugin_catalog(&self.service_root).call(
            &self.plugin_id,
            "filesystem.createFile",
            serde_json::json!({
                "repoRoot": repo_root,
                "parentPath": parent_path,
                "name": name,
                "config": config,
            }),
        )?;
        Ok(())
    }

    fn stat_entry(
        &self,
        repo_root: &Path,
        entry_path: &str,
        config: &serde_json::Value,
    ) -> Result<FileSystemEntry, String> {
        let response = plugin_catalog(&self.service_root).call(
            &self.plugin_id,
            "filesystem.statEntry",
            serde_json::json!({
                "repoRoot": repo_root,
                "entryPath": entry_path,
                "config": config,
            }),
        )?;
        serde_json::from_value(response).map_err(json_error)
    }

    fn rename_entry(
        &self,
        repo_root: &Path,
        source_path: &str,
        new_name: &str,
        config: &serde_json::Value,
    ) -> Result<FileSystemEntry, String> {
        let response = plugin_catalog(&self.service_root).call(
            &self.plugin_id,
            "filesystem.renameEntry",
            serde_json::json!({
                "repoRoot": repo_root,
                "sourcePath": source_path,
                "newName": new_name,
                "config": config,
            }),
        )?;
        serde_json::from_value(response).map_err(json_error)
    }

    fn move_entry(
        &self,
        repo_root: &Path,
        source_path: &str,
        target_parent_path: &str,
        config: &serde_json::Value,
    ) -> Result<FileSystemEntry, String> {
        let response = plugin_catalog(&self.service_root).call(
            &self.plugin_id,
            "filesystem.moveEntry",
            serde_json::json!({
                "repoRoot": repo_root,
                "sourcePath": source_path,
                "targetParentPath": target_parent_path,
                "config": config,
            }),
        )?;
        serde_json::from_value(response).map_err(json_error)
    }

    fn delete_entry(
        &self,
        repo_root: &Path,
        entry_path: &str,
        recursive: bool,
        config: &serde_json::Value,
    ) -> Result<(), String> {
        plugin_catalog(&self.service_root).call(
            &self.plugin_id,
            "filesystem.deleteEntry",
            serde_json::json!({
                "repoRoot": repo_root,
                "entryPath": entry_path,
                "recursive": recursive,
                "config": config,
            }),
        )?;
        Ok(())
    }

    fn describe_repository_state(
        &self,
        repo_root: &Path,
        config: &serde_json::Value,
    ) -> Result<SourceRepositoryStateSnapshot, String> {
        let response = plugin_catalog(&self.service_root).call(
            &self.plugin_id,
            "filesystem.describeRepositoryState",
            serde_json::json!({
                "repoRoot": repo_root,
                "config": config,
            }),
        )?;
        serde_json::from_value(response).map_err(json_error)
    }

    fn write_asset_metadata(
        &self,
        repo_root: &Path,
        path: &str,
        shared_asset_id: Option<&str>,
        metadata: &BTreeMap<String, serde_json::Value>,
        previous_metadata: &BTreeMap<String, serde_json::Value>,
        operation: &str,
        config: &serde_json::Value,
    ) -> Result<(), String> {
        plugin_catalog(&self.service_root).call(
            &self.plugin_id,
            "filesystem.writeAssetMetadata",
            serde_json::json!({
                "repoRoot": repo_root,
                "path": path,
                "sharedAssetId": shared_asset_id,
                "metadata": metadata,
                "previousMetadata": previous_metadata,
                "operation": operation,
                "config": config,
            }),
        )?;
        Ok(())
    }

    fn write_repository_state(
        &self,
        repo_root: &Path,
        state: &SourceRepositoryStateSnapshot,
        config: &serde_json::Value,
    ) -> Result<(), String> {
        plugin_catalog(&self.service_root).call(
            &self.plugin_id,
            "filesystem.writeRepositoryState",
            serde_json::json!({
                "repoRoot": repo_root,
                "directoryMetadataByPath": &state.directory_metadata_by_path,
                "quickAccess": &state.quick_access,
                "tagGroups": &state.tag_groups,
                "smartFolders": &state.smart_folders,
                "repositoryActions": &state.repository_actions,
                "config": config,
            }),
        )?;
        Ok(())
    }
}
