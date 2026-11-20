//! 后端条目到文件浏览条目的映射、元数据附加与本地目录读取。

use super::*;

pub(in super::super) fn map_file_browser_entries(
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

pub(in super::super) fn map_trash_browser_entries(
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

pub(in super::super) fn attach_browser_entry_metadata(
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

pub(in super::super) fn local_directory_entries(
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
