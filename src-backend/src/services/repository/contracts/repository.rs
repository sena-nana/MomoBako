//! 播放会话、任务进度、仓库与资源摘要、播放列表、搜索结果、仓库概览与仓库动作的契约。

use super::*;

/// Core-owned playlist playback state shared by the services and the Nana host.
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackSessionState {
    pub session_id: String,
    pub repo_id: String,
    pub playlist_id: String,
    pub playlist_item_id: Option<String>,
    pub status: String,
    pub current_time_ms: u64,
    pub duration_ms: Option<u64>,
    pub volume: f32,
    pub can_seek: bool,
    pub can_volume: bool,
    pub error: Option<String>,
    pub updated_at: String,
}

/// Stable projection of a long-running task progress event.
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TaskProgressSnapshot {
    pub task_id: String,
    pub protocol_id: String,
    pub status: String,
    pub phase: Option<String>,
    pub label: Option<String>,
    pub current: Option<u64>,
    pub total: Option<u64>,
    pub percent: Option<f32>,
    pub error: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryBackendSummary {
    pub plugin_id: String,
    pub kind: String,
    pub name: String,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authentication: Option<RepositoryAuthenticationStatus>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
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
    pub last_accessed_at: Option<String>,
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

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryAuthenticationStatus {
    pub required: bool,
    pub logged_in: bool,
    pub login_expired: bool,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FolderSummary {
    pub path: String,
    pub label: String,
    pub asset_count: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryShortcut {
    pub shortcut_id: String,
    pub label: String,
    pub target_kind: String,
    pub target_path: Option<String>,
    pub target_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryTagGroup {
    pub tag_group_id: String,
    pub name: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SourceTrashEntry {
    pub trash_path: String,
    pub original_path: String,
    pub kind: String,
    pub deleted_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shared_asset_id: Option<String>,
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
    pub metadata: Option<serde_json::Value>,
    pub local_absolute_path: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistDetail {
    pub playlist: PlaylistSummary,
    pub items: Vec<PlaylistItem>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
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
    pub trash_count: i64,
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

#[derive(Debug, Serialize, Deserialize)]
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
