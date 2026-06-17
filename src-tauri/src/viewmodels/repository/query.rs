//! Read-heavy repository command orchestration for snapshots, search, metadata, and file preview.

use crate::services::repository::{
    AssetDetail, FilePreviewSourceResponse, FileReadRequest, MetadataUpdateRequest,
    MetadataUpdateResponse, RepositorySnapshot, RepositorySummary, SearchRequest, SearchResponse,
};
use crate::services::runtime::RepositoryRuntime;

#[derive(Clone)]
pub struct RepositoryQueryViewModel {
    runtime: RepositoryRuntime,
}

impl RepositoryQueryViewModel {
    pub fn new(runtime: RepositoryRuntime) -> Self {
        Self { runtime }
    }

    pub async fn list_repositories(&self) -> Result<Vec<RepositorySummary>, String> {
        self.runtime
            .run_read(|state| state.list_repositories())
            .await
    }

    pub async fn get_repository_snapshot(
        &self,
        repo_id: String,
    ) -> Result<RepositorySnapshot, String> {
        self.runtime
            .run_read(move |state| state.load_snapshot(&repo_id))
            .await
    }

    pub async fn get_asset_detail(
        &self,
        repo_id: String,
        asset_id: String,
    ) -> Result<AssetDetail, String> {
        self.runtime
            .run_read(move |state| state.load_asset_detail(&repo_id, &asset_id))
            .await
    }

    pub async fn search_assets(&self, request: SearchRequest) -> Result<SearchResponse, String> {
        self.runtime
            .run_read(move |state| state.search_assets(request))
            .await
    }

    pub async fn update_asset_metadata(
        &self,
        request: MetadataUpdateRequest,
    ) -> Result<MetadataUpdateResponse, String> {
        self.runtime
            .run_write(move |state| state.update_asset_metadata(request))
            .await
    }

    pub async fn read_file(&self, request: FileReadRequest) -> Result<Vec<u8>, String> {
        self.runtime
            .run_read(move |state| state.read_file(request))
            .await
    }

    pub async fn prepare_preview_file_source(
        &self,
        request: FileReadRequest,
    ) -> Result<FilePreviewSourceResponse, String> {
        self.runtime
            .run_read(move |state| state.prepare_preview_file_source(request))
            .await
    }
}
