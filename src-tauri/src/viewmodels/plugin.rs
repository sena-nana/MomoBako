//! Plugin and cache command orchestration.

use crate::services::repository::{
    ApiDesignSnapshot, CacheSnapshot, PluginArchiveReadRequest, PluginArchiveTextResponse,
    PluginCallRequest, PluginCallResult, PluginConfigDeleteRequest, PluginConfigSetRequest,
    PluginConfigSnapshot, PluginDataDirectoryResponse, PluginDataFilePreviewSourceRequest,
    PluginDataFilePreviewSourceResponse, PluginEnabledRequest, PluginHookExecutionListRequest,
    PluginHookExecutionListResponse, PluginInstallRequest, PluginManifest, PluginMutationResponse,
};
use crate::services::runtime::RepositoryRuntime;

#[derive(Clone)]
pub struct PluginViewModel {
    runtime: RepositoryRuntime,
}

impl PluginViewModel {
    pub fn new(runtime: RepositoryRuntime) -> Self {
        Self { runtime }
    }

    pub async fn call_plugin(
        &self,
        request: PluginCallRequest,
    ) -> Result<PluginCallResult, String> {
        self.runtime
            .run_read(move |state| state.call_plugin(request))
            .await
    }

    pub async fn read_plugin_archive_text(
        &self,
        request: PluginArchiveReadRequest,
    ) -> Result<PluginArchiveTextResponse, String> {
        self.runtime
            .run_read(move |state| state.read_plugin_archive_text(request))
            .await
    }

    pub async fn get_plugin_data_directory(
        &self,
        plugin_id: String,
    ) -> Result<PluginDataDirectoryResponse, String> {
        self.runtime
            .run_write(move |state| state.get_plugin_data_directory(plugin_id))
            .await
    }

    pub async fn prepare_plugin_data_file_preview_source(
        &self,
        request: PluginDataFilePreviewSourceRequest,
    ) -> Result<PluginDataFilePreviewSourceResponse, String> {
        let mut response = self
            .runtime
            .run_read(move |state| state.prepare_plugin_data_file_preview_source(request))
            .await?;
        response.source_url = Some(self.runtime.preview_source_url(&response.token));
        Ok(response)
    }

    pub async fn get_plugin_config(
        &self,
        plugin_id: String,
    ) -> Result<PluginConfigSnapshot, String> {
        self.runtime
            .run_write(move |state| state.get_plugin_config(plugin_id))
            .await
    }

    pub async fn set_plugin_config_value(
        &self,
        request: PluginConfigSetRequest,
    ) -> Result<PluginConfigSnapshot, String> {
        self.runtime
            .run_write(move |state| state.set_plugin_config_value(request))
            .await
    }

    pub async fn delete_plugin_config_value(
        &self,
        request: PluginConfigDeleteRequest,
    ) -> Result<PluginConfigSnapshot, String> {
        self.runtime
            .run_write(move |state| state.delete_plugin_config_value(request))
            .await
    }

    pub async fn list_plugins(&self) -> Result<Vec<PluginManifest>, String> {
        self.runtime.run_read(|state| state.list_plugins()).await
    }

    pub async fn list_plugin_hook_executions(
        &self,
        request: Option<PluginHookExecutionListRequest>,
    ) -> Result<PluginHookExecutionListResponse, String> {
        self.runtime
            .run_read(move |state| state.list_plugin_hook_executions(request.unwrap_or_default()))
            .await
    }

    pub async fn set_plugin_enabled(
        &self,
        request: PluginEnabledRequest,
    ) -> Result<PluginMutationResponse, String> {
        self.runtime
            .run_write(move |state| state.set_plugin_enabled(request))
            .await
    }

    pub async fn delete_plugin(&self, plugin_id: String) -> Result<PluginMutationResponse, String> {
        self.runtime
            .run_write(move |state| state.delete_plugin(plugin_id))
            .await
    }

    pub async fn install_plugin_from_archive(
        &self,
        request: PluginInstallRequest,
    ) -> Result<PluginMutationResponse, String> {
        self.runtime
            .run_write(move |state| state.install_plugin_from_archive(request))
            .await
    }

    pub async fn get_cache_snapshot(&self) -> Result<CacheSnapshot, String> {
        self.runtime
            .run_read(|state| state.get_cache_snapshot())
            .await
    }

    pub async fn get_api_design_snapshot(&self) -> Result<ApiDesignSnapshot, String> {
        self.runtime
            .run_read(|state| state.get_api_design_snapshot())
            .await
    }
}
