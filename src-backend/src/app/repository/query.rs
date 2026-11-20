//! Read-heavy repository orchestration for snapshots, search, metadata, and file preview.

use crate::services::repository::{
    AssetDetail, FilePreviewSourceResponse, FileReadRequest, MetadataUpdateRequest,
    MetadataUpdateResponse, RepositorySnapshot, RepositorySummary, SearchRequest, SearchResponse,
};
use crate::services::runtime::RepositoryRuntime;

#[derive(Clone)]
pub struct RepositoryQueryService {
    runtime: RepositoryRuntime,
}

impl RepositoryQueryService {
    pub fn new(runtime: RepositoryRuntime) -> Self {
        Self { runtime }
    }

    pub fn list_repositories(&self) -> Result<Vec<RepositorySummary>, String> {
        self.runtime.read(|state| state.list_repositories())
    }

    pub fn get_repository_snapshot(&self, repo_id: String) -> Result<RepositorySnapshot, String> {
        self.runtime
            .read(move |state| state.load_snapshot(&repo_id))
    }

    pub fn get_asset_detail(
        &self,
        repo_id: String,
        asset_id: String,
    ) -> Result<AssetDetail, String> {
        self.runtime
            .read(move |state| state.load_asset_detail(&repo_id, &asset_id))
    }

    pub fn search_assets(&self, request: SearchRequest) -> Result<SearchResponse, String> {
        self.runtime.read(move |state| state.search_assets(request))
    }

    pub fn update_asset_metadata(
        &self,
        request: MetadataUpdateRequest,
    ) -> Result<MetadataUpdateResponse, String> {
        self.runtime
            .write(move |state| state.update_asset_metadata(request))
    }

    pub fn read_file(&self, request: FileReadRequest) -> Result<Vec<u8>, String> {
        self.runtime.read(move |state| state.read_file(request))
    }

    pub fn prepare_preview_file_source(
        &self,
        request: FileReadRequest,
    ) -> Result<FilePreviewSourceResponse, String> {
        self.runtime
            .write(move |state| state.prepare_preview_file_source(request))
    }
}
