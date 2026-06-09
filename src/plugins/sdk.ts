import type { Component } from "vue";
import type { FileBrowserEntry, PluginManifest } from "../types/repository";

export type FilePreviewPlugin = {
  pluginId: string;
  name: string;
  kind: "preview";
  supportedExtensions: string[];
  component: Component;
  generateThumbnail?: (context: {
    repoId: string;
    entry: FileBrowserEntry;
  }) => Promise<{ bytes: number[]; mediaType: string } | null>;
  manifest?: PluginManifest;
};

export type PreviewPluginDefinition = {
  manifest: PluginManifest;
  supportedExtensions: string[];
  component: Component;
  generateThumbnail?: FilePreviewPlugin["generateThumbnail"];
};

const previewPluginRegistry = new Map<string, FilePreviewPlugin>();

export function definePreviewPlugin(definition: PreviewPluginDefinition) {
  const extensions = definition.supportedExtensions
    .map((extension) => extension.trim().toLowerCase())
    .filter(Boolean);
  return {
    pluginId: definition.manifest.pluginId,
    name: definition.manifest.name,
    kind: "preview" as const,
    supportedExtensions: [...new Set(extensions)],
    component: definition.component,
    generateThumbnail: definition.generateThumbnail,
    manifest: definition.manifest,
  };
}

export function registerPreviewPlugin(plugin: FilePreviewPlugin) {
  previewPluginRegistry.set(plugin.pluginId, plugin);
  return plugin;
}

export function listRegisteredPreviewPlugins() {
  return [...previewPluginRegistry.values()];
}

export function syncRegisteredPreviewPluginManifests(manifests: PluginManifest[]) {
  const manifestMap = new Map(manifests.map((manifest) => [manifest.pluginId, manifest]));
  for (const plugin of previewPluginRegistry.values()) {
    const manifest = manifestMap.get(plugin.pluginId);
    if (!manifest) continue;
    plugin.manifest = manifest;
  }
}

export function getRegisteredPreviewPluginForEntry(entry: FileBrowserEntry | null) {
  const extension = entry?.extension?.toLowerCase();
  if (!extension) return null;
  return listRegisteredPreviewPlugins()
    .filter((plugin) => plugin.manifest?.enabled ?? true)
    .find((plugin) => plugin.supportedExtensions.includes(extension)) ?? null;
}

export function clearPreviewPluginRegistry() {
  previewPluginRegistry.clear();
}
