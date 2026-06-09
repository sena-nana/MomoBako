import type { FileBrowserEntry } from "../types/repository";
import threeModelPreviewPlugin from "../../plugins/builtin/three-model-preview/preview";
import mediaPreviewPlugin from "../../plugins/builtin/media-preview/preview";
import textPreviewPlugin from "../../plugins/builtin/text-preview/preview";
import officePreviewPlugin from "../../plugins/builtin/office-preview/preview";
import {
  getRegisteredPreviewPluginForEntry,
  listRegisteredPreviewPlugins,
  registerPreviewPlugin,
  type FilePreviewPlugin,
} from "./sdk";

registerPreviewPlugin(threeModelPreviewPlugin);
registerPreviewPlugin(mediaPreviewPlugin);
registerPreviewPlugin(textPreviewPlugin);
registerPreviewPlugin(officePreviewPlugin);

export function getPreviewPluginForEntry(entry: FileBrowserEntry | null) {
  return getRegisteredPreviewPluginForEntry(entry);
}

export function listPreviewPlugins() {
  return listRegisteredPreviewPlugins();
}

export type { FilePreviewPlugin };
