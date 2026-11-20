//! 文件预览、创建导入、Eagle 导入、外部添加资源、复制硬链接、回收站与内部记录的契约。

use super::*;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilePreviewSourceResponse {
    pub repo_id: String,
    pub path: String,
    pub token: String,
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
    /// 目标文件夹里已有同名条目（或同一批里名字重复）时跳过它、照常导入其余的，已有的文件不覆盖。
    /// 不传或为假时照旧整批拒绝（`entry already exists`），一个都不导入。
    #[serde(default)]
    pub skip_existing: bool,
}

/// 导入的结果：导入以后目标文件夹的目录快照，加上因为同名跳过的条目。快照的字段摊平在同一层，只认目录快照的
/// 调用方照旧能读。
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FileImportResponse {
    #[serde(flatten)]
    pub snapshot: FileBrowserSnapshot,
    /// `skipExisting` 时跳过的条目，写成它们在目标资源库里的相对路径；没有跳过时不写这个键。
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub skipped_entries: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FileArchiveImportRequest {
    pub repo_id: String,
    pub parent_path: Option<String>,
    pub archive_path: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct EagleLibraryImportRequest {
    pub repo_id: String,
    pub parent_path: Option<String>,
    pub library_path: String,
    pub mode: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EagleLibraryImportSummary {
    pub imported_files: usize,
    pub imported_directories: usize,
    pub imported_trash_entries: usize,
    pub imported_shortcuts: usize,
    pub imported_smart_folders: usize,
    pub imported_repository_actions: usize,
    pub imported_tag_groups: usize,
    pub imported_alias_groups: usize,
    pub imported_hardlink_groups: usize,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct EagleLibraryImportWarning {
    #[serde(rename = "type")]
    pub warning_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_relative_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conditions: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct EagleLibraryPluginImportResponse {
    pub summary: EagleLibraryImportSummary,
    pub warnings: Vec<EagleLibraryImportWarning>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct EagleLibraryImportResponse {
    pub snapshot: FileBrowserSnapshot,
    pub summary: EagleLibraryImportSummary,
    pub warnings: Vec<EagleLibraryImportWarning>,
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

#[derive(Debug, Serialize, Deserialize, Clone)]
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
pub(in super::super) struct TrashManifest {
    pub(in super::super) entries: Vec<TrashManifestEntry>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(in super::super) struct TrashManifestEntry {
    pub(in super::super) original_path: String,
    pub(in super::super) trash_path: String,
    pub(in super::super) deleted_at: String,
    pub(in super::super) kind: String,
}

#[derive(Debug)]
pub(in super::super) struct ThumbnailRecord {
    pub(in super::super) path: String,
    pub(in super::super) custom: bool,
}

#[derive(Debug, Clone)]
pub(in super::super) struct DirectoryRecord {
    pub(in super::super) path: String,
    pub(in super::super) parent_path: String,
    pub(in super::super) name: String,
    pub(in super::super) updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(in super::super) struct DirectoryPageResult {
    pub(in super::super) entries: Vec<FileSystemEntry>,
    pub(in super::super) total_entries: usize,
}

#[derive(Debug, Clone)]
pub(in super::super) struct NeteaseDirectoryCacheRecord {
    pub(in super::super) total_entries: usize,
    pub(in super::super) refreshed_at: String,
}

#[derive(Debug, Clone)]
pub(in super::super) struct RepositoryStructureCacheSnapshot {
    pub(in super::super) cache_state: RepositoryStructureCacheState,
    pub(in super::super) indexed_at: Option<String>,
}

#[derive(Debug, Clone)]
pub(in super::super) struct AssetPathRecord {
    pub(in super::super) asset_id: String,
    pub(in super::super) status: String,
    pub(in super::super) thumbnail_path: Option<String>,
    pub(in super::super) hardlink_group_id: Option<String>,
    pub(in super::super) hardlink_state: Option<String>,
    pub(in super::super) is_virtual: bool,
    pub(in super::super) provider_id: Option<String>,
    pub(in super::super) provider_item_id: Option<String>,
    pub(in super::super) source_payload: Option<serde_json::Value>,
    pub(in super::super) local_absolute_path: Option<String>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(in super::super) struct ExistingAssetRecord {
    pub(in super::super) asset_id: String,
    pub(in super::super) status: String,
    pub(in super::super) thumbnail_path: Option<String>,
    pub(in super::super) size_bytes: i64,
    pub(in super::super) created_at: String,
    pub(in super::super) modified_at: String,
    pub(in super::super) hash: Option<String>,
    pub(in super::super) is_virtual: bool,
    pub(in super::super) provider_id: Option<String>,
    pub(in super::super) provider_item_id: Option<String>,
    pub(in super::super) source_payload: Option<serde_json::Value>,
    pub(in super::super) local_absolute_path: Option<String>,
}

#[derive(Debug, Clone)]
pub(in super::super) struct SourceTrashEntryRecord {
    pub(in super::super) trash_path: String,
    pub(in super::super) original_path: String,
    pub(in super::super) kind: String,
    pub(in super::super) deleted_at: String,
    pub(in super::super) shared_asset_id: Option<String>,
}

#[derive(Debug, Clone)]
pub(in super::super) struct HardlinkCopyOutcome {
    pub(in super::super) source_path: Option<String>,
    pub(in super::super) target_path: String,
    pub(in super::super) link_state: String,
}

#[derive(Debug, Clone)]
pub(in super::super) struct HardlinkAssetRecord {
    pub(in super::super) asset_id: String,
    pub(in super::super) content_hash: String,
    pub(in super::super) size_bytes: i64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryMutationResponse {
    pub repository: RepositorySummary,
}
