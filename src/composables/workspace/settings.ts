import {
  deletePlugin,
  getApiDesignSnapshot,
  getCacheSnapshot,
  getExternalApiConnectionStatus,
  installPluginFromArchive,
  listPlugins,
  setPluginEnabled,
} from "../../services/repositoryApi";
import { syncRegisteredPreviewPluginManifests } from "../../plugins/sdk";
import type { PluginManifest } from "../../types/repository";
import {
  apiDesign,
  cacheSnapshot,
  error,
  externalApiConnection,
  isLoadingSettingsData,
  isManagingPlugins,
  plugins,
} from "./state";

type SettingsDataLoadOptions = {
  failFast?: boolean;
};

function syncPreviewPluginsInBackground(items: PluginManifest[]) {
  void syncRegisteredPreviewPluginManifests(items).catch((cause) => {
    error.value = cause instanceof Error ? cause.message : String(cause);
  });
}

export async function loadSettingsData(options: SettingsDataLoadOptions = {}) {
  isLoadingSettingsData.value = true;

  try {
    const [pluginItems, cache, api, externalApi] = await Promise.all([
      listPlugins(),
      getCacheSnapshot(),
      getApiDesignSnapshot(),
      getExternalApiConnectionStatus(),
    ]);
    plugins.value = pluginItems;
    syncPreviewPluginsInBackground(pluginItems);
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
    syncPreviewPluginsInBackground(response.plugins);
    return response;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    return null;
  } finally {
    isManagingPlugins.value = false;
  }
}

export function setPluginEnabledInWorkspace(pluginId: string, enabled: boolean) {
  return applyPluginMutation(() => setPluginEnabled({ pluginId, enabled }));
}

export function deletePluginInWorkspace(pluginId: string) {
  return applyPluginMutation(() => deletePlugin(pluginId));
}

export function installPluginArchiveInWorkspace(packagePath: string) {
  return applyPluginMutation(() => installPluginFromArchive({ packagePath }));
}
