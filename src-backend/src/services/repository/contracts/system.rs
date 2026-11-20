//! 系统日志、插件调用、下载器、插件钩子记录与插件配置的契约。

use super::*;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SystemLogSource {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct SystemLogLocation {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SystemLogRecord {
    pub id: String,
    pub timestamp: String,
    pub level: String,
    pub category: String,
    pub action: String,
    pub message: String,
    pub source: SystemLogSource,
    pub location: SystemLogLocation,
    pub context: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct SystemLogQuery {
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub before: Option<String>,
    #[serde(default)]
    pub levels: Vec<String>,
    #[serde(default)]
    pub source_kinds: Vec<String>,
    #[serde(default)]
    pub plugin_id: Option<String>,
    #[serde(default)]
    pub repo_id: Option<String>,
    #[serde(default)]
    pub query: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SystemLogPage {
    pub records: Vec<SystemLogRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct SystemLogWriteRequest {
    pub level: String,
    pub category: String,
    pub action: String,
    pub message: String,
    #[serde(default)]
    pub context: Option<serde_json::Value>,
    #[serde(default)]
    pub repo_id: Option<String>,
    #[serde(default)]
    pub plugin_id: Option<String>,
    #[serde(default)]
    pub source_kind: Option<String>,
    #[serde(default)]
    pub source_label: Option<String>,
    #[serde(default)]
    pub location: Option<SystemLogLocationInput>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct SystemLogLocationInput {
    #[serde(default)]
    pub module_path: Option<String>,
    #[serde(default)]
    pub file: Option<String>,
    #[serde(default)]
    pub line: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginCallRequest {
    pub plugin_id: String,
    pub method: String,
    #[serde(default)]
    pub repository_id: Option<String>,
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
    #[serde(default)]
    pub source_repository_id: Option<String>,
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

#[derive(Debug, Serialize, Deserialize, Clone)]
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
pub(in super::super) struct PluginRuntimeCallResult {
    pub(in super::super) plugin_id: String,
    pub(in super::super) payload: serde_json::Value,
    pub(in super::super) runtime: Option<PluginCallRuntime>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PluginDataDirectoryResponse {
    pub plugin_id: String,
    pub path: String,
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
