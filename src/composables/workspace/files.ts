import { getFileBrowser } from "../../services/repositoryApi";
import type { FileBrowserSnapshot } from "../../types/repository";
import {
  activePanel,
  activeRepoId,
  activeSnapshot,
  currentDirectoryPath,
  error,
  fileBrowser,
  fileTree,
  isLoadingFileBrowser,
  selectedFilePaths,
  selectionAnchorPath,
  selectedFilePath,
} from "./state";
import {
  cancelOperationProgress,
  finishOperationProgress,
  startOperationProgress,
  updateOperationProgress,
} from "./tasks";
import { loadThumbnailsForSnapshot } from "./thumbnails";
import { joinRepositoryPath } from "./paths";

export type FileBrowserLoadOptions = {
  includeTree?: boolean;
  specialLocation?: "trash";
};

export { entryNameFromPath } from "./paths";

export function getDefaultFileBrowserSelection(snapshot: FileBrowserSnapshot) {
  return snapshot.entries.find((entry) => entry.kind === "file")?.path
    ?? snapshot.entries[0]?.path
    ?? null;
}

export function applyFileBrowserSnapshot(snapshot: FileBrowserSnapshot) {
  const displaySnapshot = {
    ...snapshot,
    entries: snapshot.entries.map((entry) => ({ ...entry })),
  };
  fileBrowser.value = displaySnapshot;
  if (displaySnapshot.tree) {
    fileTree.value = displaySnapshot.tree;
  }
  currentDirectoryPath.value = displaySnapshot.currentPath;

  const entryPaths = new Set(displaySnapshot.entries.map((entry) => entry.path));
  let nextSelectedPaths = selectedFilePaths.value.filter((path) => entryPaths.has(path));
  let nextPrimaryPath = selectedFilePath.value && entryPaths.has(selectedFilePath.value)
    ? selectedFilePath.value
    : null;

  if (nextPrimaryPath && !nextSelectedPaths.includes(nextPrimaryPath)) {
    nextSelectedPaths = [nextPrimaryPath, ...nextSelectedPaths];
  }

  if (!nextSelectedPaths.length) {
    nextPrimaryPath = getDefaultFileBrowserSelection(displaySnapshot);
    nextSelectedPaths = nextPrimaryPath ? [nextPrimaryPath] : [];
  } else if (!nextPrimaryPath) {
    nextPrimaryPath = nextSelectedPaths[0] ?? null;
  }

  selectedFilePaths.value = nextSelectedPaths;
  selectedFilePath.value = nextPrimaryPath;
  selectionAnchorPath.value = selectionAnchorPath.value && entryPaths.has(selectionAnchorPath.value)
    ? selectionAnchorPath.value
    : nextPrimaryPath;
  loadThumbnailsForSnapshot(displaySnapshot);
}

export async function loadFileBrowserForDirectory(directoryPath = "", options: FileBrowserLoadOptions = {}) {
  if (!activeRepoId.value) return null;

  const includeTree = options.includeTree ?? false;
  const specialLocation = options.specialLocation ?? (activePanel.value === "deleted" ? "trash" : undefined);
  isLoadingFileBrowser.value = true;
  error.value = null;
  const progressId = startOperationProgress(
    specialLocation === "trash" ? "读取回收站" : includeTree ? "读取文件树" : "读取目录",
    directoryPath ? `正在读取 ${directoryPath}` : specialLocation === "trash" ? "正在读取回收站" : "正在读取根目录",
    { initial: 14, indeterminate: true },
  );
  try {
    const snapshot = await getFileBrowser({
      repoId: activeRepoId.value,
      directoryPath,
      includeTree,
      specialLocation,
    });
    updateOperationProgress(progressId, { detail: "整理目录条目", value: 92 });
    applyFileBrowserSnapshot(snapshot);
    finishOperationProgress(progressId);
    return snapshot;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    cancelOperationProgress(progressId);
    return null;
  } finally {
    isLoadingFileBrowser.value = false;
  }
}

export function joinActiveRepositoryPath(relativePath: string) {
  if (!activeSnapshot.value) return null;
  return joinRepositoryPath(activeSnapshot.value.repository.path, relativePath);
}
