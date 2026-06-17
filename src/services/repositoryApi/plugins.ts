import type {
  ApiDesignSnapshot,
  CacheSnapshot,
  PluginArchiveReadRequest,
  PluginArchiveTextResponse,
  PluginCallRequest,
  PluginCallResponse,
  PluginConfigDeleteRequest,
  PluginConfigSetRequest,
  PluginConfigSnapshot,
  PluginDataDirectoryResponse,
  PluginDataFilePreviewSourceRequest,
  PluginDataFilePreviewSourceResponse,
  PluginEnabledRequest,
  PluginHookExecutionListRequest,
  PluginHookExecutionListResponse,
  PluginInstallRequest,
  PluginManifest,
  PluginMutationResponse,
} from "../../types/repository";
import { invokeCommand } from "./core";

export function callPlugin<T = unknown>(request: PluginCallRequest) {
  return invokeCommand<PluginCallResponse<T>>("call_plugin", { request });
}

export function readPluginArchiveText(request: PluginArchiveReadRequest) {
  return invokeCommand<PluginArchiveTextResponse>("read_plugin_archive_text", { request });
}

export function getPluginDataDirectory(pluginId: string) {
  return invokeCommand<PluginDataDirectoryResponse>("get_plugin_data_directory", { pluginId });
}

export function preparePluginDataFilePreviewSource(request: PluginDataFilePreviewSourceRequest) {
  return invokeCommand<PluginDataFilePreviewSourceResponse>(
    "prepare_plugin_data_file_preview_source",
    { request },
  );
}

export function getPluginConfig(pluginId: string) {
  return invokeCommand<PluginConfigSnapshot>("get_plugin_config", { pluginId });
}

export function setPluginConfigValue(request: PluginConfigSetRequest) {
  return invokeCommand<PluginConfigSnapshot>("set_plugin_config_value", { request });
}

export function deletePluginConfigValue(request: PluginConfigDeleteRequest) {
  return invokeCommand<PluginConfigSnapshot>("delete_plugin_config_value", { request });
}

export function listPlugins() {
  return invokeCommand<PluginManifest[]>("list_plugins");
}

export function listPluginHookExecutions(request?: PluginHookExecutionListRequest) {
  return invokeCommand<PluginHookExecutionListResponse>("list_plugin_hook_executions", { request });
}

export function setPluginEnabled(request: PluginEnabledRequest) {
  return invokeCommand<PluginMutationResponse>("set_plugin_enabled", { request });
}

export function deletePlugin(pluginId: string) {
  return invokeCommand<PluginMutationResponse>("delete_plugin", { pluginId });
}

export function installPluginFromArchive(request: PluginInstallRequest) {
  return invokeCommand<PluginMutationResponse>("install_plugin_from_archive", { request });
}

export function getCacheSnapshot() {
  return invokeCommand<CacheSnapshot>("get_cache_snapshot");
}

export function getApiDesignSnapshot() {
  return invokeCommand<ApiDesignSnapshot>("get_api_design_snapshot");
}
