import {
  getRegisteredPluginSettingsPage,
  listRegisteredPluginSettingsPages,
  type RegisteredPluginSettingsPage,
} from "./sdk";

export function listPluginSettingsPages() {
  return listRegisteredPluginSettingsPages();
}

export function getPluginSettingsPage(pluginId: string | null | undefined) {
  return getRegisteredPluginSettingsPage(pluginId);
}

export type { RegisteredPluginSettingsPage };
