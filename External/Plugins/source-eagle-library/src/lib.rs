//! Eagle Library source 插件。
//!
//! 该插件把 Eagle `.library` 直接暴露为 MomoBako 的 source 后端，
//! 读侧复用宿主的 Eagle 解析快照，写侧直接修改 Eagle 顶层 JSON 与素材 sidecar。

mod models;
#[cfg(test)]
mod tests;

use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use momobako_backend::{
    build_eagle_source_snapshot, EagleSourceEntry, EagleSourceEntryKind, EagleSourceSnapshot,
};
use momobako_mutsuki_plugin_sdk::{export_mutsuki_momobako_plugin, PluginCallEnvelope};
use serde_json::{Map, Value};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

use self::models::*;

mod library;
mod operations;
mod paths;

use library::*;
use operations::*;
use paths::*;

const METADATA_FILE_NAME: &str = "metadata.json";
const TAGS_FILE_NAME: &str = "tags.json";
const SAVED_FILTERS_FILE_NAME: &str = "saved-filters.json";
const ACTIONS_FILE_NAME: &str = "actions.json";

export_mutsuki_momobako_plugin!(
    "momobako.source.eagle-library",
    "0.1.0",
    protocols = [
        "filesystem.ensureAttachable",
        "filesystem.prepareRepositoryRoot",
        "filesystem.listFiles",
        "filesystem.listTree",
        "filesystem.listDirectory",
        "filesystem.listDirectoryPage",
        "filesystem.statEntry",
        "filesystem.createDirectory",
        "filesystem.createFile",
        "filesystem.renameEntry",
        "filesystem.moveEntry",
        "filesystem.deleteEntry",
        "filesystem.describeRepositoryState",
        "filesystem.writeAssetMetadata",
        "filesystem.writeRepositoryState",
    ],
    requires = [],
    permissions = ["filesystem:read", "filesystem:write"],
    handle_call
);

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
