//! Internal repository records passed between repository service modules.

use super::*;

#[derive(Debug)]
pub(super) struct RepositorySeed<'a> {
    pub(super) repo_id: &'a str,
    pub(super) name: &'a str,
    pub(super) root_path: &'a str,
    pub(super) status: &'a str,
    pub(super) assets: &'a [AssetSeed<'a>],
}

#[derive(Debug)]
pub(super) struct AssetSeed<'a> {
    pub(super) asset_id: &'a str,
    pub(super) path: &'a str,
    pub(super) filename: &'a str,
    pub(super) extension: &'a str,
    pub(super) size_bytes: i64,
    pub(super) modified_at: &'a str,
    pub(super) status: &'a str,
    pub(super) tags: &'a [&'a str],
    pub(super) metadata: &'a [(&'a str, &'a str, &'a str)],
}

#[derive(Debug, Clone)]
pub(super) struct RepositoryBackendRecord {
    pub(super) plugin_id: String,
    pub(super) config: serde_json::Value,
}

#[derive(Debug, Clone)]
pub(super) struct RepositoryRecord {
    pub(super) summary: RepositorySummary,
    pub(super) backend_record: RepositoryBackendRecord,
}

#[derive(Debug, Clone)]
pub(super) struct RepositoryStoragePaths {
    pub(super) metadata_dir: PathBuf,
    pub(super) database_path: PathBuf,
}

#[derive(Debug, Clone)]
pub(super) struct PreviewFileSource {
    pub(super) path: PathBuf,
    pub(super) media_type: String,
}
