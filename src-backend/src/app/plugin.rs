//! Plugin and cache orchestration.

use crate::services::repository::{
    ApiDesignSnapshot, CacheSnapshot, PlaylistPlayerContribution, PluginCallRequest,
    PluginCallResult, PluginConfigDeleteRequest, PluginConfigSetRequest, PluginConfigSnapshot,
    PluginDataDirectoryResponse, PluginEnabledRequest, PluginHookExecutionListRequest,
    PluginHookExecutionListResponse, PluginInstallRequest, PluginManifest, PluginMutationResponse,
};
use crate::services::runtime::RepositoryRuntime;

#[derive(Clone)]
pub struct PluginService {
    runtime: RepositoryRuntime,
}

impl PluginService {
    pub fn new(runtime: RepositoryRuntime) -> Self {
        Self { runtime }
    }

    pub fn call_plugin(&self, request: PluginCallRequest) -> Result<PluginCallResult, String> {
        self.runtime.read(move |state| state.call_plugin(request))
    }

    pub fn get_plugin_data_directory(
        &self,
        plugin_id: String,
    ) -> Result<PluginDataDirectoryResponse, String> {
        self.runtime
            .write(move |state| state.get_plugin_data_directory(plugin_id))
    }

    pub fn get_plugin_config(&self, plugin_id: String) -> Result<PluginConfigSnapshot, String> {
        self.runtime
            .write(move |state| state.get_plugin_config(plugin_id))
    }

    pub fn set_plugin_config_value(
        &self,
        request: PluginConfigSetRequest,
    ) -> Result<PluginConfigSnapshot, String> {
        let snapshot = self
            .runtime
            .write(move |state| state.set_plugin_config_value(request))?;
        self.runtime.reload_mutsuki_plugins()?;
        Ok(snapshot)
    }

    pub fn delete_plugin_config_value(
        &self,
        request: PluginConfigDeleteRequest,
    ) -> Result<PluginConfigSnapshot, String> {
        let snapshot = self
            .runtime
            .write(move |state| state.delete_plugin_config_value(request))?;
        self.runtime.reload_mutsuki_plugins()?;
        Ok(snapshot)
    }

    pub fn list_plugins(&self) -> Result<Vec<PluginManifest>, String> {
        self.runtime.read(|state| state.list_plugins())
    }

    pub fn list_playlist_players(&self) -> Result<Vec<PlaylistPlayerContribution>, String> {
        self.runtime.read(|state| state.list_playlist_players())
    }

    pub fn list_plugin_hook_executions(
        &self,
        request: Option<PluginHookExecutionListRequest>,
    ) -> Result<PluginHookExecutionListResponse, String> {
        self.runtime
            .read(move |state| state.list_plugin_hook_executions(request.unwrap_or_default()))
    }

    pub fn set_plugin_enabled(
        &self,
        request: PluginEnabledRequest,
    ) -> Result<PluginMutationResponse, String> {
        let response = self
            .runtime
            .write(move |state| state.set_plugin_enabled(request))?;
        self.runtime.reload_mutsuki_plugins()?;
        Ok(response)
    }

    pub fn delete_plugin(&self, plugin_id: String) -> Result<PluginMutationResponse, String> {
        let response = self
            .runtime
            .write(move |state| state.delete_plugin(plugin_id))?;
        self.runtime.reload_mutsuki_plugins()?;
        Ok(response)
    }

    pub fn install_plugin_from_archive(
        &self,
        request: PluginInstallRequest,
    ) -> Result<PluginMutationResponse, String> {
        let response = self
            .runtime
            .write(move |state| state.install_plugin_from_archive(request))?;
        self.runtime.reload_mutsuki_plugins()?;
        Ok(response)
    }

    /// 资源库的缓存占用：缩略图和 `.momo/cache` 下的各子缓存。
    pub fn get_cache_snapshot(&self, repo_id: String) -> Result<CacheSnapshot, String> {
        self.runtime
            .read(move |state| state.get_cache_snapshot(&repo_id))
    }

    pub fn get_api_design_snapshot(&self) -> Result<ApiDesignSnapshot, String> {
        self.runtime.read(|state| state.get_api_design_snapshot())
    }
}
