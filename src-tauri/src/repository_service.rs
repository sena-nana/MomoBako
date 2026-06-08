use rusqlite::{params, types::Type, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
    process::Command,
    sync::{Mutex, OnceLock},
    time::SystemTime,
};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

const REGISTRY_FILE_NAME: &str = "repositories.db";
const REPO_META_DIR: &str = ".momo";
const LEGACY_REPO_META_DIR: &str = ".meta";
const REPO_METADATA_FILE_NAME: &str = "repository.json";
const REPO_DB_FILE_NAME: &str = "metadata.db";
const REPO_SCHEMA_VERSION: i64 = 1;
const THUMBNAIL_SIZE: u32 = 256;
const THUMBNAIL_CACHE_PREFIX_CHARS: usize = 48;
const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

static FFMPEG_READY: OnceLock<Result<(), String>> = OnceLock::new();

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

const LOCAL_FILESYSTEM_PLUGIN_ID: &str = "builtin.local-filesystem";
const WEBDAV_PLUGIN_ID: &str = "builtin.webdav";
const CLOUD_DRIVE_PLUGIN_ID: &str = "builtin.cloud-drive";

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

#[derive(Debug, Serialize, Clone)]
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
    backend_plugin_id: Option<String>,
    backend_config: Option<serde_json::Value>,
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
pub struct FileBrowserRequest {
    pub repo_id: String,
    pub directory_path: Option<String>,
    pub include_tree: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileReadRequest {
    pub repo_id: String,
    pub path: String,
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
pub struct FileRenameRequest {
    pub repo_id: String,
    pub path: String,
    pub new_name: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDeleteRequest {
    pub repo_id: String,
    pub path: String,
    pub mode: Option<String>,
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
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThumbnailResponse {
    pub repo_id: String,
    pub path: String,
    pub asset_id: String,
    pub thumbnail_path: Option<String>,
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

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchRequest {
    pub query: String,
    pub repo_id: Option<String>,
    pub metadata_key: Option<String>,
    pub metadata_value: Option<String>,
    pub tag: Option<String>,
    pub min_rating: Option<f64>,
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

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PluginManifest {
    pub plugin_id: String,
    pub name: String,
    pub version: String,
    pub kind: String,
    pub description: String,
    pub capabilities: Vec<String>,
    pub enabled: bool,
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
struct FileSystemPluginDescriptor {
    plugin_id: &'static str,
    kind: &'static str,
    name: &'static str,
    capabilities: &'static [&'static str],
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

    fn delete_entry(
        &self,
        repo_root: &Path,
        entry_path: &str,
        recursive: bool,
        config: &serde_json::Value,
    ) -> Result<(), String>;
}

#[derive(Debug, Clone)]
enum FileSystemEntryKind {
    Directory,
    File,
}

#[derive(Debug, Clone)]
struct FileSystemEntry {
    path: String,
    name: String,
    kind: FileSystemEntryKind,
    extension: Option<String>,
    size_bytes: Option<i64>,
    modified_at: Option<String>,
}

struct LocalFileSystemBackend;
struct UnsupportedFileSystemBackend {
    descriptor: &'static FileSystemPluginDescriptor,
}

pub struct RepositoryState {
    root: PathBuf,
    thumbnail_root: PathBuf,
    registry_path: PathBuf,
    initialized: Mutex<bool>,
}

impl RepositoryState {
    pub fn from_roots(root: PathBuf, thumbnail_root: PathBuf) -> Self {
        let registry_path = root.join(REGISTRY_FILE_NAME);
        Self {
            root,
            thumbnail_root,
            registry_path,
            initialized: Mutex::new(false),
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

        let rows = stmt
            .query_map([], |row| {
                let repo_id: String = row.get(0)?;
                let path: String = row.get(2)?;
                let backend_plugin_id: String = row.get(3)?;
                let asset_count =
                    load_asset_count(&self.root, &repo_id, &path, &backend_plugin_id).unwrap_or(0);

                Ok(RepositorySummary {
                    repo_id,
                    name: row.get(1)?,
                    path,
                    backend: backend_summary(&backend_plugin_id),
                    status: row.get(5)?,
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

        let backend = parse_backend_request(&request)?;
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

        let requested_backend = parse_backend_request(&request)?;
        let repo_root =
            normalize_repository_root_for_backend(&request.path, &requested_backend, true)?;
        migrate_legacy_meta_dir_if_needed(&repo_root, &requested_backend.plugin_id)?;
        let metadata_path = repository_meta_dir(&repo_root).join(REPO_METADATA_FILE_NAME);
        let imported_metadata = if metadata_path.exists() {
            let raw = fs::read_to_string(&metadata_path).map_err(io_error)?;
            Some(serde_json::from_str::<RepositoryMetadataFileImport>(&raw).map_err(json_error)?)
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
            .and_then(import_backend_record)
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

        let backend = parse_backend_request(&RepositoryMutationRequest {
            repo_id: None,
            name: String::new(),
            path: path.to_string(),
            backend_plugin_id: None,
            backend_config: None,
        })?;
        let repo_root = normalize_repository_root_for_backend(path, &backend, true)?;
        ensure_backend_path_is_attachable(&backend, &repo_root)?;
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
        Ok(())
    }

    pub fn export_repository(&self, repo_id: &str) -> Result<RepositoryMutationResponse, String> {
        self.ensure_initialized()?;
        let repository = self.load_repository_record(repo_id)?.summary;
        Ok(RepositoryMutationResponse { repository })
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

        let folders = load_folder_summaries(&connection, repo_id).map_err(db_error)?;
        let assets = load_assets(&connection, repo_id).map_err(db_error)?;
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
        let current_path =
            normalize_directory_path(request.directory_path.as_deref().unwrap_or_default())?;
        let connection = self.open_repository_connection(
            &repo.summary.repo_id,
            &repo.summary.path,
            &repo.backend_record,
        )?;
        let asset_map = load_asset_path_map(&connection, &request.repo_id).map_err(db_error)?;
        let tree = if request.include_tree.unwrap_or(true) {
            Some(list_backend_tree(&repo, &repo_root)?)
        } else {
            None
        };
        let entries = list_backend_directory_entries(&repo, &repo_root, &current_path, &asset_map)?;

        Ok(FileBrowserSnapshot {
            repo_id: request.repo_id,
            root_path: repo.summary.path,
            backend_plugin_id: repo.backend_record.plugin_id.clone(),
            backend_kind: repo.summary.backend.kind,
            current_path,
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

    pub fn search_assets(&self, request: SearchRequest) -> Result<SearchResponse, String> {
        self.ensure_initialized()?;

        let normalized_query = request.query.trim().to_lowercase();
        if normalized_query.is_empty()
            && request.tag.is_none()
            && request.metadata_key.is_none()
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
        self.ensure_initialized()?;
        let repo = self.load_repository_record(&request.repo_id)?;
        let mut connection = self.open_repository_connection(
            &repo.summary.repo_id,
            &repo.summary.path,
            &repo.backend_record,
        )?;
        let tx = connection.transaction().map_err(db_error)?;

        let scan = sync_repository_files(&tx, &repo).map_err(db_error)?;
        tx.commit().map_err(db_error)?;
        Ok(scan)
    }

    pub fn ensure_thumbnail(&self, request: ThumbnailRequest) -> Result<ThumbnailResponse, String> {
        self.ensure_initialized()?;
        let repo = self.load_repository_record(&request.repo_id)?;
        let entry_path = normalize_entry_path(&request.path)?;
        let repo_root = PathBuf::from(&repo.summary.path);
        let connection = self.open_repository_connection(
            &repo.summary.repo_id,
            &repo.summary.path,
            &repo.backend_record,
        )?;

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
        let file = DiscoveredFile {
            relative_path: entry_path.clone(),
            filename,
            extension,
            size_bytes,
            modified_at,
        };
        let thumbnail_path = ensure_thumbnail_for_file(
            &repo,
            &repo_root,
            &self.thumbnail_root,
            &asset_id,
            &file,
            existing_thumbnail_path,
        )?;

        if let Some(path) = &thumbnail_path {
            connection
                .execute(
                    r#"
                    UPDATE assets
                    SET thumbnail_path = ?3, updated_at = ?4
                    WHERE repo_id = ?1 AND asset_id = ?2
                    "#,
                    params![&request.repo_id, &asset_id, path, now_rfc3339()],
                )
                .map_err(db_error)?;
        }

        Ok(ThumbnailResponse {
            repo_id: request.repo_id,
            path: entry_path,
            asset_id,
            thumbnail_path,
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
        create_backend_directory(&repo, &repo_root, &parent_path, &name)?;
        self.load_file_browser(FileBrowserRequest {
            repo_id: request.repo_id,
            directory_path: Some(parent_path),
            include_tree: Some(true),
        })
    }

    pub fn create_file(&self, request: FileCreateRequest) -> Result<FileBrowserSnapshot, String> {
        self.ensure_initialized()?;
        let repo = self.load_repository_record(&request.repo_id)?;
        let repo_root = PathBuf::from(&repo.summary.path);
        let parent_path =
            normalize_directory_path(request.parent_path.as_deref().unwrap_or_default())?;
        let name = validate_new_entry_name(&request.name)?;
        create_backend_file(&repo, &repo_root, &parent_path, &name)?;
        let _ = self.sync_repository(SyncRequest {
            repo_id: request.repo_id.clone(),
        })?;

        self.load_file_browser(FileBrowserRequest {
            repo_id: request.repo_id,
            directory_path: Some(parent_path),
            include_tree: Some(false),
        })
    }

    pub fn import_entries(
        &self,
        request: FileImportRequest,
    ) -> Result<FileBrowserSnapshot, String> {
        self.ensure_initialized()?;
        let repo = self.load_repository_record(&request.repo_id)?;
        if repo.backend_record.plugin_id != LOCAL_FILESYSTEM_PLUGIN_ID {
            return Err(
                "importing files is only supported for local filesystem repositories".to_string(),
            );
        }

        let repo_root = PathBuf::from(&repo.summary.path);
        let parent_path =
            normalize_directory_path(request.parent_path.as_deref().unwrap_or_default())?;
        let target_dir = resolve_repository_relative_path(&repo_root, &parent_path)?;
        if !target_dir.exists() || !target_dir.is_dir() {
            return Err(format!("directory not found: {parent_path}"));
        }
        if request.source_paths.is_empty() {
            return Err("no source files were provided".to_string());
        }

        let mut imported_directory = false;
        for source_path in &request.source_paths {
            let source = PathBuf::from(source_path);
            copy_external_entry_into_directory(&source, &repo_root, &target_dir)?;
            if source.is_dir() {
                imported_directory = true;
            }
        }

        let _ = self.sync_repository(SyncRequest {
            repo_id: request.repo_id.clone(),
        })?;

        self.load_file_browser(FileBrowserRequest {
            repo_id: request.repo_id,
            directory_path: Some(parent_path),
            include_tree: Some(imported_directory),
        })
    }

    pub fn rename_entry(&self, request: FileRenameRequest) -> Result<FileBrowserSnapshot, String> {
        self.ensure_initialized()?;
        let repo = self.load_repository_record(&request.repo_id)?;
        let repo_root = PathBuf::from(&repo.summary.path);
        let source_path = normalize_entry_path(&request.path)?;
        let new_name = validate_new_entry_name(&request.new_name)?;
        let parent_path = parent_relative_path(&source_path);
        let target_path = join_relative_path(&parent_path, &new_name);
        let renamed = rename_backend_entry(&repo, &repo_root, &source_path, &new_name)?;

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
        })
    }

    pub fn delete_entry(&self, request: FileDeleteRequest) -> Result<FileBrowserSnapshot, String> {
        self.ensure_initialized()?;
        let repo = self.load_repository_record(&request.repo_id)?;
        let repo_root = PathBuf::from(&repo.summary.path);
        let entry_path = normalize_entry_path(&request.path)?;
        let parent_path = parent_relative_path(&entry_path);
        let entry = stat_backend_entry(&repo, &repo_root, &entry_path)?;

        let is_directory = matches!(entry.kind, FileSystemEntryKind::Directory);
        if is_directory {
            let delete_mode = request.mode.as_deref().unwrap_or("delete");
            if delete_mode == "moveToParent" {
                move_directory_contents_to_parent(
                    &self.root,
                    &repo,
                    &repo_root,
                    &request.repo_id,
                    &entry_path,
                )?;
            } else {
                delete_backend_entry(&repo, &repo_root, &entry_path, true)?;
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
            delete_backend_entry(&repo, &repo_root, &entry_path, false)?;
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
        Ok(default_plugins())
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
                    let backend_config_json: String = row.get(4)?;
                    let backend_config = parse_backend_config_json(&backend_config_json).map_err(to_from_sql_error)?;
                    Ok(RepositoryRecord {
                        summary: RepositorySummary {
                            repo_id: row.get(0)?,
                            name: row.get(1)?,
                            path: row.get(2)?,
                            backend: backend_summary(&backend_plugin_id),
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
        let rows = stmt
            .query_map([], |row| {
                let backend_plugin_id: String = row.get(3)?;
                let backend_config_json: String = row.get(4)?;
                let backend_config =
                    parse_backend_config_json(&backend_config_json).map_err(to_from_sql_error)?;
                Ok(RepositoryRecord {
                    summary: RepositorySummary {
                        repo_id: row.get(0)?,
                        name: row.get(1)?,
                        path: row.get(2)?,
                        backend: backend_summary(&backend_plugin_id),
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
) -> Result<BTreeMap<String, (String, String, Option<String>)>, rusqlite::Error> {
    let mut stmt = connection.prepare(
        r#"
        SELECT path, asset_id, status, thumbnail_path
        FROM assets
        WHERE repo_id = ?1
        "#,
    )?;

    let rows = stmt.query_map([repo_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, Option<String>>(3)?,
        ))
    })?;

    let mut map = BTreeMap::new();
    for row in rows {
        let (path, asset_id, status, thumbnail_path) = row?;
        map.insert(path, (asset_id, status, thumbnail_path));
    }
    Ok(map)
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
        SELECT asset_id, repo_id, path, filename, extension, size_bytes, status, modified_at, version, thumbnail_path
        FROM assets
        WHERE repo_id = ?1 AND status != 'deleted'
        ORDER BY modified_at DESC, filename COLLATE NOCASE
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
            SELECT asset_id, repo_id, path, filename, extension, size_bytes, status, modified_at, version, thumbnail_path
            FROM assets
            WHERE repo_id = ?1 AND asset_id = ?2
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
                ))
            },
        )
        .optional()?
        .map(
            |(asset_id, repo_id, path, filename, extension, size_bytes, status, modified_at, version, thumbnail_path)| {
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
            SELECT asset_id, repo_id, path, filename, extension, size_bytes, status, modified_at, version, thumbnail_path
            FROM assets
            WHERE repo_id = ?1 AND asset_id = ?2
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
        if let Some(tag) = &request.tag {
            if !asset
                .tags
                .iter()
                .any(|item| item.to_lowercase().contains(&tag.to_lowercase()))
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
    tx: &Transaction<'_>,
    repo: &RepositoryRecord,
) -> Result<SyncResult, rusqlite::Error> {
    let repo_root = PathBuf::from(&repo.summary.path);
    let files = list_backend_files(repo, &repo_root).map_err(|error| {
        rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
            std::io::ErrorKind::Other,
            error,
        )))
    })?;

    let mut existing_stmt = tx.prepare(
        r#"
        SELECT asset_id, path, status, thumbnail_path
        FROM assets
        WHERE repo_id = ?1
        "#,
    )?;
    let existing_rows = existing_stmt.query_map([repo.summary.repo_id.as_str()], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, Option<String>>(3)?,
        ))
    })?;
    let existing = existing_rows.collect::<Result<Vec<_>, _>>()?;
    let mut existing_by_path = existing
        .into_iter()
        .map(|(asset_id, path, status, thumbnail_path)| (path, (asset_id, status, thumbnail_path)))
        .collect::<BTreeMap<_, _>>();

    let now = now_rfc3339();
    let mut created_assets = 0_i64;
    let mut updated_assets = 0_i64;
    let mut deleted_assets = 0_i64;
    let mut created_events = 0_i64;

    for file in &files {
        if let Some((asset_id, previous_status, existing_thumbnail_path)) =
            existing_by_path.remove(&file.relative_path)
        {
            tx.execute(
                r#"
                UPDATE assets
                SET filename = ?3, extension = ?4, size_bytes = ?5, modified_at = ?6, status = 'synced', updated_at = ?7, thumbnail_path = ?8
                WHERE repo_id = ?1 AND asset_id = ?2
                "#,
                params![
                    repo.summary.repo_id,
                    asset_id,
                    file.filename,
                    file.extension,
                    file.size_bytes,
                    file.modified_at,
                    now,
                    existing_thumbnail_path
                ],
            )?;
            if previous_status == "deleted" {
                created_events += 1;
            }
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
            let asset_id = format!(
                "asset-{}",
                slugify_repo_id(&file.filename, &file.relative_path)
            );
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
                    format!("sha256:{}", safe_prefix(&asset_id, 18)),
                    now,
                    Option::<String>::None
                ],
            )?;
            insert_default_metadata(
                tx,
                &asset_id,
                &file.filename,
                &file.extension,
                &file.modified_at,
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

    for (path, (asset_id, status, _thumbnail_path)) in existing_by_path {
        if status == "deleted" {
            continue;
        }
        tx.execute(
            r#"
            UPDATE assets
            SET status = 'deleted', updated_at = ?3
            WHERE repo_id = ?1 AND asset_id = ?2
            "#,
            params![repo.summary.repo_id, asset_id, now],
        )?;
        insert_event(
            tx,
            &repo.summary,
            &asset_id,
            "asset.deleted",
            &path,
            serde_json::json!({
                "origin": "scan"
            }),
        )?;
        deleted_assets += 1;
        created_events += 1;
    }

    Ok(SyncResult {
        repo_id: repo.summary.repo_id.clone(),
        scanned_files: files.len() as i64,
        created_assets,
        updated_assets,
        deleted_assets,
        created_events,
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
    updated_at: &str,
) -> Result<(), rusqlite::Error> {
    let defaults = [
        ("title", serde_json::Value::String(filename.to_string())),
        ("favorite", serde_json::Value::Bool(false)),
        ("type", serde_json::Value::String(extension.to_string())),
    ];

    for (key, value) in defaults {
        tx.execute(
            r#"
            INSERT OR REPLACE INTO metadata (asset_id, key, value_type, value_json, version, updated_at)
            VALUES (?1, ?2, ?3, ?4, 1, ?5)
            "#,
            params![asset_id, key, infer_value_type(&value), value.to_string(), updated_at],
        )?;
    }

    Ok(())
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
            relative_path: relative,
            filename: file_name.to_string(),
            extension,
            size_bytes: metadata.len() as i64,
            modified_at: now_rfc3339(),
        });
    }

    Ok(())
}

fn generate_thumbnail_for_file(
    repo: &RepositoryRecord,
    repo_root: &Path,
    thumbnail_root: &Path,
    asset_id: &str,
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
    let thumbnail_path = thumbnail_dir.join(thumbnail_file_name(asset_id, &file.relative_path));

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
    asset_id: &str,
    file: &DiscoveredFile,
    existing_thumbnail_path: Option<String>,
) -> Result<Option<String>, String> {
    if let Some(path) = existing_thumbnail_path {
        let expected_dir = thumbnail_root.join(thumbnail_repository_dir_name(
            &repo.summary.repo_id,
            &repo.summary.path,
        ));
        let thumbnail_path = Path::new(&path);
        if thumbnail_path.starts_with(&expected_dir) && thumbnail_path.is_file() {
            return Ok(Some(path));
        }
    }

    generate_thumbnail_for_file(repo, repo_root, thumbnail_root, asset_id, file)
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

fn is_video_extension(extension: &str) -> bool {
    matches!(extension, "mp4" | "mov" | "mkv" | "webm" | "avi" | "m4v")
}

#[derive(Debug)]
struct DiscoveredFile {
    relative_path: String,
    filename: String,
    extension: String,
    size_bytes: i64,
    modified_at: String,
}

fn slugify_repo_id(name: &str, path: &str) -> String {
    slugify_ascii_component(&format!("{name}-{path}"))
}

fn thumbnail_repository_dir_name(repo_id: &str, repo_path: &str) -> String {
    compact_thumbnail_component(repo_id, &[repo_id, repo_path])
}

fn thumbnail_file_name(asset_id: &str, relative_path: &str) -> String {
    format!(
        "{}.jpg",
        compact_thumbnail_component(asset_id, &[asset_id, relative_path])
    )
}

fn compact_thumbnail_component(label: &str, hash_parts: &[&str]) -> String {
    let slug = slugify_ascii_component(label);
    let slug = if slug.is_empty() {
        "thumbnail".to_string()
    } else {
        slug
    };
    let prefix = safe_prefix(&slug, THUMBNAIL_CACHE_PREFIX_CHARS);
    format!("{prefix}-{}", stable_hash_hex(hash_parts))
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

fn stable_hash_hex(parts: &[&str]) -> String {
    let mut hash = FNV_OFFSET_BASIS;
    for part in parts {
        for byte in part.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(FNV_PRIME);
        }
        hash ^= 0xff;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("{hash:016x}")
}

fn default_plugins() -> Vec<PluginManifest> {
    let mut plugins = file_system_plugin_descriptors()
        .iter()
        .map(|descriptor| PluginManifest {
            plugin_id: descriptor.plugin_id.to_string(),
            name: descriptor.name.to_string(),
            version: if descriptor.plugin_id == LOCAL_FILESYSTEM_PLUGIN_ID {
                "1.0.0".to_string()
            } else {
                "0.1.0".to_string()
            },
            kind: descriptor.kind.to_string(),
            description: match descriptor.plugin_id {
                LOCAL_FILESYSTEM_PLUGIN_ID => "使用本地目录作为仓库文件管理后端。".to_string(),
                WEBDAV_PLUGIN_ID => "通过 WebDAV 适配远程文件管理服务。".to_string(),
                CLOUD_DRIVE_PLUGIN_ID => "预留云盘文件系统接入点，如对象存储或网盘。".to_string(),
                _ => "文件系统后端插件。".to_string(),
            },
            capabilities: descriptor
                .capabilities
                .iter()
                .map(|item| (*item).to_string())
                .collect(),
            enabled: descriptor.plugin_id == LOCAL_FILESYSTEM_PLUGIN_ID,
        })
        .collect::<Vec<_>>();
    plugins.extend([
        PluginManifest {
            plugin_id: "builtin.three-model-preview".to_string(),
            name: "3D Model Preview".to_string(),
            version: "1.0.0".to_string(),
            kind: "preview".to_string(),
            description: "为 FBX、OBJ、GLB 与 glTF 模型提供可旋转缩放的 3D 文件预览。".to_string(),
            capabilities: vec![
                "preview".to_string(),
                "3d-model".to_string(),
                "fbx".to_string(),
                "obj".to_string(),
                "gltf".to_string(),
            ],
            enabled: true,
        },
        PluginManifest {
            plugin_id: "builtin.filesystem-watcher".to_string(),
            name: "Filesystem Watcher".to_string(),
            version: "1.0.0".to_string(),
            kind: "watcher".to_string(),
            description: "监听仓库目录，记录新增、删除、修改与重命名事件。".to_string(),
            capabilities: vec![
                "watch".to_string(),
                "events".to_string(),
                "sync".to_string(),
            ],
            enabled: true,
        },
        PluginManifest {
            plugin_id: "builtin.metadata-provider".to_string(),
            name: "Metadata Provider".to_string(),
            version: "1.0.0".to_string(),
            kind: "metadata".to_string(),
            description: "提供可扩展的元数据生成与写入能力。".to_string(),
            capabilities: vec![
                "metadata".to_string(),
                "tags".to_string(),
                "ocr".to_string(),
            ],
            enabled: false,
        },
        PluginManifest {
            plugin_id: "builtin.vector-index".to_string(),
            name: "Vector Index".to_string(),
            version: "0.1.0".to_string(),
            kind: "search".to_string(),
            description: "预留向量检索与 AI 语义搜索扩展点。".to_string(),
            capabilities: vec!["semantic-search".to_string(), "embedding".to_string()],
            enabled: false,
        },
    ]);
    plugins
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
            group: "Plugin API".to_string(),
            method: "GET".to_string(),
            path: "/plugins".to_string(),
            summary: "列出插件与能力声明。".to_string(),
        },
    ]
}

fn file_system_plugin_descriptors() -> &'static [FileSystemPluginDescriptor] {
    &[
        FileSystemPluginDescriptor {
            plugin_id: LOCAL_FILESYSTEM_PLUGIN_ID,
            kind: "filesystem",
            name: "Local Filesystem",
            capabilities: &["browse", "read", "write", "watch", "sync"],
        },
        FileSystemPluginDescriptor {
            plugin_id: WEBDAV_PLUGIN_ID,
            kind: "webdav",
            name: "WebDAV",
            capabilities: &["browse", "read", "write", "sync"],
        },
        FileSystemPluginDescriptor {
            plugin_id: CLOUD_DRIVE_PLUGIN_ID,
            kind: "cloud",
            name: "Cloud Drive",
            capabilities: &["browse", "read", "write", "sync"],
        },
    ]
}

fn file_system_plugin_descriptor(plugin_id: &str) -> Option<&'static FileSystemPluginDescriptor> {
    file_system_plugin_descriptors()
        .iter()
        .find(|descriptor| descriptor.plugin_id == plugin_id)
}

fn backend_summary(plugin_id: &str) -> RepositoryBackendSummary {
    let descriptor = file_system_plugin_descriptor(plugin_id)
        .or_else(|| file_system_plugin_descriptor(LOCAL_FILESYSTEM_PLUGIN_ID))
        .expect("local filesystem plugin descriptor must exist");
    RepositoryBackendSummary {
        plugin_id: descriptor.plugin_id.to_string(),
        kind: descriptor.kind.to_string(),
        name: descriptor.name.to_string(),
        capabilities: descriptor
            .capabilities
            .iter()
            .map(|value| (*value).to_string())
            .collect(),
    }
}

fn parse_backend_request(
    request: &RepositoryMutationRequest,
) -> Result<RepositoryBackendRecord, String> {
    let plugin_id = request
        .backend_plugin_id
        .as_deref()
        .unwrap_or(LOCAL_FILESYSTEM_PLUGIN_ID)
        .trim();
    let descriptor = file_system_plugin_descriptor(plugin_id)
        .ok_or_else(|| format!("unsupported filesystem backend plugin: {plugin_id}"))?;
    let config = request
        .backend_config
        .clone()
        .unwrap_or_else(|| serde_json::json!({}));
    if !config.is_object() {
        return Err("backend config must be a JSON object".to_string());
    }
    Ok(RepositoryBackendRecord {
        plugin_id: descriptor.plugin_id.to_string(),
        config,
    })
}

fn import_backend_record(
    metadata: &RepositoryMetadataFileImport,
) -> Option<RepositoryBackendRecord> {
    let plugin_id = metadata
        .backend_plugin_id
        .as_deref()
        .unwrap_or(LOCAL_FILESYSTEM_PLUGIN_ID);
    file_system_plugin_descriptor(plugin_id).map(|descriptor| RepositoryBackendRecord {
        plugin_id: descriptor.plugin_id.to_string(),
        config: metadata
            .backend_config
            .clone()
            .unwrap_or_else(|| serde_json::json!({})),
    })
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

fn migrate_repository_schema(connection: &Connection) -> Result<(), rusqlite::Error> {
    connection.execute_batch(REPOSITORY_SCHEMA_SQL)?;
    let mut stmt = connection.prepare("PRAGMA table_info(assets)")?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    if !columns.iter().any(|column| column == "thumbnail_path") {
        connection.execute("ALTER TABLE assets ADD COLUMN thumbnail_path TEXT", [])?;
    }
    Ok(())
}

fn ensure_backend_path_is_attachable(
    backend: &RepositoryBackendRecord,
    repo_root: &Path,
) -> Result<(), String> {
    let descriptor = file_system_plugin_descriptor(&backend.plugin_id).ok_or_else(|| {
        format!(
            "unsupported filesystem backend plugin: {}",
            backend.plugin_id
        )
    })?;
    let adapter: Box<dyn FileSystemBackendAdapter> = match backend.plugin_id.as_str() {
        LOCAL_FILESYSTEM_PLUGIN_ID => Box::new(LocalFileSystemBackend),
        _ => Box::new(UnsupportedFileSystemBackend { descriptor }),
    };
    adapter.ensure_attachable(repo_root, &backend.config)
}

fn initialize_repository_directory(
    service_root: &Path,
    repo_root: &Path,
    seed: &RepositorySeed<'_>,
    backend: &RepositoryBackendRecord,
) -> Result<(), String> {
    let descriptor = file_system_plugin_descriptor(&backend.plugin_id).ok_or_else(|| {
        format!(
            "unsupported filesystem backend plugin: {}",
            backend.plugin_id
        )
    })?;
    let adapter: Box<dyn FileSystemBackendAdapter> = match backend.plugin_id.as_str() {
        LOCAL_FILESYSTEM_PLUGIN_ID => Box::new(LocalFileSystemBackend),
        _ => Box::new(UnsupportedFileSystemBackend { descriptor }),
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
    let metadata_dir = if backend_plugin_id == LOCAL_FILESYSTEM_PLUGIN_ID {
        migrate_legacy_meta_dir_if_needed(repo_root, backend_plugin_id)?;
        let metadata_dir = repository_meta_dir(repo_root);
        create_repository_metadata_dirs(&metadata_dir)?;
        metadata_dir
    } else {
        let service_repo_dir = repository_state_storage_dir(service_root, repo_id);
        fs::create_dir_all(&service_repo_dir).map_err(io_error)?;
        let metadata_dir = service_repo_dir.join(REPO_META_DIR);
        create_repository_metadata_dirs(&metadata_dir)?;
        metadata_dir
    };
    Ok(RepositoryStoragePaths {
        database_path: metadata_dir.join(REPO_DB_FILE_NAME),
        metadata_dir,
    })
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

fn legacy_repository_meta_dir(repo_root: &Path) -> PathBuf {
    repo_root.join(LEGACY_REPO_META_DIR)
}

fn create_repository_metadata_dirs(metadata_dir: &Path) -> Result<(), String> {
    fs::create_dir_all(metadata_dir.join("cache")).map_err(io_error)?;
    fs::create_dir_all(metadata_dir.join("thumbnails")).map_err(io_error)?;
    fs::create_dir_all(metadata_dir.join("logs")).map_err(io_error)?;
    fs::create_dir_all(metadata_dir.join("indexes")).map_err(io_error)?;
    Ok(())
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

fn copy_external_entry_into_directory(
    source: &Path,
    repo_root: &Path,
    target_dir: &Path,
) -> Result<(), String> {
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
    if target.exists() {
        return Err(format!("entry already exists: {name}"));
    }

    if source.is_dir() {
        let source_canonical = source.canonicalize().map_err(io_error)?;
        let repo_canonical = repo_root.canonicalize().map_err(io_error)?;
        let target_canonical_parent = target_dir.canonicalize().map_err(io_error)?;
        if source_canonical == repo_canonical || repo_canonical.starts_with(&source_canonical) {
            return Err("cannot import a repository folder into itself".to_string());
        }
        if target_canonical_parent.starts_with(&source_canonical) {
            return Err("cannot import a folder into one of its descendants".to_string());
        }
        copy_directory_recursive(&source_canonical, &target)?;
    } else if source.is_file() {
        fs::copy(source, target).map_err(io_error)?;
    } else {
        return Err(format!(
            "unsupported source path type: {}",
            source.to_string_lossy()
        ));
    }

    Ok(())
}

fn copy_directory_recursive(source: &Path, target: &Path) -> Result<(), String> {
    fs::create_dir(target).map_err(io_error)?;
    for entry in fs::read_dir(source).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let name = entry.file_name().to_string_lossy().to_string();
        if is_internal_repository_dir(&name) {
            continue;
        }

        let child_source = entry.path();
        let child_target = target.join(&name);
        let metadata = entry.metadata().map_err(io_error)?;
        if metadata.is_dir() {
            copy_directory_recursive(&child_source, &child_target)?;
        } else if metadata.is_file() {
            fs::copy(&child_source, &child_target).map_err(io_error)?;
        }
    }

    Ok(())
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

fn backend_adapter<'a>(repo: &'a RepositoryRecord) -> Box<dyn FileSystemBackendAdapter + 'a> {
    match repo.backend_record.plugin_id.as_str() {
        LOCAL_FILESYSTEM_PLUGIN_ID => Box::new(LocalFileSystemBackend),
        plugin_id => Box::new(UnsupportedFileSystemBackend {
            descriptor: file_system_plugin_descriptor(plugin_id)
                .or_else(|| file_system_plugin_descriptor(LOCAL_FILESYSTEM_PLUGIN_ID))
                .expect("filesystem backend descriptor must exist"),
        }),
    }
}

fn list_backend_files(
    repo: &RepositoryRecord,
    repo_root: &Path,
) -> Result<Vec<DiscoveredFile>, String> {
    backend_adapter(repo).list_files(repo_root, &repo.backend_record.config)
}

fn list_backend_tree(
    repo: &RepositoryRecord,
    repo_root: &Path,
) -> Result<Vec<FileTreeNode>, String> {
    backend_adapter(repo).list_tree(repo_root, &repo.backend_record.config)
}

fn list_backend_directory_entries(
    repo: &RepositoryRecord,
    repo_root: &Path,
    current_path: &str,
    asset_map: &BTreeMap<String, (String, String, Option<String>)>,
) -> Result<Vec<FileBrowserEntry>, String> {
    let entries = backend_adapter(repo).list_directory_entries(
        repo_root,
        current_path,
        &repo.backend_record.config,
    )?;
    Ok(map_file_browser_entries(entries, asset_map))
}

fn create_backend_directory(
    repo: &RepositoryRecord,
    repo_root: &Path,
    parent_path: &str,
    name: &str,
) -> Result<(), String> {
    backend_adapter(repo).create_directory(
        repo_root,
        parent_path,
        name,
        &repo.backend_record.config,
    )
}

fn create_backend_file(
    repo: &RepositoryRecord,
    repo_root: &Path,
    parent_path: &str,
    name: &str,
) -> Result<(), String> {
    backend_adapter(repo).create_file(repo_root, parent_path, name, &repo.backend_record.config)
}

fn stat_backend_entry(
    repo: &RepositoryRecord,
    repo_root: &Path,
    entry_path: &str,
) -> Result<FileSystemEntry, String> {
    backend_adapter(repo).stat_entry(repo_root, entry_path, &repo.backend_record.config)
}

fn rename_backend_entry(
    repo: &RepositoryRecord,
    repo_root: &Path,
    source_path: &str,
    new_name: &str,
) -> Result<FileSystemEntry, String> {
    backend_adapter(repo).rename_entry(
        repo_root,
        source_path,
        new_name,
        &repo.backend_record.config,
    )
}

fn delete_backend_entry(
    repo: &RepositoryRecord,
    repo_root: &Path,
    entry_path: &str,
    recursive: bool,
) -> Result<(), String> {
    backend_adapter(repo).delete_entry(
        repo_root,
        entry_path,
        recursive,
        &repo.backend_record.config,
    )
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
    asset_map: &BTreeMap<String, (String, String, Option<String>)>,
) -> Vec<FileBrowserEntry> {
    entries.sort_by(|left, right| match (&left.kind, &right.kind) {
        (FileSystemEntryKind::Directory, FileSystemEntryKind::File) => std::cmp::Ordering::Less,
        (FileSystemEntryKind::File, FileSystemEntryKind::Directory) => std::cmp::Ordering::Greater,
        _ => left.path.to_lowercase().cmp(&right.path.to_lowercase()),
    });

    entries
        .into_iter()
        .map(|entry| {
            let (asset_id, status, thumbnail_path) = asset_map
                .get(&entry.path)
                .map(|(id, entry_status, thumbnail)| {
                    (
                        Some(id.clone()),
                        Some(entry_status.clone()),
                        thumbnail.clone(),
                    )
                })
                .unwrap_or((None, None, None));
            let size_bytes = entry.size_bytes;
            FileBrowserEntry {
                path: entry.path.clone(),
                name: entry.name,
                kind: match entry.kind {
                    FileSystemEntryKind::Directory => "directory".to_string(),
                    FileSystemEntryKind::File => "file".to_string(),
                },
                extension: entry.extension,
                size_bytes,
                size_label: size_bytes.map(format_size_label),
                modified_at: entry.modified_at,
                asset_id,
                status,
                thumbnail_path,
            }
        })
        .collect()
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

impl FileSystemBackendAdapter for UnsupportedFileSystemBackend {
    fn ensure_attachable(
        &self,
        _repo_root: &Path,
        _config: &serde_json::Value,
    ) -> Result<(), String> {
        Err(format!(
            "filesystem backend is registered but not implemented yet: {}",
            self.descriptor.plugin_id
        ))
    }

    fn prepare_repository_root(
        &self,
        _repo_root: &Path,
        _config: &serde_json::Value,
    ) -> Result<(), String> {
        Ok(())
    }

    fn list_files(
        &self,
        _repo_root: &Path,
        _config: &serde_json::Value,
    ) -> Result<Vec<DiscoveredFile>, String> {
        Err(format!(
            "filesystem backend is registered but not implemented yet: {}",
            self.descriptor.plugin_id
        ))
    }

    fn list_tree(
        &self,
        _repo_root: &Path,
        _config: &serde_json::Value,
    ) -> Result<Vec<FileTreeNode>, String> {
        Err(format!(
            "filesystem backend is registered but not implemented yet: {}",
            self.descriptor.plugin_id
        ))
    }

    fn list_directory_entries(
        &self,
        _repo_root: &Path,
        _directory_path: &str,
        _config: &serde_json::Value,
    ) -> Result<Vec<FileSystemEntry>, String> {
        Err(format!(
            "filesystem backend is registered but not implemented yet: {}",
            self.descriptor.plugin_id
        ))
    }

    fn create_directory(
        &self,
        _repo_root: &Path,
        _parent_path: &str,
        _name: &str,
        _config: &serde_json::Value,
    ) -> Result<(), String> {
        Err(format!(
            "filesystem backend is registered but not implemented yet: {}",
            self.descriptor.plugin_id
        ))
    }

    fn create_file(
        &self,
        _repo_root: &Path,
        _parent_path: &str,
        _name: &str,
        _config: &serde_json::Value,
    ) -> Result<(), String> {
        Err(format!(
            "filesystem backend is registered but not implemented yet: {}",
            self.descriptor.plugin_id
        ))
    }

    fn stat_entry(
        &self,
        _repo_root: &Path,
        _entry_path: &str,
        _config: &serde_json::Value,
    ) -> Result<FileSystemEntry, String> {
        Err(format!(
            "filesystem backend is registered but not implemented yet: {}",
            self.descriptor.plugin_id
        ))
    }

    fn rename_entry(
        &self,
        _repo_root: &Path,
        _source_path: &str,
        _new_name: &str,
        _config: &serde_json::Value,
    ) -> Result<FileSystemEntry, String> {
        Err(format!(
            "filesystem backend is registered but not implemented yet: {}",
            self.descriptor.plugin_id
        ))
    }

    fn delete_entry(
        &self,
        _repo_root: &Path,
        _entry_path: &str,
        _recursive: bool,
        _config: &serde_json::Value,
    ) -> Result<(), String> {
        Err(format!(
            "filesystem backend is registered but not implemented yet: {}",
            self.descriptor.plugin_id
        ))
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
        delete_backend_entry(repo, repo_root, source_path, true)?;
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
        return Ok(());
    }

    rename_directory_asset_records(tx, repo_id, source_path, target_path)
}

fn mark_file_asset_deleted(
    tx: &Transaction<'_>,
    repo_id: &str,
    path: &str,
) -> Result<(), rusqlite::Error> {
    tx.execute(
        r#"
        UPDATE assets
        SET status = 'deleted', updated_at = ?3
        WHERE repo_id = ?1 AND path = ?2
        "#,
        params![repo_id, path, now_rfc3339()],
    )?;
    Ok(())
}

fn mark_directory_assets_deleted(
    tx: &Transaction<'_>,
    repo_id: &str,
    path: &str,
) -> Result<(), rusqlite::Error> {
    let prefix = format!("{path}/%");
    tx.execute(
        r#"
        UPDATE assets
        SET status = 'deleted', updated_at = ?4
        WHERE repo_id = ?1 AND (path = ?2 OR path LIKE ?3)
        "#,
        params![repo_id, path, prefix, now_rfc3339()],
    )?;
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
        let state_root = root.join("state");
        let thumbnail_root = root.join("thumbs");
        fs::create_dir_all(&repo_root).expect("repo root should be created");
        (
            RepositoryState::from_roots(state_root, thumbnail_root.clone()),
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

    fn write_test_image(path: &Path) {
        let image = image::RgbImage::from_pixel(2, 2, image::Rgb([120, 120, 120]));
        image.save(path).expect("test image should be saved");
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
        let thumbnail_path = thumbnail_dir.join(thumbnail_file_name(&asset_id, "cover.png"));
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
            })
            .expect("thumbnail should be ensured");

        assert_eq!(
            response.thumbnail_path,
            Some(thumbnail_path.to_string_lossy().to_string())
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
            })
            .expect("unsupported thumbnail request should succeed");

        assert_eq!(response.thumbnail_path, None);

        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn thumbnail_file_name_stays_within_windows_component_limit() {
        let asset_id = format!(
            "asset-{}",
            slugify_repo_id("startIcon.png", LONG_RELATIVE_PATH)
        );
        let repo_dir = thumbnail_repository_dir_name(&asset_id, LONG_RELATIVE_PATH);
        let file_name = thumbnail_file_name(&asset_id, LONG_RELATIVE_PATH);

        assert!(repo_dir.len() <= 255);
        assert!(file_name.len() <= 255);
        assert!(file_name.ends_with(".jpg"));
    }

    #[test]
    fn thumbnail_cache_names_are_stable() {
        let repo_dir = thumbnail_repository_dir_name("repo-cubism", "C:/Assets/Cubism");
        let file_name = thumbnail_file_name("asset-start-icon", LONG_RELATIVE_PATH);

        assert_eq!(
            repo_dir,
            thumbnail_repository_dir_name("repo-cubism", "C:/Assets/Cubism")
        );
        assert_eq!(
            file_name,
            thumbnail_file_name("asset-start-icon", LONG_RELATIVE_PATH)
        );
    }

    #[test]
    fn thumbnail_cache_names_differ_for_different_paths() {
        let first = thumbnail_file_name("asset-icon", LONG_RELATIVE_PATH);
        let second = thumbnail_file_name(
            "asset-icon",
            "CubismSdkForNative-5-r.5/Samples/OpenGL/Demo/proj.harmonyos.cmake/Full/entry/src/ohosTest/resources/base/media/icon.png",
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
