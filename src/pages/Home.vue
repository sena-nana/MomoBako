<script setup lang="ts">
import { computed } from "vue";
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
  playlistMemberships,
  playlists,
  setPlaylistMembershipInWorkspace,
});
const { fileEntryContextMenu } = useWorkspaceContextMenu({
  activeRepoId,
  hasMultipleSelection,
  isMutatingFiles,
  isSmartFolderPanel,
  isTrashPanel,
  selectedFilePathSet,
  selectedFilePaths,
  chooseCustomThumbnail,
  clearCustomThumbnail,
  deleteContextSelection,
  playlistMenuItems,
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
</template>
