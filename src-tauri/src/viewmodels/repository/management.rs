//! Repository management command orchestration.

use tauri::AppHandle;

use crate::services::repository::{
    NeteaseRepositoryCacheConfigureRequest, NeteaseRepositoryCacheConfigureResponse,
    RepositoryBackendConfigUpdateRequest, RepositoryDeleteRequest, RepositoryMutationResponse,
};
use crate::services::runtime::RepositoryRuntime;

#[derive(Clone)]
pub struct RepositoryManagementViewModel {
    runtime: RepositoryRuntime,
}

impl RepositoryManagementViewModel {
    pub fn new(runtime: RepositoryRuntime) -> Self {
        Self { runtime }
    }

    pub async fn delete_repository(&self, request: RepositoryDeleteRequest) -> Result<(), String> {
        self.runtime
            .run_repository_collection_write(move |state| state.delete_repository(request))
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

    pub async fn refresh_thumbnail_scope(&self, app: &AppHandle) -> Result<(), String> {
        let paths = self.runtime.repository_thumbnail_roots().await?;
        crate::app_shell::allow_thumbnail_asset_roots(app, paths)
    }
}
