//! Repository management orchestration.

use crate::services::repository::{
    NeteaseRepositoryCacheConfigureRequest, NeteaseRepositoryCacheConfigureResponse,
    RepositoryBackendConfigUpdateRequest, RepositoryDeleteRequest, RepositoryMutationResponse,
};
use crate::services::runtime::RepositoryRuntime;

#[derive(Clone)]
pub struct RepositoryManagementService {
    runtime: RepositoryRuntime,
}

impl RepositoryManagementService {
    pub fn new(runtime: RepositoryRuntime) -> Self {
        Self { runtime }
    }

    pub fn delete_repository(&self, request: RepositoryDeleteRequest) -> Result<(), String> {
        self.runtime
            .collection_write(move |state| state.delete_repository(request))
    }

    pub fn update_repository_backend_config(
        &self,
        request: RepositoryBackendConfigUpdateRequest,
    ) -> Result<RepositoryMutationResponse, String> {
        self.runtime
            .collection_write(move |state| state.update_repository_backend_config(request))
    }

    pub fn configure_netease_repository_cache(
        &self,
        request: NeteaseRepositoryCacheConfigureRequest,
    ) -> Result<NeteaseRepositoryCacheConfigureResponse, String> {
        self.runtime
            .collection_write(move |state| state.configure_netease_repository_cache(request))
    }
}
