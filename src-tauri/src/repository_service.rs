use rusqlite::{params, types::Type, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    ffi::{CStr, CString, OsString},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::raw::c_char,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex, OnceLock},
    thread,
    time::SystemTime,
};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

const REGISTRY_FILE_NAME: &str = "repositories.db";
const REPO_META_DIR: &str = ".momo";
const LEGACY_REPO_META_DIR: &str = ".meta";
const REPO_TRASH_DIR: &str = "trash";
const REPO_TRASH_MANIFEST_FILE_NAME: &str = "trash.json";
const REPO_METADATA_FILE_NAME: &str = "repository.json";
const REPO_DB_FILE_NAME: &str = "metadata.db";
const REPO_SCHEMA_VERSION: i64 = 1;
const THUMBNAIL_SIZE: u32 = 256;

static FFMPEG_READY: OnceLock<Result<(), String>> = OnceLock::new();
static RUNTIME_PLUGIN_RESOURCE_DIR: OnceLock<PathBuf> = OnceLock::new();

const REGISTRY_SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS repositories (
  repo_id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  path TEXT NOT NULL UNIQUE,
  backend_plugin_id TEXT NOT NULL DEFAULT 'builtin.local-filesystem',
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
const WEBDAV_PLUGIN_ID: &str = "momobako.webdav";
const CLOUD_DRIVE_PLUGIN_ID: &str = "momobako.cloud-drive";
const LEGACY_LOCAL_FILESYSTEM_PLUGIN_ID: &str = "builtin.local-filesystem";
const LEGACY_WEBDAV_PLUGIN_ID: &str = "builtin.webdav";
const LEGACY_CLOUD_DRIVE_PLUGIN_ID: &str = "builtin.cloud-drive";
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
    pub metadata: BTreeMap<String, serde_json::Value>,
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
pub struct FilePreviewSourceResponse {
    pub repo_id: String,
    pub path: String,
    pub token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
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

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchRequest {
    pub query: String,
    pub repo_id: Option<String>,
    pub metadata_key: Option<String>,
    pub metadata_value: Option<String>,
    pub tag: Option<String>,
    pub tags: Option<Vec<String>>,
    pub metadata_filters: Option<Vec<SearchMetadataFilter>>,
    pub formats: Option<Vec<String>>,
    pub min_rating: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct SmartFolderFilter {
    pub query: Option<String>,
    pub path_prefix: Option<String>,
    pub tags: Option<Vec<String>>,
    pub formats: Option<Vec<String>>,
    pub colors: Option<Vec<String>>,
    pub shapes: Option<Vec<String>>,
    pub metadata_filters: Option<Vec<SearchMetadataFilter>>,
    pub min_rating: Option<f64>,
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
pub struct PluginManifest {
    pub plugin_id: String,
    pub legacy_plugin_ids: Vec<String>,
    pub name: String,
    pub version: String,
    pub kind: String,
    pub description: String,
    pub capabilities: Vec<String>,
    pub enabled: bool,
    pub sdk: String,
    pub entry: serde_json::Value,
    pub source: String,
    pub runtime: String,
    pub permissions: Vec<String>,
    pub compat: PluginCompat,
    pub status: String,
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
    pub archive_path: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PluginMutationResponse {
    pub plugins: Vec<PluginManifest>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PluginCompat {
    pub sdk_version: String,
    pub legacy_plugin_ids: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginCallEnvelope {
    method: String,
    payload: serde_json::Value,
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
    pub method: String,
    pub path: String,
    pub summary: String,
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
}

struct RuntimeFileSystemBackendAdapter {
    service_root: PathBuf,
    plugin_id: String,
}

struct LocalFileSystemBackend;
pub struct RepositoryState {
    root: PathBuf,
    registry_path: PathBuf,
    initialized: Mutex<bool>,
    preview_sources: Mutex<BTreeMap<String, PreviewFileSource>>,
}

impl RepositoryState {
    pub fn from_root(root: PathBuf) -> Self {
        let registry_path = root.join(REGISTRY_FILE_NAME);
        Self {
            root,
            registry_path,
            initialized: Mutex::new(false),
            preview_sources: Mutex::new(BTreeMap::new()),
        }
    }

    pub fn ensure_initialized(&self) -> Result<(), String> {
        let mut initialized = self
            .initialized
            .lock()
            .map_err(|_| "repository state lock poisoned".to_string())?;
        if *initialized {
            return Ok(());
        }

        fs::create_dir_all(&self.root).map_err(io_error)?;
        let registry = Connection::open(&self.registry_path).map_err(db_error)?;
        registry
            .execute_batch(REGISTRY_SCHEMA_SQL)
            .map_err(db_error)?;
        migrate_registry_schema(&registry).map_err(db_error)?;
        migrate_registry_plugin_ids(&registry).map_err(db_error)?;

        *initialized = true;
        Ok(())
    }

    pub fn list_repositories(&self) -> Result<Vec<RepositorySummary>, String> {
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
                let repo_id: String = row.get(0)?;
                let path: String = row.get(2)?;
                let backend_plugin_id: String = row.get(3)?;
                let backend_plugin_id = plugin_registry.normalize_plugin_id(&backend_plugin_id);
                let status = repository_runtime_status(
                    &path,
                    &backend_plugin_id,
                    row.get::<_, String>(5)?.as_str(),
                );
                let asset_count = if status == "missing" {
                    0
                } else {
                    load_asset_count(&self.root, &repo_id, &path, &backend_plugin_id).unwrap_or(0)
                };

                Ok(RepositorySummary {
                    repo_id,
                    name: row.get(1)?,
                    path,
                    backend: backend_summary_from_registry(&plugin_registry, &backend_plugin_id),
                    status,
                    asset_count,
                    updated_at: row.get(6)?,
                })
            })
            .map_err(db_error)?;

        rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
    }

    pub fn create_repository(
        &self,
        request: RepositoryMutationRequest,
    ) -> Result<RepositoryMutationResponse, String> {
        self.ensure_initialized()?;

        let backend = parse_backend_request(&self.root, &request)?;
        let repo_id = request
            .repo_id
            .unwrap_or_else(|| slugify_repo_id(&request.name, &request.path));
        let repo_root = normalize_repository_root_for_backend(&request.path, &backend, false)?;
        let seed = RepositorySeed {
            repo_id: &repo_id,
            name: &request.name,
            root_path: "",
            status: "ready",
            assets: &[],
        };
        initialize_repository_directory(&self.root, &repo_root, &seed, &backend)?;

        let registry = Connection::open(&self.registry_path).map_err(db_error)?;
        upsert_registry_entry(&registry, &repo_root, &seed, &backend)?;
        self.sync_repository(SyncRequest {
            repo_id: repo_id.clone(),
        })?;

        let repository = self.load_repository_record(&repo_id)?.summary;
        Ok(RepositoryMutationResponse { repository })
    }

    pub fn import_repository(
        &self,
        request: RepositoryMutationRequest,
    ) -> Result<RepositoryMutationResponse, String> {
        self.ensure_initialized()?;

        let requested_backend = parse_backend_request(&self.root, &request)?;
        let repo_root =
            normalize_repository_root_for_backend(&request.path, &requested_backend, true)?;
        migrate_legacy_meta_dir_if_needed(&repo_root, &requested_backend.plugin_id)?;
        let metadata_path = repository_meta_dir(&repo_root).join(REPO_METADATA_FILE_NAME);
        let imported_metadata = if metadata_path.exists() {
            let raw = fs::read_to_string(&metadata_path).map_err(io_error)?;
            let metadata =
                serde_json::from_str::<RepositoryMetadataFileImport>(&raw).map_err(json_error)?;
            rewrite_repository_metadata_if_needed(
                &self.root,
                &metadata_path,
                &metadata,
                &repo_root,
                None,
            )?;
            Some(metadata)
        } else {
            None
        };
        let repo_id = imported_metadata
            .as_ref()
            .map(|metadata| metadata.repo_id.clone())
            .unwrap_or_else(|| slugify_repo_id(&request.name, &request.path));
        let repo_name = imported_metadata
            .as_ref()
            .and_then(|metadata| metadata.name.as_ref())
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .unwrap_or(request.name.as_str())
            .to_string();
        let backend = imported_metadata
            .as_ref()
            .and_then(|metadata| import_backend_record(&self.root, metadata))
            .unwrap_or(requested_backend);

        let seed = RepositorySeed {
            repo_id: &repo_id,
            name: &repo_name,
            root_path: "",
            status: "ready",
            assets: &[],
        };

        if !repository_meta_dir(&repo_root).exists()
            && !legacy_repository_meta_dir(&repo_root).exists()
        {
            initialize_repository_directory(&self.root, &repo_root, &seed, &backend)?;
        }

        let registry = Connection::open(&self.registry_path).map_err(db_error)?;
        upsert_registry_entry(&registry, &repo_root, &seed, &backend)?;
        self.sync_repository(SyncRequest {
            repo_id: repo_id.clone(),
        })?;

        let repository = self.load_repository_record(&repo_id)?.summary;
        Ok(RepositoryMutationResponse { repository })
    }

    pub fn attach_repository_folder(
        &self,
        request: RepositoryFolderRequest,
    ) -> Result<RepositoryMutationResponse, String> {
        self.ensure_initialized()?;

        let path = request.path.trim();
        if path.is_empty() {
            return Err("repository path cannot be empty".to_string());
        }

        let backend = parse_backend_request(
            &self.root,
            &RepositoryMutationRequest {
                repo_id: None,
                name: String::new(),
                path: path.to_string(),
                backend_plugin_id: None,
                backend_config: None,
            },
        )?;
        let repo_root = normalize_repository_root_for_backend(path, &backend, true)?;
        ensure_backend_path_is_attachable(&self.root, &backend, &repo_root)?;
        let name = infer_repository_name(&repo_root);
        let metadata_path = if repository_meta_dir(&repo_root).exists() {
            repository_meta_dir(&repo_root).join(REPO_METADATA_FILE_NAME)
        } else {
            legacy_repository_meta_dir(&repo_root).join(REPO_METADATA_FILE_NAME)
        };
        let mutation = RepositoryMutationRequest {
            repo_id: None,
            name,
            path: path.to_string(),
            backend_plugin_id: Some(backend.plugin_id.clone()),
            backend_config: Some(backend.config.clone()),
        };

        if metadata_path.exists() {
            self.import_repository(mutation)
        } else {
            self.create_repository(mutation)
        }
    }

    pub fn delete_repository(&self, repo_id: &str) -> Result<(), String> {
        self.ensure_initialized()?;
        let registry = Connection::open(&self.registry_path).map_err(db_error)?;
        registry
            .execute("DELETE FROM repositories WHERE repo_id = ?1", [repo_id])
            .map_err(db_error)?;
        let storage_dir = repository_state_storage_dir(&self.root, repo_id);
        if storage_dir.exists() {
            fs::remove_dir_all(storage_dir).map_err(io_error)?;
        }
        Ok(())
    }

    pub fn relocate_repository(
        &self,
        request: RepositoryRelocateRequest,
    ) -> Result<RepositoryMutationResponse, String> {
        self.ensure_initialized()?;

        let repo = self.load_repository_record(&request.repo_id)?;
        if normalized_builtin_plugin_id(&repo.backend_record.plugin_id)
            != LOCAL_FILESYSTEM_PLUGIN_ID
        {
            return Err("only local filesystem repositories can be relocated".to_string());
        }

        let next_path = request.path.trim();
        if next_path.is_empty() {
            return Err("repository path cannot be empty".to_string());
        }

        let repo_root =
            normalize_repository_root_for_backend(next_path, &repo.backend_record, true)?;
        if !repo_root.is_dir() {
            return Err("repository path is not a directory".to_string());
        }
        ensure_backend_path_is_attachable(&self.root, &repo.backend_record, &repo_root)?;

        let metadata_path = repository_meta_dir(&repo_root).join(REPO_METADATA_FILE_NAME);
        if !metadata_path.exists() {
            return Err("repository metadata not found in selected folder".to_string());
        }
        let raw = fs::read_to_string(&metadata_path).map_err(io_error)?;
        let metadata =
            serde_json::from_str::<RepositoryMetadataFileImport>(&raw).map_err(json_error)?;
        if metadata.repo_id != request.repo_id {
            return Err("selected folder belongs to a different repository".to_string());
        }
        rewrite_repository_metadata_if_needed(
            &self.root,
            &metadata_path,
            &metadata,
            &repo_root,
            Some(&repo_root),
        )?;

        let registry = Connection::open(&self.registry_path).map_err(db_error)?;
        registry
            .execute(
                r#"
                UPDATE repositories
                SET path = ?2, status = 'ready', updated_at = ?3
                WHERE repo_id = ?1
                "#,
                params![
                    request.repo_id.as_str(),
                    repo_root.to_string_lossy().to_string(),
                    now_rfc3339()
                ],
            )
            .map_err(db_error)?;

        self.sync_repository(SyncRequest {
            repo_id: request.repo_id.clone(),
        })?;

        let repository = self.load_repository_record(&request.repo_id)?.summary;
        Ok(RepositoryMutationResponse { repository })
    }

    pub fn export_repository(
        &self,
        request: RepositoryExportRequest,
    ) -> Result<RepositoryExportResponse, String> {
        self.ensure_initialized()?;
        let repository = self.load_repository_record(&request.repo_id)?.summary;
        let repo_root = PathBuf::from(&repository.path);

        match request.target.as_str() {
            "archive" => {
                let archive = request
                    .archive
                    .ok_or_else(|| "archive export options are required".to_string())?;
                export_repository_archive(&repo_root, &archive)?;
                Ok(RepositoryExportResponse {
                    repository,
                    target: "archive".to_string(),
                    output_path: Some(archive.output_path),
                    format: Some(archive.format),
                    encrypted: Some(archive.encrypt),
                    remote: None,
                    branch: None,
                    message: "资源库压缩包已导出".to_string(),
                })
            }
            "git" => {
                let git = request.git.unwrap_or(RepositoryGitExportOptions {
                    remote: None,
                    branch: None,
                    message: None,
                });
                let result = export_repository_to_git(&repo_root, &git)?;
                Ok(RepositoryExportResponse {
                    repository,
                    target: "git".to_string(),
                    output_path: None,
                    format: None,
                    encrypted: None,
                    remote: Some(result.remote),
                    branch: Some(result.branch),
                    message: result.message,
                })
            }
            value => Err(format!("unsupported repository export target: {value}")),
        }
    }

    pub fn load_snapshot(&self, repo_id: &str) -> Result<RepositorySnapshot, String> {
        self.ensure_initialized()?;

        let repo = self.load_repository_record(repo_id)?;
        let repo_root = PathBuf::from(&repo.summary.path);
        let connection = self.open_repository_connection(
            &repo.summary.repo_id,
            &repo.summary.path,
            &repo.backend_record,
        )?;
        let asset_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM assets WHERE status != 'deleted'",
                [],
                |row| row.get(0),
            )
            .map_err(db_error)?;

        let thumbnail_root = self.repository_thumbnail_root(&repo)?;
        let folders = load_folder_summaries(&connection, repo_id).map_err(db_error)?;
        let assets = normalize_asset_summaries(
            &connection,
            &repo,
            &thumbnail_root,
            load_assets(&connection, repo_id).map_err(db_error)?,
        )?;
        let metadata_fields = load_metadata_fields(&connection).map_err(db_error)?;
        let recent_revision_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM revisions", [], |row| row.get(0))
            .map_err(db_error)?;
        let overview = build_repository_overview(&repo_root, &assets)?;

        Ok(RepositorySnapshot {
            repository: RepositorySummary {
                asset_count,
                ..repo.summary
            },
            folder_label: dominant_folder_label(&folders, &assets),
            folders,
            assets,
            metadata_fields,
            recent_revision_count,
            overview,
        })
    }

    pub fn load_asset_detail(&self, repo_id: &str, asset_id: &str) -> Result<AssetDetail, String> {
        self.ensure_initialized()?;
        let repo = self.load_repository_record(repo_id)?;
        let connection = self.open_repository_connection(
            &repo.summary.repo_id,
            &repo.summary.path,
            &repo.backend_record,
        )?;
        load_asset_detail_from_connection(&connection, repo_id, asset_id).map_err(db_error)
    }

    pub fn load_file_browser(
        &self,
        request: FileBrowserRequest,
    ) -> Result<FileBrowserSnapshot, String> {
        self.ensure_initialized()?;

        let repo = self.load_repository_record(&request.repo_id)?;
        let repo_root = PathBuf::from(&repo.summary.path);
        let connection = self.open_repository_connection(
            &repo.summary.repo_id,
            &repo.summary.path,
            &repo.backend_record,
        )?;
        let thumbnail_root = self.repository_thumbnail_root(&repo)?;
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
        let special_location = normalize_special_location(request.special_location.as_deref())?;
        if special_location.is_some() && repo.backend_record.plugin_id != LOCAL_FILESYSTEM_PLUGIN_ID
        {
            return Err(format!(
                "trash browser is only supported for local filesystem repositories, got: {}",
                repo.backend_record.plugin_id
            ));
        }
        let current_path =
            normalize_directory_path(request.directory_path.as_deref().unwrap_or_default())?;
        let tree = if special_location.is_some() {
            None
        } else if request.include_tree.unwrap_or(true) {
            Some(list_backend_tree(&self.root, &repo, &repo_root)?)
        } else {
            None
        };
        let entries = if special_location.as_deref() == Some("trash") {
            list_trash_directory_entries(&repo_root, &current_path, &asset_map, &thumbnail_map)?
        } else {
            list_backend_directory_entries(
                &self.root,
                &repo,
                &repo_root,
                &current_path,
                &asset_map,
                &thumbnail_map,
            )?
        };
        let entries = attach_browser_entry_metadata(&connection, entries).map_err(db_error)?;

        Ok(FileBrowserSnapshot {
            repo_id: request.repo_id,
            root_path: repo.summary.path,
            backend_plugin_id: repo.backend_record.plugin_id.clone(),
            backend_kind: repo.summary.backend.kind,
            current_path,
            special_location,
            tree,
            entries,
        })
    }

    pub fn read_file(&self, request: FileReadRequest) -> Result<Vec<u8>, String> {
        self.ensure_initialized()?;

        let repo = self.load_repository_record(&request.repo_id)?;
        if repo.backend_record.plugin_id != LOCAL_FILESYSTEM_PLUGIN_ID {
            return Err(format!(
                "file preview read is not available for backend: {}",
                repo.backend_record.plugin_id
            ));
        }

        let entry_path = normalize_entry_path(&request.path)?;
        let repo_root = PathBuf::from(&repo.summary.path);
        let file_path = resolve_repository_relative_path(&repo_root, &entry_path)?;
        if !file_path.exists() {
            return Err(format!("file not found: {entry_path}"));
        }
        if !file_path.is_file() {
            return Err(format!("path is not a file: {entry_path}"));
        }

        fs::read(file_path).map_err(io_error)
    }

    pub fn prepare_preview_file_source(
        &self,
        request: FileReadRequest,
    ) -> Result<FilePreviewSourceResponse, String> {
        self.ensure_initialized()?;

        let repo = self.load_repository_record(&request.repo_id)?;
        if repo.backend_record.plugin_id != LOCAL_FILESYSTEM_PLUGIN_ID {
            return Err(format!(
                "file preview source is not available for backend: {}",
                repo.backend_record.plugin_id
            ));
        }

        let entry_path = normalize_entry_path(&request.path)?;
        let repo_root = PathBuf::from(&repo.summary.path);
        let file_path = resolve_repository_relative_path(&repo_root, &entry_path)?;
        if !file_path.exists() {
            return Err(format!("file not found: {entry_path}"));
        }
        if !file_path.is_file() {
            return Err(format!("path is not a file: {entry_path}"));
        }

        let metadata = fs::metadata(&file_path).map_err(io_error)?;
        let modified_at = metadata
            .modified()
            .ok()
            .map(system_time_to_rfc3339)
            .transpose()
            .map_err(time_error)?;
        let extension = file_path
            .extension()
            .map(|value| value.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let media_type = preview_media_type_for_extension(&extension).to_string();
        let token = preview_file_token(
            &repo.summary.repo_id,
            &repo.summary.path,
            &entry_path,
            metadata.len(),
            modified_at.as_deref().unwrap_or_default(),
        );

        self.preview_sources
            .lock()
            .map_err(|_| "preview source lock poisoned".to_string())?
            .insert(
                token.clone(),
                PreviewFileSource {
                    path: file_path,
                    media_type: media_type.clone(),
                },
            );

        Ok(FilePreviewSourceResponse {
            repo_id: request.repo_id,
            path: entry_path,
            token,
            source_url: None,
            media_type,
            size_bytes: metadata.len() as i64,
            modified_at,
        })
    }

    pub fn open_preview_file_source(&self, token: &str) -> Result<(File, String), String> {
        let source = self
            .preview_sources
            .lock()
            .map_err(|_| "preview source lock poisoned".to_string())?
            .get(token)
            .cloned()
            .ok_or_else(|| "preview source not found".to_string())?;
        if !source.path.is_file() {
            return Err("preview source file is no longer available".to_string());
        }
        let file = File::open(&source.path).map_err(io_error)?;
        Ok((file, source.media_type))
    }

    pub fn list_smart_folders(&self, repo_id: &str) -> Result<Vec<SmartFolderTreeNode>, String> {
        self.ensure_initialized()?;
        let repo = self.load_repository_record(repo_id)?;
        let connection = self.open_repository_connection(
            &repo.summary.repo_id,
            &repo.summary.path,
            &repo.backend_record,
        )?;
        let folders = load_smart_folders(&connection, repo_id).map_err(db_error)?;
        Ok(build_smart_folder_tree(folders))
    }

    pub fn create_smart_folder(
        &self,
        request: SmartFolderMutationRequest,
    ) -> Result<SmartFolderMutationResponse, String> {
        self.ensure_initialized()?;
        let repo = self.load_repository_record(&request.repo_id)?;
        let connection = self.open_repository_connection(
            &repo.summary.repo_id,
            &repo.summary.path,
            &repo.backend_record,
        )?;
        let name = validate_smart_folder_name(&request.name)?;
        validate_smart_folder_parent(
            &connection,
            &request.repo_id,
            request.parent_id.as_deref(),
            None,
        )
        .map_err(db_error)?;
        let smart_folder_id = request
            .smart_folder_id
            .as_deref()
            .map(validate_smart_folder_id)
            .transpose()?
            .unwrap_or_else(|| {
                smart_folder_id_for(&request.repo_id, request.parent_id.as_deref(), &name)
            });
        let filter = normalize_smart_folder_filter(request.filter);
        let filter_json = serde_json::to_string(&filter).map_err(json_error)?;
        let now = now_rfc3339();
        let sort_order = next_smart_folder_sort_order(
            &connection,
            &request.repo_id,
            request.parent_id.as_deref(),
        )
        .map_err(db_error)?;
        connection
            .execute(
                r#"
                INSERT INTO smart_folders (
                  smart_folder_id, repo_id, parent_id, name, filter_json,
                  sort_order, created_at, updated_at
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
                "#,
                params![
                    smart_folder_id,
                    request.repo_id,
                    normalized_optional_id(request.parent_id.as_deref()),
                    name,
                    filter_json,
                    sort_order,
                    now
                ],
            )
            .map_err(db_error)?;

        let folders = load_smart_folders(&connection, &request.repo_id).map_err(db_error)?;
        let smart_folder = folders
            .iter()
            .find(|folder| folder.smart_folder_id == smart_folder_id)
            .cloned();
        Ok(SmartFolderMutationResponse {
            smart_folders: build_smart_folder_tree(folders),
            smart_folder,
        })
    }

    pub fn update_smart_folder(
        &self,
        request: SmartFolderUpdateRequest,
    ) -> Result<SmartFolderMutationResponse, String> {
        self.ensure_initialized()?;
        let repo = self.load_repository_record(&request.repo_id)?;
        let connection = self.open_repository_connection(
            &repo.summary.repo_id,
            &repo.summary.path,
            &repo.backend_record,
        )?;
        let smart_folder_id = validate_smart_folder_id(&request.smart_folder_id)?;
        let name = validate_smart_folder_name(&request.name)?;
        let existing = load_smart_folder(&connection, &request.repo_id, &smart_folder_id)
            .map_err(db_error)?
            .ok_or_else(|| format!("smart folder not found: {smart_folder_id}"))?;
        validate_smart_folder_parent(
            &connection,
            &request.repo_id,
            request.parent_id.as_deref(),
            Some(&smart_folder_id),
        )
        .map_err(db_error)?;
        let filter = normalize_smart_folder_filter(request.filter);
        let filter_json = serde_json::to_string(&filter).map_err(json_error)?;
        let now = now_rfc3339();
        let sort_order =
            if normalized_optional_id(request.parent_id.as_deref()) == existing.parent_id {
                existing.sort_order
            } else {
                next_smart_folder_sort_order(
                    &connection,
                    &request.repo_id,
                    request.parent_id.as_deref(),
                )
                .map_err(db_error)?
            };
        connection
            .execute(
                r#"
                UPDATE smart_folders
                SET parent_id = ?3, name = ?4, filter_json = ?5,
                    sort_order = ?6, updated_at = ?7
                WHERE repo_id = ?1 AND smart_folder_id = ?2
                "#,
                params![
                    request.repo_id,
                    smart_folder_id,
                    normalized_optional_id(request.parent_id.as_deref()),
                    name,
                    filter_json,
                    sort_order,
                    now
                ],
            )
            .map_err(db_error)?;

        let folders = load_smart_folders(&connection, &request.repo_id).map_err(db_error)?;
        let smart_folder = folders
            .iter()
            .find(|folder| folder.smart_folder_id == smart_folder_id)
            .cloned();
        Ok(SmartFolderMutationResponse {
            smart_folders: build_smart_folder_tree(folders),
            smart_folder,
        })
    }

    pub fn delete_smart_folder(
        &self,
        repo_id: &str,
        smart_folder_id: &str,
    ) -> Result<SmartFolderMutationResponse, String> {
        self.ensure_initialized()?;
        let repo = self.load_repository_record(repo_id)?;
        let connection = self.open_repository_connection(
            &repo.summary.repo_id,
            &repo.summary.path,
            &repo.backend_record,
        )?;
        let smart_folder_id = validate_smart_folder_id(smart_folder_id)?;
        load_smart_folder(&connection, repo_id, &smart_folder_id)
            .map_err(db_error)?
            .ok_or_else(|| format!("smart folder not found: {smart_folder_id}"))?;
        connection
            .execute(
                r#"
                WITH RECURSIVE deleting(id) AS (
                  SELECT smart_folder_id
                  FROM smart_folders
                  WHERE repo_id = ?1 AND smart_folder_id = ?2
                  UNION ALL
                  SELECT child.smart_folder_id
                  FROM smart_folders child
                  INNER JOIN deleting ON child.parent_id = deleting.id
                  WHERE child.repo_id = ?1
                )
                DELETE FROM smart_folders
                WHERE repo_id = ?1 AND smart_folder_id IN (SELECT id FROM deleting)
                "#,
                params![repo_id, smart_folder_id],
            )
            .map_err(db_error)?;
        let folders = load_smart_folders(&connection, repo_id).map_err(db_error)?;
        Ok(SmartFolderMutationResponse {
            smart_folders: build_smart_folder_tree(folders),
            smart_folder: None,
        })
    }

    pub fn query_smart_folder(
        &self,
        repo_id: &str,
        smart_folder_id: &str,
    ) -> Result<SmartFolderResultSnapshot, String> {
        self.ensure_initialized()?;
        let repo = self.load_repository_record(repo_id)?;
        let connection = self.open_repository_connection(
            &repo.summary.repo_id,
            &repo.summary.path,
            &repo.backend_record,
        )?;
        let smart_folder_id = validate_smart_folder_id(smart_folder_id)?;
        let folders = load_smart_folders(&connection, repo_id).map_err(db_error)?;
        let smart_folder = folders
            .iter()
            .find(|folder| folder.smart_folder_id == smart_folder_id)
            .cloned()
            .ok_or_else(|| format!("smart folder not found: {smart_folder_id}"))?;
        let inherited_filter = inherited_smart_folder_filter(&folders, &smart_folder);
        let thumbnail_root = self.repository_thumbnail_root(&repo)?;
        let asset_map = normalize_asset_thumbnail_map(
            &connection,
            &repo,
            &thumbnail_root,
            load_asset_path_map(&connection, repo_id).map_err(db_error)?,
        )?;
        let results =
            query_smart_folder_entries(&connection, &repo.summary, &inherited_filter, &asset_map)
                .map_err(db_error)?;
        Ok(SmartFolderResultSnapshot {
            repo_id: repo_id.to_string(),
            smart_folder,
            inherited_filter,
            results,
        })
    }

    pub fn search_assets(&self, request: SearchRequest) -> Result<SearchResponse, String> {
        self.ensure_initialized()?;

        let normalized_query = request.query.trim().to_lowercase();
        if normalized_query.is_empty()
            && request.tag.is_none()
            && request
                .tags
                .as_ref()
                .map(|items| items.iter().all(|item| item.trim().is_empty()))
                .unwrap_or(true)
            && request.metadata_key.is_none()
            && request
                .metadata_filters
                .as_ref()
                .map(|items| {
                    items
                        .iter()
                        .all(|item| item.key.trim().is_empty() || item.value.trim().is_empty())
                })
                .unwrap_or(true)
            && request
                .formats
                .as_ref()
                .map(|items| items.iter().all(|item| item.trim().is_empty()))
                .unwrap_or(true)
            && request.min_rating.is_none()
        {
            return Ok(SearchResponse {
                query: request.query,
                results: Vec::new(),
            });
        }

        let repositories = self.load_repository_records()?;
        let mut results = Vec::new();

        for repo in repositories {
            if let Some(filter_repo_id) = &request.repo_id {
                if &repo.summary.repo_id != filter_repo_id {
                    continue;
                }
            }
            let connection = self.open_repository_connection(
                &repo.summary.repo_id,
                &repo.summary.path,
                &repo.backend_record,
            )?;
            let repo_results =
                search_repository_assets(&connection, &repo.summary, &normalized_query, &request)
                    .map_err(db_error)?;
            results.extend(repo_results);
        }

        Ok(SearchResponse {
            query: request.query,
            results,
        })
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
        self.ensure_initialized()?;
        let repo = self.load_repository_record(&request.repo_id)?;
        let mut connection = self.open_repository_connection(
            &repo.summary.repo_id,
            &repo.summary.path,
            &repo.backend_record,
        )?;
        let tx = connection.transaction().map_err(db_error)?;

        let current_version: i64 = tx
            .query_row(
                "SELECT version FROM assets WHERE repo_id = ?1 AND asset_id = ?2",
                params![request.repo_id, request.asset_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(db_error)?
            .ok_or_else(|| format!("asset not found: {}", request.asset_id))?;

        if current_version != request.expected_version {
            let asset =
                load_asset_detail_from_transaction(&tx, &request.repo_id, &request.asset_id)
                    .map_err(db_error)?;
            return Ok(MetadataUpdateResponse {
                outcome: "conflict".to_string(),
                asset,
            });
        }

        let before_map =
            load_metadata_map_from_transaction(&tx, &request.asset_id).map_err(db_error)?;
        let now = now_rfc3339();
        let next_version = current_version + 1;

        for (key, value) in &request.metadata {
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
                params![request.asset_id, key, value_type, value.to_string(), now],
            )
            .map_err(db_error)?;
        }

        tx.execute(
            r#"
            UPDATE assets
            SET version = ?3, updated_at = ?4, modified_at = ?4
            WHERE repo_id = ?1 AND asset_id = ?2
            "#,
            params![request.repo_id, request.asset_id, next_version, now],
        )
        .map_err(db_error)?;

        let after_map =
            load_metadata_map_from_transaction(&tx, &request.asset_id).map_err(db_error)?;
        tx.execute(
            r#"
            INSERT INTO revisions (
              revision_id, repo_id, asset_id, timestamp, operation, before_json, after_json, source
            )
            VALUES (?1, ?2, ?3, ?4, 'metadata.updated', ?5, ?6, ?7)
            "#,
            params![
                format!("rev-{}-{}", request.asset_id, next_version),
                request.repo_id,
                request.asset_id,
                now,
                serde_json::to_string(&before_map).map_err(json_error)?,
                serde_json::to_string(&after_map).map_err(json_error)?,
                request.source.unwrap_or_else(|| "desktop".to_string())
            ],
        )
        .map_err(db_error)?;

        tx.commit().map_err(db_error)?;
        let asset = self.load_asset_detail(&request.repo_id, &request.asset_id)?;

        Ok(MetadataUpdateResponse {
            outcome: "success".to_string(),
            asset,
        })
    }

    pub fn sync_repository(&self, request: SyncRequest) -> Result<SyncResult, String> {
        self.sync_repository_with_candidate_skips(&request.repo_id, &HashSet::new())
    }

    fn sync_repository_with_candidate_skips(
        &self,
        repo_id: &str,
        skip_hardlink_candidate_paths: &HashSet<String>,
    ) -> Result<SyncResult, String> {
        self.ensure_initialized()?;
        let repo = self.load_repository_record(repo_id)?;
        let mut connection = self.open_repository_connection(
            &repo.summary.repo_id,
            &repo.summary.path,
            &repo.backend_record,
        )?;
        let tx = connection.transaction().map_err(db_error)?;

        let scan = sync_repository_files(&self.root, &tx, &repo, skip_hardlink_candidate_paths)
            .map_err(db_error)?;
        tx.commit().map_err(db_error)?;
        Ok(scan)
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
            absolute_path: resolve_repository_relative_path(&repo_root, &entry_path)?,
            relative_path: entry_path.clone(),
            filename,
            extension,
            size_bytes,
            created_at: None,
            modified_at,
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
        self.ensure_initialized()?;
        let repo = self.load_repository_record(&request.repo_id)?;
        let repo_root = PathBuf::from(&repo.summary.path);
        let parent_path =
            normalize_directory_path(request.parent_path.as_deref().unwrap_or_default())?;
        let name = validate_new_entry_name(&request.name)?;
        create_backend_directory(&self.root, &repo, &repo_root, &parent_path, &name)?;
        self.load_file_browser(FileBrowserRequest {
            repo_id: request.repo_id,
            directory_path: Some(parent_path),
            include_tree: Some(true),
            special_location: None,
        })
    }

    pub fn create_file(&self, request: FileCreateRequest) -> Result<FileBrowserSnapshot, String> {
        self.ensure_initialized()?;
        let repo = self.load_repository_record(&request.repo_id)?;
        let repo_root = PathBuf::from(&repo.summary.path);
        let parent_path =
            normalize_directory_path(request.parent_path.as_deref().unwrap_or_default())?;
        let name = validate_new_entry_name(&request.name)?;
        create_backend_file(&self.root, &repo, &repo_root, &parent_path, &name)?;
        let _ = self.sync_repository(SyncRequest {
            repo_id: request.repo_id.clone(),
        })?;

        self.load_file_browser(FileBrowserRequest {
            repo_id: request.repo_id,
            directory_path: Some(parent_path),
            include_tree: Some(false),
            special_location: None,
        })
    }

    pub fn import_entries(
        &self,
        request: FileImportRequest,
    ) -> Result<FileBrowserSnapshot, String> {
        self.ensure_initialized()?;
        let repo = self.load_repository_record(&request.repo_id)?;
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
        self.finish_file_copy_operation(&request.repo_id, parent_path, include_tree, outcomes)
    }

    pub fn copy_entries(&self, request: FileCopyRequest) -> Result<FileBrowserSnapshot, String> {
        self.ensure_initialized()?;
        let repo = self.load_repository_record(&request.repo_id)?;
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
        self.finish_file_copy_operation(&request.repo_id, parent_path, include_tree, outcomes)
    }

    pub fn move_entries(&self, request: FileMoveRequest) -> Result<FileBrowserSnapshot, String> {
        self.ensure_initialized()?;
        let repo = self.load_repository_record(&request.repo_id)?;
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
        let mut connection = self.open_repository_connection(
            &repo.summary.repo_id,
            &repo.summary.path,
            &repo.backend_record,
        )?;
        let tx = connection.transaction().map_err(db_error)?;

        for entry in &move_plan {
            let moved = move_backend_entry(
                &self.root,
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

        self.load_file_browser(FileBrowserRequest {
            repo_id: request.repo_id,
            directory_path: Some(parent_path),
            include_tree: Some(include_tree),
            special_location: None,
        })
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
        self.ensure_initialized()?;
        let repo = self.load_repository_record(&request.repo_id)?;
        let repo_root = PathBuf::from(&repo.summary.path);
        let source_path = normalize_entry_path(&request.path)?;
        let new_name = validate_new_entry_name(&request.new_name)?;
        let parent_path = parent_relative_path(&source_path);
        let target_path = join_relative_path(&parent_path, &new_name);
        let renamed = rename_backend_entry(&self.root, &repo, &repo_root, &source_path, &new_name)?;

        let is_directory = matches!(renamed.kind, FileSystemEntryKind::Directory);
        if !is_directory {
            let extension = renamed.extension.unwrap_or_default();
            let modified_at = renamed.modified_at.unwrap_or_else(now_rfc3339);
            let mut connection = self.open_repository_connection(
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
            let mut connection = self.open_repository_connection(
                &repo.summary.repo_id,
                &repo.summary.path,
                &repo.backend_record,
            )?;
            let tx = connection.transaction().map_err(db_error)?;
            rename_directory_asset_records(&tx, &request.repo_id, &source_path, &target_path)
                .map_err(db_error)?;
            tx.commit().map_err(db_error)?;
        }

        self.load_file_browser(FileBrowserRequest {
            repo_id: request.repo_id,
            directory_path: Some(parent_path),
            include_tree: Some(is_directory),
            special_location: None,
        })
    }

    pub fn delete_entry(&self, request: FileDeleteRequest) -> Result<FileBrowserSnapshot, String> {
        self.ensure_initialized()?;
        let repo = self.load_repository_record(&request.repo_id)?;
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
            return self.load_file_browser(FileBrowserRequest {
                repo_id: request.repo_id,
                directory_path: Some(parent_path),
                include_tree: Some(false),
                special_location: Some("trash".to_string()),
            });
        }

        let entry_path = normalize_entry_path(&request.path)?;
        let parent_path = parent_relative_path(&entry_path);
        let entry = stat_backend_entry(&self.root, &repo, &repo_root, &entry_path)?;

        let is_directory = matches!(entry.kind, FileSystemEntryKind::Directory);
        if is_directory {
            if delete_mode == "moveToParent" {
                move_directory_contents_to_parent(
                    &self.root,
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
                let mut connection = self.open_repository_connection(
                    &repo.summary.repo_id,
                    &repo.summary.path,
                    &repo.backend_record,
                )?;
                let tx = connection.transaction().map_err(db_error)?;
                mark_directory_assets_deleted(&tx, &request.repo_id, &entry_path)
                    .map_err(db_error)?;
                tx.commit().map_err(db_error)?;
            }
        } else {
            if repo.backend_record.plugin_id != LOCAL_FILESYSTEM_PLUGIN_ID {
                return Err(format!(
                    "trash delete is only supported for local filesystem repositories, got: {}",
                    repo.backend_record.plugin_id
                ));
            }
            move_entry_to_trash(&repo_root, &entry_path, is_directory)?;
            let mut connection = self.open_repository_connection(
                &repo.summary.repo_id,
                &repo.summary.path,
                &repo.backend_record,
            )?;
            let tx = connection.transaction().map_err(db_error)?;
            mark_file_asset_deleted(&tx, &request.repo_id, &entry_path).map_err(db_error)?;
            tx.commit().map_err(db_error)?;
        }

        self.load_file_browser(FileBrowserRequest {
            repo_id: request.repo_id,
            directory_path: Some(parent_path),
            include_tree: Some(is_directory),
            special_location: None,
        })
    }

    pub fn mutate_trash(
        &self,
        request: TrashMutationRequest,
    ) -> Result<FileBrowserSnapshot, String> {
        self.ensure_initialized()?;
        let repo = self.load_repository_record(&request.repo_id)?;
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

        let _ = self.sync_repository(SyncRequest {
            repo_id: request.repo_id.clone(),
        })?;

        self.load_file_browser(FileBrowserRequest {
            repo_id: request.repo_id,
            directory_path: Some(String::new()),
            include_tree: Some(false),
            special_location: Some("trash".to_string()),
        })
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
        self.ensure_initialized()?;
        Ok(default_plugins(&self.root))
    }

    pub fn set_plugin_enabled(
        &self,
        request: PluginEnabledRequest,
    ) -> Result<PluginMutationResponse, String> {
        self.ensure_initialized()?;
        let registry = plugin_management_registry(&self.root);
        let normalized_plugin_id = registry.normalize_plugin_id(&request.plugin_id);
        let manifest = registry
            .manifest(&normalized_plugin_id)
            .ok_or_else(|| format!("plugin not found: {}", request.plugin_id))?;

        if !request.enabled
            && is_repository_backend_plugin(manifest)
            && self.repository_backend_in_use(&normalized_plugin_id)?
        {
            return Err(format!(
                "plugin is used by an existing repository: {}",
                manifest.plugin_id
            ));
        }

        let mut settings = load_plugin_settings(&self.root)?;
        settings
            .plugins
            .entry(normalized_plugin_id)
            .or_default()
            .enabled = Some(request.enabled);
        save_plugin_settings(&self.root, &settings)?;

        Ok(PluginMutationResponse {
            plugins: default_plugins(&self.root),
        })
    }

    pub fn delete_plugin(&self, plugin_id: String) -> Result<PluginMutationResponse, String> {
        self.ensure_initialized()?;
        let registry = plugin_management_registry(&self.root);
        let normalized_plugin_id = registry.normalize_plugin_id(&plugin_id);
        let registration = registry
            .registration(&normalized_plugin_id)
            .ok_or_else(|| format!("plugin not found: {plugin_id}"))?;
        if registration.manifest.source != "user" {
            return Err(format!(
                "built-in plugins cannot be deleted: {}",
                registration.manifest.plugin_id
            ));
        }
        if is_repository_backend_plugin(&registration.manifest)
            && self.repository_backend_in_use(&normalized_plugin_id)?
        {
            return Err(format!(
                "plugin is used by an existing repository: {}",
                registration.manifest.plugin_id
            ));
        }

        let manifest_dir = registration
            .manifest_dir
            .as_ref()
            .ok_or_else(|| format!("plugin directory is not available: {plugin_id}"))?;
        ensure_user_plugin_dir(&self.root, manifest_dir)?;
        fs::remove_dir_all(manifest_dir).map_err(io_error)?;

        let mut settings = load_plugin_settings(&self.root)?;
        settings.plugins.remove(&normalized_plugin_id);
        save_plugin_settings(&self.root, &settings)?;

        Ok(PluginMutationResponse {
            plugins: default_plugins(&self.root),
        })
    }

    pub fn install_plugin_from_archive(
        &self,
        request: PluginInstallRequest,
    ) -> Result<PluginMutationResponse, String> {
        self.ensure_initialized()?;
        install_plugin_archive(&self.root, Path::new(request.archive_path.trim()))?;

        Ok(PluginMutationResponse {
            plugins: default_plugins(&self.root),
        })
    }

    pub fn get_cache_snapshot(&self) -> Result<CacheSnapshot, String> {
        self.ensure_initialized()?;
        Ok(CacheSnapshot {
            config: CacheConfig {
                metadata_capacity: 2_048,
                thumbnail_capacity: 512,
                query_capacity: 128,
            },
            entries: default_cache_entries(),
        })
    }

    pub fn get_api_design_snapshot(&self) -> Result<ApiDesignSnapshot, String> {
        self.ensure_initialized()?;
        Ok(ApiDesignSnapshot {
            transport: "REST over local repository service, gRPC-ready contract design".to_string(),
            endpoints: default_api_definitions(),
        })
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
                    let backend_config_json: String = row.get(4)?;
                    let backend_config = parse_backend_config_json(&backend_config_json).map_err(to_from_sql_error)?;
                    Ok(RepositoryRecord {
                        summary: RepositorySummary {
                            repo_id: row.get(0)?,
                            name: row.get(1)?,
                            path: row.get(2)?,
                            backend: backend_summary_from_registry(&plugin_registry, &backend_plugin_id),
                            status: row.get(5)?,
                            asset_count: 0,
                            updated_at: row.get(6)?,
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
                let backend_config_json: String = row.get(4)?;
                let backend_config =
                    parse_backend_config_json(&backend_config_json).map_err(to_from_sql_error)?;
                Ok(RepositoryRecord {
                    summary: RepositorySummary {
                        repo_id: row.get(0)?,
                        name: row.get(1)?,
                        path: row.get(2)?,
                        backend: backend_summary_from_registry(
                            &plugin_registry,
                            &backend_plugin_id,
                        ),
                        status: row.get(5)?,
                        asset_count: 0,
                        updated_at: row.get(6)?,
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
        SELECT a.path, a.asset_id, a.status, a.thumbnail_path, hm.group_id, hm.link_state
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
        ))
    })?;

    let mut map = BTreeMap::new();
    for row in rows {
        let (path, asset_id, status, thumbnail_path, hardlink_group_id, hardlink_state) = row?;
        map.insert(
            path,
            AssetPathRecord {
                asset_id,
                status,
                thumbnail_path,
                hardlink_group_id,
                hardlink_state,
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
          hm.link_state
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
    Ok(entries
        .into_iter()
        .map(|entry| (entry.key, entry.value))
        .collect::<BTreeMap<_, _>>())
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
    Ok(map)
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
    Ok(pairs.into_iter().collect::<BTreeMap<_, _>>())
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
    let metadata = load_metadata_entries(connection, asset_id)?;
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
    let metadata = metadata_rows.collect::<Result<Vec<_>, _>>()?;

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
              hm.link_state
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
              hm.link_state
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
        let haystack = build_search_haystack(repo, &asset, &metadata);
        if !query.is_empty() && !haystack.contains(query) {
            continue;
        }
        if let Some(formats) = &request.formats {
            let formats = normalized_filter_values(formats);
            if !formats.is_empty() && !formats.contains(&asset.extension.to_lowercase()) {
                continue;
            }
        }
        if let Some(tag) = &request.tag {
            if !asset
                .tags
                .iter()
                .any(|item| item.to_lowercase().contains(&tag.to_lowercase()))
            {
                continue;
            }
        }
        if let Some(tags) = &request.tags {
            let tags = normalized_filter_values(tags);
            if !tags.is_empty()
                && !asset.tags.iter().any(|item| {
                    let normalized_tag = item.to_lowercase();
                    tags.iter().any(|tag| normalized_tag.contains(tag))
                })
            {
                continue;
            }
        }
        if let Some(metadata_key) = &request.metadata_key {
            let Some(value) = metadata.get(metadata_key) else {
                continue;
            };
            if let Some(expected) = &request.metadata_value {
                if !json_value_to_search_text(value)
                    .to_lowercase()
                    .contains(&expected.to_lowercase())
                {
                    continue;
                }
            }
        }
        if let Some(filters) = &request.metadata_filters {
            if !metadata_filters_match(&metadata, filters) {
                continue;
            }
        }
        if let Some(min_rating) = request.min_rating {
            let rating = metadata
                .get("rating")
                .and_then(|value| value.as_f64())
                .unwrap_or_default();
            if rating < min_rating {
                continue;
            }
        }
        if asset.status == "deleted" {
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
        });
    }

    Ok(results)
}

fn normalized_filter_values(values: &[String]) -> Vec<String> {
    values
        .iter()
        .map(|value| value.trim().to_lowercase())
        .filter(|value| !value.is_empty())
        .collect()
}

fn metadata_filters_match(
    metadata: &BTreeMap<String, serde_json::Value>,
    filters: &[SearchMetadataFilter],
) -> bool {
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

    grouped_filters.into_iter().all(|(key, expected_values)| {
        let Some(actual_value) = metadata.get(&key) else {
            return false;
        };
        let actual_text = json_value_to_search_text(actual_value).to_lowercase();
        expected_values
            .iter()
            .any(|expected| actual_text == *expected || actual_text.contains(expected))
    })
}

fn normalize_smart_folder_filter(filter: SmartFolderFilter) -> SmartFolderFilter {
    SmartFolderFilter {
        query: normalize_optional_text(filter.query),
        path_prefix: normalize_optional_path_prefix(filter.path_prefix),
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
        min_rating: filter.min_rating.filter(|value| *value > 0.0),
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

fn normalize_optional_path_prefix(value: Option<String>) -> Option<String> {
    value
        .and_then(|path| normalize_directory_path(&path).ok())
        .filter(|path| !path.is_empty())
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
        tags: merge_optional_lists(parent.tags, child.tags.clone()),
        formats: merge_optional_lists(parent.formats, child.formats.clone()),
        colors: empty_vec_to_none(colors),
        shapes: empty_vec_to_none(shapes),
        metadata_filters: empty_vec_to_none(metadata_filters),
        min_rating: match (parent.min_rating, child.min_rating) {
            (Some(left), Some(right)) => Some(left.max(right)),
            (Some(left), None) => Some(left),
            (None, Some(right)) => Some(right),
            (None, None) => None,
        },
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
    if let Some(path_prefix) = &filter.path_prefix {
        let prefixes = path_prefix
            .lines()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>();
        if !prefixes.is_empty()
            && !prefixes.iter().all(|prefix| {
                asset.path == *prefix || asset.path.starts_with(&format!("{prefix}/"))
            })
        {
            return false;
        }
    }
    if let Some(query) = &filter.query {
        let haystack = build_search_haystack(repo, asset, metadata);
        if !query
            .lines()
            .map(|value| value.trim().to_lowercase())
            .filter(|value| !value.is_empty())
            .all(|value| haystack.contains(&value))
        {
            return false;
        }
    }
    if let Some(formats) = &filter.formats {
        let formats = normalized_filter_values(formats);
        if !formats.is_empty() && !formats.contains(&asset.extension.to_lowercase()) {
            return false;
        }
    }
    if let Some(tags) = &filter.tags {
        let tags = normalized_filter_values(tags);
        if !tags.is_empty()
            && !asset.tags.iter().any(|item| {
                let normalized_tag = item.to_lowercase();
                tags.iter().any(|tag| normalized_tag.contains(tag))
            })
        {
            return false;
        }
    }
    let metadata_filters = smart_folder_filter_metadata_filters(filter);
    if !metadata_filters.is_empty() && !metadata_filters_match(metadata, &metadata_filters) {
        return false;
    }
    if let Some(min_rating) = filter.min_rating {
        let rating = metadata
            .get("rating")
            .and_then(|value| value.as_f64())
            .unwrap_or_default();
        if rating < min_rating {
            return false;
        }
    }
    true
}

fn query_smart_folder_entries(
    connection: &Connection,
    repo: &RepositorySummary,
    filter: &SmartFolderFilter,
    asset_map: &BTreeMap<String, AssetPathRecord>,
) -> Result<Vec<FileBrowserEntry>, rusqlite::Error> {
    let assets = load_assets(connection, &repo.repo_id)?;
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
            metadata,
        });
    }
    results.sort_by(|left, right| left.path.to_lowercase().cmp(&right.path.to_lowercase()));
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
        SELECT asset_id, path, status, thumbnail_path, size_bytes, created_at, modified_at, hash
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
            },
        ))
    })?;
    let existing = existing_rows.collect::<Result<Vec<_>, _>>()?;
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
            let content_hash = if existing_record.size_bytes == file.size_bytes
                && existing_record.modified_at == file.modified_at
            {
                match existing_record.hash.filter(|hash| is_content_hash(hash)) {
                    Some(hash) => hash,
                    None => file_sha256_hash(&file.absolute_path).map_err(|error| {
                        rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            error,
                        )))
                    })?,
                }
            } else {
                file_sha256_hash(&file.absolute_path).map_err(|error| {
                    rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        error,
                    )))
                })?
            };
            tx.execute(
                r#"
                UPDATE assets
                SET filename = ?3, extension = ?4, size_bytes = ?5, modified_at = ?6, hash = ?7, status = 'synced', updated_at = ?8, thumbnail_path = ?9
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
                    existing_record.thumbnail_path
                ],
            )?;
            if existing_record.status == "deleted" {
                created_events += 1;
            }
            update_hardlink_member_verification(
                tx,
                &repo.summary.repo_id,
                &asset_id,
                &file.relative_path,
                &content_hash,
            )?;
            ensure_default_metadata(
                tx,
                &asset_id,
                &file.filename,
                &file.extension,
                &asset_created_at,
                file.created_at.as_deref(),
                &[],
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
            let content_hash = file_sha256_hash(&file.absolute_path).map_err(|error| {
                rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    error,
                )))
            })?;
            tx.execute(
                r#"
                INSERT INTO assets (
                  asset_id, repo_id, path, filename, extension, size_bytes,
                  created_at, modified_at, hash, status, version, updated_at, thumbnail_path
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'synced', 1, ?10, ?11)
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
                    Option::<String>::None
                ],
            )?;
            if !skip_hardlink_candidate_paths.contains(&file.relative_path) {
                record_hardlink_candidate_for_new_asset(
                    tx,
                    &repo.summary.repo_id,
                    &asset_id,
                    &file.relative_path,
                    &content_hash,
                    file.size_bytes,
                )?;
            }
            let palette = extract_image_palette(&file.absolute_path, &file.extension);
            insert_default_metadata(
                tx,
                &asset_id,
                &file.filename,
                &file.extension,
                &now,
                file.created_at.as_deref(),
                &palette,
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
    filename: &str,
    extension: &str,
    added_to_library_at: &str,
    file_created_at: Option<&str>,
    palette: &[String],
) -> Result<(), rusqlite::Error> {
    ensure_default_metadata(
        tx,
        asset_id,
        filename,
        extension,
        added_to_library_at,
        file_created_at,
        palette,
        true,
    )
}

fn ensure_default_metadata(
    tx: &Transaction<'_>,
    asset_id: &str,
    filename: &str,
    extension: &str,
    added_to_library_at: &str,
    file_created_at: Option<&str>,
    palette: &[String],
    overwrite_existing: bool,
) -> Result<(), rusqlite::Error> {
    let mut defaults = vec![
        ("title".to_string(), serde_json::Value::String(filename.to_string())),
        ("favorite".to_string(), serde_json::Value::Bool(false)),
        ("type".to_string(), serde_json::Value::String(extension.to_string())),
        ("rating".to_string(), serde_json::json!(0)),
        ("comment".to_string(), serde_json::Value::String(String::new())),
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
        params![asset_id, key, infer_value_type(value), value.to_string(), now],
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

    collect_repository_files_recursive(repo_root, repo_root, &mut files)?;
    Ok(files)
}

fn count_repository_directories(repo_root: &Path) -> Result<i64, String> {
    if !repo_root.exists() {
        return Ok(0);
    }

    count_repository_directories_recursive(repo_root)
}

fn count_repository_directories_recursive(current: &Path) -> Result<i64, String> {
    let mut total = 0;
    for entry in fs::read_dir(current).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if is_internal_repository_dir(file_name.as_ref()) {
            continue;
        }

        let metadata = entry.metadata().map_err(io_error)?;
        if metadata.is_dir() {
            total += 1;
            total += count_repository_directories_recursive(&entry.path())?;
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
) -> std::io::Result<()> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if is_internal_repository_dir(file_name.as_ref()) {
            continue;
        }

        let metadata = entry.metadata()?;
        if metadata.is_dir() {
            collect_repository_files_recursive(repo_root, &path, files)?;
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
            absolute_path: path,
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
    if !is_image_extension(&extension) && !is_video_extension(&extension) {
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
        generate_image_thumbnail(&source_path, &thumbnail_path)
    } else {
        generate_video_thumbnail(&source_path, &thumbnail_path)
    };

    match generated {
        Ok(()) => Ok(Some(thumbnail_path.to_string_lossy().to_string())),
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
        let bucket = buckets.entry((red & 0xf8, green & 0xf8, blue & 0xf8)).or_default();
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

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DiscoveredFile {
    absolute_path: PathBuf,
    relative_path: String,
    filename: String,
    extension: String,
    size_bytes: i64,
    created_at: Option<String>,
    modified_at: String,
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
}

impl BackendDiscoveredFile {
    fn into_discovered_file(self, repo_root: &Path) -> Result<DiscoveredFile, String> {
        let relative_path = normalize_entry_path(&self.relative_path)?;
        let absolute_path = self
            .absolute_path
            .map(Ok)
            .unwrap_or_else(|| resolve_repository_relative_path(repo_root, &relative_path))?;
        Ok(DiscoveredFile {
            absolute_path,
            relative_path,
            filename: self.filename,
            extension: self.extension,
            size_bytes: self.size_bytes,
            created_at: self.created_at,
            modified_at: self.modified_at,
        })
    }
}

fn slugify_repo_id(name: &str, path: &str) -> String {
    slugify_ascii_component(&format!("{name}-{path}"))
}

fn asset_id_for_path(repo_id: &str, relative_path: &str) -> String {
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

const BUILTIN_PLUGIN_MANIFESTS: &[&str] = &[
    include_str!("../../plugins/builtin/local-filesystem/manifest.json"),
    include_str!("../../plugins/builtin/webdav/manifest.json"),
    include_str!("../../plugins/builtin/cloud-drive/manifest.json"),
    include_str!("../../plugins/builtin/three-model-preview/manifest.json"),
    include_str!("../../plugins/builtin/media-preview/manifest.json"),
    include_str!("../../plugins/builtin/filesystem-watcher/manifest.json"),
    include_str!("../../plugins/builtin/metadata-provider/manifest.json"),
    include_str!("../../plugins/builtin/vector-index/manifest.json"),
];

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
    manifest_dir: Option<PathBuf>,
}

struct BackendPluginRegistration {
    manifest: PluginManifest,
    manifest_dir: Option<PathBuf>,
    native: Option<NativePlugin>,
    load_error: Option<String>,
}

struct BackendPluginRegistry {
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
                    match load_native_plugin(&manifest, discovered.manifest_dir.as_deref()) {
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
                    manifest_dir: discovered.manifest_dir,
                    native,
                    load_error,
                },
            );
        }

        Self {
            registrations,
            legacy_ids,
        }
    }

    fn list_manifests(&self) -> Vec<PluginManifest> {
        self.registrations
            .values()
            .map(|registration| {
                let mut manifest = registration.manifest.clone();
                if manifest.runtime == "native-dylib"
                    && manifest.enabled
                    && registration.native.is_none()
                    && !embedded_local_filesystem_fallback_enabled(&manifest.plugin_id)
                {
                    manifest.status = "unavailable".to_string();
                }
                manifest
            })
            .collect()
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
            .unwrap_or_else(|| normalized_builtin_plugin_id(trimmed).to_string())
    }

    fn call(
        &self,
        plugin_id: &str,
        method: &str,
        payload: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let normalized = self.normalize_plugin_id(plugin_id);
        let registration = self
            .registrations
            .get(normalized.as_str())
            .ok_or_else(|| format!("unsupported plugin: {plugin_id}"))?;
        if !registration.manifest.enabled {
            return Err(format!(
                "plugin is disabled: {}",
                registration.manifest.plugin_id
            ));
        }
        if let Some(native) = &registration.native {
            return native.call(method, payload);
        }
        if embedded_local_filesystem_fallback_enabled(&registration.manifest.plugin_id) {
            return call_builtin_local_filesystem(method, payload);
        }
        if let Some(error) = &registration.load_error {
            return Err(format!(
                "plugin runtime is not available: {} ({error})",
                registration.manifest.plugin_id
            ));
        }
        Err(format!(
            "plugin runtime is not available: {}",
            registration.manifest.plugin_id
        ))
    }
}

impl NativePlugin {
    fn call(&self, method: &str, payload: serde_json::Value) -> Result<serde_json::Value, String> {
        let request = PluginCallEnvelope {
            method: method.to_string(),
            payload,
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

fn backend_plugin_registry(service_root: &Path) -> BackendPluginRegistry {
    BackendPluginRegistry::load(service_root)
}

fn plugin_management_registry(service_root: &Path) -> BackendPluginRegistry {
    BackendPluginRegistry::load_for_management(service_root)
}

pub fn set_runtime_plugin_resource_dir(resource_dir: PathBuf) {
    let _ = RUNTIME_PLUGIN_RESOURCE_DIR.set(resource_dir);
}

fn load_runtime_plugin_manifests(service_root: &Path) -> Vec<DiscoveredPluginManifest> {
    let mut manifests =
        load_plugin_manifests_from_runtime(runtime_builtin_plugins_dir().as_deref(), cfg!(test));
    if let Ok(mut user_manifests) =
        read_plugin_manifests_from_dir(&user_plugins_dir(service_root), Some("user"))
    {
        manifests.append(&mut user_manifests);
    }
    manifests.sort_by(|left, right| left.manifest.plugin_id.cmp(&right.manifest.plugin_id));
    manifests
}

fn load_plugin_manifests_from_runtime(
    runtime_root: Option<&Path>,
    allow_compiled_fallback: bool,
) -> Vec<DiscoveredPluginManifest> {
    if let Some(runtime_root) = runtime_root {
        return read_plugin_manifests_from_dir(runtime_root, Some("builtin")).unwrap_or_default();
    }

    if !allow_compiled_fallback {
        return Vec::new();
    }

    load_compiled_builtin_plugin_manifests()
        .into_iter()
        .map(|manifest| DiscoveredPluginManifest {
            manifest,
            manifest_dir: None,
        })
        .collect()
}

fn runtime_builtin_plugins_dir() -> Option<PathBuf> {
    let resource_dir = RUNTIME_PLUGIN_RESOURCE_DIR.get().map(PathBuf::as_path);
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf));
    let current_dir = std::env::current_dir().ok();
    runtime_builtin_plugins_dir_from(resource_dir, exe_dir.as_deref(), current_dir.as_deref())
}

fn runtime_builtin_plugins_dir_from(
    resource_dir: Option<&Path>,
    exe_dir: Option<&Path>,
    current_dir: Option<&Path>,
) -> Option<PathBuf> {
    builtin_plugin_dir_candidates(resource_dir, exe_dir, current_dir)
        .into_iter()
        .find(|candidate| candidate.is_dir())
}

fn builtin_plugin_dir_candidates(
    resource_dir: Option<&Path>,
    exe_dir: Option<&Path>,
    current_dir: Option<&Path>,
) -> Vec<PathBuf> {
    let relative_plugin_dir = PathBuf::from("plugins").join("builtin");
    let mut candidates = Vec::new();
    if let Some(dir) = resource_dir {
        candidates.push(dir.join(&relative_plugin_dir));
        return candidates;
    }
    if let Some(dir) = exe_dir {
        candidates.extend([
            dir.join("resources").join(&relative_plugin_dir),
            dir.join(&relative_plugin_dir),
            dir.join("..").join("Resources").join(&relative_plugin_dir),
            dir.join("..").join("resources").join(&relative_plugin_dir),
        ]);
    }
    if let Some(dir) = current_dir {
        candidates.extend([
            dir.join(&relative_plugin_dir),
            dir.join("..").join(&relative_plugin_dir),
        ]);
    }
    candidates
}

fn read_plugin_manifests_from_dir(
    root: &Path,
    source_override: Option<&str>,
) -> Result<Vec<DiscoveredPluginManifest>, String> {
    let mut manifests = Vec::new();
    for entry in fs::read_dir(root).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let manifest_dir = entry.path();
        let manifest_path = manifest_dir.join("manifest.json");
        if !manifest_path.is_file() {
            continue;
        }
        let raw = fs::read_to_string(&manifest_path).map_err(io_error)?;
        manifests.push(DiscoveredPluginManifest {
            manifest: parse_plugin_manifest_with_source(&raw, source_override)?,
            manifest_dir: Some(manifest_dir),
        });
    }
    manifests.sort_by(|left, right| left.manifest.plugin_id.cmp(&right.manifest.plugin_id));
    Ok(manifests)
}

fn load_compiled_builtin_plugin_manifests() -> Vec<PluginManifest> {
    BUILTIN_PLUGIN_MANIFESTS
        .iter()
        .filter_map(|raw| parse_plugin_manifest(raw).ok())
        .collect()
}

fn parse_plugin_manifest(raw: &str) -> Result<PluginManifest, String> {
    parse_plugin_manifest_with_source(raw, None)
}

fn parse_plugin_manifest_with_source(
    raw: &str,
    source_override: Option<&str>,
) -> Result<PluginManifest, String> {
    let mut manifest = serde_json::from_str::<PluginManifest>(raw).map_err(json_error)?;
    manifest.plugin_id = normalized_builtin_plugin_id(&manifest.plugin_id).to_string();
    if let Some(source) = source_override {
        manifest.source = source.to_string();
    }
    manifest.legacy_plugin_ids = plugin_legacy_ids(&manifest);
    manifest.compat.legacy_plugin_ids = plugin_legacy_ids(&manifest);
    if manifest.compat.sdk_version.trim().is_empty() {
        manifest.compat.sdk_version = PLUGIN_SDK_VERSION.to_string();
    }
    Ok(manifest)
}

fn plugin_legacy_ids(manifest: &PluginManifest) -> Vec<String> {
    let mut values = manifest.legacy_plugin_ids.clone();
    values.extend(manifest.compat.legacy_plugin_ids.clone());
    values.sort();
    values.dedup();
    values
}

fn plugin_settings_path(service_root: &Path) -> PathBuf {
    service_root.join("plugin-state.json")
}

fn user_plugins_dir(service_root: &Path) -> PathBuf {
    service_root.join("plugins").join("user")
}

fn load_plugin_settings(service_root: &Path) -> Result<PluginSettings, String> {
    let path = plugin_settings_path(service_root);
    if !path.is_file() {
        return Ok(PluginSettings::default());
    }
    let raw = fs::read_to_string(path).map_err(io_error)?;
    serde_json::from_str::<PluginSettings>(&raw).map_err(json_error)
}

fn save_plugin_settings(service_root: &Path, settings: &PluginSettings) -> Result<(), String> {
    fs::create_dir_all(service_root).map_err(io_error)?;
    let raw = serde_json::to_string_pretty(settings).map_err(json_error)?;
    fs::write(plugin_settings_path(service_root), raw).map_err(io_error)
}

fn apply_plugin_settings(manifest: &mut PluginManifest, settings: &PluginSettings) {
    let Some(entry) = settings.plugins.get(&manifest.plugin_id) else {
        if !manifest.enabled {
            manifest.status = "disabled".to_string();
        }
        return;
    };
    if let Some(enabled) = entry.enabled {
        manifest.enabled = enabled;
        manifest.status = if enabled {
            "ready".to_string()
        } else {
            "disabled".to_string()
        };
    } else if !manifest.enabled {
        manifest.status = "disabled".to_string();
    }
}

fn is_repository_backend_plugin(manifest: &PluginManifest) -> bool {
    matches!(manifest.kind.as_str(), "filesystem" | "webdav" | "cloud")
}

fn ensure_user_plugin_dir(service_root: &Path, plugin_dir: &Path) -> Result<(), String> {
    let user_root = user_plugins_dir(service_root);
    let user_root = user_root.canonicalize().map_err(io_error)?;
    let plugin_dir = plugin_dir.canonicalize().map_err(io_error)?;
    if plugin_dir.starts_with(&user_root) {
        Ok(())
    } else {
        Err(format!(
            "plugin directory is outside the user plugin root: {}",
            plugin_dir.display()
        ))
    }
}

fn install_plugin_archive(
    service_root: &Path,
    archive_path: &Path,
) -> Result<PluginManifest, String> {
    if archive_path.as_os_str().is_empty() {
        return Err("plugin archive path cannot be empty".to_string());
    }
    if !archive_path.is_file() {
        return Err(format!(
            "plugin archive not found: {}",
            archive_path.display()
        ));
    }

    let file = File::open(archive_path).map_err(io_error)?;
    let mut archive = zip::ZipArchive::new(file).map_err(|error| error.to_string())?;
    let (manifest_index, manifest_prefix) = find_zip_plugin_manifest(&mut archive)?;
    let mut manifest_raw = String::new();
    archive
        .by_index(manifest_index)
        .map_err(|error| error.to_string())?
        .read_to_string(&mut manifest_raw)
        .map_err(io_error)?;

    let manifest = parse_plugin_manifest_with_source(&manifest_raw, Some("user"))?;
    if manifest.plugin_id.trim().is_empty() {
        return Err("plugin manifest is missing pluginId".to_string());
    }

    let existing_registry = plugin_management_registry(service_root);
    if existing_registry.manifest(&manifest.plugin_id).is_some() {
        return Err(format!("plugin already exists: {}", manifest.plugin_id));
    }

    let user_root = user_plugins_dir(service_root);
    fs::create_dir_all(&user_root).map_err(io_error)?;
    let install_slug = slugify_ascii_component(&manifest.plugin_id);
    let install_slug = if install_slug.is_empty() {
        "plugin".to_string()
    } else {
        install_slug
    };
    let target_dir = user_root.join(&install_slug);
    if target_dir.exists() {
        return Err(format!(
            "plugin install directory already exists: {}",
            target_dir.display()
        ));
    }
    let staging_dir = user_root.join(format!(".installing-{install_slug}"));
    if staging_dir.exists() {
        fs::remove_dir_all(&staging_dir).map_err(io_error)?;
    }
    fs::create_dir_all(&staging_dir).map_err(io_error)?;

    let extract_result = extract_zip_plugin(&mut archive, &manifest_prefix, &staging_dir);
    if let Err(error) = extract_result {
        let _ = fs::remove_dir_all(&staging_dir);
        return Err(error);
    }

    let staged_manifest = staging_dir.join("manifest.json");
    if !staged_manifest.is_file() {
        let _ = fs::remove_dir_all(&staging_dir);
        return Err("plugin archive did not extract a manifest.json".to_string());
    }
    fs::rename(&staging_dir, &target_dir).map_err(io_error)?;

    Ok(manifest)
}

fn find_zip_plugin_manifest<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
) -> Result<(usize, String), String> {
    let mut fallback = None;
    for index in 0..archive.len() {
        let name = archive
            .by_index(index)
            .map_err(|error| error.to_string())?
            .name()
            .replace('\\', "/")
            .trim_start_matches('/')
            .to_string();
        if name == "manifest.json" {
            return Ok((index, String::new()));
        }
        if let Some(prefix) = name.strip_suffix("/manifest.json") {
            if !prefix.is_empty() && !prefix.split('/').any(|part| part == "..") {
                fallback.get_or_insert((index, format!("{prefix}/")));
            }
        }
    }
    fallback.ok_or_else(|| "plugin archive must contain a manifest.json".to_string())
}

fn extract_zip_plugin<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    manifest_prefix: &str,
    staging_dir: &Path,
) -> Result<(), String> {
    let prefix = manifest_prefix.trim_start_matches('/').replace('\\', "/");
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| error.to_string())?;
        let entry_name = entry
            .name()
            .replace('\\', "/")
            .trim_start_matches('/')
            .to_string();
        if !prefix.is_empty() && !entry_name.starts_with(&prefix) {
            continue;
        }
        let relative_name = if prefix.is_empty() {
            entry_name.as_str()
        } else {
            &entry_name[prefix.len()..]
        };
        if relative_name.is_empty() {
            continue;
        }
        let relative_path = safe_zip_relative_path(relative_name)?;
        let output_path = staging_dir.join(&relative_path);
        if entry.is_dir() {
            fs::create_dir_all(&output_path).map_err(io_error)?;
            continue;
        }
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).map_err(io_error)?;
        }
        let mut output = File::create(&output_path).map_err(io_error)?;
        std::io::copy(&mut entry, &mut output).map_err(io_error)?;
        output.flush().map_err(io_error)?;
    }
    Ok(())
}

fn safe_zip_relative_path(value: &str) -> Result<PathBuf, String> {
    let mut path = PathBuf::new();
    for part in value.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." || part.contains(':') || part.contains('\\') {
            return Err(format!("unsafe plugin archive path: {value}"));
        }
        path.push(part);
    }
    if path.as_os_str().is_empty() {
        Err(format!("unsafe plugin archive path: {value}"))
    } else {
        Ok(path)
    }
}

fn normalized_builtin_plugin_id(plugin_id: &str) -> &str {
    match plugin_id {
        LEGACY_LOCAL_FILESYSTEM_PLUGIN_ID => LOCAL_FILESYSTEM_PLUGIN_ID,
        LEGACY_WEBDAV_PLUGIN_ID => WEBDAV_PLUGIN_ID,
        LEGACY_CLOUD_DRIVE_PLUGIN_ID => CLOUD_DRIVE_PLUGIN_ID,
        "builtin.three-model-preview" => "momobako.preview.three-model",
        "builtin.media-preview" => "momobako.preview.media",
        "builtin.filesystem-watcher" => "momobako.filesystem-watcher",
        "builtin.metadata-provider" => "momobako.metadata-provider",
        "builtin.vector-index" => "momobako.vector-index",
        value => value,
    }
}

fn load_native_plugin(
    manifest: &PluginManifest,
    manifest_dir: Option<&Path>,
) -> Result<NativePlugin, String> {
    let library_name = manifest
        .entry
        .get("library")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            format!(
                "native plugin is missing entry.library: {}",
                manifest.plugin_id
            )
        })?;
    let library_path = native_plugin_library_path(library_name, manifest_dir)
        .ok_or_else(|| format!("native plugin library not found: {library_name}"))?;
    let library = unsafe { libloading::Library::new(&library_path) }.map_err(|error| {
        format!(
            "failed to load plugin library {}: {error}",
            library_path.display()
        )
    })?;
    let (call, free) = unsafe {
        let manifest_fn: libloading::Symbol<PluginManifestFn> = library
            .get(b"momobako_plugin_manifest")
            .map_err(|error| format!("missing momobako_plugin_manifest: {error}"))?;
        let manifest_ptr = manifest_fn();
        if !manifest_ptr.is_null() {
            let free_fn: libloading::Symbol<PluginFreeFn> = library
                .get(b"momobako_plugin_free")
                .map_err(|error| format!("missing momobako_plugin_free: {error}"))?;
            free_fn(manifest_ptr);
        }
        let call = *library
            .get::<PluginCallFn>(b"momobako_plugin_call")
            .map_err(|error| format!("missing momobako_plugin_call: {error}"))?;
        let free = *library
            .get::<PluginFreeFn>(b"momobako_plugin_free")
            .map_err(|error| format!("missing momobako_plugin_free: {error}"))?;
        (call, free)
    };
    Ok(NativePlugin {
        _library: library,
        call,
        free,
    })
}

fn native_plugin_library_path(library_name: &str, manifest_dir: Option<&Path>) -> Option<PathBuf> {
    let file_name = native_plugin_library_file_name(library_name);
    let mut candidates = Vec::new();
    if let Some(dir) = manifest_dir {
        candidates.push(dir.join(&file_name));
        candidates.push(dir.join(library_name).join(&file_name));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            candidates.push(parent.join(&file_name));
        }
    }
    candidates.push(PathBuf::from(&file_name));
    candidates.into_iter().find(|candidate| candidate.is_file())
}

fn native_plugin_library_file_name(library_name: &str) -> String {
    if cfg!(target_os = "windows") {
        format!("{library_name}.dll")
    } else if cfg!(target_os = "macos") {
        format!("lib{library_name}.dylib")
    } else {
        format!("lib{library_name}.so")
    }
}

fn embedded_local_filesystem_fallback_enabled(plugin_id: &str) -> bool {
    cfg!(test) && plugin_id == LOCAL_FILESYSTEM_PLUGIN_ID
}

fn default_plugins(service_root: &Path) -> Vec<PluginManifest> {
    backend_plugin_registry(service_root).list_manifests()
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

fn default_api_definitions() -> Vec<ApiDefinition> {
    vec![
        ApiDefinition {
            group: "Repository API".to_string(),
            method: "GET".to_string(),
            path: "/repositories".to_string(),
            summary: "列出所有仓库。".to_string(),
        },
        ApiDefinition {
            group: "Repository API".to_string(),
            method: "POST".to_string(),
            path: "/repositories".to_string(),
            summary: "创建或导入仓库。".to_string(),
        },
        ApiDefinition {
            group: "Asset API".to_string(),
            method: "GET".to_string(),
            path: "/repositories/{repoId}/assets/{assetId}".to_string(),
            summary: "读取资产详情与元数据。".to_string(),
        },
        ApiDefinition {
            group: "Thumbnail API".to_string(),
            method: "POST".to_string(),
            path: "/repositories/{repoId}/thumbnails:ensure".to_string(),
            summary: "按需复用或生成单个资产缩略图。".to_string(),
        },
        ApiDefinition {
            group: "Preview API".to_string(),
            method: "POST".to_string(),
            path: "/repositories/{repoId}/files:preparePreviewSource".to_string(),
            summary: "为本地文件预览准备流式读取源。".to_string(),
        },
        ApiDefinition {
            group: "Metadata API".to_string(),
            method: "PATCH".to_string(),
            path: "/repositories/{repoId}/assets/{assetId}/metadata".to_string(),
            summary: "带乐观锁更新 metadata。".to_string(),
        },
        ApiDefinition {
            group: "Revision API".to_string(),
            method: "POST".to_string(),
            path: "/repositories/{repoId}/assets/{assetId}:undo".to_string(),
            summary: "回滚到上一版 metadata 状态。".to_string(),
        },
        ApiDefinition {
            group: "Search API".to_string(),
            method: "POST".to_string(),
            path: "/search".to_string(),
            summary: "执行跨仓库结构化搜索。".to_string(),
        },
        ApiDefinition {
            group: "Smart Folder API".to_string(),
            method: "GET".to_string(),
            path: "/repositories/{repoId}/smart-folders".to_string(),
            summary: "列出资源库内的智能文件夹树。".to_string(),
        },
        ApiDefinition {
            group: "Smart Folder API".to_string(),
            method: "POST".to_string(),
            path: "/repositories/{repoId}/smart-folders".to_string(),
            summary: "创建资源库内的智能文件夹模板。".to_string(),
        },
        ApiDefinition {
            group: "Smart Folder API".to_string(),
            method: "POST".to_string(),
            path: "/repositories/{repoId}/smart-folders/{smartFolderId}:query".to_string(),
            summary: "按智能文件夹条件查询虚拟文件列表。".to_string(),
        },
        ApiDefinition {
            group: "Plugin API".to_string(),
            method: "GET".to_string(),
            path: "/plugins".to_string(),
            summary: "列出插件与能力声明。".to_string(),
        },
    ]
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
    if normalized_builtin_plugin_id(backend_plugin_id) == LOCAL_FILESYSTEM_PLUGIN_ID {
        if Path::new(path).is_dir() {
            "ready".to_string()
        } else {
            "missing".to_string()
        }
    } else {
        stored_status.to_string()
    }
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
    let manifest = registry
        .manifest(&normalized_plugin_id)
        .ok_or_else(|| format!("unsupported filesystem backend plugin: {plugin_id}"))?;
    if !["filesystem", "webdav", "cloud"].contains(&manifest.kind.as_str()) {
        return Err(format!(
            "plugin is not a filesystem backend: {}",
            manifest.plugin_id
        ));
    }
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
            "ALTER TABLE repositories ADD COLUMN backend_plugin_id TEXT NOT NULL DEFAULT 'builtin.local-filesystem'",
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

fn migrate_registry_plugin_ids(registry: &Connection) -> Result<(), rusqlite::Error> {
    for (legacy_id, plugin_id) in [
        (
            LEGACY_LOCAL_FILESYSTEM_PLUGIN_ID,
            LOCAL_FILESYSTEM_PLUGIN_ID,
        ),
        (LEGACY_WEBDAV_PLUGIN_ID, WEBDAV_PLUGIN_ID),
        (LEGACY_CLOUD_DRIVE_PLUGIN_ID, CLOUD_DRIVE_PLUGIN_ID),
    ] {
        registry.execute(
            "UPDATE repositories SET backend_plugin_id = ?1 WHERE backend_plugin_id = ?2",
            params![plugin_id, legacy_id],
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
    let normalized_backend_plugin_id = normalized_builtin_plugin_id(backend_plugin_id);
    let metadata_dir = if normalized_backend_plugin_id == LOCAL_FILESYSTEM_PLUGIN_ID {
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
            let entry =
                backend.move_entry(&repo_root, source_path, target_parent_path, &config)?;
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
) -> Result<Vec<FileBrowserEntry>, String> {
    let entries = backend_adapter(service_root, repo).list_directory_entries(
        repo_root,
        current_path,
        &repo.backend_record.config,
    )?;
    Ok(map_file_browser_entries(entries, asset_map, thumbnail_map))
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

fn stat_backend_entry(
    service_root: &Path,
    repo: &RepositoryRecord,
    repo_root: &Path,
    entry_path: &str,
) -> Result<FileSystemEntry, String> {
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
        let entry = entry.map_err(io_error)?;
        let metadata = entry.metadata().map_err(io_error)?;
        if !metadata.is_dir() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().to_string();
        if is_internal_repository_dir(&name) {
            continue;
        }

        let path = name.clone();
        children.push(build_directory_node(repo_root, &path)?);
    }

    children.sort_by(|left, right| left.label.to_lowercase().cmp(&right.label.to_lowercase()));
    Ok(children)
}

fn build_directory_node(repo_root: &Path, relative_path: &str) -> Result<FileTreeNode, String> {
    let abs_path = resolve_repository_relative_path(repo_root, relative_path)?;
    let mut children = Vec::new();

    for entry in fs::read_dir(&abs_path).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let metadata = entry.metadata().map_err(io_error)?;
        if !metadata.is_dir() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().to_string();
        if is_internal_repository_dir(&name) {
            continue;
        }

        let child_path = join_relative_path(relative_path, &name);
        children.push(build_directory_node(repo_root, &child_path)?);
    }

    children.sort_by(|left, right| left.label.to_lowercase().cmp(&right.label.to_lowercase()));
    Ok(FileTreeNode {
        path: relative_path.to_string(),
        label: Path::new(relative_path)
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| relative_path.to_string()),
        children,
    })
}

fn map_file_browser_entries(
    mut entries: Vec<FileSystemEntry>,
    asset_map: &BTreeMap<String, AssetPathRecord>,
    thumbnail_map: &BTreeMap<(String, String), ThumbnailRecord>,
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
                metadata: BTreeMap::new(),
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
                metadata,
            }
        })
        .collect()
}

fn attach_browser_entry_metadata(
    connection: &Connection,
    mut entries: Vec<FileBrowserEntry>,
) -> Result<Vec<FileBrowserEntry>, rusqlite::Error> {
    let asset_ids = entries
        .iter()
        .filter_map(|entry| entry.asset_id.clone())
        .collect::<Vec<_>>();
    let metadata_by_asset = load_metadata_maps_for_assets(connection, &asset_ids)?;

    for entry in &mut entries {
        let Some(asset_id) = &entry.asset_id else {
            continue;
        };
        let Some(metadata) = metadata_by_asset.get(asset_id) else {
            continue;
        };
        let mut merged = metadata.clone();
        merged.extend(entry.metadata.clone());
        entry.metadata = merged;
    }

    Ok(entries)
}

fn local_directory_entries(
    repo_root: &Path,
    current_dir: &Path,
) -> Result<Vec<FileSystemEntry>, String> {
    let mut entries = Vec::new();

    for entry in fs::read_dir(current_dir).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let name = entry.file_name().to_string_lossy().to_string();
        if is_internal_repository_dir(&name) {
            continue;
        }

        let path = entry.path();
        let metadata = entry.metadata().map_err(io_error)?;
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
    use std::time::{SystemTime, UNIX_EPOCH};

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

    #[test]
    fn create_local_repository_creates_metadata_storage_dirs() {
        let workspace = TestWorkspace::new("local-repository-create");
        let service_root = workspace.path("service");
        let repo_root = workspace.path("repo");
        let state = RepositoryState::from_root(service_root);

        state
            .create_repository(RepositoryMutationRequest {
                repo_id: Some("repo-test".to_string()),
                name: "测试资源库".to_string(),
                path: repo_root.to_string_lossy().to_string(),
                backend_plugin_id: Some(LOCAL_FILESYSTEM_PLUGIN_ID.to_string()),
                backend_config: None,
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
    fn plugin_registry_discovers_builtin_manifests_and_legacy_ids() {
        let workspace = TestWorkspace::new("plugin-registry");
        let registry = backend_plugin_registry(&workspace.path("service"));
        let manifests = registry.list_manifests();

        assert!(manifests.iter().any(|manifest| {
            manifest.plugin_id == LOCAL_FILESYSTEM_PLUGIN_ID
                && manifest.runtime == "native-dylib"
                && manifest
                    .legacy_plugin_ids
                    .contains(&LEGACY_LOCAL_FILESYSTEM_PLUGIN_ID.to_string())
        }));
        assert!(manifests.iter().any(|manifest| {
            manifest.plugin_id == "momobako.preview.media" && manifest.sdk == "frontend"
        }));
        assert_eq!(
            registry.normalize_plugin_id(LEGACY_LOCAL_FILESYSTEM_PLUGIN_ID),
            LOCAL_FILESYSTEM_PLUGIN_ID
        );
        assert_eq!(
            registry.normalize_plugin_id("builtin.three-model-preview"),
            "momobako.preview.three-model"
        );
    }

    #[test]
    fn release_plugin_manifest_loading_does_not_fall_back_to_compiled_manifests() {
        let manifests = load_plugin_manifests_from_runtime(None, false);

        assert!(manifests.is_empty());
    }

    #[test]
    fn runtime_manifest_scan_reflects_deleted_plugin_directories() {
        let workspace = TestWorkspace::new("runtime-plugin-scan");
        let plugin_root = workspace.path("plugins").join("builtin");
        fs::create_dir_all(plugin_root.join("local-filesystem"))
            .expect("runtime plugin dir should be created");
        fs::write(
            plugin_root.join("local-filesystem").join("manifest.json"),
            include_str!("../../plugins/builtin/local-filesystem/manifest.json"),
        )
        .expect("runtime manifest should be written");

        let manifests = load_plugin_manifests_from_runtime(Some(&plugin_root), false);
        assert_eq!(manifests.len(), 1);
        assert_eq!(manifests[0].manifest.plugin_id, LOCAL_FILESYSTEM_PLUGIN_ID);

        fs::remove_dir_all(plugin_root.join("local-filesystem"))
            .expect("runtime plugin dir should be removable");
        let manifests = load_plugin_manifests_from_runtime(Some(&plugin_root), false);
        assert!(manifests.is_empty());
    }

    #[test]
    fn bundled_plugin_dir_candidates_do_not_use_tauri_up_resource_path() {
        let resource_dir = Path::new("C:/Apps/MomoBako/resources");
        let exe_dir = Path::new("C:/Apps/MomoBako");
        let cwd = Path::new("C:/Workspace/MomoBako/src-tauri");
        let candidates =
            builtin_plugin_dir_candidates(Some(resource_dir), Some(exe_dir), Some(cwd));

        assert_eq!(candidates.len(), 1);
        assert_eq!(
            candidates[0],
            PathBuf::from("C:/Apps/MomoBako/resources")
                .join("plugins")
                .join("builtin")
        );
        assert!(candidates
            .iter()
            .all(|path| !path.to_string_lossy().contains("_up_")));
    }

    #[test]
    fn tauri_config_bundles_staged_plugin_resources() {
        let config: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json"))
            .expect("tauri config should parse");
        let resources = config
            .pointer("/bundle/resources")
            .and_then(serde_json::Value::as_object)
            .expect("tauri config should declare bundle resources");

        assert_eq!(
            resources
                .get("resources/plugins/builtin/")
                .and_then(serde_json::Value::as_str),
            Some("plugins/builtin/")
        );
        assert!(resources.keys().all(|path| {
            !path.contains("..") && !path.contains("_up_") && !path.contains("\\_up_")
        }));
    }

    #[test]
    fn resource_plugin_dir_does_not_fallback_to_cwd_when_missing() {
        let workspace = TestWorkspace::new("resource-plugin-dir-strict");
        let resource_dir = workspace.path("resources");
        let cwd = workspace.path("workspace");
        let cwd_plugin_dir = cwd.join("plugins").join("builtin");
        fs::create_dir_all(&resource_dir).expect("resource dir should be created");
        fs::create_dir_all(&cwd_plugin_dir).expect("cwd plugin dir should be created");

        assert_eq!(
            runtime_builtin_plugins_dir_from(Some(&resource_dir), None, Some(&cwd)),
            None
        );
    }

    #[test]
    fn resource_plugin_dir_uses_resource_plugins_when_present() {
        let workspace = TestWorkspace::new("resource-plugin-dir-present");
        let resource_dir = workspace.path("resources");
        let resource_plugin_dir = resource_dir.join("plugins").join("builtin");
        fs::create_dir_all(&resource_plugin_dir).expect("resource plugin dir should be created");

        assert_eq!(
            runtime_builtin_plugins_dir_from(Some(&resource_dir), None, None),
            Some(resource_plugin_dir)
        );
    }

    #[test]
    fn set_plugin_enabled_persists_plugin_state() {
        let workspace = TestWorkspace::new("plugin-enabled-state");
        let service_root = workspace.path("service");
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
        let state = RepositoryState::from_root(workspace.path("service"));

        let error = state
            .delete_plugin("momobako.preview.media".to_string())
            .expect_err("built-in plugins should not be deleted");

        assert!(error.contains("built-in plugins cannot be deleted"));
    }

    #[test]
    fn install_plugin_from_archive_loads_and_deletes_user_plugin() {
        let workspace = TestWorkspace::new("plugin-archive-install");
        let service_root = workspace.path("service");
        let archive_path = workspace.path("sample-plugin.zip");
        write_test_plugin_archive(&archive_path, "user.sample-metadata");
        let state = RepositoryState::from_root(service_root.clone());

        let response = state
            .install_plugin_from_archive(PluginInstallRequest {
                archive_path: archive_path.to_string_lossy().to_string(),
            })
            .expect("plugin archive should install");
        let installed = response
            .plugins
            .iter()
            .find(|plugin| plugin.plugin_id == "user.sample-metadata")
            .expect("installed plugin should be listed");
        assert_eq!(installed.source, "user");
        assert!(installed.enabled);
        assert!(user_plugins_dir(&service_root)
            .join("user-sample-metadata")
            .join("manifest.json")
            .is_file());

        let response = state
            .delete_plugin("user.sample-metadata".to_string())
            .expect("user plugin should be deleted");
        assert!(!response
            .plugins
            .iter()
            .any(|plugin| plugin.plugin_id == "user.sample-metadata"));
        assert!(!user_plugins_dir(&service_root)
            .join("user-sample-metadata")
            .exists());
    }

    fn write_test_plugin_archive(path: &Path, plugin_id: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("plugin archive parent should be created");
        }
        let file = File::create(path).expect("plugin archive should be created");
        let mut archive = zip::ZipWriter::new(file);
        archive
            .start_file(
                "sample-plugin/manifest.json",
                zip::write::SimpleFileOptions::default(),
            )
            .expect("manifest entry should start");
        archive
            .write_all(
                serde_json::to_string_pretty(&serde_json::json!({
                    "pluginId": plugin_id,
                    "legacyPluginIds": [],
                    "name": "Sample Metadata",
                    "version": "0.1.0",
                    "kind": "metadata",
                    "description": "Test plugin installed from archive.",
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
    fn native_plugin_library_path_prefers_manifest_directory() {
        let workspace = TestWorkspace::new("native-plugin-path");
        let plugin_dir = workspace
            .path("plugins")
            .join("builtin")
            .join("local-filesystem");
        fs::create_dir_all(&plugin_dir).expect("plugin dir should be created");
        let library_name = "momobako_builtin_local_filesystem";
        let library_path = plugin_dir.join(native_plugin_library_file_name(library_name));
        fs::write(&library_path, b"test").expect("library file should be written");

        assert_eq!(
            native_plugin_library_path(library_name, Some(&plugin_dir)),
            Some(library_path)
        );
    }

    #[test]
    fn disabled_manifest_only_backend_is_not_attachable() {
        let workspace = TestWorkspace::new("disabled-backend");
        let state = RepositoryState::from_root(workspace.path("service"));
        let repo_root = workspace.path("repo");

        let error = state
            .create_repository(RepositoryMutationRequest {
                repo_id: Some("repo-webdav".to_string()),
                name: "WebDAV Repo".to_string(),
                path: repo_root.to_string_lossy().to_string(),
                backend_plugin_id: Some(WEBDAV_PLUGIN_ID.to_string()),
                backend_config: None,
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
        let state = RepositoryState::from_root(service_root);

        let repo_id = state
            .create_repository(RepositoryMutationRequest {
                repo_id: Some("repo-runtime".to_string()),
                name: "Runtime Repo".to_string(),
                path: repo_root.to_string_lossy().to_string(),
                backend_plugin_id: Some(LEGACY_LOCAL_FILESYSTEM_PLUGIN_ID.to_string()),
                backend_config: None,
            })
            .expect("legacy local filesystem id should migrate and create")
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
    fn ensure_initialized_migrates_registry_plugin_ids() {
        let workspace = TestWorkspace::new("registry-plugin-id-migration");
        let service_root = workspace.path("service");
        fs::create_dir_all(&service_root).expect("service root should be created");
        let registry_path = service_root.join(REGISTRY_FILE_NAME);
        let registry = Connection::open(&registry_path).expect("registry should open");
        registry
            .execute_batch(REGISTRY_SCHEMA_SQL)
            .expect("registry schema should initialize");
        registry
            .execute(
                r#"
                INSERT INTO repositories (
                  repo_id, name, path, backend_plugin_id, backend_config_json, status, created_at, updated_at
                )
                VALUES (?1, ?2, ?3, ?4, '{}', 'ready', ?5, ?5)
                "#,
                params![
                    "repo-legacy",
                    "Legacy Repo",
                    workspace.path("repo").to_string_lossy().to_string(),
                    LEGACY_LOCAL_FILESYSTEM_PLUGIN_ID,
                    now_rfc3339()
                ],
            )
            .expect("legacy registry row should insert");
        drop(registry);

        let state = RepositoryState::from_root(service_root.clone());
        state
            .ensure_initialized()
            .expect("state should initialize and migrate registry IDs");

        let registry = Connection::open(registry_path).expect("registry should reopen");
        let plugin_id: String = registry
            .query_row(
                "SELECT backend_plugin_id FROM repositories WHERE repo_id = ?1",
                ["repo-legacy"],
                |row| row.get(0),
            )
            .expect("plugin id should load");
        assert_eq!(plugin_id, LOCAL_FILESYSTEM_PLUGIN_ID);
    }

    #[test]
    fn import_repository_migrates_repository_metadata_plugin_id() {
        let workspace = TestWorkspace::new("metadata-plugin-id-migration");
        let service_root = workspace.path("service");
        let repo_root = workspace.path("repo");
        let meta_dir = repo_root.join(REPO_META_DIR);
        fs::create_dir_all(&meta_dir).expect("metadata dir should be created");
        let metadata_path = meta_dir.join(REPO_METADATA_FILE_NAME);
        fs::write(
            &metadata_path,
            serde_json::to_string_pretty(&serde_json::json!({
                "repoId": "repo-metadata-legacy",
                "name": "Metadata Legacy",
                "rootPath": repo_root.to_string_lossy(),
                "backendPluginId": LEGACY_LOCAL_FILESYSTEM_PLUGIN_ID,
                "backendConfig": {},
                "createdAt": now_rfc3339(),
                "schemaVersion": REPO_SCHEMA_VERSION,
            }))
            .expect("metadata json should encode"),
        )
        .expect("legacy metadata should be written");

        let state = RepositoryState::from_root(service_root);
        state
            .import_repository(RepositoryMutationRequest {
                repo_id: None,
                name: "Metadata Legacy".to_string(),
                path: repo_root.to_string_lossy().to_string(),
                backend_plugin_id: Some(LOCAL_FILESYSTEM_PLUGIN_ID.to_string()),
                backend_config: None,
            })
            .expect("legacy metadata repository should import");

        let raw = fs::read_to_string(metadata_path).expect("metadata should read");
        let migrated: RepositoryMetadataFileImport =
            serde_json::from_str(&raw).expect("metadata should parse after migration");
        assert_eq!(
            migrated.backend_plugin_id.as_deref(),
            Some(LOCAL_FILESYSTEM_PLUGIN_ID)
        );
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
            relocated_root.join(REPO_META_DIR).join(REPO_METADATA_FILE_NAME),
        )
        .expect("relocated metadata should read");
        let metadata: RepositoryMetadataFileImport =
            serde_json::from_str(&raw_metadata).expect("relocated metadata should parse");
        let expected_root_path = relocated_root.to_string_lossy().to_string();
        assert_eq!(metadata.repo_id, repo_id);
        assert_eq!(metadata.root_path.as_deref(), Some(expected_root_path.as_str()));
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
        let response = state
            .create_repository(RepositoryMutationRequest {
                repo_id: None,
                name: "Test Repo".to_string(),
                path: repo_root.to_string_lossy().to_string(),
                backend_plugin_id: None,
                backend_config: None,
            })
            .expect("repository should be created");
        response.repository.repo_id
    }

    fn create_repository_without_initial_sync(state: &RepositoryState, repo_root: &Path) -> String {
        let repo_id = format!(
            "repo-{}",
            slugify_repo_id("test", &repo_root.to_string_lossy())
        );
        state
            .ensure_initialized()
            .expect("repository state should initialize");
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

    fn write_test_image(path: &Path) {
        let image = image::RgbImage::from_pixel(2, 2, image::Rgb([120, 120, 120]));
        image.save(path).expect("test image should be saved");
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
        image.save(path).expect("test palette image should be saved");
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
            repo_root.join("notes").join("today.txt")
        );
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
        assert_eq!(entry.metadata.get("tagGroups"), Some(&serde_json::json!([])));
        assert!(entry
            .metadata
            .get("addedToLibraryAt")
            .and_then(serde_json::Value::as_str)
            .is_some());

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
                metadata_key: None,
                metadata_value: None,
                tag: None,
                tags: Some(vec!["封面".to_string(), "主视觉".to_string()]),
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
            })
            .expect("filtered search should complete");

        assert_eq!(response.results.len(), 1);
        assert_eq!(response.results[0].path, "cover.psd");

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
        assert!(snapshot.entries.iter().any(|entry| entry.path == "Archive/note.txt"));

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
        fs::create_dir_all(repo_root.join("Scenes/Act1"))
            .expect("nested folder should be created");
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
        fs::write(&thumbnail_path, b"cached").expect("cached thumbnail should be written");

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
}
