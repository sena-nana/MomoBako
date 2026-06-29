import { getFileBrowser } from "../../services/repositoryApi";
import type {
  FileBrowserEntry,
  FileBrowserSnapshot,
  FileTreeNode,
  RepositorySnapshot,
} from "../../types/repository";
import {
  activePanel,
  activeRepoId,
  activeSnapshot,
  currentDirectoryPath,
  FILE_BROWSER_APPEND_PAGE_SIZE,
  FILE_BROWSER_INITIAL_PAGE_SIZE,
  createEmptyFileBrowserDerivedState,
  error,
  fileBrowser,
  fileBrowserDerived,
  fileTree,
  isLoadingFileBrowserMore,
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
import { loadThumbnailsForEntries, loadThumbnailsForSnapshot } from "./thumbnails";
import { joinRepositoryPath } from "./paths";
import { scheduleIdleTask, shouldYieldEvery, yieldEvery } from "./scheduler";

const NETEASE_SOURCE_PLUGIN_ID = "momobako.source.netease-cloud-music";

export type FileBrowserLoadOptions = {
  includeTree?: boolean;
  specialLocation?: "trash";
  append?: boolean;
  limit?: number;
  silent?: boolean;
};

export { entryNameFromPath } from "./paths";

export function getDefaultFileBrowserSelection(snapshot: FileBrowserSnapshot) {
  return snapshot.entries.find((entry) => entry.kind === "file")?.path
    ?? snapshot.entries[0]?.path
    ?? null;
}

function folderLabelFromPath(path: string, fallback: string) {
  const segments = path.split("/").filter(Boolean);
  return segments[segments.length - 1] ?? fallback;
}

function appendTreeNode(nodes: FileTreeNode[], path: string, label: string) {
  const segments = path.split("/").filter(Boolean);
  if (!segments.length) return;

  let cursor = "";
  let currentNodes = nodes;
  for (let index = 0; index < segments.length; index += 1) {
    const segment = segments[index];
    cursor = cursor ? `${cursor}/${segment}` : segment;
    let node = currentNodes.find((item) => item.path === cursor);
    if (!node) {
      node = {
        path: cursor,
        label: index === segments.length - 1 ? label : segment,
        children: [],
      };
      currentNodes.push(node);
    }
    currentNodes = node.children;
  }
}

function buildPresetFileTree(snapshot: RepositorySnapshot) {
  const nodes: FileTreeNode[] = [];
  for (const folder of snapshot.folders) {
    appendTreeNode(nodes, folder.path, folderLabelFromPath(folder.path, folder.label));
  }
  return nodes;
}

export function buildPresetRootFileBrowserSnapshot(snapshot: RepositorySnapshot): FileBrowserSnapshot {
  const tree = buildPresetFileTree(snapshot);
  return {
    repoId: snapshot.repository.repoId,
    rootPath: snapshot.repository.path,
    backendPluginId: snapshot.repository.backend.pluginId,
    backendKind: snapshot.repository.backend.kind,
    cacheState: "ready",
    indexedAt: null,
    currentPath: "",
    totalEntries: tree.length,
    loadedCount: tree.length,
    nextOffset: null,
    hasMore: false,
    tree,
    entries: tree.map((node) => ({
      path: node.path,
      name: node.label,
      kind: "directory",
      extension: null,
      sizeBytes: null,
      sizeLabel: null,
      modifiedAt: null,
      assetId: null,
      status: null,
      thumbnailPath: null,
      thumbnailCustom: false,
      hardlinkGroupId: null,
      hardlinkState: null,
      tags: [],
      aliasPaths: [],
      folderMetadata: null,
      metadata: {},
      isVirtual: false,
      providerId: null,
      providerItemId: null,
      sourcePayload: null,
      localAbsolutePath: null,
    })),
  };
}

let derivedRequestId = 0;
let latestDerivedPromise: Promise<void> = Promise.resolve();
let cancelNeteaseThumbnailPrefetch: (() => void) | null = null;
const queuedNeteaseThumbnailPrefetchKeys = new Set<string>();

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
  queueNeteaseThumbnailPrefetch(snapshot);
}

export function appendFileBrowserSnapshot(snapshot: FileBrowserSnapshot) {
  const current = fileBrowser.value;
  if (!current) {
    applyFileBrowserSnapshot(snapshot);
    return;
  }

  const requestId = ++derivedRequestId;
  const existingEntries = current.entries;
  const existingPathSet = new Set(existingEntries.map((entry) => `${entry.kind}:${entry.path}`));
  const appendedEntries = snapshot.entries.filter((entry) => !existingPathSet.has(`${entry.kind}:${entry.path}`));
  const mergedSnapshot: FileBrowserSnapshot = {
    ...snapshot,
    tree: snapshot.tree ?? current.tree,
    entries: [...existingEntries, ...appendedEntries],
  };
  fileBrowser.value = mergedSnapshot;
  if (mergedSnapshot.tree) {
    fileTree.value = mergedSnapshot.tree;
  }
  currentDirectoryPath.value = mergedSnapshot.currentPath;
  latestDerivedPromise = buildFileBrowserDerivedState(mergedSnapshot, requestId);
  void latestDerivedPromise;
  if (appendedEntries.length) {
    loadThumbnailsForEntries(
      mergedSnapshot.repoId,
      mergedSnapshot.currentPath,
      appendedEntries,
    );
  }
  queueNeteaseThumbnailPrefetch(mergedSnapshot);
}

function queueNeteaseThumbnailPrefetch(snapshot: FileBrowserSnapshot) {
  cancelNeteaseThumbnailPrefetch?.();
  cancelNeteaseThumbnailPrefetch = null;
  if (
    snapshot.backendPluginId !== NETEASE_SOURCE_PLUGIN_ID
    || !snapshot.hasMore
    || snapshot.nextOffset == null
  ) {
    return;
  }
  const key = `${snapshot.repoId}:${snapshot.currentPath}:${snapshot.nextOffset}`;
  if (queuedNeteaseThumbnailPrefetchKeys.has(key)) return;
  cancelNeteaseThumbnailPrefetch = scheduleIdleTask(() => {
    queuedNeteaseThumbnailPrefetchKeys.add(key);
    void prefetchNeteaseThumbnailPage(snapshot, key);
  }, 420);
}

async function prefetchNeteaseThumbnailPage(snapshot: FileBrowserSnapshot, key: string) {
  try {
    const preload = await getFileBrowser({
      repoId: snapshot.repoId,
      directoryPath: snapshot.currentPath,
      includeTree: false,
      specialLocation: snapshot.specialLocation ?? undefined,
      offset: snapshot.nextOffset ?? snapshot.loadedCount,
      limit: FILE_BROWSER_APPEND_PAGE_SIZE,
    });
    if (
      fileBrowser.value?.repoId !== snapshot.repoId
      || fileBrowser.value?.currentPath !== snapshot.currentPath
      || fileBrowser.value?.specialLocation !== (snapshot.specialLocation ?? null)
    ) {
      return;
    }
    loadThumbnailsForEntries(preload.repoId, preload.currentPath, preload.entries);
  } finally {
    if (cancelNeteaseThumbnailPrefetch) {
      cancelNeteaseThumbnailPrefetch = null;
    }
    queuedNeteaseThumbnailPrefetchKeys.delete(key);
  }
}

export function waitForFileBrowserDerivedState() {
  return latestDerivedPromise;
}

export async function loadFileBrowserForDirectory(directoryPath = "", options: FileBrowserLoadOptions = {}) {
  if (!activeRepoId.value) return null;

  const append = options.append ?? false;
  const includeTree = options.includeTree ?? false;
  const silent = options.silent ?? false;
  const specialLocation = options.specialLocation ?? (activePanel.value === "deleted" ? "trash" : undefined);
  const currentSnapshot = fileBrowser.value;
  const offset = append
    ? currentSnapshot?.currentPath === directoryPath && currentSnapshot.specialLocation === (specialLocation ?? null)
      ? currentSnapshot.nextOffset ?? currentSnapshot.entries.length
      : 0
    : 0;
  const limit = options.limit ?? (append ? FILE_BROWSER_APPEND_PAGE_SIZE : FILE_BROWSER_INITIAL_PAGE_SIZE);

  if (append) {
    if (!currentSnapshot?.hasMore) return currentSnapshot;
    isLoadingFileBrowserMore.value = true;
  } else {
    isLoadingFileBrowser.value = true;
    if (!silent) {
      error.value = null;
    }
  }
  const progressId = append || silent ? null : startOperationProgress(
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
      offset,
      limit,
    });
    if (append) {
      appendFileBrowserSnapshot(snapshot);
    } else {
      if (progressId != null) {
        updateOperationProgress(progressId, { detail: "整理目录条目", value: 92 });
      }
      applyFileBrowserSnapshot(snapshot);
      if (progressId != null) {
        finishOperationProgress(progressId);
      }
    }
    return snapshot;
  } catch (cause) {
    if (!silent) {
      error.value = cause instanceof Error ? cause.message : String(cause);
    }
    if (progressId != null) {
      cancelOperationProgress(progressId);
    }
    return null;
  } finally {
    if (append) {
      isLoadingFileBrowserMore.value = false;
    } else {
      isLoadingFileBrowser.value = false;
    }
  }
}

export function joinActiveRepositoryPath(relativePath: string) {
  if (!activeSnapshot.value) return null;
  return joinRepositoryPath(activeSnapshot.value.repository.path, relativePath);
}
