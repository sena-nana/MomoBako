<script setup lang="ts">
import { convertFileSrc } from "@tauri-apps/api/core";
import { computed, nextTick, onUnmounted, ref, watch } from "vue";
import {
  AlertTriangle,
  GripVertical,
  Play,
  Trash2,
  X,
} from "lucide-vue-next";
import ConfirmDialog from "../components/ConfirmDialog.vue";
import WorkspacePlayerBar from "../components/WorkspacePlayerBar.vue";
import { useRepositoryWorkspace } from "../composables/useRepositoryWorkspace";
import { usePlaylistPlayer } from "../composables/usePlaylistPlayer";
import { getPreviewPluginForEntry } from "../plugins/previewPlugins";
import { getPlaylistPlayerByType } from "../plugins/playlistPlayers";
import { getRegisteredLibraryExtensionsForEntry, listRegisteredLibraryExtensions, type PlaylistPlayerObjectFit } from "../plugins/sdk";
import { getWorkspaceParentPath } from "./workspace/dragBehavior";
import { useWorkspacePreviewUi } from "./workspace/useWorkspacePreviewUi";
import {
  cancelWorkspaceComponentPreload,
  CopyTargetDialog,
  FileBrowserPanel,
  FilePreviewPane,
  HardlinkCandidateDialog,
  PluginManagerPanel,
  queueWorkspaceComponentPreload,
  RepositoryActionsPanel,
  SearchPanel,
  type WorkspacePreloadHandle,
} from "./workspace/lazyComponents";
import { useWorkspaceSearchUi } from "./workspace/useWorkspaceSearchUi";
import { useMissingRepositoryActions } from "./workspace/useMissingRepositoryActions";
import { useWorkspaceThumbnailActions } from "./workspace/useWorkspaceThumbnailActions";
import { useWorkspaceDragDrop } from "./workspace/useWorkspaceDragDrop";
import { useWorkspaceFileActions } from "./workspace/useWorkspaceFileActions";
import { useWorkspaceContextMenu } from "./workspace/useWorkspaceContextMenu";
import type {
  FileBrowserEntry,
  HardlinkCandidate,
  PlaylistItem,
  SearchHit,
} from "../types/repository";

type FileDisplayMode = "adaptive" | "masonry" | "grid" | "list";

const fileDisplayModeStorageKey = "momobako.fileDisplayMode";
const fileDisplayModeOptions: Array<{ value: FileDisplayMode; label: string }> = [
  { value: "adaptive", label: "自适应" },
  { value: "masonry", label: "瀑布流" },
  { value: "grid", label: "网格" },
  { value: "list", label: "列表" },
];
let preloadHandle: WorkspacePreloadHandle | null = null;
let hasQueuedWorkspacePreload = false;

function isFileDisplayMode(value: string | null): value is FileDisplayMode {
  return fileDisplayModeOptions.some((option) => option.value === value);
}

function readInitialFileDisplayMode(): FileDisplayMode {
  try {
    const savedMode = localStorage.getItem(fileDisplayModeStorageKey);
    return isFileDisplayMode(savedMode) ? savedMode : "adaptive";
  } catch {
    return "adaptive";
  }
}

const previewFilePath = ref<string | null>(null);
const fileDisplayMode = ref<FileDisplayMode>(readInitialFileDisplayMode());
const playlistDragItemId = ref<string | null>(null);

const {
  activePanel,
  activeAssetId,
  activePlaylistDetail,
  activePlaylistId,
  activePreviewPath,
  activeSnapshot,
  activeRepository,
  activeRepoId,
  currentDirectoryPath,
  dragHoverFolderPath,
  draggedWorkspacePaths,
  fileBrowser,
  repositories,
  filters,
  isFilterBarOpen,
  activeFilterCount,
  hasActiveFilters,
  breadcrumbSegments,
  directoryEntries,
  fileBrowserEntryMap,
  fileEntries,
  hasMultipleSelection,
  hasSplitFileGroups,
  selectedEntry,
  selectedEntries,
  searchQuery,
  selectedFilePathSet,
  selectedFilePaths,
  selectedFilePath,
  searchResults,
  smartFolderResult,
  repositoryActions,
  playlistMemberships,
  playlists,
  activeRepositoryActionId,
  hardlinkCandidates,
  isExternalDragActive,
  isInternalDragActive,
  isLoadingFileBrowser,
  isSearching,
  isSavingMetadata,
  isLoadingSmartFolder,
  isLoadingRepositoryActions,
  isMutatingFiles,
  isRunningRepositoryAction,
  error,
  refreshRepositoryWorkspace,
  selectRepository,
  selectAsset,
  loadFileBrowserForDirectory,
  createFileInWorkspace,
  copyWorkspaceEntries,
  moveWorkspaceEntries,
  attachRepository,
  removeRepository,
  relocateMissingRepository,
  clearDraggedWorkspaceState,
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
  removePlaylistItemInWorkspace,
  reorderPlaylistItemsInWorkspace,
  setPlaylistMembershipInWorkspace,
  startWorkspaceEntriesDrag,
  selectWorkspaceEntry,
  selectWorkspaceEntries,
  setActivePreviewPath,
  setDraggedWorkspacePaths,
  setActivePanel,
  setDragHoverFolderPath,
  setExternalDragActive,
  setInternalDragActive,
  setWorkspaceEntryThumbnail,
  setWorkspaceEntryThumbnailFromBytes,
  setWorkspaceEntryThumbnailFromUrl,
  clearWorkspaceEntryThumbnail,
  refreshWorkspaceEntryThumbnail,
  setFilterBarOpen,
  toggleFilterValue,
  setMinimumRatingFilter,
  updateFilters,
  clearFilters,
  runFilteredSearch,
  confirmWorkspaceHardlinkCandidate,
  saveAssetMetadata,
  selectRepositoryAction,
  runActiveRepositoryAction,
} = useRepositoryWorkspace();
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

const hasRepository = computed(() => Boolean(activeSnapshot.value));
const isMissingRepository = computed(() => activeRepository.value?.status === "missing");
const isFilesPanel = computed(() => activePanel.value === "files");
const isTrashPanel = computed(() => activePanel.value === "deleted");
const isSearchPanel = computed(() => activePanel.value === "search");
const isSmartFolderPanel = computed(() => activePanel.value === "smartFolder");
const isActionsPanel = computed(() => activePanel.value === "actions");
const isExtensionsPanel = computed(() => activePanel.value === "extensions");
const isPlaylistPanel = computed(() => activePanel.value === "playlist");
const isFileBrowserPanel = computed(() => isFilesPanel.value || isTrashPanel.value || isSmartFolderPanel.value);
const smartFolderEntryMap = computed<ReadonlyMap<string, FileBrowserEntry>>(() => (
  new Map((smartFolderResult.value?.results ?? []).map((entry) => [entry.path, entry]))
));
const playlistPreviewEntryMap = computed<ReadonlyMap<string, FileBrowserEntry>>(() => {
  const items = playlistPlayer.activePlaylist.value?.items ?? activePlaylistDetail.value?.items ?? [];
  return new Map(items.map((item) => [item.path, playlistItemToFileEntry(item)]));
});
const currentFileEntry = computed(() => {
  if (isSmartFolderPanel.value) {
    return selectedFilePath.value ? smartFolderEntryMap.value.get(selectedFilePath.value) ?? null : null;
  }
  return selectedEntry.value;
});

const isRepositoryWritable = computed(() => hasRepository.value && !isMissingRepository.value);
const canRenameSelected = computed(() => selectedEntries.value.length === 1 && isRepositoryWritable.value && !isTrashPanel.value && !isSmartFolderPanel.value);
const canOpenSelected = computed(() => selectedEntries.value.length === 1 && !isMissingRepository.value && !isTrashPanel.value);
const canDeleteSelected = computed(() => selectedEntries.value.length > 0 && isRepositoryWritable.value && !isSmartFolderPanel.value);
const canRestoreSelected = computed(() => selectedEntries.value.length > 0 && isRepositoryWritable.value && isTrashPanel.value);
const canDragEntries = computed(() => isRepositoryWritable.value && !isTrashPanel.value && !isSmartFolderPanel.value && fileBrowser.value?.backendKind === "filesystem");
const openSelectedLabel = computed(() => currentFileEntry.value?.kind === "directory" ? "进入" : "查看");
const previewFileEntry = computed(() => {
  if (!previewFilePath.value) return null;
  const activeEntryMap = isSmartFolderPanel.value ? smartFolderEntryMap.value : fileBrowserEntryMap.value;
  return activeEntryMap.get(previewFilePath.value)
    ?? fileBrowserEntryMap.value.get(previewFilePath.value)
    ?? smartFolderEntryMap.value.get(previewFilePath.value)
    ?? playlistPreviewEntryMap.value.get(previewFilePath.value)
    ?? null;
});
const previewPlugin = computed(() => getPreviewPluginForEntry(previewFileEntry.value));
const libraryExtensions = computed(() => listRegisteredLibraryExtensions());
const previewLibraryExtensions = computed(() => getRegisteredLibraryExtensionsForEntry(previewFileEntry.value));
const currentLibraryExtensions = computed(() => getRegisteredLibraryExtensionsForEntry(currentFileEntry.value));
const fileDisplayModeClass = computed(() => `files-list__files--${fileDisplayMode.value}`);
const activeDirectoryEntries = computed(() => (isSmartFolderPanel.value ? [] : directoryEntries.value));
const activeFileEntries = computed(() => (isSmartFolderPanel.value ? smartFolderResult.value?.results ?? [] : fileEntries.value));
const hasActiveSplitFileGroups = computed(() => (
  isSmartFolderPanel.value ? false : hasSplitFileGroups.value
));
const isActiveBrowserLoading = computed(() => (
  isSmartFolderPanel.value ? isLoadingSmartFolder.value : isLoadingFileBrowser.value
));
const smartFolderSummary = computed(() => {
  if (!smartFolderResult.value) return "";
  const filter = smartFolderResult.value.inheritedFilter;
  const parts = [
    filter.query ? `关键词 ${filter.query.replace(/\n/g, " + ")}` : "",
    filter.pathPrefix ? `路径 ${filter.pathPrefix.replace(/\n/g, " + ")}` : "",
    filter.formats?.length ? `格式 ${filter.formats.join(" / ")}` : "",
    filter.tags?.length ? `标签 ${filter.tags.join(" / ")}` : "",
    filter.colors?.length ? `颜色 ${filter.colors.join(" / ")}` : "",
    filter.shapes?.length ? `形状 ${filter.shapes.join(" / ")}` : "",
    filter.minRating ? `${filter.minRating} 星+` : "",
    filter.metadataFilters?.length ? `${filter.metadataFilters.length} 个元数据条件` : "",
  ].filter(Boolean);
  return `${smartFolderResult.value.results.length} 条结果${parts.length ? ` · ${parts.join(" · ")}` : ""}`;
});
const activePlaylistPlayer = computed(() => getPlaylistPlayerByType(activePlaylistDetail.value?.playlist.playerTypeId));
const workspacePlayerDefinition = computed(() => (
  getPlaylistPlayerByType(playlistPlayer.activePlaylist.value?.playlist.playerTypeId)
));
const showWorkspacePlayer = computed(() => Boolean(activeRepoId.value));
const playerQueueItems = computed<PlaylistItem[]>(() => (
  (playlistPlayer.activePlaylist.value?.items ?? []).map((item) => ({
    ...item,
    thumbnailPath: item.thumbnailPath ? convertFileSrc(item.thumbnailPath) : null,
  }))
));
const currentPlayerItem = computed<PlaylistItem | null>(() => {
  const currentId = playlistPlayer.currentItemId.value;
  return currentId
    ? playerQueueItems.value.find((item) => item.playlistItemId === currentId) ?? null
    : null;
});
const workspacePlayerBarProps = computed(() => ({
  item: currentPlayerItem.value,
  playerLabel: playlistPlayer.activePlaylist.value?.playlist.playerLabel,
  fileClass: workspacePlayerDefinition.value?.fileClass,
  supportsSeek: workspacePlayerDefinition.value?.supportsSeek ?? false,
  supportsVolume: workspacePlayerDefinition.value?.supportsVolume ?? false,
  canPlay: playlistPlayer.canPlay.value,
  mode: playlistPlayer.mode.value,
  currentTimeMs: playlistPlayer.currentTimeMs.value,
  durationMs: playlistPlayer.durationMs.value,
  volume: playlistPlayer.volume.value,
  imageDurationMs: playlistPlayer.playbackSettings.value.imageDurationMs,
  objectFit: playlistPlayer.playbackSettings.value.objectFit,
  isPlaying: playlistPlayer.isPlaying.value,
  errorMessage: playlistPlayer.errorMessage.value,
  queueOpen: playlistPlayer.queueOpen.value,
  queueItems: playerQueueItems.value,
  currentItemId: playlistPlayer.currentItemId.value,
}));
const workspacePlayerBarHandlers = {
  togglePlay: () => playlistPlayer.setPlaybackState({ isPlaying: !playlistPlayer.isPlaying.value }),
  previous: () => playlistPlayer.playPrevious(),
  next: () => playlistPlayer.playNext(false),
  cycleMode: cycleWorkspacePlayerMode,
  openQueue: () => playlistPlayer.setQueueOpen(!playlistPlayer.queueOpen.value),
  openPreview: openCurrentPlayerPreview,
  setVolume: (value: number) => playlistPlayer.setPlaybackState({ volume: value }),
  selectQueueItem: (playlistItemId: string) => playlistPlayer.playItem(playlistItemId, true),
  seek: (timeMs: number) => playlistPlayer.setPlaybackState({ currentTimeMs: timeMs }),
  setImageDuration: (imageDurationMs: number) => playlistPlayer.updatePlaybackSettings({ imageDurationMs }),
  setObjectFit: (objectFit: PlaylistPlayerObjectFit) => playlistPlayer.updatePlaybackSettings({ objectFit }),
};
const playlistStatusLabel = computed(() => {
  if (!activePlaylistDetail.value) return "";
  return `${activePlaylistDetail.value.playlist.playerLabel} · ${activePlaylistDetail.value.items.length} 项`;
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

const activeLibrarySearchShortcuts = computed(() => {
  const entries = fileBrowser.value?.entries ?? [];
  const results = searchResults.value;
  return libraryExtensions.value.flatMap((extension) => {
    if (!extension.searchShortcuts?.length) return [];
    const hasMatches = entries.some((entry) => extension.matchEntry(entry))
      || results.some((result) => extension.matchEntry(searchHitToFileEntry(result)));
    return hasMatches ? extension.searchShortcuts.map((shortcut) => ({ extension, shortcut })) : [];
  });
});

function searchHitToFileEntry(result: SearchHit): FileBrowserEntry {
  return {
    path: result.path,
    name: result.filename,
    kind: "file",
    assetId: result.assetId,
    status: result.status,
    tags: result.tags,
    metadata: result.metadata,
  };
}

function applyMetadataFilterShortcut(metadataFilters: string, sortField = "", sortDirection: "asc" | "desc" = "asc") {
  if (!isRepositoryWritable.value) return;
  metadataFiltersInput.value = metadataFilters;
  excludeTagsInput.value = "";
  excludeFormatsInput.value = "";
  limitInput.value = "";
  sortFieldInput.value = sortField;
  sortDirectionInput.value = sortDirection;
  updateFilters({
    metadataFilters,
    excludeQuery: excludeQueryInput.value.trim(),
    excludePathPrefixes: excludePathPrefixesInput.value.trim(),
    excludeTags: [],
    excludeFormats: [],
    excludeMetadataFilters: excludeMetadataFiltersInput.value.trim(),
    excludeNumberFilters: excludeNumberFiltersInput.value.trim(),
    excludeDateFilters: excludeDateFiltersInput.value.trim(),
    numberFilters: numberFiltersInput.value.trim(),
    dateFilters: dateFiltersInput.value.trim(),
    sortField,
    sortDirection,
    limit: null,
  });
  setActivePanel("search");
  void runFilteredSearch();
}

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
  playlistMenuItems: (entry) => compatiblePlaylistsForEntry(entry).map((playlist) => ({
    id: `playlist-${playlist.playlistId}`,
    label: playlist.name,
    checked: (playlistMemberships.value[entry.assetId ?? ""] ?? []).includes(playlist.playlistId),
    onSelect: () => togglePlaylistMembership(entry, playlist.playlistId),
  })),
  openCopyTargetDialog,
  openDirectory,
  openWorkspaceEntry,
  pasteCustomThumbnail,
  previewEntry: (entry) => {
    if (entry.kind === "file") {
      previewFilePath.value = entry.path;
    }
  },
  refreshEntryThumbnail,
  restoreContextSelection,
  revealWorkspaceEntry,
  selectWorkspaceEntries,
  startRenameEntry,
});

function hardlinkStateLabel(entry: FileBrowserEntry) {
  switch (entry.hardlinkState) {
    case "primary":
      return "主归属";
    case "linked":
      return "硬链接关联";
    case "copied":
    case "copiedFallback":
      return "普通复制";
    case "broken":
      return "关联异常";
    case "missing":
      return "关联缺失";
    default:
      return "";
  }
}

function hardlinkCandidateMessage(candidate: HardlinkCandidate) {
  return `${candidate.newPath} 与 ${candidate.existingPath} 内容哈希一致，大小 ${candidate.sizeLabel}。确认后会将新文件加入硬链接关联。`;
}

watch(fileDisplayMode, (mode) => {
  try {
    localStorage.setItem(fileDisplayModeStorageKey, mode);
  } catch {
    return;
  }
});

watch(
  () => isFileBrowserPanel.value,
  (enabled) => {
    if (enabled && activeRepoId.value && !fileBrowser.value) {
      void loadFileBrowserForDirectory("", isTrashPanel.value ? { specialLocation: "trash" } : { includeTree: true });
    }
  },
);

watch(selectedFilePath, (path) => {
  if (previewFilePath.value && previewFilePath.value !== path) {
    previewFilePath.value = null;
  }
});

watch(activePreviewPath, (path) => {
  previewFilePath.value = path;
});

watch(hasMultipleSelection, (multiple) => {
  if (multiple) {
    previewFilePath.value = null;
  }
});

function statusLabel(status: string) {
  switch (status) {
    case "synced":
      return "已同步";
    case "processing":
      return "处理中";
    case "indexed":
      return "已索引";
    case "deleted":
      return "已删除";
    case "ready":
      return "已同步";
    default:
      return status;
  }
}

function fileTone(entry: FileBrowserEntry) {
  if (entry.kind === "directory") {
    return "linear-gradient(135deg, #c7a566 0%, #73552f 100%)";
  }
  return "var(--thumbnail-placeholder-bg)";
}

function playlistItemThumbnailSrc(item: PlaylistItem) {
  if (!item.thumbnailPath) return null;
  return convertFileSrc(item.thumbnailPath);
}

function playlistItemToFileEntry(item: PlaylistItem): FileBrowserEntry {
  return {
    path: item.path,
    name: item.filename,
    kind: "file",
    extension: item.extension,
    assetId: item.assetId,
    status: item.status,
    thumbnailPath: item.thumbnailPath,
  };
}

function entryDeletedAtLabel(entry: FileBrowserEntry) {
  const value = entry.metadata?.deletedAt;
  if (typeof value !== "string" || !value) return null;
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString("zh-CN");
}

function entryModifiedAtLabel(entry: FileBrowserEntry) {
  return entry.modifiedAt ? new Date(entry.modifiedAt).toLocaleString("zh-CN") : "未记录";
}

function getParentPath(path: string) {
  return getWorkspaceParentPath(path);
}

function openDirectory(path: string) {
  setDragHoverFolderPath(null);
  void loadFileBrowserForDirectory(path, isTrashPanel.value ? { specialLocation: "trash" } : {});
}

function selectFileEntry(entry: FileBrowserEntry, mode: "replace" | "toggle" | "range") {
  selectWorkspaceEntry(entry.path, { mode });
}

async function saveFileMetadata(entry: FileBrowserEntry, metadata: Record<string, unknown>) {
  if (entry.kind !== "file" || !entry.assetId) return null;
  if (activeAssetId.value !== entry.assetId) {
    await selectAsset(entry.assetId);
  }
  return saveAssetMetadata(metadata);
}

function previewFileEntryByDoubleClick(entry: FileBrowserEntry) {
  if (entry.kind !== "file" || isTrashPanel.value) return;
  selectWorkspaceEntries([entry.path], { primaryPath: entry.path, anchorPath: entry.path });
  setActivePreviewPath(entry.path);
  previewFilePath.value = entry.path;
}

function exitPreview() {
  previewFilePath.value = null;
  setActivePreviewPath(null);
}

function compatiblePlaylistsForEntry(entry: FileBrowserEntry) {
  if (entry.kind !== "file" || !entry.assetId) return [];
  const extension = (entry.extension ?? "").toLowerCase();
  return playlists.value.filter((playlist) => {
    const player = getPlaylistPlayerByType(playlist.playerTypeId);
    return Boolean(player?.supportedExtensions.includes(extension));
  });
}

async function togglePlaylistMembership(entry: FileBrowserEntry, playlistId: string) {
  if (!entry.assetId) return;
  const currentMemberships = playlistMemberships.value[entry.assetId] ?? [];
  const nextMemberships = currentMemberships.includes(playlistId)
    ? currentMemberships.filter((item) => item !== playlistId)
    : [...currentMemberships, playlistId];
  await setPlaylistMembershipInWorkspace(entry.assetId, nextMemberships);
}

function openPlaylistItemPreview(item: PlaylistItem) {
  previewFilePath.value = item.path;
  setActivePreviewPath(item.path);
  selectWorkspaceEntry(item.path);
  setActivePanel("files");
}

function openCurrentPlayerPreview() {
  const item = playlistPlayer.currentItem.value;
  if (!item) return;
  previewFilePath.value = item.path;
  setActivePreviewPath(item.path);
  selectWorkspaceEntry(item.path);
  setActivePanel("files");
}

function cycleWorkspacePlayerMode() {
  const nextMode = playlistPlayer.mode.value === "listLoop"
    ? "shuffle"
    : playlistPlayer.mode.value === "shuffle"
      ? "singleLoop"
      : "listLoop";
  void playlistPlayer.setPlaybackState({ mode: nextMode });
}

async function playPlaylistFromItem(item?: PlaylistItem | null) {
  if (!activeRepoId.value || !activePlaylistDetail.value) return;
  const startItemId = item?.playlistItemId
    ?? activePlaylistDetail.value.items.find((entry) => entry.status === "ready")?.playlistItemId
    ?? activePlaylistDetail.value.items[0]?.playlistItemId
    ?? null;
  await playlistPlayer.setActivePlaylist(activeRepoId.value, activePlaylistDetail.value, startItemId, { autoPlay: true });
}

async function removePlaylistItem(item: PlaylistItem) {
  if (!activePlaylistId.value) return;
  await removePlaylistItemInWorkspace(activePlaylistId.value, item.playlistItemId);
}

function handlePlaylistDragStart(item: PlaylistItem) {
  playlistDragItemId.value = item.playlistItemId;
}

async function handlePlaylistDrop(item: PlaylistItem) {
  if (!activePlaylistId.value || !activePlaylistDetail.value || !playlistDragItemId.value) return;
  const sourceId = playlistDragItemId.value;
  if (sourceId === item.playlistItemId) {
    playlistDragItemId.value = null;
    return;
  }
  const items = [...activePlaylistDetail.value.items];
  const sourceIndex = items.findIndex((entry) => entry.playlistItemId === sourceId);
  const targetIndex = items.findIndex((entry) => entry.playlistItemId === item.playlistItemId);
  if (sourceIndex < 0 || targetIndex < 0) {
    playlistDragItemId.value = null;
    return;
  }
  const [moved] = items.splice(sourceIndex, 1);
  items.splice(targetIndex, 0, moved);
  playlistDragItemId.value = null;
  await reorderPlaylistItemsInWorkspace(activePlaylistId.value, items.map((entry) => entry.playlistItemId));
}

async function openSearchHit(result: SearchHit) {
  previewFilePath.value = null;
  setActivePanel("files");

  if (activeRepoId.value !== result.repoId) {
    await selectRepository(result.repoId);
  }
  if (activeRepoId.value !== result.repoId) return;

  const snapshot = await loadFileBrowserForDirectory(getParentPath(result.path), { includeTree: true });
  const matchedEntry = snapshot?.entries.find((entry) => entry.path === result.path);
  if (matchedEntry) {
    selectWorkspaceEntries([matchedEntry.path], { primaryPath: matchedEntry.path, anchorPath: matchedEntry.path });
    if (matchedEntry.kind === "file") {
      await nextTick();
      previewFilePath.value = matchedEntry.path;
    }
  }

  await selectAsset(result.assetId);
}

function closeFilterBar() {
  setFilterBarOpen(false);
}

function queueWorkspacePreload() {
  if (hasQueuedWorkspacePreload) return;
  hasQueuedWorkspacePreload = true;
  preloadHandle = queueWorkspaceComponentPreload(activePanel.value, preloadHandle);
}

function cancelWorkspacePreload() {
  cancelWorkspaceComponentPreload(preloadHandle);
  preloadHandle = null;
}

watch(hasRepository, (ready) => {
  if (ready) {
    queueWorkspacePreload();
  }
}, { immediate: true });

onUnmounted(cancelWorkspacePreload);
</script>

<template>
  <div class="workspace-page">
  <div v-if="hasRepository && isFilterBarOpen" class="workspace-filter-bar" aria-label="资源筛选">
    <div class="workspace-filter-bar__head">
      <div>
        <p class="asset-browser__eyebrow">当前资源库筛选</p>
        <strong>{{ activeSnapshot?.repository.name }}</strong>
      </div>
      <div class="workspace-filter-bar__actions">
        <span v-if="activeFilterCount" class="asset-stat">{{ activeFilterCount }} 个条件</span>
        <button type="button" class="ghost workspace-filter-bar__btn" :disabled="!hasActiveFilters && !searchQuery.trim()" @click="clearSearchFilters">
          清除
        </button>
        <button type="button" class="ghost workspace-filter-bar__btn" aria-label="关闭筛选栏" @click="closeFilterBar">
          <X :size="14" aria-hidden="true" />
        </button>
      </div>
    </div>

    <div class="workspace-filter-bar__groups">
      <section v-if="formatFilterOptions.length" class="workspace-filter-bar__group" aria-label="格式筛选">
        <span>格式</span>
        <div class="workspace-filter-bar__options">
          <button
            v-for="format in formatFilterOptions"
            :key="format"
            type="button"
            class="workspace-filter-chip"
            :class="{ 'is-active': filters.formats.includes(format) }"
            @click="toggleSearchFilter('formats', format)"
          >
            {{ format }}
          </button>
        </div>
      </section>

      <section v-if="tagFilterOptions.length" class="workspace-filter-bar__group" aria-label="文件标签筛选">
        <span>标签</span>
        <div class="workspace-filter-bar__options">
          <button
            v-for="tag in tagFilterOptions"
            :key="tag"
            type="button"
            class="workspace-filter-chip"
            :class="{ 'is-active': filters.tags.includes(tag) }"
            @click="toggleSearchFilter('tags', tag)"
          >
            {{ tag }}
          </button>
        </div>
      </section>

      <section class="workspace-filter-bar__group" aria-label="文件颜色筛选">
        <span>颜色</span>
        <div class="workspace-filter-bar__options">
          <button
            v-for="color in colorFilterOptions"
            :key="color"
            type="button"
            class="workspace-filter-chip"
            :class="{ 'is-active': filters.colors.includes(color) }"
            :style="filterColorStyle(color)"
            @click="toggleSearchFilter('colors', color)"
          >
            <i class="workspace-filter-chip__swatch" aria-hidden="true"></i>
            {{ color }}
          </button>
          <label class="workspace-filter-input">
            <input
              v-model="colorFilterInput"
              type="text"
              aria-label="输入文件颜色"
              placeholder="输入颜色"
              @keydown.enter.prevent="submitMetadataFilterInput('colors')"
            />
            <button type="button" :disabled="!colorFilterInput.trim()" @click="submitMetadataFilterInput('colors')">
              添加
            </button>
          </label>
        </div>
      </section>

      <section class="workspace-filter-bar__group" aria-label="形状筛选">
        <span>形状</span>
        <div class="workspace-filter-bar__options">
          <button
            v-for="shape in shapeFilterOptions"
            :key="shape"
            type="button"
            class="workspace-filter-chip"
            :class="{ 'is-active': filters.shapes.includes(shape) }"
            @click="toggleSearchFilter('shapes', shape)"
          >
            {{ shape }}
          </button>
          <label class="workspace-filter-input">
            <input
              v-model="shapeFilterInput"
              type="text"
              aria-label="输入形状"
              placeholder="输入形状"
              @keydown.enter.prevent="submitMetadataFilterInput('shapes')"
            />
            <button type="button" :disabled="!shapeFilterInput.trim()" @click="submitMetadataFilterInput('shapes')">
              添加
            </button>
          </label>
        </div>
      </section>

      <section class="workspace-filter-bar__group" aria-label="评分筛选">
        <span>评分</span>
        <div class="workspace-filter-bar__options">
          <button
            type="button"
            class="workspace-filter-chip"
            :class="{ 'is-active': filters.minRating == null }"
            @click="selectMinimumRating(null)"
          >
            全部
          </button>
          <button
            v-for="rating in ratingFilterOptions"
            :key="rating"
            type="button"
            class="workspace-filter-chip"
            :class="{ 'is-active': filters.minRating === rating }"
            @click="selectMinimumRating(rating)"
          >
            {{ rating }} 星+
          </button>
        </div>
      </section>

      <section v-if="activeLibrarySearchShortcuts.length" class="workspace-filter-bar__group" aria-label="库类型筛选">
        <span>库类型</span>
        <div class="workspace-filter-bar__options">
          <button
            v-for="{ extension, shortcut } in activeLibrarySearchShortcuts"
            :key="`${extension.pluginId}:${shortcut.id}`"
            type="button"
            class="workspace-filter-chip"
            @click="applyMetadataFilterShortcut(shortcut.metadataFilters, shortcut.sort?.field ?? '', shortcut.sort?.direction === 'desc' ? 'desc' : 'asc')"
          >
            {{ shortcut.label }}
          </button>
        </div>
      </section>

      <section class="workspace-filter-bar__group workspace-filter-bar__group--wide" aria-label="高级筛选">
        <span>高级</span>
        <div class="workspace-filter-bar__advanced">
          <label class="workspace-filter-input">
            <input
              v-model="excludeQueryInput"
              type="text"
              aria-label="排除关键词"
              placeholder="排除关键词"
              @keydown.enter.prevent="applyAdvancedSearchFilters"
            />
          </label>
          <label class="workspace-filter-input">
            <input
              v-model="excludePathPrefixesInput"
              type="text"
              aria-label="排除路径"
              placeholder="排除路径"
              @keydown.enter.prevent="applyAdvancedSearchFilters"
            />
          </label>
          <label class="workspace-filter-input">
            <input
              v-model="excludeTagsInput"
              type="text"
              aria-label="排除标签"
              placeholder="排除标签"
              @keydown.enter.prevent="applyAdvancedSearchFilters"
            />
          </label>
          <label class="workspace-filter-input">
            <input
              v-model="excludeFormatsInput"
              type="text"
              aria-label="排除格式"
              placeholder="排除格式"
              @keydown.enter.prevent="applyAdvancedSearchFilters"
            />
          </label>
          <label class="workspace-filter-input workspace-filter-input--wide">
            <input
              v-model="metadataFiltersInput"
              type="text"
              aria-label="元数据"
              placeholder="libraryKind=audio"
              @keydown.enter.prevent="applyAdvancedSearchFilters"
            />
          </label>
          <label class="workspace-filter-input workspace-filter-input--wide">
            <input
              v-model="excludeMetadataFiltersInput"
              type="text"
              aria-label="排除元数据"
              placeholder="status=archived"
              @keydown.enter.prevent="applyAdvancedSearchFilters"
            />
          </label>
          <label class="workspace-filter-input workspace-filter-input--wide">
            <input
              v-model="excludeNumberFiltersInput"
              type="text"
              aria-label="排除数值范围"
              placeholder="排除 width=0..640"
              @keydown.enter.prevent="applyAdvancedSearchFilters"
            />
          </label>
          <label class="workspace-filter-input workspace-filter-input--wide">
            <input
              v-model="excludeDateFiltersInput"
              type="text"
              aria-label="排除日期范围"
              placeholder="排除 fileCreatedAt=2024-01-01T00:00:00Z.."
              @keydown.enter.prevent="applyAdvancedSearchFilters"
            />
          </label>
          <label class="workspace-filter-input workspace-filter-input--wide">
            <input
              v-model="numberFiltersInput"
              type="text"
              aria-label="数值范围"
              placeholder="width=1024..4096"
              @keydown.enter.prevent="applyAdvancedSearchFilters"
            />
          </label>
          <label class="workspace-filter-input workspace-filter-input--wide">
            <input
              v-model="dateFiltersInput"
              type="text"
              aria-label="日期范围"
              placeholder="fileCreatedAt=2024-01-01T00:00:00Z.."
              @keydown.enter.prevent="applyAdvancedSearchFilters"
            />
          </label>
          <label class="workspace-filter-input">
            <input
              v-model="sortFieldInput"
              type="text"
              aria-label="排序字段"
              placeholder="排序字段"
              @keydown.enter.prevent="applyAdvancedSearchFilters"
            />
          </label>
          <label class="workspace-filter-input workspace-filter-input--select">
            <select v-model="sortDirectionInput" aria-label="排序方向">
              <option value="asc">升序</option>
              <option value="desc">降序</option>
            </select>
          </label>
          <label class="workspace-filter-input">
            <input
              v-model="limitInput"
              type="number"
              min="1"
              step="1"
              aria-label="结果数量"
              placeholder="数量"
              @keydown.enter.prevent="applyAdvancedSearchFilters"
            />
          </label>
          <button type="button" class="ghost workspace-filter-bar__btn" @click="applyAdvancedSearchFilters">
            应用
          </button>
        </div>
      </section>
    </div>
  </div>

  <div
    class="workspace-page__body"
    :class="{ 'workspace-page__body--fixed': hasRepository && isFileBrowserPanel }"
  >
  <section v-if="isMissingRepository" class="missing-repository-page" aria-live="polite">
    <div class="missing-repository-page__panel">
      <div class="missing-repository-page__icon" aria-hidden="true">
        <AlertTriangle :size="22" />
      </div>
      <p class="asset-browser__eyebrow">资源库丢失</p>
      <h1>{{ activeRepository?.name ?? "资源库不可用" }}</h1>
      <p class="missing-repository-page__summary">
        MomoBako 找不到这个资源库的本地文件夹。可以重定向到原资源库位置，或移除这条注册记录和本机缓存。
      </p>
      <p class="missing-repository-page__path">
        {{ activeRepository?.path }}
      </p>
      <p v-if="missingRepositoryError" class="missing-repository-page__error">
        {{ missingRepositoryError }}
      </p>
      <div class="missing-repository-page__actions">
        <button
          type="button"
          class="primary"
          :disabled="isMissingRepositoryBusy"
          @click="chooseMissingRepositoryPath"
        >
          {{ isRepairingMissingRepository ? "重定向中..." : "重定向" }}
        </button>
        <button
          type="button"
          class="ghost"
          :disabled="isMissingRepositoryBusy"
          @click="refreshMissingRepository"
        >
          刷新
        </button>
        <button
          type="button"
          class="ghost danger"
          :disabled="isMissingRepositoryBusy"
          @click="openMissingRepositoryDeleteDialog"
        >
          {{ isDeletingMissingRepository ? "删除中..." : "删除资源库" }}
        </button>
      </div>
    </div>
  </section>

  <section v-else-if="hasRepository && isFileBrowserPanel" :class="previewFileEntry ? 'files-preview-page' : 'files-workbench'">
    <template v-if="previewFileEntry">
      <FilePreviewPane
        :entry="previewFileEntry"
        :plugin="previewPlugin"
        :repo-id="activeRepoId ?? ''"
        :thumbnail-src="thumbnailSrc"
        :is-video-entry="isVideoEntry"
        :is-audio-entry="isAudioEntry"
        :hardlink-state-label="hardlinkStateLabel"
        :is-saving-metadata="isSavingMetadata"
        :available-tags="tagFilterOptions"
        :tag-groups="activeSnapshot?.tagGroups ?? []"
        :playlist-entries="activeFileEntries"
        :library-extensions="previewLibraryExtensions"
        :thumbnail-palette="thumbnailPaletteColors"
        :save-metadata="saveFileMetadata"
        :save-cover-thumbnail="setWorkspaceEntryThumbnailFromUrl"
        :status-label="statusLabel"
        @back="exitPreview"
        @open="openWorkspaceEntry"
        @reveal="revealWorkspaceEntry"
        @preview="previewFileEntryByDoubleClick"
        @thumbnail-loaded="updateThumbnailAspectRatio"
        @thumbnail-error="markThumbnailFailed"
      />
      <WorkspacePlayerBar
        v-if="showWorkspacePlayer"
        v-bind="workspacePlayerBarProps"
        v-on="workspacePlayerBarHandlers"
      />
    </template>

    <template v-else>
      <FileBrowserPanel
        v-model:create-file-name="createFileName"
        v-model:file-display-mode="fileDisplayMode"
        v-model:rename-value="renameValue"
        :breadcrumbs="breadcrumbSegments"
        :can-drag-entries="canDragEntries"
        :can-delete-selected="canDeleteSelected"
        :can-open-selected="canOpenSelected"
        :can-rename-selected="canRenameSelected"
        :can-restore-selected="canRestoreSelected"
        :current-file-entry="currentFileEntry"
        :directory-entries="activeDirectoryEntries"
        :display-mode-class="fileDisplayModeClass"
        :display-mode-options="fileDisplayModeOptions"
        :drop-target-path="dragHoverFolderPath"
        :entry-deleted-at-label="entryDeletedAtLabel"
        :entry-modified-at-label="entryModifiedAtLabel"
        :error="error"
        :file-entries="activeFileEntries"
        :file-entry-context-menu="fileEntryContextMenu"
        :file-item-style="fileItemStyle"
        :file-tone="fileTone"
        :hardlink-state-label="hardlinkStateLabel"
        :has-split-file-groups="hasActiveSplitFileGroups"
        :is-audio-entry="isAudioEntry"
        :is-drag-active="isExternalDragActive || isInternalDragActive"
        :is-dragging-files="isDraggingFiles"
        :is-loading-file-browser="isActiveBrowserLoading"
        :is-model-entry="isModelEntry"
        :is-mutating-files="isMutatingFiles"
        :is-read-only-virtual="isSmartFolderPanel"
        :is-trash-panel="isTrashPanel"
        :is-video-entry="isVideoEntry"
        :open-selected-label="openSelectedLabel"
        :rename-target-path="renameTargetPath"
        :is-saving-metadata="isSavingMetadata"
        :available-tags="tagFilterOptions"
        :tag-groups="activeSnapshot?.tagGroups ?? []"
        :library-extensions="currentLibraryExtensions"
        :thumbnail-palette="thumbnailPaletteColors"
        :save-metadata="saveFileMetadata"
        :selected-entries="selectedEntries"
        :selected-file-paths="selectedFilePaths"
        :selected-file-path="selectedFilePath"
        :status-label="statusLabel"
        :thumbnail-src="thumbnailSrc"
        :virtual-subline="smartFolderSummary"
        :virtual-title="smartFolderResult?.smartFolder.name"
        @create-file="handleCreateFile"
        @delete-selected="deleteSelectedEntry"
        @drag-leave="handleDragLeave"
        @drag-over="handleDragOver"
        @drop="handleDrop"
        @empty-trash="handleEmptyTrash"
        @entry-drag-end="handleEntryDragEnd"
        @entry-drag-move="handleEntryDragMove"
        @entry-drag-start="handleEntryDragStart"
        @hover-folder="handleFolderDropHover"
        @mark-thumbnail-failed="markThumbnailFailed"
        @leave-folder="handleFolderDropLeave"
        @drop-on-folder="handleFolderDrop"
        @open-directory="openDirectory"
        @open-selected="openSelectedEntry"
        @preview-file="previewFileEntryByDoubleClick"
        @restore-all-trash="handleRestoreAllTrash"
        @restore-selected="restoreSelectedEntry"
        @reveal-selected="revealSelectedEntry"
        @select-entry="selectFileEntry"
        @select-entries="handleBoxSelection"
        @start-rename="startRenameSelected"
        @submit-rename="submitRenameSelected"
        @thumbnail-loaded="updateThumbnailAspectRatio"
      >
        <template #player>
          <WorkspacePlayerBar
            v-if="showWorkspacePlayer"
            v-bind="workspacePlayerBarProps"
            v-on="workspacePlayerBarHandlers"
          />
        </template>
      </FileBrowserPanel>
    </template>
  </section>

  <section v-else-if="hasRepository && isPlaylistPanel" class="playlist-page">
    <div v-if="activePlaylistDetail" class="playlist-page__panel">
      <header class="playlist-page__header">
        <div>
          <p class="asset-browser__eyebrow">播放集</p>
          <h1>{{ activePlaylistDetail.playlist.name }}</h1>
          <p class="playlist-page__subline">
            {{ playlistStatusLabel }}
            <template v-if="!activePlaylistPlayer"> · 缺少对应播放插件</template>
          </p>
        </div>
        <div class="playlist-page__actions">
          <button
            type="button"
            class="ghost files-toolbar__btn"
            :disabled="!activePlaylistPlayer"
            @click="playPlaylistFromItem()"
          >
            <Play :size="14" aria-hidden="true" />
            播放
          </button>
        </div>
      </header>

      <div v-if="!activePlaylistDetail.items.length" class="playlist-page__empty">
        <h2>播放集还是空的</h2>
        <p>在文件浏览区右键文件，使用“添加到播放集”把内容加入这里。</p>
      </div>

      <div v-else class="playlist-page__list" role="list" aria-label="播放集条目">
        <article
          v-for="item in activePlaylistDetail.items"
          :key="item.playlistItemId"
          class="playlist-page__item"
          :class="{ 'is-unavailable': item.status !== 'ready' }"
          role="listitem"
          draggable="true"
          @dragstart="handlePlaylistDragStart(item)"
          @dragover.prevent
          @drop.prevent="handlePlaylistDrop(item)"
          @dblclick="playPlaylistFromItem(item)"
        >
          <button type="button" class="playlist-page__drag" aria-label="拖动排序">
            <GripVertical :size="16" aria-hidden="true" />
          </button>
          <button type="button" class="playlist-page__preview" @click="openPlaylistItemPreview(item)">
            <img v-if="playlistItemThumbnailSrc(item)" :src="playlistItemThumbnailSrc(item) ?? undefined" alt="" />
            <span v-else>{{ item.extension.toUpperCase() }}</span>
          </button>
          <div class="playlist-page__meta">
            <button type="button" class="playlist-page__title" @click="openPlaylistItemPreview(item)">
              {{ item.filename }}
            </button>
            <p v-if="item.status !== 'ready'" class="playlist-page__status">
              {{ item.statusReason ?? item.status }}
            </p>
            <p v-else class="playlist-page__path">{{ item.path }}</p>
          </div>
          <div class="playlist-page__row-actions">
            <button
              type="button"
              class="ghost files-toolbar__btn"
              :disabled="item.status !== 'ready' || !activePlaylistPlayer"
              @click="playPlaylistFromItem(item)"
            >
              <Play :size="14" aria-hidden="true" />
              播放
            </button>
            <button
              type="button"
              class="ghost danger files-toolbar__btn"
              @click="removePlaylistItem(item)"
            >
              <Trash2 :size="14" aria-hidden="true" />
              移除
            </button>
          </div>
        </article>
      </div>

      <WorkspacePlayerBar
        v-if="showWorkspacePlayer"
        v-bind="workspacePlayerBarProps"
        v-on="workspacePlayerBarHandlers"
      />
    </div>

    <div v-else class="playlist-page__empty">
      <h2>选择一个播放集</h2>
      <p>在左侧播放集区选择要查看或播放的列表。</p>
    </div>
  </section>

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

  <section v-else-if="isExtensionsPanel" class="extensions-workbench">
    <PluginManagerPanel
      title="文件系统与插件"
      eyebrow="拓展能力"
      subline="这里集中展示当前插件和后端能力。"
      search-placeholder="筛选导入器、脚本或元数据拓展"
      empty-title="没有匹配的插件"
      empty-description="试试其他关键词，或从 .momoplug 安装新的插件。"
    />
  </section>

  <section
    v-else
    class="empty-state-page"
    :class="{ 'is-dragging': isDraggingRepositoryFolder }"
    @dragover="handleEmptyRepositoryDragOver"
    @dragleave="handleEmptyRepositoryDragLeave"
    @drop="handleEmptyRepositoryDrop"
  >
    <div class="empty-state-page__panel">
      <h1>还没有可用资源库</h1>
      <p>拖入一个本地文件夹创建资源库。</p>
      <p v-if="emptyRepositoryError" class="empty-state-page__error">
        {{ emptyRepositoryError }}
      </p>
    </div>
  </section>
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
