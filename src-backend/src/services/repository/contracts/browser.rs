//! 播放列表变更请求、文件树与文件浏览、条目删除与播放、元数据与仓库变更、导出和访问记录的契约。

use super::*;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistMutationRequest {
    pub repo_id: String,
    pub playlist_id: Option<String>,
    pub name: String,
    pub player_type_id: String,
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
    #[serde(default)]
    pub file_count: usize,
    pub children: Vec<FileTreeNode>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RepositoryStructureCacheState {
    Warming,
    Ready,
    Refreshing,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryStructureUpdatedEvent {
    pub repo_id: String,
    pub reason: String,
    pub indexed_at: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryTreeSnapshot {
    pub repo_id: String,
    pub root_path: String,
    pub backend_plugin_id: String,
    pub backend_kind: String,
    pub cache_state: RepositoryStructureCacheState,
    pub indexed_at: Option<String>,
    pub tree: Vec<FileTreeNode>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
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

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct SourceRepositoryStateSnapshot {
    #[serde(default)]
    pub directory_metadata_by_path: BTreeMap<String, FolderMetadata>,
    #[serde(default)]
    pub quick_access: Vec<RepositoryShortcut>,
    #[serde(default)]
    pub tag_groups: Vec<RepositoryTagGroup>,
    #[serde(default)]
    pub smart_folders: Vec<SmartFolder>,
    #[serde(default)]
    pub repository_actions: Vec<RepositoryAction>,
    #[serde(default)]
    pub trash_entries: Vec<SourceTrashEntry>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FileBrowserSnapshot {
    pub repo_id: String,
    pub root_path: String,
    pub backend_plugin_id: String,
    pub backend_kind: String,
    pub cache_state: RepositoryStructureCacheState,
    pub indexed_at: Option<String>,
    pub current_path: String,
    #[serde(default)]
    pub total_entries: usize,
    #[serde(default)]
    pub loaded_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_offset: Option<usize>,
    #[serde(default)]
    pub has_more: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub special_location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tree: Option<Vec<FileTreeNode>>,
    pub entries: Vec<FileBrowserEntry>,
}

/// 长任务 `momobako.entry.delete` 的请求：一条（同 [`FileDeleteRequest`]）或同一资源库的一批路径。
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(untagged)]
pub enum EntryDeleteRequest {
    Single(FileDeleteRequest),
    Batch {
        #[serde(rename = "repoId")]
        repo_id: String,
        paths: Vec<String>,
        mode: Option<String>,
    },
}

/// 长任务 `momobako.entry.delete` 的结果：删了几条，以及最后一次删除后的目录快照。
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct EntryDeleteResponse {
    pub repo_id: String,
    pub deleted_count: usize,
    pub snapshot: Option<FileBrowserSnapshot>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct EntryPlaybackRequest {
    pub repo_id: String,
    pub path: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct EntryPlaybackSourceResponse {
    pub repo_id: String,
    pub path: String,
    pub media_type: String,
    pub source_url: Option<String>,
    pub local_path: Option<String>,
    pub temp_file_path: Option<String>,
    pub lyric_path: Option<String>,
    pub word_lyric_path: Option<String>,
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
pub(in super::super) struct RepositoryMetadataFile {
    pub(in super::super) repo_id: String,
    pub(in super::super) name: String,
    pub(in super::super) root_path: String,
    pub(in super::super) backend_plugin_id: String,
    pub(in super::super) backend_config: serde_json::Value,
    pub(in super::super) created_at: String,
    pub(in super::super) schema_version: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(in super::super) struct RepositoryMetadataFileImport {
    pub(in super::super) repo_id: String,
    pub(in super::super) name: Option<String>,
    pub(in super::super) root_path: Option<String>,
    pub(in super::super) backend_plugin_id: Option<String>,
    pub(in super::super) backend_config: Option<serde_json::Value>,
    pub(in super::super) created_at: Option<String>,
    pub(in super::super) schema_version: Option<i64>,
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

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RepositoryDeleteMode {
    RecordOnly,
    DeleteMetadata,
    DeleteFolder,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryDeleteRequest {
    pub repo_id: String,
    pub mode: RepositoryDeleteMode,
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
    pub(in super::super) fn empty() -> Self {
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

#[derive(Debug, Serialize, Deserialize)]
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
    pub offset: Option<usize>,
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileReadRequest {
    pub repo_id: String,
    pub path: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryAccessRecordRequest {
    pub repo_id: String,
    pub path: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct EntryAccessRecordResponse {
    pub repo_id: String,
    pub path: String,
    pub recorded_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RecentAccessHistoryClearRequest {
    pub repo_id: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RecentAccessHistoryClearResponse {
    pub repo_id: String,
    pub cleared_count: usize,
}
