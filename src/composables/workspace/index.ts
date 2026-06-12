import { computed } from "vue";
import {
  attachRepositoryFolder,
  confirmHardlinkCandidate,
  deletePlugin,
  copyEntries,
  createDirectory,
  createFile,
  createPlaylist,
  createRepository,
  deleteEntry,
  deletePlaylist,
  deleteRepository,
  exportRepository,
  getApiDesignSnapshot,
  getAssetDetail,
  getCacheSnapshot,
  getPlaylistDetail,
  importEntries,
  getRepositorySnapshot,
  importRepository,
  installPluginFromArchive,
  listPlaylists,
  listRepositoryActions,
  listSmartFolders,
  listPlugins,
  addPlaylistItems,
  reorderPlaylistItems,
  removePlaylistItem,
  createSmartFolder,
  deleteSmartFolder,
  querySmartFolder,
  moveEntries,
  setPlaylistMembership,
  setPluginEnabled,
  startExternalFileDrag,
  mutateTrash,
  openRepositoryPath,
  redoLastRevision,
  relocateRepository,
  renameEntry,
  revealRepositoryPath,
  runRepositoryAction,
  syncRepository,
  undoLastRevision,
  updateSmartFolder,
  updateAssetMetadata,
} from "../../services/repositoryApi";
import { syncRegisteredPreviewPluginManifests } from "../../plugins/sdk";
import type {
  AssetDetail,
  FileBrowserSnapshot,
  FileDeleteMode,
  HardlinkConfirmResponse,
  PlaylistDetail,
  PlaylistMembershipSnapshot,
  PlaylistMutationRequest,
  PlaylistSummary,
  RepositoryExportRequest,
  RepositoryExportResponse,
  SmartFolderFilter,
  SmartFolderMutationRequest,
  SmartFolderUpdateRequest,
} from "../../types/repository";
import {
  activeAssetDetail,
  activeAssetId,
  activePanel,
  activePreviewPath,
  activePlaylistDetail,
  activePlaylistId,
  activeRepoId,
  activeRepositoryActionId,
  activeSmartFolderId,
  activeSnapshot,
  apiDesign,
  cacheSnapshot,
  currentDirectoryPath,
  dragHoverFolderPath,
  error,
  fileBrowser,
  fileTree,
  filters,
  hardlinkCandidates,
  isFilterBarOpen,
  isLoadingAssetDetail,
  isLoadingFileBrowser,
  isLoadingRepositories,
  isLoadingRepositoryActions,
  isLoadingSnapshot,
  isLoadingSettingsData,
  isLoadingSmartFolder,
  isManagingPlugins,
  isExternalDragActive,
  isInternalDragActive,
  isMutatingFiles,
  isMutatingSmartFolder,
  isRunningRepositoryAction,
  isSavingMetadata,
  isSearching,
  isSyncing,
  lastSyncResult,
  plugins,
  playlists,
  playlistMemberships,
  repositories,
  repositoryActions,
  searchQuery,
  searchResults,
  draggedWorkspacePaths,
  selectedFilePaths,
  selectionAnchorPath,
  selectedFilePath,
  smartFolderResult,
  smartFolders,
  workspaceStartup,
  type WorkspaceFilterState,
  type WorkspacePanelKey,
} from "./state";
import {
  activeFilterCount,
  activeRepository,
  breadcrumbSegments,
  directoryEntries,
  fileBrowserEntryMap,
  fileEntries,
  hasActiveFilters,
  hasMultipleSelection,
  hasSplitFileGroups,
  libraryOverview,
  repositoryBackendOptions,
  selectedEntries,
  selectedEntry,
  selectedFilePathSet,
  visibleEntries,
} from "./selectors";
import {
  clearFilters,
  resetSearchState,
  runFilteredSearch,
  runSearch,
  setFilterBarOpen,
  setMinimumRatingFilter,
  toggleFilterBar,
  toggleFilterValue,
  updateFilters,
} from "./search";
import {
  refreshHardlinkCandidates,
  refreshWorkspaceAfterMutation as refreshWorkspaceWithDirectory,
  type WorkspaceRefreshPlan,
} from "./refresh";
import {
  cancelOperationProgress,
  finishOperationProgress,
  operationProgress,
  setSyncProgress,
  startOperationProgress,
  syncProgress,
  updateOperationProgress,
  type WorkspaceOperationProgress,
} from "./tasks";
import {
  clearWorkspaceEntryThumbnail,
  refreshWorkspaceEntryThumbnail,
  saveGeneratedWorkspaceEntryThumbnail,
  setWorkspaceEntryThumbnail,
  setWorkspaceEntryThumbnailFromBytes,
} from "./thumbnails";
import {
  applyFileBrowserSnapshot,
  entryNameFromPath,
  getDefaultFileBrowserSelection,
  joinActiveRepositoryPath,
  loadFileBrowserForDirectory,
} from "./files";
import {
  ensureRepositoryWorkspace as ensureRepositoryWorkspaceLifecycle,
  loadRepositories as loadRepositoriesLifecycle,
  resetActiveRepositoryContent,
} from "./lifecycle";

export type { WorkspaceFilterState, WorkspaceOperationProgress, WorkspacePanelKey };
export { resetRepositoryWorkspaceForTests } from "./lifecycle";

async function loadRepositories() {
  return loadRepositoriesLifecycle(selectRepository);
}

function normalizeSelectionPaths(paths: string[]) {
  return Array.from(new Set(
    paths
      .map((path) => path.trim())
      .filter(Boolean),
  ));
}

function applyWorkspaceSelection(
  paths: string[],
  primaryPath: string | null = paths[0] ?? null,
  anchorPath: string | null = primaryPath,
) {
  const nextPaths = normalizeSelectionPaths(paths);
  const nextPrimaryPath = primaryPath && nextPaths.includes(primaryPath)
    ? primaryPath
    : nextPaths[0] ?? null;

  selectedFilePaths.value = nextPaths;
  selectedFilePath.value = nextPrimaryPath;
  selectionAnchorPath.value = nextPaths.length
    ? anchorPath && nextPaths.includes(anchorPath) ? anchorPath : nextPrimaryPath
    : null;
}

function rebuildPlaylistMemberships(
  details: PlaylistDetail[],
) {
  const nextMemberships: Record<string, string[]> = {};

  for (const detail of details) {
    for (const item of detail.items) {
      if (!nextMemberships[item.assetId]) {
        nextMemberships[item.assetId] = [];
      }
      nextMemberships[item.assetId].push(detail.playlist.playlistId);
    }
  }

  playlistMemberships.value = nextMemberships;
}

async function syncPlaylistMemberships(
  repoId: string,
  playlistItems: PlaylistSummary[] = playlists.value,
) {
  if (!repoId || !playlistItems.length) {
    playlistMemberships.value = {};
    if (activePlaylistId.value && !playlistItems.some((item) => item.playlistId === activePlaylistId.value)) {
      activePlaylistId.value = null;
      activePlaylistDetail.value = null;
    }
    return [];
  }

  const details = (await Promise.all(
    playlistItems.map(async (playlist) => {
      try {
        return await getPlaylistDetail(repoId, playlist.playlistId);
      } catch {
        return null;
      }
    }),
  )).filter((detail): detail is PlaylistDetail => Boolean(detail));

  rebuildPlaylistMemberships(details);

  if (activePlaylistId.value) {
    activePlaylistDetail.value = details.find((detail) => detail.playlist.playlistId === activePlaylistId.value) ?? null;
  }

  return details;
}

function defaultDirectoryRefreshPlan(paths: string[]): WorkspaceRefreshPlan["directory"] {
  if (activePanel.value === "deleted") return "trash";
  const selectedPaths = new Set(paths);
  const includesDirectory = visibleEntries.value.some((entry) => (
    entry.kind === "directory" && selectedPaths.has(entry.path)
  ));
  return includesDirectory ? "currentWithTree" : "current";
}

function resolveBatchMutationPrimaryPath(excludedPaths: string[]) {
  const excluded = new Set(excludedPaths);
  return visibleEntries.value.find((entry) => !excluded.has(entry.path))?.path ?? null;
}

async function finishFileTransfer(
  repoId: string,
  snapshot: FileBrowserSnapshot,
  sourcePaths: string[],
) {
  applyFileBrowserSnapshot(snapshot);
  const sourceNames = new Set(sourcePaths.map(entryNameFromPath));
  const nextSelection = snapshot.entries
    .filter((entry) => sourceNames.has(entry.name))
    .map((entry) => entry.path);
  if (nextSelection.length) {
    applyWorkspaceSelection(nextSelection, nextSelection[0], nextSelection[0]);
  }
  await refreshWorkspaceAfterMutation(repoId, { hardlinkCandidates: true, repositorySnapshot: true });
}

export async function selectRepository(repoId: string) {
  if (!repoId) return;

  const isSwitchingRepository = activeRepoId.value !== repoId;
  isLoadingSnapshot.value = true;
  error.value = null;
  const progressId = startOperationProgress("加载资源库", "读取资源库快照", { initial: 10, indeterminate: true });

  try {
    const repository = repositories.value.find((item) => item.repoId === repoId);
    if (repository?.status === "missing") {
      activeRepoId.value = repoId;
      resetActiveRepositoryContent();
      if (isSwitchingRepository) {
        resetSearchState();
      }
      finishOperationProgress(progressId);
      return;
    }

    const snapshot = await getRepositorySnapshot(repoId);
    updateOperationProgress(progressId, { detail: "加载资源索引", value: 46 });
    activeRepoId.value = repoId;
    activeSnapshot.value = snapshot;
    playlists.value = snapshot.playlists ?? await listPlaylists(repoId);
    await syncPlaylistMemberships(repoId, playlists.value);
    smartFolders.value = await listSmartFolders(repoId);
    repositoryActions.value = await listRepositoryActions(repoId);
    if (isSwitchingRepository) {
      resetSearchState();
      activePlaylistId.value = null;
      activePlaylistDetail.value = null;
      activePreviewPath.value = null;
      activeSmartFolderId.value = null;
      smartFolderResult.value = null;
      activeRepositoryActionId.value = repositoryActions.value[0]?.actionId ?? null;
    }

    const defaultAssetId = activeAssetId.value && snapshot.assets.some((item) => item.assetId === activeAssetId.value)
      ? activeAssetId.value
      : snapshot.assets[0]?.assetId ?? null;

    activeAssetId.value = defaultAssetId;
    activeAssetDetail.value = null;

    currentDirectoryPath.value = "";
    await loadFileBrowserForDirectory("", { includeTree: true });
    if (defaultAssetId) {
      void selectAsset(defaultAssetId);
    }
    void refreshHardlinkCandidates(repoId);
    finishOperationProgress(progressId);
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    cancelOperationProgress(progressId);
  } finally {
    isLoadingSnapshot.value = false;
  }
}

export async function selectAsset(assetId: string) {
  if (!assetId || !activeRepoId.value) return;

  isLoadingAssetDetail.value = true;
  error.value = null;

  try {
    activeAssetId.value = assetId;
    activeAssetDetail.value = await getAssetDetail(activeRepoId.value, assetId);
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
  } finally {
    isLoadingAssetDetail.value = false;
  }
}

export async function createDirectoryInWorkspace(name: string, parentPath = currentDirectoryPath.value) {
  if (!activeRepoId.value) return null;
  isMutatingFiles.value = true;
  error.value = null;
  try {
    const snapshot = await createDirectory({
      repoId: activeRepoId.value,
      parentPath,
      name,
    });
    applyFileBrowserSnapshot(snapshot);
    await refreshWorkspaceAfterMutation(activeRepoId.value, { repositorySnapshot: true });
    return snapshot;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    return null;
  } finally {
    isMutatingFiles.value = false;
  }
}

export async function createFileInWorkspace(name: string, parentPath = currentDirectoryPath.value) {
  if (!activeRepoId.value) return null;
  isMutatingFiles.value = true;
  error.value = null;
  try {
    const snapshot = await createFile({
      repoId: activeRepoId.value,
      parentPath,
      name,
    });
    applyFileBrowserSnapshot(snapshot);
    const createdPath = snapshot.entries.find((entry) => entry.name === name)?.path ?? null;
    if (createdPath) {
      applyWorkspaceSelection([createdPath], createdPath, createdPath);
    }
    await refreshWorkspaceAfterMutation(activeRepoId.value, { repositorySnapshot: true });
    return snapshot;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    return null;
  } finally {
    isMutatingFiles.value = false;
  }
}

export async function importEntriesToWorkspace(sourcePaths: string[], parentPath = currentDirectoryPath.value) {
  const repoId = activeRepoId.value;
  if (!repoId || !sourcePaths.length) return null;
  error.value = null;
  const progressId = startOperationProgress(
    "导入文件",
    `准备导入 ${sourcePaths.length} 个条目`,
    { initial: 8 },
  );
  try {
    updateOperationProgress(progressId, { detail: "导入文件到当前资源库", value: 24 });
    const snapshot = await importEntries({
      repoId,
      parentPath,
      sourcePaths,
    });
    updateOperationProgress(progressId, { detail: "刷新文件索引", value: 84 });
    await finishFileTransfer(repoId, snapshot, sourcePaths);
    finishOperationProgress(progressId);
    return snapshot;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    cancelOperationProgress(progressId);
    return null;
  }
}

export async function copyWorkspaceEntries(sourcePaths: string[], parentPath = currentDirectoryPath.value) {
  const repoId = activeRepoId.value;
  if (!repoId || !sourcePaths.length) return null;
  isMutatingFiles.value = true;
  error.value = null;
  const progressId = startOperationProgress("复制文件", `准备复制 ${sourcePaths.length} 个条目`, { initial: 8 });
  try {
    updateOperationProgress(progressId, { detail: "创建硬链接或复制文件", value: 32 });
    const snapshot = await copyEntries({
      repoId,
      sourcePaths,
      parentPath,
      mode: "hardlinkPreferred",
    });
    updateOperationProgress(progressId, { detail: "刷新文件索引", value: 84 });
    await finishFileTransfer(repoId, snapshot, sourcePaths);
    finishOperationProgress(progressId);
    return snapshot;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    cancelOperationProgress(progressId);
    return null;
  } finally {
    isMutatingFiles.value = false;
  }
}

export async function moveWorkspaceEntries(sourcePaths: string[], parentPath: string) {
  const repoId = activeRepoId.value;
  const nextPaths = normalizeSelectionPaths(sourcePaths);
  if (!repoId || !nextPaths.length) return null;

  isMutatingFiles.value = true;
  error.value = null;
  const progressId = startOperationProgress("移动文件", `准备移动 ${nextPaths.length} 个条目`, { initial: 8 });
  try {
    updateOperationProgress(progressId, { detail: "移动到目标文件夹", value: 36 });
    const snapshot = await moveEntries({
      repoId,
      sourcePaths: nextPaths,
      parentPath,
    });
    updateOperationProgress(progressId, { detail: "刷新文件索引", value: 82 });

    if (currentDirectoryPath.value === parentPath) {
      applyFileBrowserSnapshot(snapshot);
      const movedNames = new Set(nextPaths.map(entryNameFromPath));
      const nextSelection = snapshot.entries
        .filter((entry) => movedNames.has(entry.name))
        .map((entry) => entry.path);
      if (nextSelection.length) {
        applyWorkspaceSelection(nextSelection, nextSelection[0], nextSelection[0]);
      }
      await refreshWorkspaceAfterMutation(repoId, { repositorySnapshot: true });
    } else {
      const nextPrimaryPath = resolveBatchMutationPrimaryPath(nextPaths);
      applyWorkspaceSelection(nextPrimaryPath ? [nextPrimaryPath] : [], nextPrimaryPath, nextPrimaryPath);
      await refreshWorkspaceAfterMutation(repoId, {
        directory: defaultDirectoryRefreshPlan(nextPaths),
        repositorySnapshot: true,
      });
    }

    finishOperationProgress(progressId);
    return snapshot;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    cancelOperationProgress(progressId);
    return null;
  } finally {
    isMutatingFiles.value = false;
  }
}

export async function renameWorkspaceEntry(path: string, newName: string) {
  if (!activeRepoId.value) return null;
  isMutatingFiles.value = true;
  error.value = null;
  try {
    const snapshot = await renameEntry({
      repoId: activeRepoId.value,
      path,
      newName,
    });
    applyFileBrowserSnapshot(snapshot);
    const renamedPath = snapshot.entries.find((entry) => entry.name === newName)?.path ?? null;
    if (renamedPath) {
      applyWorkspaceSelection([renamedPath], renamedPath, renamedPath);
    }
    await refreshWorkspaceAfterMutation(activeRepoId.value, { repositorySnapshot: true });
    return snapshot;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    return null;
  } finally {
    isMutatingFiles.value = false;
  }
}

export async function deleteWorkspaceEntry(path: string, mode?: FileDeleteMode) {
  if (!activeRepoId.value) return null;
  isMutatingFiles.value = true;
  error.value = null;
  try {
    const deleteMode = mode ?? (activePanel.value === "deleted" ? "permanentDelete" : undefined);
    const snapshot = await deleteEntry({
      repoId: activeRepoId.value,
      path,
      mode: deleteMode,
    });
    const shouldSelectDefault = selectedFilePath.value === path;
    applyFileBrowserSnapshot(snapshot);
    if (shouldSelectDefault) {
      selectedFilePath.value = getDefaultFileBrowserSelection(snapshot);
    }
    await refreshWorkspaceAfterMutation(activeRepoId.value, { repositorySnapshot: true });
    return snapshot;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    return null;
  } finally {
    isMutatingFiles.value = false;
  }
}

export async function deleteWorkspaceEntries(paths: string[], mode?: FileDeleteMode) {
  const repoId = activeRepoId.value;
  const nextPaths = normalizeSelectionPaths(paths);
  if (!repoId || !nextPaths.length) return null;

  isMutatingFiles.value = true;
  error.value = null;
  const progressId = startOperationProgress("删除文件", `准备处理 ${nextPaths.length} 个条目`, { initial: 10 });
  try {
    const deleteMode = mode ?? (activePanel.value === "deleted" ? "permanentDelete" : undefined);
    const nextPrimaryPath = resolveBatchMutationPrimaryPath(nextPaths);
    for (const [index, path] of nextPaths.entries()) {
      updateOperationProgress(progressId, {
        detail: `正在处理 ${entryNameFromPath(path)}`,
        value: Math.round(((index + 1) / nextPaths.length) * 72),
      });
      await deleteEntry({
        repoId,
        path,
        mode: deleteMode,
      });
    }
    applyWorkspaceSelection(nextPrimaryPath ? [nextPrimaryPath] : [], nextPrimaryPath, nextPrimaryPath);
    await refreshWorkspaceAfterMutation(repoId, {
      directory: defaultDirectoryRefreshPlan(nextPaths),
      repositorySnapshot: true,
    });
    finishOperationProgress(progressId);
    return fileBrowser.value;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    cancelOperationProgress(progressId);
    return null;
  } finally {
    isMutatingFiles.value = false;
  }
}

export async function restoreTrashEntry(path: string) {
  if (!activeRepoId.value) return null;
  isMutatingFiles.value = true;
  error.value = null;
  try {
    const snapshot = await mutateTrash({
      repoId: activeRepoId.value,
      action: "restore",
      path,
    });
    const shouldSelectDefault = selectedFilePath.value === path;
    applyFileBrowserSnapshot(snapshot);
    if (shouldSelectDefault) {
      selectedFilePath.value = getDefaultFileBrowserSelection(snapshot);
    }
    await refreshWorkspaceAfterMutation(activeRepoId.value, {
      repositorySnapshot: true,
      repositorySummary: true,
    });
    return snapshot;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    return null;
  } finally {
    isMutatingFiles.value = false;
  }
}

export async function restoreTrashEntries(paths: string[]) {
  const repoId = activeRepoId.value;
  const nextPaths = normalizeSelectionPaths(paths);
  if (!repoId || !nextPaths.length) return null;

  isMutatingFiles.value = true;
  error.value = null;
  const progressId = startOperationProgress("还原文件", `准备还原 ${nextPaths.length} 个条目`, { initial: 10 });
  try {
    for (const [index, path] of nextPaths.entries()) {
      updateOperationProgress(progressId, {
        detail: `正在还原 ${entryNameFromPath(path)}`,
        value: Math.round(((index + 1) / nextPaths.length) * 72),
      });
      await mutateTrash({
        repoId,
        action: "restore",
        path,
      });
    }
    clearWorkspaceSelection();
    await refreshWorkspaceAfterMutation(repoId, {
      directory: "trash",
      repositorySnapshot: true,
      repositorySummary: true,
    });
    finishOperationProgress(progressId);
    return fileBrowser.value;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    cancelOperationProgress(progressId);
    return null;
  } finally {
    isMutatingFiles.value = false;
  }
}

export async function restoreAllTrashEntries() {
  if (!activeRepoId.value) return null;
  isMutatingFiles.value = true;
  error.value = null;
  try {
    const snapshot = await mutateTrash({
      repoId: activeRepoId.value,
      action: "restoreAll",
    });
    applyFileBrowserSnapshot(snapshot);
    selectedFilePath.value = getDefaultFileBrowserSelection(snapshot);
    await refreshWorkspaceAfterMutation(activeRepoId.value, {
      repositorySnapshot: true,
      repositorySummary: true,
    });
    return snapshot;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    return null;
  } finally {
    isMutatingFiles.value = false;
  }
}

export async function emptyTrash() {
  if (!activeRepoId.value) return null;
  isMutatingFiles.value = true;
  error.value = null;
  try {
    const snapshot = await mutateTrash({
      repoId: activeRepoId.value,
      action: "empty",
    });
    applyFileBrowserSnapshot(snapshot);
    selectedFilePath.value = getDefaultFileBrowserSelection(snapshot);
    await refreshWorkspaceAfterMutation(activeRepoId.value, {
      repositorySnapshot: true,
      repositorySummary: true,
    });
    return snapshot;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    return null;
  } finally {
    isMutatingFiles.value = false;
  }
}

export async function openWorkspaceEntry(path: string) {
  const absolutePath = joinActiveRepositoryPath(path);
  if (!absolutePath) return;
  error.value = null;
  try {
    await openRepositoryPath(absolutePath);
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
  }
}

export async function revealWorkspaceEntry(path: string) {
  const absolutePath = joinActiveRepositoryPath(path);
  if (!absolutePath) return;
  error.value = null;
  try {
    await revealRepositoryPath(absolutePath);
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
  }
}

export async function startWorkspaceEntryDrag(path: string, icon?: string) {
  return startWorkspaceEntriesDrag([path], icon);
}

export async function startWorkspaceEntriesDrag(paths: string[], icon?: string) {
  if (fileBrowser.value?.specialLocation === "trash") return false;
  if (activeSnapshot.value?.repository.backend.kind !== "filesystem") return false;
  const absolutePaths = normalizeSelectionPaths(paths)
    .map((path) => joinActiveRepositoryPath(path))
    .filter((path): path is string => Boolean(path));
  if (!absolutePaths.length) return false;

  error.value = null;
  try {
    await startExternalFileDrag(absolutePaths, icon);
    return true;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    return false;
  }
}

export function clearWorkspaceSelection() {
  applyWorkspaceSelection([]);
}

export function selectWorkspaceEntries(
  paths: string[],
  options: {
    primaryPath?: string | null;
    anchorPath?: string | null;
  } = {},
) {
  const nextPaths = normalizeSelectionPaths(paths);
  applyWorkspaceSelection(nextPaths, options.primaryPath ?? nextPaths[0] ?? null, options.anchorPath ?? options.primaryPath ?? nextPaths[0] ?? null);
}

export function selectWorkspaceEntry(
  path: string,
  options: {
    mode?: "replace" | "toggle" | "range";
  } = {},
) {
  const mode = options.mode ?? "replace";

  if (mode === "toggle") {
    const nextSelection = new Set(selectedFilePaths.value);
    if (nextSelection.has(path)) {
      nextSelection.delete(path);
      const nextPaths = Array.from(nextSelection);
      const nextPrimaryPath = selectedFilePath.value === path
        ? nextPaths[0] ?? null
        : selectedFilePath.value;
      applyWorkspaceSelection(nextPaths, nextPrimaryPath, selectionAnchorPath.value === path ? nextPrimaryPath : selectionAnchorPath.value);
      return;
    }
    const nextPaths = [...selectedFilePaths.value, path];
    applyWorkspaceSelection(nextPaths, path, path);
    return;
  }

  if (mode === "range") {
    const orderedPaths = visibleEntries.value.map((entry) => entry.path);
    const anchorPath = selectionAnchorPath.value ?? selectedFilePath.value ?? path;
    const anchorIndex = orderedPaths.indexOf(anchorPath);
    const targetIndex = orderedPaths.indexOf(path);
    if (anchorIndex >= 0 && targetIndex >= 0) {
      const [start, end] = anchorIndex <= targetIndex ? [anchorIndex, targetIndex] : [targetIndex, anchorIndex];
      const nextPaths = orderedPaths.slice(start, end + 1);
      applyWorkspaceSelection(nextPaths, path, anchorPath);
      return;
    }
  }

  applyWorkspaceSelection([path], path, path);
}

export function setExternalDragActive(value: boolean) {
  isExternalDragActive.value = value;
}

export function setInternalDragActive(value: boolean) {
  isInternalDragActive.value = value;
}

export function setDraggedWorkspacePaths(paths: string[]) {
  draggedWorkspacePaths.value = normalizeSelectionPaths(paths);
}

export function clearDraggedWorkspaceState() {
  isInternalDragActive.value = false;
  draggedWorkspacePaths.value = [];
}

export function setDragHoverFolderPath(path: string | null) {
  dragHoverFolderPath.value = path;
}

function smartFolderTreeContains(items: typeof smartFolders.value, smartFolderId: string | null): boolean {
  if (!smartFolderId) return false;
  return items.some((item) => (
    item.smartFolderId === smartFolderId || smartFolderTreeContains(item.children, smartFolderId)
  ));
}

function normalizeSmartFolderFilter(filter: SmartFolderFilter): SmartFolderFilter {
  const normalizeList = (items?: string[]) => {
    const values = Array.from(new Set((items ?? []).map((item) => item.trim()).filter(Boolean)));
    return values.length ? values : undefined;
  };
  const normalizeMetadataFilters = (items = filter.metadataFilters) => items
    ?.map((item) => ({ key: item.key.trim(), value: item.value.trim() }))
    .filter((item) => item.key && item.value);
  const numberFilters = filter.numberFilters
    ?.map((item) => ({ key: item.key.trim(), min: item.min, max: item.max }))
    .filter((item) => item.key && (item.min != null || item.max != null));
  const excludeNumberFilters = filter.excludeNumberFilters
    ?.map((item) => ({ key: item.key.trim(), min: item.min, max: item.max }))
    .filter((item) => item.key && (item.min != null || item.max != null));
  const dateFilters = filter.dateFilters
    ?.map((item) => ({ key: item.key.trim(), from: item.from?.trim() || undefined, to: item.to?.trim() || undefined }))
    .filter((item) => item.key && (item.from || item.to));
  const excludeDateFilters = filter.excludeDateFilters
    ?.map((item) => ({ key: item.key.trim(), from: item.from?.trim() || undefined, to: item.to?.trim() || undefined }))
    .filter((item) => item.key && (item.from || item.to));
  const sortField = filter.sort?.field.trim();
  return {
    query: filter.query?.trim() || undefined,
    pathPrefix: filter.pathPrefix?.trim() || undefined,
    excludeQuery: filter.excludeQuery?.trim() || undefined,
    excludePathPrefixes: normalizeList(filter.excludePathPrefixes),
    tags: normalizeList(filter.tags),
    formats: normalizeList(filter.formats),
    colors: normalizeList(filter.colors),
    shapes: normalizeList(filter.shapes),
    metadataFilters: normalizeMetadataFilters()?.length ? normalizeMetadataFilters() : undefined,
    excludeTags: normalizeList(filter.excludeTags),
    excludeFormats: normalizeList(filter.excludeFormats),
    excludeMetadataFilters: normalizeMetadataFilters(filter.excludeMetadataFilters)?.length
      ? normalizeMetadataFilters(filter.excludeMetadataFilters)
      : undefined,
    excludeNumberFilters: excludeNumberFilters?.length ? excludeNumberFilters : undefined,
    excludeDateFilters: excludeDateFilters?.length ? excludeDateFilters : undefined,
    numberFilters: numberFilters?.length ? numberFilters : undefined,
    dateFilters: dateFilters?.length ? dateFilters : undefined,
    minRating: filter.minRating && filter.minRating > 0 ? filter.minRating : undefined,
    matchMode: filter.matchMode === "or" ? "or" : undefined,
    sort: sortField ? { field: sortField, direction: filter.sort?.direction === "desc" ? "desc" : "asc" } : undefined,
    limit: filter.limit && filter.limit > 0 ? filter.limit : undefined,
  };
}

export async function refreshSmartFolders(repoId = activeRepoId.value) {
  if (!repoId) {
    smartFolders.value = [];
    return [];
  }
  const items = await listSmartFolders(repoId);
  if (activeRepoId.value === repoId) {
    smartFolders.value = items;
  }
  return items;
}

export async function selectSmartFolder(smartFolderId: string) {
  const repoId = activeRepoId.value;
  if (!repoId || !smartFolderId) return null;
  activePanel.value = "smartFolder";
  activeSmartFolderId.value = smartFolderId;
  isLoadingSmartFolder.value = true;
  error.value = null;
  try {
    const snapshot = await querySmartFolder(repoId, smartFolderId);
    if (activeRepoId.value !== repoId || activeSmartFolderId.value !== smartFolderId) {
      return snapshot;
    }
    smartFolderResult.value = snapshot;
    selectedFilePath.value = snapshot.results[0]?.path ?? null;
    return snapshot;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    return null;
  } finally {
    isLoadingSmartFolder.value = false;
  }
}

export async function createSmartFolderInWorkspace(request: Omit<SmartFolderMutationRequest, "repoId">) {
  const repoId = activeRepoId.value;
  if (!repoId) return null;
  isMutatingSmartFolder.value = true;
  error.value = null;
  try {
    const response = await createSmartFolder({
      ...request,
      repoId,
      parentId: request.parentId || undefined,
      filter: normalizeSmartFolderFilter(request.filter),
    });
    smartFolders.value = response.smartFolders;
    if (response.smartFolder) {
      await selectSmartFolder(response.smartFolder.smartFolderId);
    }
    return response;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    return null;
  } finally {
    isMutatingSmartFolder.value = false;
  }
}

export async function updateSmartFolderInWorkspace(request: Omit<SmartFolderUpdateRequest, "repoId">) {
  const repoId = activeRepoId.value;
  if (!repoId) return null;
  isMutatingSmartFolder.value = true;
  error.value = null;
  try {
    const response = await updateSmartFolder({
      ...request,
      repoId,
      parentId: request.parentId || undefined,
      filter: normalizeSmartFolderFilter(request.filter),
    });
    smartFolders.value = response.smartFolders;
    if (activeSmartFolderId.value === request.smartFolderId) {
      await selectSmartFolder(request.smartFolderId);
    }
    return response;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    return null;
  } finally {
    isMutatingSmartFolder.value = false;
  }
}

export async function deleteSmartFolderInWorkspace(smartFolderId: string) {
  const repoId = activeRepoId.value;
  if (!repoId) return null;
  isMutatingSmartFolder.value = true;
  error.value = null;
  try {
    const response = await deleteSmartFolder(repoId, smartFolderId);
    smartFolders.value = response.smartFolders;
    if (!smartFolderTreeContains(response.smartFolders, activeSmartFolderId.value)) {
      activeSmartFolderId.value = null;
      smartFolderResult.value = null;
      activePanel.value = "files";
    }
    return response;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    return null;
  } finally {
    isMutatingSmartFolder.value = false;
  }
}

export async function refreshRepositoryActions(repoId = activeRepoId.value) {
  if (!repoId) {
    repositoryActions.value = [];
    activeRepositoryActionId.value = null;
    return [];
  }
  isLoadingRepositoryActions.value = true;
  error.value = null;
  try {
    const actions = await listRepositoryActions(repoId);
    if (activeRepoId.value === repoId) {
      repositoryActions.value = actions;
      if (activeRepositoryActionId.value && !actions.some((action) => action.actionId === activeRepositoryActionId.value)) {
        activeRepositoryActionId.value = null;
      }
      activeRepositoryActionId.value = activeRepositoryActionId.value ?? actions[0]?.actionId ?? null;
    }
    return actions;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    return [];
  } finally {
    isLoadingRepositoryActions.value = false;
  }
}

export function selectRepositoryAction(actionId: string) {
  activeRepositoryActionId.value = actionId;
  activePanel.value = "actions";
}

export async function runActiveRepositoryAction(actionId = activeRepositoryActionId.value) {
  const repoId = activeRepoId.value;
  if (!repoId || !actionId) return null;
  const targetPaths = selectedFilePaths.value.length
    ? selectedFilePaths.value
    : selectedFilePath.value ? [selectedFilePath.value] : [];
  isRunningRepositoryAction.value = true;
  error.value = null;
  try {
    const response = await runRepositoryAction({
      repoId,
      actionId,
      targetPaths,
    });
    repositoryActions.value = repositoryActions.value.map((action) => (
      action.actionId === response.action.actionId ? response.action : action
    ));
    await refreshWorkspaceAfterMutation(repoId, {
      directory: fileBrowser.value && !fileBrowser.value.specialLocation ? "current" : undefined,
      repositorySnapshot: true,
    });
    return response;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    return null;
  } finally {
    isRunningRepositoryAction.value = false;
  }
}

export function setActivePanel(panel: WorkspacePanelKey) {
  activePanel.value = panel;
  if (panel === "files" && activeRepoId.value && fileBrowser.value?.specialLocation === "trash") {
    void loadFileBrowserForDirectory("", { includeTree: true });
  }
  if (panel === "deleted" && activeRepoId.value) {
    void loadFileBrowserForDirectory("", { specialLocation: "trash" });
  }
}

export async function saveAssetMetadata(metadata: Record<string, unknown>) {
  if (!activeRepoId.value || !activeAssetDetail.value) return null;

  isSavingMetadata.value = true;
  error.value = null;

  try {
    const response = await updateAssetMetadata({
      repoId: activeRepoId.value,
      assetId: activeAssetDetail.value.summary.assetId,
      expectedVersion: activeAssetDetail.value.summary.version,
      metadata,
      source: "desktop",
    });

    applyAssetResponse(response);
    return response;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    return null;
  } finally {
    isSavingMetadata.value = false;
  }
}

export async function confirmWorkspaceHardlinkCandidate(candidateId: string): Promise<HardlinkConfirmResponse | null> {
  const repoId = activeRepoId.value;
  if (!repoId) return null;
  isMutatingFiles.value = true;
  error.value = null;
  try {
    const response = await confirmHardlinkCandidate({
      repoId,
      candidateId,
    });
    await refreshWorkspaceAfterMutation(repoId, {
      directory: fileBrowser.value && !fileBrowser.value.specialLocation ? "current" : undefined,
      hardlinkCandidates: true,
      repositorySnapshot: true,
    });
    return response;
  } catch (cause) {
    const confirmError = cause instanceof Error ? cause.message : String(cause);
    try {
      await refreshHardlinkCandidates(repoId);
    } catch {
      // Keep the confirmation error visible if the follow-up refresh also fails.
    }
    error.value = confirmError;
    return null;
  } finally {
    isMutatingFiles.value = false;
  }
}

export async function syncActiveRepository() {
  if (!activeRepoId.value) return null;

  isSyncing.value = true;
  error.value = null;
  const progressId = startOperationProgress("同步资源库", "扫描文件变化", { initial: 10 });
  setSyncProgress("scanning", "扫描仓库文件", 1);

  try {
    const result = await syncRepository({ repoId: activeRepoId.value });
    updateOperationProgress(progressId, {
      detail: `已扫描 ${result.scannedFiles} 个文件`,
      value: 72,
      indeterminate: false,
    });
    setSyncProgress("writing", "写入索引结果", 2);
    lastSyncResult.value = result;
    setSyncProgress("refreshing", "刷新仓库视图", 3);
    await refreshWorkspaceAfterMutation(activeRepoId.value, {
      directory: activePanel.value === "files"
        ? "currentWithTree"
        : activePanel.value === "deleted" ? "trash" : undefined,
      hardlinkCandidates: true,
      repositorySnapshot: true,
      repositorySummary: true,
    });
    setSyncProgress("complete", "同步完成", 3);
    finishOperationProgress(progressId);
    return result;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    cancelOperationProgress(progressId);
    setSyncProgress("error", error.value, 3);
    return null;
  } finally {
    isSyncing.value = false;
  }
}

export async function refreshFileBrowserTree() {
  if (!activeRepoId.value) return null;

  isLoadingFileBrowser.value = true;
  error.value = null;
  const progressId = startOperationProgress("刷新文件树", "同步并读取目录结构", { initial: 12 });
  setSyncProgress("scanning", "扫描文件夹结构", 1);
  try {
    const result = await syncRepository({ repoId: activeRepoId.value });
    updateOperationProgress(progressId, { detail: `已扫描 ${result.scannedFiles} 个文件`, value: 58 });
    setSyncProgress("writing", "写入索引结果", 2);
    lastSyncResult.value = result;
    setSyncProgress("refreshing", "刷新文件夹树", 3);
    await refreshWorkspaceAfterMutation(activeRepoId.value, {
      directory: activePanel.value === "deleted" ? "trash" : "currentWithTree",
      hardlinkCandidates: true,
      repositorySnapshot: true,
    });
    setSyncProgress("complete", "刷新完成", 3);
    finishOperationProgress(progressId);
    return fileBrowser.value;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    cancelOperationProgress(progressId);
    setSyncProgress("error", error.value, 3);
    return null;
  } finally {
    isLoadingFileBrowser.value = false;
  }
}

export async function undoAssetRevision() {
  if (!activeRepoId.value || !activeAssetId.value) return null;

  try {
    const response = await undoLastRevision({
      repoId: activeRepoId.value,
      assetId: activeAssetId.value,
    });
    applyAssetResponse(response);
    return response;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    return null;
  }
}

export async function redoAssetRevision() {
  if (!activeRepoId.value || !activeAssetId.value) return null;

  try {
    const response = await redoLastRevision({
      repoId: activeRepoId.value,
      assetId: activeAssetId.value,
    });
    applyAssetResponse(response);
    return response;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    return null;
  }
}

export async function createNewRepository(
  name: string,
  path: string,
  backendPluginId?: string,
  backendConfig?: Record<string, unknown>,
) {
  const progressId = startOperationProgress("创建资源库", "初始化资源库并扫描文件", { initial: 8 });
  try {
    await createRepository({ name, path, backendPluginId, backendConfig });
    await loadRepositories();
    finishOperationProgress(progressId);
  } catch (cause) {
    cancelOperationProgress(progressId);
    throw cause;
  }
}

export async function importExistingRepository(name: string, path: string) {
  const progressId = startOperationProgress("导入资源库", "读取资源库元数据并扫描文件", { initial: 8 });
  try {
    await importRepository({ name, path });
    await loadRepositories();
    finishOperationProgress(progressId);
  } catch (cause) {
    cancelOperationProgress(progressId);
    throw cause;
  }
}

export async function attachRepository(path: string) {
  const progressId = startOperationProgress("挂载资源库", "检查文件夹并读取索引", { initial: 8 });
  try {
    await attachRepositoryFolder({ path });
    await loadRepositories();
    finishOperationProgress(progressId);
  } catch (cause) {
    cancelOperationProgress(progressId);
    throw cause;
  }
}

export async function removeRepository(repoId: string) {
  await deleteRepository(repoId);
  await loadRepositories();
}

export async function relocateMissingRepository(repoId: string, path: string) {
  const progressId = startOperationProgress("重定向资源库", "校验资源库位置", { initial: 12 });
  try {
    const response = await relocateRepository({ repoId, path });
    updateOperationProgress(progressId, { detail: "刷新资源库列表", value: 64 });
    await loadRepositories();
    if (activeRepoId.value !== response.repository.repoId || !activeSnapshot.value) {
      await selectRepository(response.repository.repoId);
    }
    finishOperationProgress(progressId);
    return response;
  } catch (cause) {
    cancelOperationProgress(progressId);
    throw cause;
  }
}

export async function exportCurrentRepository(
  request: Omit<RepositoryExportRequest, "repoId">,
): Promise<RepositoryExportResponse | null> {
  if (!activeRepoId.value) return null;

  error.value = null;
  const progressId = startOperationProgress(
    request.target === "git" ? "上传到 Git" : "导出资源库",
    request.target === "git" ? "准备提交并推送资源库" : "准备打包资源库文件",
    { initial: 8 },
  );

  try {
    const response = await exportRepository({
      ...request,
      repoId: activeRepoId.value,
    });
    updateOperationProgress(progressId, { detail: response.message, value: 92 });
    finishOperationProgress(progressId);
    return response;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    cancelOperationProgress(progressId);
    return null;
  }
}

type SettingsDataLoadOptions = {
  failFast?: boolean;
};

export async function loadSettingsData(options: SettingsDataLoadOptions = {}) {
  isLoadingSettingsData.value = true;

  try {
    const [pluginItems, cache, api] = await Promise.all([
      listPlugins(),
      getCacheSnapshot(),
      getApiDesignSnapshot(),
    ]);
    plugins.value = pluginItems;
    await syncRegisteredPreviewPluginManifests(pluginItems);
    cacheSnapshot.value = cache;
    apiDesign.value = api;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    if (options.failFast) {
      throw cause;
    }
  } finally {
    isLoadingSettingsData.value = false;
  }
}

export function setActivePreviewPath(path: string | null) {
  activePreviewPath.value = path;
}

export async function refreshPlaylists(repoId = activeRepoId.value) {
  if (!repoId) return [];
  const items = await listPlaylists(repoId);
  playlists.value = items;
  await syncPlaylistMemberships(repoId, items);
  if (activePlaylistId.value && !items.some((item) => item.playlistId === activePlaylistId.value)) {
    activePlaylistId.value = null;
    activePlaylistDetail.value = null;
    if (activePanel.value === "playlist") {
      activePanel.value = "files";
    }
  }
  return items;
}

export async function selectPlaylist(playlistId: string) {
  if (!activeRepoId.value) return null;
  activePanel.value = "playlist";
  activePlaylistId.value = playlistId;
  activePlaylistDetail.value = await getPlaylistDetail(activeRepoId.value, playlistId);
  return activePlaylistDetail.value;
}

export async function createPlaylistInWorkspace(request: Omit<PlaylistMutationRequest, "repoId">) {
  if (!activeRepoId.value) return null;
  const response = await createPlaylist({ ...request, repoId: activeRepoId.value });
  playlists.value = response.playlists;
  await syncPlaylistMemberships(activeRepoId.value, response.playlists);
  if (response.playlist?.playlistId) {
    await selectPlaylist(response.playlist.playlistId);
  }
  return response;
}

export async function deletePlaylistInWorkspace(playlistId: string) {
  if (!activeRepoId.value) return null;
  const response = await deletePlaylist(activeRepoId.value, playlistId);
  playlists.value = response.playlists;
  await syncPlaylistMemberships(activeRepoId.value, response.playlists);
  if (activePlaylistId.value === playlistId) {
    activePlaylistId.value = null;
    activePlaylistDetail.value = null;
    if (activePanel.value === "playlist") activePanel.value = "files";
  }
  return response;
}

export async function addPlaylistItemsInWorkspace(playlistId: string, assetIds: string[]) {
  if (!activeRepoId.value) return null;
  const detail = await addPlaylistItems({
    repoId: activeRepoId.value,
    playlistId,
    assetIds,
  });
  activePlaylistDetail.value = detail;
  await refreshPlaylists(activeRepoId.value);
  return detail;
}

export async function reorderPlaylistItemsInWorkspace(playlistId: string, itemIds: string[]) {
  if (!activeRepoId.value) return null;
  const detail = await reorderPlaylistItems({
    repoId: activeRepoId.value,
    playlistId,
    itemIds,
  });
  activePlaylistDetail.value = detail;
  await refreshPlaylists(activeRepoId.value);
  return detail;
}

export async function removePlaylistItemInWorkspace(playlistId: string, playlistItemId: string) {
  if (!activeRepoId.value) return null;
  const detail = await removePlaylistItem({
    repoId: activeRepoId.value,
    playlistId,
    playlistItemId,
  });
  activePlaylistDetail.value = detail;
  await refreshPlaylists(activeRepoId.value);
  return detail;
}

export async function setPlaylistMembershipInWorkspace(assetId: string, playlistIds: string[]) {
  if (!activeRepoId.value) return null;
  const response: PlaylistMembershipSnapshot = await setPlaylistMembership({
    repoId: activeRepoId.value,
    assetId,
    playlistIds,
  });
  await refreshPlaylists(activeRepoId.value);
  return response;
}
async function applyPluginMutation(action: () => Promise<{ plugins: import("../../types/repository").PluginManifest[] }>) {
  isManagingPlugins.value = true;
  error.value = null;
  try {
    const response = await action();
    plugins.value = response.plugins;
    await syncRegisteredPreviewPluginManifests(response.plugins);
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
function applyAssetResponse(response: { asset: AssetDetail }) {
  activeAssetDetail.value = response.asset;
  activeAssetId.value = response.asset.summary.assetId;
  const metadata = Object.fromEntries(response.asset.metadata.map((entry) => [entry.key, entry.value]));

  if (fileBrowser.value) {
    fileBrowser.value = {
      ...fileBrowser.value,
      entries: fileBrowser.value.entries.map((entry) => (
        entry.assetId === response.asset.summary.assetId
          ? {
              ...entry,
              metadata,
            }
          : entry
      )),
    };
  }

  if (!activeSnapshot.value) return;

  activeSnapshot.value = {
    ...activeSnapshot.value,
    assets: activeSnapshot.value.assets.map((asset) => (
      asset.assetId === response.asset.summary.assetId ? response.asset.summary : asset
    )),
    recentRevisionCount: activeSnapshot.value.recentRevisionCount + 1,
  };
}

async function refreshWorkspaceAfterMutation(
  repoId: string,
  plan: WorkspaceRefreshPlan,
) {
  await refreshWorkspaceWithDirectory(repoId, plan, loadFileBrowserForDirectory);
}

export function ensureRepositoryWorkspace() {
  return ensureRepositoryWorkspaceLifecycle(selectAsset, loadSettingsData);
}

export function refreshRepositoryWorkspace() {
  return loadRepositories();
}

export function useRepositoryWorkspace() {
  return {
    repositories: computed(() => repositories.value),
    activeRepoId: computed(() => activeRepoId.value),
    activeSnapshot: computed(() => activeSnapshot.value),
    activeAssetId: computed(() => activeAssetId.value),
    activeAssetDetail: computed(() => activeAssetDetail.value),
    activePreviewPath: computed(() => activePreviewPath.value),
    activePanel: computed(() => activePanel.value),
    currentDirectoryPath: computed(() => currentDirectoryPath.value),
    fileBrowser: computed(() => fileBrowser.value),
    fileTree: computed(() => fileTree.value),
    selectedFilePath: computed(() => selectedFilePath.value),
    selectedFilePaths: computed(() => selectedFilePaths.value),
    searchQuery: computed(() => searchQuery.value),
    searchResults: computed(() => searchResults.value ?? []),
    smartFolders: computed(() => smartFolders.value ?? []),
    repositoryActions: computed(() => repositoryActions.value ?? []),
    playlists: computed(() => playlists.value ?? []),
    playlistMemberships: computed(() => playlistMemberships.value ?? {}),
    activePlaylistId: computed(() => activePlaylistId.value),
    activePlaylistDetail: computed(() => activePlaylistDetail.value),
    activeSmartFolderId: computed(() => activeSmartFolderId.value),
    activeRepositoryActionId: computed(() => activeRepositoryActionId.value),
    smartFolderResult: computed(() => smartFolderResult.value),
    activeRepository,
    fileBrowserEntryMap,
    visibleEntries,
    selectedEntry,
    selectedEntries,
    selectedFilePathSet,
    hasMultipleSelection,
    directoryEntries,
    fileEntries,
    hasSplitFileGroups,
    libraryOverview,
    breadcrumbSegments,
    isFilterBarOpen: computed(() => isFilterBarOpen.value),
    filters: computed(() => filters.value),
    activeFilterCount,
    hasActiveFilters,
    hardlinkCandidates: computed(() => hardlinkCandidates.value ?? []),
    lastSyncResult: computed(() => lastSyncResult.value),
    plugins: computed(() => plugins.value ?? []),
    repositoryBackendOptions,
    cacheSnapshot: computed(() => cacheSnapshot.value),
    apiDesign: computed(() => apiDesign.value),
    operationProgress: computed(() => operationProgress.value),
    workspaceStartup: computed(() => workspaceStartup.value),
    syncProgress: computed(() => syncProgress.value),
    isLoadingRepositories: computed(() => isLoadingRepositories.value),
    isLoadingSnapshot: computed(() => isLoadingSnapshot.value),
    isLoadingAssetDetail: computed(() => isLoadingAssetDetail.value),
    isLoadingFileBrowser: computed(() => isLoadingFileBrowser.value),
    isSearching: computed(() => isSearching.value),
    isLoadingSmartFolder: computed(() => isLoadingSmartFolder.value),
    isLoadingRepositoryActions: computed(() => isLoadingRepositoryActions.value),
    isSavingMetadata: computed(() => isSavingMetadata.value),
    isSyncing: computed(() => isSyncing.value),
    isMutatingFiles: computed(() => isMutatingFiles.value),
    isMutatingSmartFolder: computed(() => isMutatingSmartFolder.value),
    isRunningRepositoryAction: computed(() => isRunningRepositoryAction.value),
    isLoadingSettingsData: computed(() => isLoadingSettingsData.value),
    isManagingPlugins: computed(() => isManagingPlugins.value),
    isExternalDragActive: computed(() => isExternalDragActive.value),
    isInternalDragActive: computed(() => isInternalDragActive.value),
    draggedWorkspacePaths: computed(() => draggedWorkspacePaths.value),
    dragHoverFolderPath: computed(() => dragHoverFolderPath.value),
    isBusy: computed(() => (
      isLoadingRepositories.value ||
      isLoadingSnapshot.value ||
      isLoadingAssetDetail.value
    )),
    error: computed(() => error.value),
    ensureRepositoryWorkspace,
    refreshRepositoryWorkspace,
    selectRepository,
    selectAsset,
    loadFileBrowserForDirectory,
    refreshFileBrowserTree,
    createDirectoryInWorkspace,
    createFileInWorkspace,
    importEntriesToWorkspace,
    copyWorkspaceEntries,
    moveWorkspaceEntries,
    renameWorkspaceEntry,
    deleteWorkspaceEntry,
    deleteWorkspaceEntries,
    restoreTrashEntry,
    restoreTrashEntries,
    restoreAllTrashEntries,
    emptyTrash,
    openWorkspaceEntry,
    revealWorkspaceEntry,
    setActivePreviewPath,
    startWorkspaceEntryDrag,
    startWorkspaceEntriesDrag,
    selectWorkspaceEntry,
    selectWorkspaceEntries,
    clearWorkspaceSelection,
    refreshSmartFolders,
    refreshPlaylists,
    selectPlaylist,
    createPlaylistInWorkspace,
    deletePlaylistInWorkspace,
    addPlaylistItemsInWorkspace,
    reorderPlaylistItemsInWorkspace,
    removePlaylistItemInWorkspace,
    setPlaylistMembershipInWorkspace,
    selectSmartFolder,
    refreshRepositoryActions,
    selectRepositoryAction,
    runActiveRepositoryAction,
    createSmartFolderInWorkspace,
    updateSmartFolderInWorkspace,
    deleteSmartFolderInWorkspace,
    setWorkspaceEntryThumbnail,
    setWorkspaceEntryThumbnailFromBytes,
    saveGeneratedWorkspaceEntryThumbnail,
    clearWorkspaceEntryThumbnail,
    refreshWorkspaceEntryThumbnail,
    setActivePanel,
    setExternalDragActive,
    setInternalDragActive,
    setDraggedWorkspacePaths,
    clearDraggedWorkspaceState,
    setDragHoverFolderPath,
    setFilterBarOpen,
    toggleFilterBar,
    toggleFilterValue,
    setMinimumRatingFilter,
    updateFilters,
    clearFilters,
    runSearch,
    runFilteredSearch,
    saveAssetMetadata,
    refreshHardlinkCandidates,
    confirmWorkspaceHardlinkCandidate,
    syncActiveRepository,
    undoAssetRevision,
    redoAssetRevision,
    createNewRepository,
    importExistingRepository,
    attachRepository,
    removeRepository,
    relocateMissingRepository,
    exportCurrentRepository,
    loadSettingsData,
    setPluginEnabledInWorkspace,
    deletePluginInWorkspace,
    installPluginArchiveInWorkspace,
  };
}
