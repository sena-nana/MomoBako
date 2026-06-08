import { defineAsyncComponent, type Component } from "vue";
import type { FileBrowserEntry } from "../types/repository";
import { audioPreviewExtensions, videoPreviewExtensions } from "./mediaPreview/mediaExtensions";

export type FilePreviewPlugin = {
  pluginId: string;
  name: string;
  kind: "preview";
  supportedExtensions: string[];
  component: Component;
};

const previewPlugins: FilePreviewPlugin[] = [
  {
    pluginId: "builtin.three-model-preview",
    name: "3D Model Preview",
    kind: "preview",
    supportedExtensions: ["fbx", "obj", "glb", "gltf", "vrm"],
    component: defineAsyncComponent(() => import("./threeModelPreview/ThreeModelPreview.vue")),
  },
  {
    pluginId: "builtin.media-preview",
    name: "Media Preview",
    kind: "preview",
    supportedExtensions: [...videoPreviewExtensions, ...audioPreviewExtensions],
    component: defineAsyncComponent(() => import("./mediaPreview/MediaPreview.vue")),
  },
];

export function getPreviewPluginForEntry(entry: FileBrowserEntry | null) {
  const extension = entry?.extension?.toLowerCase();
  if (!extension) return null;
  return previewPlugins.find((plugin) => plugin.supportedExtensions.includes(extension)) ?? null;
}

export function listPreviewPlugins() {
  return previewPlugins;
}
