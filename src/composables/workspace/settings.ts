import {
  deletePlugin,
  deletePluginConfigValue,
  getApiDesignSnapshot,
  getCacheSnapshot,
  getExternalApiConnectionStatus,
  getPluginConfig,
  getPluginDataDirectory,
  installPluginFromArchive,
  listPluginHookExecutions,
  listPlugins,
  openRepositoryPath,
  setPluginConfigValue,
  setPluginEnabled,
} from "../../services/repositoryApi";
import { emitSystemLogSilently } from "../../services/systemLog";
import { syncRegisteredFrontendPluginManifests } from "../../plugins/sdk";
import type { PluginConfigSnapshot, PluginConfigValue, PluginManifest } from "../../types/repository";
import {
  apiDesign,
  cacheSnapshot,
  error,
  externalApiConnection,
  isLoadingSettingsData,
  isManagingPlugins,
  pluginHookExecutions,
  plugins,
} from "./state";

type SettingsDataLoadOptions = {
  failFast?: boolean;
};

async function syncPreviewPlugins(items: PluginManifest[]) {
  try {
    await syncRegisteredFrontendPluginManifests(items);
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
  }
}

export async function loadSettingsData(options: SettingsDataLoadOptions = {}) {
  isLoadingSettingsData.value = true;

  try {
    const [pluginItems, hookExecutionResponse, cache, api, externalApi] = await Promise.all([
      listPlugins(),
      listPluginHookExecutions({ limit: 200 }),
      getCacheSnapshot(),
      getApiDesignSnapshot(),
      getExternalApiConnectionStatus(),
    ]);
    plugins.value = pluginItems;
    pluginHookExecutions.value = hookExecutionResponse.records;
    await syncPreviewPlugins(pluginItems);
    cacheSnapshot.value = cache;
    apiDesign.value = api;
    externalApiConnection.value = externalApi;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    if (options.failFast) {
      throw cause;
    }
  } finally {
    isLoadingSettingsData.value = false;
  }
}

async function applyPluginMutation(action: () => Promise<{ plugins: PluginManifest[] }>) {
  isManagingPlugins.value = true;
  error.value = null;
  try {
    const response = await action();
    plugins.value = response.plugins;
    await syncPreviewPlugins(response.plugins);
    return response;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    return null;
  } finally {
    isManagingPlugins.value = false;
  }
}

export function setPluginEnabledInWorkspace(pluginId: string, enabled: boolean) {
  emitSystemLogSilently("info", {
    category: "plugin",
    action: enabled ? "enableStart" : "disableStart",
    message: enabled ? "开始启用插件。" : "开始停用插件。",
    pluginId,
    context: { enabled },
  });
  return applyPluginMutation(() => setPluginEnabled({ pluginId, enabled }));
}

export function deletePluginInWorkspace(pluginId: string) {
  emitSystemLogSilently("warn", {
    category: "plugin",
    action: "deleteStart",
    message: "开始删除插件。",
    pluginId,
  });
  return applyPluginMutation(() => deletePlugin(pluginId));
}

export function installPluginArchiveInWorkspace(packagePath: string) {
  emitSystemLogSilently("info", {
    category: "plugin",
    action: "installStart",
    message: "开始安装插件包。",
    context: { packagePath },
  });
  return applyPluginMutation(() => installPluginFromArchive({ packagePath }));
}

export async function openPluginDataDirectoryInWorkspace(pluginId: string) {
  isManagingPlugins.value = true;
  error.value = null;
  try {
    const directory = await getPluginDataDirectory(pluginId);
    await openRepositoryPath(directory.path);
    return directory;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    return null;
  } finally {
    isManagingPlugins.value = false;
  }
}

export async function loadPluginConfigInWorkspace(pluginId: string) {
  isManagingPlugins.value = true;
  error.value = null;
  try {
    return await getPluginConfig(pluginId);
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    return null;
  } finally {
    isManagingPlugins.value = false;
  }
}

export async function setPluginConfigValueInWorkspace(
  pluginId: string,
  key: string,
  value: PluginConfigValue,
): Promise<PluginConfigSnapshot | null> {
  emitSystemLogSilently("info", {
    category: "plugin.config",
    action: "setValueStart",
    message: "开始更新插件配置。",
    pluginId,
    context: { key },
  });
  isManagingPlugins.value = true;
  error.value = null;
  try {
    const response = await setPluginConfigValue({ pluginId, key, value });
    emitSystemLogSilently("info", {
      category: "plugin.config",
      action: "setValueSuccess",
      message: "插件配置已更新。",
      pluginId,
      context: { key },
    });
    return response;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    emitSystemLogSilently("error", {
      category: "plugin.config",
      action: "setValueFailed",
      message: "插件配置更新失败。",
      pluginId,
      context: { key, error: error.value },
    });
    return null;
  } finally {
    isManagingPlugins.value = false;
  }
}

export async function deletePluginConfigValueInWorkspace(pluginId: string, key: string) {
  emitSystemLogSilently("warn", {
    category: "plugin.config",
    action: "deleteValueStart",
    message: "开始删除插件配置。",
    pluginId,
    context: { key },
  });
  isManagingPlugins.value = true;
  error.value = null;
  try {
    const response = await deletePluginConfigValue({ pluginId, key });
    emitSystemLogSilently("warn", {
      category: "plugin.config",
      action: "deleteValueSuccess",
      message: "插件配置已删除。",
      pluginId,
      context: { key },
    });
    return response;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    emitSystemLogSilently("error", {
      category: "plugin.config",
      action: "deleteValueFailed",
      message: "插件配置删除失败。",
      pluginId,
      context: { key, error: error.value },
    });
    return null;
  } finally {
    isManagingPlugins.value = false;
  }
}
