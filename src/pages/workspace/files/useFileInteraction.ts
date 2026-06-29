import { nextTick, watch, type ComputedRef } from "vue";
import { getWorkspaceParentPath } from "../dragBehavior";
import type {
  FileBrowserEntry,
  FileBrowserSnapshot,
  SearchHit,
} from "../../../types/repository";

type FileBrowserLoadOptions = {
  includeTree?: boolean;
  specialLocation?: "trash";
  append?: boolean;
  limit?: number;
  silent?: boolean;
};

type WorkspaceFileInteractionOptions = {
  activeAssetId: ComputedRef<string | null>;
  activeRepoId: ComputedRef<string | null>;
  fileBrowser: ComputedRef<FileBrowserSnapshot | null>;
  isFileBrowserPanel: ComputedRef<boolean>;
  isTrashPanel: ComputedRef<boolean>;
  loadFileBrowserForDirectory: (
    directoryPath?: string,
    options?: FileBrowserLoadOptions,
  ) => Promise<FileBrowserSnapshot | null>;
  saveAssetMetadata: (metadata: Record<string, unknown>) => Promise<unknown>;
  selectAsset: (assetId: string) => Promise<unknown>;
  selectRepository: (repoId: string) => Promise<unknown>;
  selectWorkspaceEntry: (path: string, options?: { mode?: "replace" | "toggle" | "range" }) => void;
  selectWorkspaceEntries: (
    paths: string[],
    options?: { primaryPath?: string | null; anchorPath?: string | null },
  ) => void;
  setActivePanel: (panel: "files") => void;
  setActivePreviewPath: (path: string | null) => void;
  setDragHoverFolderPath: (path: string | null) => void;
  setPreviewFilePath: (path: string | null) => void;
};

export function useFileInteraction(options: WorkspaceFileInteractionOptions) {
  function openDirectory(path: string) {
    options.setDragHoverFolderPath(null);
    void options.loadFileBrowserForDirectory(path, options.isTrashPanel.value
      ? { specialLocation: "trash", silent: true }
      : { silent: true });
  }

  function selectFileEntry(entry: FileBrowserEntry, mode: "replace" | "toggle" | "range") {
    options.selectWorkspaceEntry(entry.path, { mode });
  }

  async function saveFileMetadata(entry: FileBrowserEntry, metadata: Record<string, unknown>) {
    if (entry.kind !== "file" || !entry.assetId) return null;
    if (options.activeAssetId.value !== entry.assetId) {
      await options.selectAsset(entry.assetId);
    }
    return options.saveAssetMetadata(metadata);
  }

  function previewFileEntryByDoubleClick(entry: FileBrowserEntry) {
    if (entry.kind !== "file" || options.isTrashPanel.value) return;
    options.selectWorkspaceEntries([entry.path], { primaryPath: entry.path, anchorPath: entry.path });
    options.setActivePreviewPath(entry.path);
    options.setPreviewFilePath(entry.path);
  }

  function exitPreview() {
    options.setPreviewFilePath(null);
    options.setActivePreviewPath(null);
  }

  async function openSearchHit(result: SearchHit) {
    options.setPreviewFilePath(null);
    options.setActivePanel("files");

    if (options.activeRepoId.value !== result.repoId) {
      await options.selectRepository(result.repoId);
    }
    if (options.activeRepoId.value !== result.repoId) return;

    const snapshot = await options.loadFileBrowserForDirectory(getWorkspaceParentPath(result.path), { includeTree: true });
    const matchedEntry = snapshot?.entries.find((entry) => entry.path === result.path);
    if (matchedEntry) {
      options.selectWorkspaceEntries([matchedEntry.path], {
        primaryPath: matchedEntry.path,
        anchorPath: matchedEntry.path,
      });
      if (matchedEntry.kind === "file") {
        await nextTick();
        options.setPreviewFilePath(matchedEntry.path);
      }
    }

    await options.selectAsset(result.assetId);
  }

  watch(
    () => options.isFileBrowserPanel.value,
    (enabled) => {
      if (enabled && options.activeRepoId.value && !options.fileBrowser.value) {
        void options.loadFileBrowserForDirectory(
          "",
          options.isTrashPanel.value ? { specialLocation: "trash" } : { includeTree: true },
        );
      }
    },
  );

  return {
    exitPreview,
    openDirectory,
    openSearchHit,
    previewFileEntryByDoubleClick,
    saveFileMetadata,
    selectFileEntry,
  };
}
