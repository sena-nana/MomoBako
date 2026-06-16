use tauri::AppHandle;

use crate::repository_runtime::RepositoryRuntime;
use crate::repository_service::{
    NeteaseRepositoryCacheConfigureRequest, NeteaseRepositoryCacheConfigureResponse,
    RepositoryBackendConfigUpdateRequest, RepositoryExportRequest, RepositoryExportResponse,
    RepositoryFolderRequest, RepositoryMutationRequest, RepositoryMutationResponse,
    RepositoryRelocateRequest, SyncRequest, SyncResult,
};

#[derive(Clone)]
pub struct RepositoryManagementViewModel {
    runtime: RepositoryRuntime,
}

impl RepositoryManagementViewModel {
    pub fn new(runtime: RepositoryRuntime) -> Self {
        Self { runtime }
    }

    pub async fn create_repository(
        &self,
        request: RepositoryMutationRequest,
    ) -> Result<RepositoryMutationResponse, String> {
        self.runtime
            .run_repository_collection_write(move |state| state.create_repository(request))
            .await
    }

    pub async fn import_repository(
        &self,
        request: RepositoryMutationRequest,
    ) -> Result<RepositoryMutationResponse, String> {
        self.runtime
            .run_repository_collection_write(move |state| state.import_repository(request))
            .await
    }

    pub async fn attach_repository_folder(
        &self,
        request: RepositoryFolderRequest,
    ) -> Result<RepositoryMutationResponse, String> {
        self.runtime
            .run_repository_collection_write(move |state| state.attach_repository_folder(request))
            .await
    }

    pub async fn delete_repository(&self, repo_id: String) -> Result<(), String> {
        self.runtime
            .run_repository_collection_write(move |state| state.delete_repository(&repo_id))
            .await
    }

    pub async fn relocate_repository(
        &self,
        request: RepositoryRelocateRequest,
    ) -> Result<RepositoryMutationResponse, String> {
        self.runtime
            .run_repository_collection_write(move |state| state.relocate_repository(request))
            .await
    }

    pub async fn update_repository_backend_config(
        &self,
        request: RepositoryBackendConfigUpdateRequest,
    ) -> Result<RepositoryMutationResponse, String> {
        self.runtime
            .run_repository_collection_write(move |state| {
                state.update_repository_backend_config(request)
            })
            .await
    }

    pub async fn configure_netease_repository_cache(
        &self,
        request: NeteaseRepositoryCacheConfigureRequest,
    ) -> Result<NeteaseRepositoryCacheConfigureResponse, String> {
        self.runtime
            .run_repository_collection_write(move |state| {
                state.configure_netease_repository_cache(request)
            })
            .await
    }

    pub async fn export_repository(
        &self,
        request: RepositoryExportRequest,
    ) -> Result<RepositoryExportResponse, String> {
        self.runtime
            .run_write(move |state| state.export_repository(request))
            .await
    }

    pub async fn sync_repository(&self, request: SyncRequest) -> Result<SyncResult, String> {
        self.runtime
            .run_write(move |state| state.sync_repository(request))
            .await
    }

    pub async fn refresh_thumbnail_scope(&self, app: &AppHandle) -> Result<(), String> {
        let paths = self.runtime.repository_thumbnail_roots().await?;
        super::allow_thumbnail_asset_roots(app, paths)
    }
}
