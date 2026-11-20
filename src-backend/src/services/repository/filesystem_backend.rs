//! Filesystem backend adapter abstraction and browser entry mapping.

use super::*;

mod entries;
mod operations;
mod runtime_adapter;

pub(super) use entries::*;
pub(super) use operations::*;

pub(super) trait FileSystemBackendAdapter {
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

    fn list_directory_entries_page(
        &self,
        repo_root: &Path,
        directory_path: &str,
        offset: usize,
        limit: usize,
        config: &serde_json::Value,
    ) -> Result<DirectoryPageResult, String>;

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

    fn describe_repository_state(
        &self,
        repo_root: &Path,
        config: &serde_json::Value,
    ) -> Result<SourceRepositoryStateSnapshot, String>;

    fn write_asset_metadata(
        &self,
        repo_root: &Path,
        path: &str,
        shared_asset_id: Option<&str>,
        metadata: &BTreeMap<String, serde_json::Value>,
        previous_metadata: &BTreeMap<String, serde_json::Value>,
        operation: &str,
        config: &serde_json::Value,
    ) -> Result<(), String>;

    fn write_repository_state(
        &self,
        repo_root: &Path,
        state: &SourceRepositoryStateSnapshot,
        config: &serde_json::Value,
    ) -> Result<(), String>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum FileSystemEntryKind {
    Directory,
    File,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct FileSystemEntry {
    pub(super) path: String,
    pub(super) name: String,
    pub(super) kind: FileSystemEntryKind,
    pub(super) extension: Option<String>,
    pub(super) size_bytes: Option<i64>,
    pub(super) modified_at: Option<String>,
    #[serde(default)]
    pub(super) is_virtual: bool,
    #[serde(default)]
    pub(super) provider_id: Option<String>,
    #[serde(default)]
    pub(super) provider_item_id: Option<String>,
    #[serde(default)]
    pub(super) source_payload: Option<serde_json::Value>,
    #[serde(default)]
    pub(super) local_absolute_path: Option<String>,
    #[serde(default)]
    pub(super) status: Option<String>,
    #[serde(default)]
    pub(super) shared_asset_id: Option<String>,
    #[serde(default)]
    pub(super) tags: Option<Vec<String>>,
    #[serde(default)]
    pub(super) thumbnail_local_absolute_path: Option<String>,
}

pub(super) struct RuntimeFileSystemBackendAdapter {
    pub(super) service_root: PathBuf,
    pub(super) plugin_id: String,
}
