//! Eagle Library source 插件。
//!
//! 该插件把 Eagle `.library` 直接暴露为 MomoBako 的 source 后端，
//! 读侧复用宿主的 Eagle 解析快照，写侧直接修改 Eagle 顶层 JSON 与素材 sidecar。

mod models;
#[cfg(test)]
mod tests;

use std::{
    collections::BTreeMap,
    ffi::{c_char, CString},
    fs::{self, OpenOptions},
    os::raw::c_char as raw_c_char,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use momobako_backend_plugin_sdk::{
    free_c_string, read_request, register_host_plugin_api, response_error, response_with_error_log,
    HostPluginCallFn, HostPluginFreeFn, PluginCallEnvelope,
};
use momobako_lib::{
    build_eagle_source_snapshot, EagleSourceEntry, EagleSourceEntryKind, EagleSourceSnapshot,
};
use serde_json::{Map, Value};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

use self::models::*;

const MANIFEST: &str = include_str!("../manifest.json");
const METADATA_FILE_NAME: &str = "metadata.json";
const TAGS_FILE_NAME: &str = "tags.json";
const SAVED_FILTERS_FILE_NAME: &str = "saved-filters.json";
const ACTIONS_FILE_NAME: &str = "actions.json";

#[no_mangle]
pub extern "C" fn momobako_plugin_manifest() -> *mut raw_c_char {
    CString::new(MANIFEST)
        .expect("manifest should not contain null bytes")
        .into_raw()
}

#[no_mangle]
pub extern "C" fn momobako_plugin_call(input: *const c_char) -> *mut raw_c_char {
    let request = match read_request(input) {
        Ok(request) => request,
        Err(error) => return response_error(error),
    };
    let method = request.method.clone();
    let runtime = request.runtime.clone();
    response_with_error_log(&runtime, &method, handle_call(request))
}

#[no_mangle]
pub extern "C" fn momobako_plugin_register_host_api(
    call: Option<HostPluginCallFn>,
    free: Option<HostPluginFreeFn>,
) {
    register_host_plugin_api(call, free);
}

#[no_mangle]
pub unsafe extern "C" fn momobako_plugin_free(value: *mut raw_c_char) {
    unsafe { free_c_string(value) };
}

fn handle_call(request: PluginCallEnvelope) -> Result<Value, String> {
    let payload: PluginPayload =
        serde_json::from_value(request.payload).map_err(|error| error.to_string())?;
    match request.method.as_str() {
        "filesystem.ensureAttachable" => {
            ensure_attachable(&payload.repo_root)?;
            Ok(serde_json::json!({}))
        }
        "filesystem.prepareRepositoryRoot" => {
            ensure_attachable(&payload.repo_root)?;
            Ok(serde_json::json!({}))
        }
        "filesystem.listFiles" => {
            let snapshot = load_snapshot(&payload.repo_root)?;
            serde_json::to_value(snapshot.files).map_err(|error| error.to_string())
        }
        "filesystem.listTree" => {
            let snapshot = load_snapshot(&payload.repo_root)?;
            serde_json::to_value(snapshot.tree).map_err(|error| error.to_string())
        }
        "filesystem.listDirectory" => {
            let snapshot = load_snapshot(&payload.repo_root)?;
            let directory_path =
                normalize_relative_path(payload.directory_path.as_deref().unwrap_or_default());
            let entries = snapshot
                .directories
                .get(&directory_path)
                .cloned()
                .ok_or_else(|| format!("directory not found: {directory_path}"))?;
            serde_json::to_value(entries).map_err(|error| error.to_string())
        }
        "filesystem.listDirectoryPage" => {
            let snapshot = load_snapshot(&payload.repo_root)?;
            let directory_path =
                normalize_relative_path(payload.directory_path.as_deref().unwrap_or_default());
            let entries = snapshot
                .directories
                .get(&directory_path)
                .cloned()
                .ok_or_else(|| format!("directory not found: {directory_path}"))?;
            let total_entries = entries.len();
            let offset = payload.offset.unwrap_or(0).min(total_entries);
            let limit = payload
                .limit
                .unwrap_or(total_entries.saturating_sub(offset));
            serde_json::to_value(DirectoryPageResult {
                entries: entries.into_iter().skip(offset).take(limit).collect(),
                total_entries,
            })
            .map_err(|error| error.to_string())
        }
        "filesystem.statEntry" => {
            let snapshot = load_snapshot(&payload.repo_root)?;
            let entry_path = payload.entry_path.as_deref().ok_or("missing entryPath")?;
            let entry = stat_entry(&snapshot, entry_path)?;
            serde_json::to_value(entry).map_err(|error| error.to_string())
        }
        "filesystem.createDirectory" => {
            let parent_path =
                normalize_relative_path(payload.parent_path.as_deref().unwrap_or_default());
            let name = payload.name.as_deref().ok_or("missing name")?;
            create_directory(&payload.repo_root, &parent_path, name)?;
            Ok(serde_json::json!({}))
        }
        "filesystem.createFile" => {
            let parent_path =
                normalize_relative_path(payload.parent_path.as_deref().unwrap_or_default());
            let name = payload.name.as_deref().ok_or("missing name")?;
            create_file(&payload.repo_root, &parent_path, name)?;
            Ok(serde_json::json!({}))
        }
        "filesystem.renameEntry" => {
            let source_path = payload.source_path.as_deref().ok_or("missing sourcePath")?;
            let new_name = payload.new_name.as_deref().ok_or("missing newName")?;
            rename_entry(&payload.repo_root, source_path, new_name)?;
            let snapshot = load_snapshot(&payload.repo_root)?;
            let target_path = join_relative_path(&parent_relative_path(source_path), new_name);
            serde_json::to_value(stat_entry(&snapshot, &target_path)?)
                .map_err(|error| error.to_string())
        }
        "filesystem.moveEntry" => {
            let source_path = payload.source_path.as_deref().ok_or("missing sourcePath")?;
            let target_parent_path = payload
                .target_parent_path
                .as_deref()
                .ok_or("missing targetParentPath")?;
            move_entry(&payload.repo_root, source_path, target_parent_path)?;
            let snapshot = load_snapshot(&payload.repo_root)?;
            let moved_name = Path::new(source_path)
                .file_name()
                .and_then(|value| value.to_str())
                .ok_or_else(|| format!("invalid source path: {source_path}"))?;
            let target_path = join_relative_path(target_parent_path, moved_name);
            serde_json::to_value(stat_entry(&snapshot, &target_path)?)
                .map_err(|error| error.to_string())
        }
        "filesystem.deleteEntry" => {
            let entry_path = payload.entry_path.as_deref().ok_or("missing entryPath")?;
            delete_entry(
                &payload.repo_root,
                entry_path,
                payload.recursive.unwrap_or(false),
            )?;
            Ok(serde_json::json!({}))
        }
        "filesystem.describeRepositoryState" => {
            let snapshot = load_snapshot(&payload.repo_root)?;
            serde_json::to_value(snapshot.repository_state).map_err(|error| error.to_string())
        }
        "filesystem.writeAssetMetadata" => {
            let path = payload.path.as_deref().ok_or("missing path")?;
            let metadata = payload.metadata.as_ref().ok_or("missing metadata")?;
            write_asset_metadata(
                &payload.repo_root,
                path,
                payload.shared_asset_id.as_deref(),
                metadata,
                payload.previous_metadata.as_ref(),
                payload.operation.as_deref(),
            )?;
            Ok(serde_json::json!({}))
        }
        "filesystem.writeRepositoryState" => {
            write_repository_state(&payload.repo_root, &payload)?;
            Ok(serde_json::json!({}))
        }
        method => Err(format!("unsupported method: {method}")),
    }
}

fn ensure_attachable(repo_root: &Path) -> Result<(), String> {
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

fn load_snapshot(repo_root: &Path) -> Result<EagleSourceSnapshot, String> {
    build_eagle_source_snapshot(repo_root)
}

fn stat_entry(
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

fn create_directory(repo_root: &Path, parent_path: &str, name: &str) -> Result<(), String> {
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

fn create_file(repo_root: &Path, parent_path: &str, name: &str) -> Result<(), String> {
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

fn rename_entry(repo_root: &Path, source_path: &str, new_name: &str) -> Result<(), String> {
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

fn move_entry(repo_root: &Path, source_path: &str, target_parent_path: &str) -> Result<(), String> {
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

fn delete_entry(repo_root: &Path, entry_path: &str, _recursive: bool) -> Result<(), String> {
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

fn write_asset_metadata(
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

fn write_repository_state(repo_root: &Path, payload: &PluginPayload) -> Result<(), String> {
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

fn load_metadata_map(repo_root: &Path) -> Result<Map<String, Value>, String> {
    load_json_object(&repo_root.join(METADATA_FILE_NAME))
}

fn save_metadata_map(repo_root: &Path, metadata: &Map<String, Value>) -> Result<(), String> {
    save_json_pretty(
        &repo_root.join(METADATA_FILE_NAME),
        &Value::Object(metadata.clone()),
    )
}

fn load_asset_metadata_map(
    repo_root: &Path,
    shared_asset_id: &str,
) -> Result<Map<String, Value>, String> {
    load_json_object(&info_dir_for_asset(repo_root, shared_asset_id).join(METADATA_FILE_NAME))
}

fn save_asset_metadata_map(
    repo_root: &Path,
    shared_asset_id: &str,
    metadata: &Map<String, Value>,
) -> Result<(), String> {
    save_json_pretty(
        &info_dir_for_asset(repo_root, shared_asset_id).join(METADATA_FILE_NAME),
        &Value::Object(metadata.clone()),
    )
}

fn load_json_object(path: &Path) -> Result<Map<String, Value>, String> {
    let value = load_json_or_default(path, Value::Object(Map::new()))?;
    value
        .as_object()
        .cloned()
        .ok_or_else(|| format!("json root is not an object: {}", path.to_string_lossy()))
}

fn load_json_or_default(path: &Path, default: Value) -> Result<Value, String> {
    if !path.exists() {
        return Ok(default);
    }
    let raw = fs::read_to_string(path).map_err(io_error)?;
    serde_json::from_str(&raw).map_err(|error| error.to_string())
}

fn save_json_pretty(path: &Path, value: &Value) -> Result<(), String> {
    let json = serde_json::to_string_pretty(value).map_err(|error| error.to_string())?;
    fs::write(path, json).map_err(io_error)
}

fn resolve_shared_asset_id(
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

fn resolve_primary_file_path(
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

fn file_absolute_path_from_entry(entry: &EagleSourceEntry) -> Result<PathBuf, String> {
    entry
        .local_absolute_path
        .as_deref()
        .map(PathBuf::from)
        .ok_or_else(|| format!("local file path is missing: {}", entry.path))
}

fn shared_asset_paths(snapshot: &EagleSourceSnapshot, shared_asset_id: &str) -> Vec<String> {
    snapshot
        .files
        .iter()
        .filter(|file| file.shared_asset_id.as_deref() == Some(shared_asset_id))
        .map(|file| file.relative_path.clone())
        .collect()
}

fn rewrite_asset_membership(
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

fn rename_thumbnail_with_file(
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

fn apply_folder_metadata(
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

fn insert_folder(
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

fn find_folder_mut<'a>(
    metadata: &'a mut Map<String, Value>,
    path: &str,
) -> Result<Option<&'a mut Map<String, Value>>, String> {
    let segments = split_relative_path(path);
    if segments.is_empty() {
        return Ok(None);
    }
    find_folder_in_array_mut(ensure_array_mut(metadata, "folders"), &segments)
}

fn find_folder_in_array_mut<'a>(
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

fn remove_folder(metadata: &mut Map<String, Value>, path: &str) -> Result<Option<Value>, String> {
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

fn folder_id_for_path(metadata: &Map<String, Value>, path: &str) -> Result<Option<String>, String> {
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

fn asset_folder_ids(asset: &Map<String, Value>) -> Vec<String> {
    asset
        .get("folders")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

fn split_display_name(name: &str) -> (String, String) {
    let path = Path::new(name);
    let display_name = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(name)
        .to_string();
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    (display_name, extension)
}

fn split_relative_path(path: &str) -> Vec<String> {
    normalize_relative_path(path)
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(str::to_string)
        .collect()
}

fn normalize_relative_path(path: &str) -> String {
    path.trim().replace('\\', "/").trim_matches('/').to_string()
}

fn parent_relative_path(path: &str) -> String {
    let normalized = normalize_relative_path(path);
    normalized
        .rfind('/')
        .map(|index| normalized[..index].to_string())
        .unwrap_or_default()
}

fn join_relative_path(parent: &str, name: &str) -> String {
    let parent = normalize_relative_path(parent);
    let name = normalize_relative_path(name);
    if parent.is_empty() {
        name
    } else {
        format!("{parent}/{name}")
    }
}

fn flatten_tags(value: &Value) -> Vec<String> {
    let mut tags = Vec::new();
    collect_tag_values(value, &mut tags);
    tags.sort();
    tags.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
    tags
}

fn collect_tag_values(value: &Value, tags: &mut Vec<String>) {
    match value {
        Value::String(tag) => {
            let tag = tag.trim();
            if !tag.is_empty() {
                tags.push(tag.to_string());
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_tag_values(item, tags);
            }
        }
        Value::Object(map) => {
            for key in ["tags", "items", "children", "value"] {
                if let Some(value) = map.get(key) {
                    collect_tag_values(value, tags);
                }
            }
        }
        _ => {}
    }
}

fn ensure_array_mut<'a>(map: &'a mut Map<String, Value>, key: &str) -> &'a mut Vec<Value> {
    if !matches!(map.get(key), Some(Value::Array(_))) {
        map.insert(key.to_string(), Value::Array(Vec::new()));
    }
    map.get_mut(key)
        .and_then(Value::as_array_mut)
        .expect("json array should exist")
}

fn info_dir_for_asset(repo_root: &Path, shared_asset_id: &str) -> PathBuf {
    repo_root
        .join("images")
        .join(format!("{shared_asset_id}.info"))
}

fn touch_library_metadata(metadata: &mut Map<String, Value>) {
    metadata.insert(
        "modificationTime".to_string(),
        serde_json::json!(now_unix_millis()),
    );
}

fn touch_asset_metadata(metadata: &mut Map<String, Value>, file_path: &Path) -> Result<(), String> {
    let stats = fs::metadata(file_path).map_err(io_error)?;
    metadata.insert(
        "size".to_string(),
        serde_json::json!(i64::try_from(stats.len()).unwrap_or(0)),
    );
    metadata.insert(
        "mtime".to_string(),
        serde_json::json!(stats
            .modified()
            .ok()
            .map(system_time_to_millis)
            .unwrap_or_else(now_unix_millis)),
    );
    metadata.insert(
        "modificationTime".to_string(),
        serde_json::json!(now_unix_millis()),
    );
    metadata.insert(
        "lastModified".to_string(),
        serde_json::json!(now_unix_millis()),
    );
    Ok(())
}

fn generate_eagle_id(prefix: &str) -> String {
    format!(
        "{}{:X}",
        prefix
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric())
            .flat_map(|ch| ch
                .to_ascii_uppercase()
                .to_string()
                .chars()
                .collect::<Vec<_>>())
            .collect::<String>(),
        now_unix_millis()
    )
}

fn now_unix_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or_default()
}

fn system_time_to_millis(value: SystemTime) -> i64 {
    value
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or_else(|_| now_unix_millis())
}

fn io_error(error: std::io::Error) -> String {
    error.to_string()
}

#[allow(dead_code)]
fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}
