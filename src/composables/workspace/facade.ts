import { computed, type Ref } from "vue";
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
  draggedWorkspacePaths,
  error,
  externalApiConnection,
  fileBrowser,
  fileTree,
  filters,
  hardlinkCandidates,
  isExternalDragActive,
  isFilterBarOpen,
  isInternalDragActive,
  isLoadingAssetDetail,
  isLoadingFileBrowser,
  isLoadingRepositories,
  isLoadingRepositoryActions,
  isLoadingSettingsData,
  isLoadingSmartFolder,
  isLoadingSnapshot,
  isManagingPlugins,
  isMutatingFiles,
  isMutatingSmartFolder,
  isRunningRepositoryAction,
  isSavingMetadata,
  isSearching,
  isSyncing,
  lastSyncResult,
  playlistMemberships,
  playlists,
  pluginHookExecutions,
  plugins,
  repositories,
  repositoryActions,
  searchQuery,
  searchResults,
  selectedFilePath,
  selectedFilePaths,
  smartFolderResult,
  smartFolders,
  workspaceStartup,
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
  ensureRepositoryWorkspace,
  refreshRepositoryWorkspace,
  selectAsset,
  selectRepository,
  setActivePanel,
  setActivePreviewPath,
} from "./navigation";
import {
  clearFilters,
  runFilteredSearch,
  runSearch,
  setFilterBarOpen,
  setMinimumRatingFilter,
  toggleFilterBar,
  toggleFilterValue,
  updateFilters,
} from "./search";
import { refreshHardlinkCandidates } from "./refresh";
import { operationProgress, syncProgress } from "./tasks";
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
  deletePluginConfigValueInWorkspace,
  deletePluginInWorkspace,
  installPluginArchiveInWorkspace,
  loadPluginConfigInWorkspace,
  loadSettingsData,
  openPluginDataDirectoryInWorkspace,
  setPluginConfigValueInWorkspace,
  setPluginEnabledInWorkspace,
} from "./settings";
import {
  attachRepository,
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
  clearDraggedWorkspaceState,
  clearWorkspaceSelection,
  selectWorkspaceEntries,
  selectWorkspaceEntry,
  setDragHoverFolderPath,
  setDraggedWorkspacePaths,
  setExternalDragActive,
  setInternalDragActive,
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
import {
  addPlaylistItemsInWorkspace,
  createPlaylistInWorkspace,
  deletePlaylistInWorkspace,
  refreshPlaylists,
  removePlaylistItemInWorkspace,
  reorderPlaylistItemsInWorkspace,
  selectPlaylist,
  setPlaylistMembershipInWorkspace,
} from "./playlists";

function readonlyRef<T>(source: Ref<T>) {
  return computed(() => source.value);
}

function readonlyArrayRef<T>(source: Ref<T[]>) {
  return computed(() => source.value ?? []);
}

function readonlyRecordRef<T extends Record<string, unknown>>(source: Ref<T>) {
  return computed(() => source.value ?? {});
}

export function useWorkspaceRepository() {
  return {
    repositories: readonlyArrayRef(repositories),
    activeRepoId: readonlyRef(activeRepoId),
    activeSnapshot: readonlyRef(activeSnapshot),
    activeAssetId: readonlyRef(activeAssetId),
    activeAssetDetail: readonlyRef(activeAssetDetail),
    activeRepository,
    repositoryBackendOptions,
    libraryOverview,
    ensureRepositoryWorkspace,
    refreshRepositoryWorkspace,
    selectRepository,
    selectAsset,
    createNewRepository,
    importExistingRepository,
    attachRepository,
    removeRepository,
    relocateMissingRepository,
    exportCurrentRepository,
  };
}

export function useWorkspaceFiles() {
  return {
    activePreviewPath: readonlyRef(activePreviewPath),
    currentDirectoryPath: readonlyRef(currentDirectoryPath),
    fileBrowser: readonlyRef(fileBrowser),
    fileTree: readonlyArrayRef(fileTree),
    fileBrowserEntryMap,
    visibleEntries,
    directoryEntries,
    fileEntries,
    hasSplitFileGroups,
    breadcrumbSegments,
    hardlinkCandidates: readonlyArrayRef(hardlinkCandidates),
    lastSyncResult: readonlyRef(lastSyncResult),
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
    refreshHardlinkCandidates,
    confirmWorkspaceHardlinkCandidate,
    syncActiveRepository,
    setWorkspaceEntryThumbnail,
    setWorkspaceEntryThumbnailFromBytes,
    setWorkspaceEntryThumbnailFromUrl,
    saveGeneratedWorkspaceEntryThumbnail,
    clearWorkspaceEntryThumbnail,
    refreshWorkspaceEntryThumbnail,
  };
}

export function useWorkspaceSelection() {
  return {
    selectedFilePath: readonlyRef(selectedFilePath),
    selectedFilePaths: readonlyArrayRef(selectedFilePaths),
    selectedEntry,
    selectedEntries,
    selectedFilePathSet,
    hasMultipleSelection,
    isExternalDragActive: readonlyRef(isExternalDragActive),
    isInternalDragActive: readonlyRef(isInternalDragActive),
    draggedWorkspacePaths: readonlyArrayRef(draggedWorkspacePaths),
    dragHoverFolderPath: readonlyRef(dragHoverFolderPath),
    selectWorkspaceEntry,
    selectWorkspaceEntries,
    clearWorkspaceSelection,
    setExternalDragActive,
    setInternalDragActive,
    setDraggedWorkspacePaths,
    clearDraggedWorkspaceState,
    setDragHoverFolderPath,
  };
}

export function useWorkspaceNavigation() {
  return {
    activePanel: readonlyRef(activePanel),
    setActivePanel,
  };
}

export function useWorkspaceSearch() {
  return {
    searchQuery: readonlyRef(searchQuery),
    searchResults: readonlyArrayRef(searchResults),
    isFilterBarOpen: readonlyRef(isFilterBarOpen),
    filters: readonlyRef(filters),
    activeFilterCount,
    hasActiveFilters,
    setFilterBarOpen,
    toggleFilterBar,
    toggleFilterValue,
    setMinimumRatingFilter,
    updateFilters,
    clearFilters,
    runSearch,
    runFilteredSearch,
  };
}

export function useWorkspacePlaylists() {
  return {
    playlists: readonlyArrayRef(playlists),
    playlistMemberships: readonlyRecordRef(playlistMemberships),
    activePlaylistId: readonlyRef(activePlaylistId),
    activePlaylistDetail: readonlyRef(activePlaylistDetail),
    refreshPlaylists,
    selectPlaylist,
    createPlaylistInWorkspace,
    deletePlaylistInWorkspace,
    addPlaylistItemsInWorkspace,
    reorderPlaylistItemsInWorkspace,
    removePlaylistItemInWorkspace,
    setPlaylistMembershipInWorkspace,
  };
}

export function useWorkspaceSmartFolders() {
  return {
    smartFolders: readonlyArrayRef(smartFolders),
    activeSmartFolderId: readonlyRef(activeSmartFolderId),
    smartFolderResult: readonlyRef(smartFolderResult),
    refreshSmartFolders,
    selectSmartFolder,
    createSmartFolderInWorkspace,
    updateSmartFolderInWorkspace,
    deleteSmartFolderInWorkspace,
  };
}

export function useWorkspaceActions() {
  return {
    repositoryActions: readonlyArrayRef(repositoryActions),
    activeRepositoryActionId: readonlyRef(activeRepositoryActionId),
    refreshRepositoryActions,
    selectRepositoryAction,
    runActiveRepositoryAction,
  };
}

export function useWorkspaceAssetMetadata() {
  return {
    saveAssetMetadata,
    undoAssetRevision,
    redoAssetRevision,
  };
}

export function useWorkspaceSettings() {
  return {
    plugins: readonlyArrayRef(plugins),
    pluginHookExecutions: readonlyArrayRef(pluginHookExecutions),
    cacheSnapshot: readonlyRef(cacheSnapshot),
    apiDesign: readonlyRef(apiDesign),
    externalApiConnection: readonlyRef(externalApiConnection),
    loadSettingsData,
    setPluginEnabledInWorkspace,
    deletePluginInWorkspace,
    installPluginArchiveInWorkspace,
    openPluginDataDirectoryInWorkspace,
    loadPluginConfigInWorkspace,
    setPluginConfigValueInWorkspace,
    deletePluginConfigValueInWorkspace,
  };
}

export function useWorkspaceProgress() {
  return {
    operationProgress: readonlyRef(operationProgress),
    workspaceStartup: readonlyRef(workspaceStartup),
    syncProgress: readonlyRef(syncProgress),
    isLoadingRepositories: readonlyRef(isLoadingRepositories),
    isLoadingSnapshot: readonlyRef(isLoadingSnapshot),
    isLoadingAssetDetail: readonlyRef(isLoadingAssetDetail),
    isLoadingFileBrowser: readonlyRef(isLoadingFileBrowser),
    isSearching: readonlyRef(isSearching),
    isLoadingSmartFolder: readonlyRef(isLoadingSmartFolder),
    isLoadingRepositoryActions: readonlyRef(isLoadingRepositoryActions),
    isSavingMetadata: readonlyRef(isSavingMetadata),
    isSyncing: readonlyRef(isSyncing),
    isMutatingFiles: readonlyRef(isMutatingFiles),
    isMutatingSmartFolder: readonlyRef(isMutatingSmartFolder),
    isRunningRepositoryAction: readonlyRef(isRunningRepositoryAction),
    isLoadingSettingsData: readonlyRef(isLoadingSettingsData),
    isManagingPlugins: readonlyRef(isManagingPlugins),
    isBusy: computed(() => (
      isLoadingRepositories.value ||
      isLoadingSnapshot.value ||
      isLoadingAssetDetail.value
    )),
    error: readonlyRef(error),
  };
}

export function useRepositoryWorkspace() {
  return {
    ...useWorkspaceRepository(),
    ...useWorkspaceNavigation(),
    ...useWorkspaceFiles(),
    ...useWorkspaceSelection(),
    ...useWorkspaceSearch(),
    ...useWorkspacePlaylists(),
    ...useWorkspaceSmartFolders(),
    ...useWorkspaceActions(),
    ...useWorkspaceAssetMetadata(),
    ...useWorkspaceSettings(),
    ...useWorkspaceProgress(),
  };
}
