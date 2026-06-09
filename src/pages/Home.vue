<script setup lang="ts">
import { computed, defineAsyncComponent, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { convertFileSrc } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { UnlistenFn } from "@tauri-apps/api/event";
import {
  Eye,
  FileImage,
  Files,
  FolderOpen,
  PencilLine,
  ImagePlus,
  ImageOff,
  Clipboard,
  RefreshCw,
  RotateCcw,
  Trash2,
  X,
} from "lucide-vue-next";
import PluginManagerPanel from "../components/PluginManagerPanel.vue";
import { useRepositoryWorkspace } from "../composables/useRepositoryWorkspace";
import { getPreviewPluginForEntry } from "../plugins/previewPlugins";
import { isAudioExtension, isVideoExtension } from "../plugins/mediaPreview/mediaExtensions";
import type {
  FileBrowserEntry,
  HardlinkCandidate,
  SearchHit,
} from "../types/repository";

type FileDisplayMode = "adaptive" | "masonry" | "grid" | "list";
type SearchFilterListKey = "tags" | "formats" | "colors" | "shapes";
type IdlePreloadWindow = Window & {
  requestIdleCallback?: (callback: () => void, options?: { timeout?: number }) => number;
  cancelIdleCallback?: (handle: number) => void;
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
  SearchPanel: () => import("./workspace/SearchPanel.vue"),
};
const CopyTargetDialog = defineAsyncComponent(workspaceComponentLoaders.CopyTargetDialog);
const FileBrowserPanel = defineAsyncComponent(workspaceComponentLoaders.FileBrowserPanel);
const FilePreviewPane = defineAsyncComponent(workspaceComponentLoaders.FilePreviewPane);
const HardlinkCandidateDialog = defineAsyncComponent(workspaceComponentLoaders.HardlinkCandidateDialog);
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
const previewFilePath = ref<string | null>(null);
const failedThumbnailPaths = ref<Set<string>>(new Set());
const fileDisplayMode = ref<FileDisplayMode>(readInitialFileDisplayMode());
const thumbnailAspectRatios = ref<Record<string, number>>({});
const pendingCopySourcePaths = ref<string[]>([]);
const copyTargetDialogOpen = ref(false);
const copyTargetPath = ref("");
const skippedHardlinkCandidateIds = ref<Set<string>>(new Set());
const colorFilterInput = ref("");
const shapeFilterInput = ref("");

const {
  activePanel,
  activeSnapshot,
  activeRepoId,
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
  hasSplitFileGroups,
  selectedEntry,
  searchQuery,
  selectedFilePath,
  searchResults,
  hardlinkCandidates,
  isLoadingFileBrowser,
  isSearching,
  isMutatingFiles,
  error,
  selectRepository,
  selectAsset,
  loadFileBrowserForDirectory,
  createFileInWorkspace,
  copyWorkspaceEntries,
  attachRepository,
  importEntriesToWorkspace,
  renameWorkspaceEntry,
  deleteWorkspaceEntry,
  restoreTrashEntry,
  restoreAllTrashEntries,
  emptyTrash,
  openWorkspaceEntry,
  revealWorkspaceEntry,
  startWorkspaceEntryDrag,
  selectWorkspaceEntry,
  setActivePanel,
  setWorkspaceEntryThumbnail,
  setWorkspaceEntryThumbnailFromBytes,
  clearWorkspaceEntryThumbnail,
  refreshWorkspaceEntryThumbnail,
  setFilterBarOpen,
  toggleFilterValue,
  setMinimumRatingFilter,
  clearFilters,
  runFilteredSearch,
  confirmWorkspaceHardlinkCandidate,
} = useRepositoryWorkspace();

const hasRepository = computed(() => Boolean(activeSnapshot.value));
const isFilesPanel = computed(() => activePanel.value === "files");
const isTrashPanel = computed(() => activePanel.value === "deleted");
const isSearchPanel = computed(() => activePanel.value === "search");
const isExtensionsPanel = computed(() => activePanel.value === "extensions");
const isFileBrowserPanel = computed(() => isFilesPanel.value || isTrashPanel.value);
const currentFileEntry = selectedEntry;

const canRenameSelected = computed(() => Boolean(currentFileEntry.value) && !isTrashPanel.value);
const canPreviewSelected = computed(() => currentFileEntry.value?.kind === "file" && !isTrashPanel.value);
const canDeleteSelected = computed(() => Boolean(currentFileEntry.value));
const canRestoreSelected = computed(() => Boolean(currentFileEntry.value) && isTrashPanel.value);
const canDragEntries = computed(() => !isTrashPanel.value && fileBrowser.value?.backendKind === "filesystem");
const previewFileEntry = computed(() => (
  previewFilePath.value ? fileBrowserEntryMap.value.get(previewFilePath.value) ?? null : null
));
const previewPlugin = computed(() => getPreviewPluginForEntry(previewFileEntry.value));
const fileDisplayModeClass = computed(() => `files-list__files--${fileDisplayMode.value}`);
const currentHardlinkCandidate = computed(() => (
  hardlinkCandidates.value.find((candidate) => !skippedHardlinkCandidateIds.value.has(candidate.candidateId)) ?? null
));
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
    case "linked":
      return "硬链接关联";
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

watch(currentFileEntry, (entry) => {
  if (renameTargetPath.value && renameTargetPath.value !== entry?.path) {
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
}

function fileItemStyle(entry: FileBrowserEntry) {
  return {
    "--file-thumb-aspect": String(thumbnailAspectRatios.value[entry.path] ?? 1),
  };
}

function fillRoundedRect(context: CanvasRenderingContext2D, x: number, y: number, width: number, height: number, radius: number) {
  context.beginPath();
  context.moveTo(x + radius, y);
  context.lineTo(x + width - radius, y);
  context.quadraticCurveTo(x + width, y, x + width, y + radius);
  context.lineTo(x + width, y + height - radius);
  context.quadraticCurveTo(x + width, y + height, x + width - radius, y + height);
  context.lineTo(x + radius, y + height);
  context.quadraticCurveTo(x, y + height, x, y + height - radius);
  context.lineTo(x, y + radius);
  context.quadraticCurveTo(x, y, x + radius, y);
  context.closePath();
  context.fill();
}

function createExternalDragIcon(entry: FileBrowserEntry) {
  const canvas = document.createElement("canvas");
  const context = canvas.getContext("2d");
  if (!context) return undefined;

  const size = 72;
  const scale = Math.min(Math.max(window.devicePixelRatio || 1, 1), 2);
  canvas.width = size * scale;
  canvas.height = size * scale;
  context.scale(scale, scale);

  const gradient = context.createLinearGradient(0, 0, size, size);
  if (entry.kind === "directory") {
    gradient.addColorStop(0, "#d3b26f");
    gradient.addColorStop(1, "#6e542e");
  } else {
    gradient.addColorStop(0, "#8aa8b0");
    gradient.addColorStop(1, "#314a53");
  }

  context.fillStyle = "rgba(0, 0, 0, 0.22)";
  fillRoundedRect(context, 8, 10, 56, 56, 12);
  context.fillStyle = gradient;
  fillRoundedRect(context, 6, 6, 56, 56, 12);

  context.fillStyle = "rgba(255, 255, 255, 0.9)";
  context.font = "700 16px system-ui, sans-serif";
  context.textAlign = "center";
  context.textBaseline = "middle";
  const label = entry.kind === "directory" ? "DIR" : (entry.extension || "FILE").slice(0, 4).toUpperCase();
  context.fillText(label, 34, 34, 44);

  return canvas.toDataURL("image/png");
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
  const normalizedPath = path.replace(/\\/g, "/").replace(/^\/+|\/+$/g, "");
  const index = normalizedPath.lastIndexOf("/");
  return index >= 0 ? normalizedPath.slice(0, index) : "";
}

function openDirectory(path: string) {
  void loadFileBrowserForDirectory(path, isTrashPanel.value ? { specialLocation: "trash" } : {});
}

function selectFileEntry(entry: FileBrowserEntry) {
  if (entry.kind === "directory") {
    openDirectory(entry.path);
    return;
  }
  selectWorkspaceEntry(entry.path);
}

function handleEntryDragStart(entry: FileBrowserEntry) {
  if (!canDragEntries.value) return;
  selectWorkspaceEntry(entry.path);
  void startWorkspaceEntryDrag(entry.path, createExternalDragIcon(entry));
}

function previewFileEntryByDoubleClick(entry: FileBrowserEntry) {
  if (entry.kind !== "file" || isTrashPanel.value) return;
  selectWorkspaceEntry(entry.path);
  previewFilePath.value = entry.path;
}

function exitPreview() {
  previewFilePath.value = null;
}

function getDroppedSourcePaths(event: DragEvent) {
  return Array.from(event.dataTransfer?.files ?? [])
    .map((file) => (file as File & { path?: string }).path ?? "")
    .filter((path) => path.trim().length > 0);
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

function handleDragOver(event: DragEvent) {
  if (!hasRepository.value || !isFilesPanel.value) return;
  event.preventDefault();
  if (event.dataTransfer) {
    event.dataTransfer.dropEffect = "copy";
  }
  isDraggingFiles.value = true;
}

function handleDragLeave(event: DragEvent) {
  const currentTarget = event.currentTarget as HTMLElement | null;
  const relatedTarget = event.relatedTarget as Node | null;
  if (currentTarget && relatedTarget && currentTarget.contains(relatedTarget)) return;
  isDraggingFiles.value = false;
}

async function handleDrop(event: DragEvent) {
  event.preventDefault();
  isDraggingFiles.value = false;
  if (isTrashPanel.value) return;
  const sourcePaths = getDroppedSourcePaths(event);
  if (!sourcePaths.length) return;
  void importEntriesToWorkspace(sourcePaths);
}

function handleEmptyRepositoryDragOver(event: DragEvent) {
  if (hasRepository.value) return;
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
  if (!currentFileEntry.value) return;
  await deleteWorkspaceEntry(currentFileEntry.value.path, isTrashPanel.value ? "permanentDelete" : undefined);
}

async function deleteEntry(entry: FileBrowserEntry) {
  await deleteWorkspaceEntry(entry.path, isTrashPanel.value ? "permanentDelete" : undefined);
}

function openCopyTargetDialog(entry: FileBrowserEntry) {
  if (isTrashPanel.value) return;
  pendingCopySourcePaths.value = [entry.path];
  copyTargetPath.value = fileBrowser.value?.currentPath ?? "";
  copyTargetDialogOpen.value = true;
}

async function submitCopyTarget() {
  const paths = pendingCopySourcePaths.value;
  if (!paths.length) return;
  const targetPath = copyTargetPath.value.trim().replace(/\\/g, "/").replace(/^\/+|\/+$/g, "");
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
  if (!currentFileEntry.value || !isTrashPanel.value) return;
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
  await openWorkspaceEntry(currentFileEntry.value.path);
}

async function revealSelectedEntry() {
  if (isTrashPanel.value) return;
  if (!currentFileEntry.value) return;
  await revealWorkspaceEntry(currentFileEntry.value.path);
}

function fileEntryContextMenu(entry: FileBrowserEntry) {
  selectWorkspaceEntry(entry.path);
  const items = [
    ...(isTrashPanel.value ? [{
      id: "restore",
      label: "还原",
      icon: RotateCcw,
      disabled: isMutatingFiles.value,
      onSelect: () => restoreEntry(entry),
    }] : []),
    {
      id: "preview",
      label: "预览",
      icon: Eye,
      disabled: entry.kind !== "file" || isTrashPanel.value,
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
      disabled: entry.kind !== "file" || isTrashPanel.value,
      onSelect: () => openWorkspaceEntry(entry.path),
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
      disabled: isTrashPanel.value,
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
      onSelect: () => deleteEntry(entry),
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
    selectWorkspaceEntry(matchedEntry.path);
    if (matchedEntry.kind === "file") {
      await nextTick();
      previewFilePath.value = matchedEntry.path;
    }
  }

  await selectAsset(result.assetId);
}

function toggleSearchFilter(key: SearchFilterListKey, value: string) {
  toggleFilterValue(key, value);
  setActivePanel("search");
  void runFilteredSearch();
}

function submitMetadataFilterInput(key: "colors" | "shapes") {
  const input = key === "colors" ? colorFilterInput : shapeFilterInput;
  const value = input.value.trim();
  if (!value) return;
  toggleSearchFilter(key, value);
  input.value = "";
}

function selectMinimumRating(value: number | null) {
  setMinimumRatingFilter(value);
  setActivePanel("search");
  void runFilteredSearch();
}

function clearSearchFilters() {
  clearFilters();
  colorFilterInput.value = "";
  shapeFilterInput.value = "";
  setActivePanel("search");
  void runFilteredSearch();
}

function closeFilterBar() {
  setFilterBarOpen(false);
}

function searchResultRating(result: SearchHit) {
  const value = result.metadata.rating;
  return typeof value === "number" ? value : null;
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
  return {
    "--filter-swatch": filterColorMap[color.toLowerCase()] ?? filterColorMap[color] ?? "var(--accent)",
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

onMounted(() => {
  try {
    const currentWindow = getCurrentWindow();
    currentWindow.onDragDropEvent(({ payload }) => {
      if (!hasRepository.value) {
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
      if (!hasRepository.value || !isFilesPanel.value) return;
      if (payload.type === "enter" || payload.type === "over") {
        isDraggingFiles.value = true;
        return;
      }
      if (payload.type === "leave") {
        isDraggingFiles.value = false;
        return;
      }
      isDraggingFiles.value = false;
      if (payload.paths.length) {
        void importEntriesToWorkspace(payload.paths);
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
  cancelWorkspaceComponentPreload();
});
</script>

<template>
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
    </div>
  </div>

  <section v-if="hasRepository && isFileBrowserPanel" :class="previewFileEntry ? 'files-preview-page' : 'files-workbench'">
    <template v-if="previewFileEntry">
      <FilePreviewPane
        :entry="previewFileEntry"
        :plugin="previewPlugin"
        :repo-id="activeRepoId ?? ''"
        :thumbnail-src="thumbnailSrc"
        :is-video-entry="isVideoEntry"
        :is-audio-entry="isAudioEntry"
        :hardlink-state-label="hardlinkStateLabel"
        :status-label="statusLabel"
        @back="exitPreview"
        @open="openWorkspaceEntry"
        @reveal="revealWorkspaceEntry"
        @thumbnail-error="markThumbnailFailed"
      />
    </template>

    <FileBrowserPanel
      v-else
      v-model:create-file-name="createFileName"
      v-model:file-display-mode="fileDisplayMode"
      v-model:rename-value="renameValue"
      :breadcrumbs="breadcrumbSegments"
      :can-drag-entries="canDragEntries"
      :can-delete-selected="canDeleteSelected"
      :can-preview-selected="canPreviewSelected"
      :can-rename-selected="canRenameSelected"
      :can-restore-selected="canRestoreSelected"
      :current-file-entry="currentFileEntry"
      :directory-entries="directoryEntries"
      :display-mode-class="fileDisplayModeClass"
      :display-mode-options="fileDisplayModeOptions"
      :entry-deleted-at-label="entryDeletedAtLabel"
      :entry-modified-at-label="entryModifiedAtLabel"
      :error="error"
      :file-entries="fileEntries"
      :file-entry-context-menu="fileEntryContextMenu"
      :file-item-style="fileItemStyle"
      :file-tone="fileTone"
      :hardlink-state-label="hardlinkStateLabel"
      :has-split-file-groups="hasSplitFileGroups"
      :is-audio-entry="isAudioEntry"
      :is-dragging-files="isDraggingFiles"
      :is-loading-file-browser="isLoadingFileBrowser"
      :is-model-entry="isModelEntry"
      :is-mutating-files="isMutatingFiles"
      :is-trash-panel="isTrashPanel"
      :is-video-entry="isVideoEntry"
      :rename-target-path="renameTargetPath"
      :selected-file-path="selectedFilePath"
      :status-label="statusLabel"
      :thumbnail-src="thumbnailSrc"
      @create-file="handleCreateFile"
      @delete-selected="deleteSelectedEntry"
      @drag-leave="handleDragLeave"
      @drag-over="handleDragOver"
      @drop="handleDrop"
      @empty-trash="handleEmptyTrash"
      @entry-drag-start="handleEntryDragStart"
      @mark-thumbnail-failed="markThumbnailFailed"
      @open-directory="openDirectory"
      @open-selected="openSelectedEntry"
      @preview-file="previewFileEntryByDoubleClick"
      @restore-all-trash="handleRestoreAllTrash"
      @restore-selected="restoreSelectedEntry"
      @reveal-selected="revealSelectedEntry"
      @select-entry="selectFileEntry"
      @start-rename="startRenameSelected"
      @submit-rename="submitRenameSelected"
      @thumbnail-loaded="updateThumbnailAspectRatio"
    />
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

  <section v-else-if="isExtensionsPanel" class="extensions-workbench">
    <PluginManagerPanel
      title="文件系统与插件"
      eyebrow="拓展能力"
      subline="这里集中展示当前插件和后端能力。"
      search-placeholder="筛选导入器、脚本或元数据拓展"
      empty-title="没有匹配的插件"
      empty-description="试试其他关键词，或从压缩包安装新的插件。"
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
</template>
