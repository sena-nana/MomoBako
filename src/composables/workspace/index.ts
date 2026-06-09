import { computed } from "vue";
import {
  attachRepositoryFolder,
  confirmHardlinkCandidate,
  deletePlugin,
  copyEntries,
  createDirectory,
  createFile,
  createRepository,
  deleteEntry,
  deleteRepository,
  ensureThumbnail,
  exportRepository,
  getApiDesignSnapshot,
  getAssetDetail,
  getCacheSnapshot,
  importEntries,
  getRepositorySnapshot,
  importRepository,
  installPluginFromArchive,
  listPlugins,
  setPluginEnabled,
  mutateTrash,
  openRepositoryPath,
  redoLastRevision,
  renameEntry,
  revealRepositoryPath,
  syncRepository,
  undoLastRevision,
  updateAssetMetadata,
} from "../../services/repositoryApi";
import { syncRegisteredPreviewPluginManifests } from "../../plugins/sdk";
import type {
  AssetDetail,
  FileDeleteMode,
  HardlinkConfirmResponse,
  RepositoryExportRequest,
  RepositoryExportResponse,
} from "../../types/repository";
import {
  activeAssetDetail,
  activeAssetId,
  activePanel,
  activeRepoId,
  activeSnapshot,
  apiDesign,
  cacheSnapshot,
  currentDirectoryPath,
  error,
  fileBrowser,
  fileTree,
  filters,
  hardlinkCandidates,
  isFilterBarOpen,
  isLoadingAssetDetail,
  isLoadingFileBrowser,
  isLoadingRepositories,
  isLoadingSnapshot,
  isLoadingSettingsData,
  isManagingPlugins,
  isMutatingFiles,
  isSavingMetadata,
  isSearching,
  isSyncing,
  lastSyncResult,
  plugins,
  repositories,
  searchQuery,
  searchResults,
  selectedFilePath,
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
  hasSplitFileGroups,
  libraryOverview,
  repositoryBackendOptions,
  selectedEntry,
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
  applyThumbnailResponse,
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
} from "./lifecycle";

export type { WorkspaceFilterState, WorkspaceOperationProgress, WorkspacePanelKey };
export { resetRepositoryWorkspaceForTests } from "./lifecycle";

async function loadRepositories() {
  return loadRepositoriesLifecycle(selectRepository);
}

async function finishFileTransfer(
  repoId: string,
  snapshot: import("../../types/repository").FileBrowserSnapshot,
  sourcePaths: string[],
) {
  applyFileBrowserSnapshot(snapshot);
  const sourceNames = new Set(sourcePaths.map(entryNameFromPath));
  selectedFilePath.value = snapshot.entries.find((entry) => sourceNames.has(entry.name))?.path ?? selectedFilePath.value;
  await refreshWorkspaceAfterMutation(repoId, { hardlinkCandidates: true, repositorySnapshot: true });
}

export async function selectRepository(repoId: string) {
  if (!repoId) return;

  const isSwitchingRepository = activeRepoId.value !== repoId;
  isLoadingSnapshot.value = true;
  error.value = null;
  const progressId = startOperationProgress("加载资源库", "读取资源库快照", { initial: 10, indeterminate: true });

  try {
    const snapshot = await getRepositorySnapshot(repoId);
    updateOperationProgress(progressId, { detail: "加载资源索引", value: 46 });
    activeRepoId.value = repoId;
    activeSnapshot.value = snapshot;
    if (isSwitchingRepository) {
      resetSearchState();
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
    selectedFilePath.value = snapshot.entries.find((entry) => entry.name === name)?.path ?? selectedFilePath.value;
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
    selectedFilePath.value = snapshot.entries.find((entry) => entry.name === newName)?.path ?? selectedFilePath.value;
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

export function selectWorkspaceEntry(path: string) {
  selectedFilePath.value = path;
}

export async function setWorkspaceEntryThumbnail(path: string, sourcePath: string) {
  if (!activeRepoId.value || fileBrowser.value?.specialLocation === "trash") return null;
  error.value = null;
  try {
    const response = await ensureThumbnail({
      repoId: activeRepoId.value,
      path,
      action: "save",
      sourcePath,
    });
    applyThumbnailResponse(response);
    return response;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    return null;
  }
}

export async function setWorkspaceEntryThumbnailFromBytes(path: string, imageBytes: number[], mediaType?: string) {
  if (!activeRepoId.value || fileBrowser.value?.specialLocation === "trash") return null;
  error.value = null;
  try {
    const response = await ensureThumbnail({
      repoId: activeRepoId.value,
      path,
      action: "save",
      imageBytes,
      mediaType,
    });
    applyThumbnailResponse(response);
    return response;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    return null;
  }
}

export async function saveGeneratedWorkspaceEntryThumbnail(path: string, imageBytes: number[], mediaType?: string) {
  if (!activeRepoId.value || fileBrowser.value?.specialLocation === "trash") return null;
  error.value = null;
  try {
    const response = await ensureThumbnail({
      repoId: activeRepoId.value,
      path,
      action: "saveGenerated",
      imageBytes,
      mediaType,
    });
    applyThumbnailResponse(response);
    return response;
  } catch {
    return null;
  }
}

export async function clearWorkspaceEntryThumbnail(path: string) {
  if (!activeRepoId.value || fileBrowser.value?.specialLocation === "trash") return null;
  error.value = null;
  try {
    const response = await ensureThumbnail({
      repoId: activeRepoId.value,
      path,
      action: "clear",
    });
    applyThumbnailResponse(response);
    return response;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    return null;
  }
}

export async function refreshWorkspaceEntryThumbnail(path: string) {
  if (!activeRepoId.value || fileBrowser.value?.specialLocation === "trash") return null;
  error.value = null;
  try {
    const response = await ensureThumbnail({
      repoId: activeRepoId.value,
      path,
      action: "refresh",
    });
    applyThumbnailResponse(response);
    return response;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    return null;
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
    syncRegisteredPreviewPluginManifests(pluginItems);
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
async function applyPluginMutation(action: () => Promise<{ plugins: import("../../types/repository").PluginManifest[] }>) {
  isManagingPlugins.value = true;
  error.value = null;
  try {
    const response = await action();
    plugins.value = response.plugins;
    syncRegisteredPreviewPluginManifests(response.plugins);
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

export function installPluginArchiveInWorkspace(archivePath: string) {
  return applyPluginMutation(() => installPluginFromArchive({ archivePath }));
}
function applyAssetResponse(response: { asset: AssetDetail }) {
  activeAssetDetail.value = response.asset;
  activeAssetId.value = response.asset.summary.assetId;

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
    activePanel: computed(() => activePanel.value),
    currentDirectoryPath: computed(() => currentDirectoryPath.value),
    fileBrowser: computed(() => fileBrowser.value),
    fileTree: computed(() => fileTree.value),
    selectedFilePath: computed(() => selectedFilePath.value),
    searchQuery: computed(() => searchQuery.value),
    searchResults: computed(() => searchResults.value),
    activeRepository,
    fileBrowserEntryMap,
    selectedEntry,
    directoryEntries,
    fileEntries,
    hasSplitFileGroups,
    libraryOverview,
    breadcrumbSegments,
    isFilterBarOpen: computed(() => isFilterBarOpen.value),
    filters: computed(() => filters.value),
    activeFilterCount,
    hasActiveFilters,
    hardlinkCandidates: computed(() => hardlinkCandidates.value),
    lastSyncResult: computed(() => lastSyncResult.value),
    plugins: computed(() => plugins.value),
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
    isSavingMetadata: computed(() => isSavingMetadata.value),
    isSyncing: computed(() => isSyncing.value),
    isMutatingFiles: computed(() => isMutatingFiles.value),
    isLoadingSettingsData: computed(() => isLoadingSettingsData.value),
    isManagingPlugins: computed(() => isManagingPlugins.value),
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
    renameWorkspaceEntry,
    deleteWorkspaceEntry,
    restoreTrashEntry,
    restoreAllTrashEntries,
    emptyTrash,
    openWorkspaceEntry,
    revealWorkspaceEntry,
    selectWorkspaceEntry,
    setWorkspaceEntryThumbnail,
    setWorkspaceEntryThumbnailFromBytes,
    saveGeneratedWorkspaceEntryThumbnail,
    clearWorkspaceEntryThumbnail,
    refreshWorkspaceEntryThumbnail,
    setActivePanel,
    setFilterBarOpen,
    toggleFilterBar,
    toggleFilterValue,
    setMinimumRatingFilter,
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
    exportCurrentRepository,
    loadSettingsData,
    setPluginEnabledInWorkspace,
    deletePluginInWorkspace,
    installPluginArchiveInWorkspace,
  };
}
