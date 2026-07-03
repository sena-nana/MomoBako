import { computed, onBeforeUnmount, reactive, ref } from "vue";
import {
  useWorkspaceAssetMetadata,
  useWorkspaceActions,
  useWorkspaceFiles,
  useWorkspaceNavigation,
  useWorkspacePlaylists,
  useWorkspaceProgress,
  useWorkspaceRepository,
  useWorkspaceSearch,
  useWorkspaceSelection,
  useWorkspaceSmartFolders,
} from "../../../composables/useRepositoryWorkspace";
import { usePlaylistPlayer } from "../../../composables/usePlaylistPlayer";
import { usePreviewUi } from "../preview/usePreviewUi";
import { useSearchUi } from "../search/useSearchUi";
import { useMissingRepositoryActions } from "./useMissingRepositoryActions";
import { useWorkspaceThumbnailActions } from "../useWorkspaceThumbnailActions";
import { useWorkspaceDragDrop } from "../useWorkspaceDragDrop";
import { useFileActions } from "../files/useFileActions";
import { useWorkspaceContextMenu } from "../useWorkspaceContextMenu";
import { usePlayerUi, playlistItemThumbnailSrc, playlistItemToFileEntry } from "../playlists/usePlayerUi";
import { useWorkspaceViewState } from "../useWorkspaceViewState";
import { useFileInteraction } from "../files/useFileInteraction";
import { usePlaylistMembershipUi } from "../playlists/usePlaylistMembershipUi";
import { useWorkspaceFilterShortcuts } from "../useWorkspaceFilterShortcuts";
import { useWorkspaceComponentPreload } from "../useWorkspaceComponentPreload";
import { useWorkspaceFilesSurfaceViewModel } from "../files/useFilesSurfaceViewModel";
import { usePlaylistPageBindings } from "../playlists/usePlaylistPageBindings";
import { useEntryActionRepositoryDialog } from "../useEntryActionRepositoryDialog";
import { useWorkspaceNeteaseAuthViewModel } from "./useWorkspaceNeteaseAuthViewModel";
import { clearRecentAccessHistory } from "../../../services/repositoryApi";
import { clearActiveSnapshotRecentAccess } from "../../../composables/workspace/state";
import {
  entryDeletedAtLabel,
  entryModifiedAtLabel,
  fileTone,
  hardlinkCandidateMessage,
  hardlinkStateLabel,
  statusLabel,
} from "../files/filePresentation";
import type { EagleLibraryImportResponse, FileBrowserEntry, FileBrowserSnapshot } from "../../../types/repository";
import { loadThumbnailsForEntries } from "../../../composables/workspace/thumbnails";
import { emitPluginEvent, onPluginEvent } from "../../../plugins/sdk";

type WorkspaceImportRequest =
  | {
      requestId: string;
      action: "folder";
      repoId: string;
      parentPath: string;
      sourcePath: string;
    }
  | {
      requestId: string;
      action: "zip";
      repoId: string;
      parentPath: string;
      archivePath: string;
    }
  | {
      requestId: string;
      action: "eagle";
      repoId: string;
      parentPath: string;
      libraryPath: string;
      mode: "copy" | "move";
    };

type WorkspaceImportResponse =
  | {
      requestId: string;
      action: WorkspaceImportRequest["action"];
      status: "success";
      snapshot?: FileBrowserSnapshot;
      result?: EagleLibraryImportResponse;
    }
  | {
      requestId: string;
      action: WorkspaceImportRequest["action"];
      status: "error";
      message: string;
    };

export function useWorkspaceHomeViewModel() {
  const {
    activeAssetDetail,
    activeAssetId,
    activeSnapshot,
    activeRepository,
    activeRepoId,
    repositories,
    refreshActiveRepositoryWorkspaceSilently,
    refreshRepositoryWorkspace,
    selectRepository,
    selectAsset,
    attachRepository,
    configureNeteaseRepositoryCacheInWorkspace,
    removeRepository,
    relocateMissingRepository,
  } = useWorkspaceRepository();
  const {
    activePreviewPath,
    activeLibraryCategoryLabel: fileCategoryLabel,
    currentDirectoryPath,
    fileBrowser,
    breadcrumbSegments,
    directoryEntries,
    fileBrowserEntryMap,
    fileEntries,
    hasSplitFileGroups,
    hardlinkCandidates,
    isLibraryCategoryVirtualView,
    isLoadingFileBrowserMore,
    libraryCategorySummary,
    loadFileBrowserForDirectory,
    visibleEntries,
    createFileInWorkspace,
    copyWorkspaceEntries,
    moveWorkspaceEntries,
    deleteWorkspaceEntries,
    importArchiveEntriesToWorkspace,
    importEagleLibraryToWorkspace,
    importEntriesToWorkspace,
    renameWorkspaceEntry,
    deleteWorkspaceEntry,
    restoreTrashEntries,
    restoreTrashEntry,
    restoreAllTrashEntries,
    emptyTrash,
    openWorkspaceEntry,
    revealWorkspaceEntry,
    setActivePreviewPath,
    startWorkspaceEntriesDrag,
    setWorkspaceEntryThumbnail,
    setWorkspaceEntryThumbnailFromBytes,
    setWorkspaceEntryThumbnailFromUrl,
    clearWorkspaceEntryThumbnail,
    refreshWorkspaceEntryThumbnail,
    confirmWorkspaceHardlinkCandidate,
  } = useWorkspaceFiles();
  const {
    dragHoverFolderPath,
    draggedWorkspacePaths,
    hasMultipleSelection,
    selectedEntry,
    selectedEntries,
    selectedFilePathSet,
    selectedFilePaths,
    selectedFilePath,
    isExternalDragActive,
    isInternalDragActive,
    clearDraggedWorkspaceState,
    selectWorkspaceEntry,
    selectWorkspaceEntries,
    setDraggedWorkspacePaths,
    setDragHoverFolderPath,
    setExternalDragActive,
    setInternalDragActive,
  } = useWorkspaceSelection();
  const {
    filters,
    isFilterBarOpen,
    activeFilterCount,
    hasActiveFilters,
    searchQuery,
    searchResults,
    setFilterBarOpen,
    toggleFilterValue,
    setMinimumRatingFilter,
    updateFilters,
    clearFilters,
    runFilteredSearch,
  } = useWorkspaceSearch();
  const {
    activeLibraryCategory,
    setActiveLibraryCategory,
    activePanel,
    setActivePanel,
  } = useWorkspaceNavigation();
  const {
    activePlaylistDetail,
    activePlaylistId,
    addPlaylistItemsByPathsInWorkspace,
    playlistMemberships,
    playlists,
    removePlaylistItemInWorkspace,
    reorderPlaylistItemsInWorkspace,
    setPlaylistMembershipInWorkspace,
  } = useWorkspacePlaylists();
  const {
    smartFolderResult,
  } = useWorkspaceSmartFolders();
  const {
    repositoryActions,
    activeRepositoryActionId,
    selectRepositoryAction,
    runActiveRepositoryAction,
  } = useWorkspaceActions();
  const {
    isLoadingFileBrowser,
    isSearching,
    isSavingMetadata,
    isLoadingSmartFolder,
    isLoadingRepositoryActions,
    isMutatingFiles,
    isRunningRepositoryAction,
    error,
  } = useWorkspaceProgress();
  const {
    saveAssetMetadata,
  } = useWorkspaceAssetMetadata();
  const playlistPlayer = usePlaylistPlayer();

  const {
    fileItemStyle,
    isAudioEntry,
    isModelEntry,
    isVideoEntry,
    markThumbnailFailed,
    resetThumbnailFailure,
    thumbnailPaletteColors,
    thumbnailSrc,
    updateThumbnailAspectRatio,
  } = usePreviewUi();

  const playlistPreviewEntryMap = computed<ReadonlyMap<string, FileBrowserEntry>>(() => {
    const items = playlistPlayer.activePlaylist.value?.items ?? activePlaylistDetail.value?.items ?? [];
    return new Map(items.map((item) => [item.path, playlistItemToFileEntry(item)]));
  });
  const {
    activeDirectoryEntries,
    activeFileEntries,
    activeLibrarySearchShortcuts,
    canDeleteSelected,
    canDragEntries,
    canOpenSelected,
    canRenameSelected,
    canRestoreSelected,
    currentFileEntry,
    currentLibraryExtensions,
    fileDisplayMode,
    fileDisplayModeClass,
    fileDisplayModeOptions,
    hasActiveSplitFileGroups,
    hasRepository,
    isActionsPanel,
    isActiveBrowserLoading,
    isExtensionsPanel,
    isFileBrowserPanel,
    isFilesPanel,
    isMissingRepository,
    isPlaylistPanel,
    isReadOnlyVirtualView,
    isRepositoryWritable,
    isSearchPanel,
    isSmartFolderPanel,
    isTrashPanel,
    isVirtualView,
    openSelectedLabel,
    previewFileEntry,
    previewLibraryExtensions,
    previewPlugin,
    setPreviewFilePath,
    virtualViewSummary,
    virtualViewTitle,
  } = useWorkspaceViewState({
    activeAssetDetail,
    activeLibraryCategory,
    activeLibraryCategoryLabel: fileCategoryLabel,
    activePanel,
    activePlaylistDetail,
    activePreviewPath,
    activeRepositoryStatus: computed(() => activeRepository.value?.status),
    activeSnapshot,
    directoryEntries,
    fileBrowser,
    fileBrowserEntryMap,
    fileEntries,
    hasMultipleSelection,
    hasSplitFileGroups,
    isLibraryCategoryVirtualView,
    isLoadingFileBrowser,
    isLoadingSmartFolder,
    libraryCategorySummary,
    playlistPreviewEntryMap,
    searchResults,
    selectedEntries,
    selectedEntry,
    selectedFilePath,
    smartFolderResult,
  });
  const ratingFilterOptions = [1, 2, 3, 4, 5];
  const hasMoreEntries = computed(() => !isVirtualView.value && Boolean(fileBrowser.value?.hasMore));
  const isRecentView = computed(() => (
    activePanel.value === "files" && activeLibraryCategory.value === "recent"
  ));
  const canClearRecentHistory = computed(() => (
    isRecentView.value && Boolean(activeSnapshot.value?.assets.some((asset) => asset.lastAccessedAt))
  ));
  const currentDirectoryDisplayName = computed(() => {
    if (!currentDirectoryPath.value) {
      return activeSnapshot.value?.repository.name ?? "根目录";
    }
    const segments = currentDirectoryPath.value.split("/");
    return segments[segments.length - 1] ?? currentDirectoryPath.value;
  });
  const isClearingRecentHistory = ref(false);

  function updateVisibleThumbnailEntries(entries: FileBrowserEntry[]) {
    if (!fileBrowser.value || !entries.length) return;
    loadThumbnailsForEntries(
      fileBrowser.value.repoId,
      fileBrowser.value.currentPath,
      entries,
    );
  }

  async function loadMoreEntries() {
    if (isVirtualView.value || !fileBrowser.value?.hasMore || isLoadingFileBrowserMore.value) return;
    await loadFileBrowserForDirectory(fileBrowser.value.currentPath, {
      append: true,
      specialLocation: isTrashPanel.value ? "trash" : undefined,
    });
  }

  async function handleClearRecentHistory() {
    if (!activeRepoId.value || !canClearRecentHistory.value || isClearingRecentHistory.value) return;
    isClearingRecentHistory.value = true;
    try {
      const response = await clearRecentAccessHistory({ repoId: activeRepoId.value });
      clearActiveSnapshotRecentAccess(response.repoId);
    } finally {
      isClearingRecentHistory.value = false;
    }
  }
  const {
    colorFilterInput,
    shapeFilterInput,
    excludeQueryInput,
    excludePathPrefixesInput,
    excludeTagsInput,
    excludeFormatsInput,
    metadataFiltersInput,
    excludeMetadataFiltersInput,
    excludeNumberFiltersInput,
    excludeDateFiltersInput,
    numberFiltersInput,
    dateFiltersInput,
    sortFieldInput,
    sortDirectionInput,
    limitInput,
    tagFilterOptions,
    formatFilterOptions,
    colorFilterOptions,
    shapeFilterOptions,
    searchResultScopeLabel,
    searchSummary,
    toggleSearchFilter,
    submitMetadataFilterInput,
    selectMinimumRating,
    clearSearchFilters,
    applyAdvancedSearchFilters,
    searchResultContext,
    filterColorStyle,
  } = useSearchUi({
    activeSnapshot,
    fileBrowser,
    hasActiveFilters,
    isRepositoryWritable,
    searchQuery,
    searchResults,
    clearFilters,
    runFilteredSearch,
    setActivePanel,
    setMinimumRatingFilter,
    toggleFilterValue,
    updateFilters,
  });
  const {
    activePlaylistPlayer,
    handlePlaylistDragStart,
    handlePlaylistDrop,
    openPlaylistItemPreview,
    playlistStatusLabel,
    playPlaylistFromItem,
    removePlaylistItem,
    showWorkspacePlayer,
    workspacePlayerBarHandlers,
    workspacePlayerBarProps,
  } = usePlayerUi({
    activePlaylistDetail,
    activePlaylistId,
    activeRepoId,
    playlistPlayer,
    removePlaylistItemInWorkspace,
    reorderPlaylistItemsInWorkspace,
    selectWorkspaceEntry,
    setActiveLibraryCategory,
    setActivePanel,
    setActivePreviewPath,
    setPreviewFilePath,
  });

  const { applyMetadataFilterShortcut, closeFilterBar } = useWorkspaceFilterShortcuts({
    dateFiltersInput,
    excludeDateFiltersInput,
    excludeFormatsInput,
    excludeMetadataFiltersInput,
    excludeNumberFiltersInput,
    excludePathPrefixesInput,
    excludeQueryInput,
    excludeTagsInput,
    isRepositoryWritable,
    limitInput,
    metadataFiltersInput,
    numberFiltersInput,
    sortDirectionInput,
    sortFieldInput,
    runFilteredSearch,
    setActivePanel,
    setFilterBarOpen,
    updateFilters,
  });

  const {
    missingRepositoryError,
    isMissingRepositoryBusy,
    isRepairingMissingRepository,
    isDeletingMissingRepository,
    showMissingRepositoryDeleteDialog,
    chooseMissingRepositoryPath,
    refreshMissingRepository,
    openMissingRepositoryDeleteDialog,
    closeMissingRepositoryDeleteDialog,
    confirmMissingRepositoryDelete,
  } = useMissingRepositoryActions({
    activeRepoId,
    activeRepository,
    configureNeteaseRepositoryCache: configureNeteaseRepositoryCacheInWorkspace,
    refreshRepositoryWorkspaceSilently: refreshActiveRepositoryWorkspaceSilently,
    relocateMissingRepository,
    removeRepository,
  });
  const {
    chooseCustomThumbnail,
    pasteCustomThumbnail,
    clearCustomThumbnail,
    refreshEntryThumbnail,
  } = useWorkspaceThumbnailActions({
    isTrashPanel,
    clearWorkspaceEntryThumbnail,
    refreshWorkspaceEntryThumbnail,
    resetThumbnailFailure,
    setWorkspaceEntryThumbnail,
    setWorkspaceEntryThumbnailFromBytes,
  });
  const {
    emptyRepositoryError,
    isDraggingFiles,
    isDraggingRepositoryFolder,
    handleBoxSelection,
    handleDragLeave,
    handleDragOver,
    handleDrop,
    handleEmptyRepositoryDragLeave,
    handleEmptyRepositoryDragOver,
    handleEmptyRepositoryDrop,
    handleEntryDragEnd,
    handleEntryDragMove,
    handleEntryDragStart,
    handleFolderDrop,
    handleFolderDropHover,
    handleFolderDropLeave,
  } = useWorkspaceDragDrop({
    activeRepoId,
    activeSnapshot,
    canDragEntries,
    currentDirectoryPath,
    dragHoverFolderPath,
    draggedWorkspacePaths,
    hasRepository,
    isFilesPanel,
    isInternalDragActive,
    isMissingRepository,
    isRepositoryWritable,
    isTrashPanel,
    selectedFilePath,
    selectedFilePathSet,
    selectedFilePaths,
    attachRepository,
    clearDraggedWorkspaceState,
    importEntriesToWorkspace,
    moveWorkspaceEntries,
    selectWorkspaceEntries,
    setDragHoverFolderPath,
    setDraggedWorkspacePaths,
    setExternalDragActive,
    setInternalDragActive,
    startWorkspaceEntriesDrag,
  });
  const {
    exitPreview,
    openDirectory,
    openSearchHit,
    previewFileEntryByDoubleClick,
    saveFileMetadata,
    selectFileEntry,
  } = useFileInteraction({
    activeAssetId,
    activeRepoId,
    fileBrowser,
    isFileBrowserPanel,
    isTrashPanel,
    loadFileBrowserForDirectory,
    saveAssetMetadata,
    selectAsset,
    selectRepository,
    selectWorkspaceEntry,
    selectWorkspaceEntries,
    setActiveLibraryCategory,
    setActivePanel,
    setActivePreviewPath,
    setDragHoverFolderPath,
    setPreviewFilePath,
  });
  const {
    copyTargetDialogOpen,
    copyTargetPath,
    createFileName,
    currentHardlinkCandidate,
    renameTargetPath,
    renameValue,
    cancelRenameSelected,
    cancelCopyTarget,
    confirmCurrentHardlinkCandidate,
    deleteContextSelection,
    deleteSelectedEntry,
    handleCreateFile,
    handleEmptyTrash,
    handleImportEagleCopy,
    handleImportEagleMove,
    handleImportFolder,
    handleImportZip,
    handleRestoreAllTrash,
    openCopyTargetDialog,
    openSelectedEntry,
    restoreContextSelection,
    restoreSelectedEntry,
    revealSelectedEntry,
    skipCurrentHardlinkCandidate,
    startRenameEntry,
    startRenameSelected,
    submitCopyTarget,
    submitRenameSelected,
  } = useFileActions({
    currentFileEntry,
    fileBrowser,
    hasMultipleSelection,
    hardlinkCandidates,
    isTrashPanel,
    selectedFilePathSet,
    selectedFilePaths,
    confirmWorkspaceHardlinkCandidate,
    copyWorkspaceEntries,
    createFileInWorkspace,
    deleteWorkspaceEntries,
    deleteWorkspaceEntry,
    emptyTrash,
    importArchiveEntriesToWorkspace,
    importEagleLibraryToWorkspace,
    importEntriesToWorkspace,
    openDirectory,
    openWorkspaceEntry,
    renameWorkspaceEntry,
    restoreAllTrashEntries,
    restoreTrashEntries,
    restoreTrashEntry,
    revealWorkspaceEntry,
  });
  const { playlistMenuItems } = usePlaylistMembershipUi({
    addPlaylistItemsByPathsInWorkspace,
    playlistMemberships,
    playlists,
    setPlaylistMembershipInWorkspace,
  });

  const {
    entryActionRepositoryDialogCandidates,
    entryActionRepositoryDialogOpen,
    entryActionRepositoryDialogTitle,
    closeEntryActionRepositoryDialog,
    openEntryActionDialog,
  } = useEntryActionRepositoryDialog({
    activeRepoId,
    repositories,
  });

  const {
    activeNeteaseLoginExpired,
    isRefreshingNeteaseLogin,
    refreshActiveNeteaseLoginStatus,
    requestActiveNeteaseRelogin,
  } = useWorkspaceNeteaseAuthViewModel({
    activeRepoId,
    activeSnapshot,
    fileBrowser,
  });

  const { fileEntryContextMenu } = useWorkspaceContextMenu({
    activeRepoId,
    activeRepository,
    entryMap: fileBrowserEntryMap,
    hasMultipleSelection,
    isMutatingFiles,
    isSmartFolderPanel,
    isTrashPanel,
    selectedFilePathSet,
    selectedFilePaths,
    chooseCustomThumbnail,
    clearCustomThumbnail,
    deleteContextSelection,
    openEntryActionDialog,
    playlistMenuItems,
    refreshRepositoryWorkspace,
    openCopyTargetDialog,
    openDirectory,
    openWorkspaceEntry,
    pasteCustomThumbnail,
    previewEntry: (entry) => {
      if (entry.kind === "file") {
        setPreviewFilePath(entry.path);
      }
    },
    refreshEntryThumbnail,
    restoreContextSelection,
    revealWorkspaceEntry,
    selectWorkspaceEntries,
    startRenameEntry,
  });
  const { filesSurfaceHandlers, filesSurfaceProps } = useWorkspaceFilesSurfaceViewModel({
    activeDirectoryEntries,
    activeFileEntries,
    allEntries: visibleEntries,
    activeRepoId,
    activeSnapshotTagGroups: computed(() => activeSnapshot.value?.tagGroups ?? []),
    breadcrumbSegments,
    canDeleteSelected,
    canDragEntries,
    canOpenSelected,
    canRenameSelected,
    canRestoreSelected,
    canClearRecentHistory,
    currentFileEntry,
    currentDirectoryDisplayName,
    currentDirectoryPath,
    currentLibraryExtensions,
    dragHoverFolderPath,
    entryDeletedAtLabel,
    entryModifiedAtLabel,
    error,
    fileDisplayModeClass,
    fileDisplayModeOptions,
    fileEntryContextMenu,
    fileItemStyle,
    fileTone,
    hardlinkStateLabel,
    handleBoxSelection,
    handleCreateFile,
    handleClearRecentHistory,
    handleDragLeave,
    handleDragOver,
    handleDrop,
    handleEmptyTrash,
    handleEntryDragEnd,
    handleEntryDragMove,
    handleEntryDragStart,
    handleFolderDrop,
    handleFolderDropHover,
    handleFolderDropLeave,
    handleRestoreAllTrash,
    hasActiveSplitFileGroups,
    hasMoreEntries,
    isActiveBrowserLoading,
    isAudioEntry,
    isDragActive: computed(() => isExternalDragActive.value || isInternalDragActive.value),
    isDraggingFiles,
    isLoadingFileBrowserMore,
    isModelEntry,
    isMutatingFiles,
    isClearingRecentHistory,
    isRecentView,
    isRepositoryWritable,
    isSavingMetadata,
    isReadOnlyVirtualView,
    isTrashPanel,
    isVirtualView,
    isVideoEntry,
    loadMoreEntries,
    markThumbnailFailed,
    openDirectory,
    openSelectedEntry,
    openSelectedLabel,
    openWorkspaceEntry,
    previewFileEntry,
    previewFileEntryByDoubleClick,
    previewLibraryExtensions,
    previewPlugin,
    renameTargetPath,
    restoreSelectedEntry,
    revealSelectedEntry,
    revealWorkspaceEntry,
    saveCoverThumbnail: setWorkspaceEntryThumbnailFromUrl,
    saveMetadata: saveFileMetadata,
    selectFileEntry,
    selectedEntries,
    selectedFilePath,
    selectedFilePaths,
    showWorkspacePlayer,
    startRenameSelected,
    cancelRenameSelected,
    statusLabel,
    submitRenameSelected,
    tagFilterOptions,
    thumbnailPalette: thumbnailPaletteColors,
    thumbnailSrc,
    updateThumbnailAspectRatio,
    updateVisibleThumbnailEntries,
    virtualViewSummary,
    virtualViewTitle,
    workspacePlayerBarHandlers,
    workspacePlayerBarProps,
    deleteSelectedEntry,
    exitPreview,
    handleImportEagleCopy,
    handleImportEagleMove,
    handleImportFolder,
    handleImportZip,
  });
  const { playlistPageHandlers, playlistPageProps } = usePlaylistPageBindings({
    activePlaylistDetail,
    handlePlaylistDragStart,
    handlePlaylistDrop,
    hasPlayer: computed(() => Boolean(activePlaylistPlayer.value)),
    openPlaylistItemPreview,
    playPlaylistFromItem,
    playlistItemThumbnailSrc,
    playlistStatusLabel,
    removePlaylistItem,
    showWorkspacePlayer,
    workspacePlayerBarHandlers,
    workspacePlayerBarProps,
  });

  useWorkspaceComponentPreload({
    activePanel,
    hasRepository,
  });

  async function handleWorkspaceImportRequest(request: WorkspaceImportRequest): Promise<WorkspaceImportResponse> {
    if (!activeRepoId.value || request.repoId !== activeRepoId.value) {
      return {
        requestId: request.requestId,
        action: request.action,
        status: "error",
        message: "当前工具页目标仓库已变化，请刷新后重试。",
      };
    }
    if (!hasRepository.value) {
      return {
        requestId: request.requestId,
        action: request.action,
        status: "error",
        message: "当前没有可导入的资源库。",
      };
    }
    if (!isRepositoryWritable.value || isTrashPanel.value || isVirtualView.value) {
      return {
        requestId: request.requestId,
        action: request.action,
        status: "error",
        message: "当前视图不支持导入，请切换到可写目录后重试。",
      };
    }
    if (request.action === "folder") {
      const snapshot = await importEntriesToWorkspace([request.sourcePath], request.parentPath);
      return snapshot
        ? {
            requestId: request.requestId,
            action: request.action,
            status: "success",
            snapshot,
          }
        : {
            requestId: request.requestId,
            action: request.action,
            status: "error",
            message: error.value || "文件夹导入失败。",
          };
    }
    if (request.action === "zip") {
      const snapshot = await importArchiveEntriesToWorkspace(request.archivePath, request.parentPath);
      return snapshot
        ? {
            requestId: request.requestId,
            action: request.action,
            status: "success",
            snapshot,
          }
        : {
            requestId: request.requestId,
            action: request.action,
            status: "error",
            message: error.value || "ZIP 导入失败。",
          };
    }
    const result = await importEagleLibraryToWorkspace(request.libraryPath, request.mode, request.parentPath);
    return result
      ? {
          requestId: request.requestId,
          action: request.action,
          status: "success",
          result,
        }
      : {
          requestId: request.requestId,
          action: request.action,
          status: "error",
          message: error.value || "Eagle 导入失败。",
        };
  }

  const disposeWorkspaceImportListener = onPluginEvent<WorkspaceImportRequest>(
    "workspace:import-request",
    async (request) => {
      const response = await handleWorkspaceImportRequest(request);
      emitPluginEvent<WorkspaceImportResponse>("workspace:import-response", response);
    },
  );

  onBeforeUnmount(() => {
    disposeWorkspaceImportListener();
  });

  return reactive({
    activeFilterCount,
    activeRepoId,
    activeLibrarySearchShortcuts,
    activeNeteaseLoginExpired,
    activeRepository,
    activeRepositoryActionId,
    activeSnapshot,
    applyAdvancedSearchFilters,
    applyMetadataFilterShortcut,
    cancelCopyTarget,
    chooseMissingRepositoryPath,
    clearSearchFilters,
    closeEntryActionRepositoryDialog,
    closeFilterBar,
    closeMissingRepositoryDeleteDialog,
    colorFilterInput,
    colorFilterOptions,
    confirmCurrentHardlinkCandidate,
    confirmMissingRepositoryDelete,
    copyTargetDialogOpen,
    copyTargetPath,
    createFileName,
    currentHardlinkCandidate,
    dateFiltersInput,
    entryActionRepositoryDialogCandidates,
    entryActionRepositoryDialogOpen,
    entryActionRepositoryDialogTitle,
    emptyRepositoryError,
    excludeDateFiltersInput,
    excludeFormatsInput,
    excludeMetadataFiltersInput,
    excludeNumberFiltersInput,
    excludePathPrefixesInput,
    excludeQueryInput,
    excludeTagsInput,
    fileDisplayMode,
    filesSurfaceHandlers,
    filesSurfaceProps,
    filterColorStyle,
    filters,
    formatFilterOptions,
    handleEmptyRepositoryDragLeave,
    handleEmptyRepositoryDragOver,
    handleEmptyRepositoryDrop,
    hasActiveFilters,
    hasRepository,
    hardlinkCandidateMessage,
    currentDirectoryPath,
    isActionsPanel,
    isDeletingMissingRepository,
    isDraggingRepositoryFolder,
    isExtensionsPanel,
    isFileBrowserPanel,
    isFilterBarOpen,
    isLoadingRepositoryActions,
    isMissingRepository,
    isMissingRepositoryBusy,
    isMutatingFiles,
    isPlaylistPanel,
    isRepairingMissingRepository,
    isRepositoryWritable,
    isRefreshingNeteaseLogin,
    isRunningRepositoryAction,
    isSearching,
    isSearchPanel,
    isTrashPanel,
    isVirtualView,
    limitInput,
    metadataFiltersInput,
    missingRepositoryError,
    numberFiltersInput,
    openMissingRepositoryDeleteDialog,
    openSearchHit,
    playlistPageHandlers,
    playlistPageProps,
    refreshActiveNeteaseLoginStatus,
    refreshMissingRepository,
    renameValue,
    repositories,
    repositoryActions,
    requestActiveNeteaseRelogin,
    runActiveRepositoryAction,
    ratingFilterOptions,
    searchQuery,
    searchResultContext,
    searchResultScopeLabel,
    searchResults,
    searchSummary,
    selectMinimumRating,
    selectRepositoryAction,
    selectedFilePaths,
    shapeFilterInput,
    shapeFilterOptions,
    showMissingRepositoryDeleteDialog,
    sortDirectionInput,
    sortFieldInput,
    skipCurrentHardlinkCandidate,
    submitCopyTarget,
    submitMetadataFilterInput,
    tagFilterOptions,
    toggleSearchFilter,
  });
}
