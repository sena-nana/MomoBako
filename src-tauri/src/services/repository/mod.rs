//! Repository domain facade, DTOs, persistence helpers, and feature modules.

use rusqlite::{params, types::Type, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use sha1::{Digest as Sha1Digest, Sha1};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    ffi::{CStr, CString, OsString},
    fs::{self, File, OpenOptions},
    hash::{Hash, Hasher},
    io::{Read, Write},
    os::raw::c_char,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex, OnceLock},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

mod browser;
mod action;
mod external_assets;
mod playback;
mod management;
mod playlist;
mod plugin;
mod query;
mod smart_folder;
mod state;
pub(crate) mod test_support;

use plugin::{
    apply_plugin_settings, broken_plugin_manifest, embedded_local_filesystem_fallback_enabled,
    ensure_plugin_data_dir, ensure_repository_backend_runtime_available, is_source_plugin,
    load_native_plugin, load_plugin_config_values, load_plugin_settings,
    parse_plugin_manifest_with_source, plugin_data_dir, plugin_legacy_ids,
    read_plugin_manifest_from_archive, resolve_plugin_manifest_dependencies, runtime_plugins_dir,
};
#[cfg(test)]
use plugin::{is_repository_backend_plugin, parse_plugin_manifest};

pub(crate) use playback::download_playlist_with_progress;
pub use state::RepositoryState;

const REGISTRY_FILE_NAME: &str = "repositories.db";
const REPO_META_DIR: &str = ".momo";
const LEGACY_REPO_META_DIR: &str = ".meta";
const REPO_TRASH_DIR: &str = "trash";
const REPO_TRASH_MANIFEST_FILE_NAME: &str = "trash.json";
const REPO_METADATA_FILE_NAME: &str = "repository.json";
const REPO_DB_FILE_NAME: &str = "metadata.db";
const REPO_SCHEMA_VERSION: i64 = 1;
const THUMBNAIL_SIZE: u32 = 256;
const MAX_REMOTE_THUMBNAIL_BYTES: u64 = 10 * 1024 * 1024;
const PLUGIN_HOOK_EXECUTIONS_FILE_NAME: &str = "plugin-hook-executions.jsonl";

static FFMPEG_READY: OnceLock<Result<(), String>> = OnceLock::new();
const REGISTRY_SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS repositories (
  repo_id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  path TEXT NOT NULL UNIQUE,
  backend_plugin_id TEXT NOT NULL DEFAULT 'momobako.local-filesystem',
  backend_config_json TEXT NOT NULL DEFAULT '{}',
  status TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS schema_version (
  component TEXT PRIMARY KEY,
  version INTEGER NOT NULL
);

INSERT INTO schema_version(component, version)
VALUES ('registry', 1)
ON CONFLICT(component) DO UPDATE SET version = excluded.version;
"#;

const LOCAL_FILESYSTEM_PLUGIN_ID: &str = "momobako.local-filesystem";
const LEGACY_LOCAL_FILESYSTEM_PLUGIN_ID: &str = "builtin.local-filesystem";
const NETEASE_CLOUD_MUSIC_PLUGIN_ID: &str = "momobako.source.netease-cloud-music";
const PLUGIN_SDK_VERSION: &str = "1";
const MAX_PARALLEL_IMPORTS: usize = 4;

const REPOSITORY_SCHEMA_SQL: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS repositories (
  repo_id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  root_path TEXT NOT NULL UNIQUE,
  schema_version INTEGER NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS assets (
  asset_id TEXT PRIMARY KEY,
  repo_id TEXT NOT NULL,
  path TEXT NOT NULL,
  filename TEXT NOT NULL,
  extension TEXT NOT NULL,
  size_bytes INTEGER NOT NULL,
  created_at TEXT NOT NULL,
  modified_at TEXT NOT NULL,
  hash TEXT,
  status TEXT NOT NULL,
  version INTEGER NOT NULL DEFAULT 1,
  updated_at TEXT NOT NULL,
  thumbnail_path TEXT,
  is_virtual INTEGER NOT NULL DEFAULT 0,
  provider_id TEXT,
  provider_item_id TEXT,
  source_payload_json TEXT,
  local_absolute_path TEXT,
  FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_assets_repo_path ON assets(repo_id, path);
CREATE INDEX IF NOT EXISTS idx_assets_repo_filename ON assets(repo_id, filename);
CREATE INDEX IF NOT EXISTS idx_assets_repo_status ON assets(repo_id, status);
CREATE INDEX IF NOT EXISTS idx_assets_repo_hash ON assets(repo_id, hash);

CREATE TABLE IF NOT EXISTS hardlink_groups (
  group_id TEXT PRIMARY KEY,
  repo_id TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  size_bytes INTEGER NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_hardlink_groups_repo_hash_size
ON hardlink_groups(repo_id, content_hash, size_bytes);

CREATE TABLE IF NOT EXISTS hardlink_members (
  group_id TEXT NOT NULL,
  repo_id TEXT NOT NULL,
  asset_id TEXT NOT NULL,
  path TEXT NOT NULL,
  link_state TEXT NOT NULL,
  linked_at TEXT NOT NULL,
  verified_at TEXT NOT NULL,
  PRIMARY KEY(repo_id, asset_id),
  FOREIGN KEY(group_id) REFERENCES hardlink_groups(group_id),
  FOREIGN KEY(asset_id) REFERENCES assets(asset_id)
);

CREATE INDEX IF NOT EXISTS idx_hardlink_members_repo_path
ON hardlink_members(repo_id, path);

CREATE TABLE IF NOT EXISTS hardlink_candidates (
  candidate_id TEXT PRIMARY KEY,
  repo_id TEXT NOT NULL,
  new_asset_id TEXT NOT NULL,
  new_path TEXT NOT NULL,
  existing_asset_id TEXT NOT NULL,
  existing_path TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  size_bytes INTEGER NOT NULL,
  created_at TEXT NOT NULL,
  FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_hardlink_candidates_unique
ON hardlink_candidates(repo_id, new_asset_id, existing_asset_id);

CREATE TABLE IF NOT EXISTS asset_alias_groups (
  alias_group_id TEXT PRIMARY KEY,
  repo_id TEXT NOT NULL,
  source TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
);

CREATE TABLE IF NOT EXISTS asset_alias_members (
  alias_group_id TEXT NOT NULL,
  repo_id TEXT NOT NULL,
  asset_id TEXT NOT NULL,
  path TEXT NOT NULL,
  role TEXT NOT NULL,
  created_at TEXT NOT NULL,
  PRIMARY KEY(repo_id, asset_id),
  FOREIGN KEY(alias_group_id) REFERENCES asset_alias_groups(alias_group_id),
  FOREIGN KEY(asset_id) REFERENCES assets(asset_id)
);

CREATE INDEX IF NOT EXISTS idx_asset_alias_members_group
ON asset_alias_members(repo_id, alias_group_id, path);

CREATE TABLE IF NOT EXISTS repository_shortcuts (
  shortcut_id TEXT PRIMARY KEY,
  repo_id TEXT NOT NULL,
  label TEXT NOT NULL,
  target_kind TEXT NOT NULL,
  target_path TEXT,
  target_id TEXT,
  sort_order INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
);

CREATE INDEX IF NOT EXISTS idx_repository_shortcuts_repo_order
ON repository_shortcuts(repo_id, sort_order, label);

CREATE TABLE IF NOT EXISTS tag_groups (
  tag_group_id TEXT PRIMARY KEY,
  repo_id TEXT NOT NULL,
  name TEXT NOT NULL,
  sort_order INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
);

CREATE TABLE IF NOT EXISTS tag_group_members (
  tag_group_id TEXT NOT NULL,
  repo_id TEXT NOT NULL,
  tag TEXT NOT NULL,
  normalized_tag TEXT NOT NULL,
  sort_order INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY(tag_group_id, normalized_tag),
  FOREIGN KEY(tag_group_id) REFERENCES tag_groups(tag_group_id)
);

CREATE INDEX IF NOT EXISTS idx_tag_group_members_repo_tag
ON tag_group_members(repo_id, normalized_tag);

CREATE TABLE IF NOT EXISTS folder_metadata (
  repo_id TEXT NOT NULL,
  path TEXT NOT NULL,
  protected INTEGER NOT NULL DEFAULT 0,
  password_tip TEXT,
  updated_at TEXT NOT NULL,
  PRIMARY KEY(repo_id, path),
  FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
);

CREATE TABLE IF NOT EXISTS entry_thumbnails (
  repo_id TEXT NOT NULL,
  path TEXT NOT NULL,
  kind TEXT NOT NULL,
  thumbnail_path TEXT NOT NULL,
  custom INTEGER NOT NULL DEFAULT 0,
  updated_at TEXT NOT NULL,
  PRIMARY KEY(repo_id, path, kind)
);

CREATE TABLE IF NOT EXISTS smart_folders (
  smart_folder_id TEXT PRIMARY KEY,
  repo_id TEXT NOT NULL,
  parent_id TEXT,
  name TEXT NOT NULL,
  filter_json TEXT NOT NULL,
  sort_order INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(repo_id) REFERENCES repositories(repo_id),
  FOREIGN KEY(parent_id) REFERENCES smart_folders(smart_folder_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_smart_folders_repo_parent
ON smart_folders(repo_id, parent_id, sort_order, name);

CREATE TABLE IF NOT EXISTS repository_actions (
  action_id TEXT PRIMARY KEY,
  repo_id TEXT NOT NULL,
  source TEXT NOT NULL,
  source_action_id TEXT,
  name TEXT NOT NULL,
  status TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 1,
  raw_json TEXT NOT NULL,
  unsupported_reason TEXT,
  sort_order INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
);

CREATE INDEX IF NOT EXISTS idx_repository_actions_repo_order
ON repository_actions(repo_id, sort_order, name);

CREATE TABLE IF NOT EXISTS playlists (
  playlist_id TEXT PRIMARY KEY,
  repo_id TEXT NOT NULL,
  name TEXT NOT NULL,
  player_type_id TEXT NOT NULL,
  player_plugin_id TEXT NOT NULL,
  player_label TEXT NOT NULL,
  file_class TEXT NOT NULL,
  sort_order INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
);

CREATE INDEX IF NOT EXISTS idx_playlists_repo_order
ON playlists(repo_id, sort_order, updated_at DESC);

CREATE TABLE IF NOT EXISTS playlist_items (
  playlist_item_id TEXT PRIMARY KEY,
  repo_id TEXT NOT NULL,
  playlist_id TEXT NOT NULL,
  asset_id TEXT NOT NULL,
  sort_order INTEGER NOT NULL DEFAULT 0,
  added_at TEXT NOT NULL,
  FOREIGN KEY(repo_id) REFERENCES repositories(repo_id),
  FOREIGN KEY(playlist_id) REFERENCES playlists(playlist_id) ON DELETE CASCADE,
  FOREIGN KEY(asset_id) REFERENCES assets(asset_id)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_playlist_items_unique_asset
ON playlist_items(playlist_id, asset_id);

CREATE INDEX IF NOT EXISTS idx_playlist_items_repo_order
ON playlist_items(repo_id, playlist_id, sort_order, added_at);

CREATE TABLE IF NOT EXISTS repository_action_steps (
  step_id TEXT PRIMARY KEY,
  action_id TEXT NOT NULL,
  repo_id TEXT NOT NULL,
  step_kind TEXT NOT NULL,
  label TEXT NOT NULL,
  status TEXT NOT NULL,
  config_json TEXT NOT NULL,
  raw_json TEXT NOT NULL,
  unsupported_reason TEXT,
  sort_order INTEGER NOT NULL DEFAULT 0,
  FOREIGN KEY(action_id) REFERENCES repository_actions(action_id) ON DELETE CASCADE,
  FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
);

CREATE INDEX IF NOT EXISTS idx_repository_action_steps_action_order
ON repository_action_steps(action_id, sort_order);

CREATE TABLE IF NOT EXISTS repository_action_runs (
  run_id TEXT PRIMARY KEY,
  action_id TEXT NOT NULL,
  repo_id TEXT NOT NULL,
  status TEXT NOT NULL,
  target_json TEXT NOT NULL,
  message TEXT,
  started_at TEXT NOT NULL,
  finished_at TEXT,
  FOREIGN KEY(action_id) REFERENCES repository_actions(action_id),
  FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
);

CREATE INDEX IF NOT EXISTS idx_repository_action_runs_action_time
ON repository_action_runs(action_id, started_at DESC);

CREATE TABLE IF NOT EXISTS repository_action_run_steps (
  run_step_id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL,
  step_id TEXT NOT NULL,
  repo_id TEXT NOT NULL,
  status TEXT NOT NULL,
  message TEXT,
  started_at TEXT NOT NULL,
  finished_at TEXT,
  FOREIGN KEY(run_id) REFERENCES repository_action_runs(run_id) ON DELETE CASCADE,
  FOREIGN KEY(step_id) REFERENCES repository_action_steps(step_id),
  FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
);

CREATE TABLE IF NOT EXISTS metadata (
  asset_id TEXT NOT NULL,
  key TEXT NOT NULL,
  value_type TEXT NOT NULL,
  value_json TEXT NOT NULL,
  version INTEGER NOT NULL DEFAULT 1,
  updated_at TEXT NOT NULL,
  PRIMARY KEY(asset_id, key),
  FOREIGN KEY(asset_id) REFERENCES assets(asset_id)
);

CREATE INDEX IF NOT EXISTS idx_metadata_key ON metadata(key);

CREATE TABLE IF NOT EXISTS tags (
  asset_id TEXT NOT NULL,
  tag TEXT NOT NULL,
  normalized_tag TEXT NOT NULL,
  PRIMARY KEY(asset_id, normalized_tag),
  FOREIGN KEY(asset_id) REFERENCES assets(asset_id)
);

CREATE INDEX IF NOT EXISTS idx_tags_tag ON tags(normalized_tag);

CREATE TABLE IF NOT EXISTS revisions (
  revision_id TEXT PRIMARY KEY,
  repo_id TEXT NOT NULL,
  asset_id TEXT NOT NULL,
  timestamp TEXT NOT NULL,
  operation TEXT NOT NULL,
  before_json TEXT,
  after_json TEXT,
  source TEXT NOT NULL,
  FOREIGN KEY(repo_id) REFERENCES repositories(repo_id),
  FOREIGN KEY(asset_id) REFERENCES assets(asset_id)
);

CREATE INDEX IF NOT EXISTS idx_revisions_asset_time ON revisions(asset_id, timestamp DESC);

CREATE TABLE IF NOT EXISTS events (
  event_id TEXT PRIMARY KEY,
  repo_id TEXT NOT NULL,
  asset_id TEXT,
  event_type TEXT NOT NULL,
  path TEXT NOT NULL,
  payload_json TEXT,
  created_at TEXT NOT NULL,
  FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
);

CREATE INDEX IF NOT EXISTS idx_events_repo_time ON events(repo_id, created_at DESC);

CREATE TABLE IF NOT EXISTS schema_version (
  component TEXT PRIMARY KEY,
  version INTEGER NOT NULL
);

INSERT INTO schema_version(component, version)
VALUES ('repository', 1)
ON CONFLICT(component) DO UPDATE SET version = excluded.version;
"#;

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryBackendSummary {
    pub plugin_id: String,
    pub kind: String,
    pub name: String,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RepositorySummary {
    pub repo_id: String,
    pub name: String,
    pub path: String,
    pub backend: RepositoryBackendSummary,
    pub status: String,
    pub asset_count: i64,
    pub updated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_cache: Option<RepositoryLocalCacheStatus>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryLocalCacheStatus {
    pub required: bool,
    pub path: Option<String>,
    pub status: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AssetSummary {
    pub asset_id: String,
    pub repo_id: String,
    pub path: String,
    pub filename: String,
    pub extension: String,
    pub size_bytes: i64,
    pub size_label: String,
    pub status: String,
    pub modified_at: String,
    pub version: i64,
    pub tags: Vec<String>,
    pub thumbnail_path: Option<String>,
    pub hardlink_group_id: Option<String>,
    pub hardlink_state: Option<String>,
    pub is_virtual: bool,
    pub provider_id: Option<String>,
    pub provider_item_id: Option<String>,
    pub source_payload: Option<serde_json::Value>,
    pub local_absolute_path: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FolderSummary {
    pub path: String,
    pub label: String,
    pub asset_count: i64,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryShortcut {
    pub shortcut_id: String,
    pub label: String,
    pub target_kind: String,
    pub target_path: Option<String>,
    pub target_id: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryTagGroup {
    pub tag_group_id: String,
    pub name: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistPlayerContribution {
    pub player_type_id: String,
    pub label: String,
    pub file_class: String,
    pub supported_extensions: Vec<String>,
    pub supports_seek: bool,
    pub supports_volume: bool,
    pub supports_preview_navigation: bool,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistSummary {
    pub playlist_id: String,
    pub repo_id: String,
    pub name: String,
    pub player_type_id: String,
    pub player_plugin_id: String,
    pub player_label: String,
    pub file_class: String,
    pub item_count: i64,
    pub sort_order: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistItem {
    pub playlist_item_id: String,
    pub playlist_id: String,
    pub asset_id: String,
    pub path: String,
    pub filename: String,
    pub extension: String,
    pub thumbnail_path: Option<String>,
    pub status: String,
    pub status_reason: Option<String>,
    pub sort_order: i64,
    pub added_at: String,
    pub is_virtual: bool,
    pub provider_id: Option<String>,
    pub provider_item_id: Option<String>,
    pub source_payload: Option<serde_json::Value>,
    pub local_absolute_path: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistDetail {
    pub playlist: PlaylistSummary,
    pub items: Vec<PlaylistItem>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FolderMetadata {
    pub protected: bool,
    pub password_tip: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MetadataEntry {
    pub key: String,
    pub value_type: String,
    pub value: serde_json::Value,
    pub version: i64,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RevisionEntry {
    pub revision_id: String,
    pub asset_id: String,
    pub timestamp: String,
    pub operation: String,
    pub before: serde_json::Value,
    pub after: serde_json::Value,
    pub source: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AssetDetail {
    pub summary: AssetSummary,
    pub metadata: Vec<MetadataEntry>,
    pub revisions: Vec<RevisionEntry>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub repo_id: String,
    pub repo_name: String,
    pub asset_id: String,
    pub path: String,
    pub filename: String,
    pub status: String,
    pub tags: Vec<String>,
    pub metadata: BTreeMap<String, serde_json::Value>,
    pub is_virtual: bool,
    pub provider_id: Option<String>,
    pub provider_item_id: Option<String>,
    pub source_payload: Option<serde_json::Value>,
    pub local_absolute_path: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SearchResponse {
    pub query: String,
    pub results: Vec<SearchHit>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RepositorySnapshot {
    pub repository: RepositorySummary,
    pub folder_label: String,
    pub folders: Vec<FolderSummary>,
    pub assets: Vec<AssetSummary>,
    pub playlists: Vec<PlaylistSummary>,
    pub quick_access: Vec<RepositoryShortcut>,
    pub tag_groups: Vec<RepositoryTagGroup>,
    pub metadata_fields: Vec<String>,
    pub recent_revision_count: i64,
    pub overview: RepositoryOverview,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryOverview {
    pub total_size_bytes: i64,
    pub total_size_label: String,
    pub file_count: i64,
    pub folder_count: i64,
    pub readme_content: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryActionStep {
    pub step_id: String,
    pub action_id: String,
    pub repo_id: String,
    pub step_kind: String,
    pub label: String,
    pub status: String,
    pub config: serde_json::Value,
    pub raw: serde_json::Value,
    pub unsupported_reason: Option<String>,
    pub sort_order: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryAction {
    pub action_id: String,
    pub repo_id: String,
    pub source: String,
    pub source_action_id: Option<String>,
    pub name: String,
    pub status: String,
    pub enabled: bool,
    pub raw: serde_json::Value,
    pub unsupported_reason: Option<String>,
    pub sort_order: i64,
    pub created_at: String,
    pub updated_at: String,
    pub steps: Vec<RepositoryActionStep>,
    pub last_run: Option<RepositoryActionRun>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryActionRun {
    pub run_id: String,
    pub action_id: String,
    pub repo_id: String,
    pub status: String,
    pub target: serde_json::Value,
    pub message: Option<String>,
    pub started_at: String,
    pub finished_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryActionRunRequest {
    pub repo_id: String,
    pub action_id: String,
    pub target_paths: Option<Vec<String>>,
    pub asset_ids: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryActionRunResponse {
    pub action: RepositoryAction,
    pub run: RepositoryActionRun,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryActionEnabledRequest {
    pub repo_id: String,
    pub action_id: String,
    pub enabled: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryActionMutationResponse {
    pub action: RepositoryAction,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistMutationRequest {
    pub repo_id: String,
    pub playlist_id: Option<String>,
    pub name: String,
    pub player_type_id: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistUpdateRequest {
    pub repo_id: String,
    pub playlist_id: String,
    pub name: Option<String>,
    pub player_type_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistMutationResponse {
    pub playlists: Vec<PlaylistSummary>,
    pub playlist: Option<PlaylistSummary>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistItemsAddRequest {
    pub repo_id: String,
    pub playlist_id: String,
    pub asset_ids: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistItemsByPathsAddRequest {
    pub repo_id: String,
    pub playlist_id: String,
    pub paths: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistItemsOrderRequest {
    pub repo_id: String,
    pub playlist_id: String,
    pub item_ids: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistItemRemoveRequest {
    pub repo_id: String,
    pub playlist_id: String,
    pub playlist_item_id: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistMembershipRequest {
    pub repo_id: String,
    pub asset_id: String,
    pub playlist_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistMembershipSnapshot {
    pub asset_id: String,
    pub playlist_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistMembershipIndex {
    pub memberships: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FileTreeNode {
    pub path: String,
    pub label: String,
    pub children: Vec<FileTreeNode>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FileBrowserEntry {
    pub path: String,
    pub name: String,
    pub kind: String,
    pub extension: Option<String>,
    pub size_bytes: Option<i64>,
    pub size_label: Option<String>,
    pub modified_at: Option<String>,
    pub asset_id: Option<String>,
    pub status: Option<String>,
    pub thumbnail_path: Option<String>,
    pub thumbnail_custom: bool,
    pub hardlink_group_id: Option<String>,
    pub hardlink_state: Option<String>,
    pub tags: Vec<String>,
    pub alias_paths: Vec<String>,
    pub folder_metadata: Option<FolderMetadata>,
    pub metadata: BTreeMap<String, serde_json::Value>,
    pub is_virtual: bool,
    pub provider_id: Option<String>,
    pub provider_item_id: Option<String>,
    pub source_payload: Option<serde_json::Value>,
    pub local_absolute_path: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FileBrowserSnapshot {
    pub repo_id: String,
    pub root_path: String,
    pub backend_plugin_id: String,
    pub backend_kind: String,
    pub current_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub special_location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tree: Option<Vec<FileTreeNode>>,
    pub entries: Vec<FileBrowserEntry>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct EntryPlaybackRequest {
    pub repo_id: String,
    pub path: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct EntryPlaybackSourceResponse {
    pub repo_id: String,
    pub path: String,
    pub media_type: String,
    pub source_url: Option<String>,
    pub local_path: Option<String>,
    pub temp_file_path: Option<String>,
    pub lyric_path: Option<String>,
    pub lyric_source_url: Option<String>,
    pub word_lyric_path: Option<String>,
    pub word_lyric_source_url: Option<String>,
    pub expires_at: Option<String>,
    pub size_bytes: Option<i64>,
    pub modified_at: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct EntryPlaybackProgressEvent {
    pub phase: String,
    pub repo_id: String,
    pub path: String,
    pub value: u8,
    pub detail: String,
    pub indeterminate: bool,
    pub cached: Option<bool>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RepositoryMetadataFile {
    repo_id: String,
    name: String,
    root_path: String,
    backend_plugin_id: String,
    backend_config: serde_json::Value,
    created_at: String,
    schema_version: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RepositoryMetadataFileImport {
    repo_id: String,
    name: Option<String>,
    root_path: Option<String>,
    backend_plugin_id: Option<String>,
    backend_config: Option<serde_json::Value>,
    created_at: Option<String>,
    schema_version: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataUpdateRequest {
    pub repo_id: String,
    pub asset_id: String,
    pub expected_version: i64,
    pub metadata: BTreeMap<String, serde_json::Value>,
    pub source: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataUpdateResponse {
    pub outcome: String,
    pub asset: AssetDetail,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryMutationRequest {
    pub repo_id: Option<String>,
    pub name: String,
    pub path: String,
    pub backend_plugin_id: Option<String>,
    pub backend_config: Option<serde_json::Value>,
    #[serde(default)]
    pub skip_initial_sync: bool,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryFolderRequest {
    pub path: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryRelocateRequest {
    pub repo_id: String,
    pub path: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryBackendConfigUpdateRequest {
    pub repo_id: String,
    pub backend_config: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NeteaseRepositoryCacheConfigureRequest {
    pub repo_id: String,
    pub path: String,
    #[serde(default)]
    pub migrate_legacy_cache: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NeteaseRepositoryCacheMigrationSummary {
    pub moved_state_files: usize,
    pub migrated_playback_cache_files: usize,
    pub skipped_playback_cache_files: usize,
    pub failed_playback_cache_files: usize,
}

impl NeteaseRepositoryCacheMigrationSummary {
    fn empty() -> Self {
        Self {
            moved_state_files: 0,
            migrated_playback_cache_files: 0,
            skipped_playback_cache_files: 0,
            failed_playback_cache_files: 0,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NeteaseRepositoryCacheConfigureResponse {
    pub repository: RepositorySummary,
    pub migration: NeteaseRepositoryCacheMigrationSummary,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryArchiveExportOptions {
    pub format: String,
    pub output_path: String,
    pub compression: String,
    pub encrypt: bool,
    pub password: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryGitExportOptions {
    pub remote: Option<String>,
    pub branch: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryExportRequest {
    pub repo_id: String,
    pub target: String,
    pub archive: Option<RepositoryArchiveExportOptions>,
    pub git: Option<RepositoryGitExportOptions>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryExportResponse {
    pub repository: RepositorySummary,
    pub target: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encrypted: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileBrowserRequest {
    pub repo_id: String,
    pub directory_path: Option<String>,
    pub include_tree: Option<bool>,
    pub special_location: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileReadRequest {
    pub repo_id: String,
    pub path: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginCallRequest {
    pub plugin_id: String,
    pub method: String,
    #[serde(default)]
    pub payload: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DownloaderDestinationRequest {
    pub kind: String,
    pub path: Option<String>,
    pub repo_id: Option<String>,
    pub parent_path: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DownloaderPlaylistTrackRequest {
    pub song_id: i64,
    #[serde(default)]
    pub song_name: Option<String>,
    #[serde(default)]
    pub source_payload: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DownloaderPlaylistRequest {
    pub playlist_id: i64,
    pub playlist_name: Option<String>,
    pub tracks: Vec<DownloaderPlaylistTrackRequest>,
    pub destination: DownloaderDestinationRequest,
    #[serde(default)]
    pub managed_cache_root: Option<String>,
    #[serde(default)]
    pub source_payload: Option<serde_json::Value>,
    #[serde(default)]
    pub level: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DownloaderPlaylistProgressEvent {
    pub phase: String,
    pub playlist_id: i64,
    pub playlist_name: Option<String>,
    pub total: usize,
    pub completed: usize,
    pub failed: usize,
    pub current_song_id: Option<i64>,
    pub current_song_name: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginCallResult {
    pub plugin_id: String,
    pub method: String,
    pub payload: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<PluginCallRuntime>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PluginCallRuntime {
    pub degraded: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub degradation_reason: Option<String>,
    pub dependency_status: PluginDependencyStatus,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PluginHookExecutionRecord {
    pub execution_id: String,
    pub plugin_id: String,
    pub hook_slot: String,
    pub hook_action: String,
    pub hook_label: Option<String>,
    pub status: String,
    pub message: String,
    pub target: serde_json::Value,
    pub started_at: String,
    pub finished_at: String,
    pub runtime: Option<PluginCallRuntime>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct PluginHookExecutionListRequest {
    pub plugin_id: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PluginHookExecutionListResponse {
    pub records: Vec<PluginHookExecutionRecord>,
}

#[derive(Debug)]
struct PluginRuntimeCallResult {
    plugin_id: String,
    payload: serde_json::Value,
    runtime: Option<PluginCallRuntime>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginArchiveReadRequest {
    pub plugin_id: String,
    pub path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginArchiveTextResponse {
    pub plugin_id: String,
    pub path: String,
    pub text: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PluginDataDirectoryResponse {
    pub plugin_id: String,
    pub path: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginDataFilePreviewSourceRequest {
    pub plugin_id: String,
    pub path: String,
    pub media_type: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginDataFilePreviewSourceResponse {
    pub plugin_id: String,
    pub path: String,
    pub token: String,
    pub source_url: Option<String>,
    pub media_type: String,
    pub size_bytes: i64,
    pub modified_at: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PluginConfigSnapshot {
    pub plugin_id: String,
    pub data_directory: String,
    pub schema: serde_json::Value,
    pub values: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginConfigSetRequest {
    pub plugin_id: String,
    pub key: String,
    pub value: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginConfigDeleteRequest {
    pub plugin_id: String,
    pub key: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinaryFileWriteRequest {
    pub path: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BinaryFileWriteResponse {
    pub path: String,
    pub size_bytes: i64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilePreviewSourceResponse {
    pub repo_id: String,
    pub path: String,
    pub token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_path: Option<String>,
    pub media_type: String,
    pub size_bytes: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileCreateRequest {
    pub repo_id: String,
    pub parent_path: Option<String>,
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileImportRequest {
    pub repo_id: String,
    pub parent_path: Option<String>,
    pub source_paths: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct ExternalAddAssetClient {
    pub id: Option<String>,
    pub name: Option<String>,
    pub version: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ExternalAddAssetItem {
    pub kind: String,
    pub url: Option<String>,
    pub filename: Option<String>,
    pub headers: Option<BTreeMap<String, String>>,
    pub metadata: Option<BTreeMap<String, serde_json::Value>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ExternalAddAssetRequest {
    pub repo_id: String,
    pub parent_path: Option<String>,
    pub client: Option<ExternalAddAssetClient>,
    pub items: Vec<ExternalAddAssetItem>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ExternalImportedAsset {
    pub item_index: usize,
    pub asset_id: Option<String>,
    pub path: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ExternalAddAssetFailure {
    pub item_index: usize,
    pub code: String,
    pub message: String,
    pub retryable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ExternalAddAssetSummary {
    pub total: usize,
    pub imported: usize,
    pub failed: usize,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ExternalAddAssetResponse {
    pub request_id: String,
    pub status: String,
    pub imported: Vec<ExternalImportedAsset>,
    pub failed: Vec<ExternalAddAssetFailure>,
    pub summary: ExternalAddAssetSummary,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileCopyRequest {
    pub repo_id: String,
    pub source_paths: Vec<String>,
    pub parent_path: Option<String>,
    pub mode: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct HardlinkCandidate {
    pub candidate_id: String,
    pub repo_id: String,
    pub new_asset_id: String,
    pub new_path: String,
    pub existing_asset_id: String,
    pub existing_path: String,
    pub content_hash: String,
    pub size_bytes: i64,
    pub size_label: String,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HardlinkCandidateResponse {
    pub repo_id: String,
    pub candidates: Vec<HardlinkCandidate>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HardlinkConfirmRequest {
    pub repo_id: String,
    pub candidate_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HardlinkConfirmResponse {
    pub repo_id: String,
    pub candidate: HardlinkCandidate,
    pub state: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileRenameRequest {
    pub repo_id: String,
    pub path: String,
    pub new_name: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileMoveRequest {
    pub repo_id: String,
    pub source_paths: Vec<String>,
    pub parent_path: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDeleteRequest {
    pub repo_id: String,
    pub path: String,
    pub mode: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrashMutationRequest {
    pub repo_id: String,
    pub action: String,
    pub path: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct TrashManifest {
    entries: Vec<TrashManifestEntry>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct TrashManifestEntry {
    original_path: String,
    trash_path: String,
    deleted_at: String,
    kind: String,
}

#[derive(Debug)]
struct ThumbnailRecord {
    path: String,
    custom: bool,
}

#[derive(Debug, Clone)]
struct AssetPathRecord {
    asset_id: String,
    status: String,
    thumbnail_path: Option<String>,
    hardlink_group_id: Option<String>,
    hardlink_state: Option<String>,
    is_virtual: bool,
    provider_id: Option<String>,
    provider_item_id: Option<String>,
    source_payload: Option<serde_json::Value>,
    local_absolute_path: Option<String>,
}

#[derive(Debug, Clone)]
struct ExistingAssetRecord {
    asset_id: String,
    status: String,
    thumbnail_path: Option<String>,
    size_bytes: i64,
    created_at: String,
    modified_at: String,
    hash: Option<String>,
    is_virtual: bool,
    provider_id: Option<String>,
    provider_item_id: Option<String>,
    source_payload: Option<serde_json::Value>,
    local_absolute_path: Option<String>,
}

#[derive(Debug, Clone)]
struct HardlinkCopyOutcome {
    source_path: Option<String>,
    target_path: String,
    link_state: String,
}

#[derive(Debug, Clone)]
struct HardlinkAssetRecord {
    asset_id: String,
    content_hash: String,
    size_bytes: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryMutationResponse {
    pub repository: RepositorySummary,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncRequest {
    pub repo_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThumbnailRequest {
    pub repo_id: String,
    pub path: String,
    pub action: Option<String>,
    pub source_path: Option<String>,
    pub source_url: Option<String>,
    pub image_bytes: Option<Vec<u8>>,
    pub media_type: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThumbnailResponse {
    pub repo_id: String,
    pub path: String,
    pub asset_id: String,
    pub kind: String,
    pub thumbnail_path: Option<String>,
    pub thumbnail_custom: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<BTreeMap<String, serde_json::Value>>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SyncResult {
    pub repo_id: String,
    pub scanned_files: i64,
    pub created_assets: i64,
    pub updated_assets: i64,
    pub deleted_assets: i64,
    pub created_events: i64,
    pub hardlink_candidates: i64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionActionRequest {
    pub repo_id: String,
    pub asset_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionActionResponse {
    pub outcome: String,
    pub asset: AssetDetail,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SearchMetadataFilter {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SearchNumberFilter {
    pub key: String,
    pub min: Option<f64>,
    pub max: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SearchDateFilter {
    pub key: String,
    pub from: Option<String>,
    pub to: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SearchSort {
    pub field: String,
    pub direction: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchRequest {
    pub query: String,
    pub repo_id: Option<String>,
    pub exclude_query: Option<String>,
    pub metadata_key: Option<String>,
    pub metadata_value: Option<String>,
    pub tag: Option<String>,
    pub tags: Option<Vec<String>>,
    pub metadata_filters: Option<Vec<SearchMetadataFilter>>,
    pub exclude_tags: Option<Vec<String>>,
    pub exclude_formats: Option<Vec<String>>,
    pub exclude_metadata_filters: Option<Vec<SearchMetadataFilter>>,
    pub exclude_path_prefixes: Option<Vec<String>>,
    pub exclude_number_filters: Option<Vec<SearchNumberFilter>>,
    pub exclude_date_filters: Option<Vec<SearchDateFilter>>,
    pub number_filters: Option<Vec<SearchNumberFilter>>,
    pub date_filters: Option<Vec<SearchDateFilter>>,
    pub formats: Option<Vec<String>>,
    pub min_rating: Option<f64>,
    pub match_mode: Option<String>,
    pub sort: Option<SearchSort>,
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct SmartFolderFilter {
    pub query: Option<String>,
    pub path_prefix: Option<String>,
    pub exclude_query: Option<String>,
    pub exclude_path_prefixes: Option<Vec<String>>,
    pub tags: Option<Vec<String>>,
    pub formats: Option<Vec<String>>,
    pub colors: Option<Vec<String>>,
    pub shapes: Option<Vec<String>>,
    pub metadata_filters: Option<Vec<SearchMetadataFilter>>,
    pub exclude_tags: Option<Vec<String>>,
    pub exclude_formats: Option<Vec<String>>,
    pub exclude_metadata_filters: Option<Vec<SearchMetadataFilter>>,
    pub exclude_number_filters: Option<Vec<SearchNumberFilter>>,
    pub exclude_date_filters: Option<Vec<SearchDateFilter>>,
    pub number_filters: Option<Vec<SearchNumberFilter>>,
    pub date_filters: Option<Vec<SearchDateFilter>>,
    pub min_rating: Option<f64>,
    pub match_mode: Option<String>,
    pub sort: Option<SearchSort>,
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SmartFolder {
    pub smart_folder_id: String,
    pub repo_id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub filter: SmartFolderFilter,
    pub sort_order: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SmartFolderTreeNode {
    #[serde(flatten)]
    pub folder: SmartFolder,
    pub children: Vec<SmartFolderTreeNode>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SmartFolderResultSnapshot {
    pub repo_id: String,
    pub smart_folder: SmartFolder,
    pub inherited_filter: SmartFolderFilter,
    pub results: Vec<FileBrowserEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SmartFolderMutationRequest {
    pub repo_id: String,
    pub smart_folder_id: Option<String>,
    pub parent_id: Option<String>,
    pub name: String,
    pub filter: SmartFolderFilter,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SmartFolderUpdateRequest {
    pub repo_id: String,
    pub smart_folder_id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub filter: SmartFolderFilter,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SmartFolderMutationResponse {
    pub smart_folders: Vec<SmartFolderTreeNode>,
    pub smart_folder: Option<SmartFolder>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CacheConfig {
    pub metadata_capacity: usize,
    pub thumbnail_capacity: usize,
    pub query_capacity: usize,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CacheEntry {
    pub cache_type: String,
    pub key: String,
    pub last_accessed_at: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CacheSnapshot {
    pub config: CacheConfig,
    pub entries: Vec<CacheEntry>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PluginTypeDefinition {
    pub layer: String,
    pub kind: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct PluginCompat {
    pub sdk_version: String,
    #[serde(default)]
    pub legacy_plugin_ids: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PluginManifest {
    pub plugin_id: String,
    #[serde(default)]
    pub legacy_plugin_ids: Vec<String>,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub r#type: Option<PluginTypeDefinition>,
    pub kind: String,
    #[serde(default)]
    pub category: String,
    pub description: String,
    pub capabilities: Vec<String>,
    pub enabled: bool,
    pub sdk: String,
    pub entry: serde_json::Value,
    #[serde(default)]
    pub contributes: serde_json::Value,
    pub source: String,
    pub runtime: String,
    pub permissions: Vec<String>,
    #[serde(default)]
    pub requires: Vec<String>,
    #[serde(default)]
    pub optional: Vec<String>,
    #[serde(default)]
    pub hooks: Vec<PluginHook>,
    pub compat: PluginCompat,
    pub status: String,
    #[serde(default)]
    pub dependency_status: PluginDependencyStatus,
    #[serde(default)]
    pub disable_reason: Option<String>,
    #[serde(default)]
    pub degraded: bool,
    #[serde(default)]
    pub degradation_reason: Option<String>,
    #[serde(default)]
    pub archive_path: Option<String>,
}

#[derive(Debug, Clone)]
struct PlaylistPlayerRegistration {
    plugin_id: String,
    player_type_id: String,
    label: String,
    file_class: String,
    supported_extensions: Vec<String>,
    supports_seek: bool,
    supports_volume: bool,
    supports_preview_navigation: bool,
    description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct PluginDependencyStatus {
    pub required: Vec<PluginDependencyState>,
    pub optional: Vec<PluginDependencyState>,
    pub missing_required: Vec<String>,
    pub missing_optional: Vec<String>,
    pub disabled_required: Vec<String>,
    pub disabled_optional: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PluginDependencyState {
    pub plugin_id: String,
    #[serde(default)]
    pub name: Option<String>,
    pub status: String,
    pub enabled: bool,
    pub available: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PluginHook {
    pub slot: String,
    pub action: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub requires: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PluginEnabledRequest {
    pub plugin_id: String,
    pub enabled: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PluginInstallRequest {
    pub package_path: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PluginMutationResponse {
    pub plugins: Vec<PluginManifest>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginCallEnvelope {
    method: String,
    payload: serde_json::Value,
    runtime: PluginCallHostRuntime,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginCallHostRuntime {
    plugin_id: String,
    plugin_data_dir: String,
    plugin_config: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginCallResponse {
    ok: bool,
    payload: Option<serde_json::Value>,
    error: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ApiDefinition {
    pub group: String,
    pub transport: String,
    pub method: String,
    pub path: String,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin_method: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requires_auth: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_template: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ApiDesignSnapshot {
    pub transport: String,
    pub endpoints: Vec<ApiDefinition>,
}

#[derive(Debug)]
struct RepositorySeed<'a> {
    repo_id: &'a str,
    name: &'a str,
    root_path: &'a str,
    status: &'a str,
    assets: &'a [AssetSeed<'a>],
}

#[derive(Debug)]
struct AssetSeed<'a> {
    asset_id: &'a str,
    path: &'a str,
    filename: &'a str,
    extension: &'a str,
    size_bytes: i64,
    modified_at: &'a str,
    status: &'a str,
    tags: &'a [&'a str],
    metadata: &'a [(&'a str, &'a str, &'a str)],
}

#[derive(Debug, Clone)]
struct RepositoryBackendRecord {
    plugin_id: String,
    config: serde_json::Value,
}

#[derive(Debug, Clone)]
struct RepositoryRecord {
    summary: RepositorySummary,
    backend_record: RepositoryBackendRecord,
}

#[derive(Debug, Clone)]
struct RepositoryStoragePaths {
    metadata_dir: PathBuf,
    database_path: PathBuf,
}

#[derive(Debug, Clone)]
struct PreviewFileSource {
    path: PathBuf,
    media_type: String,
}

trait FileSystemBackendAdapter {
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum FileSystemEntryKind {
    Directory,
    File,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileSystemEntry {
    path: String,
    name: String,
    kind: FileSystemEntryKind,
    extension: Option<String>,
    size_bytes: Option<i64>,
    modified_at: Option<String>,
    #[serde(default)]
    is_virtual: bool,
    #[serde(default)]
    provider_id: Option<String>,
    #[serde(default)]
    provider_item_id: Option<String>,
    #[serde(default)]
    source_payload: Option<serde_json::Value>,
    #[serde(default)]
    local_absolute_path: Option<String>,
}

struct RuntimeFileSystemBackendAdapter {
    service_root: PathBuf,
    plugin_id: String,
}

struct LocalFileSystemBackend;

impl RepositoryState {

    pub fn create_repository(
        &self,
        request: RepositoryMutationRequest,
    ) -> Result<RepositoryMutationResponse, String> {
        management::create_repository(self, request)
    }

    pub fn import_repository(
        &self,
        request: RepositoryMutationRequest,
    ) -> Result<RepositoryMutationResponse, String> {
        management::import_repository(self, request)
    }

    pub fn attach_repository_folder(
        &self,
        request: RepositoryFolderRequest,
    ) -> Result<RepositoryMutationResponse, String> {
        management::attach_repository_folder(self, request)
    }

    pub fn delete_repository(&self, repo_id: &str) -> Result<(), String> {
        management::delete_repository(self, repo_id)
    }

    pub fn relocate_repository(
        &self,
        request: RepositoryRelocateRequest,
    ) -> Result<RepositoryMutationResponse, String> {
        management::relocate_repository(self, request)
    }

    pub fn update_repository_backend_config(
        &self,
        request: RepositoryBackendConfigUpdateRequest,
    ) -> Result<RepositoryMutationResponse, String> {
        management::update_repository_backend_config(self, request)
    }

    pub fn configure_netease_repository_cache(
        &self,
        request: NeteaseRepositoryCacheConfigureRequest,
    ) -> Result<NeteaseRepositoryCacheConfigureResponse, String> {
        management::configure_netease_repository_cache(self, request)
    }

    pub fn export_repository(
        &self,
        request: RepositoryExportRequest,
    ) -> Result<RepositoryExportResponse, String> {
        management::export_repository(self, request)
    }

    pub fn load_snapshot(&self, repo_id: &str) -> Result<RepositorySnapshot, String> {
        query::load_snapshot(self, repo_id)
    }

    pub fn load_asset_detail(&self, repo_id: &str, asset_id: &str) -> Result<AssetDetail, String> {
        query::load_asset_detail(self, repo_id, asset_id)
    }

    pub fn list_playlists(&self, repo_id: &str) -> Result<Vec<PlaylistSummary>, String> {
        playlist::list_playlists(self, repo_id)
    }

    pub fn list_playlist_memberships(
        &self,
        repo_id: &str,
    ) -> Result<PlaylistMembershipIndex, String> {
        playlist::list_playlist_memberships(self, repo_id)
    }

    pub fn create_playlist(
        &self,
        request: PlaylistMutationRequest,
    ) -> Result<PlaylistMutationResponse, String> {
        playlist::create_playlist(self, request)
    }

    pub fn update_playlist(
        &self,
        request: PlaylistUpdateRequest,
    ) -> Result<PlaylistMutationResponse, String> {
        playlist::update_playlist(self, request)
    }

    pub fn delete_playlist(
        &self,
        repo_id: &str,
        playlist_id: &str,
    ) -> Result<PlaylistMutationResponse, String> {
        playlist::delete_playlist(self, repo_id, playlist_id)
    }

    pub fn get_playlist_detail(
        &self,
        repo_id: &str,
        playlist_id: &str,
    ) -> Result<PlaylistDetail, String> {
        playlist::get_playlist_detail(self, repo_id, playlist_id)
    }

    pub fn add_playlist_items(
        &self,
        request: PlaylistItemsAddRequest,
    ) -> Result<PlaylistDetail, String> {
        playlist::add_playlist_items(self, request)
    }

    pub fn add_playlist_items_by_paths(
        &self,
        request: PlaylistItemsByPathsAddRequest,
    ) -> Result<PlaylistDetail, String> {
        playlist::add_playlist_items_by_paths(self, request)
    }

    pub fn reorder_playlist_items(
        &self,
        request: PlaylistItemsOrderRequest,
    ) -> Result<PlaylistDetail, String> {
        playlist::reorder_playlist_items(self, request)
    }

    pub fn remove_playlist_item(
        &self,
        request: PlaylistItemRemoveRequest,
    ) -> Result<PlaylistDetail, String> {
        playlist::remove_playlist_item(self, request)
    }

    pub fn set_playlist_membership(
        &self,
        request: PlaylistMembershipRequest,
    ) -> Result<PlaylistMembershipSnapshot, String> {
        playlist::set_playlist_membership(self, request)
    }

    pub fn load_file_browser(
        &self,
        request: FileBrowserRequest,
    ) -> Result<FileBrowserSnapshot, String> {
        browser::load_file_browser(self, request)
    }

    pub fn read_file(&self, request: FileReadRequest) -> Result<Vec<u8>, String> {
        query::read_file(self, request)
    }

    pub fn prepare_preview_file_source(
        &self,
        request: FileReadRequest,
    ) -> Result<FilePreviewSourceResponse, String> {
        query::prepare_preview_file_source(self, request)
    }

    pub fn prepare_entry_playback_source(
        &self,
        request: EntryPlaybackRequest,
    ) -> Result<EntryPlaybackSourceResponse, String> {
        query::prepare_entry_playback_source(self, request)
    }

    pub fn prepare_entry_playback_source_with_progress(
        &self,
        request: EntryPlaybackRequest,
        emit: &mut dyn FnMut(EntryPlaybackProgressEvent) -> Result<(), String>,
    ) -> Result<EntryPlaybackSourceResponse, String> {
        query::prepare_entry_playback_source_with_progress(self, request, emit)
    }

    pub fn call_plugin(&self, request: PluginCallRequest) -> Result<PluginCallResult, String> {
        plugin::call_plugin(self, request)
    }

    pub fn read_plugin_archive_text(
        &self,
        request: PluginArchiveReadRequest,
    ) -> Result<PluginArchiveTextResponse, String> {
        plugin::read_plugin_archive_text(self, request)
    }

    pub fn get_plugin_data_directory(
        &self,
        plugin_id: String,
    ) -> Result<PluginDataDirectoryResponse, String> {
        plugin::get_plugin_data_directory(self, plugin_id)
    }

    pub fn prepare_plugin_data_file_preview_source(
        &self,
        request: PluginDataFilePreviewSourceRequest,
    ) -> Result<PluginDataFilePreviewSourceResponse, String> {
        plugin::prepare_plugin_data_file_preview_source(self, request)
    }

    pub fn get_plugin_config(&self, plugin_id: String) -> Result<PluginConfigSnapshot, String> {
        plugin::get_plugin_config(self, plugin_id)
    }

    pub fn set_plugin_config_value(
        &self,
        request: PluginConfigSetRequest,
    ) -> Result<PluginConfigSnapshot, String> {
        plugin::set_plugin_config_value(self, request)
    }

    pub fn delete_plugin_config_value(
        &self,
        request: PluginConfigDeleteRequest,
    ) -> Result<PluginConfigSnapshot, String> {
        plugin::delete_plugin_config_value(self, request)
    }

    fn load_plugin_config_values(
        &self,
        plugin_id: &str,
    ) -> Result<(PluginManifest, PathBuf, BTreeMap<String, serde_json::Value>), String> {
        let registry = plugin_management_registry(&self.root);
        let normalized_plugin_id = registry.normalize_plugin_id(plugin_id);
        let registration = registry
            .registration(&normalized_plugin_id)
            .ok_or_else(|| format!("plugin not found: {plugin_id}"))?;
        let manifest = registration.manifest.clone();
        let data_dir = ensure_plugin_data_dir(&self.root, &manifest.plugin_id)?;
        let values = load_plugin_config_values(&data_dir)?;
        Ok((manifest, data_dir, values))
    }

    pub fn list_smart_folders(&self, repo_id: &str) -> Result<Vec<SmartFolderTreeNode>, String> {
        smart_folder::list_smart_folders(self, repo_id)
    }

    pub fn create_smart_folder(
        &self,
        request: SmartFolderMutationRequest,
    ) -> Result<SmartFolderMutationResponse, String> {
        smart_folder::create_smart_folder(self, request)
    }

    pub fn update_smart_folder(
        &self,
        request: SmartFolderUpdateRequest,
    ) -> Result<SmartFolderMutationResponse, String> {
        smart_folder::update_smart_folder(self, request)
    }

    pub fn delete_smart_folder(
        &self,
        repo_id: &str,
        smart_folder_id: &str,
    ) -> Result<SmartFolderMutationResponse, String> {
        smart_folder::delete_smart_folder(self, repo_id, smart_folder_id)
    }

    pub fn query_smart_folder(
        &self,
        repo_id: &str,
        smart_folder_id: &str,
    ) -> Result<SmartFolderResultSnapshot, String> {
        smart_folder::query_smart_folder(self, repo_id, smart_folder_id)
    }

    pub fn list_repository_actions(&self, repo_id: &str) -> Result<Vec<RepositoryAction>, String> {
        action::list_repository_actions(self, repo_id)
    }

    pub fn get_repository_action(
        &self,
        repo_id: &str,
        action_id: &str,
    ) -> Result<RepositoryAction, String> {
        action::get_repository_action(self, repo_id, action_id)
    }

    pub fn set_repository_action_enabled(
        &self,
        request: RepositoryActionEnabledRequest,
    ) -> Result<RepositoryActionMutationResponse, String> {
        action::set_repository_action_enabled(self, request)
    }

    pub fn run_repository_action(
        &self,
        request: RepositoryActionRunRequest,
    ) -> Result<RepositoryActionRunResponse, String> {
        action::run_repository_action(self, request)
    }

    pub fn search_assets(&self, request: SearchRequest) -> Result<SearchResponse, String> {
        query::search_assets(self, request)
    }

    pub fn list_hardlink_candidates(
        &self,
        repo_id: &str,
    ) -> Result<HardlinkCandidateResponse, String> {
        self.ensure_initialized()?;
        let repo = self.load_repository_record(repo_id)?;
        let connection = self.open_repository_connection(
            &repo.summary.repo_id,
            &repo.summary.path,
            &repo.backend_record,
        )?;
        let candidates = load_hardlink_candidates(&connection, repo_id).map_err(db_error)?;
        Ok(HardlinkCandidateResponse {
            repo_id: repo_id.to_string(),
            candidates,
        })
    }

    pub fn confirm_hardlink_candidate(
        &self,
        request: HardlinkConfirmRequest,
    ) -> Result<HardlinkConfirmResponse, String> {
        self.ensure_initialized()?;
        let repo = self.load_repository_record(&request.repo_id)?;
        if repo.backend_record.plugin_id != LOCAL_FILESYSTEM_PLUGIN_ID {
            return Err(
                "hardlink confirmation is only supported for local filesystem repositories"
                    .to_string(),
            );
        }
        let repo_root = PathBuf::from(&repo.summary.path);
        let mut connection = self.open_repository_connection(
            &repo.summary.repo_id,
            &repo.summary.path,
            &repo.backend_record,
        )?;
        let tx = connection.transaction().map_err(db_error)?;
        let candidate =
            load_hardlink_candidate_from_transaction(&tx, &request.repo_id, &request.candidate_id)
                .map_err(db_error)?
                .ok_or_else(|| format!("hardlink candidate not found: {}", request.candidate_id))?;

        let existing_abs = resolve_repository_relative_path(&repo_root, &candidate.existing_path)?;
        let new_abs = resolve_repository_relative_path(&repo_root, &candidate.new_path)?;
        let existing_file_current = current_file_matches_content(
            &existing_abs,
            &candidate.content_hash,
            candidate.size_bytes,
        )?;
        let new_file_current =
            current_file_matches_content(&new_abs, &candidate.content_hash, candidate.size_bytes)?;
        if !existing_file_current || !new_file_current {
            delete_hardlink_candidate(&tx, &request.repo_id, &request.candidate_id)
                .map_err(db_error)?;
            tx.commit().map_err(db_error)?;
            return Err("hardlink candidate is no longer valid".to_string());
        }

        replace_file_with_hardlink(&repo_root, &existing_abs, &new_abs)?;
        upsert_hardlink_member(
            &tx,
            &request.repo_id,
            &candidate.existing_asset_id,
            &candidate.existing_path,
            &candidate.content_hash,
            candidate.size_bytes,
            "linked",
        )
        .map_err(db_error)?;
        upsert_hardlink_member(
            &tx,
            &request.repo_id,
            &candidate.new_asset_id,
            &candidate.new_path,
            &candidate.content_hash,
            candidate.size_bytes,
            "linked",
        )
        .map_err(db_error)?;
        delete_hardlink_candidate(&tx, &request.repo_id, &request.candidate_id)
            .map_err(db_error)?;
        tx.commit().map_err(db_error)?;

        Ok(HardlinkConfirmResponse {
            repo_id: request.repo_id,
            candidate,
            state: "linked".to_string(),
        })
    }

    pub fn update_asset_metadata(
        &self,
        request: MetadataUpdateRequest,
    ) -> Result<MetadataUpdateResponse, String> {
        query::update_asset_metadata(self, request)
    }

    pub fn sync_repository(&self, request: SyncRequest) -> Result<SyncResult, String> {
        management::sync_repository(self, request)
    }

    fn sync_repository_with_candidate_skips(
        &self,
        repo_id: &str,
        skip_hardlink_candidate_paths: &HashSet<String>,
    ) -> Result<SyncResult, String> {
        management::sync_repository_with_candidate_skips(
            self,
            repo_id,
            skip_hardlink_candidate_paths,
        )
    }

    pub fn ensure_thumbnail(&self, request: ThumbnailRequest) -> Result<ThumbnailResponse, String> {
        self.ensure_initialized()?;
        let repo = self.load_repository_record(&request.repo_id)?;
        let entry_path = normalize_entry_path(&request.path)?;
        let repo_root = PathBuf::from(&repo.summary.path);
        let entry = stat_backend_entry(&self.root, &repo, &repo_root, &entry_path)?;
        let connection = self.open_repository_connection(
            &repo.summary.repo_id,
            &repo.summary.path,
            &repo.backend_record,
        )?;
        let thumbnail_root = self.repository_thumbnail_root(&repo)?;
        let action = request.action.as_deref().unwrap_or("ensure");
        let kind = match entry.kind {
            FileSystemEntryKind::Directory => "directory",
            FileSystemEntryKind::File => "file",
        };

        if kind == "directory" {
            let response = match action {
                "ensure" => {
                    let record = load_entry_thumbnail_record(
                        &connection,
                        &request.repo_id,
                        &entry_path,
                        kind,
                    )
                    .map_err(db_error)?;
                    normalize_entry_thumbnail_record(
                        &connection,
                        &repo,
                        &thumbnail_root,
                        &entry_path,
                        kind,
                        record,
                    )?
                    .map(|record| (Some(record.path), record.custom))
                    .unwrap_or((None, false))
                }
                "save" => {
                    let bytes = thumbnail_bytes_from_request(&request)?;
                    let thumbnail_path = save_custom_thumbnail_bytes(
                        &thumbnail_root,
                        &repo,
                        &entry_path,
                        kind,
                        &bytes,
                    )?;
                    upsert_entry_thumbnail_record(
                        &connection,
                        &request.repo_id,
                        &entry_path,
                        kind,
                        &thumbnail_path,
                        true,
                    )
                    .map_err(db_error)?;
                    (Some(thumbnail_path), true)
                }
                "saveGenerated" => {
                    let bytes = thumbnail_bytes_from_request(&request)?;
                    let thumbnail_path = save_thumbnail_bytes(
                        &thumbnail_root,
                        &repo,
                        &entry_path,
                        kind,
                        "generated",
                        &bytes,
                    )?;
                    upsert_entry_thumbnail_record(
                        &connection,
                        &request.repo_id,
                        &entry_path,
                        kind,
                        &thumbnail_path,
                        false,
                    )
                    .map_err(db_error)?;
                    (Some(thumbnail_path), false)
                }
                "clear" => {
                    remove_entry_thumbnail_record(&connection, &request.repo_id, &entry_path, kind)
                        .map_err(db_error)?;
                    (None, false)
                }
                "refresh" => {
                    remove_entry_thumbnail_record(&connection, &request.repo_id, &entry_path, kind)
                        .map_err(db_error)?;
                    (None, false)
                }
                value => return Err(format!("unsupported thumbnail action: {value}")),
            };

            return Ok(ThumbnailResponse {
                repo_id: request.repo_id,
                path: entry_path,
                asset_id: String::new(),
                kind: kind.to_string(),
                thumbnail_path: response.0,
                thumbnail_custom: response.1,
                metadata: None,
            });
        }

        let asset = connection
            .query_row(
                r#"
                SELECT asset_id, filename, extension, size_bytes, modified_at, thumbnail_path
                FROM assets
                WHERE repo_id = ?1 AND path = ?2 AND status != 'deleted'
                "#,
                params![&request.repo_id, &entry_path],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, Option<String>>(5)?,
                    ))
                },
            )
            .optional()
            .map_err(db_error)?
            .ok_or_else(|| format!("asset not found: {}", request.path))?;

        let (asset_id, filename, extension, size_bytes, modified_at, existing_thumbnail_path) =
            asset;
        let existing_thumbnail_path = normalize_asset_thumbnail_path(
            &connection,
            &repo,
            &thumbnail_root,
            &asset_id,
            &entry_path,
            existing_thumbnail_path,
        )?;
        let file = DiscoveredFile {
            absolute_path: Some(resolve_repository_relative_path(&repo_root, &entry_path)?),
            relative_path: entry_path.clone(),
            filename,
            extension,
            size_bytes,
            created_at: None,
            modified_at,
            is_virtual: false,
            provider_id: None,
            provider_item_id: None,
            source_payload: None,
            local_absolute_path: None,
        };
        let existing_record =
            load_entry_thumbnail_record(&connection, &request.repo_id, &entry_path, kind)
                .map_err(db_error)?;
        let custom_record = normalize_entry_thumbnail_record(
            &connection,
            &repo,
            &thumbnail_root,
            &entry_path,
            kind,
            existing_record,
        )?
        .filter(|record| record.custom);
        let (thumbnail_path, thumbnail_custom) = match action {
            "ensure" => {
                if let Some(record) = custom_record {
                    (Some(record.path), true)
                } else {
                    (
                        ensure_thumbnail_for_file(
                            &repo,
                            &repo_root,
                            &thumbnail_root,
                            &file,
                            existing_thumbnail_path,
                            false,
                        )?,
                        false,
                    )
                }
            }
            "refresh" => (
                ensure_thumbnail_for_file(
                    &repo,
                    &repo_root,
                    &thumbnail_root,
                    &file,
                    existing_thumbnail_path,
                    true,
                )?,
                false,
            ),
            "save" => {
                let bytes = thumbnail_bytes_from_request(&request)?;
                let thumbnail_path =
                    save_custom_thumbnail_bytes(&thumbnail_root, &repo, &entry_path, kind, &bytes)?;
                upsert_entry_thumbnail_record(
                    &connection,
                    &request.repo_id,
                    &entry_path,
                    kind,
                    &thumbnail_path,
                    true,
                )
                .map_err(db_error)?;
                (Some(thumbnail_path), true)
            }
            "saveGenerated" => {
                let bytes = thumbnail_bytes_from_request(&request)?;
                let thumbnail_path = save_thumbnail_bytes(
                    &thumbnail_root,
                    &repo,
                    &entry_path,
                    kind,
                    "generated",
                    &bytes,
                )?;
                remove_entry_thumbnail_record(&connection, &request.repo_id, &entry_path, kind)
                    .map_err(db_error)?;
                (Some(thumbnail_path), false)
            }
            "clear" => {
                remove_entry_thumbnail_record(&connection, &request.repo_id, &entry_path, kind)
                    .map_err(db_error)?;
                (existing_thumbnail_path, false)
            }
            value => return Err(format!("unsupported thumbnail action: {value}")),
        };

        if thumbnail_custom {
            update_asset_thumbnail_path(&connection, &request.repo_id, &asset_id, None)
                .map_err(db_error)?;
        } else {
            update_asset_thumbnail_path(
                &connection,
                &request.repo_id,
                &asset_id,
                thumbnail_path.as_deref(),
            )
            .map_err(db_error)?;
        }
        sync_thumbnail_palette_metadata(&connection, &asset_id, thumbnail_path.as_deref())
            .map_err(db_error)?;
        let metadata = load_metadata_map(&connection, &asset_id).map_err(db_error)?;

        Ok(ThumbnailResponse {
            repo_id: request.repo_id,
            path: entry_path,
            asset_id,
            kind: kind.to_string(),
            thumbnail_path,
            thumbnail_custom,
            metadata: Some(metadata),
        })
    }

    pub fn create_directory(
        &self,
        request: FileCreateRequest,
    ) -> Result<FileBrowserSnapshot, String> {
        browser::create_directory(self, request)
    }

    pub fn create_file(&self, request: FileCreateRequest) -> Result<FileBrowserSnapshot, String> {
        browser::create_file(self, request)
    }

    pub fn import_entries(
        &self,
        request: FileImportRequest,
    ) -> Result<FileBrowserSnapshot, String> {
        browser::import_entries(self, request)
    }

    pub fn add_external_assets(
        &self,
        request_id: String,
        request: ExternalAddAssetRequest,
    ) -> ExternalAddAssetResponse {
        external_assets::add_external_assets(self, request_id, request)
    }

    pub fn copy_entries(&self, request: FileCopyRequest) -> Result<FileBrowserSnapshot, String> {
        browser::copy_entries(self, request)
    }

    pub fn move_entries(&self, request: FileMoveRequest) -> Result<FileBrowserSnapshot, String> {
        browser::move_entries(self, request)
    }

    fn finish_file_copy_operation(
        &self,
        repo_id: &str,
        parent_path: String,
        include_tree: bool,
        outcomes: Vec<HardlinkCopyOutcome>,
    ) -> Result<FileBrowserSnapshot, String> {
        let skip_candidate_paths = hardlink_outcome_target_paths(&outcomes);
        self.sync_repository_with_candidate_skips(repo_id, &skip_candidate_paths)?;
        self.record_hardlink_copy_outcomes(repo_id, outcomes)?;

        self.load_file_browser(FileBrowserRequest {
            repo_id: repo_id.to_string(),
            directory_path: Some(parent_path),
            include_tree: Some(include_tree),
            special_location: None,
        })
    }

    fn record_hardlink_copy_outcomes(
        &self,
        repo_id: &str,
        outcomes: Vec<HardlinkCopyOutcome>,
    ) -> Result<(), String> {
        if outcomes.is_empty() {
            return Ok(());
        }
        let repo = self.load_repository_record(repo_id)?;
        let mut connection = self.open_repository_connection(
            &repo.summary.repo_id,
            &repo.summary.path,
            &repo.backend_record,
        )?;
        let tx = connection.transaction().map_err(db_error)?;
        for outcome in outcomes {
            let Some(target_asset) =
                load_hardlink_asset_for_path(&tx, repo_id, &outcome.target_path)
                    .map_err(db_error)?
            else {
                continue;
            };
            upsert_hardlink_member(
                &tx,
                repo_id,
                &target_asset.asset_id,
                &outcome.target_path,
                &target_asset.content_hash,
                target_asset.size_bytes,
                &outcome.link_state,
            )
            .map_err(db_error)?;

            if outcome.link_state == "linked" {
                let Some(source_path) = outcome.source_path.as_deref() else {
                    continue;
                };
                let Some(source_asset) =
                    load_hardlink_asset_for_path(&tx, repo_id, source_path).map_err(db_error)?
                else {
                    continue;
                };
                if source_asset.content_hash == target_asset.content_hash
                    && source_asset.size_bytes == target_asset.size_bytes
                {
                    upsert_hardlink_member(
                        &tx,
                        repo_id,
                        &source_asset.asset_id,
                        source_path,
                        &target_asset.content_hash,
                        target_asset.size_bytes,
                        "linked",
                    )
                    .map_err(db_error)?;
                }
            }
        }
        tx.commit().map_err(db_error)
    }

    pub fn rename_entry(&self, request: FileRenameRequest) -> Result<FileBrowserSnapshot, String> {
        browser::rename_entry(self, request)
    }

    pub fn delete_entry(&self, request: FileDeleteRequest) -> Result<FileBrowserSnapshot, String> {
        browser::delete_entry(self, request)
    }

    pub fn mutate_trash(
        &self,
        request: TrashMutationRequest,
    ) -> Result<FileBrowserSnapshot, String> {
        browser::mutate_trash(self, request)
    }

    pub fn undo_last_revision(
        &self,
        request: RevisionActionRequest,
    ) -> Result<RevisionActionResponse, String> {
        self.ensure_initialized()?;
        let repo = self.load_repository_record(&request.repo_id)?;
        let mut connection = self.open_repository_connection(
            &repo.summary.repo_id,
            &repo.summary.path,
            &repo.backend_record,
        )?;
        let tx = connection.transaction().map_err(db_error)?;

        let revision = load_latest_revision(&tx, &request.asset_id)
            .map_err(db_error)?
            .ok_or_else(|| format!("no revision found for asset: {}", request.asset_id))?;
        apply_revision_state(
            &tx,
            &request.repo_id,
            &request.asset_id,
            &revision.before,
            "revision.undo",
            "undo",
        )
        .map_err(db_error)?;
        tx.commit().map_err(db_error)?;

        let asset = self.load_asset_detail(&request.repo_id, &request.asset_id)?;
        Ok(RevisionActionResponse {
            outcome: "success".to_string(),
            asset,
        })
    }

    pub fn redo_last_revision(
        &self,
        request: RevisionActionRequest,
    ) -> Result<RevisionActionResponse, String> {
        self.ensure_initialized()?;
        let repo = self.load_repository_record(&request.repo_id)?;
        let mut connection = self.open_repository_connection(
            &repo.summary.repo_id,
            &repo.summary.path,
            &repo.backend_record,
        )?;
        let tx = connection.transaction().map_err(db_error)?;

        let revision = load_latest_revision(&tx, &request.asset_id)
            .map_err(db_error)?
            .ok_or_else(|| format!("no revision found for asset: {}", request.asset_id))?;
        apply_revision_state(
            &tx,
            &request.repo_id,
            &request.asset_id,
            &revision.after,
            "revision.redo",
            "redo",
        )
        .map_err(db_error)?;
        tx.commit().map_err(db_error)?;

        let asset = self.load_asset_detail(&request.repo_id, &request.asset_id)?;
        Ok(RevisionActionResponse {
            outcome: "success".to_string(),
            asset,
        })
    }

    pub fn list_plugins(&self) -> Result<Vec<PluginManifest>, String> {
        plugin::list_plugins(self)
    }

    pub fn list_plugin_hook_executions(
        &self,
        request: PluginHookExecutionListRequest,
    ) -> Result<PluginHookExecutionListResponse, String> {
        plugin::list_plugin_hook_executions(self, request)
    }

    pub fn set_plugin_enabled(
        &self,
        request: PluginEnabledRequest,
    ) -> Result<PluginMutationResponse, String> {
        plugin::set_plugin_enabled(self, request)
    }

    pub fn delete_plugin(&self, plugin_id: String) -> Result<PluginMutationResponse, String> {
        plugin::delete_plugin(self, plugin_id)
    }

    pub fn install_plugin_from_archive(
        &self,
        request: PluginInstallRequest,
    ) -> Result<PluginMutationResponse, String> {
        plugin::install_plugin_from_archive(self, request)
    }

    pub fn get_cache_snapshot(&self) -> Result<CacheSnapshot, String> {
        plugin::get_cache_snapshot(self)
    }

    pub fn get_api_design_snapshot(&self) -> Result<ApiDesignSnapshot, String> {
        plugin::get_api_design_snapshot(self)
    }

    fn load_repository_record(&self, repo_id: &str) -> Result<RepositoryRecord, String> {
        let registry = Connection::open(&self.registry_path).map_err(db_error)?;
        let plugin_registry = backend_plugin_registry(&self.root);
        registry
            .query_row(
                r#"
                SELECT repo_id, name, path, backend_plugin_id, backend_config_json, status, updated_at
                FROM repositories
                WHERE repo_id = ?1
                "#,
                [repo_id],
                |row| {
                    let backend_plugin_id: String = row.get(3)?;
                    let backend_plugin_id = plugin_registry.normalize_plugin_id(&backend_plugin_id);
                    let path: String = row.get(2)?;
                    let stored_status: String = row.get(5)?;
                    let status = repository_runtime_status(
                        &path,
                        &backend_plugin_id,
                        stored_status.as_str(),
                    );
                    let backend_config_json: String = row.get(4)?;
                    let backend_config = parse_backend_config_json(&backend_config_json).map_err(to_from_sql_error)?;
                    Ok(RepositoryRecord {
                        summary: RepositorySummary {
                            repo_id: row.get(0)?,
                            name: row.get(1)?,
                            path: path.clone(),
                            backend: backend_summary_from_registry(&plugin_registry, &backend_plugin_id),
                            status,
                            asset_count: 0,
                            updated_at: row.get(6)?,
                            local_cache: repository_local_cache_status(&path, &backend_plugin_id),
                        },
                        backend_record: RepositoryBackendRecord {
                            plugin_id: backend_plugin_id,
                            config: backend_config,
                        },
                    })
                },
            )
            .optional()
            .map_err(db_error)?
            .ok_or_else(|| format!("repository not found: {repo_id}"))
    }

    fn load_repository_records(&self) -> Result<Vec<RepositoryRecord>, String> {
        self.ensure_initialized()?;
        let registry = Connection::open(&self.registry_path).map_err(db_error)?;
        let mut stmt = registry
            .prepare(
                r#"
                SELECT repo_id, name, path, backend_plugin_id, backend_config_json, status, updated_at
                FROM repositories
                ORDER BY name COLLATE NOCASE
                "#,
            )
            .map_err(db_error)?;
        let plugin_registry = backend_plugin_registry(&self.root);
        let rows = stmt
            .query_map([], |row| {
                let backend_plugin_id: String = row.get(3)?;
                let backend_plugin_id = plugin_registry.normalize_plugin_id(&backend_plugin_id);
                let path: String = row.get(2)?;
                let stored_status: String = row.get(5)?;
                let status =
                    repository_runtime_status(&path, &backend_plugin_id, stored_status.as_str());
                let backend_config_json: String = row.get(4)?;
                let backend_config =
                    parse_backend_config_json(&backend_config_json).map_err(to_from_sql_error)?;
                Ok(RepositoryRecord {
                    summary: RepositorySummary {
                        repo_id: row.get(0)?,
                        name: row.get(1)?,
                        path: path.clone(),
                        backend: backend_summary_from_registry(
                            &plugin_registry,
                            &backend_plugin_id,
                        ),
                        status,
                        asset_count: 0,
                        updated_at: row.get(6)?,
                        local_cache: repository_local_cache_status(&path, &backend_plugin_id),
                    },
                    backend_record: RepositoryBackendRecord {
                        plugin_id: backend_plugin_id,
                        config: backend_config,
                    },
                })
            })
            .map_err(db_error)?;

        rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
    }

    fn find_existing_repository_for_backend(
        &self,
        backend: &RepositoryBackendRecord,
    ) -> Result<Option<RepositorySummary>, String> {
        if backend.plugin_id != NETEASE_CLOUD_MUSIC_PLUGIN_ID {
            return Ok(None);
        }
        let account_id = backend
            .config
            .get("accountId")
            .and_then(normalized_netease_account_id);
        let Some(account_id) = account_id else {
            return Ok(None);
        };
        Ok(self
            .load_repository_records()?
            .into_iter()
            .find(|record| {
                record.backend_record.plugin_id == NETEASE_CLOUD_MUSIC_PLUGIN_ID
                    && record
                        .backend_record
                        .config
                        .get("accountId")
                        .and_then(normalized_netease_account_id)
                        .as_deref()
                        == Some(account_id.as_str())
            })
            .map(|record| record.summary))
    }

    fn repository_backend_in_use(&self, plugin_id: &str) -> Result<bool, String> {
        let registry = Connection::open(&self.registry_path).map_err(db_error)?;
        let plugin_registry = backend_plugin_registry(&self.root);
        let mut stmt = registry
            .prepare("SELECT backend_plugin_id FROM repositories")
            .map_err(db_error)?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(db_error)?;

        for row in rows {
            let stored_plugin_id = row.map_err(db_error)?;
            if plugin_registry.normalize_plugin_id(&stored_plugin_id) == plugin_id {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn open_repository_connection(
        &self,
        repo_id: &str,
        repo_path: &str,
        backend_record: &RepositoryBackendRecord,
    ) -> Result<Connection, String> {
        let repo_root = Path::new(repo_path);
        let storage_paths = ensure_repository_storage_paths(
            &self.root,
            repo_id,
            repo_root,
            &backend_record.plugin_id,
        )?;
        let connection = Connection::open(storage_paths.database_path).map_err(db_error)?;
        connection
            .pragma_update(None, "journal_mode", "WAL")
            .map_err(db_error)?;
        migrate_repository_schema(&connection).map_err(db_error)?;
        Ok(connection)
    }

    fn repository_thumbnail_root(&self, repo: &RepositoryRecord) -> Result<PathBuf, String> {
        let repo_root = Path::new(&repo.summary.path);
        let storage_paths = ensure_repository_storage_paths(
            &self.root,
            &repo.summary.repo_id,
            repo_root,
            &repo.backend_record.plugin_id,
        )?;
        Ok(storage_paths.metadata_dir.join("thumbnails"))
    }

    fn repository_cache_root(&self, repo: &RepositoryRecord) -> Result<PathBuf, String> {
        let repo_root = Path::new(&repo.summary.path);
        let storage_paths = ensure_repository_storage_paths(
            &self.root,
            &repo.summary.repo_id,
            repo_root,
            &repo.backend_record.plugin_id,
        )?;
        Ok(storage_paths.metadata_dir.join("cache"))
    }
}

fn dominant_folder_label(folders: &[FolderSummary], assets: &[AssetSummary]) -> String {
    if let Some(folder) = folders.first() {
        return folder.label.clone();
    }

    assets
        .first()
        .and_then(|asset| Path::new(&asset.path).parent())
        .map(|parent| parent.to_string_lossy().replace('\\', "/"))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "仓库根目录".to_string())
}

fn build_repository_overview(
    repo_root: &Path,
    assets: &[AssetSummary],
) -> Result<RepositoryOverview, String> {
    let file_count = assets.len() as i64;
    let total_size_bytes = assets.iter().map(|asset| asset.size_bytes).sum::<i64>();
    let folder_count = count_repository_directories(repo_root)?;
    let readme_content = read_repository_readme(repo_root)?;

    Ok(RepositoryOverview {
        total_size_bytes,
        total_size_label: format_size_label(total_size_bytes),
        file_count,
        folder_count,
        readme_content,
    })
}

fn load_asset_count(
    service_root: &Path,
    repo_id: &str,
    repo_path: &str,
    backend_plugin_id: &str,
) -> Result<i64, rusqlite::Error> {
    let storage_paths = ensure_repository_storage_paths(
        service_root,
        repo_id,
        Path::new(repo_path),
        backend_plugin_id,
    )
    .map_err(|error| {
        rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
            std::io::ErrorKind::Other,
            error,
        )))
    })?;
    let connection = Connection::open(storage_paths.database_path)?;
    connection.query_row(
        "SELECT COUNT(*) FROM assets WHERE status != 'deleted'",
        [],
        |row| row.get(0),
    )
}

fn load_asset_path_map(
    connection: &Connection,
    repo_id: &str,
) -> Result<BTreeMap<String, AssetPathRecord>, rusqlite::Error> {
    let mut stmt = connection.prepare(
        r#"
        SELECT
          a.path,
          a.asset_id,
          a.status,
          a.thumbnail_path,
          hm.group_id,
          hm.link_state,
          a.is_virtual,
          a.provider_id,
          a.provider_item_id,
          a.source_payload_json,
          a.local_absolute_path
        FROM assets a
        LEFT JOIN hardlink_members hm ON hm.repo_id = a.repo_id AND hm.asset_id = a.asset_id
        WHERE a.repo_id = ?1
        "#,
    )?;

    let rows = stmt.query_map([repo_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, Option<String>>(5)?,
            row.get::<_, i64>(6)?,
            row.get::<_, Option<String>>(7)?,
            row.get::<_, Option<String>>(8)?,
            row.get::<_, Option<String>>(9)?,
            row.get::<_, Option<String>>(10)?,
        ))
    })?;

    let mut map = BTreeMap::new();
    for row in rows {
        let (
            path,
            asset_id,
            status,
            thumbnail_path,
            hardlink_group_id,
            hardlink_state,
            is_virtual,
            provider_id,
            provider_item_id,
            source_payload_json,
            local_absolute_path,
        ) = row?;
        map.insert(
            path,
            AssetPathRecord {
                asset_id,
                status,
                thumbnail_path,
                hardlink_group_id,
                hardlink_state,
                is_virtual: is_virtual != 0,
                provider_id,
                provider_item_id,
                source_payload: parse_json_column_nullable(source_payload_json)?,
                local_absolute_path,
            },
        );
    }
    Ok(map)
}

fn load_entry_thumbnail_map(
    connection: &Connection,
    repo_id: &str,
) -> Result<BTreeMap<(String, String), ThumbnailRecord>, rusqlite::Error> {
    let mut stmt = connection.prepare(
        r#"
        SELECT path, kind, thumbnail_path, custom
        FROM entry_thumbnails
        WHERE repo_id = ?1
        "#,
    )?;

    let rows = stmt.query_map([repo_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)?,
        ))
    })?;

    let mut map = BTreeMap::new();
    for row in rows {
        let (path, kind, thumbnail_path, custom) = row?;
        map.insert(
            (path, kind),
            ThumbnailRecord {
                path: thumbnail_path,
                custom: custom != 0,
            },
        );
    }
    Ok(map)
}

fn load_entry_thumbnail_record(
    connection: &Connection,
    repo_id: &str,
    path: &str,
    kind: &str,
) -> Result<Option<ThumbnailRecord>, rusqlite::Error> {
    connection
        .query_row(
            r#"
            SELECT thumbnail_path, custom
            FROM entry_thumbnails
            WHERE repo_id = ?1 AND path = ?2 AND kind = ?3
            "#,
            params![repo_id, path, kind],
            |row| {
                Ok(ThumbnailRecord {
                    path: row.get(0)?,
                    custom: row.get::<_, i64>(1)? != 0,
                })
            },
        )
        .optional()
}

fn upsert_entry_thumbnail_record(
    connection: &Connection,
    repo_id: &str,
    path: &str,
    kind: &str,
    thumbnail_path: &str,
    custom: bool,
) -> Result<(), rusqlite::Error> {
    connection.execute(
        r#"
        INSERT INTO entry_thumbnails (repo_id, path, kind, thumbnail_path, custom, updated_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6)
        ON CONFLICT(repo_id, path, kind)
        DO UPDATE SET
          thumbnail_path = excluded.thumbnail_path,
          custom = excluded.custom,
          updated_at = excluded.updated_at
        "#,
        params![
            repo_id,
            path,
            kind,
            thumbnail_path,
            if custom { 1 } else { 0 },
            now_rfc3339()
        ],
    )?;
    Ok(())
}

fn remove_entry_thumbnail_record(
    connection: &Connection,
    repo_id: &str,
    path: &str,
    kind: &str,
) -> Result<(), rusqlite::Error> {
    connection.execute(
        r#"
        DELETE FROM entry_thumbnails
        WHERE repo_id = ?1 AND path = ?2 AND kind = ?3
        "#,
        params![repo_id, path, kind],
    )?;
    Ok(())
}

fn update_asset_thumbnail_path(
    connection: &Connection,
    repo_id: &str,
    asset_id: &str,
    thumbnail_path: Option<&str>,
) -> Result<(), rusqlite::Error> {
    connection.execute(
        r#"
        UPDATE assets
        SET thumbnail_path = ?3, updated_at = ?4
        WHERE repo_id = ?1 AND asset_id = ?2
        "#,
        params![repo_id, asset_id, thumbnail_path, now_rfc3339()],
    )?;
    Ok(())
}

fn normalize_asset_summaries(
    connection: &Connection,
    repo: &RepositoryRecord,
    thumbnail_root: &Path,
    assets: Vec<AssetSummary>,
) -> Result<Vec<AssetSummary>, String> {
    assets
        .into_iter()
        .map(|mut asset| {
            asset.thumbnail_path = normalize_asset_thumbnail_path(
                connection,
                repo,
                thumbnail_root,
                &asset.asset_id,
                &asset.path,
                asset.thumbnail_path,
            )?;
            Ok(asset)
        })
        .collect()
}

fn normalize_asset_thumbnail_map(
    connection: &Connection,
    repo: &RepositoryRecord,
    thumbnail_root: &Path,
    asset_map: BTreeMap<String, AssetPathRecord>,
) -> Result<BTreeMap<String, AssetPathRecord>, String> {
    asset_map
        .into_iter()
        .map(|(path, mut record)| {
            let thumbnail_path = normalize_asset_thumbnail_path(
                connection,
                repo,
                thumbnail_root,
                &record.asset_id,
                &path,
                record.thumbnail_path,
            )?;
            record.thumbnail_path = thumbnail_path;
            Ok((path, record))
        })
        .collect()
}

fn normalize_asset_thumbnail_path(
    connection: &Connection,
    repo: &RepositoryRecord,
    thumbnail_root: &Path,
    asset_id: &str,
    entry_path: &str,
    thumbnail_path: Option<String>,
) -> Result<Option<String>, String> {
    let original_path = thumbnail_path.clone();
    let normalized = normalize_thumbnail_path(
        repo,
        thumbnail_root,
        entry_path,
        "file",
        "generated",
        thumbnail_path,
    )?;
    if normalized != original_path {
        update_asset_thumbnail_path(
            connection,
            &repo.summary.repo_id,
            asset_id,
            normalized.as_deref(),
        )
        .map_err(db_error)?;
    }
    Ok(normalized)
}

fn normalize_entry_thumbnail_map(
    connection: &Connection,
    repo: &RepositoryRecord,
    thumbnail_root: &Path,
    thumbnail_map: BTreeMap<(String, String), ThumbnailRecord>,
) -> Result<BTreeMap<(String, String), ThumbnailRecord>, String> {
    thumbnail_map
        .into_iter()
        .filter_map(|((path, kind), record)| {
            match normalize_entry_thumbnail_record(
                connection,
                repo,
                thumbnail_root,
                &path,
                &kind,
                Some(record),
            ) {
                Ok(Some(record)) => Some(Ok(((path, kind), record))),
                Ok(None) => None,
                Err(error) => Some(Err(error)),
            }
        })
        .collect()
}

fn normalize_entry_thumbnail_record(
    connection: &Connection,
    repo: &RepositoryRecord,
    thumbnail_root: &Path,
    entry_path: &str,
    kind: &str,
    record: Option<ThumbnailRecord>,
) -> Result<Option<ThumbnailRecord>, String> {
    let Some(record) = record else {
        return Ok(None);
    };
    let source = if record.custom { "custom" } else { "generated" };
    let original_path = record.path.clone();
    let normalized = normalize_thumbnail_path(
        repo,
        thumbnail_root,
        entry_path,
        kind,
        source,
        Some(record.path),
    )?;
    match normalized {
        Some(path) => {
            if path != original_path {
                upsert_entry_thumbnail_record(
                    connection,
                    &repo.summary.repo_id,
                    entry_path,
                    kind,
                    &path,
                    record.custom,
                )
                .map_err(db_error)?;
            }
            Ok(Some(ThumbnailRecord {
                path,
                custom: record.custom,
            }))
        }
        None => {
            remove_entry_thumbnail_record(connection, &repo.summary.repo_id, entry_path, kind)
                .map_err(db_error)?;
            Ok(None)
        }
    }
}

fn normalize_thumbnail_path(
    repo: &RepositoryRecord,
    thumbnail_root: &Path,
    entry_path: &str,
    kind: &str,
    source: &str,
    thumbnail_path: Option<String>,
) -> Result<Option<String>, String> {
    let Some(path) = thumbnail_path else {
        return Ok(None);
    };
    if thumbnail_path_is_valid(thumbnail_root, &path) {
        return Ok(Some(path));
    }

    let source_path = Path::new(&path);
    if !source_path.is_file() {
        return Ok(None);
    }

    let thumbnail_dir = thumbnail_root.join(thumbnail_repository_dir_name(
        &repo.summary.repo_id,
        &repo.summary.path,
    ));
    fs::create_dir_all(&thumbnail_dir).map_err(io_error)?;
    let target_path = thumbnail_dir.join(thumbnail_file_name(
        &repo.summary.repo_id,
        &repo.summary.path,
        entry_path,
        kind,
        source,
    ));
    if source_path != target_path {
        fs::copy(source_path, &target_path).map_err(io_error)?;
    }
    Ok(Some(target_path.to_string_lossy().to_string()))
}

fn load_folder_summaries(
    connection: &Connection,
    repo_id: &str,
) -> Result<Vec<FolderSummary>, rusqlite::Error> {
    let mut stmt = connection.prepare(
        r#"
        SELECT
          CASE
            WHEN instr(path, '/') > 0 THEN substr(path, 1, instr(path, '/') - 1)
            ELSE path
          END AS top_folder,
          COUNT(*) AS asset_count
        FROM assets
        WHERE repo_id = ?1 AND status != 'deleted'
        GROUP BY top_folder
        ORDER BY asset_count DESC, top_folder COLLATE NOCASE
        "#,
    )?;

    let rows = stmt.query_map([repo_id], |row| {
        let path: String = row.get(0)?;
        Ok(FolderSummary {
            label: path.clone(),
            path,
            asset_count: row.get(1)?,
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>()
}

fn load_repository_shortcuts(
    connection: &Connection,
    repo_id: &str,
) -> Result<Vec<RepositoryShortcut>, rusqlite::Error> {
    let mut stmt = connection.prepare(
        r#"
        SELECT shortcut_id, label, target_kind, target_path, target_id
        FROM repository_shortcuts
        WHERE repo_id = ?1
        ORDER BY sort_order, label COLLATE NOCASE
        "#,
    )?;
    let rows = stmt.query_map([repo_id], |row| {
        Ok(RepositoryShortcut {
            shortcut_id: row.get(0)?,
            label: row.get(1)?,
            target_kind: row.get(2)?,
            target_path: row.get(3)?,
            target_id: row.get(4)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>()
}

fn load_playlists(
    connection: &Connection,
    repo_id: &str,
) -> Result<Vec<PlaylistSummary>, rusqlite::Error> {
    let mut stmt = connection.prepare(
        r#"
        SELECT
          p.playlist_id,
          p.repo_id,
          p.name,
          p.player_type_id,
          p.player_plugin_id,
          p.player_label,
          p.file_class,
          COUNT(pi.playlist_item_id) AS item_count,
          p.sort_order,
          p.created_at,
          p.updated_at
        FROM playlists p
        LEFT JOIN playlist_items pi
          ON pi.repo_id = p.repo_id AND pi.playlist_id = p.playlist_id
        WHERE p.repo_id = ?1
        GROUP BY
          p.playlist_id, p.repo_id, p.name, p.player_type_id, p.player_plugin_id,
          p.player_label, p.file_class, p.sort_order, p.created_at, p.updated_at
        ORDER BY p.sort_order, p.updated_at DESC, p.name COLLATE NOCASE
        "#,
    )?;

    let rows = stmt.query_map([repo_id], |row| {
        Ok(PlaylistSummary {
            playlist_id: row.get(0)?,
            repo_id: row.get(1)?,
            name: row.get(2)?,
            player_type_id: row.get(3)?,
            player_plugin_id: row.get(4)?,
            player_label: row.get(5)?,
            file_class: row.get(6)?,
            item_count: row.get(7)?,
            sort_order: row.get(8)?,
            created_at: row.get(9)?,
            updated_at: row.get(10)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>()
}

fn load_playlist_summary(
    connection: &Connection,
    repo_id: &str,
    playlist_id: &str,
) -> Result<Option<PlaylistSummary>, rusqlite::Error> {
    load_playlists(connection, repo_id).map(|items| {
        items
            .into_iter()
            .find(|item| item.playlist_id == playlist_id)
    })
}

fn load_playlist_memberships(
    connection: &Connection,
    repo_id: &str,
) -> Result<BTreeMap<String, Vec<String>>, rusqlite::Error> {
    let mut stmt = connection.prepare(
        r#"
        SELECT pi.asset_id, pi.playlist_id
        FROM playlist_items pi
        INNER JOIN playlists p
          ON p.repo_id = pi.repo_id AND p.playlist_id = pi.playlist_id
        WHERE pi.repo_id = ?1
        ORDER BY pi.asset_id, p.sort_order, p.updated_at DESC, p.name COLLATE NOCASE
        "#,
    )?;
    let rows = stmt.query_map([repo_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut memberships = BTreeMap::<String, Vec<String>>::new();
    for row in rows {
        let (asset_id, playlist_id) = row?;
        memberships.entry(asset_id).or_default().push(playlist_id);
    }
    Ok(memberships)
}

fn load_playlist_detail(
    connection: &Connection,
    repo: &RepositoryRecord,
    registry: &BackendPluginRegistry,
    repo_id: &str,
    playlist_id: &str,
) -> Result<PlaylistDetail, rusqlite::Error> {
    let playlist = load_playlist_summary(connection, repo_id, playlist_id)?
        .ok_or(rusqlite::Error::QueryReturnedNoRows)?;
    let active_player = registry.playlist_player(&playlist.player_type_id);
    let mut stmt = connection.prepare(
        r#"
        SELECT
          pi.playlist_item_id,
          pi.playlist_id,
          pi.asset_id,
          pi.sort_order,
          pi.added_at,
          a.path,
          a.filename,
          a.extension,
          a.thumbnail_path,
          a.status,
          a.is_virtual,
          a.provider_id,
          a.provider_item_id,
          a.source_payload_json,
          a.local_absolute_path
        FROM playlist_items pi
        LEFT JOIN assets a
          ON a.repo_id = pi.repo_id AND a.asset_id = pi.asset_id
        WHERE pi.repo_id = ?1 AND pi.playlist_id = ?2
        ORDER BY pi.sort_order, pi.added_at
        "#,
    )?;
    let rows = stmt.query_map(params![repo_id, playlist_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, Option<String>>(5)?,
            row.get::<_, Option<String>>(6)?,
            row.get::<_, Option<String>>(7)?,
            row.get::<_, Option<String>>(8)?,
            row.get::<_, Option<String>>(9)?,
            row.get::<_, Option<i64>>(10)?,
            row.get::<_, Option<String>>(11)?,
            row.get::<_, Option<String>>(12)?,
            row.get::<_, Option<String>>(13)?,
            row.get::<_, Option<String>>(14)?,
        ))
    })?;

    let items = rows
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|row| {
            let (
                playlist_item_id,
                playlist_id,
                asset_id,
                sort_order,
                added_at,
                path,
                filename,
                extension,
                thumbnail_path,
                asset_status,
                is_virtual,
                provider_id,
                provider_item_id,
                source_payload_json,
                local_absolute_path,
            ) = row;
            let extension = extension.unwrap_or_default();
            let path_value = path.clone().unwrap_or_default();
            let thumbnail_asset_id = asset_id.clone();
            let thumbnail_entry_path = path.clone().unwrap_or_default();
            let (status, status_reason) = resolve_playlist_item_status(
                &playlist,
                active_player.as_ref(),
                path.as_deref(),
                &extension,
                asset_status.as_deref(),
            );
            Ok(PlaylistItem {
                playlist_item_id,
                playlist_id,
                asset_id,
                path: path_value,
                filename: filename.unwrap_or_else(|| "(已失效文件)".to_string()),
                extension,
                thumbnail_path: thumbnail_path.and_then(|item| {
                    normalize_asset_thumbnail_path(
                        connection,
                        repo,
                        &repo
                            .summary
                            .path
                            .parse::<PathBuf>()
                            .unwrap_or_else(|_| PathBuf::from(&repo.summary.path))
                            .join(REPO_META_DIR)
                            .join("thumbnails"),
                        &thumbnail_asset_id,
                        &thumbnail_entry_path,
                        Some(item),
                    )
                    .ok()
                    .flatten()
                }),
                status,
                status_reason,
                sort_order,
                added_at,
                is_virtual: is_virtual.unwrap_or(0) != 0,
                provider_id,
                provider_item_id,
                source_payload: parse_json_column_nullable(source_payload_json)?,
                local_absolute_path,
            })
        })
        .collect::<Result<Vec<_>, rusqlite::Error>>()?;

    Ok(PlaylistDetail { playlist, items })
}

fn resolve_playlist_item_status(
    playlist: &PlaylistSummary,
    player: Option<&PlaylistPlayerRegistration>,
    path: Option<&str>,
    extension: &str,
    asset_status: Option<&str>,
) -> (String, Option<String>) {
    let Some(path) = path else {
        return (
            "missing".to_string(),
            Some("资源索引中已找不到该文件".to_string()),
        );
    };
    let Some(asset_status) = asset_status else {
        return (
            "missing".to_string(),
            Some("资源索引中已找不到该文件".to_string()),
        );
    };
    if asset_status == "deleted" {
        return (
            "trashed".to_string(),
            Some(format!("文件已移入回收站: {path}")),
        );
    }
    let Some(player) = player else {
        return (
            "pluginUnavailable".to_string(),
            Some(format!("缺少播放类型插件: {}", playlist.player_type_id)),
        );
    };
    if !playlist_player_supports_extension(player, extension) {
        return (
            "incompatible".to_string(),
            Some(format!("当前文件扩展名不再兼容: .{extension}")),
        );
    }
    ("ready".to_string(), None)
}

fn playlist_player_supports_extension(
    player: &PlaylistPlayerRegistration,
    extension: &str,
) -> bool {
    let normalized = extension.trim().to_ascii_lowercase();
    !normalized.is_empty()
        && player
            .supported_extensions
            .iter()
            .any(|item| item == &normalized)
}

fn next_playlist_sort_order(
    connection: &Connection,
    repo_id: &str,
) -> Result<i64, rusqlite::Error> {
    connection.query_row(
        "SELECT COALESCE(MAX(sort_order), -1) + 1 FROM playlists WHERE repo_id = ?1",
        [repo_id],
        |row| row.get(0),
    )
}

fn next_playlist_item_sort_order(
    connection: &Connection,
    repo_id: &str,
    playlist_id: &str,
) -> Result<i64, rusqlite::Error> {
    connection.query_row(
        "SELECT COALESCE(MAX(sort_order), -1) + 1 FROM playlist_items WHERE repo_id = ?1 AND playlist_id = ?2",
        params![repo_id, playlist_id],
        |row| row.get(0),
    )
}

fn validate_playlist_name(name: &str) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("playlist name cannot be empty".to_string());
    }
    Ok(trimmed.to_string())
}

fn validate_playlist_id(id: &str) -> Result<String, String> {
    let trimmed = id.trim();
    if trimmed.is_empty() {
        return Err("playlist id cannot be empty".to_string());
    }
    Ok(trimmed.to_string())
}

fn playlist_id_for(repo_id: &str, name: &str) -> String {
    format!(
        "playlist-{}",
        sha256_hex(&[repo_id.as_bytes(), name.trim().as_bytes()])
    )
}

fn playlist_item_id_for(playlist_id: &str, asset_id: &str) -> String {
    format!(
        "playlist-item-{}",
        sha256_hex(&[playlist_id.as_bytes(), asset_id.as_bytes()])
    )
}

fn normalize_id_list(values: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    values
        .iter()
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .filter(|item| seen.insert(item.clone()))
        .collect()
}

fn load_repository_tag_groups(
    connection: &Connection,
    repo_id: &str,
) -> Result<Vec<RepositoryTagGroup>, rusqlite::Error> {
    let mut group_stmt = connection.prepare(
        r#"
        SELECT tag_group_id, name
        FROM tag_groups
        WHERE repo_id = ?1
        ORDER BY sort_order, name COLLATE NOCASE
        "#,
    )?;
    let group_rows = group_stmt.query_map([repo_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let groups = group_rows.collect::<Result<Vec<_>, _>>()?;
    let mut result = Vec::new();
    for (tag_group_id, name) in groups {
        let mut member_stmt = connection.prepare(
            r#"
            SELECT tag
            FROM tag_group_members
            WHERE repo_id = ?1 AND tag_group_id = ?2
            ORDER BY sort_order, tag COLLATE NOCASE
            "#,
        )?;
        let member_rows =
            member_stmt.query_map(params![repo_id, tag_group_id.as_str()], |row| row.get(0))?;
        result.push(RepositoryTagGroup {
            tag_group_id,
            name,
            tags: member_rows.collect::<Result<Vec<String>, _>>()?,
        });
    }
    Ok(result)
}

fn load_repository_actions(
    connection: &Connection,
    repo_id: &str,
) -> Result<Vec<RepositoryAction>, rusqlite::Error> {
    let mut stmt = connection.prepare(
        r#"
        SELECT action_id
        FROM repository_actions
        WHERE repo_id = ?1
        ORDER BY sort_order, name COLLATE NOCASE
        "#,
    )?;
    let ids = stmt
        .query_map([repo_id], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    ids.into_iter()
        .filter_map(
            |action_id| match load_repository_action(connection, repo_id, &action_id) {
                Ok(Some(action)) => Some(Ok(action)),
                Ok(None) => None,
                Err(error) => Some(Err(error)),
            },
        )
        .collect()
}

fn load_repository_action(
    connection: &Connection,
    repo_id: &str,
    action_id: &str,
) -> Result<Option<RepositoryAction>, rusqlite::Error> {
    let Some(base) = connection
        .query_row(
            r#"
            SELECT action_id, repo_id, source, source_action_id, name, status, enabled,
                   raw_json, unsupported_reason, sort_order, created_at, updated_at
            FROM repository_actions
            WHERE repo_id = ?1 AND action_id = ?2
            "#,
            params![repo_id, action_id],
            |row| {
                let raw_json: String = row.get(7)?;
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, i64>(6)? != 0,
                    parse_json_column(&raw_json)?,
                    row.get::<_, Option<String>>(8)?,
                    row.get::<_, i64>(9)?,
                    row.get::<_, String>(10)?,
                    row.get::<_, String>(11)?,
                ))
            },
        )
        .optional()?
    else {
        return Ok(None);
    };
    let steps = load_repository_action_steps(connection, repo_id, action_id)?;
    let last_run = load_repository_action_last_run(connection, repo_id, action_id)?;
    Ok(Some(RepositoryAction {
        action_id: base.0,
        repo_id: base.1,
        source: base.2,
        source_action_id: base.3,
        name: base.4,
        status: base.5,
        enabled: base.6,
        raw: base.7,
        unsupported_reason: base.8,
        sort_order: base.9,
        created_at: base.10,
        updated_at: base.11,
        steps,
        last_run,
    }))
}

fn load_repository_action_steps(
    connection: &Connection,
    repo_id: &str,
    action_id: &str,
) -> Result<Vec<RepositoryActionStep>, rusqlite::Error> {
    let mut stmt = connection.prepare(
        r#"
        SELECT step_id, action_id, repo_id, step_kind, label, status,
               config_json, raw_json, unsupported_reason, sort_order
        FROM repository_action_steps
        WHERE repo_id = ?1 AND action_id = ?2
        ORDER BY sort_order, label COLLATE NOCASE
        "#,
    )?;
    let rows = stmt.query_map(params![repo_id, action_id], |row| {
        let config_json: String = row.get(6)?;
        let raw_json: String = row.get(7)?;
        Ok(RepositoryActionStep {
            step_id: row.get(0)?,
            action_id: row.get(1)?,
            repo_id: row.get(2)?,
            step_kind: row.get(3)?,
            label: row.get(4)?,
            status: row.get(5)?,
            config: parse_json_column(&config_json)?,
            raw: parse_json_column(&raw_json)?,
            unsupported_reason: row.get(8)?,
            sort_order: row.get(9)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>()
}

fn load_repository_action_last_run(
    connection: &Connection,
    repo_id: &str,
    action_id: &str,
) -> Result<Option<RepositoryActionRun>, rusqlite::Error> {
    connection
        .query_row(
            r#"
            SELECT run_id, action_id, repo_id, status, target_json, message, started_at, finished_at
            FROM repository_action_runs
            WHERE repo_id = ?1 AND action_id = ?2
            ORDER BY started_at DESC
            LIMIT 1
            "#,
            params![repo_id, action_id],
            |row| {
                let target_json: String = row.get(4)?;
                Ok(RepositoryActionRun {
                    run_id: row.get(0)?,
                    action_id: row.get(1)?,
                    repo_id: row.get(2)?,
                    status: row.get(3)?,
                    target: parse_json_column(&target_json)?,
                    message: row.get(5)?,
                    started_at: row.get(6)?,
                    finished_at: row.get(7)?,
                })
            },
        )
        .optional()
}

fn resolve_action_target_asset_ids(
    connection: &Connection,
    request: &RepositoryActionRunRequest,
) -> Result<Vec<String>, String> {
    let mut ids = request.asset_ids.clone().unwrap_or_default();
    for path in request.target_paths.clone().unwrap_or_default() {
        let entry_path = normalize_entry_path(&path)?;
        let asset_id = connection
            .query_row(
                "SELECT asset_id FROM assets WHERE repo_id = ?1 AND path = ?2 AND status != 'deleted'",
                params![request.repo_id, entry_path],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(db_error)?
            .ok_or_else(|| format!("action target asset not found: {path}"))?;
        ids.push(asset_id);
    }
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for id in ids {
        let id = id.trim().to_string();
        if id.is_empty() || !seen.insert(id.clone()) {
            continue;
        }
        let exists = connection
            .query_row(
                "SELECT 1 FROM assets WHERE repo_id = ?1 AND asset_id = ?2 AND status != 'deleted'",
                params![request.repo_id, id.as_str()],
                |_| Ok(()),
            )
            .optional()
            .map_err(db_error)?
            .is_some();
        if !exists {
            return Err(format!("action target asset not found: {id}"));
        }
        result.push(id);
    }
    Ok(result)
}

fn apply_repository_action_step(
    tx: &Transaction<'_>,
    repo_id: &str,
    target_asset_ids: &[String],
    step: &RepositoryActionStep,
    source: &str,
) -> Result<String, String> {
    if step.status != "ready" {
        return Err(step
            .unsupported_reason
            .clone()
            .unwrap_or_else(|| "repository action step is unsupported".to_string()));
    }
    match step.step_kind.as_str() {
        "metadata.update" => {
            let metadata = step
                .config
                .get("metadata")
                .and_then(serde_json::Value::as_object)
                .ok_or_else(|| "metadata action step is missing metadata".to_string())?;
            let patch = metadata
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect::<BTreeMap<_, _>>();
            for asset_id in target_asset_ids {
                update_metadata_for_asset_in_transaction(tx, repo_id, asset_id, &patch, source)
                    .map_err(db_error)?;
            }
            Ok(format!("已更新 {} 个目标的元数据", target_asset_ids.len()))
        }
        "tagGroups.set" => {
            let tags = step
                .config
                .get("tags")
                .cloned()
                .unwrap_or_else(|| serde_json::json!([]));
            let patch = BTreeMap::from([("tagGroups".to_string(), tags)]);
            for asset_id in target_asset_ids {
                update_metadata_for_asset_in_transaction(tx, repo_id, asset_id, &patch, source)
                    .map_err(db_error)?;
            }
            Ok(format!("已更新 {} 个目标的标签", target_asset_ids.len()))
        }
        value => Err(format!("unsupported repository action step kind: {value}")),
    }
}

fn update_metadata_for_asset_in_transaction(
    tx: &Transaction<'_>,
    repo_id: &str,
    asset_id: &str,
    metadata: &BTreeMap<String, serde_json::Value>,
    source: &str,
) -> Result<(), rusqlite::Error> {
    let target_asset_ids = load_alias_member_asset_ids(tx, repo_id, asset_id)?;
    let sync_tags = metadata.contains_key("tagGroups");
    let synced_tags = if sync_tags {
        metadata_tags_from_tag_groups(metadata.get("tagGroups"))
    } else {
        Vec::new()
    };
    let now = now_rfc3339();
    for target_asset_id in target_asset_ids {
        let before_map = load_metadata_map_from_transaction(tx, &target_asset_id)?;
        for (key, value) in metadata {
            let value_type = infer_value_type(value);
            tx.execute(
                r#"
                INSERT INTO metadata (asset_id, key, value_type, value_json, version, updated_at)
                VALUES (?1, ?2, ?3, ?4, 1, ?5)
                ON CONFLICT(asset_id, key)
                DO UPDATE SET
                  value_type = excluded.value_type,
                  value_json = excluded.value_json,
                  version = metadata.version + 1,
                  updated_at = excluded.updated_at
                "#,
                params![target_asset_id, key, value_type, value.to_string(), now],
            )?;
        }
        if sync_tags {
            replace_asset_tags(tx, &target_asset_id, &synced_tags)?;
        }
        let target_version: i64 = tx.query_row(
            "SELECT version + 1 FROM assets WHERE repo_id = ?1 AND asset_id = ?2",
            params![repo_id, target_asset_id],
            |row| row.get(0),
        )?;
        tx.execute(
            r#"
            UPDATE assets
            SET version = ?3, updated_at = ?4, modified_at = ?4
            WHERE repo_id = ?1 AND asset_id = ?2
            "#,
            params![repo_id, target_asset_id, target_version, now],
        )?;
        let after_map = load_metadata_map_from_transaction(tx, &target_asset_id)?;
        tx.execute(
            r#"
            INSERT INTO revisions (
              revision_id, repo_id, asset_id, timestamp, operation, before_json, after_json, source
            )
            VALUES (?1, ?2, ?3, ?4, 'metadata.updated', ?5, ?6, ?7)
            "#,
            params![
                format!("rev-{}-{}", target_asset_id, target_version),
                repo_id,
                target_asset_id,
                now,
                serde_json::to_string(&before_map).map_err(|error| {
                    rusqlite::Error::ToSqlConversionFailure(Box::new(error))
                })?,
                serde_json::to_string(&after_map).map_err(|error| {
                    rusqlite::Error::ToSqlConversionFailure(Box::new(error))
                })?,
                source
            ],
        )?;
    }
    Ok(())
}

fn load_assets(
    connection: &Connection,
    repo_id: &str,
) -> Result<Vec<AssetSummary>, rusqlite::Error> {
    let mut stmt = connection.prepare(
        r#"
        SELECT
          a.asset_id,
          a.repo_id,
          a.path,
          a.filename,
          a.extension,
          a.size_bytes,
          a.status,
          a.modified_at,
          a.version,
          a.thumbnail_path,
          hm.group_id,
          hm.link_state,
          a.is_virtual,
          a.provider_id,
          a.provider_item_id,
          a.source_payload_json,
          a.local_absolute_path
        FROM assets a
        LEFT JOIN hardlink_members hm ON hm.repo_id = a.repo_id AND hm.asset_id = a.asset_id
        WHERE a.repo_id = ?1 AND a.status != 'deleted'
        ORDER BY a.modified_at DESC, a.filename COLLATE NOCASE
        "#,
    )?;

    let rows = stmt.query_map([repo_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, i64>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, String>(7)?,
            row.get::<_, i64>(8)?,
            row.get::<_, Option<String>>(9)?,
            row.get::<_, Option<String>>(10)?,
            row.get::<_, Option<String>>(11)?,
            row.get::<_, i64>(12)?,
            row.get::<_, Option<String>>(13)?,
            row.get::<_, Option<String>>(14)?,
            row.get::<_, Option<String>>(15)?,
            row.get::<_, Option<String>>(16)?,
        ))
    })?;

    let base_assets = rows.collect::<Result<Vec<_>, _>>()?;

    base_assets
        .into_iter()
        .map(
            |(
                asset_id,
                repo_id,
                path,
                filename,
                extension,
                size_bytes,
                status,
                modified_at,
                version,
                thumbnail_path,
                hardlink_group_id,
                hardlink_state,
                is_virtual,
                provider_id,
                provider_item_id,
                source_payload_json,
                local_absolute_path,
            )| {
                let tags = load_tags(connection, &asset_id)?;

                Ok(AssetSummary {
                    asset_id,
                    repo_id,
                    path,
                    filename,
                    extension,
                    size_bytes,
                    size_label: format_size_label(size_bytes),
                    status,
                    modified_at,
                    version,
                    tags,
                    thumbnail_path,
                    hardlink_group_id,
                    hardlink_state,
                    is_virtual: is_virtual != 0,
                    provider_id,
                    provider_item_id,
                    source_payload: parse_json_column_nullable(source_payload_json)?,
                    local_absolute_path,
                })
            },
        )
        .collect::<Result<Vec<_>, _>>()
}

fn load_tags(connection: &Connection, asset_id: &str) -> Result<Vec<String>, rusqlite::Error> {
    let mut stmt = connection.prepare(
        r#"
        SELECT tag
        FROM tags
        WHERE asset_id = ?1
        ORDER BY normalized_tag COLLATE NOCASE
        "#,
    )?;

    let rows = stmt.query_map([asset_id], |row| row.get(0))?;
    rows.collect::<Result<Vec<_>, _>>()
}

fn load_metadata_entries(
    connection: &Connection,
    asset_id: &str,
) -> Result<Vec<MetadataEntry>, rusqlite::Error> {
    let mut stmt = connection.prepare(
        r#"
        SELECT key, value_type, value_json, version, updated_at
        FROM metadata
        WHERE asset_id = ?1
        ORDER BY key COLLATE NOCASE
        "#,
    )?;

    let rows = stmt.query_map([asset_id], |row| {
        let value_json: String = row.get(2)?;
        let value = parse_json_column(&value_json)?;
        Ok(MetadataEntry {
            key: row.get(0)?,
            value_type: row.get(1)?,
            value,
            version: row.get(3)?,
            updated_at: row.get(4)?,
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>()
}

fn load_metadata_map(
    connection: &Connection,
    asset_id: &str,
) -> Result<BTreeMap<String, serde_json::Value>, rusqlite::Error> {
    let entries = load_metadata_entries(connection, asset_id)?;
    let mut metadata = entries
        .into_iter()
        .map(|entry| (entry.key, entry.value))
        .collect::<BTreeMap<_, _>>();
    normalize_loaded_metadata(&mut metadata);
    Ok(metadata)
}

fn load_metadata_maps_for_assets(
    connection: &Connection,
    asset_ids: &[String],
) -> Result<BTreeMap<String, BTreeMap<String, serde_json::Value>>, rusqlite::Error> {
    if asset_ids.is_empty() {
        return Ok(BTreeMap::new());
    }

    let placeholders = std::iter::repeat("?")
        .take(asset_ids.len())
        .collect::<Vec<_>>()
        .join(", ");
    let mut stmt = connection.prepare(&format!(
        r#"
        SELECT asset_id, key, value_json
        FROM metadata
        WHERE asset_id IN ({placeholders})
        ORDER BY key COLLATE NOCASE
        "#
    ))?;
    let rows = stmt.query_map(rusqlite::params_from_iter(asset_ids.iter()), |row| {
        let value_json: String = row.get(2)?;
        let value = parse_json_column(&value_json)?;
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, value))
    })?;

    let mut map = BTreeMap::new();
    for row in rows {
        let (asset_id, key, value) = row?;
        map.entry(asset_id)
            .or_insert_with(BTreeMap::new)
            .insert(key, value);
    }
    for metadata in map.values_mut() {
        normalize_loaded_metadata(metadata);
    }
    Ok(map)
}

fn load_alias_paths_for_assets(
    connection: &Connection,
    repo_id: &str,
    asset_ids: &[String],
) -> Result<BTreeMap<String, Vec<String>>, rusqlite::Error> {
    if asset_ids.is_empty() {
        return Ok(BTreeMap::new());
    }
    let placeholders = std::iter::repeat("?")
        .take(asset_ids.len())
        .collect::<Vec<_>>()
        .join(", ");
    let mut stmt = connection.prepare(&format!(
        r#"
        WITH selected_assets(asset_id) AS (
          SELECT asset_id FROM assets WHERE asset_id IN ({placeholders})
        ),
        selected_groups(alias_group_id) AS (
          SELECT DISTINCT alias_group_id
          FROM asset_alias_members
          WHERE repo_id = ? AND asset_id IN (SELECT asset_id FROM selected_assets)
        )
        SELECT selected_assets.asset_id, member.path
        FROM selected_assets
        JOIN asset_alias_members selected_member
          ON selected_member.repo_id = ? AND selected_member.asset_id = selected_assets.asset_id
        JOIN asset_alias_members member
          ON member.repo_id = selected_member.repo_id
         AND member.alias_group_id = selected_member.alias_group_id
        JOIN selected_groups
          ON selected_groups.alias_group_id = member.alias_group_id
        ORDER BY member.role DESC, member.path COLLATE NOCASE
        "#
    ))?;
    let params = asset_ids
        .iter()
        .map(String::as_str)
        .chain(std::iter::once(repo_id))
        .chain(std::iter::once(repo_id));
    let rows = stmt.query_map(rusqlite::params_from_iter(params), |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for row in rows {
        let (asset_id, path) = row?;
        map.entry(asset_id).or_default().push(path);
    }
    Ok(map)
}

fn load_folder_metadata_map(
    connection: &Connection,
    repo_id: &str,
) -> Result<BTreeMap<String, FolderMetadata>, rusqlite::Error> {
    let mut stmt = connection.prepare(
        r#"
        SELECT path, protected, password_tip
        FROM folder_metadata
        WHERE repo_id = ?1
        "#,
    )?;
    let rows = stmt.query_map([repo_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            FolderMetadata {
                protected: row.get::<_, i64>(1)? != 0,
                password_tip: row.get(2)?,
            },
        ))
    })?;
    rows.collect::<Result<BTreeMap<_, _>, _>>()
}

fn load_metadata_map_from_transaction(
    tx: &Transaction<'_>,
    asset_id: &str,
) -> Result<BTreeMap<String, serde_json::Value>, rusqlite::Error> {
    let mut stmt = tx.prepare(
        r#"
        SELECT key, value_json
        FROM metadata
        WHERE asset_id = ?1
        ORDER BY key COLLATE NOCASE
        "#,
    )?;

    let rows = stmt.query_map([asset_id], |row| {
        let value_json: String = row.get(1)?;
        let value = parse_json_column(&value_json)?;
        Ok((row.get::<_, String>(0)?, value))
    })?;

    let pairs = rows.collect::<Result<Vec<_>, _>>()?;
    let mut metadata = pairs.into_iter().collect::<BTreeMap<_, _>>();
    normalize_loaded_metadata(&mut metadata);
    Ok(metadata)
}

fn normalize_loaded_metadata(metadata: &mut BTreeMap<String, serde_json::Value>) {
    let comment_is_empty = metadata
        .get("comment")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .unwrap_or_default()
        .is_empty();
    if comment_is_empty {
        if let Some(note) = metadata.get("note").and_then(|value| value.as_str()) {
            if !note.trim().is_empty() {
                metadata.insert(
                    "comment".to_string(),
                    serde_json::Value::String(note.to_string()),
                );
            }
        }
    }
}

fn normalize_metadata_entries(mut entries: Vec<MetadataEntry>) -> Vec<MetadataEntry> {
    let comment_is_empty = entries
        .iter()
        .find(|entry| entry.key == "comment")
        .and_then(|entry| entry.value.as_str())
        .map(str::trim)
        .unwrap_or_default()
        .is_empty();
    if !comment_is_empty {
        return entries;
    }
    let Some(note) = entries
        .iter()
        .find(|entry| entry.key == "note")
        .and_then(|entry| entry.value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
    else {
        return entries;
    };
    if let Some(comment) = entries.iter_mut().find(|entry| entry.key == "comment") {
        comment.value = serde_json::Value::String(note);
    } else {
        entries.push(MetadataEntry {
            key: "comment".to_string(),
            value_type: "string".to_string(),
            value: serde_json::Value::String(note),
            version: 1,
            updated_at: now_rfc3339(),
        });
        entries.sort_by(|left, right| left.key.to_lowercase().cmp(&right.key.to_lowercase()));
    }
    entries
}

fn load_alias_member_asset_ids(
    tx: &Transaction<'_>,
    repo_id: &str,
    asset_id: &str,
) -> Result<Vec<String>, rusqlite::Error> {
    let alias_group_id = tx
        .query_row(
            r#"
            SELECT alias_group_id
            FROM asset_alias_members
            WHERE repo_id = ?1 AND asset_id = ?2
            "#,
            params![repo_id, asset_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;

    let Some(alias_group_id) = alias_group_id else {
        return Ok(vec![asset_id.to_string()]);
    };

    let mut stmt = tx.prepare(
        r#"
        SELECT asset_id
        FROM asset_alias_members
        WHERE repo_id = ?1 AND alias_group_id = ?2
        ORDER BY role DESC, path COLLATE NOCASE
        "#,
    )?;
    let rows = stmt.query_map(params![repo_id, alias_group_id], |row| row.get(0))?;
    let mut asset_ids = rows.collect::<Result<Vec<String>, _>>()?;
    if !asset_ids.iter().any(|item| item == asset_id) {
        asset_ids.push(asset_id.to_string());
    }
    Ok(asset_ids)
}

fn metadata_tags_from_tag_groups(value: Option<&serde_json::Value>) -> Vec<String> {
    let mut tags = Vec::new();
    if let Some(value) = value {
        collect_metadata_tags(value, &mut tags);
    }
    let mut seen = HashSet::new();
    tags.into_iter()
        .map(|tag| tag.trim().to_string())
        .filter(|tag| !tag.is_empty())
        .filter(|tag| seen.insert(tag.to_lowercase()))
        .collect()
}

fn collect_metadata_tags(value: &serde_json::Value, tags: &mut Vec<String>) {
    match value {
        serde_json::Value::String(tag) => tags.push(tag.clone()),
        serde_json::Value::Array(items) => {
            for item in items {
                collect_metadata_tags(item, tags);
            }
        }
        serde_json::Value::Object(map) => {
            for key in ["tags", "items", "children"] {
                if let Some(value) = map.get(key) {
                    collect_metadata_tags(value, tags);
                }
            }
            if let Some(label) = map
                .get("label")
                .or_else(|| map.get("name"))
                .and_then(|value| value.as_str())
            {
                tags.push(label.to_string());
            }
        }
        _ => {}
    }
}

fn replace_asset_tags(
    tx: &Transaction<'_>,
    asset_id: &str,
    tags: &[String],
) -> Result<(), rusqlite::Error> {
    tx.execute("DELETE FROM tags WHERE asset_id = ?1", [asset_id])?;
    for tag in tags {
        tx.execute(
            r#"
            INSERT OR REPLACE INTO tags (asset_id, tag, normalized_tag)
            VALUES (?1, ?2, ?3)
            "#,
            params![asset_id, tag, tag.to_lowercase()],
        )?;
    }
    Ok(())
}

fn load_revision_entries(
    connection: &Connection,
    asset_id: &str,
) -> Result<Vec<RevisionEntry>, rusqlite::Error> {
    let mut stmt = connection.prepare(
        r#"
        SELECT revision_id, asset_id, timestamp, operation, before_json, after_json, source
        FROM revisions
        WHERE asset_id = ?1
        ORDER BY timestamp DESC
        LIMIT 12
        "#,
    )?;

    let rows = stmt.query_map([asset_id], |row| {
        let before_json: Option<String> = row.get(4)?;
        let after_json: Option<String> = row.get(5)?;
        Ok(RevisionEntry {
            revision_id: row.get(0)?,
            asset_id: row.get(1)?,
            timestamp: row.get(2)?,
            operation: row.get(3)?,
            before: parse_json_column_optional(before_json)?,
            after: parse_json_column_optional(after_json)?,
            source: row.get(6)?,
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>()
}

fn load_metadata_fields(connection: &Connection) -> Result<Vec<String>, rusqlite::Error> {
    let mut stmt = connection.prepare(
        r#"
        SELECT DISTINCT key
        FROM metadata
        ORDER BY key COLLATE NOCASE
        "#,
    )?;

    let rows = stmt.query_map([], |row| row.get(0))?;
    rows.collect::<Result<Vec<_>, _>>()
}

fn load_asset_detail_from_connection(
    connection: &Connection,
    repo_id: &str,
    asset_id: &str,
) -> Result<AssetDetail, rusqlite::Error> {
    let summary = load_asset_summary(connection, repo_id, asset_id)?
        .ok_or_else(|| rusqlite::Error::QueryReturnedNoRows)?;
    let metadata = normalize_metadata_entries(load_metadata_entries(connection, asset_id)?);
    let revisions = load_revision_entries(connection, asset_id)?;

    Ok(AssetDetail {
        summary,
        metadata,
        revisions,
    })
}

fn load_asset_detail_from_transaction(
    tx: &Transaction<'_>,
    repo_id: &str,
    asset_id: &str,
) -> Result<AssetDetail, rusqlite::Error> {
    let summary = load_asset_summary_from_transaction(tx, repo_id, asset_id)?
        .ok_or_else(|| rusqlite::Error::QueryReturnedNoRows)?;

    let mut metadata_stmt = tx.prepare(
        r#"
        SELECT key, value_type, value_json, version, updated_at
        FROM metadata
        WHERE asset_id = ?1
        ORDER BY key COLLATE NOCASE
        "#,
    )?;
    let metadata_rows = metadata_stmt.query_map([asset_id], |row| {
        let value_json: String = row.get(2)?;
        let value = parse_json_column(&value_json)?;
        Ok(MetadataEntry {
            key: row.get(0)?,
            value_type: row.get(1)?,
            value,
            version: row.get(3)?,
            updated_at: row.get(4)?,
        })
    })?;
    let metadata = normalize_metadata_entries(metadata_rows.collect::<Result<Vec<_>, _>>()?);

    let mut revision_stmt = tx.prepare(
        r#"
        SELECT revision_id, asset_id, timestamp, operation, before_json, after_json, source
        FROM revisions
        WHERE asset_id = ?1
        ORDER BY timestamp DESC
        LIMIT 12
        "#,
    )?;
    let revision_rows = revision_stmt.query_map([asset_id], |row| {
        let before_json: Option<String> = row.get(4)?;
        let after_json: Option<String> = row.get(5)?;
        Ok(RevisionEntry {
            revision_id: row.get(0)?,
            asset_id: row.get(1)?,
            timestamp: row.get(2)?,
            operation: row.get(3)?,
            before: parse_json_column_optional(before_json)?,
            after: parse_json_column_optional(after_json)?,
            source: row.get(6)?,
        })
    })?;
    let revisions = revision_rows.collect::<Result<Vec<_>, _>>()?;

    Ok(AssetDetail {
        summary,
        metadata,
        revisions,
    })
}

fn load_asset_summary(
    connection: &Connection,
    repo_id: &str,
    asset_id: &str,
) -> Result<Option<AssetSummary>, rusqlite::Error> {
    connection
        .query_row(
            r#"
            SELECT
              a.asset_id,
              a.repo_id,
              a.path,
              a.filename,
              a.extension,
              a.size_bytes,
              a.status,
              a.modified_at,
              a.version,
              a.thumbnail_path,
              hm.group_id,
              hm.link_state,
              a.is_virtual,
              a.provider_id,
              a.provider_item_id,
              a.source_payload_json,
              a.local_absolute_path
            FROM assets a
            LEFT JOIN hardlink_members hm ON hm.repo_id = a.repo_id AND hm.asset_id = a.asset_id
            WHERE a.repo_id = ?1 AND a.asset_id = ?2
            "#,
            params![repo_id, asset_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, i64>(8)?,
                    row.get::<_, Option<String>>(9)?,
                    row.get::<_, Option<String>>(10)?,
                    row.get::<_, Option<String>>(11)?,
                    row.get::<_, i64>(12)?,
                    row.get::<_, Option<String>>(13)?,
                    row.get::<_, Option<String>>(14)?,
                    row.get::<_, Option<String>>(15)?,
                    row.get::<_, Option<String>>(16)?,
                ))
            },
        )
        .optional()?
        .map(
            |(
                asset_id,
                repo_id,
                path,
                filename,
                extension,
                size_bytes,
                status,
                modified_at,
                version,
                thumbnail_path,
                hardlink_group_id,
                hardlink_state,
                is_virtual,
                provider_id,
                provider_item_id,
                source_payload_json,
                local_absolute_path,
            )| {
                let tags = load_tags(connection, &asset_id)?;
                Ok(AssetSummary {
                    asset_id,
                    repo_id,
                    path,
                    filename,
                    extension,
                    size_bytes,
                    size_label: format_size_label(size_bytes),
                    status,
                    modified_at,
                    version,
                    tags,
                    thumbnail_path,
                    hardlink_group_id,
                    hardlink_state,
                    is_virtual: is_virtual != 0,
                    provider_id,
                    provider_item_id,
                    source_payload: parse_json_column_nullable(source_payload_json)?,
                    local_absolute_path,
                })
            },
        )
        .transpose()
}

fn load_asset_summary_from_transaction(
    tx: &Transaction<'_>,
    repo_id: &str,
    asset_id: &str,
) -> Result<Option<AssetSummary>, rusqlite::Error> {
    let base = tx
        .query_row(
            r#"
            SELECT
              a.asset_id,
              a.repo_id,
              a.path,
              a.filename,
              a.extension,
              a.size_bytes,
              a.status,
              a.modified_at,
              a.version,
              a.thumbnail_path,
              hm.group_id,
              hm.link_state,
              a.is_virtual,
              a.provider_id,
              a.provider_item_id,
              a.source_payload_json,
              a.local_absolute_path
            FROM assets a
            LEFT JOIN hardlink_members hm ON hm.repo_id = a.repo_id AND hm.asset_id = a.asset_id
            WHERE a.repo_id = ?1 AND a.asset_id = ?2
            "#,
            params![repo_id, asset_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, i64>(8)?,
                    row.get::<_, Option<String>>(9)?,
                    row.get::<_, Option<String>>(10)?,
                    row.get::<_, Option<String>>(11)?,
                    row.get::<_, i64>(12)?,
                    row.get::<_, Option<String>>(13)?,
                    row.get::<_, Option<String>>(14)?,
                    row.get::<_, Option<String>>(15)?,
                    row.get::<_, Option<String>>(16)?,
                ))
            },
        )
        .optional()?;

    let Some((
        asset_id,
        repo_id,
        path,
        filename,
        extension,
        size_bytes,
        status,
        modified_at,
        version,
        thumbnail_path,
        hardlink_group_id,
        hardlink_state,
        is_virtual,
        provider_id,
        provider_item_id,
        source_payload_json,
        local_absolute_path,
    )) = base
    else {
        return Ok(None);
    };

    let mut tag_stmt = tx.prepare(
        r#"
        SELECT tag
        FROM tags
        WHERE asset_id = ?1
        ORDER BY normalized_tag COLLATE NOCASE
        "#,
    )?;
    let tag_rows = tag_stmt.query_map([asset_id.as_str()], |row| row.get(0))?;
    let tags = tag_rows.collect::<Result<Vec<String>, _>>()?;

    Ok(Some(AssetSummary {
        asset_id,
        repo_id,
        path,
        filename,
        extension,
        size_bytes,
        size_label: format_size_label(size_bytes),
        status,
        modified_at,
        version,
        tags,
        thumbnail_path,
        hardlink_group_id,
        hardlink_state,
        is_virtual: is_virtual != 0,
        provider_id,
        provider_item_id,
        source_payload: parse_json_column_nullable(source_payload_json)?,
        local_absolute_path,
    }))
}

fn search_repository_assets(
    connection: &Connection,
    repo: &RepositorySummary,
    query: &str,
    request: &SearchRequest,
) -> Result<Vec<SearchHit>, rusqlite::Error> {
    let assets = load_assets(connection, &repo.repo_id)?;
    let mut results = Vec::new();

    for asset in assets {
        let metadata = load_metadata_map(connection, &asset.asset_id)?;
        if asset.status == "deleted" {
            continue;
        }
        if !search_filter_matches(repo, &asset, &metadata, query, request) {
            continue;
        }

        results.push(SearchHit {
            repo_id: repo.repo_id.clone(),
            repo_name: repo.name.clone(),
            asset_id: asset.asset_id.clone(),
            path: asset.path.clone(),
            filename: asset.filename.clone(),
            status: asset.status.clone(),
            tags: asset.tags.clone(),
            metadata,
            is_virtual: asset.is_virtual,
            provider_id: asset.provider_id.clone(),
            provider_item_id: asset.provider_item_id.clone(),
            source_payload: asset.source_payload.clone(),
            local_absolute_path: asset.local_absolute_path.clone(),
        });
    }

    sort_search_hits(&mut results, request.sort.as_ref());
    if let Some(limit) = request.limit.filter(|value| *value > 0) {
        results.truncate(limit);
    }

    Ok(results)
}

fn search_filter_matches(
    repo: &RepositorySummary,
    asset: &AssetSummary,
    metadata: &BTreeMap<String, serde_json::Value>,
    query: &str,
    request: &SearchRequest,
) -> bool {
    let mut include_matches = Vec::new();
    push_query_match(&mut include_matches, repo, asset, metadata, Some(query));
    push_format_match(
        &mut include_matches,
        &asset.extension,
        request.formats.as_ref(),
    );
    push_legacy_tag_match(&mut include_matches, &asset.tags, request.tag.as_deref());
    push_tag_match(&mut include_matches, &asset.tags, request.tags.as_ref());
    push_legacy_metadata_match(
        &mut include_matches,
        metadata,
        request.metadata_key.as_deref(),
        request.metadata_value.as_deref(),
    );
    push_metadata_match(
        &mut include_matches,
        metadata,
        request.metadata_filters.as_ref(),
    );
    push_number_match(
        &mut include_matches,
        metadata,
        request.number_filters.as_ref(),
    );
    push_date_match(
        &mut include_matches,
        metadata,
        request.date_filters.as_ref(),
    );
    push_rating_match(&mut include_matches, metadata, request.min_rating);

    if !combine_include_matches(&include_matches, request.match_mode.as_deref()) {
        return false;
    }

    !matches_excluded_filters(
        repo,
        asset,
        &asset.tags,
        &asset.extension,
        metadata,
        request.exclude_query.as_deref(),
        request.exclude_path_prefixes.as_ref(),
        request.exclude_tags.as_ref(),
        request.exclude_formats.as_ref(),
        request.exclude_metadata_filters.as_ref(),
        request.exclude_number_filters.as_ref(),
        request.exclude_date_filters.as_ref(),
    )
}

fn push_query_match(
    include_matches: &mut Vec<bool>,
    repo: &RepositorySummary,
    asset: &AssetSummary,
    metadata: &BTreeMap<String, serde_json::Value>,
    query: Option<&str>,
) {
    let terms = query_terms(query);
    if terms.is_empty() {
        return;
    }
    let haystack = build_search_haystack(repo, asset, metadata);
    include_matches.push(terms.iter().all(|term| haystack.contains(term)));
}

fn push_path_prefix_match(
    include_matches: &mut Vec<bool>,
    asset: &AssetSummary,
    path_prefix: Option<&str>,
) {
    let prefixes = query_terms(path_prefix);
    if prefixes.is_empty() {
        return;
    }
    include_matches.push(
        prefixes
            .iter()
            .all(|prefix| asset.path == *prefix || asset.path.starts_with(&format!("{prefix}/"))),
    );
}

fn push_format_match(
    include_matches: &mut Vec<bool>,
    extension: &str,
    formats: Option<&Vec<String>>,
) {
    let formats = formats.map_or_else(Vec::new, |values| normalized_filter_values(values));
    if formats.is_empty() {
        return;
    }
    include_matches.push(formats.contains(&extension.to_lowercase()));
}

fn push_legacy_tag_match(
    include_matches: &mut Vec<bool>,
    asset_tags: &[String],
    tag: Option<&str>,
) {
    let Some(tag) = tag.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };
    let tag = tag.to_lowercase();
    include_matches.push(
        asset_tags
            .iter()
            .any(|item| item.to_lowercase().contains(&tag)),
    );
}

fn push_tag_match(
    include_matches: &mut Vec<bool>,
    asset_tags: &[String],
    tags: Option<&Vec<String>>,
) {
    let tags = tags.map_or_else(Vec::new, |values| normalized_filter_values(values));
    if tags.is_empty() {
        return;
    }
    include_matches.push(tags_match(asset_tags, &tags, Some("and")));
}

fn push_legacy_metadata_match(
    include_matches: &mut Vec<bool>,
    metadata: &BTreeMap<String, serde_json::Value>,
    metadata_key: Option<&str>,
    metadata_value: Option<&str>,
) {
    let Some(key) = metadata_key
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return;
    };
    let matched = metadata.get(key).is_some_and(|value| {
        metadata_value
            .map(str::trim)
            .filter(|expected| !expected.is_empty())
            .is_none_or(|expected| {
                json_value_to_search_text(value)
                    .to_lowercase()
                    .contains(&expected.to_lowercase())
            })
    });
    include_matches.push(matched);
}

fn push_metadata_match(
    include_matches: &mut Vec<bool>,
    metadata: &BTreeMap<String, serde_json::Value>,
    filters: Option<&Vec<SearchMetadataFilter>>,
) {
    let Some(filters) = filters else {
        return;
    };
    if has_active_metadata_filters(filters) {
        include_matches.push(metadata_filters_match_with_mode(
            metadata,
            filters,
            Some("and"),
        ));
    }
}

fn push_number_match(
    include_matches: &mut Vec<bool>,
    metadata: &BTreeMap<String, serde_json::Value>,
    filters: Option<&Vec<SearchNumberFilter>>,
) {
    let Some(filters) = filters else {
        return;
    };
    if has_active_number_filters(filters) {
        include_matches.push(number_filters_match(metadata, filters));
    }
}

fn push_date_match(
    include_matches: &mut Vec<bool>,
    metadata: &BTreeMap<String, serde_json::Value>,
    filters: Option<&Vec<SearchDateFilter>>,
) {
    let Some(filters) = filters else {
        return;
    };
    if has_active_date_filters(filters) {
        include_matches.push(date_filters_match(metadata, filters));
    }
}

fn push_rating_match(
    include_matches: &mut Vec<bool>,
    metadata: &BTreeMap<String, serde_json::Value>,
    min_rating: Option<f64>,
) {
    let Some(min_rating) = min_rating else {
        return;
    };
    let rating = metadata
        .get("rating")
        .and_then(|value| value.as_f64())
        .unwrap_or_default();
    include_matches.push(rating >= min_rating);
}

fn combine_include_matches(matches: &[bool], match_mode: Option<&str>) -> bool {
    if matches.is_empty() {
        true
    } else if is_or_match_mode(match_mode) {
        matches.iter().any(|matched| *matched)
    } else {
        matches.iter().all(|matched| *matched)
    }
}

fn matches_excluded_filters(
    repo: &RepositorySummary,
    asset: &AssetSummary,
    asset_tags: &[String],
    extension: &str,
    metadata: &BTreeMap<String, serde_json::Value>,
    exclude_query: Option<&str>,
    exclude_path_prefixes: Option<&Vec<String>>,
    exclude_tags: Option<&Vec<String>>,
    exclude_formats: Option<&Vec<String>>,
    exclude_metadata_filters: Option<&Vec<SearchMetadataFilter>>,
    exclude_number_filters: Option<&Vec<SearchNumberFilter>>,
    exclude_date_filters: Option<&Vec<SearchDateFilter>>,
) -> bool {
    if query_terms(exclude_query)
        .iter()
        .any(|term| build_search_haystack(repo, asset, metadata).contains(term))
    {
        return true;
    }
    if exclude_path_prefixes.is_some_and(|prefixes| {
        prefixes.iter().any(|prefix| {
            normalize_directory_path(prefix).ok().is_some_and(|prefix| {
                !prefix.is_empty()
                    && (asset.path == prefix || asset.path.starts_with(&format!("{prefix}/")))
            })
        })
    }) {
        return true;
    }
    let tags = exclude_tags.map_or_else(Vec::new, |values| normalized_filter_values(values));
    if !tags.is_empty() && tags_match(asset_tags, &tags, Some("or")) {
        return true;
    }

    let formats = exclude_formats.map_or_else(Vec::new, |values| normalized_filter_values(values));
    if !formats.is_empty() && formats.contains(&extension.to_lowercase()) {
        return true;
    }

    exclude_metadata_filters.is_some_and(|filters| {
        has_active_metadata_filters(filters)
            && metadata_filters_match_with_mode(metadata, filters, Some("or"))
    }) || exclude_number_filters.is_some_and(|filters| {
        has_active_number_filters(filters) && number_filters_match(metadata, filters)
    }) || exclude_date_filters.is_some_and(|filters| {
        has_active_date_filters(filters) && date_filters_match(metadata, filters)
    })
}

fn normalized_filter_values(values: &[String]) -> Vec<String> {
    values
        .iter()
        .map(|value| value.trim().to_lowercase())
        .filter(|value| !value.is_empty())
        .collect()
}

fn tags_match(asset_tags: &[String], filters: &[String], match_mode: Option<&str>) -> bool {
    let matches = |filter: &String| {
        asset_tags.iter().any(|item| {
            let normalized_tag = item.to_lowercase();
            normalized_tag.contains(filter)
        })
    };
    if is_or_match_mode(match_mode) {
        filters.iter().any(matches)
    } else {
        filters.iter().all(matches)
    }
}

fn query_terms(value: Option<&str>) -> Vec<String> {
    value
        .map(|value| {
            value
                .lines()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| value.to_lowercase())
                .collect()
        })
        .unwrap_or_default()
}

fn metadata_filter_groups(filters: &[SearchMetadataFilter]) -> BTreeMap<String, Vec<String>> {
    let mut grouped_filters = BTreeMap::<String, Vec<String>>::new();
    for filter in filters {
        let key = filter.key.trim();
        let value = filter.value.trim();
        if key.is_empty() || value.is_empty() {
            continue;
        }
        grouped_filters
            .entry(key.to_string())
            .or_default()
            .push(value.to_lowercase());
    }
    grouped_filters
}

fn has_active_metadata_filters(filters: &[SearchMetadataFilter]) -> bool {
    filters
        .iter()
        .any(|filter| !filter.key.trim().is_empty() && !filter.value.trim().is_empty())
}

fn metadata_filters_match_with_mode(
    metadata: &BTreeMap<String, serde_json::Value>,
    filters: &[SearchMetadataFilter],
    match_mode: Option<&str>,
) -> bool {
    let grouped_filters = metadata_filter_groups(filters);

    let matcher = |(key, expected_values): (String, Vec<String>)| {
        let Some(actual_value) = metadata.get(&key) else {
            return false;
        };
        let actual_text = json_value_to_search_text(actual_value).to_lowercase();
        expected_values
            .iter()
            .any(|expected| actual_text == *expected || actual_text.contains(expected))
    };
    if is_or_match_mode(match_mode) {
        grouped_filters.into_iter().any(matcher)
    } else {
        grouped_filters.into_iter().all(matcher)
    }
}

fn number_filters_match(
    metadata: &BTreeMap<String, serde_json::Value>,
    filters: &[SearchNumberFilter],
) -> bool {
    filters.iter().all(|filter| {
        let key = filter.key.trim();
        if key.is_empty() {
            return true;
        }
        let Some(value) = metadata.get(key).and_then(|value| value.as_f64()) else {
            return false;
        };
        if filter.min.is_some_and(|min| value < min) {
            return false;
        }
        if filter.max.is_some_and(|max| value > max) {
            return false;
        }
        true
    })
}

fn has_active_number_filters(filters: &[SearchNumberFilter]) -> bool {
    filters.iter().any(|filter| {
        !filter.key.trim().is_empty() && (filter.min.is_some() || filter.max.is_some())
    })
}

fn date_filters_match(
    metadata: &BTreeMap<String, serde_json::Value>,
    filters: &[SearchDateFilter],
) -> bool {
    filters.iter().all(|filter| {
        let key = filter.key.trim();
        if key.is_empty() {
            return true;
        }
        let Some(value) = metadata
            .get(key)
            .and_then(|value| value.as_str())
            .and_then(parse_rfc3339_timestamp)
        else {
            return false;
        };
        if filter
            .from
            .as_deref()
            .and_then(parse_rfc3339_timestamp)
            .is_some_and(|from| value < from)
        {
            return false;
        }
        if filter
            .to
            .as_deref()
            .and_then(parse_rfc3339_timestamp)
            .is_some_and(|to| value > to)
        {
            return false;
        }
        true
    })
}

fn has_active_date_filters(filters: &[SearchDateFilter]) -> bool {
    filters.iter().any(|filter| {
        !filter.key.trim().is_empty() && (filter.from.is_some() || filter.to.is_some())
    })
}

fn parse_rfc3339_timestamp(value: &str) -> Option<OffsetDateTime> {
    OffsetDateTime::parse(value, &Rfc3339).ok()
}

fn is_or_match_mode(match_mode: Option<&str>) -> bool {
    matches!(
        match_mode.map(|value| value.trim().to_lowercase()),
        Some(value) if matches!(value.as_str(), "or" | "any" | "some")
    )
}

fn sort_search_hits(results: &mut [SearchHit], sort: Option<&SearchSort>) {
    let Some(sort) = sort else {
        return;
    };
    let field = sort.field.trim();
    let normalized_field = field.to_lowercase();
    if normalized_field == "random" {
        sort_by_random_key(
            results,
            |hit| &hit.path,
            sort.direction.trim().eq_ignore_ascii_case("desc"),
        );
        return;
    }
    let descending = sort.direction.trim().eq_ignore_ascii_case("desc");
    results.sort_by(|left, right| {
        let ordering =
            compare_sort_field(
                field,
                &left.metadata,
                &right.metadata,
                || match normalized_field.as_str() {
                    "filename" | "name" => left
                        .filename
                        .to_lowercase()
                        .cmp(&right.filename.to_lowercase()),
                    "path" => left.path.to_lowercase().cmp(&right.path.to_lowercase()),
                    "rating" => metadata_sort_number(&left.metadata, "rating")
                        .partial_cmp(&metadata_sort_number(&right.metadata, "rating"))
                        .unwrap_or(std::cmp::Ordering::Equal),
                    _ => left.path.to_lowercase().cmp(&right.path.to_lowercase()),
                },
            );
        if descending {
            ordering.reverse()
        } else {
            ordering
        }
    });
}

fn normalize_smart_folder_filter(filter: SmartFolderFilter) -> SmartFolderFilter {
    SmartFolderFilter {
        query: normalize_optional_text(filter.query),
        path_prefix: normalize_optional_path_prefix(filter.path_prefix),
        exclude_query: normalize_optional_text(filter.exclude_query),
        exclude_path_prefixes: normalize_optional_path_values(filter.exclude_path_prefixes),
        tags: normalize_optional_values(filter.tags),
        formats: normalize_optional_values(filter.formats).map(|items| {
            items
                .into_iter()
                .map(|item| item.to_lowercase())
                .collect::<Vec<_>>()
        }),
        colors: normalize_optional_values(filter.colors),
        shapes: normalize_optional_values(filter.shapes),
        metadata_filters: normalize_metadata_filter_values(filter.metadata_filters),
        exclude_tags: normalize_optional_values(filter.exclude_tags),
        exclude_formats: normalize_optional_values(filter.exclude_formats).map(|items| {
            items
                .into_iter()
                .map(|item| item.to_lowercase())
                .collect::<Vec<_>>()
        }),
        exclude_metadata_filters: normalize_metadata_filter_values(filter.exclude_metadata_filters),
        exclude_number_filters: normalize_number_filter_values(filter.exclude_number_filters),
        exclude_date_filters: normalize_date_filter_values(filter.exclude_date_filters),
        number_filters: normalize_number_filter_values(filter.number_filters),
        date_filters: normalize_date_filter_values(filter.date_filters),
        min_rating: filter.min_rating.filter(|value| *value > 0.0),
        match_mode: normalize_match_mode(filter.match_mode),
        sort: normalize_search_sort(filter.sort),
        limit: filter.limit.filter(|value| *value > 0),
    }
}

fn normalize_optional_text(value: Option<String>) -> Option<String> {
    value
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
}

fn normalize_optional_values(values: Option<Vec<String>>) -> Option<Vec<String>> {
    let normalized = values
        .unwrap_or_default()
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn normalize_metadata_filter_values(
    filters: Option<Vec<SearchMetadataFilter>>,
) -> Option<Vec<SearchMetadataFilter>> {
    let normalized = filters
        .unwrap_or_default()
        .into_iter()
        .filter_map(|filter| {
            let key = filter.key.trim();
            let value = filter.value.trim();
            if key.is_empty() || value.is_empty() {
                return None;
            }
            Some(SearchMetadataFilter {
                key: key.to_string(),
                value: value.to_string(),
            })
        })
        .collect::<Vec<_>>();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn normalize_number_filter_values(
    filters: Option<Vec<SearchNumberFilter>>,
) -> Option<Vec<SearchNumberFilter>> {
    let normalized = filters
        .unwrap_or_default()
        .into_iter()
        .filter_map(|filter| {
            let key = filter.key.trim().to_string();
            if key.is_empty() || (filter.min.is_none() && filter.max.is_none()) {
                return None;
            }
            Some(SearchNumberFilter {
                key,
                min: filter.min,
                max: filter.max,
            })
        })
        .collect::<Vec<_>>();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn normalize_date_filter_values(
    filters: Option<Vec<SearchDateFilter>>,
) -> Option<Vec<SearchDateFilter>> {
    let normalized = filters
        .unwrap_or_default()
        .into_iter()
        .filter_map(|filter| {
            let key = filter.key.trim().to_string();
            let from = normalize_optional_text(filter.from);
            let to = normalize_optional_text(filter.to);
            if key.is_empty() || (from.is_none() && to.is_none()) {
                return None;
            }
            Some(SearchDateFilter { key, from, to })
        })
        .collect::<Vec<_>>();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn normalize_match_mode(value: Option<String>) -> Option<String> {
    let value = value?.trim().to_lowercase();
    match value.as_str() {
        "or" | "any" | "some" => Some("or".to_string()),
        "and" | "all" => Some("and".to_string()),
        _ => None,
    }
}

fn normalize_search_sort(sort: Option<SearchSort>) -> Option<SearchSort> {
    let sort = sort?;
    let field = sort.field.trim().to_string();
    if field.is_empty() {
        return None;
    }
    let direction = if sort.direction.trim().eq_ignore_ascii_case("desc") {
        "desc"
    } else {
        "asc"
    };
    Some(SearchSort {
        field,
        direction: direction.to_string(),
    })
}

fn normalize_optional_path_prefix(value: Option<String>) -> Option<String> {
    value
        .and_then(|path| normalize_directory_path(&path).ok())
        .filter(|path| !path.is_empty())
}

fn normalize_optional_path_values(values: Option<Vec<String>>) -> Option<Vec<String>> {
    let normalized = values
        .unwrap_or_default()
        .into_iter()
        .filter_map(|path| normalize_directory_path(&path).ok())
        .filter(|path| !path.is_empty())
        .collect::<Vec<_>>();
    empty_vec_to_none(normalized)
}

fn normalized_optional_id(value: Option<&str>) -> Option<String> {
    value
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
}

fn validate_smart_folder_name(name: &str) -> Result<String, String> {
    let value = name.trim();
    if value.is_empty() {
        return Err("smart folder name cannot be empty".to_string());
    }
    if value.contains('/') || value.contains('\\') {
        return Err("smart folder name cannot contain path separators".to_string());
    }
    Ok(value.to_string())
}

fn validate_smart_folder_id(id: &str) -> Result<String, String> {
    let value = id.trim();
    if value.is_empty() {
        return Err("smart folder id cannot be empty".to_string());
    }
    Ok(slugify_ascii_component(value))
}

fn smart_folder_id_for(repo_id: &str, parent_id: Option<&str>, name: &str) -> String {
    slugify_ascii_component(&format!(
        "smart-{repo_id}-{}-{name}-{}",
        parent_id.unwrap_or("root"),
        now_rfc3339()
    ))
}

fn smart_folder_filter_metadata_filters(filter: &SmartFolderFilter) -> Vec<SearchMetadataFilter> {
    let mut filters = filter.metadata_filters.clone().unwrap_or_default();
    filters.extend(
        filter
            .colors
            .clone()
            .unwrap_or_default()
            .into_iter()
            .map(|value| SearchMetadataFilter {
                key: "color".to_string(),
                value,
            }),
    );
    filters.extend(
        filter
            .shapes
            .clone()
            .unwrap_or_default()
            .into_iter()
            .map(|value| SearchMetadataFilter {
                key: "shape".to_string(),
                value,
            }),
    );
    filters
}

fn merge_smart_folder_filters(
    parent: SmartFolderFilter,
    child: &SmartFolderFilter,
) -> SmartFolderFilter {
    let mut metadata_filters = parent.metadata_filters.unwrap_or_default();
    metadata_filters.extend(child.metadata_filters.clone().unwrap_or_default());
    let mut exclude_metadata_filters = parent.exclude_metadata_filters.unwrap_or_default();
    exclude_metadata_filters.extend(child.exclude_metadata_filters.clone().unwrap_or_default());
    let mut exclude_number_filters = parent.exclude_number_filters.unwrap_or_default();
    exclude_number_filters.extend(child.exclude_number_filters.clone().unwrap_or_default());
    let mut exclude_date_filters = parent.exclude_date_filters.unwrap_or_default();
    exclude_date_filters.extend(child.exclude_date_filters.clone().unwrap_or_default());
    let mut exclude_path_prefixes = parent.exclude_path_prefixes.unwrap_or_default();
    exclude_path_prefixes.extend(child.exclude_path_prefixes.clone().unwrap_or_default());
    let mut number_filters = parent.number_filters.unwrap_or_default();
    number_filters.extend(child.number_filters.clone().unwrap_or_default());
    let mut date_filters = parent.date_filters.unwrap_or_default();
    date_filters.extend(child.date_filters.clone().unwrap_or_default());
    let mut colors = parent.colors.unwrap_or_default();
    colors.extend(child.colors.clone().unwrap_or_default());
    let mut shapes = parent.shapes.unwrap_or_default();
    shapes.extend(child.shapes.clone().unwrap_or_default());
    SmartFolderFilter {
        query: match (parent.query, child.query.clone()) {
            (Some(left), Some(right)) => Some(format!("{left}\n{right}")),
            (Some(left), None) => Some(left),
            (None, Some(right)) => Some(right),
            (None, None) => None,
        },
        path_prefix: merge_path_prefix(parent.path_prefix, child.path_prefix.clone()),
        exclude_query: match (parent.exclude_query, child.exclude_query.clone()) {
            (Some(left), Some(right)) => Some(format!("{left}\n{right}")),
            (Some(left), None) => Some(left),
            (None, Some(right)) => Some(right),
            (None, None) => None,
        },
        exclude_path_prefixes: empty_vec_to_none(exclude_path_prefixes),
        tags: merge_optional_lists(parent.tags, child.tags.clone()),
        formats: merge_optional_lists(parent.formats, child.formats.clone()),
        colors: empty_vec_to_none(colors),
        shapes: empty_vec_to_none(shapes),
        metadata_filters: empty_vec_to_none(metadata_filters),
        exclude_tags: merge_optional_lists(parent.exclude_tags, child.exclude_tags.clone()),
        exclude_formats: merge_optional_lists(
            parent.exclude_formats,
            child.exclude_formats.clone(),
        ),
        exclude_metadata_filters: empty_vec_to_none(exclude_metadata_filters),
        exclude_number_filters: empty_vec_to_none(exclude_number_filters),
        exclude_date_filters: empty_vec_to_none(exclude_date_filters),
        number_filters: empty_vec_to_none(number_filters),
        date_filters: empty_vec_to_none(date_filters),
        min_rating: match (parent.min_rating, child.min_rating) {
            (Some(left), Some(right)) => Some(left.max(right)),
            (Some(left), None) => Some(left),
            (None, Some(right)) => Some(right),
            (None, None) => None,
        },
        match_mode: child.match_mode.clone().or(parent.match_mode),
        sort: child.sort.clone().or(parent.sort),
        limit: child.limit.or(parent.limit),
    }
}

fn merge_optional_lists(
    parent: Option<Vec<String>>,
    child: Option<Vec<String>>,
) -> Option<Vec<String>> {
    let mut values = parent.unwrap_or_default();
    values.extend(child.unwrap_or_default());
    empty_vec_to_none(values)
}

fn empty_vec_to_none<T>(values: Vec<T>) -> Option<Vec<T>> {
    if values.is_empty() {
        None
    } else {
        Some(values)
    }
}

fn merge_path_prefix(parent: Option<String>, child: Option<String>) -> Option<String> {
    match (parent, child) {
        (Some(left), Some(right)) if right.starts_with(&format!("{left}/")) || right == left => {
            Some(right)
        }
        (Some(left), Some(right)) if left.starts_with(&format!("{right}/")) || left == right => {
            Some(left)
        }
        (Some(left), Some(right)) => Some(format!("{left}\n{right}")),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

fn load_smart_folder(
    connection: &Connection,
    repo_id: &str,
    smart_folder_id: &str,
) -> Result<Option<SmartFolder>, rusqlite::Error> {
    connection
        .query_row(
            r#"
            SELECT smart_folder_id, repo_id, parent_id, name, filter_json,
                   sort_order, created_at, updated_at
            FROM smart_folders
            WHERE repo_id = ?1 AND smart_folder_id = ?2
            "#,
            params![repo_id, smart_folder_id],
            map_smart_folder_row,
        )
        .optional()
}

fn load_smart_folders(
    connection: &Connection,
    repo_id: &str,
) -> Result<Vec<SmartFolder>, rusqlite::Error> {
    let mut stmt = connection.prepare(
        r#"
        SELECT smart_folder_id, repo_id, parent_id, name, filter_json,
               sort_order, created_at, updated_at
        FROM smart_folders
        WHERE repo_id = ?1
        ORDER BY parent_id IS NOT NULL, parent_id, sort_order, name COLLATE NOCASE
        "#,
    )?;
    let rows = stmt.query_map([repo_id], map_smart_folder_row)?;
    rows.collect::<Result<Vec<_>, _>>()
}

fn map_smart_folder_row(row: &rusqlite::Row<'_>) -> Result<SmartFolder, rusqlite::Error> {
    let filter_json: String = row.get(4)?;
    let filter = serde_json::from_str::<SmartFolderFilter>(&filter_json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(4, Type::Text, Box::new(error))
    })?;
    Ok(SmartFolder {
        smart_folder_id: row.get(0)?,
        repo_id: row.get(1)?,
        parent_id: row.get(2)?,
        name: row.get(3)?,
        filter,
        sort_order: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

fn build_smart_folder_tree(folders: Vec<SmartFolder>) -> Vec<SmartFolderTreeNode> {
    fn build(parent_id: Option<&str>, folders: &[SmartFolder]) -> Vec<SmartFolderTreeNode> {
        folders
            .iter()
            .filter(|folder| folder.parent_id.as_deref() == parent_id)
            .map(|folder| SmartFolderTreeNode {
                folder: folder.clone(),
                children: build(Some(&folder.smart_folder_id), folders),
            })
            .collect()
    }
    build(None, &folders)
}

fn validate_smart_folder_parent(
    connection: &Connection,
    repo_id: &str,
    parent_id: Option<&str>,
    editing_id: Option<&str>,
) -> Result<(), rusqlite::Error> {
    let Some(parent_id) = normalized_optional_id(parent_id) else {
        return Ok(());
    };
    if editing_id == Some(parent_id.as_str()) {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let mut cursor = Some(parent_id);
    while let Some(current_id) = cursor {
        let parent = load_smart_folder(connection, repo_id, &current_id)?
            .ok_or_else(|| rusqlite::Error::QueryReturnedNoRows)?;
        if editing_id == Some(parent.smart_folder_id.as_str()) {
            return Err(rusqlite::Error::InvalidQuery);
        }
        cursor = parent.parent_id;
    }
    Ok(())
}

fn next_smart_folder_sort_order(
    connection: &Connection,
    repo_id: &str,
    parent_id: Option<&str>,
) -> Result<i64, rusqlite::Error> {
    connection.query_row(
        r#"
        SELECT COALESCE(MAX(sort_order), -1) + 1
        FROM smart_folders
        WHERE repo_id = ?1 AND parent_id IS ?2
        "#,
        params![repo_id, normalized_optional_id(parent_id)],
        |row| row.get(0),
    )
}

fn inherited_smart_folder_filter(
    folders: &[SmartFolder],
    smart_folder: &SmartFolder,
) -> SmartFolderFilter {
    let mut chain = Vec::<SmartFolder>::new();
    let mut current = Some(smart_folder.clone());
    while let Some(folder) = current {
        current = folder
            .parent_id
            .as_ref()
            .and_then(|parent_id| {
                folders
                    .iter()
                    .find(|item| &item.smart_folder_id == parent_id)
            })
            .cloned();
        chain.push(folder);
    }
    chain.reverse();
    let mut filter = SmartFolderFilter::default();
    for folder in chain {
        filter = merge_smart_folder_filters(filter, &normalize_smart_folder_filter(folder.filter));
    }
    filter
}

fn smart_folder_filter_matches(
    repo: &RepositorySummary,
    asset: &AssetSummary,
    metadata: &BTreeMap<String, serde_json::Value>,
    filter: &SmartFolderFilter,
) -> bool {
    if asset.status == "deleted" {
        return false;
    }
    let mut include_matches = Vec::new();
    push_path_prefix_match(&mut include_matches, asset, filter.path_prefix.as_deref());
    push_query_match(
        &mut include_matches,
        repo,
        asset,
        metadata,
        filter.query.as_deref(),
    );
    push_format_match(
        &mut include_matches,
        &asset.extension,
        filter.formats.as_ref(),
    );
    push_tag_match(&mut include_matches, &asset.tags, filter.tags.as_ref());
    let metadata_filters = smart_folder_filter_metadata_filters(filter);
    push_metadata_match(&mut include_matches, metadata, Some(&metadata_filters));
    push_number_match(
        &mut include_matches,
        metadata,
        filter.number_filters.as_ref(),
    );
    push_date_match(&mut include_matches, metadata, filter.date_filters.as_ref());
    push_rating_match(&mut include_matches, metadata, filter.min_rating);

    combine_include_matches(&include_matches, filter.match_mode.as_deref())
        && !matches_excluded_filters(
            repo,
            asset,
            &asset.tags,
            &asset.extension,
            metadata,
            filter.exclude_query.as_deref(),
            filter.exclude_path_prefixes.as_ref(),
            filter.exclude_tags.as_ref(),
            filter.exclude_formats.as_ref(),
            filter.exclude_metadata_filters.as_ref(),
            filter.exclude_number_filters.as_ref(),
            filter.exclude_date_filters.as_ref(),
        )
}

fn query_smart_folder_entries(
    connection: &Connection,
    repo: &RepositorySummary,
    filter: &SmartFolderFilter,
    asset_map: &BTreeMap<String, AssetPathRecord>,
) -> Result<Vec<FileBrowserEntry>, rusqlite::Error> {
    let assets = load_assets(connection, &repo.repo_id)?;
    let asset_ids = assets
        .iter()
        .map(|asset| asset.asset_id.clone())
        .collect::<Vec<_>>();
    let alias_paths_by_asset = load_alias_paths_for_assets(connection, &repo.repo_id, &asset_ids)?;
    let mut results = Vec::new();
    for asset in assets {
        let metadata = load_metadata_map(connection, &asset.asset_id)?;
        if !smart_folder_filter_matches(repo, &asset, &metadata, filter) {
            continue;
        }
        let asset_record = asset_map.get(&asset.path);
        results.push(FileBrowserEntry {
            path: asset.path.clone(),
            name: asset.filename.clone(),
            kind: "file".to_string(),
            extension: Some(asset.extension.clone()),
            size_bytes: Some(asset.size_bytes),
            size_label: Some(asset.size_label.clone()),
            modified_at: Some(asset.modified_at.clone()),
            asset_id: Some(asset.asset_id.clone()),
            status: Some(asset.status.clone()),
            thumbnail_path: asset_record
                .and_then(|record| record.thumbnail_path.clone())
                .or(asset.thumbnail_path.clone()),
            thumbnail_custom: false,
            hardlink_group_id: asset_record.and_then(|record| record.hardlink_group_id.clone()),
            hardlink_state: asset_record.and_then(|record| record.hardlink_state.clone()),
            tags: asset.tags.clone(),
            alias_paths: alias_paths_by_asset
                .get(&asset.asset_id)
                .cloned()
                .unwrap_or_default(),
            folder_metadata: None,
            metadata,
            is_virtual: asset.is_virtual,
            provider_id: asset.provider_id.clone(),
            provider_item_id: asset.provider_item_id.clone(),
            source_payload: asset.source_payload.clone(),
            local_absolute_path: asset.local_absolute_path.clone(),
        });
    }
    sort_file_browser_entries(&mut results, filter.sort.as_ref());
    if let Some(limit) = filter.limit.filter(|value| *value > 0) {
        results.truncate(limit);
    }
    Ok(results)
}

fn build_search_haystack(
    repo: &RepositorySummary,
    asset: &AssetSummary,
    metadata: &BTreeMap<String, serde_json::Value>,
) -> String {
    let metadata_values = metadata
        .iter()
        .map(|(key, value)| format!("{key} {}", json_value_to_search_text(value)))
        .collect::<Vec<_>>()
        .join(" ");

    [
        repo.name.as_str(),
        asset.filename.as_str(),
        asset.path.as_str(),
        asset.status.as_str(),
        &asset.tags.join(" "),
        metadata_values.as_str(),
    ]
    .join(" ")
    .to_lowercase()
}

fn sort_file_browser_entries(entries: &mut [FileBrowserEntry], sort: Option<&SearchSort>) {
    let Some(sort) = sort else {
        entries.sort_by(|left, right| left.path.to_lowercase().cmp(&right.path.to_lowercase()));
        return;
    };
    let field = sort.field.trim();
    let normalized_field = field.to_lowercase();
    if normalized_field == "random" {
        sort_by_random_key(
            entries,
            |entry| &entry.path,
            sort.direction.trim().eq_ignore_ascii_case("desc"),
        );
        return;
    }
    let descending = sort.direction.trim().eq_ignore_ascii_case("desc");
    entries.sort_by(|left, right| {
        let ordering =
            compare_sort_field(
                field,
                &left.metadata,
                &right.metadata,
                || match normalized_field.as_str() {
                    "filename" | "name" => left.name.to_lowercase().cmp(&right.name.to_lowercase()),
                    "size" | "sizebytes" => left.size_bytes.cmp(&right.size_bytes),
                    "modified" | "modifiedat" => left.modified_at.cmp(&right.modified_at),
                    "rating" => metadata_sort_number(&left.metadata, "rating")
                        .partial_cmp(&metadata_sort_number(&right.metadata, "rating"))
                        .unwrap_or(std::cmp::Ordering::Equal),
                    _ => left.path.to_lowercase().cmp(&right.path.to_lowercase()),
                },
            );
        if descending {
            ordering.reverse()
        } else {
            ordering
        }
    });
}

fn sort_by_random_key<T>(items: &mut [T], key: impl Fn(&T) -> &str, descending: bool) {
    use std::collections::hash_map::DefaultHasher;

    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(0);
    items.sort_by(|left, right| {
        let mut left_hasher = DefaultHasher::new();
        seed.hash(&mut left_hasher);
        key(left).hash(&mut left_hasher);
        let mut right_hasher = DefaultHasher::new();
        seed.hash(&mut right_hasher);
        key(right).hash(&mut right_hasher);
        let ordering = left_hasher.finish().cmp(&right_hasher.finish());
        if descending {
            ordering.reverse()
        } else {
            ordering
        }
    });
}

fn metadata_sort_number(metadata: &BTreeMap<String, serde_json::Value>, key: &str) -> f64 {
    metadata
        .get(key)
        .and_then(|value| value.as_f64())
        .unwrap_or(0.0)
}

fn metadata_sort_field_key(field: &str) -> Option<&str> {
    const PREFIX: &str = "metadata.";
    if field.len() > PREFIX.len() && field[..PREFIX.len()].eq_ignore_ascii_case(PREFIX) {
        Some(&field[PREFIX.len()..])
    } else {
        None
    }
}

fn compare_sort_field(
    field: &str,
    left_metadata: &BTreeMap<String, serde_json::Value>,
    right_metadata: &BTreeMap<String, serde_json::Value>,
    fallback: impl FnOnce() -> std::cmp::Ordering,
) -> std::cmp::Ordering {
    if let Some(metadata_key) = metadata_sort_field_key(field) {
        compare_metadata_values(left_metadata, right_metadata, metadata_key)
    } else {
        fallback()
    }
}

fn compare_metadata_values(
    left: &BTreeMap<String, serde_json::Value>,
    right: &BTreeMap<String, serde_json::Value>,
    key: &str,
) -> std::cmp::Ordering {
    compare_optional_json_values(left.get(key), right.get(key))
}

fn compare_optional_json_values(
    left: Option<&serde_json::Value>,
    right: Option<&serde_json::Value>,
) -> std::cmp::Ordering {
    match (left, right) {
        (Some(left), Some(right)) => compare_json_values(left, right),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

fn compare_json_values(left: &serde_json::Value, right: &serde_json::Value) -> std::cmp::Ordering {
    if let (Some(left), Some(right)) = (json_value_to_f64(left), json_value_to_f64(right)) {
        return left
            .partial_cmp(&right)
            .unwrap_or(std::cmp::Ordering::Equal);
    }
    if let (Some(left), Some(right)) = (
        json_value_to_timestamp(left),
        json_value_to_timestamp(right),
    ) {
        return left.cmp(&right);
    }
    json_value_to_search_text(left)
        .to_lowercase()
        .cmp(&json_value_to_search_text(right).to_lowercase())
}

fn json_value_to_f64(value: &serde_json::Value) -> Option<f64> {
    value.as_f64().or_else(|| {
        value
            .as_str()
            .map(str::trim)
            .and_then(|value| value.parse::<f64>().ok())
    })
}

fn json_value_to_timestamp(value: &serde_json::Value) -> Option<OffsetDateTime> {
    value
        .as_str()
        .map(str::trim)
        .and_then(parse_rfc3339_timestamp)
}

fn json_value_to_search_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(value) => value.clone(),
        _ => value.to_string(),
    }
}

fn infer_value_type(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::String(_) => "string",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::Bool(_) => "boolean",
        _ => "json",
    }
}

fn parse_json_column(value_json: &str) -> Result<serde_json::Value, rusqlite::Error> {
    serde_json::from_str(value_json)
        .map_err(|error| rusqlite::Error::FromSqlConversionFailure(0, Type::Text, Box::new(error)))
}

fn parse_json_column_optional(
    value_json: Option<String>,
) -> Result<serde_json::Value, rusqlite::Error> {
    match value_json {
        Some(value) => parse_json_column(&value),
        None => Ok(serde_json::json!({})),
    }
}

fn parse_json_column_nullable(
    value_json: Option<String>,
) -> Result<Option<serde_json::Value>, rusqlite::Error> {
    match value_json {
        Some(value) => Ok(Some(parse_json_column(&value)?)),
        None => Ok(None),
    }
}

fn metadata_defaults_for_files(
    service_root: &Path,
    files: &[DiscoveredFile],
    existing_metadata_by_path: &BTreeMap<String, BTreeMap<String, serde_json::Value>>,
) -> Result<BTreeMap<String, BTreeMap<String, serde_json::Value>>, String> {
    if files.is_empty() {
        return Ok(BTreeMap::new());
    }

    let registry = backend_plugin_registry(service_root);
    let providers = registry.metadata_default_providers();
    if providers.is_empty() {
        return Ok(BTreeMap::new());
    }

    let entries = files
        .iter()
        .map(|file| MetadataDefaultsBatchEntry {
            path: file.relative_path.clone(),
            name: file.filename.clone(),
            extension: file.extension.clone(),
            kind: "file".to_string(),
            metadata: existing_metadata_by_path.get(&file.relative_path).cloned(),
        })
        .collect::<Vec<_>>();
    let payload = serde_json::json!({ "entries": entries });
    let mut defaults_by_path = BTreeMap::<String, BTreeMap<String, serde_json::Value>>::new();

    for (plugin_id, action) in providers {
        let response = registry.call(&plugin_id, &action, payload.clone())?;
        let parsed = serde_json::from_value::<MetadataDefaultsBatchResponse>(response)
            .map_err(json_error)?;
        for (path, defaults) in parsed.defaults_by_path {
            if !files.iter().any(|file| file.relative_path == path) {
                continue;
            }
            defaults_by_path.entry(path).or_default().extend(defaults);
        }
    }

    Ok(defaults_by_path)
}

fn sync_repository_files(
    service_root: &Path,
    tx: &Transaction<'_>,
    repo: &RepositoryRecord,
    skip_hardlink_candidate_paths: &HashSet<String>,
) -> Result<SyncResult, rusqlite::Error> {
    let repo_root = PathBuf::from(&repo.summary.path);
    let files = list_backend_files(service_root, repo, &repo_root).map_err(|error| {
        rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
            std::io::ErrorKind::Other,
            error,
        )))
    })?;

    let mut existing_stmt = tx.prepare(
        r#"
        SELECT
          asset_id,
          path,
          status,
          thumbnail_path,
          size_bytes,
          created_at,
          modified_at,
          hash,
          is_virtual,
          provider_id,
          provider_item_id,
          source_payload_json,
          local_absolute_path
        FROM assets
        WHERE repo_id = ?1
        "#,
    )?;
    let existing_rows = existing_stmt.query_map([repo.summary.repo_id.as_str()], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            ExistingAssetRecord {
                asset_id: row.get::<_, String>(0)?,
                status: row.get::<_, String>(2)?,
                thumbnail_path: row.get::<_, Option<String>>(3)?,
                size_bytes: row.get::<_, i64>(4)?,
                created_at: row.get::<_, String>(5)?,
                modified_at: row.get::<_, String>(6)?,
                hash: row.get::<_, Option<String>>(7)?,
                is_virtual: row.get::<_, i64>(8)? != 0,
                provider_id: row.get::<_, Option<String>>(9)?,
                provider_item_id: row.get::<_, Option<String>>(10)?,
                source_payload: parse_json_column_nullable(row.get::<_, Option<String>>(11)?)?,
                local_absolute_path: row.get::<_, Option<String>>(12)?,
            },
        ))
    })?;
    let existing = existing_rows.collect::<Result<Vec<_>, _>>()?;
    let existing_asset_ids = existing
        .iter()
        .map(|(_asset_id, _path, record)| record.asset_id.clone())
        .collect::<Vec<_>>();
    let existing_metadata_by_asset_id = load_metadata_maps_for_assets(tx, &existing_asset_ids)?;
    let existing_metadata_by_path = existing
        .iter()
        .filter_map(|(_asset_id, path, record)| {
            existing_metadata_by_asset_id
                .get(&record.asset_id)
                .cloned()
                .map(|metadata| (path.clone(), metadata))
        })
        .collect::<BTreeMap<_, _>>();
    let plugin_defaults_by_path =
        metadata_defaults_for_files(service_root, &files, &existing_metadata_by_path).map_err(
            |error| {
                rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    error,
                )))
            },
        )?;
    let mut existing_by_path = existing
        .into_iter()
        .map(|(_asset_id, path, record)| (path, record))
        .collect::<BTreeMap<_, _>>();

    let now = now_rfc3339();
    let mut created_assets = 0_i64;
    let mut updated_assets = 0_i64;
    let mut deleted_assets = 0_i64;
    let mut created_events = 0_i64;

    for file in &files {
        if let Some(existing_record) = existing_by_path.remove(&file.relative_path) {
            let asset_id = existing_record.asset_id;
            let asset_created_at = existing_record.created_at.clone();
            let content_hash = if file.is_virtual {
                existing_record.hash.unwrap_or_default()
            } else if existing_record.size_bytes == file.size_bytes
                && existing_record.modified_at == file.modified_at
            {
                match existing_record.hash.filter(|hash| is_content_hash(hash)) {
                    Some(hash) => hash,
                    None => file_sha256_hash(file.absolute_path.as_deref().ok_or_else(|| {
                        rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            "missing absolute path for non-virtual file",
                        )))
                    })?)
                    .map_err(|error| {
                        rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            error,
                        )))
                    })?,
                }
            } else {
                file_sha256_hash(file.absolute_path.as_deref().ok_or_else(|| {
                    rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "missing absolute path for non-virtual file",
                    )))
                })?)
                .map_err(|error| {
                    rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        error,
                    )))
                })?
            };
            tx.execute(
                r#"
                UPDATE assets
                SET filename = ?3, extension = ?4, size_bytes = ?5, modified_at = ?6, hash = ?7,
                    status = 'synced', updated_at = ?8, thumbnail_path = ?9, is_virtual = ?10,
                    provider_id = ?11, provider_item_id = ?12, source_payload_json = ?13, local_absolute_path = ?14
                WHERE repo_id = ?1 AND asset_id = ?2
                "#,
                params![
                    repo.summary.repo_id,
                    asset_id,
                    file.filename,
                    file.extension,
                    file.size_bytes,
                    file.modified_at,
                    if content_hash.is_empty() { None } else { Some(content_hash.as_str()) },
                    now,
                    existing_record.thumbnail_path,
                    if file.is_virtual { 1 } else { 0 },
                    file.provider_id,
                    file.provider_item_id,
                    file.source_payload.as_ref().map(|value| value.to_string()),
                    file.local_absolute_path
                ],
            )?;
            if existing_record.status == "deleted" {
                created_events += 1;
            }
            if !file.is_virtual && !content_hash.is_empty() {
                update_hardlink_member_verification(
                    tx,
                    &repo.summary.repo_id,
                    &asset_id,
                    &file.relative_path,
                    &content_hash,
                )?;
            }
            ensure_default_metadata(
                tx,
                &asset_id,
                &file.relative_path,
                &file.filename,
                &file.extension,
                &asset_created_at,
                file.created_at.as_deref(),
                &[],
                plugin_defaults_by_path.get(&file.relative_path),
                false,
            )?;
            updated_assets += 1;
            insert_event(
                tx,
                &repo.summary,
                &asset_id,
                "asset.scanned",
                &file.relative_path,
                serde_json::json!({
                    "sizeBytes": file.size_bytes,
                    "modifiedAt": file.modified_at
                }),
            )?;
            created_events += 1;
        } else {
            let asset_id = asset_id_for_path(&repo.summary.repo_id, &file.relative_path);
            let content_hash = if file.is_virtual {
                String::new()
            } else {
                file_sha256_hash(file.absolute_path.as_deref().ok_or_else(|| {
                    rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "missing absolute path for non-virtual file",
                    )))
                })?)
                .map_err(|error| {
                    rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        error,
                    )))
                })?
            };
            tx.execute(
                r#"
                INSERT INTO assets (
                  asset_id, repo_id, path, filename, extension, size_bytes,
                  created_at, modified_at, hash, status, version, updated_at, thumbnail_path,
                  is_virtual, provider_id, provider_item_id, source_payload_json, local_absolute_path
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'synced', 1, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
                "#,
                params![
                    asset_id,
                    repo.summary.repo_id,
                    file.relative_path,
                    file.filename,
                    file.extension,
                    file.size_bytes,
                    now,
                    file.modified_at,
                    content_hash,
                    now,
                    Option::<String>::None,
                    if file.is_virtual { 1 } else { 0 },
                    file.provider_id,
                    file.provider_item_id,
                    file.source_payload.as_ref().map(|value| value.to_string()),
                    file.local_absolute_path
                ],
            )?;
            if !file.is_virtual && !skip_hardlink_candidate_paths.contains(&file.relative_path) {
                record_hardlink_candidate_for_new_asset(
                    tx,
                    &repo.summary.repo_id,
                    &asset_id,
                    &file.relative_path,
                    &content_hash,
                    file.size_bytes,
                )?;
            }
            let palette = if file.is_virtual {
                Vec::new()
            } else {
                extract_image_palette(
                    file.absolute_path.as_deref().ok_or_else(|| {
                        rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            "missing absolute path for non-virtual file",
                        )))
                    })?,
                    &file.extension,
                )
            };
            insert_default_metadata(
                tx,
                &asset_id,
                &file.relative_path,
                &file.filename,
                &file.extension,
                &now,
                file.created_at.as_deref(),
                &palette,
                plugin_defaults_by_path.get(&file.relative_path),
            )?;
            insert_event(
                tx,
                &repo.summary,
                &asset_id,
                "asset.created",
                &file.relative_path,
                serde_json::json!({
                    "origin": "scan"
                }),
            )?;
            created_assets += 1;
            created_events += 1;
        }
    }

    for (path, record) in existing_by_path {
        if record.status == "deleted" {
            continue;
        }
        tx.execute(
            r#"
            UPDATE assets
            SET status = 'deleted', updated_at = ?3
            WHERE repo_id = ?1 AND asset_id = ?2
            "#,
            params![repo.summary.repo_id, record.asset_id, now],
        )?;
        mark_hardlink_member_missing(tx, &repo.summary.repo_id, &record.asset_id)?;
        insert_event(
            tx,
            &repo.summary,
            &record.asset_id,
            "asset.deleted",
            &path,
            serde_json::json!({
                "origin": "scan"
            }),
        )?;
        deleted_assets += 1;
        created_events += 1;
    }

    let hardlink_candidates =
        count_pending_hardlink_candidates(tx, &repo.summary.repo_id).unwrap_or(0);

    Ok(SyncResult {
        repo_id: repo.summary.repo_id.clone(),
        scanned_files: files.len() as i64,
        created_assets,
        updated_assets,
        deleted_assets,
        created_events,
        hardlink_candidates,
    })
}

fn apply_revision_state(
    tx: &Transaction<'_>,
    repo_id: &str,
    asset_id: &str,
    target: &serde_json::Value,
    operation: &str,
    source: &str,
) -> Result<(), rusqlite::Error> {
    let target_map = target
        .as_object()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .collect::<BTreeMap<_, _>>();
    let before = load_metadata_map_from_transaction(tx, asset_id)?;
    let now = now_rfc3339();

    tx.execute("DELETE FROM metadata WHERE asset_id = ?1", [asset_id])?;
    for (key, value) in &target_map {
        tx.execute(
            r#"
            INSERT INTO metadata (asset_id, key, value_type, value_json, version, updated_at)
            VALUES (?1, ?2, ?3, ?4, 1, ?5)
            "#,
            params![
                asset_id,
                key,
                infer_value_type(value),
                value.to_string(),
                now
            ],
        )?;
    }

    let next_version: i64 = tx.query_row(
        "SELECT version + 1 FROM assets WHERE repo_id = ?1 AND asset_id = ?2",
        params![repo_id, asset_id],
        |row| row.get(0),
    )?;

    tx.execute(
        "UPDATE assets SET version = ?3, updated_at = ?4, modified_at = ?4 WHERE repo_id = ?1 AND asset_id = ?2",
        params![repo_id, asset_id, next_version, now],
    )?;
    tx.execute(
        r#"
        INSERT INTO revisions (
          revision_id, repo_id, asset_id, timestamp, operation, before_json, after_json, source
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        "#,
        params![
            format!("rev-{}-{}", asset_id, next_version),
            repo_id,
            asset_id,
            now,
            operation,
            serde_json::to_string(&before)
                .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?,
            serde_json::to_string(&target_map)
                .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?,
            source
        ],
    )?;

    Ok(())
}

fn load_latest_revision(
    tx: &Transaction<'_>,
    asset_id: &str,
) -> Result<Option<RevisionEntry>, rusqlite::Error> {
    tx.query_row(
        r#"
        SELECT revision_id, asset_id, timestamp, operation, before_json, after_json, source
        FROM revisions
        WHERE asset_id = ?1
        ORDER BY timestamp DESC
        LIMIT 1
        "#,
        [asset_id],
        |row| {
            let before_json: Option<String> = row.get(4)?;
            let after_json: Option<String> = row.get(5)?;
            Ok(RevisionEntry {
                revision_id: row.get(0)?,
                asset_id: row.get(1)?,
                timestamp: row.get(2)?,
                operation: row.get(3)?,
                before: parse_json_column_optional(before_json)?,
                after: parse_json_column_optional(after_json)?,
                source: row.get(6)?,
            })
        },
    )
    .optional()
}

fn insert_event(
    tx: &Transaction<'_>,
    repo: &RepositorySummary,
    asset_id: &str,
    event_type: &str,
    path: &str,
    payload: serde_json::Value,
) -> Result<(), rusqlite::Error> {
    let event_id = format!(
        "evt-{}-{}",
        event_type.replace('.', "-"),
        slugify_repo_id(asset_id, path)
    );
    tx.execute(
        r#"
        INSERT OR REPLACE INTO events (event_id, repo_id, asset_id, event_type, path, payload_json, created_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        "#,
        params![
            event_id,
            repo.repo_id,
            asset_id,
            event_type,
            path,
            payload.to_string(),
            now_rfc3339()
        ],
    )?;
    Ok(())
}

fn insert_default_metadata(
    tx: &Transaction<'_>,
    asset_id: &str,
    relative_path: &str,
    filename: &str,
    extension: &str,
    added_to_library_at: &str,
    file_created_at: Option<&str>,
    palette: &[String],
    plugin_defaults: Option<&BTreeMap<String, serde_json::Value>>,
) -> Result<(), rusqlite::Error> {
    ensure_default_metadata(
        tx,
        asset_id,
        relative_path,
        filename,
        extension,
        added_to_library_at,
        file_created_at,
        palette,
        plugin_defaults,
        true,
    )
}

fn ensure_default_metadata(
    tx: &Transaction<'_>,
    asset_id: &str,
    _relative_path: &str,
    filename: &str,
    extension: &str,
    added_to_library_at: &str,
    file_created_at: Option<&str>,
    palette: &[String],
    plugin_defaults: Option<&BTreeMap<String, serde_json::Value>>,
    overwrite_existing: bool,
) -> Result<(), rusqlite::Error> {
    let mut defaults = vec![
        (
            "title".to_string(),
            serde_json::Value::String(filename.to_string()),
        ),
        ("favorite".to_string(), serde_json::Value::Bool(false)),
        (
            "type".to_string(),
            serde_json::Value::String(extension.to_string()),
        ),
        ("rating".to_string(), serde_json::json!(0)),
        (
            "comment".to_string(),
            serde_json::Value::String(String::new()),
        ),
        ("link".to_string(), serde_json::Value::String(String::new())),
        ("tagGroups".to_string(), serde_json::json!([])),
        (
            "addedToLibraryAt".to_string(),
            serde_json::Value::String(added_to_library_at.to_string()),
        ),
    ];
    if let Some(file_created_at) = file_created_at {
        defaults.push((
            "fileCreatedAt".to_string(),
            serde_json::Value::String(file_created_at.to_string()),
        ));
    }
    if let Some(primary_color) = palette.first() {
        defaults.push((
            "color".to_string(),
            serde_json::Value::String(primary_color.clone()),
        ));
        defaults.push(("palette".to_string(), serde_json::json!(palette)));
    }
    for (key, value) in defaults {
        if overwrite_existing {
            tx.execute(
                r#"
                INSERT OR REPLACE INTO metadata (asset_id, key, value_type, value_json, version, updated_at)
                VALUES (?1, ?2, ?3, ?4, 1, ?5)
                "#,
                params![
                    asset_id,
                    key,
                    infer_value_type(&value),
                    value.to_string(),
                    added_to_library_at
                ],
            )?;
        } else {
            tx.execute(
                r#"
                INSERT OR IGNORE INTO metadata (asset_id, key, value_type, value_json, version, updated_at)
                VALUES (?1, ?2, ?3, ?4, 1, ?5)
                "#,
                params![
                    asset_id,
                    key,
                    infer_value_type(&value),
                    value.to_string(),
                    added_to_library_at
                ],
            )?;
        }
    }
    if let Some(plugin_defaults) = plugin_defaults {
        for (key, value) in plugin_defaults {
            tx.execute(
                r#"
                INSERT OR IGNORE INTO metadata (asset_id, key, value_type, value_json, version, updated_at)
                VALUES (?1, ?2, ?3, ?4, 1, ?5)
                "#,
                params![
                    asset_id,
                    key,
                    infer_value_type(value),
                    value.to_string(),
                    added_to_library_at
                ],
            )?;
        }
    }

    Ok(())
}

fn upsert_metadata_value(
    connection: &Connection,
    asset_id: &str,
    key: &str,
    value: &serde_json::Value,
) -> Result<(), rusqlite::Error> {
    let now = now_rfc3339();
    connection.execute(
        r#"
        INSERT INTO metadata (asset_id, key, value_type, value_json, version, updated_at)
        VALUES (?1, ?2, ?3, ?4, 1, ?5)
        ON CONFLICT(asset_id, key)
        DO UPDATE SET
          value_type = excluded.value_type,
          value_json = excluded.value_json,
          version = metadata.version + 1,
          updated_at = excluded.updated_at
        "#,
        params![
            asset_id,
            key,
            infer_value_type(value),
            value.to_string(),
            now
        ],
    )?;
    Ok(())
}

fn delete_metadata_value(
    connection: &Connection,
    asset_id: &str,
    key: &str,
) -> Result<(), rusqlite::Error> {
    connection.execute(
        "DELETE FROM metadata WHERE asset_id = ?1 AND key = ?2",
        params![asset_id, key],
    )?;
    Ok(())
}

fn sync_thumbnail_palette_metadata(
    connection: &Connection,
    asset_id: &str,
    thumbnail_path: Option<&str>,
) -> Result<(), rusqlite::Error> {
    let Some(thumbnail_path) = thumbnail_path else {
        return delete_metadata_value(connection, asset_id, "thumbnailPalette");
    };

    let colors = extract_thumbnail_palette(Path::new(thumbnail_path)).map_err(|error| {
        rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
            std::io::ErrorKind::Other,
            error,
        )))
    })?;
    if colors.is_empty() {
        return delete_metadata_value(connection, asset_id, "thumbnailPalette");
    }

    upsert_metadata_value(
        connection,
        asset_id,
        "thumbnailPalette",
        &serde_json::json!(colors),
    )
}

fn extract_thumbnail_palette(path: &Path) -> Result<Vec<String>, String> {
    let image = image::open(path).map_err(|error| format!("thumbnail palette error: {error}"))?;
    let thumbnail = image.thumbnail(48, 48).to_rgb8();
    if thumbnail.width() == 0 || thumbnail.height() == 0 {
        return Ok(Vec::new());
    }

    let mut buckets = HashMap::<u16, (u64, u64, u64, usize)>::new();
    for pixel in thumbnail.pixels() {
        let [r, g, b] = pixel.0;
        let key = (((r as u16) >> 4) << 8) | (((g as u16) >> 4) << 4) | ((b as u16) >> 4);
        let entry = buckets.entry(key).or_insert((0, 0, 0, 0));
        entry.0 += r as u64;
        entry.1 += g as u64;
        entry.2 += b as u64;
        entry.3 += 1;
    }

    let mut ranked = buckets
        .into_values()
        .filter(|(_, _, _, count)| *count > 0)
        .map(|(r, g, b, count)| {
            (
                count,
                (r / count as u64) as u8,
                (g / count as u64) as u8,
                (b / count as u64) as u8,
            )
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| right.0.cmp(&left.0));

    let mut palette = Vec::new();
    for (_, r, g, b) in ranked {
        if palette
            .iter()
            .any(|(pr, pg, pb)| color_distance_sq((r, g, b), (*pr, *pg, *pb)) < 720)
        {
            continue;
        }
        palette.push((r, g, b));
        if palette.len() == 5 {
            break;
        }
    }

    if palette.is_empty() {
        let mut totals = (0_u64, 0_u64, 0_u64, 0_u64);
        for pixel in thumbnail.pixels() {
            let [r, g, b] = pixel.0;
            totals.0 += r as u64;
            totals.1 += g as u64;
            totals.2 += b as u64;
            totals.3 += 1;
        }
        if totals.3 > 0 {
            palette.push((
                (totals.0 / totals.3) as u8,
                (totals.1 / totals.3) as u8,
                (totals.2 / totals.3) as u8,
            ));
        }
    }

    Ok(palette
        .into_iter()
        .map(|(r, g, b)| format!("#{r:02X}{g:02X}{b:02X}"))
        .collect())
}

fn color_distance_sq(left: (u8, u8, u8), right: (u8, u8, u8)) -> i32 {
    let dr = left.0 as i32 - right.0 as i32;
    let dg = left.1 as i32 - right.1 as i32;
    let db = left.2 as i32 - right.2 as i32;
    dr * dr + dg * dg + db * db
}

fn hardlink_group_id_for(repo_id: &str, content_hash: &str, size_bytes: i64) -> String {
    format!(
        "hardlink-{}",
        sha256_hex(&[
            repo_id.as_bytes(),
            content_hash.as_bytes(),
            size_bytes.to_string().as_bytes()
        ])
    )
}

fn hardlink_candidate_id_for(repo_id: &str, new_asset_id: &str, existing_asset_id: &str) -> String {
    format!(
        "hardlink-candidate-{}",
        sha256_hex(&[
            repo_id.as_bytes(),
            new_asset_id.as_bytes(),
            existing_asset_id.as_bytes()
        ])
    )
}

fn ensure_hardlink_group(
    tx: &Transaction<'_>,
    repo_id: &str,
    content_hash: &str,
    size_bytes: i64,
) -> Result<String, rusqlite::Error> {
    let group_id = hardlink_group_id_for(repo_id, content_hash, size_bytes);
    let now = now_rfc3339();
    tx.execute(
        r#"
        INSERT INTO hardlink_groups (group_id, repo_id, content_hash, size_bytes, created_at, updated_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?5)
        ON CONFLICT(repo_id, content_hash, size_bytes)
        DO UPDATE SET updated_at = excluded.updated_at
        "#,
        params![group_id, repo_id, content_hash, size_bytes, now],
    )?;
    tx.query_row(
        r#"
        SELECT group_id
        FROM hardlink_groups
        WHERE repo_id = ?1 AND content_hash = ?2 AND size_bytes = ?3
        "#,
        params![repo_id, content_hash, size_bytes],
        |row| row.get(0),
    )
}

fn upsert_hardlink_member(
    tx: &Transaction<'_>,
    repo_id: &str,
    asset_id: &str,
    path: &str,
    content_hash: &str,
    size_bytes: i64,
    link_state: &str,
) -> Result<(), rusqlite::Error> {
    let group_id = ensure_hardlink_group(tx, repo_id, content_hash, size_bytes)?;
    let now = now_rfc3339();
    tx.execute(
        r#"
        INSERT INTO hardlink_members (group_id, repo_id, asset_id, path, link_state, linked_at, verified_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
        ON CONFLICT(repo_id, asset_id)
        DO UPDATE SET
          group_id = excluded.group_id,
          path = excluded.path,
          link_state = excluded.link_state,
          verified_at = excluded.verified_at
        "#,
        params![group_id, repo_id, asset_id, path, link_state, now],
    )?;
    Ok(())
}

fn update_hardlink_member_verification(
    tx: &Transaction<'_>,
    repo_id: &str,
    asset_id: &str,
    path: &str,
    content_hash: &str,
) -> Result<(), rusqlite::Error> {
    let Some((group_id, expected_hash, current_state)) = tx
        .query_row(
            r#"
            SELECT hm.group_id, hg.content_hash, hm.link_state
            FROM hardlink_members hm
            JOIN hardlink_groups hg ON hg.group_id = hm.group_id
            WHERE hm.repo_id = ?1 AND hm.asset_id = ?2
            "#,
            params![repo_id, asset_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()?
    else {
        return Ok(());
    };
    let state = if expected_hash != content_hash {
        "broken"
    } else if current_state == "copiedFallback" {
        "copiedFallback"
    } else {
        "linked"
    };
    tx.execute(
        r#"
        UPDATE hardlink_members
        SET path = ?4, link_state = ?5, verified_at = ?6
        WHERE repo_id = ?1 AND asset_id = ?2 AND group_id = ?3
        "#,
        params![repo_id, asset_id, group_id, path, state, now_rfc3339()],
    )?;
    Ok(())
}

fn mark_hardlink_member_missing(
    tx: &Transaction<'_>,
    repo_id: &str,
    asset_id: &str,
) -> Result<(), rusqlite::Error> {
    tx.execute(
        r#"
        UPDATE hardlink_members
        SET link_state = 'missing', verified_at = ?3
        WHERE repo_id = ?1 AND asset_id = ?2
        "#,
        params![repo_id, asset_id, now_rfc3339()],
    )?;
    Ok(())
}

fn record_hardlink_candidate_for_new_asset(
    tx: &Transaction<'_>,
    repo_id: &str,
    new_asset_id: &str,
    new_path: &str,
    content_hash: &str,
    size_bytes: i64,
) -> Result<(), rusqlite::Error> {
    let existing = tx
        .query_row(
            r#"
            SELECT asset_id, path
            FROM assets
            WHERE repo_id = ?1
              AND asset_id != ?2
              AND hash = ?3
              AND size_bytes = ?4
              AND status != 'deleted'
            ORDER BY updated_at DESC
            LIMIT 1
            "#,
            params![repo_id, new_asset_id, content_hash, size_bytes],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?;
    let Some((existing_asset_id, existing_path)) = existing else {
        return Ok(());
    };
    let candidate_id = hardlink_candidate_id_for(repo_id, new_asset_id, &existing_asset_id);
    tx.execute(
        r#"
        INSERT OR IGNORE INTO hardlink_candidates (
          candidate_id, repo_id, new_asset_id, new_path, existing_asset_id, existing_path,
          content_hash, size_bytes, created_at
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
        "#,
        params![
            candidate_id,
            repo_id,
            new_asset_id,
            new_path,
            existing_asset_id,
            existing_path,
            content_hash,
            size_bytes,
            now_rfc3339()
        ],
    )?;
    Ok(())
}

fn count_pending_hardlink_candidates(
    tx: &Transaction<'_>,
    repo_id: &str,
) -> Result<i64, rusqlite::Error> {
    tx.query_row(
        r#"
        SELECT COUNT(*)
        FROM hardlink_candidates hc
        JOIN assets new_asset
          ON new_asset.repo_id = hc.repo_id
         AND new_asset.asset_id = hc.new_asset_id
         AND new_asset.path = hc.new_path
         AND new_asset.hash = hc.content_hash
         AND new_asset.size_bytes = hc.size_bytes
         AND new_asset.status != 'deleted'
        JOIN assets existing_asset
          ON existing_asset.repo_id = hc.repo_id
         AND existing_asset.asset_id = hc.existing_asset_id
         AND existing_asset.path = hc.existing_path
         AND existing_asset.hash = hc.content_hash
         AND existing_asset.size_bytes = hc.size_bytes
         AND existing_asset.status != 'deleted'
        WHERE hc.repo_id = ?1
        "#,
        [repo_id],
        |row| row.get(0),
    )
}

fn load_hardlink_asset_for_path(
    tx: &Transaction<'_>,
    repo_id: &str,
    path: &str,
) -> Result<Option<HardlinkAssetRecord>, rusqlite::Error> {
    let record = tx
        .query_row(
            r#"
            SELECT asset_id, hash, size_bytes
            FROM assets
            WHERE repo_id = ?1 AND path = ?2 AND status != 'deleted'
            "#,
            params![repo_id, path],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()?;
    Ok(record.and_then(|(asset_id, hash, size_bytes)| {
        hash.filter(|value| is_content_hash(value))
            .map(|content_hash| HardlinkAssetRecord {
                asset_id,
                content_hash,
                size_bytes,
            })
    }))
}

fn hardlink_outcome_target_paths(outcomes: &[HardlinkCopyOutcome]) -> HashSet<String> {
    outcomes
        .iter()
        .map(|outcome| outcome.target_path.clone())
        .collect()
}

fn load_hardlink_candidates(
    connection: &Connection,
    repo_id: &str,
) -> Result<Vec<HardlinkCandidate>, rusqlite::Error> {
    let mut stmt = connection.prepare(
        r#"
        SELECT hc.candidate_id, hc.repo_id, hc.new_asset_id, hc.new_path,
               hc.existing_asset_id, hc.existing_path, hc.content_hash,
               hc.size_bytes, hc.created_at
        FROM hardlink_candidates hc
        JOIN assets new_asset
          ON new_asset.repo_id = hc.repo_id
         AND new_asset.asset_id = hc.new_asset_id
         AND new_asset.path = hc.new_path
         AND new_asset.hash = hc.content_hash
         AND new_asset.size_bytes = hc.size_bytes
         AND new_asset.status != 'deleted'
        JOIN assets existing_asset
          ON existing_asset.repo_id = hc.repo_id
         AND existing_asset.asset_id = hc.existing_asset_id
         AND existing_asset.path = hc.existing_path
         AND existing_asset.hash = hc.content_hash
         AND existing_asset.size_bytes = hc.size_bytes
         AND existing_asset.status != 'deleted'
        WHERE hc.repo_id = ?1
        ORDER BY hc.created_at ASC
        "#,
    )?;
    let rows = stmt.query_map([repo_id], map_hardlink_candidate_row)?;
    rows.collect::<Result<Vec<_>, _>>()
}

fn load_hardlink_candidate_from_transaction(
    tx: &Transaction<'_>,
    repo_id: &str,
    candidate_id: &str,
) -> Result<Option<HardlinkCandidate>, rusqlite::Error> {
    tx.query_row(
        r#"
        SELECT hc.candidate_id, hc.repo_id, hc.new_asset_id, hc.new_path,
               hc.existing_asset_id, hc.existing_path, hc.content_hash,
               hc.size_bytes, hc.created_at
        FROM hardlink_candidates hc
        JOIN assets new_asset
          ON new_asset.repo_id = hc.repo_id
         AND new_asset.asset_id = hc.new_asset_id
         AND new_asset.path = hc.new_path
         AND new_asset.hash = hc.content_hash
         AND new_asset.size_bytes = hc.size_bytes
         AND new_asset.status != 'deleted'
        JOIN assets existing_asset
          ON existing_asset.repo_id = hc.repo_id
         AND existing_asset.asset_id = hc.existing_asset_id
         AND existing_asset.path = hc.existing_path
         AND existing_asset.hash = hc.content_hash
         AND existing_asset.size_bytes = hc.size_bytes
         AND existing_asset.status != 'deleted'
        WHERE hc.repo_id = ?1 AND hc.candidate_id = ?2
        "#,
        params![repo_id, candidate_id],
        map_hardlink_candidate_row,
    )
    .optional()
}

fn delete_hardlink_candidate(
    tx: &Transaction<'_>,
    repo_id: &str,
    candidate_id: &str,
) -> Result<(), rusqlite::Error> {
    tx.execute(
        "DELETE FROM hardlink_candidates WHERE repo_id = ?1 AND candidate_id = ?2",
        params![repo_id, candidate_id],
    )?;
    Ok(())
}

fn map_hardlink_candidate_row(
    row: &rusqlite::Row<'_>,
) -> Result<HardlinkCandidate, rusqlite::Error> {
    let size_bytes = row.get::<_, i64>(7)?;
    Ok(HardlinkCandidate {
        candidate_id: row.get(0)?,
        repo_id: row.get(1)?,
        new_asset_id: row.get(2)?,
        new_path: row.get(3)?,
        existing_asset_id: row.get(4)?,
        existing_path: row.get(5)?,
        content_hash: row.get(6)?,
        size_bytes,
        size_label: format_size_label(size_bytes),
        created_at: row.get(8)?,
    })
}

fn collect_repository_files(repo_root: &Path) -> std::io::Result<Vec<DiscoveredFile>> {
    let mut files = Vec::new();
    if !repo_root.exists() {
        return Ok(files);
    }

    collect_repository_files_recursive(repo_root, repo_root, &mut files, false)?;
    Ok(files)
}

fn count_repository_directories(repo_root: &Path) -> Result<i64, String> {
    if !repo_root.exists() {
        return Ok(0);
    }

    count_repository_directories_recursive(repo_root, false)
}

fn count_repository_directories_recursive(
    current: &Path,
    skip_current_on_access_error: bool,
) -> Result<i64, String> {
    let mut total = 0;
    let entries = match fs::read_dir(current) {
        Ok(entries) => entries,
        Err(error) if skip_current_on_access_error && is_skippable_filesystem_error(&error) => {
            return Ok(0);
        }
        Err(error) => return Err(io_error(error)),
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) if is_skippable_filesystem_error(&error) => continue,
            Err(error) => return Err(io_error(error)),
        };
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if is_internal_repository_dir(file_name.as_ref()) {
            continue;
        }

        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(error) if is_skippable_filesystem_error(&error) => continue,
            Err(error) => return Err(io_error(error)),
        };
        if metadata.is_dir() {
            total += 1;
            total += count_repository_directories_recursive(&entry.path(), true)?;
        }
    }

    Ok(total)
}

fn read_repository_readme(repo_root: &Path) -> Result<Option<String>, String> {
    for candidate in ["README.md", "readme.md"] {
        let path = repo_root.join(candidate);
        if path.is_file() {
            let content = fs::read_to_string(path).map_err(io_error)?;
            return Ok(Some(content));
        }
    }

    Ok(None)
}

fn collect_repository_files_recursive(
    repo_root: &Path,
    current: &Path,
    files: &mut Vec<DiscoveredFile>,
    skip_current_on_access_error: bool,
) -> std::io::Result<()> {
    let entries = match fs::read_dir(current) {
        Ok(entries) => entries,
        Err(error) if skip_current_on_access_error && is_skippable_filesystem_error(&error) => {
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) if is_skippable_filesystem_error(&error) => continue,
            Err(error) => return Err(error),
        };
        let path = entry.path();
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if is_internal_repository_dir(file_name.as_ref()) {
            continue;
        }

        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(error) if is_skippable_filesystem_error(&error) => continue,
            Err(error) => return Err(error),
        };
        if metadata.is_dir() {
            collect_repository_files_recursive(repo_root, &path, files, true)?;
            continue;
        }

        let relative = path
            .strip_prefix(repo_root)
            .ok()
            .map(|item| item.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|| file_name.to_string());
        let extension = path
            .extension()
            .map(|ext| ext.to_string_lossy().to_string())
            .unwrap_or_default();

        files.push(DiscoveredFile {
            absolute_path: Some(path),
            relative_path: relative,
            filename: file_name.to_string(),
            extension,
            size_bytes: metadata.len() as i64,
            created_at: metadata
                .created()
                .ok()
                .map(system_time_to_rfc3339)
                .transpose()
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error))?,
            modified_at: metadata
                .modified()
                .ok()
                .map(system_time_to_rfc3339)
                .transpose()
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error))?
                .unwrap_or_else(now_rfc3339),
            is_virtual: false,
            provider_id: None,
            provider_item_id: None,
            source_payload: None,
            local_absolute_path: None,
        });
    }

    Ok(())
}

fn generate_thumbnail_for_file(
    repo: &RepositoryRecord,
    repo_root: &Path,
    thumbnail_root: &Path,
    file: &DiscoveredFile,
) -> Result<Option<String>, String> {
    if repo.backend_record.plugin_id != LOCAL_FILESYSTEM_PLUGIN_ID {
        return Ok(None);
    }

    let extension = file.extension.to_lowercase();
    if !is_image_extension(&extension)
        && !is_video_extension(&extension)
        && !is_audio_extension(&extension)
    {
        return Ok(None);
    }

    let source_path = resolve_repository_relative_path(repo_root, &file.relative_path)?;
    let thumbnail_dir = thumbnail_root.join(thumbnail_repository_dir_name(
        &repo.summary.repo_id,
        &repo.summary.path,
    ));
    fs::create_dir_all(&thumbnail_dir).map_err(io_error)?;
    let thumbnail_path = thumbnail_dir.join(thumbnail_file_name(
        &repo.summary.repo_id,
        &repo.summary.path,
        &file.relative_path,
        "file",
        "generated",
    ));

    let generated = if is_image_extension(&extension) {
        generate_image_thumbnail(&source_path, &thumbnail_path).map(|_| true)
    } else if is_audio_extension(&extension) {
        generate_audio_thumbnail(&source_path, &thumbnail_path)
    } else {
        generate_video_thumbnail(&source_path, &thumbnail_path).map(|_| true)
    };

    match generated {
        Ok(true) => Ok(Some(thumbnail_path.to_string_lossy().to_string())),
        Ok(false) => {
            let _ = fs::remove_file(&thumbnail_path);
            Ok(None)
        }
        Err(error) => {
            let _ = fs::remove_file(&thumbnail_path);
            eprintln!(
                "thumbnail generation skipped for {}: {}",
                file.relative_path, error
            );
            Ok(None)
        }
    }
}

fn ensure_thumbnail_for_file(
    repo: &RepositoryRecord,
    repo_root: &Path,
    thumbnail_root: &Path,
    file: &DiscoveredFile,
    existing_thumbnail_path: Option<String>,
    refresh: bool,
) -> Result<Option<String>, String> {
    if !refresh {
        if let Some(path) = existing_thumbnail_path {
            let expected_dir = thumbnail_root.join(thumbnail_repository_dir_name(
                &repo.summary.repo_id,
                &repo.summary.path,
            ));
            if thumbnail_path_is_valid(&expected_dir, &path) {
                return Ok(Some(path));
            }
        }
    }

    generate_thumbnail_for_file(repo, repo_root, thumbnail_root, file)
}

fn thumbnail_path_is_valid(thumbnail_root: &Path, path: &str) -> bool {
    let thumbnail_path = Path::new(path);
    if !thumbnail_path.is_file() {
        return false;
    }

    let Ok(thumbnail_path) = canonicalize_local_path(thumbnail_path) else {
        return false;
    };
    let Ok(thumbnail_root) = canonicalize_local_path(thumbnail_root) else {
        return false;
    };
    thumbnail_path.starts_with(thumbnail_root)
}

fn thumbnail_bytes_from_request(request: &ThumbnailRequest) -> Result<Vec<u8>, String> {
    if let Some(bytes) = &request.image_bytes {
        return Ok(bytes.clone());
    }

    if let Some(source_url) = request
        .source_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if request.action.as_deref() != Some("save") {
            return Err("thumbnail sourceUrl can only be used with save action".to_string());
        }
        return download_remote_thumbnail_bytes(source_url);
    }

    let source_path = request
        .source_path
        .as_deref()
        .ok_or_else(|| "thumbnail source is required".to_string())?;
    let path = Path::new(source_path);
    if !path.is_file() {
        return Err(format!("thumbnail source file not found: {source_path}"));
    }
    fs::read(path).map_err(io_error)
}

fn download_remote_thumbnail_bytes(url: &str) -> Result<Vec<u8>, String> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err("thumbnail sourceUrl only supports http and https URLs".to_string());
    }
    let client = reqwest::blocking::Client::builder()
        .user_agent("MomoBakoThumbnail/1")
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|error| format!("thumbnail download client error: {error}"))?;
    let response = client
        .get(url)
        .send()
        .map_err(|error| format!("thumbnail download request failed: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("thumbnail download returned HTTP {status}"));
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_REMOTE_THUMBNAIL_BYTES)
    {
        return Err("thumbnail source is too large".to_string());
    }
    let bytes = response
        .bytes()
        .map_err(|error| format!("thumbnail download body error: {error}"))?;
    if bytes.len() as u64 > MAX_REMOTE_THUMBNAIL_BYTES {
        return Err("thumbnail source is too large".to_string());
    }
    Ok(bytes.to_vec())
}

fn save_custom_thumbnail_bytes(
    thumbnail_root: &Path,
    repo: &RepositoryRecord,
    entry_path: &str,
    kind: &str,
    bytes: &[u8],
) -> Result<String, String> {
    save_thumbnail_bytes(thumbnail_root, repo, entry_path, kind, "custom", bytes)
}

fn save_thumbnail_bytes(
    thumbnail_root: &Path,
    repo: &RepositoryRecord,
    entry_path: &str,
    kind: &str,
    source: &str,
    bytes: &[u8],
) -> Result<String, String> {
    if bytes.is_empty() {
        return Err("thumbnail image is empty".to_string());
    }

    let thumbnail_dir = thumbnail_root.join(thumbnail_repository_dir_name(
        &repo.summary.repo_id,
        &repo.summary.path,
    ));
    fs::create_dir_all(&thumbnail_dir).map_err(io_error)?;
    let thumbnail_path = thumbnail_dir.join(thumbnail_file_name(
        &repo.summary.repo_id,
        &repo.summary.path,
        entry_path,
        kind,
        source,
    ));
    let image = image::load_from_memory(bytes)
        .map_err(|error| format!("thumbnail image error: {error}"))?;
    let thumbnail = image.thumbnail(THUMBNAIL_SIZE, THUMBNAIL_SIZE);
    thumbnail
        .save_with_format(&thumbnail_path, image::ImageFormat::Jpeg)
        .map_err(|error| format!("thumbnail image error: {error}"))?;
    Ok(thumbnail_path.to_string_lossy().to_string())
}

fn thumbnail_repository_dir_name(repo_id: &str, repo_path: &str) -> String {
    sha256_hex(&[repo_id.as_bytes(), repo_path.as_bytes()])
}

fn thumbnail_file_name(
    repo_id: &str,
    repo_path: &str,
    entry_path: &str,
    kind: &str,
    source: &str,
) -> String {
    format!(
        "{}.jpg",
        sha256_hex(&[
            repo_id.as_bytes(),
            repo_path.as_bytes(),
            entry_path.as_bytes(),
            kind.as_bytes(),
            source.as_bytes(),
        ])
    )
}

fn preview_file_token(
    repo_id: &str,
    repo_path: &str,
    entry_path: &str,
    size_bytes: u64,
    modified_at: &str,
) -> String {
    sha256_hex(&[
        repo_id.as_bytes(),
        repo_path.as_bytes(),
        entry_path.as_bytes(),
        size_bytes.to_string().as_bytes(),
        modified_at.as_bytes(),
    ])
}

fn preview_media_type_for_extension(extension: &str) -> &'static str {
    match extension {
        "glb" | "vrm" => "model/gltf-binary",
        "gltf" => "model/gltf+json",
        "obj" => "text/plain",
        "fbx" => "application/octet-stream",
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "mkv" => "video/x-matroska",
        "webm" => "video/webm",
        "avi" => "video/x-msvideo",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" | "opus" => "audio/ogg",
        "flac" => "audio/flac",
        "m4a" | "aac" => "audio/aac",
        "pdf" => "application/pdf",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "docm" => "application/vnd.ms-word.document.macroenabled.12",
        "dotx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.template",
        "dotm" => "application/vnd.ms-word.template.macroenabled.12",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "xlsm" => "application/vnd.ms-excel.sheet.macroenabled.12",
        "xlsb" => "application/vnd.ms-excel.sheet.binary.macroenabled.12",
        "xltx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.template",
        "xltm" => "application/vnd.ms-excel.template.macroenabled.12",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "pptm" => "application/vnd.ms-powerpoint.presentation.macroenabled.12",
        "ppsx" => "application/vnd.openxmlformats-officedocument.presentationml.slideshow",
        "ppsm" => "application/vnd.ms-powerpoint.slideshow.macroenabled.12",
        "potx" => "application/vnd.openxmlformats-officedocument.presentationml.template",
        "potm" => "application/vnd.ms-powerpoint.template.macroenabled.12",
        "doc" | "dot" => "application/msword",
        "xls" | "xlt" => "application/vnd.ms-excel",
        "ppt" | "pps" | "pot" => "application/vnd.ms-powerpoint",
        "md" | "markdown" | "mdown" | "mkd" | "mkdn" | "mdx" => "text/markdown",
        "txt" | "text" | "log" | "csv" | "tsv" | "yaml" | "yml" | "toml" | "xml" | "html"
        | "css" | "scss" | "sass" | "less" | "js" | "jsx" | "ts" | "tsx" | "vue" | "rs" | "py"
        | "rb" | "go" | "java" | "c" | "h" | "cpp" | "hpp" | "cs" | "php" | "sh" | "bash"
        | "zsh" | "ps1" | "bat" | "cmd" | "ini" | "cfg" | "conf" | "env" | "gitignore"
        | "gitattributes" => "text/plain",
        "json" | "jsonl" => "application/json",
        _ => "application/octet-stream",
    }
}

fn sha256_hex(parts: &[&[u8]]) -> String {
    let mut hash = Sha256::new();
    for part in parts {
        hash.update(part);
        hash.update([0xff]);
    }
    hex::encode(hash.finalize())
}

fn file_sha256_hash(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(io_error)?;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(io_error)?;
        if read == 0 {
            break;
        }
        hash.update(&buffer[..read]);
    }
    Ok(format!("sha256:{}", hex::encode(hash.finalize())))
}

fn file_content_hash_and_size(path: &Path) -> Result<Option<(String, i64)>, String> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io_error(error)),
    };
    if !metadata.is_file() {
        return Ok(None);
    }
    let size_bytes = i64::try_from(metadata.len())
        .map_err(|_| "file size exceeds supported range".to_string())?;
    let content_hash = file_sha256_hash(path)?;
    Ok(Some((content_hash, size_bytes)))
}

fn current_file_matches_content(
    path: &Path,
    expected_hash: &str,
    expected_size_bytes: i64,
) -> Result<bool, String> {
    let Some((content_hash, size_bytes)) = file_content_hash_and_size(path)? else {
        return Ok(false);
    };
    Ok(content_hash == expected_hash && size_bytes == expected_size_bytes)
}

fn is_content_hash(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn generate_image_thumbnail(source_path: &Path, thumbnail_path: &Path) -> Result<(), String> {
    let image =
        image::open(source_path).map_err(|error| format!("image thumbnail error: {error}"))?;
    let thumbnail = image.thumbnail(THUMBNAIL_SIZE, THUMBNAIL_SIZE);
    thumbnail
        .save_with_format(thumbnail_path, image::ImageFormat::Jpeg)
        .map_err(|error| format!("image thumbnail error: {error}"))
}

fn generate_video_thumbnail(source_path: &Path, thumbnail_path: &Path) -> Result<(), String> {
    ensure_ffmpeg_ready()?;

    let status = Command::new(ffmpeg_sidecar::paths::ffmpeg_path())
        .args(video_thumbnail_ffmpeg_args(source_path, thumbnail_path))
        .status()
        .map_err(|error| format!("ffmpeg unavailable: {error}"))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("ffmpeg exited with status: {status}"))
    }
}

fn generate_audio_thumbnail(source_path: &Path, thumbnail_path: &Path) -> Result<bool, String> {
    ensure_ffmpeg_ready()?;

    if !audio_has_cover_stream(source_path)? {
        return Ok(false);
    }

    let status = Command::new(ffmpeg_sidecar::paths::ffmpeg_path())
        .args(audio_thumbnail_ffmpeg_args(source_path, thumbnail_path))
        .status()
        .map_err(|error| format!("ffmpeg unavailable: {error}"))?;

    if status.success() {
        Ok(true)
    } else {
        Err(format!("ffmpeg exited with status: {status}"))
    }
}

fn audio_has_cover_stream(source_path: &Path) -> Result<bool, String> {
    let output = match Command::new(ffmpeg_sidecar::ffprobe::ffprobe_path())
        .args(audio_cover_probe_args(source_path))
        .output()
    {
        Ok(output) => output,
        Err(_) => return Ok(false),
    };

    if !output.status.success() {
        return Err(format!("ffprobe exited with status: {}", output.status));
    }

    audio_cover_probe_output_has_stream(&output.stdout)
}

fn audio_cover_probe_output_has_stream(output: &[u8]) -> Result<bool, String> {
    let value: serde_json::Value =
        serde_json::from_slice(output).map_err(|error| format!("ffprobe output error: {error}"))?;
    Ok(value
        .get("streams")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|streams| !streams.is_empty()))
}

fn video_thumbnail_ffmpeg_args(source_path: &Path, thumbnail_path: &Path) -> Vec<OsString> {
    vec![
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-y".into(),
        "-ss".into(),
        "00:00:01".into(),
        "-i".into(),
        source_path.as_os_str().to_os_string(),
        "-frames:v".into(),
        "1".into(),
        "-update".into(),
        "1".into(),
        "-vf".into(),
        format!("scale='min({THUMBNAIL_SIZE},iw)':-1").into(),
        thumbnail_path.as_os_str().to_os_string(),
    ]
}

fn audio_thumbnail_ffmpeg_args(source_path: &Path, thumbnail_path: &Path) -> Vec<OsString> {
    vec![
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-y".into(),
        "-i".into(),
        source_path.as_os_str().to_os_string(),
        "-map".into(),
        "0:v:0".into(),
        "-frames:v".into(),
        "1".into(),
        "-update".into(),
        "1".into(),
        "-vf".into(),
        format!("scale='min({THUMBNAIL_SIZE},iw)':-1").into(),
        thumbnail_path.as_os_str().to_os_string(),
    ]
}

fn audio_cover_probe_args(source_path: &Path) -> Vec<OsString> {
    vec![
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-select_streams".into(),
        "v".into(),
        "-show_entries".into(),
        "stream=index".into(),
        "-of".into(),
        "json".into(),
        source_path.as_os_str().to_os_string(),
    ]
}

fn ensure_ffmpeg_ready() -> Result<(), String> {
    FFMPEG_READY
        .get_or_init(|| {
            ffmpeg_sidecar::download::auto_download()
                .map_err(|error| format!("ffmpeg setup error: {error}"))
        })
        .clone()
}

fn is_image_extension(extension: &str) -> bool {
    matches!(
        extension,
        "jpg" | "jpeg" | "png" | "webp" | "gif" | "bmp" | "tif" | "tiff"
    )
}

#[derive(Debug, Default)]
struct PaletteBucket {
    count: u64,
    red_sum: u64,
    green_sum: u64,
    blue_sum: u64,
}

fn extract_image_palette(source_path: &Path, extension: &str) -> Vec<String> {
    if !is_image_extension(&extension.to_ascii_lowercase()) {
        return Vec::new();
    }

    let Ok(image) = image::open(source_path) else {
        return Vec::new();
    };
    let sampled = if image.width().max(image.height()) > 160 {
        image.thumbnail(160, 160)
    } else {
        image
    };
    let thumbnail = sampled.to_rgba8();
    let mut buckets = BTreeMap::<(u8, u8, u8), PaletteBucket>::new();

    for pixel in thumbnail.pixels() {
        let [red, green, blue, alpha] = pixel.0;
        if alpha < 128 {
            continue;
        }
        let bucket = buckets
            .entry((red & 0xf8, green & 0xf8, blue & 0xf8))
            .or_default();
        bucket.count += 1;
        bucket.red_sum += u64::from(red);
        bucket.green_sum += u64::from(green);
        bucket.blue_sum += u64::from(blue);
    }

    let mut colors = buckets
        .into_iter()
        .filter(|(_, bucket)| bucket.count > 0)
        .collect::<Vec<_>>();
    colors.sort_by(|(left_key, left), (right_key, right)| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left_key.cmp(right_key))
    });

    colors
        .into_iter()
        .take(5)
        .map(|(_, bucket)| averaged_hex_color(&bucket))
        .collect()
}

fn averaged_hex_color(bucket: &PaletteBucket) -> String {
    let red = rounded_channel_average(bucket.red_sum, bucket.count);
    let green = rounded_channel_average(bucket.green_sum, bucket.count);
    let blue = rounded_channel_average(bucket.blue_sum, bucket.count);
    format!("#{red:02X}{green:02X}{blue:02X}")
}

fn rounded_channel_average(sum: u64, count: u64) -> u8 {
    ((sum + count / 2) / count) as u8
}

fn is_video_extension(extension: &str) -> bool {
    matches!(extension, "mp4" | "mov" | "mkv" | "webm" | "avi" | "m4v")
}

fn is_audio_extension(extension: &str) -> bool {
    matches!(
        extension,
        "mp3" | "wav" | "ogg" | "flac" | "m4a" | "aac" | "opus"
    )
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DiscoveredFile {
    absolute_path: Option<PathBuf>,
    relative_path: String,
    filename: String,
    extension: String,
    size_bytes: i64,
    created_at: Option<String>,
    modified_at: String,
    #[serde(default)]
    is_virtual: bool,
    #[serde(default)]
    provider_id: Option<String>,
    #[serde(default)]
    provider_item_id: Option<String>,
    #[serde(default)]
    source_payload: Option<serde_json::Value>,
    #[serde(default)]
    local_absolute_path: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackendDiscoveredFile {
    absolute_path: Option<PathBuf>,
    relative_path: String,
    filename: String,
    extension: String,
    size_bytes: i64,
    created_at: Option<String>,
    modified_at: String,
    #[serde(default)]
    is_virtual: bool,
    #[serde(default)]
    provider_id: Option<String>,
    #[serde(default)]
    provider_item_id: Option<String>,
    #[serde(default)]
    source_payload: Option<serde_json::Value>,
    #[serde(default)]
    local_absolute_path: Option<String>,
}

impl BackendDiscoveredFile {
    fn into_discovered_file(self, repo_root: &Path) -> Result<DiscoveredFile, String> {
        let relative_path = normalize_entry_path(&self.relative_path)?;
        let absolute_path =
            if self.is_virtual {
                self.absolute_path
            } else {
                Some(self.absolute_path.map(Ok).unwrap_or_else(|| {
                    resolve_repository_relative_path(repo_root, &relative_path)
                })?)
            };
        Ok(DiscoveredFile {
            absolute_path,
            relative_path,
            filename: self.filename,
            extension: self.extension,
            size_bytes: self.size_bytes,
            created_at: self.created_at,
            modified_at: self.modified_at,
            is_virtual: self.is_virtual,
            provider_id: self.provider_id,
            provider_item_id: self.provider_item_id,
            source_payload: self.source_payload,
            local_absolute_path: self.local_absolute_path,
        })
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MetadataDefaultsBatchEntry {
    path: String,
    name: String,
    extension: String,
    kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    metadata: Option<BTreeMap<String, serde_json::Value>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MetadataDefaultsBatchResponse {
    #[serde(default)]
    defaults_by_path: BTreeMap<String, BTreeMap<String, serde_json::Value>>,
}

fn slugify_repo_id(name: &str, path: &str) -> String {
    slugify_ascii_component(&format!("{name}-{path}"))
}

fn normalized_netease_account_id(value: &serde_json::Value) -> Option<String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| value.as_i64().map(|item| item.to_string()))
        .or_else(|| value.as_u64().map(|item| item.to_string()))
}

pub(crate) fn asset_id_for_path(repo_id: &str, relative_path: &str) -> String {
    format!(
        "asset-{}",
        sha256_hex(&[repo_id.as_bytes(), relative_path.as_bytes()])
    )
}

fn slugify_ascii_component(value: &str) -> String {
    let slug = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    slug.trim_matches('-').to_string()
}

type PluginManifestFn = unsafe extern "C" fn() -> *mut c_char;
type PluginCallFn = unsafe extern "C" fn(*const c_char) -> *mut c_char;
type PluginFreeFn = unsafe extern "C" fn(*mut c_char);

struct NativePlugin {
    _library: libloading::Library,
    call: PluginCallFn,
    free: PluginFreeFn,
}

#[derive(Debug)]
struct DiscoveredPluginManifest {
    manifest: PluginManifest,
    archive_path: PathBuf,
    manifest_prefix: String,
}

struct BackendPluginRegistration {
    manifest: PluginManifest,
    archive_path: PathBuf,
    manifest_prefix: String,
    native: Option<NativePlugin>,
    load_error: Option<String>,
}

struct BackendPluginRegistry {
    service_root: PathBuf,
    registrations: BTreeMap<String, BackendPluginRegistration>,
    legacy_ids: BTreeMap<String, String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginSettings {
    plugins: BTreeMap<String, PluginSettingsEntry>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginSettingsEntry {
    enabled: Option<bool>,
}

impl BackendPluginRegistry {
    fn load(service_root: &Path) -> Self {
        Self::load_with_options(service_root, true)
    }

    fn load_for_management(service_root: &Path) -> Self {
        Self::load_with_options(service_root, false)
    }

    fn load_with_options(service_root: &Path, load_native: bool) -> Self {
        let settings = load_plugin_settings(service_root).unwrap_or_default();
        let manifests = load_runtime_plugin_manifests(service_root);
        let mut registrations = BTreeMap::new();
        let mut legacy_ids = BTreeMap::new();
        for discovered in manifests {
            let mut manifest = discovered.manifest;
            apply_plugin_settings(&mut manifest, &settings);
            for legacy_id in plugin_legacy_ids(&manifest) {
                legacy_ids.insert(legacy_id, manifest.plugin_id.clone());
            }
            let (native, load_error) =
                if load_native && manifest.enabled && manifest.runtime == "native-dylib" {
                    match load_native_plugin(
                        &manifest,
                        &discovered.archive_path,
                        &discovered.manifest_prefix,
                        service_root,
                    ) {
                        Ok(native) => (Some(native), None),
                        Err(error) => (None, Some(error)),
                    }
                } else {
                    (None, None)
                };
            registrations.insert(
                manifest.plugin_id.clone(),
                BackendPluginRegistration {
                    manifest,
                    archive_path: discovered.archive_path,
                    manifest_prefix: discovered.manifest_prefix,
                    native,
                    load_error,
                },
            );
        }

        Self {
            service_root: service_root.to_path_buf(),
            registrations,
            legacy_ids,
        }
    }

    fn list_manifests(&self) -> Vec<PluginManifest> {
        let mut manifests = self
            .registrations
            .values()
            .map(|registration| {
                let mut manifest = registration.manifest.clone();
                if manifest.runtime == "native-dylib"
                    && manifest.enabled
                    && registration.native.is_none()
                    && !embedded_local_filesystem_fallback_enabled(&manifest.plugin_id)
                {
                    manifest.status = "unavailable".to_string();
                    manifest.disable_reason = Some("原生运行时不可用。".to_string());
                }
                manifest
            })
            .collect::<Vec<_>>();
        resolve_plugin_manifest_dependencies(&mut manifests);
        manifests
    }

    fn resolved_manifests_by_id(&self) -> BTreeMap<String, PluginManifest> {
        self.list_manifests()
            .into_iter()
            .map(|manifest| (manifest.plugin_id.clone(), manifest))
            .collect()
    }

    fn resolved_manifest(&self, plugin_id: &str) -> Option<PluginManifest> {
        let normalized = self.normalize_plugin_id(plugin_id);
        self.resolved_manifests_by_id().remove(normalized.as_str())
    }

    fn manifest(&self, plugin_id: &str) -> Option<&PluginManifest> {
        let normalized = self.normalize_plugin_id(plugin_id);
        self.registrations
            .get(normalized.as_str())
            .map(|registration| &registration.manifest)
    }

    fn registration(&self, plugin_id: &str) -> Option<&BackendPluginRegistration> {
        let normalized = self.normalize_plugin_id(plugin_id);
        self.registrations.get(normalized.as_str())
    }

    fn normalize_plugin_id(&self, plugin_id: &str) -> String {
        let trimmed = plugin_id.trim();
        self.legacy_ids
            .get(trimmed)
            .cloned()
            .unwrap_or_else(|| trimmed.to_string())
    }

    fn call(
        &self,
        plugin_id: &str,
        method: &str,
        payload: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        self.call_with_runtime(plugin_id, method, payload)
            .map(|result| result.payload)
    }

    fn call_with_runtime(
        &self,
        plugin_id: &str,
        method: &str,
        payload: serde_json::Value,
    ) -> Result<PluginRuntimeCallResult, String> {
        let normalized = self.normalize_plugin_id(plugin_id);
        let registration = self
            .registrations
            .get(normalized.as_str())
            .ok_or_else(|| format!("unsupported plugin: {plugin_id}"))?;
        let resolved_manifest = self
            .resolved_manifest(&normalized)
            .unwrap_or_else(|| registration.manifest.clone());
        if !resolved_manifest.enabled
            || matches!(
                resolved_manifest.status.as_str(),
                "disabled" | "unavailable" | "error"
            )
        {
            let reason = resolved_manifest
                .disable_reason
                .as_deref()
                .unwrap_or("插件不可用。");
            return Err(format!(
                "plugin call blocked by dependency status: {} {method} ({reason})",
                resolved_manifest.plugin_id
            ));
        }
        let runtime = plugin_call_runtime(&resolved_manifest);
        let plugin_data_dir =
            ensure_plugin_data_dir(&self.service_root, &resolved_manifest.plugin_id)?;
        let plugin_config = load_plugin_config_values(&plugin_data_dir)?;
        let runtime_context = PluginCallHostRuntime {
            plugin_id: resolved_manifest.plugin_id.clone(),
            plugin_data_dir: plugin_data_dir.to_string_lossy().to_string(),
            plugin_config,
        };
        let response = if let Some(native) = &registration.native {
            native.call(method, payload, runtime_context)?
        } else if embedded_local_filesystem_fallback_enabled(&registration.manifest.plugin_id) {
            call_builtin_local_filesystem(method, payload)?
        } else if let Some(error) = &registration.load_error {
            return Err(format!(
                "plugin runtime is not available: {} ({error})",
                registration.manifest.plugin_id
            ));
        } else {
            return Err(format!(
                "plugin runtime is not available: {}",
                registration.manifest.plugin_id
            ));
        };
        Ok(PluginRuntimeCallResult {
            plugin_id: resolved_manifest.plugin_id,
            payload: response,
            runtime,
        })
    }

    fn playlist_players(&self) -> Vec<PlaylistPlayerRegistration> {
        let mut players = Vec::new();
        for registration in self.registrations.values() {
            if !registration.manifest.enabled || registration.manifest.status == "error" {
                continue;
            }
            let Some(contributes) = registration.manifest.contributes.as_object() else {
                continue;
            };
            let Some(raw_players) = contributes.get("playlistPlayers") else {
                continue;
            };
            let Ok(parsed) =
                serde_json::from_value::<Vec<PlaylistPlayerContribution>>(raw_players.clone())
            else {
                continue;
            };
            for player in parsed {
                players.push(PlaylistPlayerRegistration {
                    plugin_id: registration.manifest.plugin_id.clone(),
                    player_type_id: player.player_type_id,
                    label: player.label,
                    file_class: player.file_class,
                    supported_extensions: player
                        .supported_extensions
                        .into_iter()
                        .map(|value| value.trim().to_ascii_lowercase())
                        .filter(|value| !value.is_empty())
                        .collect(),
                    supports_seek: player.supports_seek,
                    supports_volume: player.supports_volume,
                    supports_preview_navigation: player.supports_preview_navigation,
                    description: player.description,
                });
            }
        }
        players.sort_by(|left, right| {
            left.label
                .to_lowercase()
                .cmp(&right.label.to_lowercase())
                .then_with(|| left.player_type_id.cmp(&right.player_type_id))
        });
        players
    }

    fn playlist_player(&self, player_type_id: &str) -> Option<PlaylistPlayerRegistration> {
        let normalized = player_type_id.trim();
        self.playlist_players()
            .into_iter()
            .find(|player| player.player_type_id == normalized)
    }

    fn metadata_default_providers(&self) -> Vec<(String, String)> {
        let mut providers = Vec::new();
        let resolved_manifests = self.resolved_manifests_by_id();
        for registration in self.registrations.values() {
            let Some(manifest) = resolved_manifests.get(&registration.manifest.plugin_id) else {
                continue;
            };
            if !manifest.enabled
                || matches!(
                    manifest.status.as_str(),
                    "disabled" | "unavailable" | "error"
                )
            {
                continue;
            }
            let Some(contributes) = manifest.contributes.as_object() else {
                continue;
            };
            let Some(defaults) = contributes
                .get("metadataDefaults")
                .and_then(|value| value.as_object())
            else {
                continue;
            };
            let Some(action) = defaults
                .get("action")
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
            else {
                continue;
            };
            providers.push((manifest.plugin_id.clone(), action.to_string()));
        }
        providers.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
        providers
    }
}

impl NativePlugin {
    fn call(
        &self,
        method: &str,
        payload: serde_json::Value,
        runtime: PluginCallHostRuntime,
    ) -> Result<serde_json::Value, String> {
        let request = PluginCallEnvelope {
            method: method.to_string(),
            payload,
            runtime,
        };
        let request_json = serde_json::to_string(&request).map_err(json_error)?;
        let request_cstring = CString::new(request_json)
            .map_err(|_| "plugin request contains an invalid null byte".to_string())?;
        let response_ptr = unsafe { (self.call)(request_cstring.as_ptr()) };
        if response_ptr.is_null() {
            return Err("plugin returned a null response".to_string());
        }
        let response_json = unsafe { CStr::from_ptr(response_ptr) }
            .to_string_lossy()
            .to_string();
        unsafe { (self.free)(response_ptr) };
        let response: PluginCallResponse =
            serde_json::from_str(&response_json).map_err(json_error)?;
        if response.ok {
            Ok(response.payload.unwrap_or_else(|| serde_json::json!({})))
        } else {
            Err(response
                .error
                .unwrap_or_else(|| "plugin call failed without an error message".to_string()))
        }
    }
}

fn plugin_call_runtime(manifest: &PluginManifest) -> Option<PluginCallRuntime> {
    if !manifest.degraded {
        return None;
    }
    Some(PluginCallRuntime {
        degraded: true,
        degradation_reason: manifest.degradation_reason.clone(),
        dependency_status: manifest.dependency_status.clone(),
    })
}

fn backend_plugin_registry(service_root: &Path) -> BackendPluginRegistry {
    BackendPluginRegistry::load(service_root)
}

fn plugin_management_registry(service_root: &Path) -> BackendPluginRegistry {
    BackendPluginRegistry::load_for_management(service_root)
}

fn call_downloader_prepare_track_playback(
    service_root: &Path,
    payload: serde_json::Value,
) -> Result<serde_json::Value, String> {
    #[cfg(test)]
    if let Some(hook) = test_support::downloader_playback_hook()? {
        return hook(payload);
    }

    backend_plugin_registry(service_root).call(
        "momobako.service.downloader",
        "downloader.prepareTrackPlayback",
        payload,
    )
}

pub(crate) fn call_downloader_download_track_package(
    service_root: &Path,
    payload: serde_json::Value,
) -> Result<serde_json::Value, String> {
    #[cfg(test)]
    if let Some(hook) = test_support::downloader_track_package_hook()? {
        return hook(payload);
    }

    backend_plugin_registry(service_root).call(
        "momobako.service.downloader",
        "downloader.downloadTrackPackage",
        payload,
    )
}

#[cfg(test)]
pub(crate) fn set_test_downloader_playback_hook(
    hook: Option<fn(serde_json::Value) -> Result<serde_json::Value, String>>,
) {
    test_support::set_test_downloader_playback_hook(hook);
}

#[cfg(test)]
pub(crate) fn set_test_downloader_track_package_hook(
    hook: Option<fn(serde_json::Value) -> Result<serde_json::Value, String>>,
) {
    test_support::set_test_downloader_track_package_hook(hook);
}

#[cfg(test)]
pub(crate) fn set_test_backend_stat_entry_hook(
    hook: Option<fn(&RepositoryRecord, &Path, &str) -> Option<Result<FileSystemEntry, String>>>,
) {
    test_support::set_test_backend_stat_entry_hook(hook);
}

fn load_runtime_plugin_manifests(service_root: &Path) -> Vec<DiscoveredPluginManifest> {
    let mut manifests = load_plugin_manifests_from_runtime(runtime_plugins_dir(service_root));
    manifests.sort_by(|left, right| left.manifest.plugin_id.cmp(&right.manifest.plugin_id));
    manifests
}

fn load_plugin_manifests_from_runtime(runtime_root: PathBuf) -> Vec<DiscoveredPluginManifest> {
    match read_plugin_manifests_from_dir(&runtime_root) {
        Ok(manifests) => manifests,
        Err(error) => {
            eprintln!(
                "failed to read runtime plugin manifests from {}: {}",
                runtime_root.display(),
                error
            );
            Vec::new()
        }
    }
}

fn read_plugin_manifests_from_dir(root: &Path) -> Result<Vec<DiscoveredPluginManifest>, String> {
    let mut manifests = Vec::new();
    if !root.is_dir() {
        return Ok(manifests);
    }
    for entry in fs::read_dir(root).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let plugin_path = entry.path();
        if plugin_path.is_dir() {
            match read_discovered_plugin_manifest_from_directory(&plugin_path) {
                Ok(Some(discovered)) => manifests.push(discovered),
                Ok(None) => {}
                Err(error) => manifests.push(DiscoveredPluginManifest {
                    manifest: broken_plugin_manifest(&plugin_path, &error),
                    archive_path: plugin_path,
                    manifest_prefix: String::new(),
                }),
            }
            continue;
        }
        if plugin_path.extension().and_then(|value| value.to_str()) != Some("momoplug") {
            continue;
        }
        match read_discovered_plugin_manifest_from_archive(&plugin_path) {
            Ok(discovered) => manifests.push(discovered),
            Err(error) => manifests.push(DiscoveredPluginManifest {
                manifest: broken_plugin_manifest(&plugin_path, &error),
                archive_path: plugin_path,
                manifest_prefix: String::new(),
            }),
        }
    }
    manifests.sort_by(|left, right| left.manifest.plugin_id.cmp(&right.manifest.plugin_id));
    Ok(manifests)
}

fn read_discovered_plugin_manifest_from_directory(
    plugin_dir: &Path,
) -> Result<Option<DiscoveredPluginManifest>, String> {
    let manifest_path = plugin_dir.join("manifest.json");
    if !manifest_path.is_file() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&manifest_path).map_err(io_error)?;
    Ok(Some(DiscoveredPluginManifest {
        manifest: parse_plugin_manifest_with_source(&raw, None)?,
        archive_path: plugin_dir.to_path_buf(),
        manifest_prefix: String::new(),
    }))
}

fn read_discovered_plugin_manifest_from_archive(
    archive_path: &Path,
) -> Result<DiscoveredPluginManifest, String> {
    let (raw, manifest_prefix) = read_plugin_manifest_from_archive(archive_path)?;
    Ok(DiscoveredPluginManifest {
        manifest: parse_plugin_manifest_with_source(&raw, None)?,
        archive_path: archive_path.to_path_buf(),
        manifest_prefix,
    })
}

#[cfg(test)]
pub fn install_local_filesystem_test_plugin_archive(service_root: &Path) {
    let runtime_plugin_root = runtime_plugins_dir(service_root);
    let archive_path = runtime_plugin_root.join("local-filesystem.momoplug");
    if archive_path.exists() {
        return;
    }
    if let Some(parent) = archive_path.parent() {
        fs::create_dir_all(parent).expect("plugin archive parent should be created");
    }
    let file = File::create(&archive_path).expect("plugin archive should be created");
    let mut archive = zip::ZipWriter::new(file);
    archive
        .start_file(
            "momobako-local-filesystem-0.1.0/manifest.json",
            zip::write::SimpleFileOptions::default(),
        )
        .expect("manifest entry should start");
    archive
        .write_all(
            serde_json::to_string_pretty(&serde_json::json!({
                "pluginId": LOCAL_FILESYSTEM_PLUGIN_ID,
                "legacyPluginIds": [LEGACY_LOCAL_FILESYSTEM_PLUGIN_ID],
                "name": "Local Filesystem",
                "version": "0.1.0",
                "type": {
                    "layer": "source",
                    "kind": "filesystem"
                },
                "kind": "filesystem",
                "category": "source",
                "description": "Test local filesystem backend.",
                "capabilities": ["listFiles", "readFile", "writeFile", "moveFile", "deleteFile"],
                "enabled": true,
                "sdk": "backend",
                "entry": {},
                "source": "system",
                "runtime": "manifest-only",
                "permissions": [],
                "compat": {
                    "sdkVersion": "1",
                    "legacyPluginIds": []
                },
                "status": "ready"
            }))
            .expect("manifest should encode")
            .as_bytes(),
        )
        .expect("manifest should write");
    archive.finish().expect("plugin archive should finish");
}

fn default_cache_entries() -> Vec<CacheEntry> {
    vec![
        CacheEntry {
            cache_type: "metadata".to_string(),
            key: "repo-main-001:asset-01".to_string(),
            last_accessed_at: now_rfc3339(),
        },
        CacheEntry {
            cache_type: "query".to_string(),
            key: "tag=封面".to_string(),
            last_accessed_at: now_rfc3339(),
        },
        CacheEntry {
            cache_type: "thumbnail".to_string(),
            key: "asset-02".to_string(),
            last_accessed_at: now_rfc3339(),
        },
    ]
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginApiTestContribution {
    method: String,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    payload: Option<serde_json::Value>,
    #[serde(default)]
    request_template: Option<serde_json::Value>,
}

fn default_api_definitions(service_root: &Path) -> Vec<ApiDefinition> {
    let mut definitions = Vec::new();
    definitions.extend(external_api_definitions());
    definitions.extend(core_tauri_api_definitions());
    definitions.extend(plugin_api_definitions(service_root));
    definitions
}

fn external_api_definitions() -> Vec<ApiDefinition> {
    vec![
        external_api_definition(
            "GET",
            "/external/v1/health",
            "检查外部 API 服务状态。",
            false,
            None,
        ),
        external_api_definition(
            "GET",
            "/external/v1/repositories",
            "列出可接收外部素材的本地仓库。",
            true,
            None,
        ),
        external_api_definition(
            "POST",
            "/external/v1/assets:add",
            "从远程 URL 添加素材到仓库。",
            true,
            Some(serde_json::json!({
                "repoId": "",
                "parentPath": "",
                "client": {
                    "id": "momobako.api-playground",
                    "name": "API Playground",
                    "version": "0.1.0"
                },
                "items": [
                    {
                        "kind": "remoteUrl",
                        "url": "https://example.com/image.png",
                        "filename": "image.png",
                        "metadata": {
                            "sourceUrl": "https://example.com/image.png"
                        }
                    }
                ]
            })),
        ),
    ]
}

fn core_tauri_api_definitions() -> Vec<ApiDefinition> {
    vec![
        tauri_api_definition(
            "Runtime API",
            "ping",
            "检测 Tauri 命令桥。",
            serde_json::json!({}),
        ),
        tauri_api_definition(
            "Repository API",
            "list_repositories",
            "列出所有仓库。",
            serde_json::json!({}),
        ),
        tauri_api_definition(
            "Repository API",
            "get_repository_snapshot",
            "读取仓库总览、文件树和基础状态。",
            serde_json::json!({ "repoId": "<repoId>" }),
        ),
        tauri_api_definition(
            "Asset API",
            "get_asset_detail",
            "读取单个素材详情与元数据。",
            serde_json::json!({ "repoId": "<repoId>", "assetId": "<assetId>" }),
        ),
        tauri_api_definition(
            "Search API",
            "search_assets",
            "执行跨仓库结构化搜索。",
            serde_json::json!({
                "request": {
                    "query": "",
                    "repoId": null,
                    "limit": 20
                }
            }),
        ),
        tauri_api_definition(
            "Metadata API",
            "update_asset_metadata",
            "带乐观锁更新素材元数据。",
            serde_json::json!({
                "request": {
                    "repoId": "<repoId>",
                    "assetId": "<assetId>",
                    "expectedVersion": 1,
                    "metadata": {},
                    "source": "api-playground"
                }
            }),
        ),
        tauri_api_definition(
            "File API",
            "get_file_browser",
            "读取仓库文件浏览快照。",
            serde_json::json!({
                "request": {
                    "repoId": "<repoId>",
                    "directoryPath": "",
                    "includeTree": true,
                    "specialLocation": null
                }
            }),
        ),
        tauri_api_definition(
            "Playlist API",
            "list_playlists",
            "列出仓库播放列表。",
            serde_json::json!({ "repoId": "<repoId>" }),
        ),
        tauri_api_definition(
            "Playlist API",
            "list_playlist_memberships",
            "列出素材到播放列表的轻量成员关系索引。",
            serde_json::json!({ "repoId": "<repoId>" }),
        ),
        tauri_api_definition(
            "Playlist API",
            "create_playlist",
            "创建播放列表。",
            serde_json::json!({
                "request": {
                    "repoId": "<repoId>",
                    "playlistId": null,
                    "name": "New Playlist",
                    "playerTypeId": "builtin.sequence"
                }
            }),
        ),
        tauri_api_definition(
            "Playlist API",
            "update_playlist",
            "更新播放列表名称或播放器。",
            serde_json::json!({
                "request": {
                    "repoId": "<repoId>",
                    "playlistId": "<playlistId>",
                    "name": "Updated Playlist",
                    "playerTypeId": null
                }
            }),
        ),
        tauri_api_definition(
            "Playlist API",
            "delete_playlist",
            "删除播放列表。",
            serde_json::json!({ "repoId": "<repoId>", "playlistId": "<playlistId>" }),
        ),
        tauri_api_definition(
            "Playlist API",
            "get_playlist_detail",
            "读取播放列表详情。",
            serde_json::json!({ "repoId": "<repoId>", "playlistId": "<playlistId>" }),
        ),
        tauri_api_definition(
            "Playlist API",
            "add_playlist_items",
            "向播放列表添加素材。",
            serde_json::json!({
                "request": {
                    "repoId": "<repoId>",
                    "playlistId": "<playlistId>",
                    "assetIds": ["<assetId>"]
                }
            }),
        ),
        tauri_api_definition(
            "Playlist API",
            "add_playlist_items_by_paths",
            "按文件或目录路径向播放列表添加条目。",
            serde_json::json!({
                "request": {
                    "repoId": "<repoId>",
                    "playlistId": "<playlistId>",
                    "paths": ["<path>"]
                }
            }),
        ),
        tauri_api_definition(
            "Playlist API",
            "reorder_playlist_items",
            "重排播放列表条目。",
            serde_json::json!({
                "request": {
                    "repoId": "<repoId>",
                    "playlistId": "<playlistId>",
                    "itemIds": ["<playlistItemId>"]
                }
            }),
        ),
        tauri_api_definition(
            "Playlist API",
            "remove_playlist_item",
            "移除播放列表条目。",
            serde_json::json!({
                "request": {
                    "repoId": "<repoId>",
                    "playlistId": "<playlistId>",
                    "playlistItemId": "<playlistItemId>"
                }
            }),
        ),
        tauri_api_definition(
            "Playlist API",
            "set_playlist_membership",
            "设置素材所属播放列表。",
            serde_json::json!({
                "request": {
                    "repoId": "<repoId>",
                    "assetId": "<assetId>",
                    "playlistIds": ["<playlistId>"]
                }
            }),
        ),
        tauri_api_definition(
            "Smart Folder API",
            "list_smart_folders",
            "列出智能文件夹树。",
            serde_json::json!({ "repoId": "<repoId>" }),
        ),
        tauri_api_definition(
            "Smart Folder API",
            "create_smart_folder",
            "创建智能文件夹。",
            serde_json::json!({
                "request": {
                    "repoId": "<repoId>",
                    "smartFolderId": null,
                    "parentId": null,
                    "name": "New Smart Folder",
                    "filter": { "query": "", "limit": 20 }
                }
            }),
        ),
        tauri_api_definition(
            "Smart Folder API",
            "update_smart_folder",
            "更新智能文件夹。",
            serde_json::json!({
                "request": {
                    "repoId": "<repoId>",
                    "smartFolderId": "<smartFolderId>",
                    "parentId": null,
                    "name": "Updated Smart Folder",
                    "filter": { "query": "", "limit": 20 }
                }
            }),
        ),
        tauri_api_definition(
            "Smart Folder API",
            "delete_smart_folder",
            "删除智能文件夹。",
            serde_json::json!({ "repoId": "<repoId>", "smartFolderId": "<smartFolderId>" }),
        ),
        tauri_api_definition(
            "Smart Folder API",
            "query_smart_folder",
            "按智能文件夹条件查询虚拟文件列表。",
            serde_json::json!({ "repoId": "<repoId>", "smartFolderId": "<smartFolderId>" }),
        ),
        tauri_api_definition(
            "Repository Action API",
            "list_repository_actions",
            "列出仓库动作。",
            serde_json::json!({ "repoId": "<repoId>" }),
        ),
        tauri_api_definition(
            "Repository Action API",
            "get_repository_action",
            "读取单个仓库动作。",
            serde_json::json!({ "repoId": "<repoId>", "actionId": "<actionId>" }),
        ),
        tauri_api_definition(
            "Repository Action API",
            "set_repository_action_enabled",
            "启用或停用仓库动作。",
            serde_json::json!({
                "request": {
                    "repoId": "<repoId>",
                    "actionId": "<actionId>",
                    "enabled": true
                }
            }),
        ),
        tauri_api_definition(
            "Repository Action API",
            "run_repository_action",
            "运行仓库动作。",
            serde_json::json!({
                "request": {
                    "repoId": "<repoId>",
                    "actionId": "<actionId>",
                    "targetPaths": [],
                    "assetIds": []
                }
            }),
        ),
        tauri_api_definition(
            "File API",
            "read_file",
            "读取仓库文件字节。",
            serde_json::json!({ "request": { "repoId": "<repoId>", "path": "<path>" } }),
        ),
        tauri_api_definition(
            "Preview API",
            "prepare_preview_file_source",
            "为本地文件预览准备流式读取源。",
            serde_json::json!({ "request": { "repoId": "<repoId>", "path": "<path>" } }),
        ),
        tauri_api_definition(
            "Preview API",
            "prepare_entry_playback_source",
            "为本地或虚拟条目准备播放源。",
            serde_json::json!({ "request": { "repoId": "<repoId>", "path": "<path>" } }),
        ),
        tauri_api_definition(
            "Preview API",
            "prepare_entry_playback_source_with_progress",
            "为本地或虚拟条目准备播放源，并通过进度通道回报准备与下载阶段。",
            serde_json::json!({
                "request": { "repoId": "<repoId>", "path": "<path>" },
                "progress": "<Channel<EntryPlaybackProgressEvent>>"
            }),
        ),
        tauri_api_definition(
            "Playlist API",
            "download_playlist_with_progress",
            "下载歌单并通过进度通道回报逐首处理状态。",
            serde_json::json!({
                "request": {
                    "playlistId": 9001,
                    "playlistName": "夜跑歌单",
                    "tracks": [
                        {
                            "songId": 2001,
                            "songName": "稻香",
                            "sourcePayload": {
                                "provider": "netease-cloud-music",
                                "songId": 2001
                            }
                        }
                    ],
                    "destination": {
                        "kind": "localFolder",
                        "path": "C:/Downloads/Playlist"
                    },
                    "sourcePayload": {
                        "provider": "netease-cloud-music",
                        "playlistId": 9001
                    },
                    "level": "standard"
                },
                "progress": "<Channel<DownloaderPlaylistProgressEvent>>"
            }),
        ),
        tauri_api_definition(
            "Plugin API",
            "call_plugin",
            "调用后端插件方法。",
            serde_json::json!({
                "request": {
                    "pluginId": "<pluginId>",
                    "method": "<method>",
                    "payload": {}
                }
            }),
        ),
        tauri_api_definition(
            "Plugin API",
            "read_plugin_archive_text",
            "读取插件包内文本文件。",
            serde_json::json!({ "request": { "pluginId": "<pluginId>", "path": "manifest.json" } }),
        ),
        tauri_api_definition(
            "Plugin API",
            "get_plugin_data_directory",
            "创建并读取插件自有数据目录。",
            serde_json::json!({ "pluginId": "<pluginId>" }),
        ),
        tauri_api_definition(
            "Plugin API",
            "prepare_plugin_data_file_preview_source",
            "将插件数据目录内文件注册为受控预览源。",
            serde_json::json!({
                "request": {
                    "pluginId": "<pluginId>",
                    "path": "<absolutePluginDataFilePath>",
                    "mediaType": "text/plain; charset=utf-8"
                }
            }),
        ),
        tauri_api_definition(
            "Plugin API",
            "get_plugin_config",
            "读取插件 key-value 配置快照。",
            serde_json::json!({ "pluginId": "<pluginId>" }),
        ),
        tauri_api_definition(
            "Plugin API",
            "set_plugin_config_value",
            "写入插件 key-value 配置项。",
            serde_json::json!({
                "request": {
                    "pluginId": "<pluginId>",
                    "key": "apiKey",
                    "value": "<value>"
                }
            }),
        ),
        tauri_api_definition(
            "Plugin API",
            "delete_plugin_config_value",
            "删除插件 key-value 配置项。",
            serde_json::json!({
                "request": {
                    "pluginId": "<pluginId>",
                    "key": "apiKey"
                }
            }),
        ),
        tauri_api_definition(
            "File API",
            "write_binary_file",
            "写入二进制文件。",
            serde_json::json!({ "request": { "path": "<absolutePath>", "bytes": [] } }),
        ),
        tauri_api_definition(
            "File API",
            "create_directory",
            "创建目录。",
            serde_json::json!({
                "request": {
                    "repoId": "<repoId>",
                    "parentPath": "",
                    "name": "New Folder"
                }
            }),
        ),
        tauri_api_definition(
            "File API",
            "create_file",
            "创建空文件。",
            serde_json::json!({
                "request": {
                    "repoId": "<repoId>",
                    "parentPath": "",
                    "name": "new-file.txt"
                }
            }),
        ),
        tauri_api_definition(
            "File API",
            "import_entries",
            "导入外部文件或目录。",
            serde_json::json!({
                "request": {
                    "repoId": "<repoId>",
                    "parentPath": "",
                    "sourcePaths": ["<absolutePath>"]
                }
            }),
        ),
        tauri_api_definition(
            "File API",
            "copy_entries",
            "复制仓库内文件条目。",
            serde_json::json!({
                "request": {
                    "repoId": "<repoId>",
                    "sourcePaths": ["<path>"],
                    "parentPath": "",
                    "mode": "copy"
                }
            }),
        ),
        tauri_api_definition(
            "File API",
            "move_entries",
            "移动仓库内文件条目。",
            serde_json::json!({
                "request": {
                    "repoId": "<repoId>",
                    "sourcePaths": ["<path>"],
                    "parentPath": ""
                }
            }),
        ),
        tauri_api_definition(
            "File API",
            "rename_entry",
            "重命名仓库内文件条目。",
            serde_json::json!({
                "request": {
                    "repoId": "<repoId>",
                    "path": "<path>",
                    "newName": "renamed.txt"
                }
            }),
        ),
        tauri_api_definition(
            "File API",
            "delete_entry",
            "删除或移入回收站。",
            serde_json::json!({
                "request": {
                    "repoId": "<repoId>",
                    "path": "<path>",
                    "mode": "trash"
                }
            }),
        ),
        tauri_api_definition(
            "File API",
            "mutate_trash",
            "恢复或清理回收站条目。",
            serde_json::json!({
                "request": {
                    "repoId": "<repoId>",
                    "action": "restore",
                    "path": "<trashPath>"
                }
            }),
        ),
        tauri_api_definition(
            "Repository API",
            "create_repository",
            "创建仓库。",
            serde_json::json!({
                "request": {
                    "repoId": null,
                    "name": "New Repository",
                    "path": "<absolutePath>",
                    "backendPluginId": null,
                    "backendConfig": null
                }
            }),
        ),
        tauri_api_definition(
            "Repository API",
            "import_repository",
            "导入已有仓库。",
            serde_json::json!({
                "request": {
                    "repoId": null,
                    "name": "Imported Repository",
                    "path": "<absolutePath>",
                    "backendPluginId": null,
                    "backendConfig": null
                }
            }),
        ),
        tauri_api_definition(
            "Repository API",
            "attach_repository_folder",
            "挂载仓库文件夹。",
            serde_json::json!({ "request": { "path": "<absolutePath>" } }),
        ),
        tauri_api_definition(
            "Repository API",
            "delete_repository",
            "删除仓库记录。",
            serde_json::json!({ "repoId": "<repoId>" }),
        ),
        tauri_api_definition(
            "Repository API",
            "relocate_repository",
            "重定位仓库路径。",
            serde_json::json!({ "request": { "repoId": "<repoId>", "path": "<absolutePath>" } }),
        ),
        tauri_api_definition(
            "Repository API",
            "update_repository_backend_config",
            "更新仓库后端配置。",
            serde_json::json!({
                "request": {
                    "repoId": "<repoId>",
                    "backendConfig": {}
                }
            }),
        ),
        tauri_api_definition(
            "Repository API",
            "configure_netease_repository_cache",
            "配置网易云资源库本地缓存目录并迁移可识别旧缓存。",
            serde_json::json!({
                "request": {
                    "repoId": "<repoId>",
                    "path": "<absolutePath>",
                    "migrateLegacyCache": true
                }
            }),
        ),
        tauri_api_definition(
            "Repository API",
            "export_repository",
            "导出仓库元数据。",
            serde_json::json!({
                "request": {
                    "repoId": "<repoId>",
                    "target": "archive",
                    "archive": {
                        "format": "zip",
                        "outputPath": "<absolutePath>",
                        "compression": "default",
                        "encrypt": false,
                        "password": null
                    },
                    "git": null
                }
            }),
        ),
        tauri_api_definition(
            "Repository API",
            "sync_repository",
            "同步仓库文件状态。",
            serde_json::json!({ "request": { "repoId": "<repoId>" } }),
        ),
        tauri_api_definition(
            "Hardlink API",
            "list_hardlink_candidates",
            "列出硬链接候选。",
            serde_json::json!({ "repoId": "<repoId>" }),
        ),
        tauri_api_definition(
            "Hardlink API",
            "confirm_hardlink_candidate",
            "确认硬链接候选。",
            serde_json::json!({ "request": { "repoId": "<repoId>", "candidateId": "<candidateId>" } }),
        ),
        tauri_api_definition(
            "Thumbnail API",
            "ensure_thumbnail",
            "按需复用或生成缩略图。",
            serde_json::json!({
                "request": {
                    "repoId": "<repoId>",
                    "path": "<path>",
                    "action": "ensure",
                    "sourcePath": null,
                    "sourceUrl": null,
                    "imageBytes": null,
                    "mediaType": null
                }
            }),
        ),
        tauri_api_definition(
            "Revision API",
            "undo_last_revision",
            "回滚到上一版 metadata 状态。",
            serde_json::json!({ "request": { "repoId": "<repoId>", "assetId": "<assetId>" } }),
        ),
        tauri_api_definition(
            "Revision API",
            "redo_last_revision",
            "重做到下一版 metadata 状态。",
            serde_json::json!({ "request": { "repoId": "<repoId>", "assetId": "<assetId>" } }),
        ),
        tauri_api_definition(
            "Plugin API",
            "list_plugins",
            "列出插件与能力声明。",
            serde_json::json!({}),
        ),
        tauri_api_definition(
            "Plugin API",
            "list_plugin_hook_executions",
            "列出插件 Hook 执行记录。",
            serde_json::json!({ "request": { "pluginId": "<pluginId>", "limit": 50 } }),
        ),
        tauri_api_definition(
            "Plugin API",
            "set_plugin_enabled",
            "启用或停用插件。",
            serde_json::json!({ "request": { "pluginId": "<pluginId>", "enabled": true } }),
        ),
        tauri_api_definition(
            "Plugin API",
            "delete_plugin",
            "删除插件。",
            serde_json::json!({ "pluginId": "<pluginId>" }),
        ),
        tauri_api_definition(
            "Plugin API",
            "install_plugin_from_archive",
            "从插件包安装插件。",
            serde_json::json!({ "request": { "packagePath": "<absolutePackagePath>" } }),
        ),
        tauri_api_definition(
            "Cache API",
            "get_cache_snapshot",
            "读取缓存配置与状态。",
            serde_json::json!({}),
        ),
        tauri_api_definition(
            "Runtime API",
            "get_api_design_snapshot",
            "读取 API 调试契约快照。",
            serde_json::json!({}),
        ),
        tauri_api_definition(
            "Runtime API",
            "get_external_api_connection_status",
            "读取外部 API 连接信息。",
            serde_json::json!({}),
        ),
    ]
}

fn plugin_api_definitions(service_root: &Path) -> Vec<ApiDefinition> {
    let registry = backend_plugin_registry(service_root);
    let mut definitions = Vec::new();
    let mut seen = HashSet::<(String, String)>::new();

    for manifest in registry.list_manifests() {
        if !plugin_manifest_can_be_called(&manifest) {
            continue;
        }
        let Some(contributes) = manifest.contributes.as_object() else {
            continue;
        };

        if let Some(raw_tests) = contributes.get("apiTests") {
            if let Ok(tests) =
                serde_json::from_value::<Vec<PluginApiTestContribution>>(raw_tests.clone())
            {
                for test in tests {
                    if test.method.trim().is_empty() {
                        continue;
                    }
                    let method = test.method.trim().to_string();
                    if !seen.insert((manifest.plugin_id.clone(), method.clone())) {
                        continue;
                    }
                    let summary = test
                        .summary
                        .filter(|value| !value.trim().is_empty())
                        .unwrap_or_else(|| format!("调用插件 API {method}。"));
                    let payload = test
                        .payload
                        .or(test.request_template)
                        .unwrap_or_else(|| serde_json::json!({}));
                    definitions.push(plugin_api_definition(
                        &manifest.plugin_id,
                        &manifest.name,
                        &method,
                        &summary,
                        payload,
                    ));
                }
            }
        }

        if let Some(action) = contributes
            .get("provider")
            .and_then(|provider| provider.get("lookup"))
            .and_then(|lookup| lookup.get("action"))
            .and_then(|action| action.as_str())
            .map(str::trim)
            .filter(|action| !action.is_empty())
        {
            if seen.insert((manifest.plugin_id.clone(), action.to_string())) {
                definitions.push(plugin_api_definition(
                    &manifest.plugin_id,
                    &manifest.name,
                    action,
                    &format!("查询 {} 元数据候选。", manifest.name),
                    serde_json::json!({ "id": "<externalId>" }),
                ));
            }
        }

        if let Some(action) = contributes
            .get("metadataDefaults")
            .and_then(|defaults| defaults.get("action"))
            .and_then(|action| action.as_str())
            .map(str::trim)
            .filter(|action| !action.is_empty())
        {
            if seen.insert((manifest.plugin_id.clone(), action.to_string())) {
                definitions.push(plugin_api_definition(
                    &manifest.plugin_id,
                    &manifest.name,
                    action,
                    &format!("生成 {} 元数据默认值。", manifest.name),
                    serde_json::json!({
                        "entries": [
                            {
                                "path": "work/track01.mp3",
                                "name": "track01.mp3",
                                "extension": "mp3",
                                "kind": "file"
                            }
                        ]
                    }),
                ));
            }
        }
    }

    definitions.sort_by(|left, right| {
        left.group
            .cmp(&right.group)
            .then_with(|| left.path.cmp(&right.path))
    });
    definitions
}

fn plugin_manifest_can_be_called(manifest: &PluginManifest) -> bool {
    manifest.enabled
        && manifest.sdk == "backend"
        && !matches!(
            manifest.status.as_str(),
            "disabled" | "error" | "unavailable"
        )
}

fn external_api_definition(
    method: &str,
    path: &str,
    summary: &str,
    requires_auth: bool,
    request_template: Option<serde_json::Value>,
) -> ApiDefinition {
    ApiDefinition {
        group: "External Asset API".to_string(),
        transport: "external-http".to_string(),
        method: method.to_string(),
        path: path.to_string(),
        summary: summary.to_string(),
        command: None,
        plugin_id: None,
        plugin_method: None,
        requires_auth: Some(requires_auth),
        request_template,
    }
}

fn tauri_api_definition(
    group: &str,
    command: &str,
    summary: &str,
    request_template: serde_json::Value,
) -> ApiDefinition {
    ApiDefinition {
        group: group.to_string(),
        transport: "tauri-command".to_string(),
        method: "INVOKE".to_string(),
        path: command.to_string(),
        summary: summary.to_string(),
        command: Some(command.to_string()),
        plugin_id: None,
        plugin_method: None,
        requires_auth: None,
        request_template: Some(request_template),
    }
}

fn plugin_api_definition(
    plugin_id: &str,
    plugin_name: &str,
    method: &str,
    summary: &str,
    request_template: serde_json::Value,
) -> ApiDefinition {
    ApiDefinition {
        group: format!("Plugin API / {plugin_name}"),
        transport: "plugin-call".to_string(),
        method: "PLUGIN".to_string(),
        path: format!("{plugin_id}:{method}"),
        summary: summary.to_string(),
        command: None,
        plugin_id: Some(plugin_id.to_string()),
        plugin_method: Some(method.to_string()),
        requires_auth: None,
        request_template: Some(request_template),
    }
}

fn backend_summary_from_registry(
    registry: &BackendPluginRegistry,
    plugin_id: &str,
) -> RepositoryBackendSummary {
    let normalized_plugin_id = registry.normalize_plugin_id(plugin_id);
    if let Some(manifest) = registry.manifest(&normalized_plugin_id) {
        RepositoryBackendSummary {
            plugin_id: manifest.plugin_id.clone(),
            kind: manifest.kind.clone(),
            name: manifest.name.clone(),
            capabilities: manifest.capabilities.clone(),
        }
    } else {
        RepositoryBackendSummary {
            plugin_id: normalized_plugin_id,
            kind: "unavailable".to_string(),
            name: "Unavailable plugin".to_string(),
            capabilities: Vec::new(),
        }
    }
}

fn repository_runtime_status(path: &str, backend_plugin_id: &str, stored_status: &str) -> String {
    if backend_plugin_id == LOCAL_FILESYSTEM_PLUGIN_ID
        || netease_cache_root_path(path, backend_plugin_id).is_some()
    {
        if Path::new(path).is_dir() {
            "ready".to_string()
        } else {
            "missing".to_string()
        }
    } else if backend_plugin_id == NETEASE_CLOUD_MUSIC_PLUGIN_ID {
        "missing".to_string()
    } else {
        stored_status.to_string()
    }
}

fn netease_cache_root_path(path: &str, backend_plugin_id: &str) -> Option<PathBuf> {
    if backend_plugin_id != NETEASE_CLOUD_MUSIC_PLUGIN_ID {
        return None;
    }
    let trimmed = path.trim();
    if trimmed.is_empty() || trimmed.starts_with("netease-cloud-music://") {
        return None;
    }
    Some(PathBuf::from(trimmed))
}

fn repository_local_cache_status(
    path: &str,
    backend_plugin_id: &str,
) -> Option<RepositoryLocalCacheStatus> {
    if backend_plugin_id != NETEASE_CLOUD_MUSIC_PLUGIN_ID {
        return None;
    }
    let cache_root = netease_cache_root_path(path, backend_plugin_id);
    let status = match cache_root.as_ref() {
        Some(root) if root.is_dir() => "ready",
        Some(_) => "missing",
        None => "unconfigured",
    };
    Some(RepositoryLocalCacheStatus {
        required: true,
        path: cache_root.map(|path| path.to_string_lossy().to_string()),
        status: status.to_string(),
    })
}

fn ensure_netease_cache_ready(repo: &RepositoryRecord) -> Result<PathBuf, String> {
    if repo.backend_record.plugin_id != NETEASE_CLOUD_MUSIC_PLUGIN_ID {
        return Err("repository is not a netease cloud music repository".to_string());
    }
    let cache_root = netease_cache_root_path(&repo.summary.path, &repo.backend_record.plugin_id)
        .ok_or_else(|| "网易云资源库缺少本地缓存目录，请先指定缓存目录".to_string())?;
    if !cache_root.is_dir() {
        return Err("网易云资源库缓存目录不可用，请重新指定缓存目录".to_string());
    }
    Ok(cache_root)
}

fn parse_backend_request(
    service_root: &Path,
    request: &RepositoryMutationRequest,
) -> Result<RepositoryBackendRecord, String> {
    let plugin_id = request
        .backend_plugin_id
        .as_deref()
        .unwrap_or(LOCAL_FILESYSTEM_PLUGIN_ID)
        .trim();
    let registry = backend_plugin_registry(service_root);
    let normalized_plugin_id = registry.normalize_plugin_id(plugin_id);
    let registration = registry
        .registration(&normalized_plugin_id)
        .ok_or_else(|| format!("unsupported filesystem backend plugin: {plugin_id}"))?;
    let manifest = &registration.manifest;
    if !is_source_plugin(manifest) {
        return Err(format!(
            "plugin is not a repository source: {}",
            manifest.plugin_id
        ));
    }
    ensure_repository_backend_runtime_available(registration)?;
    let config = request
        .backend_config
        .clone()
        .unwrap_or_else(|| serde_json::json!({}));
    if !config.is_object() {
        return Err("backend config must be a JSON object".to_string());
    }
    Ok(RepositoryBackendRecord {
        plugin_id: manifest.plugin_id.clone(),
        config,
    })
}

fn import_backend_record(
    service_root: &Path,
    metadata: &RepositoryMetadataFileImport,
) -> Option<RepositoryBackendRecord> {
    let plugin_id = metadata
        .backend_plugin_id
        .as_deref()
        .unwrap_or(LOCAL_FILESYSTEM_PLUGIN_ID);
    let registry = backend_plugin_registry(service_root);
    let normalized_plugin_id = registry.normalize_plugin_id(plugin_id);
    registry
        .manifest(&normalized_plugin_id)
        .map(|manifest| RepositoryBackendRecord {
            plugin_id: manifest.plugin_id.clone(),
            config: metadata
                .backend_config
                .clone()
                .unwrap_or_else(|| serde_json::json!({})),
        })
}

fn rewrite_repository_metadata_if_needed(
    service_root: &Path,
    metadata_path: &Path,
    metadata: &RepositoryMetadataFileImport,
    repo_root: &Path,
    next_root_path: Option<&Path>,
) -> Result<(), String> {
    let normalized_plugin_id = metadata
        .backend_plugin_id
        .as_deref()
        .map(|plugin_id| backend_plugin_registry(service_root).normalize_plugin_id(plugin_id))
        .unwrap_or_else(|| LOCAL_FILESYSTEM_PLUGIN_ID.to_string());
    let root_path = next_root_path
        .map(|path| path.to_string_lossy().to_string())
        .or_else(|| metadata.root_path.clone())
        .unwrap_or_else(|| repo_root.to_string_lossy().to_string());

    if metadata.root_path.as_deref() == Some(root_path.as_str())
        && metadata.backend_plugin_id.as_deref() == Some(normalized_plugin_id.as_str())
    {
        return Ok(());
    }

    let rewritten = RepositoryMetadataFile {
        repo_id: metadata.repo_id.clone(),
        name: metadata
            .name
            .clone()
            .unwrap_or_else(|| infer_repository_name(repo_root)),
        root_path,
        backend_plugin_id: normalized_plugin_id,
        backend_config: metadata
            .backend_config
            .clone()
            .unwrap_or_else(|| serde_json::json!({})),
        created_at: metadata.created_at.clone().unwrap_or_else(now_rfc3339),
        schema_version: metadata.schema_version.unwrap_or(REPO_SCHEMA_VERSION),
    };
    let metadata_json = serde_json::to_string_pretty(&rewritten).map_err(json_error)?;
    fs::write(metadata_path, metadata_json).map_err(io_error)
}

fn parse_backend_config_json(value: &str) -> Result<serde_json::Value, serde_json::Error> {
    let parsed = serde_json::from_str::<serde_json::Value>(value)?;
    if parsed.is_object() {
        Ok(parsed)
    } else {
        Ok(serde_json::json!({}))
    }
}

fn preserve_netease_cache_config(
    existing: &RepositoryBackendRecord,
    next_backend_config: &mut serde_json::Value,
) {
    if existing.plugin_id != NETEASE_CLOUD_MUSIC_PLUGIN_ID {
        return;
    }
    let Some(next_object) = next_backend_config.as_object_mut() else {
        return;
    };
    for key in ["sourceUri", "localCachePath"] {
        if next_object.contains_key(key) {
            continue;
        }
        if let Some(value) = existing.config.get(key) {
            next_object.insert(key.to_string(), value.clone());
        }
    }
}

fn netease_source_uri_for_repo(repo: &RepositoryRecord) -> String {
    if repo.summary.path.starts_with("netease-cloud-music://") {
        return repo.summary.path.clone();
    }
    repo.backend_record
        .config
        .get("accountId")
        .and_then(normalized_netease_account_id)
        .map(|account_id| format!("netease-cloud-music://account/{account_id}"))
        .unwrap_or_else(|| repo.summary.path.clone())
}

fn to_from_sql_error(error: serde_json::Error) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, Type::Text, Box::new(error))
}

fn migrate_registry_schema(registry: &Connection) -> Result<(), rusqlite::Error> {
    let mut stmt = registry.prepare("PRAGMA table_info(repositories)")?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    if !columns.iter().any(|column| column == "backend_plugin_id") {
        registry.execute(
            "ALTER TABLE repositories ADD COLUMN backend_plugin_id TEXT NOT NULL DEFAULT 'momobako.local-filesystem'",
            [],
        )?;
    }
    if !columns.iter().any(|column| column == "backend_config_json") {
        registry.execute(
            "ALTER TABLE repositories ADD COLUMN backend_config_json TEXT NOT NULL DEFAULT '{}'",
            [],
        )?;
    }
    Ok(())
}

fn migrate_repository_schema(connection: &Connection) -> Result<(), rusqlite::Error> {
    connection.execute_batch(REPOSITORY_SCHEMA_SQL)?;
    let mut stmt = connection.prepare("PRAGMA table_info(assets)")?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    if !columns.iter().any(|column| column == "thumbnail_path") {
        connection.execute("ALTER TABLE assets ADD COLUMN thumbnail_path TEXT", [])?;
    }
    if !columns.iter().any(|column| column == "is_virtual") {
        connection.execute(
            "ALTER TABLE assets ADD COLUMN is_virtual INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    if !columns.iter().any(|column| column == "provider_id") {
        connection.execute("ALTER TABLE assets ADD COLUMN provider_id TEXT", [])?;
    }
    if !columns.iter().any(|column| column == "provider_item_id") {
        connection.execute("ALTER TABLE assets ADD COLUMN provider_item_id TEXT", [])?;
    }
    if !columns.iter().any(|column| column == "source_payload_json") {
        connection.execute("ALTER TABLE assets ADD COLUMN source_payload_json TEXT", [])?;
    }
    if !columns.iter().any(|column| column == "local_absolute_path") {
        connection.execute("ALTER TABLE assets ADD COLUMN local_absolute_path TEXT", [])?;
    }
    connection.execute_batch(
        r#"
        CREATE INDEX IF NOT EXISTS idx_assets_repo_hash ON assets(repo_id, hash);

        CREATE TABLE IF NOT EXISTS entry_thumbnails (
          repo_id TEXT NOT NULL,
          path TEXT NOT NULL,
          kind TEXT NOT NULL,
          thumbnail_path TEXT NOT NULL,
          custom INTEGER NOT NULL DEFAULT 0,
          updated_at TEXT NOT NULL,
          PRIMARY KEY(repo_id, path, kind)
        );

        CREATE TABLE IF NOT EXISTS hardlink_groups (
          group_id TEXT PRIMARY KEY,
          repo_id TEXT NOT NULL,
          content_hash TEXT NOT NULL,
          size_bytes INTEGER NOT NULL,
          created_at TEXT NOT NULL,
          updated_at TEXT NOT NULL,
          FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
        );

        CREATE UNIQUE INDEX IF NOT EXISTS idx_hardlink_groups_repo_hash_size
        ON hardlink_groups(repo_id, content_hash, size_bytes);

        CREATE TABLE IF NOT EXISTS hardlink_members (
          group_id TEXT NOT NULL,
          repo_id TEXT NOT NULL,
          asset_id TEXT NOT NULL,
          path TEXT NOT NULL,
          link_state TEXT NOT NULL,
          linked_at TEXT NOT NULL,
          verified_at TEXT NOT NULL,
          PRIMARY KEY(repo_id, asset_id),
          FOREIGN KEY(group_id) REFERENCES hardlink_groups(group_id),
          FOREIGN KEY(asset_id) REFERENCES assets(asset_id)
        );

        CREATE INDEX IF NOT EXISTS idx_hardlink_members_repo_path
        ON hardlink_members(repo_id, path);

        CREATE TABLE IF NOT EXISTS hardlink_candidates (
          candidate_id TEXT PRIMARY KEY,
          repo_id TEXT NOT NULL,
          new_asset_id TEXT NOT NULL,
          new_path TEXT NOT NULL,
          existing_asset_id TEXT NOT NULL,
          existing_path TEXT NOT NULL,
          content_hash TEXT NOT NULL,
          size_bytes INTEGER NOT NULL,
          created_at TEXT NOT NULL,
          FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
        );

        CREATE UNIQUE INDEX IF NOT EXISTS idx_hardlink_candidates_unique
        ON hardlink_candidates(repo_id, new_asset_id, existing_asset_id);

        CREATE TABLE IF NOT EXISTS asset_alias_groups (
          alias_group_id TEXT PRIMARY KEY,
          repo_id TEXT NOT NULL,
          source TEXT NOT NULL,
          created_at TEXT NOT NULL,
          updated_at TEXT NOT NULL,
          FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
        );

        CREATE TABLE IF NOT EXISTS asset_alias_members (
          alias_group_id TEXT NOT NULL,
          repo_id TEXT NOT NULL,
          asset_id TEXT NOT NULL,
          path TEXT NOT NULL,
          role TEXT NOT NULL,
          created_at TEXT NOT NULL,
          PRIMARY KEY(repo_id, asset_id),
          FOREIGN KEY(alias_group_id) REFERENCES asset_alias_groups(alias_group_id),
          FOREIGN KEY(asset_id) REFERENCES assets(asset_id)
        );

        CREATE INDEX IF NOT EXISTS idx_asset_alias_members_group
        ON asset_alias_members(repo_id, alias_group_id, path);

        CREATE TABLE IF NOT EXISTS repository_shortcuts (
          shortcut_id TEXT PRIMARY KEY,
          repo_id TEXT NOT NULL,
          label TEXT NOT NULL,
          target_kind TEXT NOT NULL,
          target_path TEXT,
          target_id TEXT,
          sort_order INTEGER NOT NULL DEFAULT 0,
          created_at TEXT NOT NULL,
          FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
        );

        CREATE INDEX IF NOT EXISTS idx_repository_shortcuts_repo_order
        ON repository_shortcuts(repo_id, sort_order, label);

        CREATE TABLE IF NOT EXISTS tag_groups (
          tag_group_id TEXT PRIMARY KEY,
          repo_id TEXT NOT NULL,
          name TEXT NOT NULL,
          sort_order INTEGER NOT NULL DEFAULT 0,
          created_at TEXT NOT NULL,
          updated_at TEXT NOT NULL,
          FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
        );

        CREATE TABLE IF NOT EXISTS tag_group_members (
          tag_group_id TEXT NOT NULL,
          repo_id TEXT NOT NULL,
          tag TEXT NOT NULL,
          normalized_tag TEXT NOT NULL,
          sort_order INTEGER NOT NULL DEFAULT 0,
          PRIMARY KEY(tag_group_id, normalized_tag),
          FOREIGN KEY(tag_group_id) REFERENCES tag_groups(tag_group_id)
        );

        CREATE INDEX IF NOT EXISTS idx_tag_group_members_repo_tag
        ON tag_group_members(repo_id, normalized_tag);

        CREATE TABLE IF NOT EXISTS folder_metadata (
          repo_id TEXT NOT NULL,
          path TEXT NOT NULL,
          protected INTEGER NOT NULL DEFAULT 0,
          password_tip TEXT,
          updated_at TEXT NOT NULL,
          PRIMARY KEY(repo_id, path),
          FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
        );

        CREATE TABLE IF NOT EXISTS smart_folders (
          smart_folder_id TEXT PRIMARY KEY,
          repo_id TEXT NOT NULL,
          parent_id TEXT,
          name TEXT NOT NULL,
          filter_json TEXT NOT NULL,
          sort_order INTEGER NOT NULL DEFAULT 0,
          created_at TEXT NOT NULL,
          updated_at TEXT NOT NULL,
          FOREIGN KEY(repo_id) REFERENCES repositories(repo_id),
          FOREIGN KEY(parent_id) REFERENCES smart_folders(smart_folder_id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_smart_folders_repo_parent
        ON smart_folders(repo_id, parent_id, sort_order, name);

        CREATE TABLE IF NOT EXISTS repository_actions (
          action_id TEXT PRIMARY KEY,
          repo_id TEXT NOT NULL,
          source TEXT NOT NULL,
          source_action_id TEXT,
          name TEXT NOT NULL,
          status TEXT NOT NULL,
          enabled INTEGER NOT NULL DEFAULT 1,
          raw_json TEXT NOT NULL,
          unsupported_reason TEXT,
          sort_order INTEGER NOT NULL DEFAULT 0,
          created_at TEXT NOT NULL,
          updated_at TEXT NOT NULL,
          FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
        );

        CREATE INDEX IF NOT EXISTS idx_repository_actions_repo_order
        ON repository_actions(repo_id, sort_order, name);

        CREATE TABLE IF NOT EXISTS repository_action_steps (
          step_id TEXT PRIMARY KEY,
          action_id TEXT NOT NULL,
          repo_id TEXT NOT NULL,
          step_kind TEXT NOT NULL,
          label TEXT NOT NULL,
          status TEXT NOT NULL,
          config_json TEXT NOT NULL,
          raw_json TEXT NOT NULL,
          unsupported_reason TEXT,
          sort_order INTEGER NOT NULL DEFAULT 0,
          FOREIGN KEY(action_id) REFERENCES repository_actions(action_id) ON DELETE CASCADE,
          FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
        );

        CREATE INDEX IF NOT EXISTS idx_repository_action_steps_action_order
        ON repository_action_steps(action_id, sort_order);

        CREATE TABLE IF NOT EXISTS repository_action_runs (
          run_id TEXT PRIMARY KEY,
          action_id TEXT NOT NULL,
          repo_id TEXT NOT NULL,
          status TEXT NOT NULL,
          target_json TEXT NOT NULL,
          message TEXT,
          started_at TEXT NOT NULL,
          finished_at TEXT,
          FOREIGN KEY(action_id) REFERENCES repository_actions(action_id),
          FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
        );

        CREATE INDEX IF NOT EXISTS idx_repository_action_runs_action_time
        ON repository_action_runs(action_id, started_at DESC);

        CREATE TABLE IF NOT EXISTS repository_action_run_steps (
          run_step_id TEXT PRIMARY KEY,
          run_id TEXT NOT NULL,
          step_id TEXT NOT NULL,
          repo_id TEXT NOT NULL,
          status TEXT NOT NULL,
          message TEXT,
          started_at TEXT NOT NULL,
          finished_at TEXT,
          FOREIGN KEY(run_id) REFERENCES repository_action_runs(run_id) ON DELETE CASCADE,
          FOREIGN KEY(step_id) REFERENCES repository_action_steps(step_id),
          FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
        );
        "#,
    )?;
    connection.execute_batch(
        r#"
        INSERT OR IGNORE INTO metadata (asset_id, key, value_type, value_json, version, updated_at)
        SELECT asset_id, 'rating', 'number', '0', 1, updated_at
        FROM assets;

        INSERT OR IGNORE INTO metadata (asset_id, key, value_type, value_json, version, updated_at)
        SELECT asset_id, 'comment', 'string', '""', 1, updated_at
        FROM assets;

        INSERT OR IGNORE INTO metadata (asset_id, key, value_type, value_json, version, updated_at)
        SELECT asset_id, 'link', 'string', '""', 1, updated_at
        FROM assets;

        INSERT OR IGNORE INTO metadata (asset_id, key, value_type, value_json, version, updated_at)
        SELECT asset_id, 'tagGroups', 'json', '[]', 1, updated_at
        FROM assets;

        INSERT OR IGNORE INTO metadata (asset_id, key, value_type, value_json, version, updated_at)
        SELECT asset_id, 'addedToLibraryAt', 'string', json_quote(created_at), 1, updated_at
        FROM assets;
        "#,
    )?;
    Ok(())
}

fn ensure_backend_path_is_attachable(
    service_root: &Path,
    backend: &RepositoryBackendRecord,
    repo_root: &Path,
) -> Result<(), String> {
    let adapter = RuntimeFileSystemBackendAdapter {
        service_root: service_root.to_path_buf(),
        plugin_id: backend.plugin_id.clone(),
    };
    adapter.ensure_attachable(repo_root, &backend.config)
}

fn initialize_repository_directory(
    service_root: &Path,
    repo_root: &Path,
    seed: &RepositorySeed<'_>,
    backend: &RepositoryBackendRecord,
) -> Result<(), String> {
    let adapter = RuntimeFileSystemBackendAdapter {
        service_root: service_root.to_path_buf(),
        plugin_id: backend.plugin_id.clone(),
    };
    adapter.prepare_repository_root(repo_root, &backend.config)?;
    let storage_paths =
        ensure_repository_storage_paths(service_root, seed.repo_id, repo_root, &backend.plugin_id)?;
    let meta_dir = storage_paths.metadata_dir;
    hide_repository_meta_dir(&meta_dir);

    let now = now_rfc3339();
    let metadata = RepositoryMetadataFile {
        repo_id: seed.repo_id.to_string(),
        name: seed.name.to_string(),
        root_path: repo_root.to_string_lossy().to_string(),
        backend_plugin_id: backend.plugin_id.clone(),
        backend_config: backend.config.clone(),
        created_at: now.clone(),
        schema_version: REPO_SCHEMA_VERSION,
    };
    let metadata_json = serde_json::to_string_pretty(&metadata).map_err(json_error)?;
    fs::write(meta_dir.join(REPO_METADATA_FILE_NAME), metadata_json).map_err(io_error)?;

    let connection = Connection::open(storage_paths.database_path).map_err(db_error)?;
    migrate_repository_schema(&connection).map_err(db_error)?;
    seed_repository_data(&connection, seed, &now)?;

    Ok(())
}

fn seed_repository_data(
    connection: &Connection,
    seed: &RepositorySeed<'_>,
    now: &str,
) -> Result<(), String> {
    connection
        .execute(
            r#"
            INSERT OR REPLACE INTO repositories (repo_id, name, root_path, schema_version, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            "#,
            params![
                seed.repo_id,
                seed.name,
                seed.root_path,
                REPO_SCHEMA_VERSION,
                now,
                now
            ],
        )
        .map_err(db_error)?;

    for asset in seed.assets {
        connection
            .execute(
                r#"
                INSERT OR REPLACE INTO assets (
                  asset_id, repo_id, path, filename, extension, size_bytes,
                  created_at, modified_at, hash, status, version, updated_at
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 1, ?11)
                "#,
                params![
                    asset.asset_id,
                    seed.repo_id,
                    asset.path,
                    asset.filename,
                    asset.extension,
                    asset.size_bytes,
                    now,
                    asset.modified_at,
                    format!("sha256:{}", safe_prefix(asset.asset_id, 12)),
                    asset.status,
                    asset.modified_at
                ],
            )
            .map_err(db_error)?;

        for tag in asset.tags {
            connection
                .execute(
                    r#"
                    INSERT OR REPLACE INTO tags (asset_id, tag, normalized_tag)
                    VALUES (?1, ?2, ?3)
                    "#,
                    params![asset.asset_id, tag, tag.to_lowercase()],
                )
                .map_err(db_error)?;
        }

        let before = serde_json::json!({});
        let mut after_map = BTreeMap::new();
        for (key, value_type, value_json) in asset.metadata {
            connection
                .execute(
                    r#"
                    INSERT OR REPLACE INTO metadata (asset_id, key, value_type, value_json, version, updated_at)
                    VALUES (?1, ?2, ?3, ?4, 1, ?5)
                    "#,
                    params![asset.asset_id, key, value_type, value_json, asset.modified_at],
                )
                .map_err(db_error)?;
            let parsed_value: serde_json::Value =
                serde_json::from_str(value_json).map_err(json_error)?;
            after_map.insert((*key).to_string(), parsed_value);
        }

        connection
            .execute(
                r#"
                INSERT OR REPLACE INTO revisions (
                  revision_id, repo_id, asset_id, timestamp, operation, before_json, after_json, source
                )
                VALUES (?1, ?2, ?3, ?4, 'metadata.seeded', ?5, ?6, 'seed')
                "#,
                params![
                    format!("rev-{}", asset.asset_id),
                    seed.repo_id,
                    asset.asset_id,
                    asset.modified_at,
                    before.to_string(),
                    serde_json::to_string(&after_map).map_err(json_error)?
                ],
            )
            .map_err(db_error)?;

        connection
            .execute(
                r#"
                INSERT OR REPLACE INTO events (
                  event_id, repo_id, asset_id, event_type, path, payload_json, created_at
                )
                VALUES (?1, ?2, ?3, 'asset.discovered', ?4, ?5, ?6)
                "#,
                params![
                    format!("evt-{}", asset.asset_id),
                    seed.repo_id,
                    asset.asset_id,
                    asset.path,
                    serde_json::json!({ "status": asset.status }).to_string(),
                    asset.modified_at
                ],
            )
            .map_err(db_error)?;
    }

    Ok(())
}

fn upsert_registry_entry(
    registry: &Connection,
    repo_root: &Path,
    seed: &RepositorySeed<'_>,
    backend: &RepositoryBackendRecord,
) -> Result<(), String> {
    let now = now_rfc3339();
    registry
        .execute(
            r#"
            INSERT OR REPLACE INTO repositories (
              repo_id, name, path, backend_plugin_id, backend_config_json, status, created_at, updated_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            "#,
            params![
                seed.repo_id,
                seed.name,
                repo_root.to_string_lossy().to_string(),
                backend.plugin_id,
                backend.config.to_string(),
                seed.status,
                now,
                now
            ],
        )
        .map_err(db_error)?;
    Ok(())
}

fn repository_state_storage_root(service_root: &Path) -> PathBuf {
    service_root.join("repositories")
}

fn repository_state_storage_dir(service_root: &Path, repo_id: &str) -> PathBuf {
    repository_state_storage_root(service_root).join(repo_id)
}

fn ensure_repository_storage_paths(
    service_root: &Path,
    repo_id: &str,
    repo_root: &Path,
    backend_plugin_id: &str,
) -> Result<RepositoryStoragePaths, String> {
    let metadata_dir = if backend_plugin_id == LOCAL_FILESYSTEM_PLUGIN_ID
        || netease_cache_root_path(&repo_root.to_string_lossy(), backend_plugin_id).is_some()
    {
        migrate_legacy_meta_dir_if_needed(repo_root, backend_plugin_id)?;
        let metadata_dir = repository_meta_dir(repo_root);
        if repo_root.exists() {
            ensure_repository_metadata_dirs(&metadata_dir)?;
            hide_repository_meta_dir(&metadata_dir);
        }
        metadata_dir
    } else {
        let service_repo_dir = repository_state_storage_dir(service_root, repo_id);
        fs::create_dir_all(&service_repo_dir).map_err(io_error)?;
        let metadata_dir = service_repo_dir.join(REPO_META_DIR);
        ensure_repository_metadata_dirs(&metadata_dir)?;
        metadata_dir
    };
    Ok(RepositoryStoragePaths {
        database_path: metadata_dir.join(REPO_DB_FILE_NAME),
        metadata_dir,
    })
}

fn ensure_repository_metadata_dirs(metadata_dir: &Path) -> Result<(), String> {
    fs::create_dir_all(metadata_dir).map_err(io_error)?;
    for subdir in ["cache", "thumbnails", "logs", "indexes", REPO_TRASH_DIR] {
        fs::create_dir_all(metadata_dir.join(subdir)).map_err(io_error)?;
    }
    Ok(())
}

fn write_repository_metadata(
    metadata_dir: &Path,
    repo_id: &str,
    name: &str,
    repo_root: &Path,
    backend_plugin_id: &str,
    backend_config: &serde_json::Value,
    created_at: Option<String>,
) -> Result<(), String> {
    fs::create_dir_all(metadata_dir).map_err(io_error)?;
    let metadata = RepositoryMetadataFile {
        repo_id: repo_id.to_string(),
        name: name.to_string(),
        root_path: repo_root.to_string_lossy().to_string(),
        backend_plugin_id: backend_plugin_id.to_string(),
        backend_config: backend_config.clone(),
        created_at: created_at.unwrap_or_else(now_rfc3339),
        schema_version: REPO_SCHEMA_VERSION,
    };
    let metadata_json = serde_json::to_string_pretty(&metadata).map_err(json_error)?;
    fs::write(metadata_dir.join(REPO_METADATA_FILE_NAME), metadata_json).map_err(io_error)
}

fn normalize_external_cache_root(path: &str) -> Result<PathBuf, String> {
    let cache_root = PathBuf::from(path);
    if cache_root.exists() {
        return canonicalize_local_path(&cache_root);
    }
    if let Some(parent) = cache_root.parent() {
        if parent.exists() {
            let parent = canonicalize_local_path(parent)?;
            if let Some(name) = cache_root.file_name() {
                return Ok(parent.join(name));
            }
        }
    }
    if cache_root.is_relative() {
        return Ok(std::env::current_dir().map_err(io_error)?.join(cache_root));
    }
    Ok(cache_root)
}

fn merge_netease_cache_state_contents(source: &Path, target: &Path) -> Result<usize, String> {
    fs::create_dir_all(target).map_err(io_error)?;
    if !source.exists() {
        return Ok(0);
    }
    let mut moved = 0;
    for entry in fs::read_dir(source).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let file_name = entry.file_name();
        let source_path = entry.path();
        let target_path = target.join(&file_name);
        if target_path.exists() {
            if source_path.is_dir() && target_path.is_dir() {
                moved += merge_netease_cache_state_contents(&source_path, &target_path)?;
            }
            continue;
        }
        fs::rename(&source_path, &target_path).map_err(io_error)?;
        moved += 1;
    }
    Ok(moved)
}

fn netease_playback_cache_dir(cache_root: &Path) -> PathBuf {
    repository_meta_dir(cache_root)
        .join("cache")
        .join("netease-playback")
}

fn downloader_legacy_temp_dir(service_root: &Path) -> PathBuf {
    plugin_data_dir(service_root, "momobako.service.downloader").join("temp")
}

fn netease_downloader_cache_key(song_id: i64, level: &str, account_id: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(format!("{song_id}:{level}:{account_id}"));
    format!("{:x}", Sha1Digest::finalize(hasher))
}

fn value_as_string(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(text) => {
            let trimmed = text.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }
        serde_json::Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn migrate_netease_playback_cache(
    service_root: &Path,
    repo: &RepositoryRecord,
    cache_root: &Path,
    migration: &mut NeteaseRepositoryCacheMigrationSummary,
) -> Result<(), String> {
    let legacy_temp_dir = downloader_legacy_temp_dir(service_root);
    if !legacy_temp_dir.exists() {
        return Ok(());
    }
    let account_id = repo
        .backend_record
        .config
        .get("accountId")
        .and_then(value_as_string)
        .unwrap_or_else(|| "anonymous".to_string());
    let default_level = repo
        .backend_record
        .config
        .get("defaultLevel")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("standard");
    let connection =
        match self_open_repository_connection_for_cache_migration(service_root, repo, cache_root) {
            Ok(connection) => connection,
            Err(_) => return Ok(()),
        };
    let asset_map = load_asset_path_map(&connection, &repo.summary.repo_id).map_err(db_error)?;
    let target_dir = netease_playback_cache_dir(cache_root);
    fs::create_dir_all(&target_dir).map_err(io_error)?;

    for asset in asset_map.values() {
        if asset.provider_id.as_deref() != Some("netease-cloud-music") {
            continue;
        }
        let Some(payload) = asset.source_payload.as_ref() else {
            migration.skipped_playback_cache_files += 1;
            continue;
        };
        let song_id = payload
            .get("songId")
            .and_then(serde_json::Value::as_i64)
            .or_else(|| {
                payload
                    .get("songId")
                    .and_then(serde_json::Value::as_str)
                    .and_then(|value| value.parse::<i64>().ok())
            });
        let Some(song_id) = song_id else {
            migration.skipped_playback_cache_files += 1;
            continue;
        };
        let level = payload
            .get("level")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(default_level);
        let cache_key = netease_downloader_cache_key(song_id, level, &account_id);
        for extension in ["mp3", "lrc", "yrc"] {
            let source_path = legacy_temp_dir.join(format!("{cache_key}.{extension}"));
            if !source_path.exists() {
                continue;
            }
            let target_path = target_dir.join(format!("{cache_key}.{extension}"));
            if target_path.exists() {
                migration.skipped_playback_cache_files += 1;
                continue;
            }
            match fs::rename(&source_path, &target_path) {
                Ok(()) => migration.migrated_playback_cache_files += 1,
                Err(_) => migration.failed_playback_cache_files += 1,
            }
        }
    }
    Ok(())
}

fn self_open_repository_connection_for_cache_migration(
    service_root: &Path,
    repo: &RepositoryRecord,
    cache_root: &Path,
) -> Result<Connection, String> {
    let storage_paths = ensure_repository_storage_paths(
        service_root,
        &repo.summary.repo_id,
        cache_root,
        &repo.backend_record.plugin_id,
    )?;
    let connection = Connection::open(storage_paths.database_path).map_err(db_error)?;
    migrate_repository_schema(&connection).map_err(db_error)?;
    Ok(connection)
}

fn infer_repository_name(repo_root: &Path) -> String {
    repo_root
        .file_name()
        .map(|name| name.to_string_lossy().trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| repo_root.to_string_lossy().to_string())
}

fn repository_meta_dir(repo_root: &Path) -> PathBuf {
    repo_root.join(REPO_META_DIR)
}

fn repository_trash_dir(repo_root: &Path) -> PathBuf {
    repository_meta_dir(repo_root).join(REPO_TRASH_DIR)
}

fn repository_trash_manifest_path(repo_root: &Path) -> PathBuf {
    repository_meta_dir(repo_root).join(REPO_TRASH_MANIFEST_FILE_NAME)
}

fn legacy_repository_meta_dir(repo_root: &Path) -> PathBuf {
    repo_root.join(LEGACY_REPO_META_DIR)
}

fn is_internal_repository_dir(name: &str) -> bool {
    name == REPO_META_DIR || name == LEGACY_REPO_META_DIR
}

fn migrate_legacy_meta_dir_if_needed(
    repo_root: &Path,
    _backend_plugin_id: &str,
) -> Result<(), String> {
    let current_dir = repository_meta_dir(repo_root);
    if current_dir.exists() {
        hide_repository_meta_dir(&current_dir);
        return Ok(());
    }

    let legacy_dir = legacy_repository_meta_dir(repo_root);
    if legacy_dir.exists() {
        fs::rename(&legacy_dir, &current_dir).map_err(io_error)?;
        hide_repository_meta_dir(&current_dir);
    }

    Ok(())
}

fn hide_repository_meta_dir(path: &Path) {
    #[cfg(target_os = "windows")]
    {
        let _ = Command::new("attrib").arg("+H").arg(path).status();
    }
}

fn normalize_repository_root_for_backend(
    path: &str,
    backend: &RepositoryBackendRecord,
    must_exist: bool,
) -> Result<PathBuf, String> {
    let repo_root = PathBuf::from(path);
    if backend.plugin_id != LOCAL_FILESYSTEM_PLUGIN_ID {
        return Ok(repo_root);
    }

    if must_exist || repo_root.exists() {
        return canonicalize_local_path(&repo_root);
    }

    if let Some(parent) = repo_root.parent() {
        if parent.exists() {
            let parent = canonicalize_local_path(parent)?;
            if let Some(name) = repo_root.file_name() {
                return Ok(parent.join(name));
            }
        }
    }

    if repo_root.is_relative() {
        return Ok(std::env::current_dir().map_err(io_error)?.join(repo_root));
    }

    Ok(repo_root)
}

fn canonicalize_local_path(path: &Path) -> Result<PathBuf, String> {
    let canonical = path.canonicalize().map_err(io_error)?;
    Ok(strip_windows_verbatim_prefix(canonical))
}

#[cfg(target_os = "windows")]
fn strip_windows_verbatim_prefix(path: PathBuf) -> PathBuf {
    let value = path.to_string_lossy();
    if let Some(stripped) = value.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{stripped}"));
    }
    if let Some(stripped) = value.strip_prefix(r"\\?\") {
        return PathBuf::from(stripped);
    }
    path
}

#[cfg(not(target_os = "windows"))]
fn strip_windows_verbatim_prefix(path: PathBuf) -> PathBuf {
    path
}

fn normalize_directory_path(path: &str) -> Result<String, String> {
    normalize_relative_path(path, true)
}

fn normalize_entry_path(path: &str) -> Result<String, String> {
    let normalized = normalize_relative_path(path, false)?;
    if normalized.is_empty() {
        return Err("path cannot be empty".to_string());
    }
    Ok(normalized)
}

fn normalize_special_location(value: Option<&str>) -> Result<Option<String>, String> {
    match value.map(str::trim).filter(|item| !item.is_empty()) {
        Some("trash") => Ok(Some("trash".to_string())),
        Some(value) => Err(format!("unsupported file browser location: {value}")),
        None => Ok(None),
    }
}

fn normalize_relative_path(path: &str, allow_empty: bool) -> Result<String, String> {
    let trimmed = path.trim().replace('\\', "/").trim_matches('/').to_string();
    if trimmed.is_empty() {
        return if allow_empty {
            Ok(String::new())
        } else {
            Err("path cannot be empty".to_string())
        };
    }

    let mut parts = Vec::new();
    for part in trimmed.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return Err("path cannot escape repository root".to_string());
        }
        if is_internal_repository_dir(part) {
            return Err("internal repository directory is reserved".to_string());
        }
        parts.push(part);
    }

    let normalized = parts.join("/");
    if normalized.is_empty() && allow_empty {
        Ok(String::new())
    } else if normalized.is_empty() {
        Err("path cannot be empty".to_string())
    } else {
        Ok(normalized)
    }
}

fn resolve_repository_relative_path(
    repo_root: &Path,
    relative_path: &str,
) -> Result<PathBuf, String> {
    if relative_path.is_empty() {
        return Ok(repo_root.to_path_buf());
    }

    let normalized = normalize_relative_path(relative_path, true)?;
    let mut path = repo_root.to_path_buf();
    for part in normalized.split('/') {
        if !part.is_empty() {
            path.push(part);
        }
    }
    Ok(path)
}

fn resolve_trash_relative_path(trash_root: &Path, relative_path: &str) -> Result<PathBuf, String> {
    if relative_path.is_empty() {
        return Ok(trash_root.to_path_buf());
    }

    let normalized = normalize_trash_relative_path(relative_path, true)?;
    let mut path = trash_root.to_path_buf();
    for part in normalized.split('/') {
        if !part.is_empty() {
            path.push(part);
        }
    }
    Ok(path)
}

fn normalize_trash_relative_path(path: &str, allow_empty: bool) -> Result<String, String> {
    let trimmed = path.trim().replace('\\', "/").trim_matches('/').to_string();
    if trimmed.is_empty() {
        return if allow_empty {
            Ok(String::new())
        } else {
            Err("path cannot be empty".to_string())
        };
    }

    let mut parts = Vec::new();
    for part in trimmed.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return Err("path cannot escape trash root".to_string());
        }
        parts.push(part);
    }

    let normalized = parts.join("/");
    if normalized.is_empty() && allow_empty {
        Ok(String::new())
    } else if normalized.is_empty() {
        Err("path cannot be empty".to_string())
    } else {
        Ok(normalized)
    }
}

fn unique_trash_target_path(trash_root: &Path, entry_path: &str) -> Result<PathBuf, String> {
    let entry = Path::new(entry_path);
    let name = entry
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .ok_or_else(|| format!("invalid entry path: {entry_path}"))?;

    let parent_path = parent_relative_path(entry_path);
    let target_parent = resolve_trash_relative_path(trash_root, &parent_path)?;
    fs::create_dir_all(&target_parent).map_err(io_error)?;

    let mut target = target_parent.join(&name);
    if !target.exists() {
        return Ok(target);
    }

    let stem = entry
        .file_stem()
        .map(|value| value.to_string_lossy().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| name.clone());
    let extension = entry
        .extension()
        .map(|value| value.to_string_lossy().to_string());
    let timestamp = trash_timestamp_suffix();
    let mut suffix = 1;
    while target.exists() {
        let candidate_name = match &extension {
            Some(extension) => format!("{stem} (deleted-{timestamp}-{suffix}).{extension}"),
            None => format!("{stem} (deleted-{timestamp}-{suffix})"),
        };
        target = target_parent.join(candidate_name);
        suffix += 1;
    }
    Ok(target)
}

fn trash_timestamp_suffix() -> String {
    now_rfc3339()
        .replace(':', "")
        .replace('.', "-")
        .replace('Z', "z")
}

fn trash_relative_path_for_target(trash_root: &Path, target_abs: &Path) -> Result<String, String> {
    target_abs
        .strip_prefix(trash_root)
        .map_err(path_error)
        .map(|path| path.to_string_lossy().replace('\\', "/"))
}

fn load_trash_manifest(repo_root: &Path) -> Result<TrashManifest, String> {
    let manifest_path = repository_trash_manifest_path(repo_root);
    if !manifest_path.exists() {
        return Ok(TrashManifest::default());
    }

    let raw = fs::read_to_string(manifest_path).map_err(io_error)?;
    if raw.trim().is_empty() {
        return Ok(TrashManifest::default());
    }
    serde_json::from_str::<TrashManifest>(&raw).map_err(json_error)
}

fn save_trash_manifest(repo_root: &Path, manifest: &TrashManifest) -> Result<(), String> {
    let meta_dir = repository_meta_dir(repo_root);
    fs::create_dir_all(&meta_dir).map_err(io_error)?;
    let manifest_json = serde_json::to_string_pretty(manifest).map_err(json_error)?;
    fs::write(repository_trash_manifest_path(repo_root), manifest_json).map_err(io_error)
}

fn trash_path_matches_or_descends(path: &str, ancestor: &str) -> bool {
    path == ancestor || path.starts_with(&format!("{ancestor}/"))
}

fn relative_suffix(path: &str, ancestor: &str) -> Option<String> {
    if path == ancestor {
        Some(String::new())
    } else {
        path.strip_prefix(&format!("{ancestor}/"))
            .map(ToString::to_string)
    }
}

fn find_trash_manifest_entry<'a>(
    manifest: &'a TrashManifest,
    trash_path: &str,
) -> Option<&'a TrashManifestEntry> {
    manifest
        .entries
        .iter()
        .filter(|entry| trash_path_matches_or_descends(trash_path, &entry.trash_path))
        .max_by_key(|entry| entry.trash_path.len())
}

fn original_path_for_trash_path(entry: &TrashManifestEntry, trash_path: &str) -> String {
    match relative_suffix(trash_path, &entry.trash_path) {
        Some(suffix) if suffix.is_empty() => entry.original_path.clone(),
        Some(suffix) => join_relative_path(&entry.original_path, &suffix),
        None => entry.original_path.clone(),
    }
}

fn remove_manifest_paths(manifest: &mut TrashManifest, trash_path: &str) {
    manifest
        .entries
        .retain(|entry| !trash_path_matches_or_descends(&entry.trash_path, trash_path));
}

fn prune_empty_trash_parents(trash_root: &Path, restored_trash_path: &str) -> Result<(), String> {
    let mut current = parent_relative_path(restored_trash_path);
    while !current.is_empty() {
        let dir = resolve_trash_relative_path(trash_root, &current)?;
        if dir.exists() && dir.is_dir() && fs::read_dir(&dir).map_err(io_error)?.next().is_none() {
            fs::remove_dir(&dir).map_err(io_error)?;
            current = parent_relative_path(&current);
        } else {
            break;
        }
    }
    Ok(())
}

fn ensure_restore_target_available(
    source_abs: &Path,
    target_abs: &Path,
    target_path: &str,
) -> Result<(), String> {
    if !target_abs.exists() {
        return Ok(());
    }
    let source_metadata = source_abs.metadata().map_err(io_error)?;
    if source_metadata.is_dir() && target_abs.is_dir() {
        return ensure_directory_merge_available(source_abs, target_abs, target_path);
    }
    Err(format!("target already exists: {target_path}"))
}

fn ensure_directory_merge_available(
    source_dir: &Path,
    target_dir: &Path,
    target_path: &str,
) -> Result<(), String> {
    for entry in fs::read_dir(source_dir).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let source_child = entry.path();
        let target_child = target_dir.join(entry.file_name());
        if !target_child.exists() {
            continue;
        }
        let source_metadata = entry.metadata().map_err(io_error)?;
        if source_metadata.is_dir() && target_child.is_dir() {
            ensure_directory_merge_available(&source_child, &target_child, target_path)?;
        } else {
            return Err(format!("target already exists: {target_path}"));
        }
    }
    Ok(())
}

fn restore_path_to_target(
    source_abs: &Path,
    target_abs: &Path,
    target_path: &str,
) -> Result<(), String> {
    ensure_restore_target_available(source_abs, target_abs, target_path)?;
    if let Some(parent) = target_abs.parent() {
        fs::create_dir_all(parent).map_err(io_error)?;
    }

    if source_abs.is_dir() && target_abs.is_dir() {
        merge_directory_contents(source_abs, target_abs)
    } else {
        fs::rename(source_abs, target_abs).map_err(io_error)
    }
}

fn merge_directory_contents(source_dir: &Path, target_dir: &Path) -> Result<(), String> {
    fs::create_dir_all(target_dir).map_err(io_error)?;
    for entry in fs::read_dir(source_dir).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let source_child = entry.path();
        let target_child = target_dir.join(entry.file_name());
        if source_child.is_dir() && target_child.is_dir() {
            merge_directory_contents(&source_child, &target_child)?;
        } else {
            fs::rename(&source_child, &target_child).map_err(io_error)?;
        }
    }
    fs::remove_dir(source_dir).map_err(io_error)
}

fn ensure_local_filesystem_repository(repo: &RepositoryRecord, action: &str) -> Result<(), String> {
    if repo.backend_record.plugin_id == LOCAL_FILESYSTEM_PLUGIN_ID {
        Ok(())
    } else {
        Err(format!(
            "{action} is only supported for local filesystem repositories"
        ))
    }
}

fn resolve_file_copy_target(
    repo_root: &Path,
    parent_path: Option<&str>,
    source_paths: &[String],
) -> Result<(String, PathBuf), String> {
    let parent_path = normalize_directory_path(parent_path.unwrap_or_default())?;
    let target_dir = resolve_repository_relative_path(repo_root, &parent_path)?;
    if !target_dir.exists() || !target_dir.is_dir() {
        return Err(format!("directory not found: {parent_path}"));
    }
    if source_paths.is_empty() {
        return Err("no source files were provided".to_string());
    }
    Ok((parent_path, target_dir))
}

#[derive(Debug, Clone)]
struct FileImportPlanEntry {
    source: PathBuf,
    source_relative_path: Option<String>,
    target: PathBuf,
    target_relative_path: String,
    is_directory: bool,
}

#[derive(Debug, Clone)]
struct FileMovePlanEntry {
    source_relative_path: String,
    target_relative_path: String,
    target_name: String,
    is_directory: bool,
}

fn validate_external_import_entries(
    source_paths: &[String],
    repo_root: &Path,
    target_dir: &Path,
) -> Result<Vec<FileImportPlanEntry>, String> {
    let repo_canonical = repo_root.canonicalize().map_err(io_error)?;
    let target_canonical_parent = target_dir.canonicalize().map_err(io_error)?;
    let mut planned_targets = Vec::<PathBuf>::new();
    let mut plan = Vec::with_capacity(source_paths.len());

    for source_path in source_paths {
        let source = PathBuf::from(source_path);
        if !source.exists() {
            return Err(format!(
                "source path does not exist: {}",
                source.to_string_lossy()
            ));
        }

        let name = source
            .file_name()
            .map(|value| value.to_string_lossy().to_string())
            .ok_or_else(|| format!("invalid source path: {}", source.to_string_lossy()))?;
        let name = validate_new_entry_name(&name)?;
        let target = target_dir.join(&name);
        let target_relative_path = target
            .strip_prefix(repo_root)
            .map_err(path_error)?
            .to_string_lossy()
            .replace('\\', "/");
        if target.exists() || planned_targets.iter().any(|planned| planned == &target) {
            return Err(format!("entry already exists: {name}"));
        }

        if source.is_dir() {
            let source_canonical = source.canonicalize().map_err(io_error)?;
            if source_canonical == repo_canonical || repo_canonical.starts_with(&source_canonical) {
                return Err("cannot import a repository folder into itself".to_string());
            }
            if target_canonical_parent.starts_with(&source_canonical) {
                return Err("cannot import a folder into one of its descendants".to_string());
            }
            plan.push(FileImportPlanEntry {
                source: source_canonical,
                source_relative_path: None,
                target: target.clone(),
                target_relative_path: target_relative_path.clone(),
                is_directory: true,
            });
        } else if source.is_file() {
            plan.push(FileImportPlanEntry {
                source,
                source_relative_path: None,
                target: target.clone(),
                target_relative_path: target_relative_path.clone(),
                is_directory: false,
            });
        } else {
            return Err(format!(
                "unsupported source path type: {}",
                source.to_string_lossy()
            ));
        }
        planned_targets.push(target);
    }

    Ok(plan)
}

fn validate_repository_copy_entries(
    source_paths: &[String],
    repo_root: &Path,
    target_dir: &Path,
) -> Result<Vec<FileImportPlanEntry>, String> {
    let target_canonical_parent = target_dir.canonicalize().map_err(io_error)?;
    let mut planned_targets = Vec::<PathBuf>::new();
    let mut plan = Vec::with_capacity(source_paths.len());

    for source_path in source_paths {
        let source_relative = normalize_entry_path(source_path)?;
        let source = resolve_repository_relative_path(repo_root, &source_relative)?;
        if !source.exists() {
            return Err(format!("source path does not exist: {source_relative}"));
        }
        let source_canonical = source.canonicalize().map_err(io_error)?;
        let source_parent = source_canonical
            .parent()
            .ok_or_else(|| format!("invalid source path: {source_relative}"))?;
        if source_parent == target_canonical_parent {
            return Err("不能复制到原目录".to_string());
        }
        let name = source
            .file_name()
            .map(|value| value.to_string_lossy().to_string())
            .ok_or_else(|| format!("invalid source path: {source_relative}"))?;
        let name = validate_new_entry_name(&name)?;
        let target = target_dir.join(&name);
        let target_relative_path = target
            .strip_prefix(repo_root)
            .map_err(path_error)?
            .to_string_lossy()
            .replace('\\', "/");
        if target.exists() || planned_targets.iter().any(|planned| planned == &target) {
            return Err(format!("entry already exists: {name}"));
        }

        if source.is_dir() {
            if target_canonical_parent.starts_with(&source_canonical) {
                return Err("cannot copy a folder into one of its descendants".to_string());
            }
            plan.push(FileImportPlanEntry {
                source: source_canonical,
                source_relative_path: Some(source_relative.clone()),
                target: target.clone(),
                target_relative_path: target_relative_path.clone(),
                is_directory: true,
            });
        } else if source.is_file() {
            plan.push(FileImportPlanEntry {
                source,
                source_relative_path: Some(source_relative.clone()),
                target: target.clone(),
                target_relative_path: target_relative_path.clone(),
                is_directory: false,
            });
        } else {
            return Err(format!("unsupported source path type: {source_relative}"));
        }
        planned_targets.push(target);
    }

    Ok(plan)
}

fn validate_repository_move_entries(
    source_paths: &[String],
    repo_root: &Path,
    target_dir: &Path,
) -> Result<Vec<FileMovePlanEntry>, String> {
    let target_canonical_parent = target_dir.canonicalize().map_err(io_error)?;
    let mut planned_targets = Vec::<PathBuf>::new();
    let mut plan = Vec::with_capacity(source_paths.len());

    for source_path in source_paths {
        let source_relative = normalize_entry_path(source_path)?;
        let source = resolve_repository_relative_path(repo_root, &source_relative)?;
        if !source.exists() {
            return Err(format!("source path does not exist: {source_relative}"));
        }

        let source_canonical = source.canonicalize().map_err(io_error)?;
        let source_parent = source_canonical
            .parent()
            .ok_or_else(|| format!("invalid source path: {source_relative}"))?;
        if source_parent == target_canonical_parent {
            return Err("不能移动到原目录".to_string());
        }

        let name = source
            .file_name()
            .map(|value| value.to_string_lossy().to_string())
            .ok_or_else(|| format!("invalid source path: {source_relative}"))?;
        let target_name = validate_new_entry_name(&name)?;
        let target = target_dir.join(&target_name);
        let target_relative_path = target
            .strip_prefix(repo_root)
            .map_err(path_error)?
            .to_string_lossy()
            .replace('\\', "/");
        if target.exists() || planned_targets.iter().any(|planned| planned == &target) {
            return Err(format!("entry already exists: {target_name}"));
        }

        let is_directory = source.is_dir();
        if is_directory && target_canonical_parent.starts_with(&source_canonical) {
            return Err("文件夹不能移动到自身或其子文件夹内".to_string());
        }
        if !is_directory && !source.is_file() {
            return Err(format!("unsupported source path type: {source_relative}"));
        }

        planned_targets.push(target);
        plan.push(FileMovePlanEntry {
            source_relative_path: source_relative,
            target_relative_path,
            target_name,
            is_directory,
        });
    }

    Ok(plan)
}

fn copy_external_entries_parallel(
    plan: Vec<FileImportPlanEntry>,
    hardlink_preferred: bool,
) -> Result<Vec<HardlinkCopyOutcome>, String> {
    if plan.is_empty() {
        return Ok(Vec::new());
    }

    let worker_count = plan.len().min(MAX_PARALLEL_IMPORTS);
    let queue = Arc::new(Mutex::new(plan.into_iter()));
    let outcomes = Arc::new(Mutex::new(Vec::new()));
    let mut handles = Vec::with_capacity(worker_count);

    for _ in 0..worker_count {
        let queue = queue.clone();
        let outcomes = outcomes.clone();
        handles.push(thread::spawn(move || loop {
            let Some(entry) = ({
                let mut entries = queue
                    .lock()
                    .map_err(|_| "import queue lock poisoned".to_string())?;
                entries.next()
            }) else {
                return Ok(());
            };

            let mut entry_outcomes = if entry.is_directory {
                copy_directory_recursive_with_mode(
                    &entry.source,
                    entry.source_relative_path.as_deref(),
                    &entry.target,
                    &entry.target_relative_path,
                    hardlink_preferred,
                )?
            } else {
                vec![copy_file_with_mode(
                    &entry.source,
                    entry.source_relative_path.as_deref(),
                    &entry.target,
                    &entry.target_relative_path,
                    hardlink_preferred,
                )?]
            };
            outcomes
                .lock()
                .map_err(|_| "import outcome lock poisoned".to_string())?
                .append(&mut entry_outcomes);
        }));
    }

    for handle in handles {
        match handle.join() {
            Ok(Ok(())) => {}
            Ok(Err(error)) => return Err(error),
            Err(_) => return Err("file import worker panicked".to_string()),
        }
    }

    let outcomes = Arc::try_unwrap(outcomes)
        .map_err(|_| "import outcome still shared".to_string())?
        .into_inner()
        .map_err(|_| "import outcome lock poisoned".to_string())?;
    Ok(outcomes)
}

fn external_add_asset_response(
    request_id: String,
    mut imported: Vec<ExternalImportedAsset>,
    mut failed: Vec<ExternalAddAssetFailure>,
    total: usize,
) -> ExternalAddAssetResponse {
    imported.sort_by_key(|item| item.item_index);
    failed.sort_by_key(|item| item.item_index);
    let status = if imported.is_empty() {
        "failed"
    } else if failed.is_empty() {
        "success"
    } else {
        "partial"
    };
    ExternalAddAssetResponse {
        request_id,
        status: status.to_string(),
        summary: ExternalAddAssetSummary {
            total,
            imported: imported.len(),
            failed: failed.len(),
        },
        imported,
        failed,
    }
}

fn external_failure(
    item_index: usize,
    code: &str,
    message: String,
    retryable: bool,
    details: Option<serde_json::Value>,
) -> ExternalAddAssetFailure {
    ExternalAddAssetFailure {
        item_index,
        code: code.to_string(),
        message,
        retryable,
        details,
    }
}

fn external_import_error_code(error: &str) -> &'static str {
    if error.contains("entry already exists") {
        "duplicateTarget"
    } else if error.contains("directory not found")
        || error.contains("path escapes repository root")
        || error.contains("invalid path")
    {
        "invalidTargetPath"
    } else {
        "importRejected"
    }
}

fn external_metadata_source(client: Option<&ExternalAddAssetClient>) -> String {
    let Some(client) = client else {
        return "external".to_string();
    };
    client
        .id
        .as_deref()
        .or(client.name.as_deref())
        .map(|value| format!("external:{value}"))
        .unwrap_or_else(|| "external".to_string())
}

#[derive(Debug)]
struct ExternalRequestError {
    code: &'static str,
    message: String,
    retryable: bool,
}

#[derive(Debug)]
struct ExternalAssetImportContext {
    parent_path: String,
    target_dir: PathBuf,
    staging_root: PathBuf,
}

#[derive(Debug)]
struct PlannedExternalAsset {
    item_index: usize,
    source_path: String,
    target_path: String,
    metadata: Option<BTreeMap<String, serde_json::Value>>,
}

fn external_item_filename(
    item: &ExternalAddAssetItem,
    item_index: usize,
) -> Result<String, String> {
    if let Some(filename) = item
        .filename
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return validate_new_entry_name(filename);
    }
    if let Some(url) = item.url.as_deref() {
        let url_path = url.split(['?', '#']).next().unwrap_or(url);
        if let Some(candidate) = url_path
            .rsplit('/')
            .find(|segment| !segment.trim().is_empty())
            .map(percent_decode_filename)
            .filter(|value| !value.trim().is_empty())
        {
            return validate_new_entry_name(&candidate);
        }
    }
    validate_new_entry_name(&format!("external-asset-{item_index}.bin"))
}

fn percent_decode_filename(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(hex) = u8::from_str_radix(&value[index + 1..index + 3], 16) {
                output.push(hex);
                index += 3;
                continue;
            }
        }
        output.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&output).to_string()
}

fn stage_external_asset_item(
    item_index: usize,
    item: &ExternalAddAssetItem,
    context: &ExternalAssetImportContext,
    planned_targets: &mut HashSet<String>,
) -> Result<PlannedExternalAsset, ExternalAddAssetFailure> {
    if item.kind != "remoteUrl" {
        return Err(external_failure(
            item_index,
            "invalidInput",
            format!("unsupported item kind: {}", item.kind),
            false,
            None,
        ));
    }
    let Some(url) = item
        .url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Err(external_failure(
            item_index,
            "invalidInput",
            "remoteUrl item requires url".to_string(),
            false,
            None,
        ));
    };
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(external_failure(
            item_index,
            "invalidInput",
            "remoteUrl only supports http and https URLs".to_string(),
            false,
            None,
        ));
    }

    let filename = external_item_filename(item, item_index)
        .map_err(|error| external_failure(item_index, "invalidInput", error, false, None))?;
    let target_path = join_relative_path(&context.parent_path, &filename);
    if context.target_dir.join(&filename).exists() || !planned_targets.insert(target_path.clone()) {
        return Err(external_failure(
            item_index,
            "duplicateTarget",
            format!("entry already exists: {filename}"),
            false,
            None,
        ));
    }

    let staged_path = context.staging_root.join(&filename);
    download_remote_asset(url, item.headers.as_ref(), &staged_path).map_err(|error| {
        external_failure(
            item_index,
            "downloadFailed",
            error,
            true,
            Some(serde_json::json!({ "url": url })),
        )
    })?;

    Ok(PlannedExternalAsset {
        item_index,
        source_path: staged_path.to_string_lossy().to_string(),
        target_path,
        metadata: item.metadata.clone(),
    })
}

fn sanitize_external_component(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>();
    let trimmed = sanitized.trim_matches('-');
    if trimmed.is_empty() {
        "request".to_string()
    } else {
        trimmed.to_string()
    }
}

fn download_remote_asset(
    url: &str,
    headers: Option<&BTreeMap<String, String>>,
    output_path: &Path,
) -> Result<(), String> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err("remoteUrl only supports http and https URLs".to_string());
    }
    let client = reqwest::blocking::Client::builder()
        .user_agent("MomoBakoExternalImport/1")
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|error| format!("download client error: {error}"))?;
    let mut request = client.get(url);
    if let Some(headers) = headers {
        for (name, value) in headers {
            if is_safe_external_header_name(name) && !value.contains(['\r', '\n']) {
                request = request.header(name, value);
            }
        }
    }
    let response = request
        .send()
        .map_err(|error| format!("download request failed: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("download returned HTTP {status}"));
    }
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).map_err(io_error)?;
    }
    let mut file = File::create(output_path).map_err(io_error)?;
    let mut response = response;
    response
        .copy_to(&mut file)
        .map_err(|error| format!("download body error: {error}"))?;
    Ok(())
}

fn is_safe_external_header_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-'))
}

fn export_repository_archive(
    repo_root: &Path,
    options: &RepositoryArchiveExportOptions,
) -> Result<(), String> {
    if !repo_root.is_dir() {
        return Err("repository path is not a directory".to_string());
    }

    let output_path = normalize_archive_output_path(options)?;
    if output_path.as_os_str().is_empty() {
        return Err("archive output path cannot be empty".to_string());
    }

    if let Some(parent) = output_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(io_error)?;
        }
    }
    validate_archive_output_path(repo_root, &output_path)?;
    if output_path.exists() {
        if !output_path.is_file() {
            return Err("archive output path must be a file".to_string());
        }
        fs::remove_file(&output_path).map_err(io_error)?;
    }

    match options.format.as_str() {
        "zip" => export_zip_archive(repo_root, &output_path, options),
        "7z" => export_7z_archive(repo_root, &output_path, options),
        "tar" => export_tar_archive(repo_root, &output_path, options),
        value => Err(format!("unsupported archive format: {value}")),
    }
}

fn normalize_archive_output_path(
    options: &RepositoryArchiveExportOptions,
) -> Result<PathBuf, String> {
    let trimmed = options.output_path.trim();
    if trimmed.is_empty() {
        return Err("archive output path cannot be empty".to_string());
    }

    let output_path = PathBuf::from(trimmed);
    if output_path.extension().is_some() {
        return Ok(output_path);
    }

    Ok(output_path.with_extension(match options.format.as_str() {
        "7z" => "7z",
        "tar" if options.compression == "none" => "tar",
        "tar" => "tar.gz",
        _ => "zip",
    }))
}

fn validate_archive_output_path(repo_root: &Path, output_path: &Path) -> Result<(), String> {
    let repo_canonical = repo_root.canonicalize().map_err(io_error)?;
    let parent = output_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent_canonical = parent.canonicalize().map_err(io_error)?;
    if parent_canonical.starts_with(&repo_canonical) {
        return Err("archive output path cannot be inside the repository".to_string());
    }
    Ok(())
}

fn export_zip_archive(
    repo_root: &Path,
    output_path: &Path,
    options: &RepositoryArchiveExportOptions,
) -> Result<(), String> {
    if let Some(binary) = find_7z_binary() {
        run_7z_archive(&binary, "zip", repo_root, output_path, options)
    } else if options.encrypt {
        Err("zip encryption requires 7z/7zz/7za in PATH".to_string())
    } else {
        run_powershell_compress_archive(repo_root, output_path, &options.compression)
    }
}

fn export_7z_archive(
    repo_root: &Path,
    output_path: &Path,
    options: &RepositoryArchiveExportOptions,
) -> Result<(), String> {
    let binary =
        find_7z_binary().ok_or_else(|| "7z export requires 7z/7zz/7za in PATH".to_string())?;
    run_7z_archive(&binary, "7z", repo_root, output_path, options)
}

fn export_tar_archive(
    repo_root: &Path,
    output_path: &Path,
    options: &RepositoryArchiveExportOptions,
) -> Result<(), String> {
    if options.encrypt {
        return Err("tar export does not support encryption; use zip or 7z".to_string());
    }

    let mut command = Command::new("tar");
    command
        .current_dir(repo_root)
        .arg(if options.compression == "none" {
            "-cf"
        } else {
            "-czf"
        })
        .arg(output_path)
        .arg(".");

    run_command(command, "tar export")
}

fn run_7z_archive(
    binary: &str,
    archive_type: &str,
    repo_root: &Path,
    output_path: &Path,
    options: &RepositoryArchiveExportOptions,
) -> Result<(), String> {
    let mut command = Command::new(binary);
    command
        .current_dir(repo_root)
        .arg("a")
        .arg("-y")
        .arg(format!("-t{archive_type}"))
        .arg(compression_flag(&options.compression))
        .arg(output_path)
        .arg(".");

    if options.encrypt {
        let password = options
            .password
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "archive password cannot be empty".to_string())?;
        command.arg(format!("-p{password}"));
        if archive_type == "7z" {
            command.arg("-mhe=on");
        }
    }

    run_command(command, "7z export")
}

fn run_powershell_compress_archive(
    repo_root: &Path,
    output_path: &Path,
    compression: &str,
) -> Result<(), String> {
    let script = format!(
        "Add-Type -AssemblyName System.IO.Compression.FileSystem; [System.IO.Compression.ZipFile]::CreateFromDirectory('{}', '{}', [System.IO.Compression.CompressionLevel]::{}, $true)",
        escape_powershell_single_quoted_path(repo_root),
        escape_powershell_single_quoted_path(output_path),
        powershell_compression_level(compression),
    );
    let mut command = Command::new("powershell");
    command.arg("-NoProfile").arg("-Command").arg(script);
    run_command(command, "zip export")
}

fn export_repository_to_git(
    repo_root: &Path,
    options: &RepositoryGitExportOptions,
) -> Result<GitExportResult, String> {
    if !repo_root.join(".git").is_dir() {
        return Err("repository folder is not a Git repository".to_string());
    }

    run_git(repo_root, &["add", "-A"])?;

    if has_git_changes(repo_root)? {
        let message = options
            .message
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("导出资源库");
        run_git(repo_root, &["commit", "-m", message])?;
    }

    let remote = options
        .remote
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("origin")
        .to_string();
    let branch = options
        .branch
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| current_git_branch(repo_root).unwrap_or_else(|_| "HEAD".to_string()));
    if branch == "HEAD" {
        return Err("cannot infer Git branch; specify a branch before uploading".to_string());
    }

    run_git(repo_root, &["push", &remote, &branch])?;
    Ok(GitExportResult {
        remote: remote.clone(),
        branch: branch.clone(),
        message: format!("资源库已上传到 {remote}/{branch}"),
    })
}

struct GitExportResult {
    remote: String,
    branch: String,
    message: String,
}

fn has_git_changes(repo_root: &Path) -> Result<bool, String> {
    let output = Command::new("git")
        .current_dir(repo_root)
        .arg("status")
        .arg("--porcelain")
        .output()
        .map_err(|error| format!("git unavailable: {error}"))?;
    if !output.status.success() {
        return Err(command_error("git status", &output));
    }
    Ok(!String::from_utf8_lossy(&output.stdout).trim().is_empty())
}

fn current_git_branch(repo_root: &Path) -> Result<String, String> {
    let output = Command::new("git")
        .current_dir(repo_root)
        .arg("branch")
        .arg("--show-current")
        .output()
        .map_err(|error| format!("git unavailable: {error}"))?;
    if !output.status.success() {
        return Err(command_error("git branch", &output));
    }
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if branch.is_empty() {
        Ok("HEAD".to_string())
    } else {
        Ok(branch)
    }
}

fn run_git(repo_root: &Path, args: &[&str]) -> Result<(), String> {
    let mut command = Command::new("git");
    command.current_dir(repo_root).args(args);
    run_command(command, &format!("git {}", args.join(" ")))
}

fn find_7z_binary() -> Option<&'static str> {
    ["7z", "7zz", "7za"]
        .into_iter()
        .find(|binary| Command::new(binary).arg("--help").output().is_ok())
}

fn compression_flag(value: &str) -> &'static str {
    match value {
        "none" => "-mx=0",
        "fast" => "-mx=3",
        "maximum" => "-mx=9",
        _ => "-mx=5",
    }
}

fn powershell_compression_level(value: &str) -> &'static str {
    match value {
        "none" => "NoCompression",
        "fast" => "Fastest",
        _ => "Optimal",
    }
}

fn escape_powershell_single_quoted_path(path: &Path) -> String {
    path.to_string_lossy().replace('\'', "''")
}

fn run_command(mut command: Command, label: &str) -> Result<(), String> {
    let output = command
        .output()
        .map_err(|error| format!("{label} failed to start: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(command_error(label, &output))
    }
}

fn command_error(label: &str, output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let detail = if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        output.status.to_string()
    };
    format!("{label} failed: {detail}")
}

fn copy_directory_recursive_with_mode(
    source: &Path,
    source_relative_path: Option<&str>,
    target: &Path,
    target_relative_path: &str,
    hardlink_preferred: bool,
) -> Result<Vec<HardlinkCopyOutcome>, String> {
    fs::create_dir(target).map_err(io_error)?;
    let mut outcomes = Vec::new();
    for entry in fs::read_dir(source).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let name = entry.file_name().to_string_lossy().to_string();
        if is_internal_repository_dir(&name) {
            continue;
        }

        let child_source = entry.path();
        let child_target = target.join(&name);
        let child_source_relative_path =
            source_relative_path.map(|parent| join_relative_path(parent, &name));
        let child_relative_path = join_relative_path(target_relative_path, &name);
        let metadata = entry.metadata().map_err(io_error)?;
        if metadata.is_dir() {
            outcomes.extend(copy_directory_recursive_with_mode(
                &child_source,
                child_source_relative_path.as_deref(),
                &child_target,
                &child_relative_path,
                hardlink_preferred,
            )?);
        } else if metadata.is_file() {
            outcomes.push(copy_file_with_mode(
                &child_source,
                child_source_relative_path.as_deref(),
                &child_target,
                &child_relative_path,
                hardlink_preferred,
            )?);
        }
    }

    Ok(outcomes)
}

fn copy_file_with_mode(
    source: &Path,
    source_relative_path: Option<&str>,
    target: &Path,
    target_relative_path: &str,
    hardlink_preferred: bool,
) -> Result<HardlinkCopyOutcome, String> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(io_error)?;
    }
    if hardlink_preferred && fs::hard_link(source, target).is_ok() {
        return Ok(HardlinkCopyOutcome {
            source_path: source_relative_path.map(str::to_string),
            target_path: target_relative_path.to_string(),
            link_state: "linked".to_string(),
        });
    }
    fs::copy(source, target).map_err(io_error)?;
    Ok(HardlinkCopyOutcome {
        source_path: source_relative_path.map(str::to_string),
        target_path: target_relative_path.to_string(),
        link_state: "copiedFallback".to_string(),
    })
}

fn replace_file_with_hardlink(
    repo_root: &Path,
    source: &Path,
    target: &Path,
) -> Result<(), String> {
    if !source.is_file() {
        return Err("hardlink source is not a file".to_string());
    }
    if !target.is_file() {
        return Err("hardlink target is not a file".to_string());
    }
    let staging_dir = repository_meta_dir(repo_root).join("hardlink-staging");
    fs::create_dir_all(&staging_dir).map_err(io_error)?;
    let backup = staging_dir.join(format!(
        "{}.bak",
        sha256_hex(&[
            target.to_string_lossy().as_bytes(),
            now_rfc3339().as_bytes()
        ])
    ));
    fs::rename(target, &backup).map_err(io_error)?;
    match fs::hard_link(source, target) {
        Ok(()) => {
            fs::remove_file(&backup).map_err(io_error)?;
            Ok(())
        }
        Err(error) => {
            let restore_result = fs::rename(&backup, target);
            if let Err(restore_error) = restore_result {
                return Err(format!(
                    "hardlink failed: {error}; restore failed: {restore_error}"
                ));
            }
            Err(format!("hardlink failed: {error}"))
        }
    }
}

fn validate_new_entry_name(name: &str) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("name cannot be empty".to_string());
    }
    if trimmed.contains(['/', '\\']) {
        return Err("name cannot contain path separators".to_string());
    }
    if trimmed == "." || trimmed == ".." {
        return Err("invalid entry name".to_string());
    }
    if is_internal_repository_dir(trimmed) {
        return Err("internal repository directory is reserved".to_string());
    }
    Ok(trimmed.to_string())
}

fn parent_relative_path(path: &str) -> String {
    Path::new(path)
        .parent()
        .map(|parent| parent.to_string_lossy().replace('\\', "/"))
        .filter(|value| value != ".")
        .unwrap_or_default()
}

fn join_relative_path(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_string()
    } else {
        format!("{parent}/{name}")
    }
}

fn backend_adapter<'a>(
    service_root: &'a Path,
    repo: &'a RepositoryRecord,
) -> Box<dyn FileSystemBackendAdapter + 'a> {
    Box::new(RuntimeFileSystemBackendAdapter {
        service_root: service_root.to_path_buf(),
        plugin_id: repo.backend_record.plugin_id.clone(),
    })
}

fn call_builtin_local_filesystem(
    method: &str,
    payload: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let backend = LocalFileSystemBackend;
    let repo_root = PathBuf::from(
        payload
            .get("repoRoot")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| format!("plugin call is missing repoRoot for {method}"))?,
    );
    let config = payload
        .get("config")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    match method {
        "filesystem.ensureAttachable" => {
            backend.ensure_attachable(&repo_root, &config)?;
            Ok(serde_json::json!({}))
        }
        "filesystem.prepareRepositoryRoot" => {
            backend.prepare_repository_root(&repo_root, &config)?;
            Ok(serde_json::json!({}))
        }
        "filesystem.listFiles" => {
            let files = backend.list_files(&repo_root, &config)?;
            serde_json::to_value(files).map_err(json_error)
        }
        "filesystem.listTree" => {
            let tree = backend.list_tree(&repo_root, &config)?;
            serde_json::to_value(tree).map_err(json_error)
        }
        "filesystem.listDirectory" => {
            let directory_path = payload
                .get("directoryPath")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            let entries = backend.list_directory_entries(&repo_root, directory_path, &config)?;
            serde_json::to_value(entries).map_err(json_error)
        }
        "filesystem.createDirectory" => {
            let parent_path = payload
                .get("parentPath")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            let name = payload
                .get("name")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| "plugin call is missing name".to_string())?;
            backend.create_directory(&repo_root, parent_path, name, &config)?;
            Ok(serde_json::json!({}))
        }
        "filesystem.createFile" => {
            let parent_path = payload
                .get("parentPath")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            let name = payload
                .get("name")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| "plugin call is missing name".to_string())?;
            backend.create_file(&repo_root, parent_path, name, &config)?;
            Ok(serde_json::json!({}))
        }
        "filesystem.statEntry" => {
            let entry_path = payload
                .get("entryPath")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| "plugin call is missing entryPath".to_string())?;
            let entry = backend.stat_entry(&repo_root, entry_path, &config)?;
            serde_json::to_value(entry).map_err(json_error)
        }
        "filesystem.renameEntry" => {
            let source_path = payload
                .get("sourcePath")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| "plugin call is missing sourcePath".to_string())?;
            let new_name = payload
                .get("newName")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| "plugin call is missing newName".to_string())?;
            let entry = backend.rename_entry(&repo_root, source_path, new_name, &config)?;
            serde_json::to_value(entry).map_err(json_error)
        }
        "filesystem.moveEntry" => {
            let source_path = payload
                .get("sourcePath")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| "plugin call is missing sourcePath".to_string())?;
            let target_parent_path = payload
                .get("targetParentPath")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| "plugin call is missing targetParentPath".to_string())?;
            let entry = backend.move_entry(&repo_root, source_path, target_parent_path, &config)?;
            serde_json::to_value(entry).map_err(json_error)
        }
        "filesystem.deleteEntry" => {
            let entry_path = payload
                .get("entryPath")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| "plugin call is missing entryPath".to_string())?;
            let recursive = payload
                .get("recursive")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            backend.delete_entry(&repo_root, entry_path, recursive, &config)?;
            Ok(serde_json::json!({}))
        }
        _ => Err(format!("unsupported filesystem plugin method: {method}")),
    }
}

fn list_backend_files(
    service_root: &Path,
    repo: &RepositoryRecord,
    repo_root: &Path,
) -> Result<Vec<DiscoveredFile>, String> {
    backend_adapter(service_root, repo).list_files(repo_root, &repo.backend_record.config)
}

fn list_backend_tree(
    service_root: &Path,
    repo: &RepositoryRecord,
    repo_root: &Path,
) -> Result<Vec<FileTreeNode>, String> {
    backend_adapter(service_root, repo).list_tree(repo_root, &repo.backend_record.config)
}

fn list_backend_directory_entries(
    service_root: &Path,
    repo: &RepositoryRecord,
    repo_root: &Path,
    current_path: &str,
    asset_map: &BTreeMap<String, AssetPathRecord>,
    thumbnail_map: &BTreeMap<(String, String), ThumbnailRecord>,
    folder_metadata: &BTreeMap<String, FolderMetadata>,
) -> Result<Vec<FileBrowserEntry>, String> {
    let entries = backend_adapter(service_root, repo).list_directory_entries(
        repo_root,
        current_path,
        &repo.backend_record.config,
    )?;
    Ok(map_file_browser_entries(
        entries,
        asset_map,
        thumbnail_map,
        folder_metadata,
    ))
}

fn list_trash_directory_entries(
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

fn create_backend_directory(
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

fn create_backend_file(
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
fn emit_entry_playback_progress(
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

fn stat_backend_entry(
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

fn rename_backend_entry(
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

fn move_backend_entry(
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

fn delete_backend_entry(
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

fn move_entry_to_trash(
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

fn delete_trash_entry(repo_root: &Path, trash_path: &str) -> Result<(), String> {
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

fn restore_trash_entry(repo_root: &Path, trash_path: &str) -> Result<(), String> {
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

fn restore_all_trash_entries(repo_root: &Path) -> Result<(), String> {
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

fn empty_trash(repo_root: &Path) -> Result<(), String> {
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

fn clean_empty_trash_directories(trash_root: &Path) -> Result<(), String> {
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

fn collect_trash_directories(
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

fn build_directory_tree(repo_root: &Path) -> Result<Vec<FileTreeNode>, String> {
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

fn build_directory_node(
    repo_root: &Path,
    relative_path: &str,
) -> Result<Option<FileTreeNode>, String> {
    let abs_path = resolve_repository_relative_path(repo_root, relative_path)?;
    let mut children = Vec::new();

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
        children,
    }))
}

fn map_file_browser_entries(
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

fn map_trash_browser_entries(
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

fn attach_browser_entry_metadata(
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

    for entry in &mut entries {
        let Some(asset_id) = &entry.asset_id else {
            continue;
        };
        let Some(metadata) = metadata_by_asset.get(asset_id) else {
            continue;
        };
        let mut merged = metadata.clone();
        merged.extend(entry.metadata.clone());
        normalize_loaded_metadata(&mut merged);
        entry.metadata = merged;
        entry.tags = load_tags(connection, asset_id)?;
        entry.alias_paths = alias_paths_by_asset
            .get(asset_id)
            .cloned()
            .unwrap_or_default();
    }

    Ok(entries)
}

fn local_directory_entries(
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
        backend_plugin_registry(&self.service_root).call(
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
        backend_plugin_registry(&self.service_root).call(
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
        let response = backend_plugin_registry(&self.service_root).call(
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
        let response = backend_plugin_registry(&self.service_root).call(
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
        let response = backend_plugin_registry(&self.service_root).call(
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

    fn create_directory(
        &self,
        repo_root: &Path,
        parent_path: &str,
        name: &str,
        config: &serde_json::Value,
    ) -> Result<(), String> {
        backend_plugin_registry(&self.service_root).call(
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
        backend_plugin_registry(&self.service_root).call(
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
        let response = backend_plugin_registry(&self.service_root).call(
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
        let response = backend_plugin_registry(&self.service_root).call(
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
        let response = backend_plugin_registry(&self.service_root).call(
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
        backend_plugin_registry(&self.service_root).call(
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
}

impl FileSystemBackendAdapter for LocalFileSystemBackend {
    fn ensure_attachable(
        &self,
        repo_root: &Path,
        _config: &serde_json::Value,
    ) -> Result<(), String> {
        if !repo_root.exists() {
            return Err(format!(
                "repository folder does not exist: {}",
                repo_root.to_string_lossy()
            ));
        }
        if !repo_root.is_dir() {
            return Err(format!(
                "repository path is not a folder: {}",
                repo_root.to_string_lossy()
            ));
        }
        Ok(())
    }

    fn prepare_repository_root(
        &self,
        repo_root: &Path,
        _config: &serde_json::Value,
    ) -> Result<(), String> {
        fs::create_dir_all(repo_root).map_err(io_error)
    }

    fn list_files(
        &self,
        repo_root: &Path,
        _config: &serde_json::Value,
    ) -> Result<Vec<DiscoveredFile>, String> {
        collect_repository_files(repo_root).map_err(io_error)
    }

    fn list_tree(
        &self,
        repo_root: &Path,
        _config: &serde_json::Value,
    ) -> Result<Vec<FileTreeNode>, String> {
        build_directory_tree(repo_root)
    }

    fn list_directory_entries(
        &self,
        repo_root: &Path,
        directory_path: &str,
        _config: &serde_json::Value,
    ) -> Result<Vec<FileSystemEntry>, String> {
        let current_dir = resolve_repository_relative_path(repo_root, directory_path)?;
        if !current_dir.exists() || !current_dir.is_dir() {
            return Err(format!("directory not found: {directory_path}"));
        }
        local_directory_entries(repo_root, &current_dir)
    }

    fn create_directory(
        &self,
        repo_root: &Path,
        parent_path: &str,
        name: &str,
        _config: &serde_json::Value,
    ) -> Result<(), String> {
        let parent_dir = resolve_repository_relative_path(repo_root, parent_path)?;
        let target_dir = parent_dir.join(name);
        if target_dir.exists() {
            return Err(format!("entry already exists: {name}"));
        }
        fs::create_dir(&target_dir).map_err(io_error)
    }

    fn create_file(
        &self,
        repo_root: &Path,
        parent_path: &str,
        name: &str,
        _config: &serde_json::Value,
    ) -> Result<(), String> {
        let parent_dir = resolve_repository_relative_path(repo_root, parent_path)?;
        let target_file = parent_dir.join(name);
        if target_file.exists() {
            return Err(format!("entry already exists: {name}"));
        }
        OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(target_file)
            .map_err(io_error)?;
        Ok(())
    }

    fn stat_entry(
        &self,
        repo_root: &Path,
        entry_path: &str,
        _config: &serde_json::Value,
    ) -> Result<FileSystemEntry, String> {
        let entry_abs = resolve_repository_relative_path(repo_root, entry_path)?;
        if !entry_abs.exists() {
            return Err(format!("entry not found: {entry_path}"));
        }
        let metadata = fs::metadata(&entry_abs).map_err(io_error)?;
        Ok(FileSystemEntry {
            path: entry_path.to_string(),
            name: entry_abs
                .file_name()
                .map(|value| value.to_string_lossy().to_string())
                .unwrap_or_else(|| entry_path.to_string()),
            kind: if metadata.is_dir() {
                FileSystemEntryKind::Directory
            } else {
                FileSystemEntryKind::File
            },
            extension: if metadata.is_file() {
                entry_abs
                    .extension()
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
            local_absolute_path: Some(entry_abs.to_string_lossy().to_string()),
        })
    }

    fn rename_entry(
        &self,
        repo_root: &Path,
        source_path: &str,
        new_name: &str,
        config: &serde_json::Value,
    ) -> Result<FileSystemEntry, String> {
        let source_abs = resolve_repository_relative_path(repo_root, source_path)?;
        if !source_abs.exists() {
            return Err(format!("entry not found: {source_path}"));
        }
        let target_abs = source_abs
            .parent()
            .ok_or_else(|| "cannot rename repository root".to_string())?
            .join(new_name);
        if target_abs.exists() {
            return Err(format!("entry already exists: {new_name}"));
        }
        fs::rename(&source_abs, &target_abs).map_err(io_error)?;
        let parent_path = parent_relative_path(source_path);
        let target_path = join_relative_path(&parent_path, new_name);
        self.stat_entry(repo_root, &target_path, config)
    }

    fn move_entry(
        &self,
        repo_root: &Path,
        source_path: &str,
        target_parent_path: &str,
        config: &serde_json::Value,
    ) -> Result<FileSystemEntry, String> {
        let source_abs = resolve_repository_relative_path(repo_root, source_path)?;
        if !source_abs.exists() {
            return Err(format!("entry not found: {source_path}"));
        }
        let name = source_abs
            .file_name()
            .map(|value| value.to_string_lossy().to_string())
            .ok_or_else(|| format!("invalid source path: {source_path}"))?;
        let target_parent_abs = resolve_repository_relative_path(repo_root, target_parent_path)?;
        let target_abs = target_parent_abs.join(&name);
        if target_abs.exists() {
            return Err(format!("entry already exists: {name}"));
        }
        fs::rename(&source_abs, &target_abs).map_err(io_error)?;
        let target_path = join_relative_path(target_parent_path, &name);
        self.stat_entry(repo_root, &target_path, config)
    }

    fn delete_entry(
        &self,
        repo_root: &Path,
        entry_path: &str,
        recursive: bool,
        _config: &serde_json::Value,
    ) -> Result<(), String> {
        let entry_abs = resolve_repository_relative_path(repo_root, entry_path)?;
        if !entry_abs.exists() {
            return Err(format!("entry not found: {entry_path}"));
        }
        if recursive {
            fs::remove_dir_all(entry_abs).map_err(io_error)
        } else {
            fs::remove_file(entry_abs).map_err(io_error)
        }
    }
}

fn rename_file_asset_record(
    tx: &Transaction<'_>,
    repo_id: &str,
    source_path: &str,
    target_path: &str,
    new_name: &str,
    new_extension: &str,
    modified_at: &str,
) -> Result<(), rusqlite::Error> {
    let updated = tx.execute(
        r#"
        UPDATE assets
        SET path = ?3, filename = ?4, extension = ?5, modified_at = ?6, updated_at = ?6
        WHERE repo_id = ?1 AND path = ?2
        "#,
        params![
            repo_id,
            source_path,
            target_path,
            new_name,
            new_extension,
            modified_at
        ],
    )?;

    if updated == 0 {
        return Ok(());
    }

    tx.execute(
        r#"
        INSERT OR REPLACE INTO events (event_id, repo_id, asset_id, event_type, path, payload_json, created_at)
        SELECT
          ?4,
          repo_id,
          asset_id,
          'asset.renamed',
          ?2,
          ?3,
          ?5
        FROM assets
        WHERE repo_id = ?1 AND path = ?2
        "#,
        params![
            repo_id,
            target_path,
            serde_json::json!({ "sourcePath": source_path }).to_string(),
            format!("evt-asset-renamed-{}", slugify_repo_id(repo_id, target_path)),
            now_rfc3339()
        ],
    )?;
    tx.execute(
        r#"
        UPDATE hardlink_members
        SET path = ?3, verified_at = ?4
        WHERE repo_id = ?1
          AND asset_id = (
            SELECT asset_id FROM assets WHERE repo_id = ?1 AND path = ?2 LIMIT 1
          )
        "#,
        params![repo_id, target_path, target_path, now_rfc3339()],
    )?;

    Ok(())
}

fn rename_directory_asset_records(
    tx: &Transaction<'_>,
    repo_id: &str,
    source_path: &str,
    target_path: &str,
) -> Result<(), rusqlite::Error> {
    let mut stmt = tx.prepare(
        r#"
        SELECT asset_id, path
        FROM assets
        WHERE repo_id = ?1 AND (path = ?2 OR path LIKE ?3)
        "#,
    )?;
    let prefix = format!("{source_path}/%");
    let rows = stmt.query_map(params![repo_id, source_path, prefix], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let assets = rows.collect::<Result<Vec<_>, _>>()?;
    let now = now_rfc3339();

    for (asset_id, old_path) in assets {
        let suffix = old_path.strip_prefix(source_path).unwrap_or("");
        let new_path = format!("{target_path}{suffix}");
        let filename = Path::new(&new_path)
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| asset_id.clone());
        let extension = Path::new(&new_path)
            .extension()
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_default();

        tx.execute(
            r#"
            UPDATE assets
            SET path = ?3, filename = ?4, extension = ?5, updated_at = ?6, modified_at = ?6
            WHERE repo_id = ?1 AND asset_id = ?2
            "#,
            params![repo_id, asset_id, new_path, filename, extension, now],
        )?;
        tx.execute(
            r#"
            UPDATE hardlink_members
            SET path = ?3, verified_at = ?4
            WHERE repo_id = ?1 AND asset_id = ?2
            "#,
            params![repo_id, asset_id, new_path, now],
        )?;
    }

    Ok(())
}

fn move_directory_contents_to_parent(
    service_root: &Path,
    repo: &RepositoryRecord,
    repo_root: &Path,
    repo_id: &str,
    source_path: &str,
) -> Result<(), String> {
    let source_abs = resolve_repository_relative_path(repo_root, source_path)?;
    if !source_abs.exists() || !source_abs.is_dir() {
        return Err(format!("directory not found: {source_path}"));
    }

    let target_parent_path = parent_relative_path(source_path);
    let target_parent_abs = if target_parent_path.is_empty() {
        repo_root.to_path_buf()
    } else {
        resolve_repository_relative_path(repo_root, &target_parent_path)?
    };

    let mut children = Vec::new();
    for entry in fs::read_dir(&source_abs).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let child_name = entry.file_name().to_string_lossy().to_string();
        if is_internal_repository_dir(&child_name) {
            continue;
        }

        let child_source_path = join_relative_path(source_path, &child_name);
        let child_target_path = join_relative_path(&target_parent_path, &child_name);
        let child_target_abs = target_parent_abs.join(&child_name);

        if child_target_abs.exists() {
            return Err(format!("target already exists: {child_target_path}"));
        }

        children.push((entry.path(), child_source_path, child_target_path));
    }

    if children.is_empty() {
        delete_backend_entry(service_root, repo, repo_root, source_path, true)?;
        return Ok(());
    }

    for (child_abs, _, child_target_path) in &children {
        let child_name = Path::new(child_target_path)
            .file_name()
            .map(|value| value.to_string_lossy().to_string())
            .ok_or_else(|| format!("invalid target path: {child_target_path}"))?;
        fs::rename(child_abs, target_parent_abs.join(child_name)).map_err(io_error)?;
    }
    fs::remove_dir(&source_abs).map_err(io_error)?;

    let storage_paths = ensure_repository_storage_paths(
        service_root,
        repo_id,
        repo_root,
        &repo.backend_record.plugin_id,
    )?;
    let mut connection = Connection::open(storage_paths.database_path).map_err(db_error)?;
    connection
        .pragma_update(None, "journal_mode", "WAL")
        .map_err(db_error)?;
    let tx = connection.transaction().map_err(db_error)?;
    for (_, child_source_path, child_target_path) in &children {
        rename_directory_move_asset_records(&tx, repo_id, child_source_path, child_target_path)
            .map_err(db_error)?;
    }
    tx.commit().map_err(db_error)?;

    Ok(())
}

fn rename_directory_move_asset_records(
    tx: &Transaction<'_>,
    repo_id: &str,
    source_path: &str,
    target_path: &str,
) -> Result<(), rusqlite::Error> {
    let existing = tx
        .query_row(
            r#"
            SELECT 1
            FROM assets
            WHERE repo_id = ?1 AND path = ?2
            LIMIT 1
            "#,
            params![repo_id, source_path],
            |_| Ok(()),
        )
        .optional()?;

    if existing.is_some() {
        let filename = Path::new(target_path)
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| target_path.to_string());
        let extension = Path::new(target_path)
            .extension()
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_default();
        let now = now_rfc3339();

        tx.execute(
            r#"
            UPDATE assets
            SET path = ?3, filename = ?4, extension = ?5, updated_at = ?6, modified_at = ?6
            WHERE repo_id = ?1 AND path = ?2
            "#,
            params![repo_id, source_path, target_path, filename, extension, now],
        )?;
        tx.execute(
            r#"
            UPDATE hardlink_members
            SET path = ?3, verified_at = ?4
            WHERE repo_id = ?1 AND asset_id = (
              SELECT asset_id FROM assets WHERE repo_id = ?1 AND path = ?3 LIMIT 1
            )
            "#,
            params![repo_id, source_path, target_path, now],
        )?;
        return Ok(());
    }

    rename_directory_asset_records(tx, repo_id, source_path, target_path)
}

fn mark_file_asset_deleted(
    tx: &Transaction<'_>,
    repo_id: &str,
    path: &str,
) -> Result<(), rusqlite::Error> {
    let asset_id = tx
        .query_row(
            "SELECT asset_id FROM assets WHERE repo_id = ?1 AND path = ?2",
            params![repo_id, path],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    tx.execute(
        r#"
        UPDATE assets
        SET status = 'deleted', updated_at = ?3
        WHERE repo_id = ?1 AND path = ?2
        "#,
        params![repo_id, path, now_rfc3339()],
    )?;
    if let Some(asset_id) = asset_id {
        mark_hardlink_member_missing(tx, repo_id, &asset_id)?;
    }
    Ok(())
}

fn mark_directory_assets_deleted(
    tx: &Transaction<'_>,
    repo_id: &str,
    path: &str,
) -> Result<(), rusqlite::Error> {
    let prefix = format!("{path}/%");
    let mut stmt = tx.prepare(
        r#"
        SELECT asset_id
        FROM assets
        WHERE repo_id = ?1 AND (path = ?2 OR path LIKE ?3)
        "#,
    )?;
    let asset_rows = stmt.query_map(params![repo_id, path, prefix.as_str()], |row| {
        row.get::<_, String>(0)
    })?;
    let asset_ids = asset_rows.collect::<Result<Vec<_>, _>>()?;
    tx.execute(
        r#"
        UPDATE assets
        SET status = 'deleted', updated_at = ?4
        WHERE repo_id = ?1 AND (path = ?2 OR path LIKE ?3)
        "#,
        params![repo_id, path, prefix, now_rfc3339()],
    )?;
    for asset_id in asset_ids {
        mark_hardlink_member_missing(tx, repo_id, &asset_id)?;
    }
    Ok(())
}

fn system_time_to_rfc3339(value: SystemTime) -> Result<String, time::error::Format> {
    let datetime: OffsetDateTime = value.into();
    datetime.format(&Rfc3339)
}

fn format_size_label(size_bytes: i64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let size = size_bytes as f64;

    if size >= GB {
        format!("{:.1} GB", size / GB)
    } else if size >= MB {
        format!("{:.1} MB", size / MB)
    } else if size >= KB {
        format!("{:.1} KB", size / KB)
    } else {
        format!("{size_bytes} B")
    }
}

fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

fn db_error(error: rusqlite::Error) -> String {
    format!("database error: {error}")
}

fn io_error(error: std::io::Error) -> String {
    format!("io error: {error}")
}

fn is_skippable_filesystem_error(error: &std::io::Error) -> bool {
    matches!(error.kind(), std::io::ErrorKind::PermissionDenied)
}

fn path_error(error: std::path::StripPrefixError) -> String {
    format!("path error: {error}")
}

fn json_error(error: serde_json::Error) -> String {
    format!("json error: {error}")
}

fn time_error(error: time::error::Format) -> String {
    format!("time error: {error}")
}

fn safe_prefix(value: &str, max_chars: usize) -> &str {
    let end = value
        .char_indices()
        .nth(max_chars)
        .map(|(index, _)| index)
        .unwrap_or(value.len());
    &value[..end]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::sync::MutexGuard;
    use std::time::{SystemTime, UNIX_EPOCH};

    const TEST_WEBDAV_PLUGIN_ID: &str = "momobako.webdav";
    static PLAYBACK_TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    struct TestWorkspace {
        root: PathBuf,
    }

    impl TestWorkspace {
        fn new(name: &str) -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock must be after unix epoch")
                .as_nanos();
            let root = std::env::temp_dir()
                .join(format!("momobako-{name}-{}-{unique}", std::process::id()));
            Self { root }
        }

        fn path(&self, child: &str) -> PathBuf {
            self.root.join(child)
        }
    }

    impl Drop for TestWorkspace {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn playback_test_lock() -> MutexGuard<'static, ()> {
        PLAYBACK_TEST_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .expect("playback test lock should succeed")
    }

    #[test]
    fn create_local_repository_creates_metadata_storage_dirs() {
        let workspace = TestWorkspace::new("local-repository-create");
        let service_root = workspace.path("service");
        let repo_root = workspace.path("repo");
        install_local_filesystem_test_plugin_archive(&service_root);
        let state = RepositoryState::from_root(service_root);

        state
            .create_repository(RepositoryMutationRequest {
                repo_id: Some("repo-test".to_string()),
                name: "测试资源库".to_string(),
                path: repo_root.to_string_lossy().to_string(),
                backend_plugin_id: Some(LOCAL_FILESYSTEM_PLUGIN_ID.to_string()),
                backend_config: None,
                skip_initial_sync: false,
            })
            .expect("local repository should be created");

        let metadata_dir = repo_root.join(REPO_META_DIR);
        assert!(metadata_dir.is_dir());
        assert!(metadata_dir.join(REPO_METADATA_FILE_NAME).is_file());
        assert!(metadata_dir.join(REPO_DB_FILE_NAME).is_file());
        for subdir in ["cache", "thumbnails", "logs", "indexes"] {
            assert!(metadata_dir.join(subdir).is_dir());
        }
    }

    #[test]
    fn create_repository_can_skip_initial_sync() {
        let workspace = TestWorkspace::new("repository-create-skip-sync");
        let service_root = workspace.path("service");
        let repo_root = workspace.path("repo");
        install_local_filesystem_test_plugin_archive(&service_root);
        fs::create_dir_all(&repo_root).expect("repository root should exist");
        fs::write(repo_root.join("track.mp3"), b"demo").expect("test file should be written");
        let state = RepositoryState::from_root(service_root);

        let repo_id = state
            .create_repository(RepositoryMutationRequest {
                repo_id: Some("repo-skip-sync".to_string()),
                name: "Skip Sync Repo".to_string(),
                path: repo_root.to_string_lossy().to_string(),
                backend_plugin_id: Some(LOCAL_FILESYSTEM_PLUGIN_ID.to_string()),
                backend_config: None,
                skip_initial_sync: true,
            })
            .expect("repository should be created without inline sync")
            .repository
            .repo_id;

        let snapshot_before_sync = state
            .load_snapshot(&repo_id)
            .expect("snapshot should load before sync");
        assert!(snapshot_before_sync.assets.is_empty());

        state
            .sync_repository(SyncRequest {
                repo_id: repo_id.clone(),
            })
            .expect("repository should sync when triggered later");

        let snapshot_after_sync = state
            .load_snapshot(&repo_id)
            .expect("snapshot should load after sync");
        assert_eq!(snapshot_after_sync.assets.len(), 1);
        assert_eq!(snapshot_after_sync.assets[0].path, "track.mp3");
    }

    #[test]
    fn find_existing_repository_for_backend_matches_netease_account_id() {
        let workspace = TestWorkspace::new("netease-repository-dedupe");
        let service_root = workspace.path("service");
        install_local_filesystem_test_plugin_archive(&service_root);
        let state = RepositoryState::from_root(service_root.clone());
        state
            .ensure_initialized()
            .expect("state should initialize registry");
        let registry =
            Connection::open(service_root.join(REGISTRY_FILE_NAME)).expect("registry should open");
        let backend = RepositoryBackendRecord {
            plugin_id: NETEASE_CLOUD_MUSIC_PLUGIN_ID.to_string(),
            config: serde_json::json!({
                "accountId": "123",
                "cookie": "MUSIC_U=test"
            }),
        };
        let seed = RepositorySeed {
            repo_id: "netease-one",
            name: "网易云 A",
            root_path: "",
            status: "ready",
            assets: &[],
        };
        upsert_registry_entry(
            &registry,
            Path::new("netease-cloud-music://account/123"),
            &seed,
            &backend,
        )
        .expect("registry entry should be stored");

        let existing = state
            .find_existing_repository_for_backend(&RepositoryBackendRecord {
                plugin_id: NETEASE_CLOUD_MUSIC_PLUGIN_ID.to_string(),
                config: serde_json::json!({
                    "accountId": "123",
                    "cookie": "MUSIC_U=other"
                }),
            })
            .expect("lookup should succeed")
            .expect("existing repository should be found");

        assert_eq!(existing.repo_id, "netease-one");
    }

    #[test]
    fn find_existing_repository_for_backend_matches_numeric_netease_account_id() {
        let workspace = TestWorkspace::new("netease-repository-dedupe-numeric");
        let service_root = workspace.path("service");
        install_local_filesystem_test_plugin_archive(&service_root);
        let state = RepositoryState::from_root(service_root.clone());
        state
            .ensure_initialized()
            .expect("state should initialize registry");
        let registry =
            Connection::open(service_root.join(REGISTRY_FILE_NAME)).expect("registry should open");
        let backend = RepositoryBackendRecord {
            plugin_id: NETEASE_CLOUD_MUSIC_PLUGIN_ID.to_string(),
            config: serde_json::json!({
                "accountId": 123,
                "cookie": "MUSIC_U=test"
            }),
        };
        let seed = RepositorySeed {
            repo_id: "netease-one",
            name: "网易云 A",
            root_path: "",
            status: "ready",
            assets: &[],
        };
        upsert_registry_entry(
            &registry,
            Path::new("netease-cloud-music://account/123"),
            &seed,
            &backend,
        )
        .expect("registry entry should be stored");

        let existing = state
            .find_existing_repository_for_backend(&RepositoryBackendRecord {
                plugin_id: NETEASE_CLOUD_MUSIC_PLUGIN_ID.to_string(),
                config: serde_json::json!({
                    "accountId": "123",
                    "cookie": "MUSIC_U=other"
                }),
            })
            .expect("lookup should succeed")
            .expect("existing repository should be found");

        assert_eq!(existing.repo_id, "netease-one");
    }

    #[test]
    fn list_repositories_reports_netease_local_cache_statuses() {
        let workspace = TestWorkspace::new("netease-cache-status");
        let service_root = workspace.path("service");
        let ready_cache = workspace.path("ready-cache");
        let missing_cache = workspace.path("missing-cache");
        fs::create_dir_all(&ready_cache).expect("ready cache should be created");
        install_local_filesystem_test_plugin_archive(&service_root);
        let state = RepositoryState::from_root(service_root.clone());
        state
            .ensure_initialized()
            .expect("state should initialize registry");
        let registry =
            Connection::open(service_root.join(REGISTRY_FILE_NAME)).expect("registry should open");
        let backend = RepositoryBackendRecord {
            plugin_id: NETEASE_CLOUD_MUSIC_PLUGIN_ID.to_string(),
            config: serde_json::json!({
                "accountId": "123",
                "cookie": "MUSIC_U=test"
            }),
        };
        for (repo_id, name, path) in [
            (
                "netease-ready",
                "网易云 Ready",
                ready_cache.to_string_lossy().to_string(),
            ),
            (
                "netease-missing",
                "网易云 Missing",
                missing_cache.to_string_lossy().to_string(),
            ),
            (
                "netease-unconfigured",
                "网易云 Legacy",
                "netease-cloud-music://account/123".to_string(),
            ),
        ] {
            let seed = RepositorySeed {
                repo_id,
                name,
                root_path: "",
                status: "ready",
                assets: &[],
            };
            upsert_registry_entry(&registry, Path::new(&path), &seed, &backend)
                .expect("registry entry should be stored");
        }

        let repositories = state.list_repositories().expect("repositories should list");
        let ready = repositories
            .iter()
            .find(|repo| repo.repo_id == "netease-ready")
            .expect("ready repo should exist");
        assert_eq!(ready.status, "ready");
        assert_eq!(
            ready
                .local_cache
                .as_ref()
                .map(|cache| cache.status.as_str()),
            Some("ready")
        );
        let missing = repositories
            .iter()
            .find(|repo| repo.repo_id == "netease-missing")
            .expect("missing repo should exist");
        assert_eq!(missing.status, "missing");
        assert_eq!(
            missing
                .local_cache
                .as_ref()
                .map(|cache| cache.status.as_str()),
            Some("missing")
        );
        let unconfigured = repositories
            .iter()
            .find(|repo| repo.repo_id == "netease-unconfigured")
            .expect("legacy repo should exist");
        assert_eq!(unconfigured.status, "missing");
        assert_eq!(
            unconfigured
                .local_cache
                .as_ref()
                .map(|cache| cache.status.as_str()),
            Some("unconfigured")
        );
        assert_eq!(
            unconfigured
                .local_cache
                .as_ref()
                .and_then(|cache| cache.path.as_deref()),
            None
        );
    }

    #[test]
    fn configure_netease_repository_cache_updates_registry_metadata_and_moves_state() {
        let workspace = TestWorkspace::new("netease-cache-configure");
        let service_root = workspace.path("service");
        let cache_root = workspace.path("netease-cache");
        install_local_filesystem_test_plugin_archive(&service_root);
        let state = RepositoryState::from_root(service_root.clone());
        state
            .ensure_initialized()
            .expect("state should initialize registry");
        let registry =
            Connection::open(service_root.join(REGISTRY_FILE_NAME)).expect("registry should open");
        let backend = RepositoryBackendRecord {
            plugin_id: NETEASE_CLOUD_MUSIC_PLUGIN_ID.to_string(),
            config: serde_json::json!({
                "accountId": "123",
                "cookie": "MUSIC_U=test-cookie"
            }),
        };
        let seed = RepositorySeed {
            repo_id: "netease-one",
            name: "网易云 A",
            root_path: "",
            status: "ready",
            assets: &[],
        };
        upsert_registry_entry(
            &registry,
            Path::new("netease-cloud-music://account/123"),
            &seed,
            &backend,
        )
        .expect("registry entry should be stored");
        let old_meta_dir =
            repository_state_storage_dir(&service_root, "netease-one").join(REPO_META_DIR);
        fs::create_dir_all(old_meta_dir.join("indexes")).expect("old index dir should be created");
        fs::write(old_meta_dir.join("indexes").join("legacy.json"), "{}")
            .expect("old index should be written");

        let response = state
            .configure_netease_repository_cache(NeteaseRepositoryCacheConfigureRequest {
                repo_id: "netease-one".to_string(),
                path: cache_root.to_string_lossy().to_string(),
                migrate_legacy_cache: true,
            })
            .expect("cache should configure");

        assert_eq!(response.repository.path, cache_root.to_string_lossy());
        assert_eq!(response.repository.status, "ready");
        assert_eq!(
            response
                .repository
                .local_cache
                .as_ref()
                .map(|cache| cache.status.as_str()),
            Some("ready")
        );
        assert!(response.migration.moved_state_files >= 1);
        let metadata_path = cache_root.join(REPO_META_DIR).join(REPO_METADATA_FILE_NAME);
        let metadata_raw = fs::read_to_string(metadata_path).expect("metadata should be written");
        let metadata: RepositoryMetadataFileImport =
            serde_json::from_str(&metadata_raw).expect("metadata should parse");
        assert_eq!(metadata.repo_id, "netease-one");
        assert_eq!(
            metadata
                .backend_config
                .as_ref()
                .and_then(|config| config.get("sourceUri"))
                .and_then(serde_json::Value::as_str),
            Some("netease-cloud-music://account/123")
        );
        assert_eq!(
            metadata
                .backend_config
                .as_ref()
                .and_then(|config| config.get("localCachePath"))
                .and_then(serde_json::Value::as_str),
            Some(cache_root.to_string_lossy().as_ref())
        );
        assert!(cache_root
            .join(REPO_META_DIR)
            .join("indexes")
            .join("legacy.json")
            .is_file());
    }

    #[test]
    fn add_playlist_items_by_paths_expands_directories_and_deduplicates_files() {
        let (state, root, repo_root, _thumbnail_root) =
            create_test_state("playlist-items-by-paths");
        fs::create_dir_all(repo_root.join("Albums/Disc 1"))
            .expect("album directory should be created");
        fs::create_dir_all(repo_root.join("Singles")).expect("singles directory should be created");
        fs::write(repo_root.join("Albums/Disc 1/track-01.mp3"), b"track one")
            .expect("first track should be written");
        fs::write(repo_root.join("Albums/Disc 1/track-02.mp3"), b"track two")
            .expect("second track should be written");
        fs::write(repo_root.join("Singles/track-03.mp3"), b"track three")
            .expect("third track should be written");
        let repo_id = create_repository_without_initial_sync(&state, &repo_root);
        state
            .sync_repository(SyncRequest {
                repo_id: repo_id.clone(),
            })
            .expect("repository should sync");

        let repo = state
            .load_repository_record(&repo_id)
            .expect("repository record should load");
        let connection = state
            .open_repository_connection(
                &repo.summary.repo_id,
                &repo.summary.path,
                &repo.backend_record,
            )
            .expect("repository connection should open");
        let playlist_id = "playlist-by-paths";
        let now = now_rfc3339();
        connection
            .execute(
                r#"
                INSERT INTO playlists (
                  playlist_id, repo_id, name, player_type_id, player_plugin_id,
                  player_label, file_class, sort_order, created_at, updated_at
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8, ?8)
                "#,
                params![
                    playlist_id,
                    repo_id,
                    "路径展开测试歌单",
                    "momobako.playlist.audio-sequence",
                    "momobako.preview.media",
                    "音频顺序播放",
                    "audio",
                    now,
                ],
            )
            .expect("playlist should be inserted");

        {
            let detail = state
                .add_playlist_items_by_paths(PlaylistItemsByPathsAddRequest {
                    repo_id: repo_id.clone(),
                    playlist_id: playlist_id.to_string(),
                    paths: vec![
                        "Albums".to_string(),
                        "Albums/Disc 1/track-01.mp3".to_string(),
                        "Singles/track-03.mp3".to_string(),
                    ],
                })
                .expect("playlist items should be added by paths");

            let mut actual_paths = detail
                .items
                .iter()
                .map(|item| item.path.clone())
                .collect::<Vec<_>>();
            actual_paths.sort();
            assert_eq!(
                actual_paths,
                vec![
                    "Albums/Disc 1/track-01.mp3".to_string(),
                    "Albums/Disc 1/track-02.mp3".to_string(),
                    "Singles/track-03.mp3".to_string(),
                ]
            );
        }
        drop(connection);

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn add_playlist_items_by_paths_expands_virtual_playlist_folders() {
        let (state, root, repo_root, _thumbnail_root) =
            create_test_state("playlist-items-by-paths-virtual");
        let repo_id = create_repository_without_initial_sync(&state, &repo_root);

        let repo = state
            .load_repository_record(&repo_id)
            .expect("repository record should load");
        let connection = state
            .open_repository_connection(
                &repo.summary.repo_id,
                &repo.summary.path,
                &repo.backend_record,
            )
            .expect("repository connection should open");
        let playlist_id = "playlist-virtual-by-paths";
        let now = now_rfc3339();
        connection
            .execute(
                r#"
                INSERT INTO playlists (
                  playlist_id, repo_id, name, player_type_id, player_plugin_id,
                  player_label, file_class, sort_order, created_at, updated_at
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8, ?8)
                "#,
                params![
                    playlist_id,
                    repo_id,
                    "网易云虚拟歌单",
                    "momobako.playlist.audio-sequence",
                    "momobako.preview.media",
                    "音频顺序播放",
                    "audio",
                    now,
                ],
            )
            .expect("playlist should be inserted");

        for (asset_id, path, song_id, song_name) in [
            (
                asset_id_for_path(&repo_id, "创建的歌单/夜跑歌单/周杰伦 - 稻香.mp3"),
                "创建的歌单/夜跑歌单/周杰伦 - 稻香.mp3",
                2001_i64,
                "稻香",
            ),
            (
                asset_id_for_path(&repo_id, "创建的歌单/夜跑歌单/陈奕迅 - 孤勇者.mp3"),
                "创建的歌单/夜跑歌单/陈奕迅 - 孤勇者.mp3",
                2002_i64,
                "孤勇者",
            ),
        ] {
            connection
                .execute(
                    r#"
                    INSERT INTO assets (
                      asset_id, repo_id, path, filename, extension, size_bytes, created_at, modified_at,
                      hash, status, version, updated_at, thumbnail_path, is_virtual, provider_id,
                      provider_item_id, source_payload_json, local_absolute_path
                    )
                    VALUES (?1, ?2, ?3, ?4, 'mp3', 0, ?5, ?5, NULL, 'synced', 1, ?5, NULL, 1, ?6, ?7, ?8, NULL)
                    "#,
                    params![
                        asset_id,
                        repo_id,
                        path,
                        Path::new(path)
                            .file_name()
                            .expect("virtual asset path should contain a filename")
                            .to_string_lossy()
                            .to_string(),
                        now,
                        "netease-cloud-music",
                        song_id.to_string(),
                        serde_json::json!({
                            "provider": "netease-cloud-music",
                            "playlistId": 9001,
                            "playlistName": "夜跑歌单",
                            "playlistCategory": "created",
                            "songId": song_id,
                            "songName": song_name,
                            "virtualEntry": true
                        })
                        .to_string(),
                    ],
                )
                .expect("virtual asset should be inserted");
        }

        let detail = state
            .add_playlist_items_by_paths(PlaylistItemsByPathsAddRequest {
                repo_id: repo_id.clone(),
                playlist_id: playlist_id.to_string(),
                paths: vec!["创建的歌单/夜跑歌单".to_string()],
            })
            .expect("virtual playlist folder should expand into playable tracks");

        let mut actual_paths = detail
            .items
            .iter()
            .map(|item| item.path.clone())
            .collect::<Vec<_>>();
        actual_paths.sort();
        assert_eq!(
            actual_paths,
            vec![
                "创建的歌单/夜跑歌单/周杰伦 - 稻香.mp3".to_string(),
                "创建的歌单/夜跑歌单/陈奕迅 - 孤勇者.mp3".to_string(),
            ]
        );
        assert!(detail.items.iter().all(|item| item.is_virtual));

        drop(connection);
        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn plugin_registry_discovers_runtime_manifests() {
        let workspace = TestWorkspace::new("plugin-registry");
        let service_root = workspace.path("service");
        seed_standard_test_plugins(&service_root);
        let registry = backend_plugin_registry(&service_root);
        let manifests = registry.list_manifests();

        assert!(manifests.iter().any(|manifest| {
            manifest.plugin_id == LOCAL_FILESYSTEM_PLUGIN_ID
                && manifest.category == "source"
                && manifest.runtime == "native-dylib"
                && manifest
                    .legacy_plugin_ids
                    .contains(&LEGACY_LOCAL_FILESYSTEM_PLUGIN_ID.to_string())
        }));
        assert!(manifests.iter().any(|manifest| {
            manifest.plugin_id == "momobako.preview.media"
                && manifest.category == "preview"
                && manifest.sdk == "frontend"
                && manifest.hooks.iter().any(|hook| hook.slot == "playlist")
        }));
        assert!(manifests.iter().any(|manifest| {
            manifest.plugin_id == "momobako.library.audio"
                && manifest.category == "library-kind"
                && manifest
                    .optional
                    .contains(&"momobako.parser.audio".to_string())
        }));
        assert!(manifests.iter().any(|manifest| {
            manifest.plugin_id == "momobako.parser.audio" && manifest.category == "parser"
        }));
        assert!(manifests.iter().any(|manifest| {
            manifest.plugin_id == "momobako.service.network-search"
                && manifest.category == "service"
        }));
        assert_eq!(
            registry.normalize_plugin_id(LOCAL_FILESYSTEM_PLUGIN_ID),
            LOCAL_FILESYSTEM_PLUGIN_ID
        );
    }

    #[test]
    fn plugin_registry_resolves_dependencies_and_degraded_state() {
        let workspace = TestWorkspace::new("plugin-dependency-state");
        let plugin_root = workspace.path("service/plugins");
        fs::create_dir_all(&plugin_root).expect("runtime plugin dir should be created");
        write_test_plugin_archive_with_manifest(
            &plugin_root.join("required-provider.momoplug"),
            test_plugin_manifest_json("user.provider", "Provider", serde_json::json!({})),
        );
        write_test_plugin_archive_with_manifest(
            &plugin_root.join("optional-helper.momoplug"),
            test_plugin_manifest_json(
                "user.optional-helper",
                "Optional Helper",
                serde_json::json!({}),
            ),
        );
        write_test_plugin_archive_with_manifest(
            &plugin_root.join("dependent-plugin.momoplug"),
            test_plugin_manifest_json(
                "user.dependent",
                "Dependent Plugin",
                serde_json::json!({
                    "permissions": ["readMetadata"],
                    "requires": ["user.provider"],
                    "optional": ["user.optional-helper"]
                }),
            ),
        );
        let state = RepositoryState::from_root(workspace.path("service"));

        state
            .set_plugin_enabled(PluginEnabledRequest {
                plugin_id: "user.optional-helper".to_string(),
                enabled: false,
            })
            .expect("optional helper should be disabled");
        let plugins = state.list_plugins().expect("plugins should load");
        let dependent = plugins
            .iter()
            .find(|manifest| manifest.plugin_id == "user.dependent")
            .expect("dependent plugin should exist");

        assert_eq!(dependent.status, "ready");
        assert!(dependent.enabled);
        assert!(dependent.degraded);
        assert_eq!(
            dependent.dependency_status.optional[0].plugin_id,
            "user.optional-helper"
        );
        assert_eq!(dependent.dependency_status.optional[0].status, "disabled");
        assert!(dependent
            .degradation_reason
            .as_deref()
            .unwrap_or_default()
            .contains("Optional Helper"));

        fs::remove_file(plugin_root.join("required-provider.momoplug"))
            .expect("required provider archive should be removable");
        let plugins = state.list_plugins().expect("plugins should reload");
        let dependent = plugins
            .iter()
            .find(|manifest| manifest.plugin_id == "user.dependent")
            .expect("dependent plugin should remain listed");

        assert_eq!(dependent.status, "unavailable");
        assert!(!dependent.enabled);
        assert_eq!(dependent.dependency_status.required[0].status, "missing");
        assert!(dependent
            .disable_reason
            .as_deref()
            .unwrap_or_default()
            .contains("user.provider"));
    }

    #[test]
    fn plugin_data_directory_uses_service_plugin_data_root_and_legacy_ids() {
        let workspace = TestWorkspace::new("plugin-data-directory");
        let service_root = workspace.path("service");
        seed_standard_test_plugins(&service_root);
        let state = RepositoryState::from_root(service_root.clone());

        let response = state
            .get_plugin_data_directory(LEGACY_LOCAL_FILESYSTEM_PLUGIN_ID.to_string())
            .expect("plugin data directory should be returned");
        let expected_path = plugin_data_dir(&service_root, LOCAL_FILESYSTEM_PLUGIN_ID);

        assert_eq!(response.plugin_id, LOCAL_FILESYSTEM_PLUGIN_ID);
        assert_eq!(PathBuf::from(response.path), expected_path);
        assert!(expected_path.is_dir());
        assert!(expected_path.starts_with(service_root.join("plugin-data")));
    }

    #[test]
    fn plugin_data_file_preview_source_is_limited_to_plugin_data_dir() {
        let workspace = TestWorkspace::new("plugin-data-preview-source");
        let service_root = workspace.path("service");
        seed_standard_test_plugins(&service_root);
        let state = RepositoryState::from_root(service_root.clone());
        let data_dir = plugin_data_dir(&service_root, LOCAL_FILESYSTEM_PLUGIN_ID);
        fs::create_dir_all(&data_dir).expect("plugin data directory should be created");
        let preview_file = data_dir.join("preview.txt");
        fs::write(&preview_file, b"hello").expect("preview file should be written");
        let outside_file = service_root.join("outside.txt");
        fs::write(&outside_file, b"outside").expect("outside file should be written");

        let response = state
            .prepare_plugin_data_file_preview_source(PluginDataFilePreviewSourceRequest {
                plugin_id: LEGACY_LOCAL_FILESYSTEM_PLUGIN_ID.to_string(),
                path: preview_file.to_string_lossy().to_string(),
                media_type: "text/plain; charset=utf-8".to_string(),
            })
            .expect("plugin data preview source should register");

        assert_eq!(response.plugin_id, LOCAL_FILESYSTEM_PLUGIN_ID);
        assert_eq!(response.media_type, "text/plain; charset=utf-8");
        assert_eq!(response.size_bytes, 5);
        assert!(state.open_preview_file_source(&response.token).is_ok());

        let error = state
            .prepare_plugin_data_file_preview_source(PluginDataFilePreviewSourceRequest {
                plugin_id: LOCAL_FILESYSTEM_PLUGIN_ID.to_string(),
                path: outside_file.to_string_lossy().to_string(),
                media_type: "text/plain".to_string(),
            })
            .expect_err("outside plugin data files should be rejected");

        assert!(error.contains("outside plugin data directory"));
    }

    #[test]
    fn plugin_config_api_persists_values_in_plugin_data_dir() {
        let workspace = TestWorkspace::new("plugin-config");
        let service_root = workspace.path("service");
        seed_standard_test_plugins(&service_root);
        let state = RepositoryState::from_root(service_root.clone());

        let updated = state
            .set_plugin_config_value(PluginConfigSetRequest {
                plugin_id: LEGACY_LOCAL_FILESYSTEM_PLUGIN_ID.to_string(),
                key: "apiKey".to_string(),
                value: serde_json::json!("secret"),
            })
            .expect("plugin config value should be written");

        assert_eq!(updated.plugin_id, LOCAL_FILESYSTEM_PLUGIN_ID);
        assert_eq!(
            updated.values.get("apiKey"),
            Some(&serde_json::json!("secret"))
        );
        let data_dir = plugin_data_dir(&service_root, LOCAL_FILESYSTEM_PLUGIN_ID);
        assert!(data_dir.join("config.json").is_file());

        let loaded = state
            .get_plugin_config(LOCAL_FILESYSTEM_PLUGIN_ID.to_string())
            .expect("plugin config should be loaded");
        assert_eq!(
            loaded.values.get("apiKey"),
            Some(&serde_json::json!("secret"))
        );

        let deleted = state
            .delete_plugin_config_value(PluginConfigDeleteRequest {
                plugin_id: LOCAL_FILESYSTEM_PLUGIN_ID.to_string(),
                key: "apiKey".to_string(),
            })
            .expect("plugin config value should be deleted");
        assert!(!deleted.values.contains_key("apiKey"));
    }

    #[test]
    fn plugin_config_api_includes_schema_and_rejects_mismatched_values() {
        let workspace = TestWorkspace::new("plugin-config-schema");
        let plugin_root = workspace.path("service/plugins");
        fs::create_dir_all(&plugin_root).expect("runtime plugin dir should be created");
        write_test_plugin_archive_with_manifest(
            &plugin_root.join("configurable.momoplug"),
            test_plugin_manifest_json(
                "user.configurable",
                "Configurable Plugin",
                serde_json::json!({
                    "contributes": {
                        "settings": {
                            "schemaVersion": 1,
                            "fields": [
                                { "key": "enabled", "label": "Enabled", "type": "boolean" },
                                {
                                    "key": "mode",
                                    "label": "Mode",
                                    "type": "select",
                                    "options": [
                                        { "label": "Fast", "value": "fast" },
                                        { "label": "Careful", "value": "careful" }
                                    ]
                                }
                            ]
                        }
                    }
                }),
            ),
        );
        let state = RepositoryState::from_root(workspace.path("service"));

        let snapshot = state
            .get_plugin_config("user.configurable".to_string())
            .expect("plugin config schema should load");
        assert_eq!(
            snapshot.schema["fields"][0]["key"],
            serde_json::json!("enabled")
        );

        let error = state
            .set_plugin_config_value(PluginConfigSetRequest {
                plugin_id: "user.configurable".to_string(),
                key: "enabled".to_string(),
                value: serde_json::json!("yes"),
            })
            .expect_err("schema mismatch should be rejected");
        assert!(error.contains("boolean"));

        state
            .set_plugin_config_value(PluginConfigSetRequest {
                plugin_id: "user.configurable".to_string(),
                key: "mode".to_string(),
                value: serde_json::json!("fast"),
            })
            .expect("schema option should be accepted");
    }

    #[test]
    fn plugin_call_envelope_serializes_runtime_config_snapshot() {
        let envelope = PluginCallEnvelope {
            method: "provider.lookupMetadataCandidate".to_string(),
            payload: serde_json::json!({ "id": "sample-123456" }),
            runtime: PluginCallHostRuntime {
                plugin_id: "user.provider".to_string(),
                plugin_data_dir: "C:/MomoBako/.service-data/plugin-data/user-provider".to_string(),
                plugin_config: BTreeMap::from([(
                    "apiKey".to_string(),
                    serde_json::json!("secret"),
                )]),
            },
        };

        let value = serde_json::to_value(envelope).expect("envelope should serialize");

        assert_eq!(
            value["runtime"]["pluginId"],
            serde_json::json!("user.provider")
        );
        assert_eq!(
            value["runtime"]["pluginConfig"]["apiKey"],
            serde_json::json!("secret")
        );
    }

    #[test]
    fn plugin_dependency_resolution_accepts_legacy_ids() {
        let workspace = TestWorkspace::new("plugin-legacy-dependency");
        let plugin_root = workspace.path("service/plugins");
        fs::create_dir_all(&plugin_root).expect("runtime plugin dir should be created");
        write_test_plugin_archive_with_manifest(
            &plugin_root.join("provider.momoplug"),
            test_plugin_manifest_json(
                "user.provider",
                "Provider",
                serde_json::json!({
                    "legacyPluginIds": ["legacy.provider"],
                    "compat": {
                        "sdkVersion": "1",
                        "legacyPluginIds": []
                    }
                }),
            ),
        );
        write_test_plugin_archive_with_manifest(
            &plugin_root.join("dependent.momoplug"),
            test_plugin_manifest_json(
                "user.dependent",
                "Dependent",
                serde_json::json!({
                    "requires": ["legacy.provider"]
                }),
            ),
        );
        let state = RepositoryState::from_root(workspace.path("service"));
        let plugins = state.list_plugins().expect("plugins should load");
        let dependent = plugins
            .iter()
            .find(|manifest| manifest.plugin_id == "user.dependent")
            .expect("dependent plugin should exist");

        assert_eq!(dependent.status, "ready");
        assert_eq!(
            dependent.dependency_status.required[0].plugin_id,
            "user.provider"
        );
        assert!(dependent.dependency_status.missing_required.is_empty());
    }

    #[test]
    fn plugin_call_blocks_missing_required_dependency() {
        let workspace = TestWorkspace::new("plugin-call-required-missing");
        let service_root = workspace.path("service");
        let plugin_root = runtime_plugins_dir(&service_root);
        write_test_local_filesystem_plugin_archive(
            &plugin_root,
            serde_json::json!({ "requires": ["user.required-provider"] }),
        );
        let state = RepositoryState::from_root(service_root.clone());
        let error = state
            .call_plugin(test_list_files_plugin_call(
                LEGACY_LOCAL_FILESYSTEM_PLUGIN_ID,
                workspace.path("repo"),
            ))
            .expect_err("missing required dependency should block plugin call");

        assert!(error.contains("plugin call blocked by dependency status"));
        assert!(error.contains(LOCAL_FILESYSTEM_PLUGIN_ID));
        assert!(error.contains("filesystem.listFiles"));
        assert!(error.contains("缺少必需依赖"));
    }

    #[test]
    fn plugin_call_blocks_disabled_required_dependency() {
        let workspace = TestWorkspace::new("plugin-call-required-disabled");
        let service_root = workspace.path("service");
        let plugin_root = runtime_plugins_dir(&service_root);
        write_test_plugin_archive_with_manifest(
            &plugin_root.join("required-provider.momoplug"),
            test_plugin_manifest_json(
                "user.required-provider",
                "Required Provider",
                serde_json::json!({}),
            ),
        );
        write_test_local_filesystem_plugin_archive(
            &plugin_root,
            serde_json::json!({ "requires": ["user.required-provider"] }),
        );
        let state = RepositoryState::from_root(service_root);
        state
            .set_plugin_enabled(PluginEnabledRequest {
                plugin_id: "user.required-provider".to_string(),
                enabled: false,
            })
            .expect("required provider should be disabled");

        let error = state
            .call_plugin(test_list_files_plugin_call(
                LOCAL_FILESYSTEM_PLUGIN_ID,
                workspace.path("repo"),
            ))
            .expect_err("disabled required dependency should block plugin call");

        assert!(error.contains("plugin call blocked by dependency status"));
        assert!(error.contains("必需依赖不可用"));
        assert!(error.contains("Required Provider"));
    }

    #[test]
    fn plugin_call_returns_degraded_runtime_for_disabled_optional_dependency() {
        let workspace = TestWorkspace::new("plugin-call-optional-disabled");
        let service_root = workspace.path("service");
        let repo_root = workspace.path("repo");
        fs::create_dir_all(&repo_root).expect("repo root should be created");
        fs::write(repo_root.join("track.mp3"), b"audio").expect("test file should be written");
        let plugin_root = runtime_plugins_dir(&service_root);
        write_test_plugin_archive_with_manifest(
            &plugin_root.join("optional-helper.momoplug"),
            test_plugin_manifest_json(
                "user.optional-helper",
                "Optional Helper",
                serde_json::json!({}),
            ),
        );
        write_test_local_filesystem_plugin_archive(
            &plugin_root,
            serde_json::json!({ "optional": ["user.optional-helper"] }),
        );
        let state = RepositoryState::from_root(service_root);
        state
            .set_plugin_enabled(PluginEnabledRequest {
                plugin_id: "user.optional-helper".to_string(),
                enabled: false,
            })
            .expect("optional helper should be disabled");

        let response = state
            .call_plugin(test_list_files_plugin_call(
                LEGACY_LOCAL_FILESYSTEM_PLUGIN_ID,
                repo_root,
            ))
            .expect("optional dependency should not block plugin call");

        assert_eq!(response.plugin_id, LOCAL_FILESYSTEM_PLUGIN_ID);
        assert!(response.payload.is_array());
        let runtime = response
            .runtime
            .expect("degraded runtime context should be returned");
        assert!(runtime.degraded);
        assert!(runtime
            .degradation_reason
            .as_deref()
            .unwrap_or_default()
            .contains("Optional Helper"));
        assert_eq!(
            runtime.dependency_status.optional[0].plugin_id,
            "user.optional-helper"
        );
        assert_eq!(runtime.dependency_status.optional[0].status, "disabled");
    }

    #[test]
    fn plugin_call_accepts_legacy_required_dependency_id() {
        let workspace = TestWorkspace::new("plugin-call-legacy-required");
        let service_root = workspace.path("service");
        let repo_root = workspace.path("repo");
        fs::create_dir_all(&repo_root).expect("repo root should be created");
        let plugin_root = runtime_plugins_dir(&service_root);
        write_test_plugin_archive_with_manifest(
            &plugin_root.join("provider.momoplug"),
            test_plugin_manifest_json(
                "user.provider",
                "Provider",
                serde_json::json!({
                    "legacyPluginIds": ["legacy.provider"],
                    "compat": {
                        "sdkVersion": "1",
                        "legacyPluginIds": []
                    }
                }),
            ),
        );
        write_test_local_filesystem_plugin_archive(
            &plugin_root,
            serde_json::json!({ "requires": ["legacy.provider"] }),
        );
        let state = RepositoryState::from_root(service_root);

        let response = state
            .call_plugin(test_list_files_plugin_call(
                LEGACY_LOCAL_FILESYSTEM_PLUGIN_ID,
                repo_root,
            ))
            .expect("legacy dependency id should resolve before plugin call");

        assert_eq!(response.plugin_id, LOCAL_FILESYSTEM_PLUGIN_ID);
        assert!(response.runtime.is_none());
    }

    #[test]
    fn plugin_hook_execution_records_declared_hook_calls() {
        let workspace = TestWorkspace::new("plugin-hook-execution-records");
        let service_root = workspace.path("service");
        let repo_root = workspace.path("repo");
        fs::create_dir_all(&repo_root).expect("repo root should be created");
        fs::write(repo_root.join("note.txt"), b"note").expect("test file should be written");
        let plugin_root = runtime_plugins_dir(&service_root);
        write_test_local_filesystem_plugin_archive(
            &plugin_root,
            serde_json::json!({
                "hooks": [
                    {
                        "slot": "auditLog",
                        "action": "filesystem.listFiles",
                        "label": "记录文件列表"
                    }
                ]
            }),
        );
        let state = RepositoryState::from_root(service_root);

        state
            .call_plugin(test_list_files_plugin_call(
                LEGACY_LOCAL_FILESYSTEM_PLUGIN_ID,
                repo_root,
            ))
            .expect("declared hook plugin call should succeed");
        let all_records = state
            .list_plugin_hook_executions(PluginHookExecutionListRequest::default())
            .expect("hook execution records should load");

        assert_eq!(all_records.records.len(), 1);
        let record = &all_records.records[0];
        assert_eq!(record.plugin_id, LOCAL_FILESYSTEM_PLUGIN_ID);
        assert_eq!(record.hook_slot, "auditLog");
        assert_eq!(record.hook_action, "filesystem.listFiles");
        assert_eq!(record.hook_label.as_deref(), Some("记录文件列表"));
        assert_eq!(record.status, "success");
        assert_eq!(record.target.get("repoRoot"), None);
        assert_eq!(record.target.get("config"), None);

        state
            .call_plugin(PluginCallRequest {
                plugin_id: LOCAL_FILESYSTEM_PLUGIN_ID.to_string(),
                method: "filesystem.listTree".to_string(),
                payload: serde_json::json!({
                    "repoRoot": workspace.path("repo"),
                    "path": "note.txt"
                }),
            })
            .expect("non-hook plugin call should succeed");
        let filtered = state
            .list_plugin_hook_executions(PluginHookExecutionListRequest {
                plugin_id: Some(LEGACY_LOCAL_FILESYSTEM_PLUGIN_ID.to_string()),
                limit: Some(1),
            })
            .expect("filtered hook execution records should load");

        assert_eq!(filtered.records.len(), 1);
        assert_eq!(filtered.records[0].hook_action, "filesystem.listFiles");
    }

    #[test]
    fn release_plugin_manifest_loading_returns_empty_when_runtime_dir_is_empty() {
        let workspace = TestWorkspace::new("runtime-plugin-empty");
        let manifests = load_plugin_manifests_from_runtime(workspace.path("plugins"));

        assert!(manifests.is_empty());
    }

    #[test]
    fn runtime_manifest_scan_reflects_deleted_plugin_archives() {
        let workspace = TestWorkspace::new("runtime-plugin-scan");
        let plugin_root = workspace.path("plugins");
        fs::create_dir_all(&plugin_root).expect("runtime plugin dir should be created");
        let plugin_archive = plugin_root.join("sample-plugin.momoplug");
        write_test_plugin_archive(&plugin_archive, "user.sample-runtime");

        let manifests = load_plugin_manifests_from_runtime(plugin_root.clone());
        assert_eq!(manifests.len(), 1);
        assert_eq!(manifests[0].manifest.plugin_id, "user.sample-runtime");

        fs::remove_file(plugin_archive).expect("runtime plugin archive should be removable");
        let manifests = load_plugin_manifests_from_runtime(plugin_root);
        assert!(manifests.is_empty());
    }

    #[test]
    fn set_plugin_enabled_persists_plugin_state() {
        let workspace = TestWorkspace::new("plugin-enabled-state");
        let service_root = workspace.path("service");
        seed_standard_test_plugins(&service_root);
        let state = RepositoryState::from_root(service_root.clone());

        let response = state
            .set_plugin_enabled(PluginEnabledRequest {
                plugin_id: "momobako.preview.media".to_string(),
                enabled: false,
            })
            .expect("plugin should be disabled");
        let disabled = response
            .plugins
            .iter()
            .find(|plugin| plugin.plugin_id == "momobako.preview.media")
            .expect("media plugin should be listed");
        assert!(!disabled.enabled);
        assert_eq!(disabled.status, "disabled");

        let reloaded_state = RepositoryState::from_root(service_root);
        let plugins = reloaded_state
            .list_plugins()
            .expect("plugins should reload from persisted state");
        let disabled = plugins
            .iter()
            .find(|plugin| plugin.plugin_id == "momobako.preview.media")
            .expect("media plugin should be listed after reload");
        assert!(!disabled.enabled);
    }

    #[test]
    fn delete_plugin_rejects_builtin_plugins() {
        let workspace = TestWorkspace::new("builtin-plugin-delete");
        let service_root = workspace.path("service");
        seed_standard_test_plugins(&service_root);
        let state = RepositoryState::from_root(service_root);

        let error = state
            .delete_plugin("momobako.preview.media".to_string())
            .expect_err("built-in plugins should not be deleted");

        assert!(error.contains("built-in plugins cannot be deleted"));
    }

    #[test]
    fn install_plugin_from_archive_loads_and_deletes_user_plugin() {
        let workspace = TestWorkspace::new("plugin-archive-install");
        let service_root = workspace.path("service");
        let archive_path = workspace.path("sample-plugin.momoplug");
        write_test_plugin_archive(&archive_path, "user.sample-metadata");
        let state = RepositoryState::from_root(service_root.clone());

        let response = state
            .install_plugin_from_archive(PluginInstallRequest {
                package_path: archive_path.to_string_lossy().to_string(),
            })
            .expect("plugin archive should install");
        let installed = response
            .plugins
            .iter()
            .find(|plugin| plugin.plugin_id == "user.sample-metadata")
            .expect("installed plugin should be listed");
        assert_eq!(installed.source, "user");
        assert!(installed.enabled);
        assert!(runtime_plugins_dir(&service_root)
            .join("user-sample-metadata-0.1.0.momoplug")
            .is_file());

        let response = state
            .delete_plugin("user.sample-metadata".to_string())
            .expect("user plugin should be deleted");
        assert!(!response
            .plugins
            .iter()
            .any(|plugin| plugin.plugin_id == "user.sample-metadata"));
        assert!(!runtime_plugins_dir(&service_root)
            .join("user-sample-metadata-0.1.0.momoplug")
            .exists());
    }

    #[test]
    fn read_plugin_archive_text_supports_single_root_directory_packages() {
        let workspace = TestWorkspace::new("plugin-archive-read-text");
        let service_root = workspace.path("service");
        let runtime_root = runtime_plugins_dir(&service_root);
        fs::create_dir_all(&runtime_root).expect("runtime plugin dir should be created");
        write_test_plugin_archive_with_options(
            &runtime_root.join("example-text-preview-0.1.0.momoplug"),
            TestPluginArchiveOptions {
                plugin_id: "momobako.example.text-preview",
                name: "Example Text Preview",
                source: "user",
                runtime: "vue-module",
                sdk: "frontend",
                kind: "preview",
                plugin_type_layer: "library-kind",
                plugin_type_kind: "preview",
                entry: serde_json::json!({
                    "frontend": {
                        "module": "dist/register.js",
                        "export": "register"
                    }
                }),
                extra_files: vec![(
                    "dist/register.js".to_string(),
                    "export function register(){ return 'ok'; }".to_string(),
                )],
            },
        );
        let state = RepositoryState::from_root(service_root);

        let response = state
            .read_plugin_archive_text(PluginArchiveReadRequest {
                plugin_id: "momobako.example.text-preview".to_string(),
                path: "dist/register.js".to_string(),
            })
            .expect("archive text should load from single-root package");

        assert_eq!(
            response.path,
            "momobako-example-text-preview-0.1.0/dist/register.js"
        );
        assert!(response.text.contains("register"));
    }

    #[test]
    fn runtime_builtin_plugins_keep_manifest_source_value() {
        let workspace = TestWorkspace::new("runtime-builtin-source");
        let service_root = workspace.path("service");
        let runtime_root = runtime_plugins_dir(&service_root);
        fs::create_dir_all(&runtime_root).expect("runtime plugin dir should be created");
        write_test_plugin_archive_with_options(
            &runtime_root.join("media-preview-1.0.0.momoplug"),
            TestPluginArchiveOptions {
                plugin_id: "momobako.preview.media",
                name: "Media Preview",
                source: "builtin",
                runtime: "manifest-only",
                sdk: "frontend",
                kind: "preview",
                plugin_type_layer: "library-kind",
                plugin_type_kind: "preview",
                entry: serde_json::json!({
                    "frontend": {
                        "module": "dist/register.js",
                        "export": "register"
                    }
                }),
                extra_files: vec![(
                    "dist/register.js".to_string(),
                    "export function register() {}".to_string(),
                )],
            },
        );

        let state = RepositoryState::from_root(service_root);
        let plugins = state.list_plugins().expect("plugins should load");
        let plugin = plugins
            .iter()
            .find(|item| item.plugin_id == "momobako.preview.media")
            .expect("media plugin should be listed");

        assert_eq!(plugin.source, "builtin");
    }

    #[test]
    fn install_plugin_from_archive_rejects_zip_extension() {
        let workspace = TestWorkspace::new("plugin-archive-zip");
        let service_root = workspace.path("service");
        let archive_path = workspace.path("sample-plugin.zip");
        write_test_plugin_archive(&archive_path, "user.sample-metadata");
        let state = RepositoryState::from_root(service_root);

        let error = state
            .install_plugin_from_archive(PluginInstallRequest {
                package_path: archive_path.to_string_lossy().to_string(),
            })
            .expect_err("zip extension should be rejected");

        assert!(error.contains(".momoplug extension"));
    }

    #[test]
    fn install_plugin_from_archive_rejects_root_level_manifest_packages() {
        let workspace = TestWorkspace::new("plugin-archive-root-manifest");
        let service_root = workspace.path("service");
        let archive_path = workspace.path("sample-plugin.momoplug");
        write_test_plugin_archive_without_root_dir(&archive_path, "user.sample-rootless");
        let state = RepositoryState::from_root(service_root);

        let error = state
            .install_plugin_from_archive(PluginInstallRequest {
                package_path: archive_path.to_string_lossy().to_string(),
            })
            .expect_err("root-level manifest package should be rejected");

        assert!(error.contains("exactly one root directory with manifest.json"));
    }

    #[test]
    fn install_plugin_from_archive_rejects_duplicate_plugin_id() {
        let workspace = TestWorkspace::new("plugin-archive-duplicate-id");
        let service_root = workspace.path("service");
        let first_archive = workspace.path("sample-plugin-a.momoplug");
        let second_archive = workspace.path("sample-plugin-b.momoplug");
        write_test_plugin_archive(&first_archive, "user.sample-duplicate");
        write_test_plugin_archive(&second_archive, "user.sample-duplicate");
        let state = RepositoryState::from_root(service_root);

        state
            .install_plugin_from_archive(PluginInstallRequest {
                package_path: first_archive.to_string_lossy().to_string(),
            })
            .expect("first plugin archive should install");

        let error = state
            .install_plugin_from_archive(PluginInstallRequest {
                package_path: second_archive.to_string_lossy().to_string(),
            })
            .expect_err("duplicate plugin id should be rejected");

        assert!(error.contains("plugin already exists: user.sample-duplicate"));
    }

    #[test]
    fn broken_plugin_archives_do_not_hide_other_runtime_plugins() {
        let workspace = TestWorkspace::new("broken-plugin-archive");
        let service_root = workspace.path("service");
        let runtime_root = runtime_plugins_dir(&service_root);
        fs::create_dir_all(&runtime_root).expect("runtime plugin dir should be created");
        write_test_plugin_archive(
            &runtime_root.join("good-plugin.momoplug"),
            "user.good-plugin",
        );
        fs::write(runtime_root.join("broken-plugin.momoplug"), b"not-a-zip")
            .expect("broken plugin archive should be written");

        let state = RepositoryState::from_root(service_root);
        let plugins = state.list_plugins().expect("plugins should load");

        let good = plugins
            .iter()
            .find(|plugin| plugin.plugin_id == "user.good-plugin")
            .expect("good plugin should still be listed");
        assert_eq!(good.status, "ready");

        let broken = plugins
            .iter()
            .find(|plugin| plugin.plugin_id == "broken.broken-plugin")
            .expect("broken plugin placeholder should be listed");
        assert!(!broken.enabled);
        assert!(matches!(broken.status.as_str(), "error" | "disabled"));
        assert!(broken.description.contains("Failed to read plugin archive"));
    }

    fn write_test_plugin_archive(path: &Path, plugin_id: &str) {
        write_test_plugin_archive_with_options(
            path,
            TestPluginArchiveOptions {
                plugin_id,
                ..TestPluginArchiveOptions::default()
            },
        );
    }

    fn seed_standard_test_plugins(service_root: &Path) {
        let runtime_root = runtime_plugins_dir(service_root);
        write_test_plugin_archive_with_manifest(
            &runtime_root.join("local-filesystem.momoplug"),
            test_plugin_manifest_json(
                LOCAL_FILESYSTEM_PLUGIN_ID,
                "Local Filesystem",
                serde_json::json!({
                    "legacyPluginIds": [LEGACY_LOCAL_FILESYSTEM_PLUGIN_ID],
                    "kind": "filesystem",
                    "category": "source",
                    "type": {
                        "layer": "source",
                        "kind": "filesystem"
                    },
                    "capabilities": ["listFiles", "readFile", "writeFile", "moveFile", "deleteFile"],
                    "sdk": "backend",
                    "runtime": "native-dylib",
                    "source": "system"
                }),
            ),
        );
        write_test_plugin_archive_with_manifest(
            &runtime_root.join("media-preview.momoplug"),
            test_plugin_manifest_json(
                "momobako.preview.media",
                "Media Preview",
                serde_json::json!({
                    "kind": "preview",
                    "category": "preview",
                    "type": {
                        "layer": "library-kind",
                        "kind": "preview"
                    },
                    "capabilities": ["preview", "playlist", "media"],
                    "sdk": "frontend",
                    "runtime": "vue-module",
                    "source": "builtin",
                    "hooks": [
                        { "slot": "playlist", "action": "preview.media.enqueue", "label": "加入播放列表" }
                    ]
                }),
            ),
        );
        write_test_plugin_archive_with_manifest(
            &runtime_root.join("library-audio.momoplug"),
            test_plugin_manifest_json(
                "momobako.library.audio",
                "Audio Library",
                serde_json::json!({
                    "kind": "audio",
                    "category": "library-kind",
                    "type": {
                        "layer": "library-kind",
                        "kind": "audio"
                    },
                    "capabilities": ["library", "audio"],
                    "sdk": "frontend",
                    "runtime": "manifest-only",
                    "source": "builtin",
                    "optional": ["momobako.parser.audio"]
                }),
            ),
        );
        write_test_plugin_archive_with_manifest(
            &runtime_root.join("parser-audio.momoplug"),
            test_plugin_manifest_json(
                "momobako.parser.audio",
                "Audio Parser",
                serde_json::json!({
                    "kind": "parser",
                    "category": "parser",
                    "type": {
                        "layer": "parser",
                        "kind": "audio"
                    },
                    "capabilities": ["parse", "audio"],
                    "sdk": "backend",
                    "runtime": "manifest-only",
                    "source": "builtin"
                }),
            ),
        );
        write_test_plugin_archive_with_manifest(
            &runtime_root.join("service-network-search.momoplug"),
            test_plugin_manifest_json(
                "momobako.service.network-search",
                "Network Search",
                serde_json::json!({
                    "kind": "search",
                    "category": "service",
                    "type": {
                        "layer": "provider-service",
                        "kind": "search"
                    },
                    "capabilities": ["network", "search"],
                    "sdk": "backend",
                    "runtime": "manifest-only",
                    "source": "builtin"
                }),
            ),
        );
    }

    fn test_plugin_manifest_json(
        plugin_id: &str,
        name: &str,
        overrides: serde_json::Value,
    ) -> serde_json::Value {
        let mut manifest = serde_json::json!({
            "pluginId": plugin_id,
            "legacyPluginIds": [],
            "name": name,
            "version": "0.1.0",
            "kind": "metadata",
            "description": "Test plugin.",
            "capabilities": ["metadata"],
            "enabled": true,
            "sdk": "backend",
            "entry": {},
            "source": "user",
            "runtime": "manifest-only",
            "permissions": [],
            "compat": {
                "sdkVersion": "1",
                "legacyPluginIds": []
            },
            "status": "ready"
        });
        if let (Some(base), Some(extra)) = (manifest.as_object_mut(), overrides.as_object()) {
            for (key, value) in extra {
                base.insert(key.clone(), value.clone());
            }
        }
        manifest
    }

    fn write_test_plugin_archive_with_manifest(path: &Path, manifest: serde_json::Value) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("plugin archive parent should be created");
        }
        let plugin_id = manifest
            .get("pluginId")
            .and_then(|value| value.as_str())
            .unwrap_or("user.sample-metadata");
        let file = File::create(path).expect("plugin archive should be created");
        let mut archive = zip::ZipWriter::new(file);
        let root_dir = format!("{plugin_id}-0.1.0");
        archive
            .start_file(
                format!("{root_dir}/manifest.json"),
                zip::write::SimpleFileOptions::default(),
            )
            .expect("manifest entry should start");
        archive
            .write_all(
                serde_json::to_string_pretty(&manifest)
                    .expect("manifest should encode")
                    .as_bytes(),
            )
            .expect("manifest should write");
        archive.finish().expect("plugin archive should finish");
    }

    #[derive(Clone)]
    struct TestPluginArchiveOptions<'a> {
        plugin_id: &'a str,
        name: &'a str,
        source: &'a str,
        runtime: &'a str,
        sdk: &'a str,
        kind: &'a str,
        plugin_type_layer: &'a str,
        plugin_type_kind: &'a str,
        entry: serde_json::Value,
        extra_files: Vec<(String, String)>,
    }

    impl Default for TestPluginArchiveOptions<'_> {
        fn default() -> Self {
            Self {
                plugin_id: "user.sample-metadata",
                name: "Sample Metadata",
                source: "user",
                runtime: "manifest-only",
                sdk: "backend",
                kind: "metadata",
                plugin_type_layer: "provider-service",
                plugin_type_kind: "metadata",
                entry: serde_json::json!({}),
                extra_files: Vec::new(),
            }
        }
    }

    fn write_test_plugin_archive_with_options(path: &Path, options: TestPluginArchiveOptions<'_>) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("plugin archive parent should be created");
        }
        let file = File::create(path).expect("plugin archive should be created");
        let mut archive = zip::ZipWriter::new(file);
        let root_dir = format!("{}-0.1.0", slugify_ascii_component(options.plugin_id));
        let manifest_path = format!("{root_dir}/manifest.json");
        archive
            .start_file(manifest_path, zip::write::SimpleFileOptions::default())
            .expect("manifest entry should start");
        archive
            .write_all(
                serde_json::to_string_pretty(&serde_json::json!({
                    "pluginId": options.plugin_id,
                    "legacyPluginIds": [],
                    "name": options.name,
                    "version": "0.1.0",
                    "type": {
                        "layer": options.plugin_type_layer,
                        "kind": options.plugin_type_kind
                    },
                    "kind": options.kind,
                    "description": "Test plugin installed from archive.",
                    "capabilities": [options.kind],
                    "enabled": true,
                    "sdk": options.sdk,
                    "entry": options.entry,
                    "source": options.source,
                    "runtime": options.runtime,
                    "permissions": [],
                    "compat": {
                        "sdkVersion": "1",
                        "legacyPluginIds": []
                    },
                    "status": "ready"
                }))
                .expect("manifest should encode")
                .as_bytes(),
            )
            .expect("manifest should write");
        for (relative_path, content) in options.extra_files {
            archive
                .start_file(
                    format!("{root_dir}/{relative_path}"),
                    zip::write::SimpleFileOptions::default(),
                )
                .expect("extra entry should start");
            archive
                .write_all(content.as_bytes())
                .expect("extra entry should write");
        }
        archive.finish().expect("plugin archive should finish");
    }

    fn write_test_local_filesystem_plugin_archive(
        plugin_root: &Path,
        dependency_overrides: serde_json::Value,
    ) {
        let mut manifest = test_plugin_manifest_json(
            LOCAL_FILESYSTEM_PLUGIN_ID,
            "Local Filesystem",
            serde_json::json!({
                "legacyPluginIds": [LEGACY_LOCAL_FILESYSTEM_PLUGIN_ID],
                "kind": "filesystem",
                "category": "source",
                "type": {
                    "layer": "source",
                    "kind": "filesystem"
                },
                "capabilities": ["listFiles"],
                "sdk": "backend",
                "runtime": "native-dylib",
                "source": "system"
            }),
        );
        if let (Some(base), Some(extra)) =
            (manifest.as_object_mut(), dependency_overrides.as_object())
        {
            for (key, value) in extra {
                base.insert(key.clone(), value.clone());
            }
        }
        write_test_plugin_archive_with_manifest(
            &plugin_root.join("local-filesystem.momoplug"),
            manifest,
        );
    }

    fn test_list_files_plugin_call(plugin_id: &str, repo_root: PathBuf) -> PluginCallRequest {
        PluginCallRequest {
            plugin_id: plugin_id.to_string(),
            method: "filesystem.listFiles".to_string(),
            payload: serde_json::json!({
                "repoRoot": repo_root,
                "config": {}
            }),
        }
    }

    fn write_test_plugin_archive_without_root_dir(path: &Path, plugin_id: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("plugin archive parent should be created");
        }
        let file = File::create(path).expect("plugin archive should be created");
        let mut archive = zip::ZipWriter::new(file);
        archive
            .start_file("manifest.json", zip::write::SimpleFileOptions::default())
            .expect("manifest entry should start");
        archive
            .write_all(
                serde_json::to_string_pretty(&serde_json::json!({
                    "pluginId": plugin_id,
                    "legacyPluginIds": [],
                    "name": "Rootless Plugin",
                    "version": "0.1.0",
                    "type": {
                        "layer": "provider-service",
                        "kind": "metadata"
                    },
                    "kind": "metadata",
                    "description": "Invalid root-level manifest package.",
                    "capabilities": ["metadata"],
                    "enabled": true,
                    "sdk": "backend",
                    "entry": {},
                    "source": "user",
                    "runtime": "manifest-only",
                    "permissions": [],
                    "compat": {
                        "sdkVersion": "1",
                        "legacyPluginIds": []
                    },
                    "status": "ready"
                }))
                .expect("manifest should encode")
                .as_bytes(),
            )
            .expect("manifest should write");
        archive.finish().expect("plugin archive should finish");
    }

    #[test]
    fn disabled_manifest_only_backend_is_not_attachable() {
        let workspace = TestWorkspace::new("disabled-backend");
        let service_root = workspace.path("service");
        let runtime_root = runtime_plugins_dir(&service_root);
        write_test_plugin_archive_with_manifest(
            &runtime_root.join("webdav.momoplug"),
            test_plugin_manifest_json(
                TEST_WEBDAV_PLUGIN_ID,
                "WebDAV",
                serde_json::json!({
                    "kind": "webdav",
                    "category": "source",
                    "type": {
                        "layer": "source",
                        "kind": "webdav"
                    },
                    "capabilities": ["listFiles", "readFile", "writeFile"],
                    "enabled": false,
                    "sdk": "backend",
                    "runtime": "manifest-only",
                    "source": "system"
                }),
            ),
        );
        let state = RepositoryState::from_root(service_root);
        let repo_root = workspace.path("repo");

        let error = state
            .create_repository(RepositoryMutationRequest {
                repo_id: Some("repo-webdav".to_string()),
                name: "WebDAV Repo".to_string(),
                path: repo_root.to_string_lossy().to_string(),
                backend_plugin_id: Some(TEST_WEBDAV_PLUGIN_ID.to_string()),
                backend_config: None,
                skip_initial_sync: false,
            })
            .expect_err("disabled manifest-only backend should not create a repository");

        assert!(
            error.contains("plugin is disabled")
                || error.contains("plugin runtime is not available")
        );
    }

    #[test]
    fn local_filesystem_backend_runs_through_runtime_registry() {
        let workspace = TestWorkspace::new("runtime-local-backend");
        let service_root = workspace.path("service");
        let repo_root = workspace.path("repo");
        install_local_filesystem_test_plugin_archive(&service_root);
        let state = RepositoryState::from_root(service_root);

        let repo_id = state
            .create_repository(RepositoryMutationRequest {
                repo_id: Some("repo-runtime".to_string()),
                name: "Runtime Repo".to_string(),
                path: repo_root.to_string_lossy().to_string(),
                backend_plugin_id: Some(LOCAL_FILESYSTEM_PLUGIN_ID.to_string()),
                backend_config: None,
                skip_initial_sync: false,
            })
            .expect("local filesystem backend should create")
            .repository
            .repo_id;

        let record = state
            .load_repository_record(&repo_id)
            .expect("repository record should load");
        assert_eq!(record.backend_record.plugin_id, LOCAL_FILESYSTEM_PLUGIN_ID);

        state
            .create_file(FileCreateRequest {
                repo_id: repo_id.clone(),
                parent_path: None,
                name: "note.txt".to_string(),
            })
            .expect("runtime local backend should create a file");
        assert!(repo_root.join("note.txt").is_file());

        state
            .rename_entry(FileRenameRequest {
                repo_id: repo_id.clone(),
                path: "note.txt".to_string(),
                new_name: "renamed.txt".to_string(),
            })
            .expect("runtime local backend should rename a file");
        assert!(repo_root.join("renamed.txt").is_file());

        state
            .delete_entry(FileDeleteRequest {
                repo_id,
                path: "renamed.txt".to_string(),
                mode: None,
            })
            .expect("runtime local backend should move a file to trash");
        assert!(!repo_root.join("renamed.txt").exists());
        assert!(repository_trash_dir(&repo_root).exists());
    }

    #[test]
    fn update_repository_backend_config_persists_registry_and_repository_metadata() {
        let (state, root, repo_root, _thumbnail_root) =
            create_test_state("update-repo-backend-config");
        let repo_id = create_repository_for_path(&state, &repo_root);

        let response = state
            .update_repository_backend_config(RepositoryBackendConfigUpdateRequest {
                repo_id: repo_id.clone(),
                backend_config: serde_json::json!({
                    "cookie": "MUSIC_U=updated-cookie",
                    "accountId": "123456"
                }),
            })
            .expect("repository backend config should update");

        assert_eq!(response.repository.repo_id, repo_id);

        let repo = state
            .load_repository_record(&repo_id)
            .expect("repository record should load");
        assert_eq!(
            repo.backend_record.config,
            serde_json::json!({
                "cookie": "MUSIC_U=updated-cookie",
                "accountId": "123456"
            })
        );

        let metadata_path = repository_meta_dir(&repo_root).join(REPO_METADATA_FILE_NAME);
        let metadata_raw =
            fs::read_to_string(metadata_path).expect("repository metadata should exist");
        let metadata: RepositoryMetadataFileImport =
            serde_json::from_str(&metadata_raw).expect("repository metadata should decode");
        assert_eq!(
            metadata.backend_config,
            Some(serde_json::json!({
                "cookie": "MUSIC_U=updated-cookie",
                "accountId": "123456"
            }))
        );

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn list_repositories_marks_missing_local_paths() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("missing-repo-list");
        let repo_id = create_repository_for_path(&state, &repo_root);
        fs::remove_dir_all(&repo_root).expect("repo root should be removed");

        let repositories = state
            .list_repositories()
            .expect("repositories should list even when path is missing");
        let repository = repositories
            .iter()
            .find(|item| item.repo_id == repo_id)
            .expect("missing repository should stay registered");

        assert_eq!(repository.status, "missing");
        assert_eq!(repository.asset_count, 0);
        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn relocate_repository_requires_matching_metadata_repo_id() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("relocate-mismatch");
        let repo_id = create_repository_for_path(&state, &repo_root);
        let other_root = root.join("other-repo");
        let other_meta_dir = other_root.join(REPO_META_DIR);
        fs::create_dir_all(&other_meta_dir).expect("other metadata dir should be created");
        fs::write(
            other_meta_dir.join(REPO_METADATA_FILE_NAME),
            serde_json::to_string_pretty(&serde_json::json!({
                "repoId": "repo-other",
                "name": "Other Repo",
                "rootPath": other_root.to_string_lossy(),
                "backendPluginId": LOCAL_FILESYSTEM_PLUGIN_ID,
                "backendConfig": {},
                "createdAt": now_rfc3339(),
                "schemaVersion": REPO_SCHEMA_VERSION,
            }))
            .expect("metadata json should encode"),
        )
        .expect("other metadata should be written");

        let error = state
            .relocate_repository(RepositoryRelocateRequest {
                repo_id,
                path: other_root.to_string_lossy().to_string(),
            })
            .expect_err("mismatched metadata repo id should fail");

        assert!(error.contains("different repository"));
        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn relocate_repository_updates_path_and_preserves_repo_id() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("relocate-success");
        let repo_id = create_repository_for_path(&state, &repo_root);
        state
            .create_smart_folder(SmartFolderMutationRequest {
                repo_id: repo_id.clone(),
                smart_folder_id: Some("sf-reference".to_string()),
                parent_id: None,
                name: "Reference".to_string(),
                filter: SmartFolderFilter {
                    path_prefix: Some("Reference".to_string()),
                    ..SmartFolderFilter::default()
                },
            })
            .expect("smart folder should be created before relocation");
        let relocated_root = root.join("relocated-repo");
        fs::rename(&repo_root, &relocated_root).expect("repo root should move");

        let missing = state
            .list_repositories()
            .expect("repositories should list")
            .into_iter()
            .find(|item| item.repo_id == repo_id)
            .expect("repository should stay registered");
        assert_eq!(missing.status, "missing");

        let response = state
            .relocate_repository(RepositoryRelocateRequest {
                repo_id: repo_id.clone(),
                path: relocated_root.to_string_lossy().to_string(),
            })
            .expect("relocation should succeed");

        assert_eq!(response.repository.repo_id, repo_id);
        assert_eq!(
            PathBuf::from(&response.repository.path),
            canonicalize_local_path(&relocated_root).expect("relocated root should canonicalize")
        );
        let ready = state
            .list_repositories()
            .expect("repositories should list after relocation")
            .into_iter()
            .find(|item| item.repo_id == response.repository.repo_id)
            .expect("repository should still be registered");
        assert_eq!(ready.status, "ready");
        let raw_metadata = fs::read_to_string(
            relocated_root
                .join(REPO_META_DIR)
                .join(REPO_METADATA_FILE_NAME),
        )
        .expect("relocated metadata should read");
        let metadata: RepositoryMetadataFileImport =
            serde_json::from_str(&raw_metadata).expect("relocated metadata should parse");
        let expected_root_path = relocated_root.to_string_lossy().to_string();
        assert_eq!(metadata.repo_id, repo_id);
        assert_eq!(
            metadata.root_path.as_deref(),
            Some(expected_root_path.as_str())
        );
        let smart_folders = state
            .list_smart_folders(&response.repository.repo_id)
            .expect("smart folders should load after relocation");
        assert_eq!(smart_folders.len(), 1);
        assert_eq!(smart_folders[0].folder.smart_folder_id, "sf-reference");
        assert_eq!(
            smart_folders[0].folder.filter.path_prefix.as_deref(),
            Some("Reference")
        );
        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn delete_repository_removes_registry_and_managed_state_dir() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("delete-repo-state");
        let repo_id = create_repository_for_path(&state, &repo_root);
        let managed_state_dir = repository_state_storage_dir(&state.root, &repo_id);
        fs::create_dir_all(managed_state_dir.join("cache"))
            .expect("managed state dir should be created");
        fs::write(managed_state_dir.join("cache/index.json"), "{}")
            .expect("managed cache file should be written");

        state
            .delete_repository(&repo_id)
            .expect("repository should delete");

        assert!(!managed_state_dir.exists());
        assert!(repo_root.exists());
        assert!(state
            .list_repositories()
            .expect("repositories should list after delete")
            .iter()
            .all(|item| item.repo_id != repo_id));
        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    const LONG_RELATIVE_PATH: &str = "CubismSdkForNative-5-r.5/Samples/OpenGL/Demo/proj.harmonyos.cmake/Full/entry/src/main/resources/base/media/startIcon.png";

    fn unique_temp_dir(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("momobako-{label}-{}-{unique}", std::process::id()))
    }

    fn create_test_state(label: &str) -> (RepositoryState, PathBuf, PathBuf, PathBuf) {
        let root = unique_temp_dir(label);
        let repo_root = root.join("repo");
        fs::create_dir_all(&repo_root).expect("repo root should be created");
        let root = canonicalize_local_path(&root).expect("test root should canonicalize");
        let repo_root = canonicalize_local_path(&repo_root).expect("repo root should canonicalize");
        let state_root = root.join("state");
        let thumbnail_root = repo_root.join(REPO_META_DIR).join("thumbnails");
        (
            RepositoryState::from_root(state_root),
            root,
            repo_root,
            thumbnail_root,
        )
    }

    fn create_repository_for_path(state: &RepositoryState, repo_root: &Path) -> String {
        install_local_filesystem_test_plugin(state);
        let response = state
            .create_repository(RepositoryMutationRequest {
                repo_id: None,
                name: "Test Repo".to_string(),
                path: repo_root.to_string_lossy().to_string(),
                backend_plugin_id: None,
                backend_config: None,
                skip_initial_sync: false,
            })
            .expect("repository should be created");
        response.repository.repo_id
    }

    fn install_local_filesystem_test_plugin(state: &RepositoryState) {
        state
            .ensure_initialized()
            .expect("repository state should initialize");
        install_local_filesystem_test_plugin_archive(&state.root);
    }

    fn create_repository_without_initial_sync(state: &RepositoryState, repo_root: &Path) -> String {
        let repo_id = format!(
            "repo-{}",
            slugify_repo_id("test", &repo_root.to_string_lossy())
        );
        state
            .ensure_initialized()
            .expect("repository state should initialize");
        install_local_filesystem_test_plugin_archive(&state.root);
        let repo_path = repo_root.to_string_lossy().to_string();
        let backend = RepositoryBackendRecord {
            plugin_id: LOCAL_FILESYSTEM_PLUGIN_ID.to_string(),
            config: serde_json::json!({}),
        };
        let seed = RepositorySeed {
            repo_id: &repo_id,
            name: "Test Repo",
            root_path: "",
            status: "ready",
            assets: &[],
        };
        initialize_repository_directory(&state.root, repo_root, &seed, &backend)
            .expect("repository files should be prepared");
        let registry = Connection::open(&state.registry_path).expect("registry should open");
        registry
            .execute(
                r#"
                INSERT OR REPLACE INTO repositories (
                  repo_id, name, path, backend_plugin_id, backend_config_json, status, created_at, updated_at
                )
                VALUES (?1, ?2, ?3, ?4, ?5, 'ready', ?6, ?6)
                "#,
                params![
                    &repo_id,
                    "Test Repo",
                    &repo_path,
                    LOCAL_FILESYSTEM_PLUGIN_ID,
                    "{}",
                    now_rfc3339()
                ],
            )
            .expect("repository should be registered");
        repo_id
    }

    fn create_local_repository_record_for_external_tests(
        state: &RepositoryState,
        repo_root: &Path,
    ) -> String {
        let repo_id = format!(
            "repo-{}",
            slugify_repo_id("external", &repo_root.to_string_lossy())
        );
        state
            .ensure_initialized()
            .expect("repository state should initialize");
        let runtime_plugin_root = runtime_plugins_dir(&state.root);
        write_test_plugin_archive_with_manifest(
            &runtime_plugin_root.join("local-filesystem.momoplug"),
            test_plugin_manifest_json(
                LOCAL_FILESYSTEM_PLUGIN_ID,
                "Local Filesystem",
                serde_json::json!({
                    "kind": "filesystem",
                    "category": "source",
                    "type": {
                        "layer": "source",
                        "kind": "filesystem"
                    },
                    "capabilities": ["listFiles", "readFile", "writeFile", "moveFile", "deleteFile"],
                    "runtime": "manifest-only",
                    "source": "system"
                }),
            ),
        );
        let metadata_dir = repo_root.join(REPO_META_DIR);
        fs::create_dir_all(&metadata_dir).expect("metadata dir should be created");
        let now = now_rfc3339();
        let metadata = RepositoryMetadataFile {
            repo_id: repo_id.clone(),
            name: "External Test Repo".to_string(),
            root_path: repo_root.to_string_lossy().to_string(),
            backend_plugin_id: LOCAL_FILESYSTEM_PLUGIN_ID.to_string(),
            backend_config: serde_json::json!({}),
            created_at: now.clone(),
            schema_version: REPO_SCHEMA_VERSION,
        };
        fs::write(
            metadata_dir.join(REPO_METADATA_FILE_NAME),
            serde_json::to_string_pretty(&metadata).expect("metadata should encode"),
        )
        .expect("metadata should be written");
        let connection = Connection::open(metadata_dir.join(REPO_DB_FILE_NAME))
            .expect("repository db should open");
        migrate_repository_schema(&connection).expect("repository schema should initialize");
        seed_repository_data(
            &connection,
            &RepositorySeed {
                repo_id: &repo_id,
                name: "External Test Repo",
                root_path: "",
                status: "ready",
                assets: &[],
            },
            &now,
        )
        .expect("repository data should seed");

        let registry = Connection::open(&state.registry_path).expect("registry should open");
        registry
            .execute(
                r#"
                INSERT OR REPLACE INTO repositories (
                  repo_id, name, path, backend_plugin_id, backend_config_json, status, created_at, updated_at
                )
                VALUES (?1, ?2, ?3, ?4, ?5, 'ready', ?6, ?6)
                "#,
                params![
                    &repo_id,
                    "External Test Repo",
                    repo_root.to_string_lossy().to_string(),
                    LOCAL_FILESYSTEM_PLUGIN_ID,
                    "{}",
                    now
                ],
            )
            .expect("repository should be registered");
        repo_id
    }

    fn write_test_image(path: &Path) {
        let image = image::RgbImage::from_pixel(2, 2, image::Rgb([120, 120, 120]));
        image.save(path).expect("test image should be saved");
    }

    fn serve_test_http_body(body: impl AsRef<[u8]> + Send + 'static) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("test HTTP server should bind");
        let addr = listener
            .local_addr()
            .expect("test HTTP server address should resolve");
        thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut request = [0_u8; 1024];
                let _ = stream.read(&mut request);
                let body = body.as_ref();
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.write_all(body);
            }
        });
        format!("http://{addr}/asset.txt")
    }

    fn write_test_palette_image(path: &Path) {
        let mut image = image::RgbImage::new(100, 10);
        for x in 0..100 {
            let color = if x < 60 {
                image::Rgb([210, 40, 30])
            } else if x < 85 {
                image::Rgb([40, 180, 90])
            } else {
                image::Rgb([20, 80, 200])
            };
            for y in 0..10 {
                image.put_pixel(x, y, color);
            }
        }
        image
            .save(path)
            .expect("test palette image should be saved");
    }

    fn metadata_for_asset_path(
        state: &RepositoryState,
        repo_id: &str,
        path: &str,
    ) -> BTreeMap<String, serde_json::Value> {
        let snapshot = state
            .load_snapshot(repo_id)
            .expect("snapshot should load after sync");
        let asset_id = snapshot
            .assets
            .iter()
            .find(|asset| asset.path == path)
            .expect("asset should exist")
            .asset_id
            .clone();
        state
            .load_asset_detail(repo_id, &asset_id)
            .expect("asset detail should load")
            .metadata
            .into_iter()
            .map(|entry| (entry.key, entry.value))
            .collect()
    }

    fn asset_id_for_test_path(state: &RepositoryState, repo_id: &str, path: &str) -> String {
        let snapshot = state
            .load_snapshot(repo_id)
            .expect("snapshot should load after sync");
        snapshot
            .assets
            .iter()
            .find(|asset| asset.path == path)
            .expect("asset should exist")
            .asset_id
            .clone()
    }

    fn count_files(path: &Path) -> usize {
        if !path.exists() {
            return 0;
        }
        fs::read_dir(path)
            .expect("path should be readable")
            .map(|entry| {
                let path = entry.expect("dir entry should be readable").path();
                if path.is_dir() {
                    count_files(&path)
                } else {
                    1
                }
            })
            .sum()
    }

    #[test]
    fn backend_discovered_file_falls_back_to_repository_path_when_absolute_path_is_missing() {
        let workspace = TestWorkspace::new("backend-discovered-file-compat");
        let repo_root = workspace.path("repo");
        fs::create_dir_all(repo_root.join("notes")).expect("repo directory should be created");

        let raw = serde_json::json!({
            "relativePath": "notes/today.txt",
            "filename": "today.txt",
            "extension": "txt",
            "sizeBytes": 12,
            "modifiedAt": "2026-06-09T00:00:00Z"
        });
        let file = serde_json::from_value::<BackendDiscoveredFile>(raw)
            .expect("legacy plugin file payload should decode")
            .into_discovered_file(&repo_root)
            .expect("legacy plugin file payload should normalize");

        assert_eq!(file.relative_path, "notes/today.txt");
        assert_eq!(
            file.absolute_path,
            Some(repo_root.join("notes").join("today.txt"))
        );
    }

    #[test]
    fn filesystem_entry_decodes_legacy_payload_without_virtual_fields() {
        let raw = serde_json::json!({
            "path": "Albums",
            "name": "Albums",
            "kind": "directory",
            "modifiedAt": "2026-06-09T00:00:00Z"
        });
        let entry = serde_json::from_value::<FileSystemEntry>(raw)
            .expect("legacy filesystem entry payload should decode");

        assert!(matches!(entry.kind, FileSystemEntryKind::Directory));
        assert!(!entry.is_virtual);
        assert_eq!(entry.provider_id, None);
        assert_eq!(entry.provider_item_id, None);
        assert_eq!(entry.source_payload, None);
        assert_eq!(entry.local_absolute_path, None);
    }

    #[test]
    fn sync_repository_indexes_assets_without_generating_thumbnails() {
        let (state, root, repo_root, thumbnail_root) = create_test_state("sync-no-thumb");
        write_test_image(&repo_root.join("cover.png"));

        let repo_id = create_repository_for_path(&state, &repo_root);
        let snapshot = state
            .load_snapshot(&repo_id)
            .expect("snapshot should load after sync");

        assert_eq!(snapshot.assets.len(), 1);
        assert_eq!(snapshot.assets[0].thumbnail_path, None);
        assert_eq!(count_files(&thumbnail_root), 0);

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn load_file_browser_returns_generic_file_metadata() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("browser-metadata");
        fs::write(repo_root.join("note.txt"), "plain text").expect("test file should be written");
        let repo_id = create_repository_for_path(&state, &repo_root);

        let snapshot = state
            .load_file_browser(FileBrowserRequest {
                repo_id,
                directory_path: Some(String::new()),
                include_tree: Some(false),
                special_location: None,
            })
            .expect("file browser should load");
        let entry = snapshot
            .entries
            .iter()
            .find(|item| item.path == "note.txt")
            .expect("file entry should be listed");

        assert_eq!(entry.metadata.get("rating"), Some(&serde_json::json!(0)));
        assert_eq!(entry.metadata.get("comment"), Some(&serde_json::json!("")));
        assert_eq!(entry.metadata.get("link"), Some(&serde_json::json!("")));
        assert_eq!(
            entry.metadata.get("tagGroups"),
            Some(&serde_json::json!([]))
        );
        assert!(entry
            .metadata
            .get("addedToLibraryAt")
            .and_then(serde_json::Value::as_str)
            .is_some());

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn plugin_metadata_defaults_preserve_existing_values() {
        let (state, root, repo_root, _thumbnail_root) =
            create_test_state("plugin-metadata-defaults");
        fs::write(repo_root.join("note.txt"), "hello").expect("test file should be written");

        let repo_id = create_repository_for_path(&state, &repo_root);
        let repo = state
            .load_repository_record(&repo_id)
            .expect("repository record should load");
        let mut connection = state
            .open_repository_connection(
                &repo.summary.repo_id,
                &repo.summary.path,
                &repo.backend_record,
            )
            .expect("repository connection should open");
        let tx = connection
            .transaction()
            .expect("metadata transaction should start");
        let asset_id = tx
            .query_row(
                "SELECT asset_id FROM assets WHERE repo_id = ?1 AND path = 'note.txt'",
                [&repo_id],
                |row| row.get::<_, String>(0),
            )
            .expect("asset should exist");
        upsert_metadata_value(&tx, &asset_id, "title", &serde_json::json!("User Title"))
            .expect("existing title should update");
        let plugin_defaults = BTreeMap::from([
            ("title".to_string(), serde_json::json!("Plugin Title")),
            (
                "pluginDefault".to_string(),
                serde_json::json!("Plugin Value"),
            ),
        ]);
        ensure_default_metadata(
            &tx,
            &asset_id,
            "note.txt",
            "note.txt",
            "txt",
            &now_rfc3339(),
            None,
            &[],
            Some(&plugin_defaults),
            false,
        )
        .expect("plugin defaults should merge");
        tx.commit().expect("metadata transaction should commit");

        let metadata = metadata_for_asset_path(&state, &repo_id, "note.txt");
        assert_eq!(
            metadata.get("title"),
            Some(&serde_json::json!("User Title"))
        );
        assert_eq!(
            metadata.get("pluginDefault"),
            Some(&serde_json::json!("Plugin Value"))
        );
        drop(connection);

        fs::write(repo_root.join("second.txt"), "second")
            .expect("second test file should be written");
        state
            .sync_repository(SyncRequest {
                repo_id: repo_id.clone(),
            })
            .expect("repository should resync");
        let repo = state
            .load_repository_record(&repo_id)
            .expect("repository record should reload");
        let mut second_connection = state
            .open_repository_connection(
                &repo.summary.repo_id,
                &repo.summary.path,
                &repo.backend_record,
            )
            .expect("repository connection should reopen");
        let tx = second_connection
            .transaction()
            .expect("second metadata transaction should start");
        let second_asset_id = tx
            .query_row(
                "SELECT asset_id FROM assets WHERE repo_id = ?1 AND path = 'second.txt'",
                [&repo_id],
                |row| row.get::<_, String>(0),
            )
            .expect("second asset should exist");
        ensure_default_metadata(
            &tx,
            &second_asset_id,
            "second.txt",
            "second.txt",
            "txt",
            &now_rfc3339(),
            None,
            &[],
            Some(&plugin_defaults),
            true,
        )
        .expect("new asset plugin defaults should merge without replacing host defaults");
        tx.commit()
            .expect("second metadata transaction should commit");
        let second_metadata = metadata_for_asset_path(&state, &repo_id, "second.txt");
        assert_eq!(
            second_metadata.get("title"),
            Some(&serde_json::json!("second.txt"))
        );
        assert_eq!(
            second_metadata.get("pluginDefault"),
            Some(&serde_json::json!("Plugin Value"))
        );

        drop(second_connection);
        drop(state);
        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn sync_repository_extracts_palette_metadata_for_new_images() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("sync-palette");
        write_test_palette_image(&repo_root.join("cover.png"));

        let repo_id = create_repository_for_path(&state, &repo_root);
        let metadata = metadata_for_asset_path(&state, &repo_id, "cover.png");

        assert_eq!(
            metadata.get("color"),
            Some(&serde_json::Value::String("#D2281E".to_string()))
        );
        assert_eq!(
            metadata.get("palette"),
            Some(&serde_json::json!(["#D2281E", "#28B45A", "#1450C8"]))
        );

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn sync_repository_skips_palette_metadata_for_non_images() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("sync-no-palette");
        fs::write(repo_root.join("note.txt"), "plain text").expect("text file should be written");

        let repo_id = create_repository_for_path(&state, &repo_root);
        let metadata = metadata_for_asset_path(&state, &repo_id, "note.txt");

        assert_eq!(metadata.get("color"), None);
        assert_eq!(metadata.get("palette"), None);

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn sync_repository_ignores_broken_images_when_extracting_palette() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("sync-broken-palette");
        fs::write(repo_root.join("broken.png"), b"not an image")
            .expect("broken image should be written");

        let repo_id = create_repository_for_path(&state, &repo_root);
        let metadata = metadata_for_asset_path(&state, &repo_id, "broken.png");

        assert_eq!(metadata.get("color"), None);
        assert_eq!(metadata.get("palette"), None);

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn sync_repository_generates_unique_asset_ids_for_slug_collisions() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("sync-asset-id");
        fs::create_dir_all(repo_root.join("A B")).expect("spaced directory should be created");
        fs::create_dir_all(repo_root.join("A-B")).expect("hyphen directory should be created");
        fs::write(repo_root.join("A B").join("cover.png"), "first")
            .expect("first file should be written");
        fs::write(repo_root.join("A-B").join("cover.png"), "second")
            .expect("second file should be written");

        let repo_id = create_repository_for_path(&state, &repo_root);
        let snapshot = state
            .load_snapshot(&repo_id)
            .expect("snapshot should load after sync");
        let asset_ids = snapshot
            .assets
            .iter()
            .map(|asset| asset.asset_id.as_str())
            .collect::<std::collections::BTreeSet<_>>();

        assert_eq!(snapshot.assets.len(), 2);
        assert_eq!(asset_ids.len(), 2);
        assert!(asset_ids
            .iter()
            .all(|asset_id| asset_id.starts_with("asset-") && asset_id.len() == 70));

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn sync_repository_stores_real_content_hash_and_candidates() {
        let (state, root, repo_root, _thumbnail_root) =
            create_test_state("sync-hardlink-candidate");
        fs::write(repo_root.join("source.txt"), b"same bytes")
            .expect("source file should be written");
        let repo_id = create_repository_for_path(&state, &repo_root);
        fs::write(repo_root.join("copy.txt"), b"same bytes").expect("copy file should be written");

        let result = state
            .sync_repository(SyncRequest {
                repo_id: repo_id.clone(),
            })
            .expect("sync should complete");
        assert_eq!(result.hardlink_candidates, 1);

        let connection = state
            .open_repository_connection(
                &repo_id,
                &repo_root.to_string_lossy(),
                &RepositoryBackendRecord {
                    plugin_id: LOCAL_FILESYSTEM_PLUGIN_ID.to_string(),
                    config: serde_json::json!({}),
                },
            )
            .expect("repository connection should open");
        let hash: String = connection
            .query_row(
                "SELECT hash FROM assets WHERE repo_id = ?1 AND path = 'source.txt'",
                [repo_id.as_str()],
                |row| row.get(0),
            )
            .expect("hash should load");
        assert!(is_content_hash(&hash));

        let candidates = state
            .list_hardlink_candidates(&repo_id)
            .expect("candidates should load");
        assert_eq!(candidates.candidates.len(), 1);

        drop(connection);
        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn search_assets_filters_current_repository_metadata_and_formats() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("search-filters");
        fs::write(repo_root.join("cover.psd"), b"cover").expect("cover file should be written");
        fs::write(repo_root.join("alt.psd"), b"alternate").expect("alt file should be written");
        fs::write(repo_root.join("icon.png"), b"icon").expect("icon file should be written");
        fs::write(repo_root.join("deleted.psd"), b"deleted")
            .expect("deleted file should be written");
        let repo_id = create_repository_for_path(&state, &repo_root);

        let repo = state
            .load_repository_record(&repo_id)
            .expect("repository record should load");
        let connection = state
            .open_repository_connection(
                &repo.summary.repo_id,
                &repo.summary.path,
                &repo.backend_record,
            )
            .expect("repository connection should open");
        let now = now_rfc3339();
        let cover_asset_id: String = connection
            .query_row(
                "SELECT asset_id FROM assets WHERE repo_id = ?1 AND path = 'cover.psd'",
                [repo_id.as_str()],
                |row| row.get(0),
            )
            .expect("cover asset id should load");
        let alt_asset_id: String = connection
            .query_row(
                "SELECT asset_id FROM assets WHERE repo_id = ?1 AND path = 'alt.psd'",
                [repo_id.as_str()],
                |row| row.get(0),
            )
            .expect("alt asset id should load");
        let deleted_asset_id: String = connection
            .query_row(
                "SELECT asset_id FROM assets WHERE repo_id = ?1 AND path = 'deleted.psd'",
                [repo_id.as_str()],
                |row| row.get(0),
            )
            .expect("deleted asset id should load");

        for (asset_id, key, value) in [
            (&cover_asset_id, "color", serde_json::json!("红色")),
            (&cover_asset_id, "shape", serde_json::json!("方形")),
            (&cover_asset_id, "rating", serde_json::json!(5)),
            (&alt_asset_id, "color", serde_json::json!("蓝色")),
            (&alt_asset_id, "shape", serde_json::json!("圆形")),
            (&alt_asset_id, "rating", serde_json::json!(2)),
            (&deleted_asset_id, "color", serde_json::json!("红色")),
            (&deleted_asset_id, "shape", serde_json::json!("方形")),
            (&deleted_asset_id, "rating", serde_json::json!(5)),
        ] {
            connection
                .execute(
                    r#"
                    INSERT OR REPLACE INTO metadata (asset_id, key, value_type, value_json, version, updated_at)
                    VALUES (?1, ?2, ?3, ?4, 1, ?5)
                    "#,
                    params![asset_id, key, infer_value_type(&value), value.to_string(), now],
                )
                .expect("metadata should be written");
        }
        for (asset_id, tag) in [(&cover_asset_id, "封面"), (&alt_asset_id, "草稿")] {
            connection
                .execute(
                    r#"
                    INSERT OR REPLACE INTO tags (asset_id, tag, normalized_tag)
                    VALUES (?1, ?2, ?3)
                    "#,
                    params![asset_id, tag, tag.to_lowercase()],
                )
                .expect("tag should be written");
        }
        connection
            .execute(
                "UPDATE assets SET status = 'deleted' WHERE repo_id = ?1 AND path = 'deleted.psd'",
                [repo_id.as_str()],
            )
            .expect("deleted asset should be marked");
        drop(connection);

        let response = state
            .search_assets(SearchRequest {
                query: String::new(),
                repo_id: Some(repo_id.clone()),
                exclude_query: None,
                metadata_key: None,
                metadata_value: None,
                tag: None,
                tags: Some(vec!["封面".to_string()]),
                metadata_filters: Some(vec![
                    SearchMetadataFilter {
                        key: "color".to_string(),
                        value: "红色".to_string(),
                    },
                    SearchMetadataFilter {
                        key: "shape".to_string(),
                        value: "方形".to_string(),
                    },
                ]),
                formats: Some(vec!["psd".to_string(), "jpg".to_string()]),
                min_rating: Some(4.0),
                exclude_tags: None,
                exclude_formats: None,
                exclude_metadata_filters: None,
                exclude_path_prefixes: None,
                exclude_number_filters: None,
                exclude_date_filters: None,
                number_filters: None,
                date_filters: None,
                match_mode: None,
                sort: None,
                limit: None,
            })
            .expect("filtered search should complete");

        assert_eq!(response.results.len(), 1);
        assert_eq!(response.results[0].path, "cover.psd");

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn search_assets_preserves_virtual_entry_markers() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("search-virtual");
        fs::write(repo_root.join("virtual-track.mp3"), b"track")
            .expect("virtual track file should be written");
        let repo_id = create_repository_for_path(&state, &repo_root);

        let repo = state
            .load_repository_record(&repo_id)
            .expect("repository record should load");
        let connection = state
            .open_repository_connection(
                &repo.summary.repo_id,
                &repo.summary.path,
                &repo.backend_record,
            )
            .expect("repository connection should open");
        connection
            .execute(
                r#"
                UPDATE assets
                SET is_virtual = 1,
                    provider_id = ?2,
                    provider_item_id = ?3,
                    source_payload_json = ?4,
                    local_absolute_path = ?5
                WHERE repo_id = ?1 AND path = 'virtual-track.mp3'
                "#,
                params![
                    repo_id,
                    "netease-cloud-music",
                    "123456",
                    serde_json::json!({
                        "provider": "netease-cloud-music",
                        "songId": 123456,
                        "playlistId": 42,
                    })
                    .to_string(),
                    Option::<String>::None,
                ],
            )
            .expect("asset should be marked virtual");
        drop(connection);

        let response = state
            .search_assets(SearchRequest {
                query: "virtual-track".to_string(),
                repo_id: Some(repo_id.clone()),
                exclude_query: None,
                metadata_key: None,
                metadata_value: None,
                tag: None,
                tags: None,
                metadata_filters: None,
                formats: Some(vec!["mp3".to_string()]),
                min_rating: None,
                exclude_tags: None,
                exclude_formats: None,
                exclude_metadata_filters: None,
                exclude_path_prefixes: None,
                exclude_number_filters: None,
                exclude_date_filters: None,
                number_filters: None,
                date_filters: None,
                match_mode: None,
                sort: None,
                limit: None,
            })
            .expect("virtual search should complete");

        assert_eq!(response.results.len(), 1);
        assert_eq!(response.results[0].path, "virtual-track.mp3");
        assert!(response.results[0].is_virtual);
        assert_eq!(
            response.results[0].provider_id.as_deref(),
            Some("netease-cloud-music")
        );
        assert_eq!(
            response.results[0].provider_item_id.as_deref(),
            Some("123456")
        );
        assert_eq!(
            response.results[0]
                .source_payload
                .as_ref()
                .and_then(|value| value.get("songId"))
                .and_then(serde_json::Value::as_i64),
            Some(123456)
        );
        assert_eq!(response.results[0].local_absolute_path, None);

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn repository_actions_list_run_and_reject_unsafe_states() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("repository-actions");
        fs::write(repo_root.join("cover.png"), b"cover").expect("cover file should be written");
        let repo_id = create_repository_for_path(&state, &repo_root);
        let asset_id = asset_id_for_test_path(&state, &repo_id, "cover.png");

        let repo = state
            .load_repository_record(&repo_id)
            .expect("repository record should load");
        let connection = state
            .open_repository_connection(
                &repo.summary.repo_id,
                &repo.summary.path,
                &repo.backend_record,
            )
            .expect("repository connection should open");
        let now = now_rfc3339();
        connection
            .execute(
                r#"
                INSERT INTO repository_actions (
                  action_id, repo_id, source, source_action_id, name, status, enabled,
                  raw_json, unsupported_reason, sort_order, created_at, updated_at
                )
                VALUES
                  ('action-ready', ?1, 'eagle-importer', 'source-ready', '标记精选', 'ready', 1, '{}', NULL, 0, ?2, ?2),
                  ('action-disabled', ?1, 'eagle-importer', 'source-disabled', '停用动作', 'ready', 0, '{}', NULL, 1, ?2, ?2),
                  ('action-unsupported', ?1, 'eagle-importer', 'source-unsupported', '未知动作', 'unsupported', 0, '{}', 'unsupported action step: shell', 2, ?2, ?2)
                "#,
                params![repo_id, now],
            )
            .expect("actions should be inserted");
        connection
            .execute(
                r#"
                INSERT INTO repository_action_steps (
                  step_id, action_id, repo_id, step_kind, label, status,
                  config_json, raw_json, unsupported_reason, sort_order
                )
                VALUES
                  ('step-ready-1', 'action-ready', ?1, 'metadata.update', '更新评分', 'ready', '{"metadata":{"rating":5,"comment":"Action run"}}', '{"type":"rating"}', NULL, 0),
                  ('step-ready-2', 'action-ready', ?1, 'tagGroups.set', '设置标签', 'ready', '{"tags":["精选"]}', '{"type":"tags"}', NULL, 1),
                  ('step-disabled-1', 'action-disabled', ?1, 'metadata.update', '更新评分', 'ready', '{"metadata":{"rating":4}}', '{"type":"rating"}', NULL, 0),
                  ('step-unsupported-1', 'action-unsupported', ?1, 'unsupported', '外部脚本', 'unsupported', '{}', '{"type":"shell"}', 'unsupported action step: shell', 0)
                "#,
                [repo_id.as_str()],
            )
            .expect("action steps should be inserted");
        drop(connection);

        let actions = state
            .list_repository_actions(&repo_id)
            .expect("actions should list");
        assert_eq!(
            actions
                .iter()
                .map(|action| action.name.as_str())
                .collect::<Vec<_>>(),
            vec!["标记精选", "停用动作", "未知动作"]
        );
        assert_eq!(actions[0].steps.len(), 2);

        let disabled_error = state
            .run_repository_action(RepositoryActionRunRequest {
                repo_id: repo_id.clone(),
                action_id: "action-disabled".to_string(),
                target_paths: Some(vec!["cover.png".to_string()]),
                asset_ids: None,
            })
            .expect_err("disabled action should be rejected");
        assert!(disabled_error.contains("disabled"));

        let unsupported_error = state
            .set_repository_action_enabled(RepositoryActionEnabledRequest {
                repo_id: repo_id.clone(),
                action_id: "action-unsupported".to_string(),
                enabled: true,
            })
            .expect_err("unsupported action cannot be enabled");
        assert!(unsupported_error.contains("unsupported"));

        let missing_target_error = state
            .run_repository_action(RepositoryActionRunRequest {
                repo_id: repo_id.clone(),
                action_id: "action-ready".to_string(),
                target_paths: None,
                asset_ids: None,
            })
            .expect_err("targetless action should be rejected");
        assert!(missing_target_error.contains("at least one target"));

        let response = state
            .run_repository_action(RepositoryActionRunRequest {
                repo_id: repo_id.clone(),
                action_id: "action-ready".to_string(),
                target_paths: Some(vec!["cover.png".to_string()]),
                asset_ids: None,
            })
            .expect("ready action should run");
        assert_eq!(response.run.status, "success");
        assert_eq!(
            response
                .action
                .last_run
                .as_ref()
                .map(|run| run.status.as_str()),
            Some("success")
        );

        let detail = state
            .load_asset_detail(&repo_id, &asset_id)
            .expect("asset detail should load after action");
        let metadata = detail
            .metadata
            .iter()
            .map(|entry| (entry.key.as_str(), entry.value.clone()))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(metadata.get("rating"), Some(&serde_json::json!(5)));
        assert_eq!(
            metadata.get("comment"),
            Some(&serde_json::json!("Action run"))
        );
        assert_eq!(
            metadata.get("tagGroups"),
            Some(&serde_json::json!(["精选"]))
        );
        assert!(detail
            .revisions
            .iter()
            .any(|revision| revision.source == "repository-action:action-ready"));

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn search_and_smart_folders_apply_exclude_filters_after_include_filters() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("search-excludes");
        fs::create_dir_all(repo_root.join("Archive")).expect("archive directory should be created");
        fs::write(repo_root.join("hero.png"), b"hero").expect("hero file should be written");
        fs::write(repo_root.join("draft.png"), b"draft").expect("draft file should be written");
        fs::write(repo_root.join("Archive/old.png"), b"old")
            .expect("archived file should be written");
        let repo_id = create_repository_for_path(&state, &repo_root);

        let repo = state
            .load_repository_record(&repo_id)
            .expect("repository record should load");
        let connection = state
            .open_repository_connection(
                &repo.summary.repo_id,
                &repo.summary.path,
                &repo.backend_record,
            )
            .expect("repository connection should open");
        let now = now_rfc3339();
        let ids = ["hero.png", "draft.png", "Archive/old.png"]
            .into_iter()
            .map(|path| {
                let asset_id: String = connection
                    .query_row(
                        "SELECT asset_id FROM assets WHERE repo_id = ?1 AND path = ?2",
                        params![repo_id.as_str(), path],
                        |row| row.get(0),
                    )
                    .expect("asset id should load");
                (path.to_string(), asset_id)
            })
            .collect::<BTreeMap<_, _>>();
        for (path, width, created_at, note) in [
            (
                "hero.png",
                serde_json::json!(1920),
                serde_json::json!("2024-02-02T00:00:00Z"),
                serde_json::json!("final hero"),
            ),
            (
                "draft.png",
                serde_json::json!(480),
                serde_json::json!("2024-02-02T00:00:00Z"),
                serde_json::json!("draft hero"),
            ),
            (
                "Archive/old.png",
                serde_json::json!(1920),
                serde_json::json!("2024-01-10T00:00:00Z"),
                serde_json::json!("old hero"),
            ),
        ] {
            for (key, value) in [
                ("width", width),
                ("fileCreatedAt", created_at),
                ("note", note),
            ] {
                connection
                    .execute(
                        r#"
                        INSERT OR REPLACE INTO metadata (asset_id, key, value_type, value_json, version, updated_at)
                        VALUES (?1, ?2, ?3, ?4, 1, ?5)
                        "#,
                        params![
                            ids[path].as_str(),
                            key,
                            infer_value_type(&value),
                            value.to_string(),
                            now
                        ],
                    )
                    .expect("metadata should be written");
            }
        }
        drop(connection);

        let response = state
            .search_assets(SearchRequest {
                query: "hero".to_string(),
                repo_id: Some(repo_id.clone()),
                exclude_query: Some("draft".to_string()),
                metadata_key: None,
                metadata_value: None,
                tag: None,
                tags: None,
                metadata_filters: None,
                exclude_tags: None,
                exclude_formats: None,
                exclude_metadata_filters: None,
                exclude_path_prefixes: Some(vec!["Archive".to_string()]),
                exclude_number_filters: Some(vec![SearchNumberFilter {
                    key: "width".to_string(),
                    min: None,
                    max: Some(640.0),
                }]),
                exclude_date_filters: Some(vec![SearchDateFilter {
                    key: "fileCreatedAt".to_string(),
                    from: Some("2024-01-01T00:00:00Z".to_string()),
                    to: Some("2024-01-31T00:00:00Z".to_string()),
                }]),
                number_filters: None,
                date_filters: None,
                formats: Some(vec!["png".to_string()]),
                min_rating: None,
                match_mode: None,
                sort: None,
                limit: None,
            })
            .expect("exclude search should complete");
        assert_eq!(
            response
                .results
                .iter()
                .map(|item| item.path.as_str())
                .collect::<Vec<_>>(),
            vec!["hero.png"]
        );

        state
            .create_smart_folder(SmartFolderMutationRequest {
                repo_id: repo_id.clone(),
                smart_folder_id: Some("smart-hero".to_string()),
                parent_id: None,
                name: "Hero".to_string(),
                filter: SmartFolderFilter {
                    query: Some("hero".to_string()),
                    formats: Some(vec!["png".to_string()]),
                    exclude_query: Some("draft".to_string()),
                    exclude_path_prefixes: Some(vec!["Archive".to_string()]),
                    exclude_number_filters: Some(vec![SearchNumberFilter {
                        key: "width".to_string(),
                        min: None,
                        max: Some(640.0),
                    }]),
                    exclude_date_filters: Some(vec![SearchDateFilter {
                        key: "fileCreatedAt".to_string(),
                        from: Some("2024-01-01T00:00:00Z".to_string()),
                        to: Some("2024-01-31T00:00:00Z".to_string()),
                    }]),
                    ..SmartFolderFilter::default()
                },
            })
            .expect("smart folder should be created");
        let smart_result = state
            .query_smart_folder(&repo_id, "smart-hero")
            .expect("smart folder should query");
        assert_eq!(
            smart_result
                .results
                .iter()
                .map(|item| item.path.as_str())
                .collect::<Vec<_>>(),
            vec!["hero.png"]
        );

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn plugin_manifest_infers_category_for_legacy_kind() {
        let manifest = parse_plugin_manifest(
            r#"{
              "pluginId": "user.legacy-webdav",
              "legacyPluginIds": [],
              "name": "Legacy WebDAV",
              "version": "0.1.0",
              "kind": "webdav",
              "description": "Legacy source plugin.",
              "capabilities": ["browse"],
              "enabled": true,
              "sdk": "backend",
              "entry": {},
              "source": "user",
              "runtime": "manifest-only",
              "permissions": [],
              "compat": {
                "sdkVersion": "1",
                "legacyPluginIds": []
              },
              "status": "ready"
            }"#,
        )
        .expect("legacy manifest should parse");

        assert_eq!(manifest.category, "source");
        assert!(is_repository_backend_plugin(&manifest));
        assert!(manifest.requires.is_empty());
        assert!(manifest.optional.is_empty());
        assert!(manifest.hooks.is_empty());
        assert!(manifest.contributes.is_object());
    }

    #[test]
    fn search_assets_match_mode_or_spans_filter_families_and_metadata_sort_is_typed() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("search-or-sort");
        fs::write(repo_root.join("tag-only.png"), b"tag").expect("tag-only file should be written");
        fs::write(repo_root.join("metadata-only.png"), b"metadata")
            .expect("metadata-only file should be written");
        fs::write(repo_root.join("small.png"), b"small").expect("small file should be written");
        fs::write(repo_root.join("large.png"), b"large").expect("large file should be written");
        let repo_id = create_repository_for_path(&state, &repo_root);

        let repo = state
            .load_repository_record(&repo_id)
            .expect("repository record should load");
        let connection = state
            .open_repository_connection(
                &repo.summary.repo_id,
                &repo.summary.path,
                &repo.backend_record,
            )
            .expect("repository connection should open");
        let now = now_rfc3339();
        let ids = [
            "tag-only.png",
            "metadata-only.png",
            "small.png",
            "large.png",
        ]
        .into_iter()
        .map(|path| {
            let asset_id: String = connection
                .query_row(
                    "SELECT asset_id FROM assets WHERE repo_id = ?1 AND path = ?2",
                    params![repo_id.as_str(), path],
                    |row| row.get(0),
                )
                .expect("asset id should load");
            (path.to_string(), asset_id)
        })
        .collect::<BTreeMap<_, _>>();

        connection
            .execute(
                "INSERT OR REPLACE INTO tags (asset_id, tag, normalized_tag) VALUES (?1, ?2, ?3)",
                params![ids["tag-only.png"].as_str(), "Poster", "poster"],
            )
            .expect("tag should be written");
        for (path, width, created_at) in [
            (
                "metadata-only.png",
                serde_json::json!(1920),
                serde_json::json!("2024-01-04T00:00:00Z"),
            ),
            (
                "small.png",
                serde_json::json!(800),
                serde_json::json!("2024-01-01T00:00:00Z"),
            ),
            (
                "large.png",
                serde_json::json!(1920),
                serde_json::json!("2024-01-03T00:00:00Z"),
            ),
        ] {
            for (key, value) in [("width", width), ("fileCreatedAt", created_at)] {
                connection
                    .execute(
                        r#"
                        INSERT OR REPLACE INTO metadata (asset_id, key, value_type, value_json, version, updated_at)
                        VALUES (?1, ?2, ?3, ?4, 1, ?5)
                        "#,
                        params![
                            ids[path].as_str(),
                            key,
                            infer_value_type(&value),
                            value.to_string(),
                            now
                        ],
                    )
                    .expect("metadata should be written");
            }
        }
        drop(connection);

        let or_response = state
            .search_assets(SearchRequest {
                query: String::new(),
                repo_id: Some(repo_id.clone()),
                exclude_query: None,
                metadata_key: None,
                metadata_value: None,
                tag: None,
                tags: Some(vec!["Poster".to_string()]),
                metadata_filters: Some(vec![SearchMetadataFilter {
                    key: "width".to_string(),
                    value: "1920".to_string(),
                }]),
                exclude_tags: None,
                exclude_formats: None,
                exclude_metadata_filters: None,
                exclude_path_prefixes: None,
                exclude_number_filters: None,
                exclude_date_filters: None,
                number_filters: None,
                date_filters: None,
                formats: None,
                min_rating: None,
                match_mode: Some("or".to_string()),
                sort: Some(SearchSort {
                    field: "metadata.fileCreatedAt".to_string(),
                    direction: "asc".to_string(),
                }),
                limit: None,
            })
            .expect("or search should complete");

        let or_paths = or_response
            .results
            .iter()
            .map(|item| item.path.as_str())
            .collect::<Vec<_>>();
        assert_eq!(or_paths.len(), 3);
        assert!(or_paths.contains(&"tag-only.png"));
        assert!(or_paths.contains(&"metadata-only.png"));
        assert!(or_paths.contains(&"large.png"));

        let width_sorted = state
            .search_assets(SearchRequest {
                query: String::new(),
                repo_id: Some(repo_id.clone()),
                exclude_query: None,
                metadata_key: None,
                metadata_value: None,
                tag: None,
                tags: None,
                metadata_filters: None,
                exclude_tags: None,
                exclude_formats: None,
                exclude_metadata_filters: None,
                exclude_path_prefixes: None,
                exclude_number_filters: None,
                exclude_date_filters: None,
                number_filters: None,
                date_filters: None,
                formats: Some(vec!["png".to_string()]),
                min_rating: None,
                match_mode: None,
                sort: Some(SearchSort {
                    field: "metadata.width".to_string(),
                    direction: "asc".to_string(),
                }),
                limit: None,
            })
            .expect("metadata width sort should complete");

        let sorted_paths = width_sorted
            .results
            .iter()
            .map(|item| item.path.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            sorted_paths,
            vec![
                "small.png",
                "large.png",
                "metadata-only.png",
                "tag-only.png"
            ]
        );

        let date_sorted = state
            .search_assets(SearchRequest {
                query: String::new(),
                repo_id: Some(repo_id.clone()),
                exclude_query: None,
                metadata_key: None,
                metadata_value: None,
                tag: None,
                tags: None,
                metadata_filters: Some(vec![SearchMetadataFilter {
                    key: "width".to_string(),
                    value: "1920".to_string(),
                }]),
                exclude_tags: None,
                exclude_formats: None,
                exclude_metadata_filters: None,
                exclude_path_prefixes: None,
                exclude_number_filters: None,
                exclude_date_filters: None,
                number_filters: None,
                date_filters: None,
                formats: None,
                min_rating: None,
                match_mode: None,
                sort: Some(SearchSort {
                    field: "metadata.fileCreatedAt".to_string(),
                    direction: "asc".to_string(),
                }),
                limit: None,
            })
            .expect("metadata date sort should complete");
        let date_paths = date_sorted
            .results
            .iter()
            .map(|item| item.path.as_str())
            .collect::<Vec<_>>();
        assert_eq!(date_paths, vec!["large.png", "metadata-only.png"]);

        let random_sorted = state
            .search_assets(SearchRequest {
                query: String::new(),
                repo_id: Some(repo_id),
                exclude_query: None,
                metadata_key: None,
                metadata_value: None,
                tag: None,
                tags: None,
                metadata_filters: None,
                exclude_tags: None,
                exclude_formats: None,
                exclude_metadata_filters: None,
                exclude_path_prefixes: None,
                exclude_number_filters: None,
                exclude_date_filters: None,
                number_filters: None,
                date_filters: None,
                formats: Some(vec!["png".to_string()]),
                min_rating: None,
                match_mode: None,
                sort: Some(SearchSort {
                    field: "random".to_string(),
                    direction: "asc".to_string(),
                }),
                limit: None,
            })
            .expect("core random sort should complete");
        let random_paths = random_sorted
            .results
            .iter()
            .map(|item| item.path.as_str())
            .collect::<HashSet<_>>();
        assert_eq!(
            random_paths,
            HashSet::from([
                "large.png",
                "metadata-only.png",
                "small.png",
                "tag-only.png",
            ])
        );

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn copy_entries_records_linked_hardlink_member() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("copy-hardlink");
        fs::create_dir_all(repo_root.join("Copies")).expect("copies folder should be created");
        fs::write(repo_root.join("source.txt"), b"copy me").expect("source file should be written");
        let repo_id = create_repository_for_path(&state, &repo_root);

        state
            .copy_entries(FileCopyRequest {
                repo_id: repo_id.clone(),
                source_paths: vec!["source.txt".to_string()],
                parent_path: Some("Copies".to_string()),
                mode: None,
            })
            .expect("copy should complete");

        let snapshot = state
            .load_file_browser(FileBrowserRequest {
                repo_id: repo_id.clone(),
                directory_path: Some("Copies".to_string()),
                include_tree: Some(false),
                special_location: None,
            })
            .expect("browser should load");
        let copied = snapshot
            .entries
            .iter()
            .find(|entry| entry.path == "Copies/source.txt")
            .expect("copied entry should exist");
        assert_eq!(copied.hardlink_state.as_deref(), Some("linked"));
        assert!(copied.hardlink_group_id.is_some());
        let root_snapshot = state
            .load_file_browser(FileBrowserRequest {
                repo_id: repo_id.clone(),
                directory_path: Some(String::new()),
                include_tree: Some(false),
                special_location: None,
            })
            .expect("root browser should load");
        let source = root_snapshot
            .entries
            .iter()
            .find(|entry| entry.path == "source.txt")
            .expect("source entry should exist");
        assert_eq!(source.hardlink_state.as_deref(), Some("linked"));
        assert_eq!(source.hardlink_group_id, copied.hardlink_group_id);
        let candidates = state
            .list_hardlink_candidates(&repo_id)
            .expect("candidates should load");
        assert!(candidates.candidates.is_empty());

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn copy_entries_rejects_same_directory_target() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("copy-same-directory");
        fs::write(repo_root.join("source.txt"), b"copy me").expect("source file should be written");
        let repo_id = create_repository_for_path(&state, &repo_root);

        let error = state
            .copy_entries(FileCopyRequest {
                repo_id,
                source_paths: vec!["source.txt".to_string()],
                parent_path: Some("".to_string()),
                mode: None,
            })
            .expect_err("same-directory copy should fail");
        assert!(error.contains("不能复制到原目录"));

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn move_entries_updates_filesystem_and_asset_paths() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("move-file");
        fs::create_dir_all(repo_root.join("Archive")).expect("archive folder should be created");
        fs::write(repo_root.join("note.txt"), b"move me").expect("source file should be written");
        let repo_id = create_repository_without_initial_sync(&state, &repo_root);
        state
            .sync_repository(SyncRequest {
                repo_id: repo_id.clone(),
            })
            .expect("repository should sync");

        let snapshot = state
            .move_entries(FileMoveRequest {
                repo_id: repo_id.clone(),
                source_paths: vec!["note.txt".to_string()],
                parent_path: "Archive".to_string(),
            })
            .expect("move should complete");

        assert!(!repo_root.join("note.txt").exists());
        assert!(repo_root.join("Archive/note.txt").is_file());
        assert!(snapshot
            .entries
            .iter()
            .any(|entry| entry.path == "Archive/note.txt"));

        let repository_snapshot = state
            .load_snapshot(&repo_id)
            .expect("repository snapshot should load");
        assert!(repository_snapshot
            .assets
            .iter()
            .any(|asset| asset.path == "Archive/note.txt"));

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn move_entries_reject_same_directory_target() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("move-same-directory");
        fs::write(repo_root.join("source.txt"), b"move me").expect("source file should be written");
        let repo_id = create_repository_for_path(&state, &repo_root);

        let error = state
            .move_entries(FileMoveRequest {
                repo_id,
                source_paths: vec!["source.txt".to_string()],
                parent_path: String::new(),
            })
            .expect_err("same-directory move should fail");
        assert!(error.contains("不能移动到原目录"));

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn move_entries_reject_folder_cycle_nesting() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("move-folder-cycle");
        fs::create_dir_all(repo_root.join("Scenes/Act1")).expect("nested folder should be created");
        fs::write(repo_root.join("Scenes/Act1/shot.txt"), b"scene")
            .expect("nested file should be written");
        let repo_id = create_repository_without_initial_sync(&state, &repo_root);
        state
            .sync_repository(SyncRequest {
                repo_id: repo_id.clone(),
            })
            .expect("repository should sync");

        let error = state
            .move_entries(FileMoveRequest {
                repo_id,
                source_paths: vec!["Scenes".to_string()],
                parent_path: "Scenes/Act1".to_string(),
            })
            .expect_err("cyclic folder move should fail");
        assert!(error.contains("文件夹不能移动到自身或其子文件夹内"));

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn copy_entries_copy_mode_records_fallback_without_candidate() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("copy-fallback");
        fs::create_dir_all(repo_root.join("Copies")).expect("copies folder should be created");
        fs::write(repo_root.join("source.txt"), b"copy me").expect("source file should be written");
        let repo_id = create_repository_for_path(&state, &repo_root);

        state
            .copy_entries(FileCopyRequest {
                repo_id: repo_id.clone(),
                source_paths: vec!["source.txt".to_string()],
                parent_path: Some("Copies".to_string()),
                mode: Some("copy".to_string()),
            })
            .expect("copy should complete");

        let snapshot = state
            .load_file_browser(FileBrowserRequest {
                repo_id: repo_id.clone(),
                directory_path: Some("Copies".to_string()),
                include_tree: Some(false),
                special_location: None,
            })
            .expect("browser should load");
        let copied = snapshot
            .entries
            .iter()
            .find(|entry| entry.path == "Copies/source.txt")
            .expect("copied entry should exist");
        assert_eq!(copied.hardlink_state.as_deref(), Some("copiedFallback"));
        state
            .sync_repository(SyncRequest {
                repo_id: repo_id.clone(),
            })
            .expect("sync should preserve fallback state");
        let synced_snapshot = state
            .load_file_browser(FileBrowserRequest {
                repo_id: repo_id.clone(),
                directory_path: Some("Copies".to_string()),
                include_tree: Some(false),
                special_location: None,
            })
            .expect("browser should load after sync");
        let synced_copy = synced_snapshot
            .entries
            .iter()
            .find(|entry| entry.path == "Copies/source.txt")
            .expect("copied entry should exist after sync");
        assert_eq!(
            synced_copy.hardlink_state.as_deref(),
            Some("copiedFallback")
        );
        let candidates = state
            .list_hardlink_candidates(&repo_id)
            .expect("candidates should load");
        assert!(candidates.candidates.is_empty());

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn confirm_hardlink_candidate_rejects_changed_file() {
        let (state, root, repo_root, _thumbnail_root) =
            create_test_state("confirm-hardlink-changed");
        fs::write(repo_root.join("source.txt"), b"same bytes")
            .expect("source file should be written");
        let repo_id = create_repository_for_path(&state, &repo_root);
        fs::write(repo_root.join("copy.txt"), b"same bytes").expect("copy file should be written");
        state
            .sync_repository(SyncRequest {
                repo_id: repo_id.clone(),
            })
            .expect("sync should create candidate");
        let candidate_id = state
            .list_hardlink_candidates(&repo_id)
            .expect("candidates should load")
            .candidates
            .first()
            .expect("candidate should exist")
            .candidate_id
            .clone();

        fs::write(repo_root.join("copy.txt"), b"changed bytes")
            .expect("copy file should be modified");
        let error = state
            .confirm_hardlink_candidate(HardlinkConfirmRequest {
                repo_id: repo_id.clone(),
                candidate_id,
            })
            .expect_err("changed candidate should fail");
        assert!(error.contains("no longer valid"));
        let bytes = fs::read(repo_root.join("copy.txt")).expect("copy file should still exist");
        assert_eq!(bytes, b"changed bytes");
        let candidates = state
            .list_hardlink_candidates(&repo_id)
            .expect("candidates should load after rejection");
        assert!(candidates.candidates.is_empty());

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn hardlink_candidate_list_filters_stale_records() {
        let (state, root, repo_root, _thumbnail_root) =
            create_test_state("hardlink-stale-candidate");
        fs::write(repo_root.join("source.txt"), b"same bytes")
            .expect("source file should be written");
        let repo_id = create_repository_for_path(&state, &repo_root);
        fs::write(repo_root.join("copy.txt"), b"same bytes").expect("copy file should be written");
        state
            .sync_repository(SyncRequest {
                repo_id: repo_id.clone(),
            })
            .expect("sync should create candidate");
        assert_eq!(
            state
                .list_hardlink_candidates(&repo_id)
                .expect("candidate should load")
                .candidates
                .len(),
            1
        );

        fs::write(repo_root.join("copy.txt"), b"different bytes")
            .expect("copy file should be modified");
        state
            .sync_repository(SyncRequest {
                repo_id: repo_id.clone(),
            })
            .expect("sync should mark candidate stale");
        assert!(state
            .list_hardlink_candidates(&repo_id)
            .expect("stale candidates should be filtered")
            .candidates
            .is_empty());

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn ensure_thumbnail_reuses_existing_cache_path() {
        let (state, root, repo_root, thumbnail_root) = create_test_state("thumb-reuse");
        write_test_image(&repo_root.join("cover.png"));
        let repo_id = create_repository_for_path(&state, &repo_root);
        let snapshot = state
            .load_snapshot(&repo_id)
            .expect("snapshot should load after sync");
        let asset_id = snapshot.assets[0].asset_id.clone();
        let thumbnail_dir = thumbnail_root.join(thumbnail_repository_dir_name(
            &repo_id,
            &repo_root.to_string_lossy(),
        ));
        fs::create_dir_all(&thumbnail_dir).expect("thumbnail dir should be created");
        let thumbnail_path = thumbnail_dir.join(thumbnail_file_name(
            &repo_id,
            &repo_root.to_string_lossy(),
            "cover.png",
            "file",
            "generated",
        ));
        write_test_image(&thumbnail_path);

        let storage_paths = ensure_repository_storage_paths(
            &state.root,
            &repo_id,
            &repo_root,
            LOCAL_FILESYSTEM_PLUGIN_ID,
        )
        .expect("storage paths should resolve");
        let connection =
            Connection::open(storage_paths.database_path).expect("repository db should open");
        connection
            .execute(
                "UPDATE assets SET thumbnail_path = ?3 WHERE repo_id = ?1 AND asset_id = ?2",
                params![
                    repo_id,
                    asset_id,
                    thumbnail_path.to_string_lossy().to_string()
                ],
            )
            .expect("asset thumbnail path should update");

        let response = state
            .ensure_thumbnail(ThumbnailRequest {
                repo_id,
                path: "cover.png".to_string(),
                action: None,
                source_path: None,
                source_url: None,
                image_bytes: None,
                media_type: None,
            })
            .expect("thumbnail should be ensured");

        assert_eq!(
            response.thumbnail_path,
            Some(thumbnail_path.to_string_lossy().to_string())
        );

        drop(connection);
        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn ensure_thumbnail_migrates_existing_cache_path_to_repository_metadata_dir() {
        let (state, root, repo_root, thumbnail_root) = create_test_state("thumb-migrate");
        write_test_image(&repo_root.join("cover.png"));
        let repo_id = create_repository_for_path(&state, &repo_root);
        let snapshot = state
            .load_snapshot(&repo_id)
            .expect("snapshot should load after sync");
        let asset_id = snapshot.assets[0].asset_id.clone();
        let legacy_root = root.join("legacy-thumbnails");
        let legacy_dir = legacy_root.join(thumbnail_repository_dir_name(
            &repo_id,
            &repo_root.to_string_lossy(),
        ));
        fs::create_dir_all(&legacy_dir).expect("legacy thumbnail dir should be created");
        let legacy_thumbnail_path = legacy_dir.join(thumbnail_file_name(
            &repo_id,
            &repo_root.to_string_lossy(),
            "cover.png",
            "file",
            "generated",
        ));
        write_test_image(&legacy_thumbnail_path);

        let storage_paths = ensure_repository_storage_paths(
            &state.root,
            &repo_id,
            &repo_root,
            LOCAL_FILESYSTEM_PLUGIN_ID,
        )
        .expect("storage paths should resolve");
        let connection =
            Connection::open(storage_paths.database_path).expect("repository db should open");
        connection
            .execute(
                "UPDATE assets SET thumbnail_path = ?3 WHERE repo_id = ?1 AND asset_id = ?2",
                params![
                    repo_id,
                    asset_id,
                    legacy_thumbnail_path.to_string_lossy().to_string()
                ],
            )
            .expect("asset thumbnail path should update");

        let response = state
            .ensure_thumbnail(ThumbnailRequest {
                repo_id: repo_id.clone(),
                path: "cover.png".to_string(),
                action: None,
                source_path: None,
                source_url: None,
                image_bytes: None,
                media_type: None,
            })
            .expect("thumbnail should be ensured");
        let thumbnail_path = response
            .thumbnail_path
            .as_deref()
            .map(Path::new)
            .expect("thumbnail path should be returned");

        assert!(thumbnail_path.starts_with(&thumbnail_root));
        assert!(thumbnail_path.is_file());
        assert_ne!(thumbnail_path, legacy_thumbnail_path.as_path());
        let stored_path: String = connection
            .query_row(
                "SELECT thumbnail_path FROM assets WHERE repo_id = ?1 AND asset_id = ?2",
                params![repo_id, asset_id],
                |row| row.get(0),
            )
            .expect("asset thumbnail path should load");
        assert_eq!(stored_path, thumbnail_path.to_string_lossy());

        drop(connection);
        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn ensure_thumbnail_migrates_custom_entry_cache_path_to_repository_metadata_dir() {
        let (state, root, repo_root, thumbnail_root) = create_test_state("thumb-custom-migrate");
        fs::create_dir_all(repo_root.join("Shots")).expect("directory should be created");
        let repo_id = create_repository_for_path(&state, &repo_root);
        let legacy_root = root.join("legacy-thumbnails");
        let legacy_dir = legacy_root.join(thumbnail_repository_dir_name(
            &repo_id,
            &repo_root.to_string_lossy(),
        ));
        fs::create_dir_all(&legacy_dir).expect("legacy thumbnail dir should be created");
        let legacy_thumbnail_path = legacy_dir.join(thumbnail_file_name(
            &repo_id,
            &repo_root.to_string_lossy(),
            "Shots",
            "directory",
            "custom",
        ));
        write_test_image(&legacy_thumbnail_path);

        let storage_paths = ensure_repository_storage_paths(
            &state.root,
            &repo_id,
            &repo_root,
            LOCAL_FILESYSTEM_PLUGIN_ID,
        )
        .expect("storage paths should resolve");
        let connection =
            Connection::open(storage_paths.database_path).expect("repository db should open");
        upsert_entry_thumbnail_record(
            &connection,
            &repo_id,
            "Shots",
            "directory",
            &legacy_thumbnail_path.to_string_lossy(),
            true,
        )
        .expect("entry thumbnail should be seeded");

        let response = state
            .ensure_thumbnail(ThumbnailRequest {
                repo_id: repo_id.clone(),
                path: "Shots".to_string(),
                action: None,
                source_path: None,
                source_url: None,
                image_bytes: None,
                media_type: None,
            })
            .expect("thumbnail should be ensured");
        let thumbnail_path = response
            .thumbnail_path
            .as_deref()
            .map(Path::new)
            .expect("thumbnail path should be returned");

        assert!(response.thumbnail_custom);
        assert!(thumbnail_path.starts_with(&thumbnail_root));
        assert!(thumbnail_path.is_file());
        assert_ne!(thumbnail_path, legacy_thumbnail_path.as_path());
        let stored_path: String = connection
            .query_row(
                "SELECT thumbnail_path FROM entry_thumbnails WHERE repo_id = ?1 AND path = ?2 AND kind = ?3",
                params![repo_id, "Shots", "directory"],
                |row| row.get(0),
            )
            .expect("entry thumbnail path should load");
        assert_eq!(stored_path, thumbnail_path.to_string_lossy());

        drop(connection);
        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn ensure_thumbnail_writes_cache_under_repository_metadata_dir() {
        let (state, root, repo_root, thumbnail_root) = create_test_state("thumb-repo-meta");
        write_test_image(&repo_root.join("cover.png"));
        let repo_id = create_repository_for_path(&state, &repo_root);

        let response = state
            .ensure_thumbnail(ThumbnailRequest {
                repo_id,
                path: "cover.png".to_string(),
                action: None,
                source_path: None,
                source_url: None,
                image_bytes: None,
                media_type: None,
            })
            .expect("thumbnail should be generated");
        let thumbnail_path = response
            .thumbnail_path
            .as_deref()
            .map(Path::new)
            .expect("thumbnail path should be returned");

        assert!(thumbnail_path.starts_with(&thumbnail_root));
        assert!(thumbnail_path.is_file());
        assert_eq!(count_files(&thumbnail_root), 1);

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn ensure_thumbnail_saves_remote_source_url_as_custom_thumbnail() {
        let (state, root, repo_root, thumbnail_root) = create_test_state("thumb-remote-source");
        fs::write(repo_root.join("track.mp3"), b"fake audio")
            .expect("track file should be written");
        let repo_id = create_repository_for_path(&state, &repo_root);
        let mut body = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            2,
            2,
            image::Rgb([220, 80, 40]),
        ))
        .write_to(&mut body, image::ImageFormat::Png)
        .expect("test thumbnail image should encode");
        let source_url = serve_test_http_body(body.into_inner());

        let response = state
            .ensure_thumbnail(ThumbnailRequest {
                repo_id: repo_id.clone(),
                path: "track.mp3".to_string(),
                action: Some("save".to_string()),
                source_path: None,
                source_url: Some(source_url),
                image_bytes: None,
                media_type: None,
            })
            .expect("remote thumbnail should be saved");
        let thumbnail_path = response
            .thumbnail_path
            .as_deref()
            .map(Path::new)
            .expect("thumbnail path should be returned");

        assert!(response.thumbnail_custom);
        assert!(thumbnail_path.starts_with(&thumbnail_root));
        assert!(thumbnail_path.is_file());
        assert!(response
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("thumbnailPalette"))
            .is_some());

        let snapshot = state
            .load_file_browser(FileBrowserRequest {
                repo_id,
                directory_path: Some(String::new()),
                include_tree: Some(false),
                special_location: None,
            })
            .expect("file browser should load");
        let entry = snapshot
            .entries
            .iter()
            .find(|item| item.path == "track.mp3")
            .expect("track entry should be listed");
        assert!(entry.thumbnail_custom);
        assert_eq!(
            entry.thumbnail_path.as_deref(),
            Some(thumbnail_path.to_string_lossy().as_ref())
        );

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn ensure_thumbnail_extracts_palette_metadata() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("thumb-palette");
        write_test_image(&repo_root.join("cover.png"));
        let repo_id = create_repository_for_path(&state, &repo_root);

        let response = state
            .ensure_thumbnail(ThumbnailRequest {
                repo_id: repo_id.clone(),
                path: "cover.png".to_string(),
                action: None,
                source_path: None,
                source_url: None,
                image_bytes: None,
                media_type: None,
            })
            .expect("thumbnail should be generated");
        let palette = response
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("thumbnailPalette"))
            .and_then(serde_json::Value::as_array)
            .cloned()
            .expect("thumbnail palette should be returned");

        assert!(!palette.is_empty());
        assert!(palette
            .iter()
            .all(|item| item.as_str().is_some_and(|value| value.starts_with('#'))));

        let snapshot = state
            .load_file_browser(FileBrowserRequest {
                repo_id,
                directory_path: Some(String::new()),
                include_tree: Some(false),
                special_location: None,
            })
            .expect("file browser should load");
        let entry = snapshot
            .entries
            .iter()
            .find(|item| item.path == "cover.png")
            .expect("cover entry should be listed");
        assert_eq!(
            entry.metadata.get("thumbnailPalette"),
            Some(&serde_json::Value::Array(palette))
        );

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn ensure_thumbnail_returns_null_for_unsupported_types() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("thumb-unsupported");
        fs::write(repo_root.join("note.txt"), "plain text").expect("text file should be written");
        let repo_id = create_repository_for_path(&state, &repo_root);

        let response = state
            .ensure_thumbnail(ThumbnailRequest {
                repo_id,
                path: "note.txt".to_string(),
                action: None,
                source_path: None,
                source_url: None,
                image_bytes: None,
                media_type: None,
            })
            .expect("unsupported thumbnail request should succeed");

        assert_eq!(response.thumbnail_path, None);

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn open_preview_file_source_returns_registered_file() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("preview-open");
        let source_path = repo_root.join("model.glb");
        fs::write(&source_path, b"glb").expect("preview source should be written");
        let repo_id = create_repository_for_path(&state, &repo_root);
        let response = state
            .prepare_preview_file_source(FileReadRequest {
                repo_id,
                path: "model.glb".to_string(),
            })
            .expect("preview source should be prepared");

        let (mut file, media_type) = state
            .open_preview_file_source(&response.token)
            .expect("registered preview token should open");
        let mut body = Vec::new();
        use std::io::Read;
        file.read_to_end(&mut body)
            .expect("preview file should be readable");

        assert_eq!(body, b"glb");
        assert_eq!(media_type, "model/gltf-binary");
        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn register_preview_source_path_serves_temporary_playback_files() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("preview-register-temp");
        let audio_path = repo_root.join("temp-track.mp3");
        let lyric_path = repo_root.join("temp-track.lrc");
        fs::write(&audio_path, b"audio").expect("temporary audio should be written");
        fs::write(&lyric_path, "[00:01.00]line").expect("temporary lyric should be written");

        let audio_token = state
            .register_preview_source_path(audio_path, "audio/mpeg")
            .expect("audio source should register");
        let lyric_token = state
            .register_preview_source_path(lyric_path, "text/plain; charset=utf-8")
            .expect("lyric source should register");

        let (mut audio_file, audio_media_type) = state
            .open_preview_file_source(&audio_token)
            .expect("registered audio source should open");
        let mut audio_body = Vec::new();
        use std::io::Read;
        audio_file
            .read_to_end(&mut audio_body)
            .expect("audio source should read");
        assert_eq!(audio_body, b"audio");
        assert_eq!(audio_media_type, "audio/mpeg");

        let (mut lyric_file, lyric_media_type) = state
            .open_preview_file_source(&lyric_token)
            .expect("registered lyric source should open");
        let mut lyric_text = String::new();
        lyric_file
            .read_to_string(&mut lyric_text)
            .expect("lyric source should read");
        assert_eq!(lyric_text, "[00:01.00]line");
        assert_eq!(lyric_media_type, "text/plain; charset=utf-8");

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn prepare_entry_playback_source_uses_backend_stat_for_unindexed_virtual_tracks() {
        let _lock = playback_test_lock();
        let (state, root, repo_root, _thumbnail_root) =
            create_test_state("prepare-entry-playback-unindexed-virtual");
        let repo_id = create_repository_without_initial_sync(&state, &repo_root);
        let expected_repo_id = repo_id.clone();

        fn stat_hook(
            _repo: &RepositoryRecord,
            _repo_root: &Path,
            entry_path: &str,
        ) -> Option<Result<FileSystemEntry, String>> {
            if entry_path != "Created/lazy-track.mp3" {
                return None;
            }
            Some(Ok(FileSystemEntry {
                path: entry_path.to_string(),
                name: "lazy-track.mp3".to_string(),
                kind: FileSystemEntryKind::File,
                extension: Some("mp3".to_string()),
                size_bytes: None,
                modified_at: Some("2026-06-14T00:00:00Z".to_string()),
                is_virtual: true,
                provider_id: Some("netease-cloud-music".to_string()),
                provider_item_id: Some("3001".to_string()),
                source_payload: Some(serde_json::json!({
                    "provider": "netease-cloud-music",
                    "songId": "3001",
                    "accountCookie": "MUSIC_U=lazy-cookie",
                    "accountId": "42",
                    "level": "exhigh"
                })),
                local_absolute_path: None,
            }))
        }

        fn playback_hook(payload: serde_json::Value) -> Result<serde_json::Value, String> {
            let expected_repo_id = std::env::var("MOMOBKO_TEST_EXPECTED_REPO_ID")
                .expect("expected repo id should be provided");
            assert_eq!(payload["songId"], serde_json::json!(3001));
            assert_eq!(
                payload["accountCookie"],
                serde_json::json!("MUSIC_U=lazy-cookie")
            );
            assert_eq!(payload["level"], serde_json::json!("exhigh"));
            assert_eq!(payload["repoId"], serde_json::json!(expected_repo_id));
            assert_eq!(
                payload["entryPath"],
                serde_json::json!("Created/lazy-track.mp3")
            );
            assert_eq!(
                payload["sourcePayload"]["provider"],
                serde_json::json!("netease-cloud-music")
            );
            Ok(serde_json::json!({
                "localPath": "C:/Mock/Temp/lazy-track.mp3",
                "tempFilePath": "C:/Mock/Temp/lazy-track.mp3",
                "mediaType": "audio/mpeg"
            }))
        }

        std::env::set_var("MOMOBKO_TEST_EXPECTED_REPO_ID", &expected_repo_id);
        set_test_backend_stat_entry_hook(Some(stat_hook));
        set_test_downloader_playback_hook(Some(playback_hook));
        let response = state
            .prepare_entry_playback_source(EntryPlaybackRequest {
                repo_id: repo_id.clone(),
                path: "Created/lazy-track.mp3".to_string(),
            })
            .expect("unindexed virtual playback source should resolve from backend stat");
        set_test_downloader_playback_hook(None);
        set_test_backend_stat_entry_hook(None);
        std::env::remove_var("MOMOBKO_TEST_EXPECTED_REPO_ID");

        assert_eq!(response.repo_id, repo_id);
        assert_eq!(response.path, "Created/lazy-track.mp3");
        assert_eq!(response.media_type, "audio/mpeg");
        assert_eq!(
            response.local_path.as_deref(),
            Some("C:/Mock/Temp/lazy-track.mp3")
        );

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn open_preview_file_source_rejects_unknown_token() {
        let (state, root, _repo_root, _thumbnail_root) = create_test_state("preview-unknown");

        let error = state
            .open_preview_file_source(&"0".repeat(64))
            .expect_err("unknown preview token should fail");

        assert!(error.contains("preview source not found"));
        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn open_preview_file_source_rejects_deleted_source_file() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("preview-deleted");
        let source_path = repo_root.join("model.glb");
        fs::write(&source_path, b"glb").expect("preview source should be written");
        let repo_id = create_repository_for_path(&state, &repo_root);
        let response = state
            .prepare_preview_file_source(FileReadRequest {
                repo_id,
                path: "model.glb".to_string(),
            })
            .expect("preview source should be prepared");
        fs::remove_file(source_path).expect("preview source should be removed");

        let error = state
            .open_preview_file_source(&response.token)
            .expect_err("deleted preview source should fail");

        assert!(error.contains("preview source file is no longer available"));
        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn delete_entry_moves_to_trash_then_permanently_deletes() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("trash-delete");
        let repo_id = create_repository_without_initial_sync(&state, &repo_root);
        fs::write(repo_root.join("note.txt"), "plain text").expect("test file should be written");
        state
            .sync_repository(SyncRequest {
                repo_id: repo_id.clone(),
            })
            .expect("repository should sync");

        state
            .delete_entry(FileDeleteRequest {
                repo_id: repo_id.clone(),
                path: "note.txt".to_string(),
                mode: None,
            })
            .expect("file should move to trash");

        assert!(!repo_root.join("note.txt").exists());
        let trash_dir = repository_trash_dir(&repo_root);
        let trash_entries = fs::read_dir(&trash_dir)
            .expect("trash directory should be readable")
            .collect::<Result<Vec<_>, _>>()
            .expect("trash entries should be readable");
        assert_eq!(trash_entries.len(), 1);
        let trash_path = trash_entries[0].file_name().to_string_lossy().to_string();

        let trash_snapshot = state
            .load_file_browser(FileBrowserRequest {
                repo_id: repo_id.clone(),
                directory_path: Some(String::new()),
                include_tree: Some(false),
                special_location: Some("trash".to_string()),
            })
            .expect("trash browser should load");
        let trash_entry = trash_snapshot
            .entries
            .iter()
            .find(|entry| entry.path == trash_path)
            .expect("trash entry should be listed");
        assert_eq!(
            trash_entry.metadata.get("originalPath"),
            Some(&serde_json::Value::String("note.txt".to_string()))
        );
        assert!(trash_entry
            .metadata
            .get("deletedAt")
            .and_then(serde_json::Value::as_str)
            .is_some());

        let snapshot = state
            .load_snapshot(&repo_id)
            .expect("snapshot should load after trash delete");
        assert!(snapshot.assets.is_empty());

        state
            .delete_entry(FileDeleteRequest {
                repo_id,
                path: trash_path,
                mode: Some("permanentDelete".to_string()),
            })
            .expect("trash entry should be permanently deleted");

        assert_eq!(
            fs::read_dir(trash_dir)
                .expect("trash directory should still exist")
                .count(),
            0
        );

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn trash_restore_all_and_empty_keep_directory_metadata() {
        let (state, root, repo_root, _thumbnail_root) = create_test_state("trash-restore");
        let repo_id = create_repository_without_initial_sync(&state, &repo_root);
        fs::create_dir_all(repo_root.join("Scenes/Act1"))
            .expect("test directory should be written");
        fs::write(repo_root.join("Scenes/Act1/shot.txt"), "plain text")
            .expect("test nested file should be written");
        fs::write(repo_root.join("loose.txt"), "plain text").expect("test file should be written");
        state
            .sync_repository(SyncRequest {
                repo_id: repo_id.clone(),
            })
            .expect("repository should sync");

        state
            .delete_entry(FileDeleteRequest {
                repo_id: repo_id.clone(),
                path: "loose.txt".to_string(),
                mode: None,
            })
            .expect("file should move to trash");
        assert!(!repo_root.join("loose.txt").exists());

        state
            .mutate_trash(TrashMutationRequest {
                repo_id: repo_id.clone(),
                action: "restore".to_string(),
                path: Some("loose.txt".to_string()),
            })
            .expect("file should restore from trash");
        assert!(repo_root.join("loose.txt").exists());

        state
            .delete_entry(FileDeleteRequest {
                repo_id: repo_id.clone(),
                path: "Scenes".to_string(),
                mode: None,
            })
            .expect("directory should move to trash");
        assert!(!repo_root.join("Scenes").exists());
        assert!(repository_trash_dir(&repo_root)
            .join("Scenes/Act1/shot.txt")
            .exists());

        let nested_trash_snapshot = state
            .load_file_browser(FileBrowserRequest {
                repo_id: repo_id.clone(),
                directory_path: Some("Scenes/Act1".to_string()),
                include_tree: Some(false),
                special_location: Some("trash".to_string()),
            })
            .expect("nested trash browser should load");
        let nested_entry = nested_trash_snapshot
            .entries
            .iter()
            .find(|entry| entry.path == "Scenes/Act1/shot.txt")
            .expect("nested trash entry should be listed");
        assert_eq!(
            nested_entry.metadata.get("originalPath"),
            Some(&serde_json::Value::String(
                "Scenes/Act1/shot.txt".to_string()
            ))
        );
        assert!(nested_entry
            .metadata
            .get("deletedAt")
            .and_then(serde_json::Value::as_str)
            .is_some());

        state
            .mutate_trash(TrashMutationRequest {
                repo_id: repo_id.clone(),
                action: "restoreAll".to_string(),
                path: None,
            })
            .expect("all trash entries should restore");
        assert!(repo_root.join("Scenes/Act1/shot.txt").exists());
        assert_eq!(
            fs::read_dir(repository_trash_dir(&repo_root))
                .expect("trash directory should exist")
                .count(),
            0
        );

        state
            .delete_entry(FileDeleteRequest {
                repo_id: repo_id.clone(),
                path: "loose.txt".to_string(),
                mode: None,
            })
            .expect("file should move to trash again");
        state
            .mutate_trash(TrashMutationRequest {
                repo_id,
                action: "empty".to_string(),
                path: None,
            })
            .expect("trash should empty");
        assert!(!repo_root.join("loose.txt").exists());
        assert_eq!(
            fs::read_dir(repository_trash_dir(&repo_root))
                .expect("trash directory should exist")
                .count(),
            0
        );

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn thumbnail_file_name_stays_within_windows_component_limit() {
        let asset_id = format!(
            "asset-{}",
            slugify_repo_id("startIcon.png", LONG_RELATIVE_PATH)
        );
        let repo_dir = thumbnail_repository_dir_name(&asset_id, LONG_RELATIVE_PATH);
        let file_name = thumbnail_file_name(
            &asset_id,
            LONG_RELATIVE_PATH,
            LONG_RELATIVE_PATH,
            "file",
            "generated",
        );

        assert!(repo_dir.len() <= 255);
        assert!(file_name.len() <= 255);
        assert!(file_name.ends_with(".jpg"));
        assert_eq!(file_name.len(), 68);
    }

    #[test]
    fn thumbnail_cache_names_are_stable() {
        let repo_dir = thumbnail_repository_dir_name("repo-cubism", "C:/Assets/Cubism");
        let file_name = thumbnail_file_name(
            "repo-cubism",
            "C:/Assets/Cubism",
            LONG_RELATIVE_PATH,
            "file",
            "generated",
        );

        assert_eq!(
            repo_dir,
            thumbnail_repository_dir_name("repo-cubism", "C:/Assets/Cubism")
        );
        assert_eq!(
            file_name,
            thumbnail_file_name(
                "repo-cubism",
                "C:/Assets/Cubism",
                LONG_RELATIVE_PATH,
                "file",
                "generated",
            )
        );
    }

    #[test]
    fn thumbnail_cache_names_differ_for_different_paths() {
        let first = thumbnail_file_name(
            "repo-cubism",
            "C:/Assets/Cubism",
            LONG_RELATIVE_PATH,
            "file",
            "generated",
        );
        let second = thumbnail_file_name(
            "repo-cubism",
            "C:/Assets/Cubism",
            "CubismSdkForNative-5-r.5/Samples/OpenGL/Demo/proj.harmonyos.cmake/Full/entry/src/ohosTest/resources/base/media/icon.png",
            "file",
            "generated",
        );

        assert_ne!(first, second);
    }

    #[test]
    fn video_thumbnail_ffmpeg_args_write_a_single_image() {
        let source_path = Path::new("C:/Assets/video.mp4");
        let thumbnail_path = Path::new("C:/Cache/thumbnail.jpg");
        let args = video_thumbnail_ffmpeg_args(source_path, thumbnail_path);

        assert!(args.windows(2).any(|items| items == ["-frames:v", "1"]));
        assert!(args.windows(2).any(|items| items == ["-update", "1"]));

        let update_index = args
            .iter()
            .position(|item| item == "-update")
            .expect("missing -update");
        let output_index = args
            .iter()
            .position(|item| item == thumbnail_path.as_os_str())
            .expect("missing output path");
        assert!(update_index < output_index);
    }

    #[test]
    fn audio_thumbnail_ffmpeg_args_extract_the_first_embedded_cover_stream() {
        let source_path = Path::new("C:/Assets/track.flac");
        let thumbnail_path = Path::new("C:/Cache/thumbnail.jpg");
        let args = audio_thumbnail_ffmpeg_args(source_path, thumbnail_path);

        assert!(args.windows(2).any(|items| items == ["-map", "0:v:0"]));
        assert!(args.windows(2).any(|items| items == ["-frames:v", "1"]));
        assert!(args.windows(2).any(|items| items == ["-update", "1"]));

        let map_index = args
            .iter()
            .position(|item| item == "-map")
            .expect("missing -map");
        let output_index = args
            .iter()
            .position(|item| item == thumbnail_path.as_os_str())
            .expect("missing output path");
        assert!(map_index < output_index);
    }

    #[test]
    fn audio_cover_probe_args_select_video_streams_as_json() {
        let source_path = Path::new("C:/Assets/track.mp3");
        let args = audio_cover_probe_args(source_path);

        assert!(args
            .windows(2)
            .any(|items| items == ["-select_streams", "v"]));
        assert!(args
            .windows(2)
            .any(|items| items == ["-show_entries", "stream=index"]));
        assert!(args.windows(2).any(|items| items == ["-of", "json"]));
        assert_eq!(args.last(), Some(&source_path.as_os_str().to_os_string()));
    }

    #[test]
    fn audio_cover_probe_output_reports_missing_streams() {
        assert!(!audio_cover_probe_output_has_stream(br#"{"streams":[]}"#)
            .expect("probe output should parse"));
        assert!(!audio_cover_probe_output_has_stream(br#"{}"#).expect("probe output should parse"));
    }

    #[test]
    fn audio_cover_probe_output_reports_present_streams() {
        assert!(
            audio_cover_probe_output_has_stream(br#"{"streams":[{"index":1}]}"#)
                .expect("probe output should parse")
        );
    }
}
