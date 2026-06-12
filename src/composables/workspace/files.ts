import { getFileBrowser } from "../../services/repositoryApi";
import type { FileBrowserEntry, FileBrowserSnapshot } from "../../types/repository";
import {
  activePanel,
  activeRepoId,
  activeSnapshot,
  currentDirectoryPath,
  createEmptyFileBrowserDerivedState,
  error,
  fileBrowser,
  fileBrowserDerived,
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
import { shouldYieldEvery, yieldEvery } from "./scheduler";

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

let derivedRequestId = 0;
let latestDerivedPromise: Promise<void> = Promise.resolve();

async function buildFileBrowserDerivedState(snapshot: FileBrowserSnapshot, requestId: number) {
  const entryMap = new Map<string, FileBrowserEntry>();
  const directories: FileBrowserEntry[] = [];
  const files: FileBrowserEntry[] = [];

  for (let index = 0; index < snapshot.entries.length; index += 1) {
    const entry = snapshot.entries[index];
    entryMap.set(entry.path, entry);
    if (entry.kind === "directory") {
      directories.push(entry);
    } else {
      files.push(entry);
    }
    if (shouldYieldEvery(index)) {
      await yieldEvery(index);
    }
    if (requestId !== derivedRequestId) return;
  }

  if (requestId !== derivedRequestId) return;
  fileBrowserDerived.value = {
    entryMap,
    directories,
    files,
    visibleEntries: [...directories, ...files],
  };
  applySelectionForEntryMap(snapshot, entryMap);
}

function applySelectionForEntryMap(snapshot: FileBrowserSnapshot, entryMap: ReadonlyMap<string, FileBrowserEntry>) {
  let nextSelectedPaths = selectedFilePaths.value.filter((path) => entryMap.has(path));
  let nextPrimaryPath = selectedFilePath.value && entryMap.has(selectedFilePath.value)
    ? selectedFilePath.value
    : null;

  if (nextPrimaryPath && !nextSelectedPaths.includes(nextPrimaryPath)) {
    nextSelectedPaths = [nextPrimaryPath, ...nextSelectedPaths];
  }

  if (!nextSelectedPaths.length) {
    nextPrimaryPath = getDefaultFileBrowserSelection(snapshot);
    nextSelectedPaths = nextPrimaryPath ? [nextPrimaryPath] : [];
  } else if (!nextPrimaryPath) {
    nextPrimaryPath = nextSelectedPaths[0] ?? null;
  }

  selectedFilePaths.value = nextSelectedPaths;
  selectedFilePath.value = nextPrimaryPath;
  selectionAnchorPath.value = selectionAnchorPath.value && entryMap.has(selectionAnchorPath.value)
    ? selectionAnchorPath.value
    : nextPrimaryPath;
}

export function applyFileBrowserSnapshot(snapshot: FileBrowserSnapshot) {
  const requestId = ++derivedRequestId;
  fileBrowser.value = snapshot;
  fileBrowserDerived.value = createEmptyFileBrowserDerivedState();
  if (snapshot.tree) {
    fileTree.value = snapshot.tree;
  }
  currentDirectoryPath.value = snapshot.currentPath;
  const defaultSelection = getDefaultFileBrowserSelection(snapshot);
  if (!selectedFilePath.value) {
    selectedFilePath.value = defaultSelection;
    selectedFilePaths.value = defaultSelection ? [defaultSelection] : [];
    selectionAnchorPath.value = defaultSelection;
  }
  latestDerivedPromise = buildFileBrowserDerivedState(snapshot, requestId);
  void latestDerivedPromise;
  loadThumbnailsForSnapshot(snapshot);
}

export function waitForFileBrowserDerivedState() {
  return latestDerivedPromise;
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
