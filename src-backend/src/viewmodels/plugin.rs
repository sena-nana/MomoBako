//! Plugin and cache command orchestration.

use crate::services::repository::{
    ApiDesignSnapshot, CacheSnapshot, PluginCallRequest, PluginCallResult,
    PluginConfigDeleteRequest, PluginConfigSetRequest, PluginConfigSnapshot,
    PluginDataDirectoryResponse, PluginEnabledRequest, PluginHookExecutionListRequest,
    PluginHookExecutionListResponse, PluginInstallRequest, PluginManifest, PluginMutationResponse,
    PlaylistPlayerContribution,
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

    pub async fn get_plugin_data_directory(
        &self,
        plugin_id: String,
    ) -> Result<PluginDataDirectoryResponse, String> {
        self.runtime
            .run_write(move |state| state.get_plugin_data_directory(plugin_id))
            .await
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
        let snapshot = self
            .runtime
            .run_write(move |state| state.set_plugin_config_value(request))
            .await?;
        self.runtime.reload_mutsuki_plugins().await?;
        Ok(snapshot)
    }

    pub async fn delete_plugin_config_value(
        &self,
        request: PluginConfigDeleteRequest,
    ) -> Result<PluginConfigSnapshot, String> {
        let snapshot = self
            .runtime
            .run_write(move |state| state.delete_plugin_config_value(request))
            .await?;
        self.runtime.reload_mutsuki_plugins().await?;
        Ok(snapshot)
    }

    pub async fn list_plugins(&self) -> Result<Vec<PluginManifest>, String> {
        self.runtime.run_read(|state| state.list_plugins()).await
    }

    pub async fn list_playlist_players(&self) -> Result<Vec<PlaylistPlayerContribution>, String> {
        self.runtime
            .run_read(|state| state.list_playlist_players())
            .await
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
        let response = self
            .runtime
            .run_write(move |state| state.set_plugin_enabled(request))
            .await?;
        self.runtime.reload_mutsuki_plugins().await?;
        Ok(response)
    }

    pub async fn delete_plugin(&self, plugin_id: String) -> Result<PluginMutationResponse, String> {
        let response = self
            .runtime
            .run_write(move |state| state.delete_plugin(plugin_id))
            .await?;
        self.runtime.reload_mutsuki_plugins().await?;
        Ok(response)
    }

    pub async fn install_plugin_from_archive(
        &self,
        request: PluginInstallRequest,
    ) -> Result<PluginMutationResponse, String> {
        let response = self
            .runtime
            .run_write(move |state| state.install_plugin_from_archive(request))
            .await?;
        self.runtime.reload_mutsuki_plugins().await?;
        Ok(response)
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
