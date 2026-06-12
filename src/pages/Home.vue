<script setup lang="ts">
import { computed, defineAsyncComponent, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { convertFileSrc } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { UnlistenFn } from "@tauri-apps/api/event";
import {
  AlertTriangle,
  Eye,
  FileImage,
  Files,
  FolderOpen,
  GripVertical,
  PencilLine,
  ImagePlus,
  ImageOff,
  Clipboard,
  Play,
  RefreshCw,
  RotateCcw,
  Trash2,
  X,
} from "lucide-vue-next";
import ConfirmDialog from "../components/ConfirmDialog.vue";
import PluginManagerPanel from "../components/PluginManagerPanel.vue";
import WorkspacePlayerBar from "../components/WorkspacePlayerBar.vue";
import type { ContextMenuItem } from "../composables/useContextMenu";
import { useRepositoryWorkspace } from "../composables/useRepositoryWorkspace";
import { usePlaylistPlayer } from "../composables/usePlaylistPlayer";
import { getPreviewPluginFileActions, getPreviewPluginForEntry } from "../plugins/previewPlugins";
import { getPlaylistPlayerByType } from "../plugins/playlistPlayers";
import type { PlaylistPlayerObjectFit } from "../plugins/sdk";
import { isAudioExtension, isVideoExtension } from "../utils/filePreviewExtensions";
import { metadataPalette } from "../utils/fileMetadata";
import { splitListInput } from "../composables/workspace/filterInputs";
import {
  joinRepositoryPath,
  normalizeFilesystemPath,
  normalizeRepositoryRelativePath,
  repositoryPathParts,
  trimTrailingPathSeparators,
} from "../composables/workspace/paths";
import {
  getWorkspaceParentPath,
  internalWorkspaceDragDistance,
  normalizeWorkspaceMovePaths,
  resolveWorkspaceDropTarget,
  shouldDelegateToExternalDrag as shouldDelegateToExternalWorkspaceDrag,
} from "./workspace/dragBehavior";
import {
  createExternalDragIcon,
  extractPaletteFromImageElement,
} from "./workspace/thumbnailUi";
import type {
  FileBrowserEntry,
  HardlinkCandidate,
  PlaylistItem,
  SearchHit,
} from "../types/repository";

type FileDisplayMode = "adaptive" | "masonry" | "grid" | "list";
type SearchFilterListKey = "tags" | "formats" | "colors" | "shapes";
type IdlePreloadWindow = Window & {
  requestIdleCallback?: (callback: () => void, options?: { timeout?: number }) => number;
  cancelIdleCallback?: (handle: number) => void;
};
type InternalWorkspaceDragSession = {
  startX: number;
  startY: number;
  lastX: number;
  lastY: number;
  entry: FileBrowserEntry;
  delegatedToExternalDrag: boolean;
};

const fileDisplayModeStorageKey = "momobako.fileDisplayMode";
const fileDisplayModeOptions: Array<{ value: FileDisplayMode; label: string }> = [
  { value: "adaptive", label: "自适应" },
  { value: "masonry", label: "瀑布流" },
  { value: "grid", label: "网格" },
  { value: "list", label: "列表" },
];
const workspaceComponentLoaders = {
  CopyTargetDialog: () => import("./workspace/CopyTargetDialog.vue"),
  ExtensionsPanel: () => import("./workspace/ExtensionsPanel.vue"),
  FileBrowserPanel: () => import("./workspace/FileBrowserPanel.vue"),
  FilePreviewPane: () => import("./workspace/FilePreviewPane.vue"),
  HardlinkCandidateDialog: () => import("./workspace/HardlinkCandidateDialog.vue"),
  RepositoryActionsPanel: () => import("./workspace/RepositoryActionsPanel.vue"),
  SearchPanel: () => import("./workspace/SearchPanel.vue"),
};
const CopyTargetDialog = defineAsyncComponent(workspaceComponentLoaders.CopyTargetDialog);
const FileBrowserPanel = defineAsyncComponent(workspaceComponentLoaders.FileBrowserPanel);
const FilePreviewPane = defineAsyncComponent(workspaceComponentLoaders.FilePreviewPane);
const HardlinkCandidateDialog = defineAsyncComponent(workspaceComponentLoaders.HardlinkCandidateDialog);
const RepositoryActionsPanel = defineAsyncComponent(workspaceComponentLoaders.RepositoryActionsPanel);
const SearchPanel = defineAsyncComponent(workspaceComponentLoaders.SearchPanel);

let dragDropUnlisten: UnlistenFn | null = null;
let preloadHandle: { kind: "idle" | "timeout"; id: number } | null = null;
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

const createFileName = ref("");
const renameValue = ref("");
const renameTargetPath = ref<string | null>(null);
const isDraggingFiles = ref(false);
const isDraggingRepositoryFolder = ref(false);
const emptyRepositoryError = ref("");
const missingRepositoryError = ref("");
const missingRepositoryAction = ref<"relocating" | "deleting" | null>(null);
const showMissingRepositoryDeleteDialog = ref(false);
const previewFilePath = ref<string | null>(null);
const failedThumbnailPaths = ref<Set<string>>(new Set());
const fileDisplayMode = ref<FileDisplayMode>(readInitialFileDisplayMode());
const thumbnailAspectRatios = ref<Record<string, number>>({});
const thumbnailPalettes = ref<Record<string, string[]>>({});
const pendingCopySourcePaths = ref<string[]>([]);
const copyTargetDialogOpen = ref(false);
const copyTargetPath = ref("");
const skippedHardlinkCandidateIds = ref<Set<string>>(new Set());
const colorFilterInput = ref("");
const shapeFilterInput = ref("");
const excludeQueryInput = ref("");
const excludePathPrefixesInput = ref("");
const excludeTagsInput = ref("");
const excludeFormatsInput = ref("");
const excludeMetadataFiltersInput = ref("");
const excludeNumberFiltersInput = ref("");
const excludeDateFiltersInput = ref("");
const numberFiltersInput = ref("");
const dateFiltersInput = ref("");
const sortFieldInput = ref("");
const sortDirectionInput = ref<"asc" | "desc">("asc");
const limitInput = ref("");
const internalWorkspaceDragSession = ref<InternalWorkspaceDragSession | null>(null);
const externalDragSwitchDistance = 72;
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
  clearWorkspaceSelection,
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

const hasRepository = computed(() => Boolean(activeSnapshot.value));
const isMissingRepository = computed(() => activeRepository.value?.status === "missing");
const isMissingRepositoryBusy = computed(() => missingRepositoryAction.value !== null);
const isRepairingMissingRepository = computed(() => missingRepositoryAction.value === "relocating");
const isDeletingMissingRepository = computed(() => missingRepositoryAction.value === "deleting");
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
const currentHardlinkCandidate = computed(() => (
  hardlinkCandidates.value.find((candidate) => !skippedHardlinkCandidateIds.value.has(candidate.candidateId)) ?? null
));
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
const filterColorMap: Record<string, string> = {
  red: "#e05252",
  green: "#4f9d69",
  blue: "#4c7bd9",
  yellow: "#d6a93f",
  purple: "#8b6bd6",
  pink: "#d66b9a",
  orange: "#d98b3d",
  black: "#333333",
  white: "#e8e8e8",
  gray: "#8c9299",
  grey: "#8c9299",
  红色: "#e05252",
  绿色: "#4f9d69",
  蓝色: "#4c7bd9",
  黄色: "#d6a93f",
  紫色: "#8b6bd6",
  粉色: "#d66b9a",
  橙色: "#d98b3d",
  黑色: "#333333",
  白色: "#e8e8e8",
  灰色: "#8c9299",
};

function uniqueSorted(values: Array<string | null | undefined>) {
  return Array.from(new Set(
    values
      .map((value) => value?.trim() ?? "")
      .filter(Boolean),
  )).sort((left, right) => left.localeCompare(right, "zh-CN"));
}

function searchResultFormat(result: SearchHit) {
  const filename = result.filename || result.path;
  const index = filename.lastIndexOf(".");
  return index >= 0 ? filename.slice(index + 1).toLowerCase() : "";
}

function metadataText(metadata: Record<string, unknown>, key: string) {
  const value = metadata[key];
  if (typeof value === "string") return value;
  if (typeof value === "number" || typeof value === "boolean") return String(value);
  return "";
}

function metadataFilterOptions(key: string) {
  return uniqueSorted([
    ...searchResults.value.map((result) => metadataText(result.metadata, key)),
    ...(fileBrowser.value?.entries.map((entry) => metadataText(entry.metadata ?? {}, key)) ?? []),
  ]);
}

const tagFilterOptions = computed(() => uniqueSorted([
  ...(activeSnapshot.value?.assets.flatMap((asset) => asset.tags) ?? []),
  ...searchResults.value.flatMap((result) => result.tags),
]));

const formatFilterOptions = computed(() => uniqueSorted([
  ...(activeSnapshot.value?.assets.map((asset) => asset.extension) ?? []),
  ...searchResults.value.map(searchResultFormat),
]));

const colorFilterOptions = computed(() => metadataFilterOptions("color"));
const shapeFilterOptions = computed(() => metadataFilterOptions("shape"));

const searchResultScopeLabel = computed(() => (
  hasActiveFilters.value
    ? `${activeSnapshot.value?.repository.name ?? "当前资源库"}内筛选`
    : "全局搜索"
));

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

watch([currentFileEntry, hasMultipleSelection], ([entry, multiple]) => {
  if (multiple || (renameTargetPath.value && renameTargetPath.value !== entry?.path)) {
    renameTargetPath.value = null;
    renameValue.value = "";
  }
});

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

function isVideoEntry(entry: FileBrowserEntry) {
  return isVideoExtension(entry.extension);
}

function isAudioEntry(entry: FileBrowserEntry) {
  return isAudioExtension(entry.extension);
}

function isModelEntry(entry: FileBrowserEntry) {
  return Boolean(getPreviewPluginForEntry(entry));
}

function thumbnailSrc(entry: FileBrowserEntry) {
  if (!entry.thumbnailPath || failedThumbnailPaths.value.has(entry.path)) return null;
  return convertFileSrc(entry.thumbnailPath);
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

function markThumbnailFailed(entry: FileBrowserEntry) {
  failedThumbnailPaths.value = new Set([...failedThumbnailPaths.value, entry.path]);
}

function updateThumbnailAspectRatio(entry: FileBrowserEntry, event: Event) {
  const image = event.currentTarget as HTMLImageElement | null;
  if (!image?.naturalWidth || !image.naturalHeight) return;
  const aspectRatio = image.naturalWidth / image.naturalHeight;
  if (!Number.isFinite(aspectRatio) || aspectRatio <= 0) return;
  thumbnailAspectRatios.value = {
    ...thumbnailAspectRatios.value,
    [entry.path]: Math.min(Math.max(aspectRatio, 0.55), 2.4),
  };
  const palette = extractPaletteFromImageElement(image);
  if (palette.length) {
    thumbnailPalettes.value = {
      ...thumbnailPalettes.value,
      [entry.path]: palette,
    };
  }
}

function fileItemStyle(entry: FileBrowserEntry) {
  return {
    "--file-thumb-aspect": String(thumbnailAspectRatios.value[entry.path] ?? 1),
  };
}

function thumbnailPaletteColors(entry: FileBrowserEntry) {
  const metadataColors = metadataPalette(entry.metadata);
  if (metadataColors.length) return metadataColors;
  return thumbnailPalettes.value[entry.path] ?? [];
}

function resetThumbnailFailure(path: string) {
  const next = new Set(failedThumbnailPaths.value);
  next.delete(path);
  failedThumbnailPaths.value = next;
}

async function chooseCustomThumbnail(entry: FileBrowserEntry) {
  if (isTrashPanel.value) return;
  const selected = await openDialog({
    title: "选择自定义缩略图",
    multiple: false,
    filters: [
      {
        name: "图片",
        extensions: ["png", "jpg", "jpeg", "webp", "bmp"],
      },
    ],
  });
  if (typeof selected !== "string") return;
  const response = await setWorkspaceEntryThumbnail(entry.path, selected);
  if (response?.thumbnailPath) resetThumbnailFailure(entry.path);
}

async function readClipboardImageBytes() {
  const items = await navigator.clipboard?.read?.();
  if (!items?.length) return null;

  for (const item of items) {
    const type = item.types.find((value) => value.startsWith("image/"));
    if (!type) continue;
    const blob = await item.getType(type);
    return {
      mediaType: type,
      bytes: Array.from(new Uint8Array(await blob.arrayBuffer())),
    };
  }

  return null;
}

async function pasteCustomThumbnail(entry: FileBrowserEntry) {
  if (isTrashPanel.value) return;
  try {
    const image = await readClipboardImageBytes();
    if (!image) return;
    const response = await setWorkspaceEntryThumbnailFromBytes(entry.path, image.bytes, image.mediaType);
    if (response?.thumbnailPath) resetThumbnailFailure(entry.path);
  } catch {
    return;
  }
}

async function clearCustomThumbnail(entry: FileBrowserEntry) {
  if (isTrashPanel.value) return;
  const response = await clearWorkspaceEntryThumbnail(entry.path);
  resetThumbnailFailure(entry.path);
  if (!response?.thumbnailPath && entry.kind === "file") {
    await refreshWorkspaceEntryThumbnail(entry.path);
  }
}

async function refreshEntryThumbnail(entry: FileBrowserEntry) {
  if (isTrashPanel.value) return;
  const response = await refreshWorkspaceEntryThumbnail(entry.path);
  if (response?.thumbnailPath) resetThumbnailFailure(entry.path);
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

function startInternalWorkspaceDrag(paths: string[]) {
  setDraggedWorkspacePaths(paths);
  setInternalDragActive(true);
}

function finishInternalWorkspaceDrag() {
  internalWorkspaceDragSession.value = null;
  clearDraggedWorkspaceState();
  setDragHoverFolderPath(null);
}

function resolveFolderDropTarget(clientX: number, clientY: number) {
  return resolveWorkspaceDropTarget(document, clientX, clientY, currentDirectoryPath.value);
}

function updateInternalWorkspaceHover(clientX: number, clientY: number) {
  const nextTargetPath = resolveFolderDropTarget(clientX, clientY);
  if (nextTargetPath == null) {
    setDragHoverFolderPath(null);
    return null;
  }
  setDragHoverFolderPath(nextTargetPath);
  return nextTargetPath;
}

function shouldDelegateToExternalDrag(clientX: number, clientY: number) {
  const session = internalWorkspaceDragSession.value;
  if (!session) return false;
  return shouldDelegateToExternalWorkspaceDrag(
    session,
    clientX,
    clientY,
    { width: window.innerWidth, height: window.innerHeight },
    externalDragSwitchDistance,
  );
}

async function delegateToExternalWorkspaceDrag() {
  const session = internalWorkspaceDragSession.value;
  if (!session || session.delegatedToExternalDrag || !draggedWorkspacePaths.value.length) return;
  internalWorkspaceDragSession.value = {
    ...session,
    delegatedToExternalDrag: true,
  };
  const dragPaths = [...draggedWorkspacePaths.value];
  const icon = createExternalDragIcon(session.entry);
  finishInternalWorkspaceDrag();
  await startWorkspaceEntriesDrag(dragPaths, icon);
}

function handleEntryDragStart(entry: FileBrowserEntry, event: PointerEvent) {
  if (!canDragEntries.value) return;
  const dragPaths = selectedFilePathSet.value.has(entry.path)
    ? selectedFilePaths.value
    : [entry.path];
  if (!selectedFilePathSet.value.has(entry.path)) {
    selectWorkspaceEntries([entry.path], { primaryPath: entry.path, anchorPath: entry.path });
  }
  startInternalWorkspaceDrag(dragPaths);
  internalWorkspaceDragSession.value = {
    startX: event.clientX,
    startY: event.clientY,
    lastX: event.clientX,
    lastY: event.clientY,
    entry,
    delegatedToExternalDrag: false,
  };
  updateInternalWorkspaceHover(event.clientX, event.clientY);
}

function handleEntryDragMove(event: PointerEvent) {
  if (!isInternalDragActive.value) return;
  if (internalWorkspaceDragSession.value) {
    internalWorkspaceDragSession.value = {
      ...internalWorkspaceDragSession.value,
      lastX: event.clientX,
      lastY: event.clientY,
    };
  }
  updateInternalWorkspaceHover(event.clientX, event.clientY);
  if (shouldDelegateToExternalDrag(event.clientX, event.clientY)) {
    void delegateToExternalWorkspaceDrag();
  }
}

async function handleEntryDragEnd(event: PointerEvent | null) {
  const session = internalWorkspaceDragSession.value;
  if (!session) {
    finishInternalWorkspaceDrag();
    return;
  }
  if (session.delegatedToExternalDrag) {
    finishInternalWorkspaceDrag();
    return;
  }

  const targetPath = event ? updateInternalWorkspaceHover(event.clientX, event.clientY) : dragHoverFolderPath.value;
  if (!targetPath) {
    finishInternalWorkspaceDrag();
    return;
  }
  await moveDraggedWorkspaceEntries(targetPath);
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

function handleBoxSelection(paths: string[], mode: "replace" | "append") {
  if (mode === "append") {
    if (!paths.length) return;
    const nextPaths = Array.from(new Set([...selectedFilePaths.value, ...paths]));
    selectWorkspaceEntries(nextPaths, {
      primaryPath: selectedFilePath.value ?? paths[0] ?? null,
      anchorPath: selectedFilePath.value ?? paths[0] ?? null,
    });
    return;
  }

  if (!paths.length) {
    clearWorkspaceSelection();
    return;
  }
  selectWorkspaceEntries(paths, { primaryPath: paths[0], anchorPath: paths[0] });
}

function normalizeWorkspaceDragPaths(targetPath: string) {
  return normalizeWorkspaceMovePaths(draggedWorkspacePaths.value, targetPath);
}

async function moveDraggedWorkspaceEntries(targetPath: string) {
  const sourcePaths = normalizeWorkspaceDragPaths(targetPath);
  finishInternalWorkspaceDrag();
  if (!sourcePaths.length || isTrashPanel.value) return;
  await moveWorkspaceEntries(sourcePaths, targetPath);
}

function handleFolderDropHover(path: string) {
  if (isTrashPanel.value) return;
  setDragHoverFolderPath(path);
}

function handleFolderDropLeave(path: string) {
  if (dragHoverFolderPath.value === path) {
    setDragHoverFolderPath(null);
  }
}

async function handleFolderDrop(path: string, event: DragEvent) {
  if (isInternalWorkspaceDragEvent(event)) {
    await moveDraggedWorkspaceEntries(path);
    return;
  }

  const sourcePaths = getDroppedSourcePaths(event);
  if (!sourcePaths.length) return;
  setExternalDragActive(false);
  isDraggingFiles.value = false;
  setDragHoverFolderPath(null);
  await importEntriesToWorkspace(sourcePaths, path);
}

function handleWindowPointerLeave(event: PointerEvent) {
  const session = internalWorkspaceDragSession.value;
  if (!session || session.delegatedToExternalDrag || !isInternalDragActive.value) return;
  internalWorkspaceDragSession.value = {
    ...session,
    lastX: event.clientX,
    lastY: event.clientY,
  };
  if (internalWorkspaceDragDistance(internalWorkspaceDragSession.value) >= externalDragSwitchDistance) {
    void delegateToExternalWorkspaceDrag();
  }
}

function handleWindowBlur() {
  const session = internalWorkspaceDragSession.value;
  if (!session || session.delegatedToExternalDrag || !isInternalDragActive.value) return;
  if (internalWorkspaceDragDistance(session) >= externalDragSwitchDistance) {
    void delegateToExternalWorkspaceDrag();
  }
}

function getDroppedSourcePaths(event: DragEvent) {
  return Array.from(event.dataTransfer?.files ?? [])
    .map((file) => (file as File & { path?: string }).path ?? "")
    .filter((path) => path.trim().length > 0);
}

async function handleExternalPathsDrop(paths: string[]) {
  setExternalDragActive(false);
  const targetPath = dragHoverFolderPath.value ?? currentDirectoryPath.value;
  setDragHoverFolderPath(null);

  if (isTrashPanel.value || !activeSnapshot.value) return;

  const repoRoot = activeSnapshot.value.repository.path;
  const filteredPaths = paths.filter((sourcePath) => {
    const normalizedSourcePath = trimTrailingPathSeparators(sourcePath);
    if (!normalizedSourcePath) return false;
    const segments = repositoryPathParts(normalizedSourcePath);
    const name = segments[segments.length - 1];
    if (!name) return false;
    const targetAbsolutePath = joinRepositoryPath(repoRoot, targetPath, name);
    return normalizeFilesystemPath(targetAbsolutePath) !== normalizeFilesystemPath(normalizedSourcePath);
  });

  if (!filteredPaths.length) return;
  await importEntriesToWorkspace(filteredPaths, targetPath);
}

async function createRepositoryFromFolder(path: string) {
  const nextPath = path.trim();
  if (!nextPath) return;
  emptyRepositoryError.value = "";
  try {
    await attachRepository(nextPath);
  } catch (cause) {
    emptyRepositoryError.value = cause instanceof Error ? cause.message : String(cause);
  }
}

function isInternalWorkspaceDragEvent(event: DragEvent) {
  return isInternalDragActive.value
    || Array.from(event.dataTransfer?.types ?? []).includes("application/x-momobako-entry");
}

function handleDragOver(event: DragEvent) {
  if (!isRepositoryWritable.value || !isFilesPanel.value) return;
  event.preventDefault();
  if (isInternalWorkspaceDragEvent(event)) {
    if (event.dataTransfer) {
      event.dataTransfer.dropEffect = "move";
    }
    return;
  }
  if (event.dataTransfer) {
    event.dataTransfer.dropEffect = "copy";
  }
  setExternalDragActive(true);
  isDraggingFiles.value = true;
}

function handleDragLeave(event: DragEvent) {
  const currentTarget = event.currentTarget as HTMLElement | null;
  const relatedTarget = event.relatedTarget as Node | null;
  if (currentTarget && relatedTarget && currentTarget.contains(relatedTarget)) return;
  if (isInternalDragActive.value) return;
  setExternalDragActive(false);
  setDragHoverFolderPath(null);
  isDraggingFiles.value = false;
}

async function handleDrop(event: DragEvent) {
  event.preventDefault();
  if (isInternalWorkspaceDragEvent(event)) {
    await moveDraggedWorkspaceEntries(dragHoverFolderPath.value ?? currentDirectoryPath.value);
    return;
  }
  setExternalDragActive(false);
  isDraggingFiles.value = false;
  if (!isRepositoryWritable.value || isTrashPanel.value) return;
  const sourcePaths = getDroppedSourcePaths(event);
  if (!sourcePaths.length) return;
  await handleExternalPathsDrop(sourcePaths);
}

function handleEmptyRepositoryDragOver(event: DragEvent) {
  if (activeRepoId.value || hasRepository.value) return;
  event.preventDefault();
  if (event.dataTransfer) {
    event.dataTransfer.dropEffect = "copy";
  }
  isDraggingRepositoryFolder.value = true;
}

function handleEmptyRepositoryDragLeave(event: DragEvent) {
  const currentTarget = event.currentTarget as HTMLElement | null;
  const relatedTarget = event.relatedTarget as Node | null;
  if (currentTarget && relatedTarget && currentTarget.contains(relatedTarget)) return;
  isDraggingRepositoryFolder.value = false;
}

async function handleEmptyRepositoryDrop(event: DragEvent) {
  event.preventDefault();
  isDraggingRepositoryFolder.value = false;
  if (activeRepoId.value || hasRepository.value) return;
  const [path] = getDroppedSourcePaths(event);
  if (path) {
    await createRepositoryFromFolder(path);
  }
}

async function handleCreateFile() {
  if (isTrashPanel.value) return;
  if (!createFileName.value.trim()) return;
  const snapshot = await createFileInWorkspace(createFileName.value.trim());
  if (snapshot) {
    createFileName.value = "";
  }
}

function startRenameSelected() {
  if (!currentFileEntry.value) return;
  renameTargetPath.value = currentFileEntry.value.path;
  renameValue.value = currentFileEntry.value.name;
}

async function submitRenameSelected() {
  if (!renameTargetPath.value || !renameValue.value.trim()) return;
  const snapshot = await renameWorkspaceEntry(renameTargetPath.value, renameValue.value.trim());
  if (snapshot) {
    renameTargetPath.value = null;
    renameValue.value = "";
  }
}

async function deleteSelectedEntry() {
  if (!selectedFilePaths.value.length) return;
  if (selectedFilePaths.value.length > 1) {
    await deleteWorkspaceEntries(selectedFilePaths.value, isTrashPanel.value ? "permanentDelete" : undefined);
    return;
  }
  if (!currentFileEntry.value) return;
  await deleteWorkspaceEntry(currentFileEntry.value.path, isTrashPanel.value ? "permanentDelete" : undefined);
}

async function deleteEntry(entry: FileBrowserEntry) {
  await deleteWorkspaceEntry(entry.path, isTrashPanel.value ? "permanentDelete" : undefined);
}

function openCopyTargetDialog(entry: FileBrowserEntry) {
  if (isTrashPanel.value) return;
  pendingCopySourcePaths.value = selectedFilePathSet.value.has(entry.path)
    ? [...selectedFilePaths.value]
    : [entry.path];
  copyTargetPath.value = fileBrowser.value?.currentPath ?? "";
  copyTargetDialogOpen.value = true;
}

async function submitCopyTarget() {
  const paths = pendingCopySourcePaths.value;
  if (!paths.length) return;
  const targetPath = normalizeRepositoryRelativePath(copyTargetPath.value);
  const snapshot = await copyWorkspaceEntries(paths, targetPath);
  if (snapshot) {
    cancelCopyTarget();
  }
}

function cancelCopyTarget() {
  copyTargetDialogOpen.value = false;
  pendingCopySourcePaths.value = [];
  copyTargetPath.value = "";
}

async function confirmCurrentHardlinkCandidate() {
  const candidate = currentHardlinkCandidate.value;
  if (!candidate) return;
  const response = await confirmWorkspaceHardlinkCandidate(candidate.candidateId);
  if (response) {
    skippedHardlinkCandidateIds.value.delete(candidate.candidateId);
  }
}

function skipCurrentHardlinkCandidate() {
  const candidate = currentHardlinkCandidate.value;
  if (!candidate) return;
  skippedHardlinkCandidateIds.value = new Set([
    ...skippedHardlinkCandidateIds.value,
    candidate.candidateId,
  ]);
}

async function restoreSelectedEntry() {
  if (!selectedFilePaths.value.length || !isTrashPanel.value) return;
  if (selectedFilePaths.value.length > 1) {
    await restoreTrashEntries(selectedFilePaths.value);
    return;
  }
  if (!currentFileEntry.value) return;
  await restoreTrashEntry(currentFileEntry.value.path);
}

async function restoreEntry(entry: FileBrowserEntry) {
  if (!isTrashPanel.value) return;
  await restoreTrashEntry(entry.path);
}

async function handleRestoreAllTrash() {
  if (!isTrashPanel.value) return;
  await restoreAllTrashEntries();
}

async function handleEmptyTrash() {
  if (!isTrashPanel.value) return;
  await emptyTrash();
}

async function openSelectedEntry() {
  if (isTrashPanel.value) return;
  if (!currentFileEntry.value) return;
  if (currentFileEntry.value.kind === "directory") {
    openDirectory(currentFileEntry.value.path);
    return;
  }
  await openWorkspaceEntry(currentFileEntry.value.path);
}

async function revealSelectedEntry() {
  if (isTrashPanel.value) return;
  if (!currentFileEntry.value) return;
  await revealWorkspaceEntry(currentFileEntry.value.path);
}

function fileEntryContextMenu(entry: FileBrowserEntry) {
  if (!selectedFilePathSet.value.has(entry.path)) {
    selectWorkspaceEntries([entry.path], { primaryPath: entry.path, anchorPath: entry.path });
  }
  const contextSelectionPaths = selectedFilePathSet.value.has(entry.path)
    ? selectedFilePaths.value
    : [entry.path];
  if (isSmartFolderPanel.value) {
    return [
      {
        id: "preview",
        label: "预览",
        icon: Eye,
        disabled: entry.kind !== "file",
        onSelect: () => {
          if (entry.kind === "file") {
            previewFilePath.value = entry.path;
          }
        },
      },
      {
        id: "open",
        label: "打开",
        icon: Eye,
        disabled: entry.kind !== "file",
        onSelect: () => openWorkspaceEntry(entry.path),
      },
      {
        id: "reveal",
        label: "定位",
        icon: FolderOpen,
        onSelect: () => revealWorkspaceEntry(entry.path),
      },
    ];
  }
  const pluginActions = activeRepoId.value && entry.kind === "file" && !isTrashPanel.value
    ? getPreviewPluginFileActions(activeRepoId.value, entry).map<ContextMenuItem>((action) => ({
      id: action.id,
      label: action.label,
      icon: action.icon,
      disabled: action.disabled || hasMultipleSelection.value,
      danger: action.danger,
      confirmLabel: action.confirmLabel,
      onSelect: action.onSelect,
    }))
    : [];
  const playlistMenuItems = !isSmartFolderPanel.value
    && !isTrashPanel.value
    && entry.kind === "file"
    && !hasMultipleSelection.value
    && entry.assetId
    ? compatiblePlaylistsForEntry(entry).map<ContextMenuItem>((playlist) => ({
      id: `playlist-${playlist.playlistId}`,
      label: playlist.name,
      checked: (playlistMemberships.value[entry.assetId ?? ""] ?? []).includes(playlist.playlistId),
      onSelect: () => togglePlaylistMembership(entry, playlist.playlistId),
    }))
    : [];
  const items = [
    ...(isTrashPanel.value ? [{
      id: "restore",
      label: "还原",
      icon: RotateCcw,
      disabled: isMutatingFiles.value,
      onSelect: async () => {
        if (contextSelectionPaths.length > 1) {
          await restoreTrashEntries(contextSelectionPaths);
          return;
        }
        await restoreEntry(entry);
      },
    }] : []),
    {
      id: "preview",
      label: "预览",
      icon: Eye,
      disabled: entry.kind !== "file" || isTrashPanel.value || hasMultipleSelection.value,
      onSelect: () => {
        if (entry.kind === "file") {
          previewFilePath.value = entry.path;
        }
      },
    },
    {
      id: "open",
      label: entry.kind === "directory" ? "进入" : "打开",
      icon: Eye,
      disabled: isTrashPanel.value || hasMultipleSelection.value,
      onSelect: () => {
        if (entry.kind === "directory") {
          openDirectory(entry.path);
          return;
        }
        return openWorkspaceEntry(entry.path);
      },
    },
    {
      id: "reveal",
      label: "定位",
      icon: FolderOpen,
      disabled: isTrashPanel.value,
      onSelect: () => revealWorkspaceEntry(entry.path),
    },
    {
      id: "copy-target",
      label: "复制到…",
      icon: Files,
      disabled: isTrashPanel.value || isMutatingFiles.value,
      onSelect: () => openCopyTargetDialog(entry),
    },
    ...(playlistMenuItems.length ? [{
      id: "playlist-membership",
      label: "添加到播放集",
      children: playlistMenuItems,
    } satisfies ContextMenuItem] : []),
    ...pluginActions,
    {
      id: "thumbnail",
      label: "缩略图",
      icon: FileImage,
      disabled: isTrashPanel.value,
      children: [
        {
          id: "thumbnail-custom-file",
          label: "自定义缩略图（选择文件）",
          icon: ImagePlus,
          onSelect: () => chooseCustomThumbnail(entry),
        },
        {
          id: "thumbnail-custom-clipboard",
          label: "新增自定义缩略图（从剪贴板）",
          icon: Clipboard,
          onSelect: () => pasteCustomThumbnail(entry),
        },
        {
          id: "thumbnail-clear-custom",
          label: "取消自定义缩略图",
          icon: ImageOff,
          disabled: !entry.thumbnailCustom,
          onSelect: () => clearCustomThumbnail(entry),
        },
        {
          id: "thumbnail-refresh",
          label: "刷新缩略图",
          icon: RefreshCw,
          onSelect: () => refreshEntryThumbnail(entry),
        },
      ],
    },
    {
      id: "rename",
      label: "重命名",
      icon: PencilLine,
      disabled: isTrashPanel.value || hasMultipleSelection.value,
      onSelect: () => {
        renameTargetPath.value = entry.path;
        renameValue.value = entry.name;
      },
    },
    {
      id: "delete",
      label: isTrashPanel.value ? "彻底删除" : "删除",
      icon: Trash2,
      danger: true,
      disabled: isMutatingFiles.value,
      confirmLabel: isTrashPanel.value ? "确认彻底删除？再点一次" : undefined,
      onSelect: async () => {
        if (contextSelectionPaths.length > 1) {
          await deleteWorkspaceEntries(contextSelectionPaths, isTrashPanel.value ? "permanentDelete" : undefined);
          return;
        }
        await deleteEntry(entry);
      },
    },
  ];
  return items;
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

function toggleSearchFilter(key: SearchFilterListKey, value: string) {
  if (!isRepositoryWritable.value) return;
  toggleFilterValue(key, value);
  setActivePanel("search");
  void runFilteredSearch();
}

function submitMetadataFilterInput(key: "colors" | "shapes") {
  if (!isRepositoryWritable.value) return;
  const input = key === "colors" ? colorFilterInput : shapeFilterInput;
  const value = input.value.trim();
  if (!value) return;
  toggleSearchFilter(key, value);
  input.value = "";
}

function selectMinimumRating(value: number | null) {
  if (!isRepositoryWritable.value) return;
  setMinimumRatingFilter(value);
  setActivePanel("search");
  void runFilteredSearch();
}

function clearSearchFilters() {
  if (!isRepositoryWritable.value) return;
  clearFilters();
  colorFilterInput.value = "";
  shapeFilterInput.value = "";
  excludeQueryInput.value = "";
  excludePathPrefixesInput.value = "";
  excludeTagsInput.value = "";
  excludeFormatsInput.value = "";
  excludeMetadataFiltersInput.value = "";
  excludeNumberFiltersInput.value = "";
  excludeDateFiltersInput.value = "";
  numberFiltersInput.value = "";
  dateFiltersInput.value = "";
  sortFieldInput.value = "";
  sortDirectionInput.value = "asc";
  limitInput.value = "";
  setActivePanel("search");
  void runFilteredSearch();
}

function applyAdvancedSearchFilters() {
  if (!isRepositoryWritable.value) return;
  const limit = Number(limitInput.value);
  updateFilters({
    excludeQuery: excludeQueryInput.value.trim(),
    excludePathPrefixes: excludePathPrefixesInput.value.trim(),
    excludeTags: splitListInput(excludeTagsInput.value),
    excludeFormats: splitListInput(excludeFormatsInput.value),
    excludeMetadataFilters: excludeMetadataFiltersInput.value.trim(),
    excludeNumberFilters: excludeNumberFiltersInput.value.trim(),
    excludeDateFilters: excludeDateFiltersInput.value.trim(),
    numberFilters: numberFiltersInput.value.trim(),
    dateFilters: dateFiltersInput.value.trim(),
    sortField: sortFieldInput.value.trim(),
    sortDirection: sortDirectionInput.value,
    limit: Number.isFinite(limit) && limit > 0 ? limit : null,
  });
  setActivePanel("search");
  void runFilteredSearch();
}

function closeFilterBar() {
  setFilterBarOpen(false);
}

function searchResultRating(result: SearchHit) {
  const value = result.metadata.rating;
  return typeof value === "number" && value > 0 ? value : null;
}

function searchResultContext(result: SearchHit) {
  const rating = searchResultRating(result);
  return [
    searchResultFormat(result) || "文件",
    ...result.tags.slice(0, 3),
    metadataText(result.metadata, "color"),
    metadataText(result.metadata, "shape"),
    rating == null ? "" : `${rating} 星`,
  ].filter(Boolean);
}

function filterColorStyle(color: string) {
  const trimmed = color.trim();
  const hexColor = /^#[0-9a-f]{6}$/i.test(trimmed) ? trimmed : null;
  return {
    "--filter-swatch": hexColor ?? filterColorMap[color.toLowerCase()] ?? filterColorMap[color] ?? "var(--accent)",
  };
}


const searchSummary = computed(() => {
  if (hasActiveFilters.value) {
    return searchQuery.value.trim()
      ? `当前资源库筛选: ${searchQuery.value}`
      : "按当前资源库筛选结果。";
  }
  if (searchQuery.value.trim()) {
    return `当前查询: ${searchQuery.value}`;
  }
  return "输入关键词、标签或评分条件后，这里会展示跨仓库结果。";
});

function preloadWorkspaceComponents() {
  const primaryLoaders = activePanel.value === "search"
    ? [workspaceComponentLoaders.SearchPanel]
    : activePanel.value === "extensions"
      ? [workspaceComponentLoaders.ExtensionsPanel]
      : [workspaceComponentLoaders.FileBrowserPanel];
  const secondaryLoaders = [
    workspaceComponentLoaders.FilePreviewPane,
    workspaceComponentLoaders.SearchPanel,
    workspaceComponentLoaders.ExtensionsPanel,
    workspaceComponentLoaders.CopyTargetDialog,
    workspaceComponentLoaders.HardlinkCandidateDialog,
  ];

  for (const load of new Set([...primaryLoaders, ...secondaryLoaders])) {
    void load().catch(() => undefined);
  }
}

function queueWorkspaceComponentPreload() {
  if (hasQueuedWorkspacePreload) return;
  hasQueuedWorkspacePreload = true;
  const currentWindow = window as IdlePreloadWindow;
  if (currentWindow.requestIdleCallback) {
    preloadHandle = {
      kind: "idle",
      id: currentWindow.requestIdleCallback(preloadWorkspaceComponents, { timeout: 1200 }),
    };
    return;
  }
  preloadHandle = {
    kind: "timeout",
    id: window.setTimeout(preloadWorkspaceComponents, 250),
  };
}

function cancelWorkspaceComponentPreload() {
  if (!preloadHandle) return;
  const currentWindow = window as IdlePreloadWindow;
  if (preloadHandle.kind === "idle" && currentWindow.cancelIdleCallback) {
    currentWindow.cancelIdleCallback(preloadHandle.id);
  } else {
    window.clearTimeout(preloadHandle.id);
  }
  preloadHandle = null;
}

watch(hasRepository, (ready) => {
  if (ready) {
    queueWorkspaceComponentPreload();
  }
}, { immediate: true });

watch(activeRepoId, () => {
  missingRepositoryError.value = "";
  showMissingRepositoryDeleteDialog.value = false;
});

async function chooseMissingRepositoryPath() {
  if (!activeRepoId.value || isMissingRepositoryBusy.value) return;
  missingRepositoryError.value = "";
  const selected = await openDialog({
    title: "重定向资源库位置",
    directory: true,
    multiple: false,
  });
  if (typeof selected !== "string" || !selected.trim()) return;

  missingRepositoryAction.value = "relocating";
  try {
    await relocateMissingRepository(activeRepoId.value, selected);
    missingRepositoryError.value = "";
  } catch (cause) {
    missingRepositoryError.value = cause instanceof Error ? cause.message : String(cause);
  } finally {
    missingRepositoryAction.value = null;
  }
}

async function refreshMissingRepository() {
  if (isMissingRepositoryBusy.value) return;
  missingRepositoryError.value = "";
  try {
    await refreshRepositoryWorkspace();
  } catch (cause) {
    missingRepositoryError.value = cause instanceof Error ? cause.message : String(cause);
  }
}

function openMissingRepositoryDeleteDialog() {
  if (!activeRepoId.value || isMissingRepositoryBusy.value) return;
  missingRepositoryError.value = "";
  showMissingRepositoryDeleteDialog.value = true;
}

function closeMissingRepositoryDeleteDialog() {
  if (isDeletingMissingRepository.value) return;
  showMissingRepositoryDeleteDialog.value = false;
}

async function confirmMissingRepositoryDelete() {
  if (!activeRepoId.value) return;
  missingRepositoryAction.value = "deleting";
  missingRepositoryError.value = "";
  try {
    await removeRepository(activeRepoId.value);
    showMissingRepositoryDeleteDialog.value = false;
  } catch (cause) {
    missingRepositoryError.value = cause instanceof Error ? cause.message : String(cause);
  } finally {
    missingRepositoryAction.value = null;
  }
}

onMounted(() => {
  window.addEventListener("pointerleave", handleWindowPointerLeave);
  window.addEventListener("blur", handleWindowBlur);
  try {
    const currentWindow = getCurrentWindow();
    currentWindow.onDragDropEvent(({ payload }) => {
      if (!hasRepository.value && !isMissingRepository.value) {
        if (payload.type === "enter" || payload.type === "over") {
          isDraggingRepositoryFolder.value = true;
          return;
        }
        if (payload.type === "leave") {
          isDraggingRepositoryFolder.value = false;
          return;
        }
        isDraggingRepositoryFolder.value = false;
        if (payload.paths.length) {
          void createRepositoryFromFolder(payload.paths[0]);
        }
        return;
      }
      if (!isRepositoryWritable.value || !isFilesPanel.value) return;
      if (payload.type === "enter" || payload.type === "over") {
        setExternalDragActive(true);
        isDraggingFiles.value = true;
        return;
      }
      if (payload.type === "leave") {
        setExternalDragActive(false);
        setDragHoverFolderPath(null);
        isDraggingFiles.value = false;
        return;
      }
      setExternalDragActive(false);
      isDraggingFiles.value = false;
      if (payload.paths.length) {
        void handleExternalPathsDrop(payload.paths);
      }
    }).then((unlisten) => {
      dragDropUnlisten = unlisten;
    }).catch(() => {
      dragDropUnlisten = null;
    });
  } catch {
    dragDropUnlisten = null;
  }
});

onUnmounted(() => {
  dragDropUnlisten?.();
  window.removeEventListener("pointerleave", handleWindowPointerLeave);
  window.removeEventListener("blur", handleWindowBlur);
  setExternalDragActive(false);
  setDragHoverFolderPath(null);
  clearDraggedWorkspaceState();
  cancelWorkspaceComponentPreload();
});
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
        :thumbnail-palette="thumbnailPaletteColors"
        :save-metadata="saveFileMetadata"
        :status-label="statusLabel"
        @back="exitPreview"
        @open="openWorkspaceEntry"
        @reveal="revealWorkspaceEntry"
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
