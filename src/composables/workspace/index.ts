import { computed } from "vue";
import {
  getAssetDetail,
  getRepositorySnapshot,
} from "../../services/repositoryApi";
import {
  activeAssetDetail,
  activeAssetId,
  activePanel,
  activeRepoId,
  activeRepositoryActionId,
  activeSmartFolderId,
  activeSnapshot,
  apiDesign,
  cacheSnapshot,
  currentDirectoryPath,
  dragHoverFolderPath,
  error,
  externalApiConnection,
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
  repositories,
  repositoryActions,
  searchQuery,
  searchResults,
  draggedWorkspacePaths,
  selectedFilePath,
  selectedFilePaths,
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
} from "./refresh";
import {
  cancelOperationProgress,
  finishOperationProgress,
  operationProgress,
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
  setWorkspaceEntryThumbnailFromUrl,
} from "./thumbnails";
import { loadFileBrowserForDirectory } from "./files";
import {
  ensureRepositoryWorkspace as ensureRepositoryWorkspaceLifecycle,
  loadRepositories as loadRepositoriesLifecycle,
  resetActiveRepositoryContent,
  queueRepositoryBackgroundLoads,
} from "./lifecycle";
import {
  deletePluginInWorkspace,
  installPluginArchiveInWorkspace,
  loadSettingsData,
  setPluginEnabledInWorkspace,
} from "./settings";
import {
  attachRepository,
  configureRepositoryWorkspaceActions,
  createNewRepository,
  exportCurrentRepository,
  importExistingRepository,
  relocateMissingRepository,
  removeRepository,
} from "./repositories";
import {
  createSmartFolderInWorkspace,
  deleteSmartFolderInWorkspace,
  refreshSmartFolders,
  selectSmartFolder,
  updateSmartFolderInWorkspace,
} from "./smartFolders";
import {
  refreshRepositoryActions,
  runActiveRepositoryAction,
  selectRepositoryAction,
} from "./repositoryActions";
import {
  redoAssetRevision,
  saveAssetMetadata,
  undoAssetRevision,
} from "./assetMetadata";
import {
  clearWorkspaceSelection,
  selectWorkspaceEntries,
  selectWorkspaceEntry,
  setDragHoverFolderPath,
  setDraggedWorkspacePaths,
  setExternalDragActive,
  setInternalDragActive,
  clearDraggedWorkspaceState,
} from "./selection";
import {
  copyWorkspaceEntries,
  createDirectoryInWorkspace,
  createFileInWorkspace,
  deleteWorkspaceEntries,
  deleteWorkspaceEntry,
  emptyTrash,
  importEntriesToWorkspace,
  moveWorkspaceEntries,
  openWorkspaceEntry,
  renameWorkspaceEntry,
  restoreAllTrashEntries,
  restoreTrashEntries,
  restoreTrashEntry,
  revealWorkspaceEntry,
  startWorkspaceEntriesDrag,
  startWorkspaceEntryDrag,
} from "./fileOperations";
import {
  confirmWorkspaceHardlinkCandidate,
  refreshFileBrowserTree,
  syncActiveRepository,
} from "./sync";

export type { WorkspaceFilterState, WorkspaceOperationProgress, WorkspacePanelKey };
export { resetRepositoryWorkspaceForTests } from "./lifecycle";
export {
  deletePluginInWorkspace,
  installPluginArchiveInWorkspace,
  loadSettingsData,
  setPluginEnabledInWorkspace,
} from "./settings";
export {
  attachRepository,
  createNewRepository,
  exportCurrentRepository,
  importExistingRepository,
  relocateMissingRepository,
  removeRepository,
} from "./repositories";
export {
  createSmartFolderInWorkspace,
  deleteSmartFolderInWorkspace,
  refreshSmartFolders,
  selectSmartFolder,
  updateSmartFolderInWorkspace,
} from "./smartFolders";
export {
  refreshRepositoryActions,
  runActiveRepositoryAction,
  selectRepositoryAction,
} from "./repositoryActions";
export {
  redoAssetRevision,
  saveAssetMetadata,
  undoAssetRevision,
} from "./assetMetadata";
export {
  clearDraggedWorkspaceState,
  clearWorkspaceSelection,
  selectWorkspaceEntries,
  selectWorkspaceEntry,
  setDragHoverFolderPath,
  setDraggedWorkspacePaths,
  setExternalDragActive,
  setInternalDragActive,
} from "./selection";
export {
  copyWorkspaceEntries,
  createDirectoryInWorkspace,
  createFileInWorkspace,
  deleteWorkspaceEntries,
  deleteWorkspaceEntry,
  emptyTrash,
  importEntriesToWorkspace,
  moveWorkspaceEntries,
  openWorkspaceEntry,
  renameWorkspaceEntry,
  restoreAllTrashEntries,
  restoreTrashEntries,
  restoreTrashEntry,
  revealWorkspaceEntry,
  startWorkspaceEntriesDrag,
  startWorkspaceEntryDrag,
} from "./fileOperations";
export {
  confirmWorkspaceHardlinkCandidate,
  refreshFileBrowserTree,
  syncActiveRepository,
} from "./sync";

async function loadRepositories() {
  return loadRepositoriesLifecycle(selectRepository);
}

configureRepositoryWorkspaceActions({
  loadRepositories,
  selectRepository,
});

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
    if (isSwitchingRepository) {
      resetSearchState();
      activeSmartFolderId.value = null;
      smartFolderResult.value = null;
    }

    const defaultAssetId = activeAssetId.value && snapshot.assets.some((item) => item.assetId === activeAssetId.value)
      ? activeAssetId.value
      : snapshot.assets[0]?.assetId ?? null;

    activeAssetId.value = defaultAssetId;
    activeAssetDetail.value = null;

    currentDirectoryPath.value = "";
    await loadFileBrowserForDirectory("", { includeTree: false });
    if (defaultAssetId) {
      void selectAsset(defaultAssetId);
    }
    queueRepositoryBackgroundLoads(repoId);
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

export function setActivePanel(panel: WorkspacePanelKey) {
  activePanel.value = panel;
  if (panel === "files" && activeRepoId.value && fileBrowser.value?.specialLocation === "trash") {
    void loadFileBrowserForDirectory("", { includeTree: false });
  }
  if (panel === "deleted" && activeRepoId.value) {
    void loadFileBrowserForDirectory("", { specialLocation: "trash" });
  }
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
    selectedFilePaths: computed(() => selectedFilePaths.value),
    searchQuery: computed(() => searchQuery.value),
    searchResults: computed(() => searchResults.value),
    smartFolders: computed(() => smartFolders.value),
    repositoryActions: computed(() => repositoryActions.value),
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
    hardlinkCandidates: computed(() => hardlinkCandidates.value),
    lastSyncResult: computed(() => lastSyncResult.value),
    plugins: computed(() => plugins.value),
    repositoryBackendOptions,
    cacheSnapshot: computed(() => cacheSnapshot.value),
    apiDesign: computed(() => apiDesign.value),
    externalApiConnection: computed(() => externalApiConnection.value),
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
    startWorkspaceEntryDrag,
    startWorkspaceEntriesDrag,
    selectWorkspaceEntry,
    selectWorkspaceEntries,
    clearWorkspaceSelection,
    refreshSmartFolders,
    selectSmartFolder,
    refreshRepositoryActions,
    selectRepositoryAction,
    runActiveRepositoryAction,
    createSmartFolderInWorkspace,
    updateSmartFolderInWorkspace,
    deleteSmartFolderInWorkspace,
    setWorkspaceEntryThumbnail,
    setWorkspaceEntryThumbnailFromBytes,
    setWorkspaceEntryThumbnailFromUrl,
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
