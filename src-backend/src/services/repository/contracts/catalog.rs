//! 同步、缩略图、修订、搜索筛选、智能文件夹、缓存、插件清单与 API 设计快照的契约。

use super::*;

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

#[derive(Debug, Serialize, Deserialize)]
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

#[derive(Debug, Serialize, Deserialize, Clone)]
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

#[derive(Debug, Serialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CacheUsage {
    /// `thumbnails`、`cache/<子目录>` 或 `cache`（缓存根下的散文件）。
    pub kind: String,
    pub label: String,
    pub path: String,
    pub files: u64,
    pub bytes: u64,
    pub modified_at: Option<String>,
}

/// 一个资源库的缓存占用，见 `cache_usage.rs`。
#[derive(Debug, Serialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CacheSnapshot {
    pub repo_id: String,
    pub entries: Vec<CacheUsage>,
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
    pub package_format_version: Option<u32>,
    #[serde(default)]
    pub package_hash: Option<String>,
    #[serde(default)]
    pub provenance: Option<String>,
    #[serde(default)]
    pub trust_level: Option<String>,
    #[serde(default)]
    pub deployment: Option<String>,
    #[serde(default)]
    pub target_triple: Option<String>,
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
pub(in super::super) struct PlaylistPlayerRegistration {
    pub(in super::super) plugin_id: String,
    pub(in super::super) player_type_id: String,
    pub(in super::super) label: String,
    pub(in super::super) file_class: String,
    pub(in super::super) supported_extensions: Vec<String>,
    pub(in super::super) supports_seek: bool,
    pub(in super::super) supports_volume: bool,
    pub(in super::super) supports_preview_navigation: bool,
    pub(in super::super) description: Option<String>,
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

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ApiDefinition {
    pub group: String,
    pub transport: String,
    pub method: String,
    pub path: String,
    pub summary: String,
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
