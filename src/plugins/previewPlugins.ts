import type { FileBrowserEntry } from "../types/repository";
import {
  getRegisteredEntryActions,
  getRegisteredPreviewPluginForEntry,
  listRegisteredPreviewPlugins,
  type EntryActionContext,
  type EntryAction,
  type FilePreviewPlugin,
  type PreviewPluginFileAction,
} from "./sdk";
import { getManifestSourceEntryActions } from "./sourceEntryActions";

export function getPreviewPluginForEntry(entry: FileBrowserEntry | null) {
  return getRegisteredPreviewPluginForEntry(entry);
}

export function listPreviewPlugins() {
  return listRegisteredPreviewPlugins();
}

export function getPreviewPluginFileActions(
  repoId: string,
  entry: FileBrowserEntry | null,
): PreviewPluginFileAction[] {
  const plugin = getRegisteredPreviewPluginForEntry(entry);
  if (!plugin?.getFileActions || !entry) return [];
  return plugin.getFileActions({ repoId, entry });
}

export function getPluginEntryActions(context: EntryActionContext): EntryAction[] {
  return [
    ...getManifestSourceEntryActions(context),
    ...getRegisteredEntryActions(context),
  ];
}

export type { FilePreviewPlugin };
