//! Repository service DTOs shared with ViewModels and Tauri commands.

use super::*;

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
pub(super) struct RepositoryMetadataFile {
    pub(super) repo_id: String,
    pub(super) name: String,
    pub(super) root_path: String,
    pub(super) backend_plugin_id: String,
    pub(super) backend_config: serde_json::Value,
    pub(super) created_at: String,
    pub(super) schema_version: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RepositoryMetadataFileImport {
    pub(super) repo_id: String,
    pub(super) name: Option<String>,
    pub(super) root_path: Option<String>,
    pub(super) backend_plugin_id: Option<String>,
    pub(super) backend_config: Option<serde_json::Value>,
    pub(super) created_at: Option<String>,
    pub(super) schema_version: Option<i64>,
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
    pub(super) fn empty() -> Self {
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
pub(super) struct PluginRuntimeCallResult {
    pub(super) plugin_id: String,
    pub(super) payload: serde_json::Value,
    pub(super) runtime: Option<PluginCallRuntime>,
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
pub(super) struct TrashManifest {
    pub(super) entries: Vec<TrashManifestEntry>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(super) struct TrashManifestEntry {
    pub(super) original_path: String,
    pub(super) trash_path: String,
    pub(super) deleted_at: String,
    pub(super) kind: String,
}

#[derive(Debug)]
pub(super) struct ThumbnailRecord {
    pub(super) path: String,
    pub(super) custom: bool,
}

#[derive(Debug, Clone)]
pub(super) struct AssetPathRecord {
    pub(super) asset_id: String,
    pub(super) status: String,
    pub(super) thumbnail_path: Option<String>,
    pub(super) hardlink_group_id: Option<String>,
    pub(super) hardlink_state: Option<String>,
    pub(super) is_virtual: bool,
    pub(super) provider_id: Option<String>,
    pub(super) provider_item_id: Option<String>,
    pub(super) source_payload: Option<serde_json::Value>,
    pub(super) local_absolute_path: Option<String>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(super) struct ExistingAssetRecord {
    pub(super) asset_id: String,
    pub(super) status: String,
    pub(super) thumbnail_path: Option<String>,
    pub(super) size_bytes: i64,
    pub(super) created_at: String,
    pub(super) modified_at: String,
    pub(super) hash: Option<String>,
    pub(super) is_virtual: bool,
    pub(super) provider_id: Option<String>,
    pub(super) provider_item_id: Option<String>,
    pub(super) source_payload: Option<serde_json::Value>,
    pub(super) local_absolute_path: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct HardlinkCopyOutcome {
    pub(super) source_path: Option<String>,
    pub(super) target_path: String,
    pub(super) link_state: String,
}

#[derive(Debug, Clone)]
pub(super) struct HardlinkAssetRecord {
    pub(super) asset_id: String,
    pub(super) content_hash: String,
    pub(super) size_bytes: i64,
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
#[allow(dead_code)]
pub(super) struct PlaylistPlayerRegistration {
    pub(super) plugin_id: String,
    pub(super) player_type_id: String,
    pub(super) label: String,
    pub(super) file_class: String,
    pub(super) supported_extensions: Vec<String>,
    pub(super) supports_seek: bool,
    pub(super) supports_volume: bool,
    pub(super) supports_preview_navigation: bool,
    pub(super) description: Option<String>,
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
pub(super) struct PluginCallEnvelope {
    pub(super) method: String,
    pub(super) payload: serde_json::Value,
    pub(super) runtime: PluginCallHostRuntime,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PluginCallHostRuntime {
    pub(super) plugin_id: String,
    pub(super) plugin_data_dir: String,
    pub(super) plugin_config: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PluginCallResponse {
    pub(super) ok: bool,
    pub(super) payload: Option<serde_json::Value>,
    pub(super) error: Option<String>,
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
