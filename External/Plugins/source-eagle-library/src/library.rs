//! Eagle 元数据与 JSON 文件读写、资源归属解析、缩略图重命名与文件夹树维护。

use super::*;

pub(super) fn load_metadata_map(repo_root: &Path) -> Result<Map<String, Value>, String> {
    load_json_object(&repo_root.join(METADATA_FILE_NAME))
}

pub(super) fn save_metadata_map(
    repo_root: &Path,
    metadata: &Map<String, Value>,
) -> Result<(), String> {
    save_json_pretty(
        &repo_root.join(METADATA_FILE_NAME),
        &Value::Object(metadata.clone()),
    )
}

pub(super) fn load_asset_metadata_map(
    repo_root: &Path,
    shared_asset_id: &str,
) -> Result<Map<String, Value>, String> {
    load_json_object(&info_dir_for_asset(repo_root, shared_asset_id).join(METADATA_FILE_NAME))
}

pub(super) fn save_asset_metadata_map(
    repo_root: &Path,
    shared_asset_id: &str,
    metadata: &Map<String, Value>,
) -> Result<(), String> {
    save_json_pretty(
        &info_dir_for_asset(repo_root, shared_asset_id).join(METADATA_FILE_NAME),
        &Value::Object(metadata.clone()),
    )
}

pub(super) fn load_json_object(path: &Path) -> Result<Map<String, Value>, String> {
    let value = load_json_or_default(path, Value::Object(Map::new()))?;
    value
        .as_object()
        .cloned()
        .ok_or_else(|| format!("json root is not an object: {}", path.to_string_lossy()))
}

pub(super) fn load_json_or_default(path: &Path, default: Value) -> Result<Value, String> {
    if !path.exists() {
        return Ok(default);
    }
    let raw = fs::read_to_string(path).map_err(io_error)?;
    serde_json::from_str(&raw).map_err(|error| error.to_string())
}

pub(super) fn save_json_pretty(path: &Path, value: &Value) -> Result<(), String> {
    let json = serde_json::to_string_pretty(value).map_err(|error| error.to_string())?;
    fs::write(path, json).map_err(io_error)
}

pub(super) fn resolve_shared_asset_id(
    snapshot: &EagleSourceSnapshot,
    path: &str,
    shared_asset_id: Option<&str>,
) -> Result<String, String> {
    if let Some(shared_asset_id) = shared_asset_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Ok(shared_asset_id.to_string());
    }
    snapshot
        .files
        .iter()
        .find(|file| file.relative_path == normalize_relative_path(path))
        .and_then(|file| file.shared_asset_id.clone())
        .ok_or_else(|| format!("shared asset id not found for path: {path}"))
}

pub(super) fn resolve_primary_file_path(
    repo_root: &Path,
    snapshot: &EagleSourceSnapshot,
    shared_asset_id: &str,
) -> Result<PathBuf, String> {
    snapshot
        .files
        .iter()
        .find(|file| file.shared_asset_id.as_deref() == Some(shared_asset_id))
        .and_then(|file| file.local_absolute_path.as_deref().map(PathBuf::from))
        .or_else(|| {
            let info_dir = info_dir_for_asset(repo_root, shared_asset_id);
            fs::read_dir(info_dir)
                .ok()?
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .find(|path| {
                    path.is_file()
                        && path.file_name().and_then(|value| value.to_str())
                            != Some(METADATA_FILE_NAME)
                })
        })
        .ok_or_else(|| format!("asset file not found for shared asset: {shared_asset_id}"))
}

pub(super) fn file_absolute_path_from_entry(entry: &EagleSourceEntry) -> Result<PathBuf, String> {
    entry
        .local_absolute_path
        .as_deref()
        .map(PathBuf::from)
        .ok_or_else(|| format!("local file path is missing: {}", entry.path))
}

pub(super) fn shared_asset_paths(
    snapshot: &EagleSourceSnapshot,
    shared_asset_id: &str,
) -> Vec<String> {
    snapshot
        .files
        .iter()
        .filter(|file| file.shared_asset_id.as_deref() == Some(shared_asset_id))
        .map(|file| file.relative_path.clone())
        .collect()
}

pub(super) fn rewrite_asset_membership(
    snapshot: &EagleSourceSnapshot,
    root_metadata: &Map<String, Value>,
    asset: &mut Map<String, Value>,
    shared_asset_id: &str,
    source_path: &str,
    target_parent_path: &str,
    delete_membership: bool,
) -> Result<(), String> {
    let member_paths = shared_asset_paths(snapshot, shared_asset_id);
    let source_path = normalize_relative_path(source_path);
    let member_index = member_paths
        .iter()
        .position(|path| path == &source_path)
        .ok_or_else(|| format!("asset membership not found: {source_path}"))?;
    let mut folder_ids = asset_folder_ids(asset);
    if delete_membership {
        if member_paths.len() > 1 && member_index < folder_ids.len() {
            folder_ids.remove(member_index);
            asset.insert(
                "folders".to_string(),
                Value::Array(folder_ids.into_iter().map(Value::String).collect()),
            );
        } else {
            asset.insert("isDeleted".to_string(), Value::Bool(true));
        }
        return Ok(());
    }
    let target_folder_id = folder_id_for_path(root_metadata, target_parent_path)?;
    asset.insert("isDeleted".to_string(), Value::Bool(false));
    match (folder_ids.is_empty(), target_folder_id) {
        (true, Some(folder_id)) => folder_ids.push(folder_id),
        (false, Some(folder_id)) if member_index < folder_ids.len() => {
            folder_ids[member_index] = folder_id
        }
        (false, None) if member_index < folder_ids.len() => {
            folder_ids.remove(member_index);
        }
        (false, Some(folder_id)) => folder_ids.push(folder_id),
        _ => {}
    }
    asset.insert(
        "folders".to_string(),
        Value::Array(folder_ids.into_iter().map(Value::String).collect()),
    );
    Ok(())
}

pub(super) fn rename_thumbnail_with_file(
    info_dir: &Path,
    old_file_path: &Path,
    new_file_path: &Path,
) -> Result<(), String> {
    let old_stem = old_file_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    let new_stem = new_file_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    for entry in fs::read_dir(info_dir).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if !file_name.starts_with(&format!("{old_stem}_thumbnail")) {
            continue;
        }
        let suffix = &file_name[old_stem.len()..];
        fs::rename(&path, info_dir.join(format!("{new_stem}{suffix}"))).map_err(io_error)?;
        break;
    }
    Ok(())
}

pub(super) fn apply_folder_metadata(
    folders: &mut Vec<Value>,
    parent_path: &str,
    metadata_by_path: &BTreeMap<String, FolderMetadataPayload>,
) {
    for folder in folders {
        let Some(folder_map) = folder.as_object_mut() else {
            continue;
        };
        let name = folder_map
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let path = join_relative_path(parent_path, &name);
        if let Some(metadata) = metadata_by_path.get(&path) {
            if metadata.protected {
                folder_map.insert(
                    "passwordTips".to_string(),
                    Value::String(metadata.password_tip.clone().unwrap_or_default()),
                );
                folder_map
                    .entry("password".to_string())
                    .or_insert_with(|| Value::String("momobako-managed".to_string()));
            } else {
                folder_map.remove("password");
                folder_map.remove("passwordTip");
                folder_map.remove("passwordTips");
            }
        }
        if let Some(children) = folder_map.get_mut("children").and_then(Value::as_array_mut) {
            apply_folder_metadata(children, &path, metadata_by_path);
        }
    }
}

pub(super) fn insert_folder(
    metadata: &mut Map<String, Value>,
    parent_path: &str,
    folder: Value,
) -> Result<(), String> {
    if normalize_relative_path(parent_path).is_empty() {
        ensure_array_mut(metadata, "folders").push(folder);
        return Ok(());
    }
    let parent = find_folder_mut(metadata, parent_path)?
        .ok_or_else(|| format!("parent directory not found: {parent_path}"))?;
    ensure_array_mut(parent, "children").push(folder);
    Ok(())
}

pub(super) fn find_folder_mut<'a>(
    metadata: &'a mut Map<String, Value>,
    path: &str,
) -> Result<Option<&'a mut Map<String, Value>>, String> {
    let segments = split_relative_path(path);
    if segments.is_empty() {
        return Ok(None);
    }
    find_folder_in_array_mut(ensure_array_mut(metadata, "folders"), &segments)
}

pub(super) fn find_folder_in_array_mut<'a>(
    folders: &'a mut Vec<Value>,
    segments: &[String],
) -> Result<Option<&'a mut Map<String, Value>>, String> {
    let Some((first, rest)) = segments.split_first() else {
        return Ok(None);
    };
    let Some(index) = folders.iter().position(|folder| {
        folder
            .as_object()
            .and_then(|item| item.get("name"))
            .and_then(Value::as_str)
            == Some(first.as_str())
    }) else {
        return Ok(None);
    };
    let folder = folders
        .get_mut(index)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "folder entry is not an object".to_string())?;
    if rest.is_empty() {
        return Ok(Some(folder));
    }
    let children = ensure_array_mut(folder, "children");
    find_folder_in_array_mut(children, rest)
}

pub(super) fn remove_folder(
    metadata: &mut Map<String, Value>,
    path: &str,
) -> Result<Option<Value>, String> {
    let segments = split_relative_path(path);
    let Some((name, parent_segments)) = segments.split_last() else {
        return Ok(None);
    };
    if parent_segments.is_empty() {
        let folders = ensure_array_mut(metadata, "folders");
        let index = folders.iter().position(|folder| {
            folder
                .as_object()
                .and_then(|item| item.get("name"))
                .and_then(Value::as_str)
                == Some(name.as_str())
        });
        return Ok(index.map(|index| folders.remove(index)));
    }
    let parent = find_folder_in_array_mut(ensure_array_mut(metadata, "folders"), parent_segments)?
        .ok_or_else(|| format!("directory not found: {path}"))?;
    let children = ensure_array_mut(parent, "children");
    let index = children.iter().position(|folder| {
        folder
            .as_object()
            .and_then(|item| item.get("name"))
            .and_then(Value::as_str)
            == Some(name.as_str())
    });
    Ok(index.map(|index| children.remove(index)))
}

pub(super) fn folder_id_for_path(
    metadata: &Map<String, Value>,
    path: &str,
) -> Result<Option<String>, String> {
    let segments = split_relative_path(path);
    if segments.is_empty() {
        return Ok(None);
    }
    let mut folders = metadata
        .get("folders")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for (index, segment) in segments.iter().enumerate() {
        let Some(folder) = folders.iter().find(|folder| {
            folder
                .as_object()
                .and_then(|item| item.get("name"))
                .and_then(Value::as_str)
                == Some(segment.as_str())
        }) else {
            return Err(format!("directory not found: {path}"));
        };
        if index + 1 == segments.len() {
            return Ok(folder
                .as_object()
                .and_then(|item| item.get("id"))
                .and_then(Value::as_str)
                .map(str::to_string));
        }
        folders = folder
            .as_object()
            .and_then(|item| item.get("children"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
    }
    Ok(None)
}

pub(super) fn asset_folder_ids(asset: &Map<String, Value>) -> Vec<String> {
    asset
        .get("folders")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}
