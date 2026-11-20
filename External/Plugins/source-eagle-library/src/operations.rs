//! Eagle 资源库的挂载校验、快照读取、条目增删改移与元数据、仓库状态写回。

use super::*;

pub(super) fn ensure_attachable(repo_root: &Path) -> Result<(), String> {
    if !repo_root.is_dir() {
        return Err(format!(
            "Eagle library is not a directory: {}",
            repo_root.to_string_lossy()
        ));
    }
    for required in [METADATA_FILE_NAME, "images"] {
        if !repo_root.join(required).exists() {
            return Err(format!("invalid Eagle library, missing: {required}"));
        }
    }
    Ok(())
}

pub(super) fn load_snapshot(repo_root: &Path) -> Result<EagleSourceSnapshot, String> {
    build_eagle_source_snapshot(repo_root)
}

pub(super) fn stat_entry(
    snapshot: &EagleSourceSnapshot,
    entry_path: &str,
) -> Result<EagleSourceEntry, String> {
    let entry_path = normalize_relative_path(entry_path);
    for entries in snapshot.directories.values() {
        if let Some(entry) = entries.iter().find(|entry| entry.path == entry_path) {
            return Ok(entry.clone());
        }
    }
    let file = snapshot
        .files
        .iter()
        .find(|file| file.relative_path == entry_path)
        .ok_or_else(|| format!("entry not found: {entry_path}"))?;
    Ok(EagleSourceEntry {
        path: file.relative_path.clone(),
        name: file.filename.clone(),
        kind: EagleSourceEntryKind::File,
        extension: Some(file.extension.clone()),
        size_bytes: Some(file.size_bytes),
        modified_at: Some(file.modified_at.clone()),
        is_virtual: file.is_virtual,
        provider_id: file.provider_id.clone(),
        provider_item_id: file.provider_item_id.clone(),
        source_payload: file.source_payload.clone(),
        local_absolute_path: file.local_absolute_path.clone(),
        status: file.status.clone(),
        shared_asset_id: file.shared_asset_id.clone(),
        tags: file.tags.clone(),
        thumbnail_local_absolute_path: file.thumbnail_local_absolute_path.clone(),
    })
}

pub(super) fn create_directory(
    repo_root: &Path,
    parent_path: &str,
    name: &str,
) -> Result<(), String> {
    let mut metadata = load_metadata_map(repo_root)?;
    let new_folder = serde_json::json!({
        "id": generate_eagle_id("folder"),
        "name": name,
        "children": [],
    });
    insert_folder(&mut metadata, parent_path, new_folder)?;
    touch_library_metadata(&mut metadata);
    save_metadata_map(repo_root, &metadata)
}

pub(super) fn create_file(repo_root: &Path, parent_path: &str, name: &str) -> Result<(), String> {
    let metadata_root = load_metadata_map(repo_root)?;
    let shared_asset_id = generate_eagle_id("asset");
    let info_dir = info_dir_for_asset(repo_root, &shared_asset_id);
    if info_dir.exists() {
        return Err(format!("asset already exists: {shared_asset_id}"));
    }
    fs::create_dir_all(&info_dir).map_err(io_error)?;
    let file_path = info_dir.join(name);
    OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&file_path)
        .map_err(io_error)?;
    let (display_name, extension) = split_display_name(name);
    let mut asset = Map::new();
    asset.insert("id".to_string(), Value::String(shared_asset_id));
    asset.insert("name".to_string(), Value::String(display_name));
    asset.insert("ext".to_string(), Value::String(extension));
    asset.insert("size".to_string(), serde_json::json!(0));
    asset.insert("btime".to_string(), serde_json::json!(now_unix_millis()));
    asset.insert("mtime".to_string(), serde_json::json!(now_unix_millis()));
    asset.insert("tags".to_string(), Value::Array(Vec::new()));
    asset.insert(
        "folders".to_string(),
        Value::Array(
            folder_id_for_path(&metadata_root, parent_path)?
                .into_iter()
                .map(Value::String)
                .collect(),
        ),
    );
    asset.insert("isDeleted".to_string(), Value::Bool(false));
    asset.insert("url".to_string(), Value::String(String::new()));
    asset.insert("annotation".to_string(), Value::String(String::new()));
    asset.insert(
        "modificationTime".to_string(),
        serde_json::json!(now_unix_millis()),
    );
    asset.insert(
        "lastModified".to_string(),
        serde_json::json!(now_unix_millis()),
    );
    save_json_pretty(&info_dir.join(METADATA_FILE_NAME), &Value::Object(asset))
}

pub(super) fn rename_entry(
    repo_root: &Path,
    source_path: &str,
    new_name: &str,
) -> Result<(), String> {
    let snapshot = load_snapshot(repo_root)?;
    let entry = stat_entry(&snapshot, source_path)?;
    match entry.kind {
        EagleSourceEntryKind::Directory => {
            let mut metadata = load_metadata_map(repo_root)?;
            let folder = find_folder_mut(&mut metadata, source_path)?
                .ok_or_else(|| format!("directory not found: {source_path}"))?;
            folder.insert("name".to_string(), Value::String(new_name.to_string()));
            touch_library_metadata(&mut metadata);
            save_metadata_map(repo_root, &metadata)
        }
        EagleSourceEntryKind::File => {
            let shared_asset_id =
                resolve_shared_asset_id(&snapshot, source_path, entry.shared_asset_id.as_deref())?;
            let info_dir = info_dir_for_asset(repo_root, &shared_asset_id);
            let old_file_path = file_absolute_path_from_entry(&entry)?;
            let new_file_path = info_dir.join(new_name);
            if new_file_path.exists() {
                return Err(format!("entry already exists: {new_name}"));
            }
            fs::rename(&old_file_path, &new_file_path).map_err(io_error)?;
            rename_thumbnail_with_file(&info_dir, &old_file_path, &new_file_path)?;
            let mut asset = load_asset_metadata_map(repo_root, &shared_asset_id)?;
            let (display_name, extension) = split_display_name(new_name);
            asset.insert("name".to_string(), Value::String(display_name));
            asset.insert("ext".to_string(), Value::String(extension));
            touch_asset_metadata(&mut asset, &new_file_path)?;
            save_asset_metadata_map(repo_root, &shared_asset_id, &asset)
        }
    }
}

pub(super) fn move_entry(
    repo_root: &Path,
    source_path: &str,
    target_parent_path: &str,
) -> Result<(), String> {
    let snapshot = load_snapshot(repo_root)?;
    let entry = stat_entry(&snapshot, source_path)?;
    match entry.kind {
        EagleSourceEntryKind::Directory => {
            let mut metadata = load_metadata_map(repo_root)?;
            let folder = remove_folder(&mut metadata, source_path)?
                .ok_or_else(|| format!("directory not found: {source_path}"))?;
            insert_folder(&mut metadata, target_parent_path, folder)?;
            touch_library_metadata(&mut metadata);
            save_metadata_map(repo_root, &metadata)
        }
        EagleSourceEntryKind::File => {
            let shared_asset_id =
                resolve_shared_asset_id(&snapshot, source_path, entry.shared_asset_id.as_deref())?;
            let mut root_metadata = load_metadata_map(repo_root)?;
            let mut asset = load_asset_metadata_map(repo_root, &shared_asset_id)?;
            rewrite_asset_membership(
                &snapshot,
                &root_metadata,
                &mut asset,
                &shared_asset_id,
                source_path,
                target_parent_path,
                false,
            )?;
            touch_library_metadata(&mut root_metadata);
            save_metadata_map(repo_root, &root_metadata)?;
            save_asset_metadata_map(repo_root, &shared_asset_id, &asset)
        }
    }
}

pub(super) fn delete_entry(
    repo_root: &Path,
    entry_path: &str,
    _recursive: bool,
) -> Result<(), String> {
    let snapshot = load_snapshot(repo_root)?;
    let entry = stat_entry(&snapshot, entry_path)?;
    match entry.kind {
        EagleSourceEntryKind::Directory => {
            let entries = snapshot
                .directories
                .get(&normalize_relative_path(entry_path))
                .cloned()
                .unwrap_or_default();
            if !entries.is_empty() {
                return Err("当前仅支持删除空目录，请先移动或删除目录内素材".to_string());
            }
            let mut metadata = load_metadata_map(repo_root)?;
            let removed = remove_folder(&mut metadata, entry_path)?;
            if removed.is_none() {
                return Err(format!("directory not found: {entry_path}"));
            }
            touch_library_metadata(&mut metadata);
            save_metadata_map(repo_root, &metadata)
        }
        EagleSourceEntryKind::File => {
            let shared_asset_id =
                resolve_shared_asset_id(&snapshot, entry_path, entry.shared_asset_id.as_deref())?;
            let mut asset = load_asset_metadata_map(repo_root, &shared_asset_id)?;
            let member_paths = shared_asset_paths(&snapshot, &shared_asset_id);
            let member_index = member_paths
                .iter()
                .position(|path| path == &normalize_relative_path(entry_path))
                .ok_or_else(|| format!("asset membership not found: {entry_path}"))?;
            let mut folder_ids = asset_folder_ids(&asset);
            if member_paths.len() > 1 && member_index < folder_ids.len() {
                folder_ids.remove(member_index);
                asset.insert(
                    "folders".to_string(),
                    Value::Array(folder_ids.into_iter().map(Value::String).collect()),
                );
            } else {
                asset.insert("isDeleted".to_string(), Value::Bool(true));
            }
            touch_asset_metadata(
                &mut asset,
                Path::new(&file_absolute_path_from_entry(&entry)?),
            )?;
            save_asset_metadata_map(repo_root, &shared_asset_id, &asset)
        }
    }
}

pub(super) fn write_asset_metadata(
    repo_root: &Path,
    path: &str,
    shared_asset_id: Option<&str>,
    metadata: &BTreeMap<String, Value>,
    _previous_metadata: Option<&BTreeMap<String, Value>>,
    _operation: Option<&str>,
) -> Result<(), String> {
    let snapshot = load_snapshot(repo_root)?;
    let shared_asset_id = resolve_shared_asset_id(&snapshot, path, shared_asset_id)?;
    let mut asset = load_asset_metadata_map(repo_root, &shared_asset_id)?;
    if let Some(title) = metadata.get("title").and_then(Value::as_str) {
        asset.insert("name".to_string(), Value::String(title.to_string()));
    }
    if let Some(comment) = metadata
        .get("comment")
        .or_else(|| metadata.get("note"))
        .and_then(Value::as_str)
    {
        asset.insert("annotation".to_string(), Value::String(comment.to_string()));
    }
    if let Some(link) = metadata.get("link").and_then(Value::as_str) {
        asset.insert("url".to_string(), Value::String(link.to_string()));
    }
    if let Some(tag_groups) = metadata.get("tagGroups") {
        asset.insert(
            "tags".to_string(),
            Value::Array(
                flatten_tags(tag_groups)
                    .into_iter()
                    .map(Value::String)
                    .collect(),
            ),
        );
    }
    if let Some(rating) = metadata.get("rating") {
        asset.insert("rating".to_string(), rating.clone());
    }
    let file_path = resolve_primary_file_path(repo_root, &snapshot, &shared_asset_id)?;
    touch_asset_metadata(&mut asset, &file_path)?;
    save_asset_metadata_map(repo_root, &shared_asset_id, &asset)
}

pub(super) fn write_repository_state(
    repo_root: &Path,
    payload: &PluginPayload,
) -> Result<(), String> {
    let mut metadata = load_metadata_map(repo_root)?;
    let directory_metadata_by_path = payload
        .directory_metadata_by_path
        .clone()
        .unwrap_or_default();
    let quick_access = payload.quick_access.clone().unwrap_or_default();
    let tag_groups = payload.tag_groups.clone().unwrap_or_default();
    let smart_folders = payload.smart_folders.clone().unwrap_or_default();
    let repository_actions = payload.repository_actions.clone().unwrap_or_default();
    apply_folder_metadata(
        ensure_array_mut(&mut metadata, "folders"),
        "",
        &directory_metadata_by_path,
    );
    metadata.insert(
        "quickAccess".to_string(),
        Value::Array(quick_access.into_iter().map(serialize_shortcut).collect()),
    );
    metadata.insert(
        "tagsGroups".to_string(),
        Value::Array(
            tag_groups
                .iter()
                .cloned()
                .into_iter()
                .map(serialize_tag_group)
                .collect(),
        ),
    );
    metadata.insert(
        "smartFolders".to_string(),
        Value::Array(
            smart_folders
                .iter()
                .cloned()
                .into_iter()
                .map(serialize_smart_folder)
                .collect(),
        ),
    );
    touch_library_metadata(&mut metadata);
    save_metadata_map(repo_root, &metadata)?;

    save_json_pretty(
        &repo_root.join(SAVED_FILTERS_FILE_NAME),
        &Value::Array(
            smart_folders
                .into_iter()
                .map(serialize_smart_folder)
                .collect(),
        ),
    )?;

    let mut tags_json = load_json_or_default(
        &repo_root.join(TAGS_FILE_NAME),
        serde_json::json!({
            "historyTags": [],
            "starredTags": [],
        }),
    )?;
    if let Some(tags_map) = tags_json.as_object_mut() {
        let starred = tag_groups
            .iter()
            .find(|group| group.name == "Starred Tags")
            .map(|group| group.tags.clone())
            .unwrap_or_default();
        tags_map.insert(
            "starredTags".to_string(),
            Value::Array(starred.into_iter().map(Value::String).collect()),
        );
    }
    save_json_pretty(&repo_root.join(TAGS_FILE_NAME), &tags_json)?;

    save_json_pretty(
        &repo_root.join(ACTIONS_FILE_NAME),
        &Value::Array(
            repository_actions
                .into_iter()
                .map(|action| set_raw_action_enabled(action.raw, action.enabled))
                .collect(),
        ),
    )
}
