<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { callPlugin } from "../services/repositoryApi";
import ConfirmDialog from "../components/ConfirmDialog.vue";
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
} from "../composables/useRepositoryWorkspace";
import { usePlaylistPlayer } from "../composables/usePlaylistPlayer";
import { useWorkspacePreviewUi } from "./workspace/useWorkspacePreviewUi";
import {
  CopyTargetDialog,
  ExtensionsPanel,
  HardlinkCandidateDialog,
  RepositoryActionsPanel,
  SearchPanel,
  WorkspaceFilterBar,
  WorkspaceFilesSurface,
  WorkspacePlaylistPage,
} from "./workspace/lazyComponents";
import { useWorkspaceSearchUi } from "./workspace/useWorkspaceSearchUi";
import { useMissingRepositoryActions } from "./workspace/useMissingRepositoryActions";
import { useWorkspaceThumbnailActions } from "./workspace/useWorkspaceThumbnailActions";
import { useWorkspaceDragDrop } from "./workspace/useWorkspaceDragDrop";
import { useWorkspaceFileActions } from "./workspace/useWorkspaceFileActions";
import { useWorkspaceContextMenu } from "./workspace/useWorkspaceContextMenu";
import { useWorkspacePlayerUi, playlistItemThumbnailSrc, playlistItemToFileEntry } from "./workspace/useWorkspacePlayerUi";
import { useWorkspaceViewState } from "./workspace/useWorkspaceViewState";
import { useWorkspaceFileInteraction } from "./workspace/useWorkspaceFileInteraction";
import { useWorkspacePlaylistMembershipUi } from "./workspace/useWorkspacePlaylistMembershipUi";
import { useWorkspaceFilterShortcuts } from "./workspace/useWorkspaceFilterShortcuts";
import { useWorkspaceComponentPreload } from "./workspace/useWorkspaceComponentPreload";
import { useWorkspaceFilesSurfaceBindings } from "./workspace/useWorkspaceFilesSurfaceBindings";
import { useWorkspacePlaylistPageBindings } from "./workspace/useWorkspacePlaylistPageBindings";
import {
  entryDeletedAtLabel,
  entryModifiedAtLabel,
  fileTone,
  hardlinkCandidateMessage,
  hardlinkStateLabel,
  statusLabel,
} from "./workspace/filePresentation";
import MissingRepositoryState from "./workspace/MissingRepositoryState.vue";
import EmptyRepositoryState from "./workspace/EmptyRepositoryState.vue";
import type { FileBrowserEntry } from "../types/repository";
import type { EntryActionDialogRequest, EntryActionDialogResultMap } from "../plugins/sdk";

const NETEASE_SOURCE_PLUGIN_ID = "momobako.source.netease-cloud-music";

const {
  activeAssetId,
  activeSnapshot,
  activeRepository,
  activeRepoId,
  repositories,
  refreshRepositoryWorkspace,
  selectRepository,
  selectAsset,
  attachRepository,
  removeRepository,
  relocateMissingRepository,
} = useWorkspaceRepository();
const {
  activePreviewPath,
  currentDirectoryPath,
  fileBrowser,
  breadcrumbSegments,
  directoryEntries,
  fileBrowserEntryMap,
  fileEntries,
  hasSplitFileGroups,
  hardlinkCandidates,
  loadFileBrowserForDirectory,
  createFileInWorkspace,
  copyWorkspaceEntries,
  moveWorkspaceEntries,
  deleteWorkspaceEntries,
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
} = useWorkspacePreviewUi();

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
  isRepositoryWritable,
  isSearchPanel,
  isSmartFolderPanel,
  isTrashPanel,
  openSelectedLabel,
  previewFileEntry,
  previewLibraryExtensions,
  previewPlugin,
  setPreviewFilePath,
  smartFolderSummary,
} = useWorkspaceViewState({
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
  isLoadingFileBrowser,
  isLoadingSmartFolder,
  playlistPreviewEntryMap,
  searchResults,
  selectedEntries,
  selectedEntry,
  selectedFilePath,
  smartFolderResult,
});
const ratingFilterOptions = [1, 2, 3, 4, 5];
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
} = useWorkspaceSearchUi({
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
} = useWorkspacePlayerUi({
  activePlaylistDetail,
  activePlaylistId,
  activeRepoId,
  playlistPlayer,
  removePlaylistItemInWorkspace,
  reorderPlaylistItemsInWorkspace,
  selectWorkspaceEntry,
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
  refreshRepositoryWorkspace,
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
} = useWorkspaceFileInteraction({
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
  cancelCopyTarget,
  confirmCurrentHardlinkCandidate,
  deleteContextSelection,
  deleteSelectedEntry,
  handleCreateFile,
  handleEmptyTrash,
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
} = useWorkspaceFileActions({
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
  openDirectory,
  openWorkspaceEntry,
  renameWorkspaceEntry,
  restoreAllTrashEntries,
  restoreTrashEntries,
  restoreTrashEntry,
  revealWorkspaceEntry,
});
const { playlistMenuItems } = useWorkspacePlaylistMembershipUi({
  addPlaylistItemsByPathsInWorkspace,
  playlistMemberships,
  playlists,
  setPlaylistMembershipInWorkspace,
});

const entryActionRepositoryDialogOpen = ref(false);
const entryActionRepositoryDialogTitle = ref("选择目标资源库");
const entryActionRepositoryCandidates = ref<typeof repositories.value>([]);
const entryActionRepositoryResolve = ref<((value: EntryActionDialogResultMap["repository"]) => void) | null>(null);
const neteaseLoginStatus = ref<{ loggedIn?: boolean; loginExpired?: boolean; error?: string | null } | null>(null);
const isRefreshingNeteaseLogin = ref(false);

const isActiveNeteaseRepository = computed(() => activeSnapshot.value?.repository.backend.pluginId === NETEASE_SOURCE_PLUGIN_ID);
const activeNeteaseSourceConfig = computed(() => {
  const payload = [
    ...(fileBrowser.value?.entries ?? []),
    ...(activeSnapshot.value?.assets ?? []),
  ].map((entry) => entry.sourcePayload).find((payload) => (
    payload?.provider === "netease-cloud-music" && typeof payload.accountCookie === "string"
  ));
  if (!payload) return null;
  return {
    cookie: payload.accountCookie,
    accountId: payload.accountId,
  };
});
const activeNeteaseLoginExpired = computed(() => {
  if (!isActiveNeteaseRepository.value) return false;
  if (neteaseLoginStatus.value?.loginExpired) return true;
  return (fileBrowser.value?.entries ?? []).some((entry) => (
    entry.sourcePayload?.loginExpired === true
    || entry.metadata?.loginExpired === true
  )) || (activeSnapshot.value?.assets ?? []).some((entry) => entry.sourcePayload?.loginExpired === true);
});

async function openEntryActionDialog<TKind extends keyof EntryActionDialogResultMap>(
  request: Extract<EntryActionDialogRequest, { kind: TKind }>,
): Promise<EntryActionDialogResultMap[TKind]> {
  const dialogRequest = request as EntryActionDialogRequest;
  if (dialogRequest.kind === "directory") {
    const selected = await openDialog({
      title: dialogRequest.title ?? "选择目录",
      directory: true,
      multiple: false,
      defaultPath: dialogRequest.defaultPath ?? undefined,
    });
    return (typeof selected === "string" && selected.trim() ? selected : null) as EntryActionDialogResultMap[TKind];
  }

  const candidates = repositories.value.filter((repo) => {
    if (dialogRequest.requireReady !== false && repo.status !== "ready") return false;
    if (dialogRequest.requireWritable && !repo.backend.capabilities.includes("write")) return false;
    if (dialogRequest.backendPluginIds?.length && !dialogRequest.backendPluginIds.includes(repo.backend.pluginId)) return false;
    if (dialogRequest.backendKinds?.length && !dialogRequest.backendKinds.includes(repo.backend.kind)) return false;
    if (activeRepoId.value && repo.repoId === activeRepoId.value) return false;
    return true;
  });
  if (!candidates.length) {
    return null as EntryActionDialogResultMap[TKind];
  }
  if (candidates.length === 1) {
    return candidates[0] as EntryActionDialogResultMap[TKind];
  }
  entryActionRepositoryDialogTitle.value = dialogRequest.title ?? "选择目标资源库";
  entryActionRepositoryCandidates.value = candidates;
  entryActionRepositoryDialogOpen.value = true;
  return await new Promise<EntryActionDialogResultMap[TKind]>((resolve) => {
    entryActionRepositoryResolve.value = resolve as (value: EntryActionDialogResultMap["repository"]) => void;
  });
}

function closeEntryActionRepositoryDialog(result: EntryActionDialogResultMap["repository"] = null) {
  entryActionRepositoryDialogOpen.value = false;
  entryActionRepositoryCandidates.value = [];
  const resolve = entryActionRepositoryResolve.value;
  entryActionRepositoryResolve.value = null;
  resolve?.(result);
}

async function refreshActiveNeteaseLoginStatus() {
  if (!isActiveNeteaseRepository.value) {
    neteaseLoginStatus.value = null;
    return;
  }
  const config = activeNeteaseSourceConfig.value;
  if (!config) {
    neteaseLoginStatus.value = null;
    return;
  }
  isRefreshingNeteaseLogin.value = true;
  try {
    const response = await callPlugin<{
      loggedIn?: boolean;
      loginExpired?: boolean;
      error?: string;
    }>({
      pluginId: NETEASE_SOURCE_PLUGIN_ID,
      method: "auth.getLoginStatus",
      payload: { config },
    });
    neteaseLoginStatus.value = response.payload ?? null;
  } catch (cause) {
    neteaseLoginStatus.value = {
      loggedIn: false,
      loginExpired: true,
      error: cause instanceof Error ? cause.message : String(cause),
    };
  } finally {
    isRefreshingNeteaseLogin.value = false;
  }
}

function requestActiveNeteaseRelogin() {
  if (!activeRepoId.value) return;
  window.dispatchEvent(new CustomEvent("momo:netease-relogin", {
    detail: {
      repoId: activeRepoId.value,
      accountId: activeNeteaseSourceConfig.value?.accountId,
    },
  }));
}

watch(
  () => [activeRepoId.value, activeSnapshot.value?.repository.backend.pluginId] as const,
  () => {
    void refreshActiveNeteaseLoginStatus();
  },
  { immediate: true },
);

const { fileEntryContextMenu } = useWorkspaceContextMenu({
  activeRepoId,
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
const { filesSurfaceHandlers, filesSurfaceProps } = useWorkspaceFilesSurfaceBindings({
  activeDirectoryEntries,
  activeFileEntries,
  activeRepoId,
  activeSnapshotTagGroups: computed(() => activeSnapshot.value?.tagGroups ?? []),
  breadcrumbSegments,
  canDeleteSelected,
  canDragEntries,
  canOpenSelected,
  canRenameSelected,
  canRestoreSelected,
  currentFileEntry,
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
  isActiveBrowserLoading,
  isAudioEntry,
  isDragActive: computed(() => isExternalDragActive.value || isInternalDragActive.value),
  isDraggingFiles,
  isModelEntry,
  isMutatingFiles,
  isSavingMetadata,
  isSmartFolderPanel,
  isTrashPanel,
  isVideoEntry,
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
  smartFolderResult,
  smartFolderSummary,
  startRenameSelected,
  statusLabel,
  submitRenameSelected,
  tagFilterOptions,
  thumbnailPalette: thumbnailPaletteColors,
  thumbnailSrc,
  updateThumbnailAspectRatio,
  workspacePlayerBarHandlers,
  workspacePlayerBarProps,
  deleteSelectedEntry,
  exitPreview,
});
const { playlistPageHandlers, playlistPageProps } = useWorkspacePlaylistPageBindings({
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
</script>

<template>
  <div class="workspace-page">
  <WorkspaceFilterBar
    v-if="hasRepository && isFilterBarOpen"
    v-model:color-filter-input="colorFilterInput"
    v-model:date-filters-input="dateFiltersInput"
    v-model:exclude-date-filters-input="excludeDateFiltersInput"
    v-model:exclude-formats-input="excludeFormatsInput"
    v-model:exclude-metadata-filters-input="excludeMetadataFiltersInput"
    v-model:exclude-number-filters-input="excludeNumberFiltersInput"
    v-model:exclude-path-prefixes-input="excludePathPrefixesInput"
    v-model:exclude-query-input="excludeQueryInput"
    v-model:exclude-tags-input="excludeTagsInput"
    v-model:limit-input="limitInput"
    v-model:metadata-filters-input="metadataFiltersInput"
    v-model:number-filters-input="numberFiltersInput"
    v-model:shape-filter-input="shapeFilterInput"
    v-model:sort-direction-input="sortDirectionInput"
    v-model:sort-field-input="sortFieldInput"
    :active-filter-count="activeFilterCount"
    :active-library-search-shortcuts="activeLibrarySearchShortcuts"
    :color-filter-options="colorFilterOptions"
    :filters="filters"
    :filter-color-style="filterColorStyle"
    :format-filter-options="formatFilterOptions"
    :has-active-filters="hasActiveFilters"
    :rating-filter-options="ratingFilterOptions"
    :repository-name="activeSnapshot?.repository.name"
    :search-query="searchQuery"
    :shape-filter-options="shapeFilterOptions"
    :tag-filter-options="tagFilterOptions"
    @apply-advanced-search-filters="applyAdvancedSearchFilters"
    @apply-metadata-filter-shortcut="applyMetadataFilterShortcut"
    @clear-search-filters="clearSearchFilters"
    @close="closeFilterBar"
    @select-minimum-rating="selectMinimumRating"
    @submit-metadata-filter-input="submitMetadataFilterInput"
    @toggle-search-filter="toggleSearchFilter"
  />

  <div
    class="workspace-page__body"
    :class="{ 'workspace-page__body--fixed': hasRepository && isFileBrowserPanel }"
  >
    <div
      v-if="activeNeteaseLoginExpired"
      class="asset-browser__state asset-browser__state--error workspace-page__notice"
    >
      <span>登录已失效，请重新登录后再刷新或播放。</span>
      <button type="button" class="ghost" :disabled="isRefreshingNeteaseLogin" @click="refreshActiveNeteaseLoginStatus">
        刷新状态
      </button>
      <button type="button" class="primary" :disabled="isRefreshingNeteaseLogin" @click="requestActiveNeteaseRelogin">
        重新登录
      </button>
    </div>

    <MissingRepositoryState
      v-if="isMissingRepository"
      :active-repository="activeRepository"
      :error="missingRepositoryError"
      :is-busy="isMissingRepositoryBusy"
      :is-deleting="isDeletingMissingRepository"
      :is-repairing="isRepairingMissingRepository"
      @choose-path="chooseMissingRepositoryPath"
      @delete-repository="openMissingRepositoryDeleteDialog"
      @refresh="refreshMissingRepository"
    />

    <WorkspaceFilesSurface
      v-else-if="hasRepository && isFileBrowserPanel"
      v-model:create-file-name="createFileName"
      v-model:file-display-mode="fileDisplayMode"
      v-model:rename-value="renameValue"
      v-bind="filesSurfaceProps"
      v-on="filesSurfaceHandlers"
    />

    <WorkspacePlaylistPage
      v-else-if="hasRepository && isPlaylistPanel"
      v-bind="playlistPageProps"
      v-on="playlistPageHandlers"
    />

    <SearchPanel
      v-else-if="isSearchPanel"
      :is-searching="isSearching"
      :repositories-count="repositories.length"
      :results="searchResults"
      :scope-label="searchResultScopeLabel"
      :summary="searchSummary"
      :result-context="searchResultContext"
      @open-result="openSearchHit"
    />

    <RepositoryActionsPanel
      v-else-if="isActionsPanel"
      :actions="repositoryActions"
      :active-action-id="activeRepositoryActionId"
      :selected-count="selectedFilePaths.length"
      :is-loading="isLoadingRepositoryActions"
      :is-running="isRunningRepositoryAction"
      @select="selectRepositoryAction"
      @run="runActiveRepositoryAction"
    />

    <ExtensionsPanel v-else-if="isExtensionsPanel" />

    <EmptyRepositoryState
      v-else
      :error="emptyRepositoryError"
      :is-dragging="isDraggingRepositoryFolder"
      @drag-over="handleEmptyRepositoryDragOver"
      @drag-leave="handleEmptyRepositoryDragLeave"
      @drop="handleEmptyRepositoryDrop"
    />
  </div>
  </div>

  <CopyTargetDialog
    v-if="copyTargetDialogOpen"
    v-model:target-path="copyTargetPath"
    :open="copyTargetDialogOpen"
    :is-mutating="isMutatingFiles"
    @cancel="cancelCopyTarget"
    @submit="submitCopyTarget"
  />

  <HardlinkCandidateDialog
    v-if="currentHardlinkCandidate"
    :candidate="currentHardlinkCandidate"
    :is-mutating="isMutatingFiles"
    :message="hardlinkCandidateMessage"
    @confirm="confirmCurrentHardlinkCandidate"
    @skip="skipCurrentHardlinkCandidate"
  />

  <ConfirmDialog
    :open="showMissingRepositoryDeleteDialog"
    title="删除丢失资源库"
    message="会移除这条资源库注册记录并清理本机缓存，不会删除原路径中的用户文件。"
    confirm-text="删除"
    cancel-text="取消"
    busy-text="删除中..."
    :busy="isDeletingMissingRepository"
    danger
    @confirm="confirmMissingRepositoryDelete"
    @cancel="closeMissingRepositoryDeleteDialog"
  />
  <Teleport to="body">
    <Transition name="modal">
      <div
        v-if="entryActionRepositoryDialogOpen"
        class="modal-overlay"
        role="dialog"
        aria-modal="true"
        :aria-label="entryActionRepositoryDialogTitle"
        @click.self="closeEntryActionRepositoryDialog()"
      >
        <div class="modal-card dialog-card repository-picker-dialog">
          <div class="dialog-card__header">
            <span>{{ entryActionRepositoryDialogTitle }}</span>
          </div>
          <div class="dialog-card__body repository-picker-dialog__body">
            <button
              v-for="repository in entryActionRepositoryCandidates"
              :key="repository.repoId"
              type="button"
              class="repository-picker-dialog__item"
              @click="closeEntryActionRepositoryDialog(repository)"
            >
              <strong>{{ repository.name }}</strong>
              <span>{{ repository.path }}</span>
            </button>
          </div>
          <div class="dialog-card__actions">
            <button type="button" class="ghost" @click="closeEntryActionRepositoryDialog()">
              取消
            </button>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>
