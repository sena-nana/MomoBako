<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { convertFileSrc } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { UnlistenFn } from "@tauri-apps/api/event";
import {
  ArrowLeft,
  Archive,
  Eye,
  File,
  FileAudio,
  FileImage,
  FileVideo,
  Folder,
  FolderOpen,
  GitBranch,
  LoaderCircle,
  PencilLine,
  Plus,
  HardDrive,
  Files,
  Download,
  ImagePlus,
  ImageOff,
  Clipboard,
  Copy,
  RefreshCw,
  RotateCcw,
  Search,
  Trash2,
  X,
} from "lucide-vue-next";
import Markdown from "vue3-markdown-it";
import { useRepositoryWorkspace } from "../composables/useRepositoryWorkspace";
import { vContextMenu } from "../directives/contextMenu";
import { getPreviewPluginForEntry } from "../plugins/previewPlugins";
import { isAudioExtension, isVideoExtension } from "../plugins/mediaPreview/mediaExtensions";
import type {
  FileBrowserEntry,
  HardlinkCandidate,
  RepositoryArchiveFormat,
  RepositoryCompressionLevel,
  RepositorySummary,
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
const extensionKeyword = ref("");
const exportDialogRepository = ref<RepositorySummary | null>(null);
const exportTarget = ref<"archive" | "git">("archive");
const exportArchiveFormat = ref<RepositoryArchiveFormat>("zip");
const exportCompression = ref<RepositoryCompressionLevel>("balanced");
const exportEncrypt = ref(false);
const exportPassword = ref("");
const exportGitRemote = ref("origin");
const exportGitBranch = ref("");
const exportGitMessage = ref("");
const exportDialogError = ref("");
const isExporting = ref(false);
const failedThumbnailPaths = ref<Set<string>>(new Set());
const fileDisplayMode = ref<FileDisplayMode>(readInitialFileDisplayMode());
const thumbnailAspectRatios = ref<Record<string, number>>({});
const pendingCopySourcePaths = ref<string[]>([]);
const copyTargetDialogOpen = ref(false);
const copyTargetPath = ref("");
const skippedHardlinkCandidateIds = ref<Set<string>>(new Set());

const {
  activePanel,
  activeSnapshot,
  activeRepoId,
  fileBrowser,
  plugins,
  repositories,
  searchQuery,
  selectedFilePath,
  searchResults,
  hardlinkCandidates,
  isBusy,
  isLoadingFileBrowser,
  isSearching,
  isMutatingFiles,
  error,
  refreshRepositoryWorkspace,
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
  selectWorkspaceEntry,
  setActivePanel,
  setWorkspaceEntryThumbnail,
  setWorkspaceEntryThumbnailFromBytes,
  clearWorkspaceEntryThumbnail,
  refreshWorkspaceEntryThumbnail,
  removeRepository,
  exportCurrentRepository,
  confirmWorkspaceHardlinkCandidate,
} = useRepositoryWorkspace();

const hasRepository = computed(() => Boolean(activeSnapshot.value));
const isLibrariesPanel = computed(() => activePanel.value === "libraries");
const isFilesPanel = computed(() => activePanel.value === "files");
const isTrashPanel = computed(() => activePanel.value === "deleted");
const isSearchPanel = computed(() => activePanel.value === "search");
const isExtensionsPanel = computed(() => activePanel.value === "extensions");
const isFileBrowserPanel = computed(() => isFilesPanel.value || isTrashPanel.value);

const currentFileEntry = computed(() => (
  fileBrowser.value?.entries.find((entry) => entry.path === selectedFilePath.value) ?? null
));

const breadcrumbSegments = computed(() => {
  const currentPath = fileBrowser.value?.currentPath ?? "";
  const segments = currentPath ? currentPath.split("/") : [];
  return segments.map((segment, index) => ({
    label: segment,
    path: segments.slice(0, index + 1).join("/"),
  }));
});

const canRenameSelected = computed(() => Boolean(currentFileEntry.value) && !isTrashPanel.value);
const canPreviewSelected = computed(() => currentFileEntry.value?.kind === "file" && !isTrashPanel.value);
const canDeleteSelected = computed(() => Boolean(currentFileEntry.value));
const canRestoreSelected = computed(() => Boolean(currentFileEntry.value) && isTrashPanel.value);
const libraryOverview = computed(() => activeSnapshot.value?.overview ?? null);
const directoryEntries = computed(() => (fileBrowser.value?.entries ?? []).filter((entry) => entry.kind === "directory"));
const fileEntries = computed(() => (fileBrowser.value?.entries ?? []).filter((entry) => entry.kind === "file"));
const previewFileEntry = computed(() => (
  (fileBrowser.value?.entries ?? []).find((entry) => entry.path === previewFilePath.value && entry.kind === "file")
  ?? null
));
const previewPlugin = computed(() => getPreviewPluginForEntry(previewFileEntry.value));
const hasSplitFileGroups = computed(() => directoryEntries.value.length > 0 && fileEntries.value.length > 0);
const fileDisplayModeClass = computed(() => `files-list__files--${fileDisplayMode.value}`);
const currentHardlinkCandidate = computed(() => (
  hardlinkCandidates.value.find((candidate) => !skippedHardlinkCandidateIds.value.has(candidate.candidateId)) ?? null
));
const filteredPlugins = computed(() => {
  const keyword = extensionKeyword.value.trim().toLowerCase();
  if (!keyword) return plugins.value;
  return plugins.value.filter((plugin) => (
    plugin.name.toLowerCase().includes(keyword) ||
    plugin.description.toLowerCase().includes(keyword) ||
    plugin.kind.toLowerCase().includes(keyword) ||
    plugin.capabilities.some((capability) => capability.toLowerCase().includes(keyword))
  ));
});
const exportArchiveExtension = computed(() => {
  if (exportArchiveFormat.value === "tar" && exportCompression.value !== "none") {
    return "tar.gz";
  }
  return exportArchiveFormat.value === "7z" ? "7z" : exportArchiveFormat.value;
});
const exportArchiveFilterExtension = computed(() => (
  exportArchiveFormat.value === "tar" && exportCompression.value !== "none"
    ? "gz"
    : exportArchiveExtension.value
));
const exportActionLabel = computed(() => exportTarget.value === "git" ? "上传到 Git" : "导出压缩包");

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

function resetThumbnailFailure(path: string) {
  const next = new Set(failedThumbnailPaths.value);
  next.delete(path);
  failedThumbnailPaths.value = next;
}

async function chooseCustomThumbnail(entry: FileBrowserEntry) {
  if (isTrashPanel.value) return;
  const selected = await open({
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

function formatRepositoryStatus(status: string) {
  switch (status) {
    case "ready":
      return "已同步";
    case "readonly":
      return "只读";
    case "indexing":
      return "处理中";
    default:
      return status;
  }
}

function sanitizeExportName(value: string) {
  return value.trim().replace(/[<>:"/\\|?*\u0000-\u001F]/g, "-") || "momobako-repository";
}

function closeExportDialog(force = false) {
  if (isExporting.value && !force) return;
  exportDialogRepository.value = null;
  exportDialogError.value = "";
  exportPassword.value = "";
}

async function requestRepositoryExport(library: RepositorySummary) {
  exportDialogError.value = "";
  if (activeRepoId.value !== library.repoId) {
    await selectRepository(library.repoId);
  }
  if (activeRepoId.value !== library.repoId) return;
  exportDialogRepository.value = repositories.value.find((item) => item.repoId === library.repoId) ?? library;
}

async function chooseArchiveOutputPath(repository: RepositorySummary) {
  const extension = exportArchiveExtension.value;
  return save({
    title: "导出资源库",
    defaultPath: `${sanitizeExportName(repository.name)}.${extension}`,
    filters: [
      {
        name: extension.toUpperCase(),
        extensions: [exportArchiveFilterExtension.value],
      },
    ],
  });
}

async function submitExportDialog() {
  const repository = exportDialogRepository.value;
  if (!repository) return;

  exportDialogError.value = "";
  isExporting.value = true;

  try {
    if (exportTarget.value === "archive") {
      if (exportEncrypt.value && !exportPassword.value.trim()) {
        exportDialogError.value = "请输入加密密码。";
        return;
      }

      const outputPath = await chooseArchiveOutputPath(repository);
      if (!outputPath) return;

      const response = await exportCurrentRepository({
        target: "archive",
        archive: {
          format: exportArchiveFormat.value,
          outputPath,
          compression: exportCompression.value,
          encrypt: exportEncrypt.value,
          password: exportEncrypt.value ? exportPassword.value : undefined,
        },
      });
      if (response) {
        closeExportDialog(true);
      } else {
        exportDialogError.value = error.value ?? "资源库导出失败。";
      }
      return;
    }

    const response = await exportCurrentRepository({
      target: "git",
      git: {
        remote: exportGitRemote.value.trim() || undefined,
        branch: exportGitBranch.value.trim() || undefined,
        message: exportGitMessage.value.trim() || undefined,
      },
    });
    if (response) {
      closeExportDialog(true);
    } else {
      exportDialogError.value = error.value ?? "Git 上传失败。";
    }
  } finally {
    isExporting.value = false;
  }
}

function getAnchorFromElement(element: EventTarget | null) {
  if (!(element instanceof HTMLElement)) return null;
  const rect = element.getBoundingClientRect();
  return {
    left: rect.left,
    top: rect.top,
    right: rect.right,
    bottom: rect.bottom,
  };
}

function requestAddRepository(event?: MouseEvent) {
  window.dispatchEvent(new CustomEvent("momo:add-repository", {
    detail: {
      anchor: getAnchorFromElement(event?.currentTarget ?? null),
    },
  }));
}

const searchSummary = computed(() => {
  if (searchQuery.value.trim()) {
    return `当前查询: ${searchQuery.value}`;
  }
  return "输入关键词、标签或评分条件后，这里会展示跨仓库结果。";
});

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

let dragDropUnlisten: UnlistenFn | null = null;

onUnmounted(() => {
  dragDropUnlisten?.();
});
</script>

<template>
  <section v-if="hasRepository && isLibrariesPanel" class="library-overview">
    <div class="library-overview__panel">
      <header class="library-overview__header">
        <div>
          <p class="asset-browser__eyebrow">当前资源库</p>
          <h1>{{ activeSnapshot?.repository.name ?? "正在加载" }}</h1>
          <p class="library-overview__subline">
            {{ activeSnapshot?.repository.path }}
          </p>
        </div>
        <div class="library-overview__actions">
          <button type="button" class="ghost" @click="refreshRepositoryWorkspace">
            <RefreshCw :size="14" aria-hidden="true" />
            刷新资源库
          </button>
          <button type="button" class="primary" @click="requestAddRepository">
            <Plus :size="14" aria-hidden="true" />
            添加资源库
          </button>
        </div>
      </header>

      <div v-if="error" class="asset-browser__state asset-browser__state--error">
        {{ error }}
      </div>

      <div v-else-if="isBusy" class="asset-browser__state">
        <LoaderCircle class="spin" :size="16" aria-hidden="true" />
        正在加载资源库
      </div>

      <template v-else>
        <div class="library-overview__stats">
          <article class="library-overview__stat-card">
            <span class="library-overview__stat-label">仓库名称</span>
            <strong>{{ activeSnapshot?.repository.name }}</strong>
          </article>
          <article class="library-overview__stat-card">
            <span class="library-overview__stat-label">总大小</span>
            <strong>{{ libraryOverview?.totalSizeLabel ?? "0 B" }}</strong>
            <span class="library-overview__stat-meta">
              <HardDrive :size="13" aria-hidden="true" />
              {{ libraryOverview?.totalSizeBytes ?? 0 }} Bytes
            </span>
          </article>
          <article class="library-overview__stat-card">
            <span class="library-overview__stat-label">文件个数</span>
            <strong>{{ libraryOverview?.fileCount ?? 0 }}</strong>
            <span class="library-overview__stat-meta">
              <Files :size="13" aria-hidden="true" />
              已索引文件
            </span>
          </article>
          <article class="library-overview__stat-card">
            <span class="library-overview__stat-label">文件夹个数</span>
            <strong>{{ libraryOverview?.folderCount ?? 0 }}</strong>
            <span class="library-overview__stat-meta">
              <Folder :size="13" aria-hidden="true" />
              不含内部元数据目录
            </span>
          </article>
        </div>

        <section class="library-overview__readme">
          <div class="library-overview__section-head">
            <div>
              <p class="asset-browser__eyebrow">README</p>
              <h2>根目录说明</h2>
            </div>
          </div>

          <div v-if="libraryOverview?.readmeContent" class="library-overview__readme-card">
            <Markdown :source="libraryOverview.readmeContent" />
          </div>
          <div v-else class="library-overview__empty">
            <h2>未发现 `readme.md`</h2>
            <p>如果资源库根目录存在 `readme.md` 或 `README.md`，这里会直接展示其内容。</p>
          </div>
        </section>

        <section class="library-manager">
          <div class="library-overview__section-head">
            <div>
              <p class="asset-browser__eyebrow">Repositories</p>
              <h2>资源库管理</h2>
            </div>
          </div>

          <div class="library-manager__list">
            <article
              v-for="library in repositories"
              :key="library.repoId"
              class="library-manager__item"
              :class="{ 'is-active': activeRepoId === library.repoId }"
            >
              <button
                type="button"
                class="library-manager__summary"
                @click="selectRepository(library.repoId)"
              >
                <div class="library-manager__title">
                  <strong>{{ library.name }}</strong>
                  <span>{{ library.assetCount }} 个资源</span>
                </div>
                <span class="library-manager__meta">{{ formatRepositoryStatus(library.status) }}</span>
                <span class="library-manager__path">{{ library.path }}</span>
              </button>

              <div class="library-manager__actions">
                <button
                  type="button"
                  class="ghost"
                  @click="requestRepositoryExport(library)"
                >
                  <Download :size="14" aria-hidden="true" />
                  导出
                </button>
                <button
                  type="button"
                  class="ghost danger"
                  @click="removeRepository(library.repoId)"
                >
                  <Trash2 :size="14" aria-hidden="true" />
                  删除
                </button>
              </div>
            </article>
          </div>
        </section>
      </template>
    </div>
  </section>

  <section v-else-if="hasRepository && isFileBrowserPanel" :class="previewFileEntry ? 'files-preview-page' : 'files-workbench'">
    <template v-if="previewFileEntry">
      <header class="files-preview-page__header">
        <button type="button" class="ghost files-preview-page__back" @click="exitPreview">
          <ArrowLeft :size="15" aria-hidden="true" />
          返回
        </button>
        <div>
          <p class="asset-browser__eyebrow">文件预览</p>
          <h1>{{ previewFileEntry.name }}</h1>
          <p class="files-preview-page__subline">{{ previewFileEntry.path }}</p>
        </div>
        <div class="files-preview-page__actions">
          <button type="button" class="ghost" @click="openWorkspaceEntry(previewFileEntry.path)">
            <Eye :size="14" aria-hidden="true" />
            打开
          </button>
          <button type="button" class="ghost" @click="revealWorkspaceEntry(previewFileEntry.path)">
            <FolderOpen :size="14" aria-hidden="true" />
            定位
          </button>
        </div>
      </header>

      <div class="files-preview-page__body">
        <div class="files-preview-page__preview" :class="{ 'files-preview-page__preview--plugin': previewPlugin }">
          <component
            :is="previewPlugin.component"
            v-if="previewPlugin"
            :entry="previewFileEntry"
            :repo-id="activeRepoId ?? ''"
          />
          <img v-else-if="thumbnailSrc(previewFileEntry)" :src="thumbnailSrc(previewFileEntry) ?? undefined" alt="" @error="markThumbnailFailed(previewFileEntry)" />
          <FileVideo v-else-if="isVideoEntry(previewFileEntry)" :size="54" aria-hidden="true" />
          <FileAudio v-else-if="isAudioEntry(previewFileEntry)" :size="54" aria-hidden="true" />
          <FileImage v-else :size="54" aria-hidden="true" />
        </div>
        <div class="files-detail__stats files-preview-page__stats">
          <div class="asset-meta__row">
            <span>类型</span>
            <span class="asset-meta__value">{{ previewFileEntry.extension || '文件' }}</span>
          </div>
          <div class="asset-meta__row">
            <span>大小</span>
            <span class="asset-meta__value">{{ previewFileEntry.sizeLabel || "未知" }}</span>
          </div>
          <div class="asset-meta__row">
            <span>状态</span>
            <span class="asset-meta__value">{{ previewFileEntry.status ? statusLabel(previewFileEntry.status) : "未索引" }}</span>
          </div>
          <div v-if="hardlinkStateLabel(previewFileEntry)" class="asset-meta__row">
            <span>硬链接</span>
            <span class="asset-meta__value">{{ hardlinkStateLabel(previewFileEntry) }}</span>
          </div>
          <div class="asset-meta__row">
            <span>修改时间</span>
            <span class="asset-meta__value">{{ previewFileEntry.modifiedAt ? new Date(previewFileEntry.modifiedAt).toLocaleString("zh-CN") : "未记录" }}</span>
          </div>
        </div>
      </div>
    </template>

    <template v-else>
    <div
      class="files-browser"
      :class="{ 'is-dragging': isDraggingFiles }"
      @dragover="handleDragOver"
      @dragleave="handleDragLeave"
      @drop="handleDrop"
    >
      <header class="files-browser__header">
        <div>
          <p class="asset-browser__eyebrow">{{ isTrashPanel ? "回收站" : "当前目录" }}</p>
          <div class="files-breadcrumbs">
            <button type="button" class="files-breadcrumbs__item" @click="openDirectory('')">{{ isTrashPanel ? "回收站" : "根目录" }}</button>
            <button v-for="segment in breadcrumbSegments" :key="segment.path" type="button" class="files-breadcrumbs__item" @click="openDirectory(segment.path)">
              {{ segment.label }}
            </button>
          </div>
        </div>

        <div class="files-toolbar">
          <label class="files-toolbar__select">
            <span>展示方式</span>
            <select v-model="fileDisplayMode" aria-label="素材展示方式">
              <option v-for="option in fileDisplayModeOptions" :key="option.value" :value="option.value">
                {{ option.label }}
              </option>
            </select>
          </label>

          <template v-if="!isTrashPanel">
            <label class="files-toolbar__field">
              <Plus :size="14" aria-hidden="true" />
              <input v-model="createFileName" type="text" placeholder="新建空文件，例如 note.txt" />
            </label>
            <button type="button" class="ghost files-toolbar__btn" :disabled="isMutatingFiles" @click="handleCreateFile">
              <File :size="14" aria-hidden="true" />
              建文件
            </button>
          </template>

          <template v-else>
            <button type="button" class="ghost files-toolbar__btn" :disabled="isMutatingFiles" @click="handleRestoreAllTrash">
              <RotateCcw :size="14" aria-hidden="true" />
              还原所有项目
            </button>
            <button type="button" class="ghost danger files-toolbar__btn" :disabled="isMutatingFiles" @click="handleEmptyTrash">
              <Trash2 :size="14" aria-hidden="true" />
              清空回收站
            </button>
          </template>
        </div>
      </header>

      <div v-if="error" class="asset-browser__state asset-browser__state--error">
        {{ error }}
      </div>

      <div v-else-if="isLoadingFileBrowser" class="asset-browser__state">
        <LoaderCircle class="spin" :size="16" aria-hidden="true" />
        正在读取目录
      </div>

      <div v-else-if="isMutatingFiles" class="asset-browser__state">
        <LoaderCircle class="spin" :size="16" aria-hidden="true" />
        正在处理文件
      </div>

      <template v-else>
        <div class="files-list">
          <button
            v-for="entry in directoryEntries"
            :key="entry.path"
            v-context-menu="() => fileEntryContextMenu(entry)"
            type="button"
            class="files-list__item"
            :class="{ 'is-active': selectedFilePath === entry.path }"
            @click="selectFileEntry(entry)"
          >
            <div class="files-list__preview" :style="{ background: fileTone(entry) }">
              <Folder :size="24" aria-hidden="true" />
            </div>
            <div class="files-list__body">
              <strong>{{ entry.name }}</strong>
            </div>
          </button>

          <div v-if="hasSplitFileGroups" class="files-list__divider" aria-hidden="true"></div>

          <div class="files-list__files" :class="fileDisplayModeClass">
            <button
              v-for="entry in fileEntries"
              :key="entry.path"
              v-context-menu="() => fileEntryContextMenu(entry)"
              type="button"
              class="files-list__item files-list__item--file"
              :class="{ 'is-active': selectedFilePath === entry.path }"
              :style="fileItemStyle(entry)"
              @click="selectFileEntry(entry)"
              @dblclick="previewFileEntryByDoubleClick(entry)"
            >
              <div class="files-list__preview">
                <img v-if="thumbnailSrc(entry)" :src="thumbnailSrc(entry) ?? undefined" alt="" loading="lazy" @load="updateThumbnailAspectRatio(entry, $event)" @error="markThumbnailFailed(entry)" />
                <FileVideo v-else-if="isVideoEntry(entry)" :size="24" aria-hidden="true" />
                <FileAudio v-else-if="isAudioEntry(entry)" :size="24" aria-hidden="true" />
                <File v-else-if="isModelEntry(entry)" :size="24" aria-hidden="true" />
                <FileImage v-else :size="24" aria-hidden="true" />
              </div>
              <div class="files-list__body">
                <strong>{{ entry.name }}</strong>
                <span v-if="hardlinkStateLabel(entry) && fileDisplayMode !== 'list'">{{ hardlinkStateLabel(entry) }}</span>
                <span v-if="fileDisplayMode === 'list'">{{ entry.path }}</span>
              </div>
              <div v-if="fileDisplayMode === 'list'" class="files-list__meta">
                <span>{{ entry.extension || '文件' }}</span>
                <span>{{ entry.sizeLabel || "未知" }}</span>
                <span>{{ entry.status ? statusLabel(entry.status) : "未索引" }}</span>
                <span v-if="hardlinkStateLabel(entry)">{{ hardlinkStateLabel(entry) }}</span>
                <span>{{ entryModifiedAtLabel(entry) }}</span>
              </div>
            </button>
          </div>
        </div>
      </template>
    </div>

    <aside class="files-detail">
      <div v-if="currentFileEntry" class="files-detail__card">
        <div class="files-detail__preview" :style="{ background: currentFileEntry.kind === 'directory' ? fileTone(currentFileEntry) : undefined }">
          <img v-if="thumbnailSrc(currentFileEntry)" :src="thumbnailSrc(currentFileEntry) ?? undefined" alt="" @error="markThumbnailFailed(currentFileEntry)" />
          <Folder v-else-if="currentFileEntry.kind === 'directory'" :size="34" aria-hidden="true" />
          <FileVideo v-else-if="isVideoEntry(currentFileEntry)" :size="34" aria-hidden="true" />
          <FileAudio v-else-if="isAudioEntry(currentFileEntry)" :size="34" aria-hidden="true" />
          <File v-else-if="isModelEntry(currentFileEntry)" :size="34" aria-hidden="true" />
          <FileImage v-else :size="34" aria-hidden="true" />
        </div>

        <div class="files-detail__section">
          <p class="asset-browser__eyebrow">选中项</p>
          <h2>{{ currentFileEntry.name }}</h2>
          <p class="files-detail__subline">{{ currentFileEntry.path }}</p>
        </div>

        <div class="files-detail__section">
          <div class="files-detail__actions">
            <button v-if="isTrashPanel" type="button" class="ghost" :disabled="isMutatingFiles || !canRestoreSelected" @click="restoreSelectedEntry">
              <RotateCcw :size="14" aria-hidden="true" />
              还原
            </button>
            <button type="button" class="ghost" :disabled="!canPreviewSelected" @click="openSelectedEntry">
              <Eye :size="14" aria-hidden="true" />
              查看
            </button>
            <button type="button" class="ghost" :disabled="isTrashPanel" @click="revealSelectedEntry">
              <FolderOpen :size="14" aria-hidden="true" />
              定位
            </button>
            <button type="button" class="ghost" :disabled="!canRenameSelected" @click="startRenameSelected">
              <PencilLine :size="14" aria-hidden="true" />
              重命名
            </button>
            <button type="button" class="ghost danger" :disabled="isMutatingFiles || !canDeleteSelected" @click="deleteSelectedEntry">
              <File :size="14" aria-hidden="true" />
              {{ isTrashPanel ? "彻底删除" : "删除" }}
            </button>
          </div>
          <p v-if="isTrashPanel" class="files-detail__hint">
            回收站中的删除会直接从文件系统移除。
          </p>
        </div>

        <div v-if="renameTargetPath === currentFileEntry.path" class="files-detail__section">
          <p class="asset-browser__eyebrow">重命名</p>
          <div class="files-detail__rename">
            <input v-model="renameValue" type="text" />
            <button type="button" :disabled="isMutatingFiles" @click="submitRenameSelected">
              <PencilLine :size="14" aria-hidden="true" />
              保存
            </button>
          </div>
        </div>

        <div class="files-detail__stats">
          <div class="asset-meta__row">
            <span>类型</span>
            <span class="asset-meta__value">{{ currentFileEntry.kind === 'directory' ? '文件夹' : currentFileEntry.extension || '文件' }}</span>
          </div>
          <div class="asset-meta__row">
            <span>大小</span>
            <span class="asset-meta__value">{{ currentFileEntry.sizeLabel || "目录项" }}</span>
          </div>
          <div class="asset-meta__row">
            <span>状态</span>
            <span class="asset-meta__value">{{ currentFileEntry.status ? statusLabel(currentFileEntry.status) : "未索引" }}</span>
          </div>
          <div v-if="hardlinkStateLabel(currentFileEntry)" class="asset-meta__row">
            <span>硬链接</span>
            <span class="asset-meta__value">{{ hardlinkStateLabel(currentFileEntry) }}</span>
          </div>
          <div class="asset-meta__row">
            <span>修改时间</span>
            <span class="asset-meta__value">{{ currentFileEntry.modifiedAt ? new Date(currentFileEntry.modifiedAt).toLocaleString("zh-CN") : "未记录" }}</span>
          </div>
          <div v-if="isTrashPanel" class="asset-meta__row">
            <span>删除时间</span>
            <span class="asset-meta__value">{{ entryDeletedAtLabel(currentFileEntry) || "未记录" }}</span>
          </div>
        </div>
      </div>

      <div v-else class="files-detail__empty">
        <p class="asset-browser__eyebrow">{{ isTrashPanel ? "回收站" : "文件管理" }}</p>
        <h2>选择一个文件或文件夹</h2>
        <p>{{ isTrashPanel ? "在中间列表中选择目标，然后可执行还原或彻底删除。" : "在中间列表中选择目标，然后可执行查看、定位、重命名和删除。" }}</p>
      </div>
    </aside>
    </template>
  </section>

  <section v-else-if="isSearchPanel" class="search-workbench">
    <div class="search-workbench__panel">
      <header class="search-workbench__header">
        <div>
          <p class="asset-browser__eyebrow">全局搜索</p>
          <h1>搜索结果</h1>
          <p class="search-workbench__subline">{{ searchSummary }}</p>
        </div>
        <div class="search-workbench__stats">
          <span class="asset-stat">{{ repositories.length }} 个仓库</span>
          <span class="asset-stat">{{ searchResults.length }} 条结果</span>
        </div>
      </header>

      <div v-if="isSearching" class="asset-browser__state">
        <LoaderCircle class="spin" :size="16" aria-hidden="true" />
        正在执行全局搜索
      </div>

      <div v-if="!repositories.length" class="search-workbench__empty">
        <h2>还没有可搜索的资源库</h2>
        <p>先在资源库页面添加一个仓库，再执行跨仓库搜索。</p>
      </div>

      <div v-else-if="!isSearching && !searchResults.length" class="search-workbench__empty">
        <h2>等待搜索条件</h2>
        <p>输入关键词、标签或评分条件后，这里会展示结果。</p>
      </div>

      <div v-else class="search-workbench__results">
        <button
          v-for="result in searchResults"
          :key="`${result.repoId}:${result.assetId}`"
          type="button"
          class="search-workbench__item"
          @click="openSearchHit(result)"
        >
          <div class="search-workbench__item-icon">
            <FileImage :size="18" aria-hidden="true" />
          </div>
          <div class="search-workbench__item-body">
            <strong>{{ result.filename }}</strong>
            <span>{{ result.repoName }} / {{ result.path }}</span>
          </div>
        </button>
      </div>
    </div>
  </section>

  <section v-else-if="isExtensionsPanel" class="extensions-workbench">
    <div class="search-workbench__panel">
      <header class="search-workbench__header">
        <div>
          <p class="asset-browser__eyebrow">拓展能力</p>
          <h1>文件系统与插件</h1>
          <p class="search-workbench__subline">这里集中展示当前插件和后端能力。</p>
        </div>
        <div class="search-workbench__stats">
          <span class="asset-stat">{{ filteredPlugins.length }} 个插件</span>
        </div>
      </header>

      <label class="search-workbench__field">
        <Search :size="15" aria-hidden="true" />
        <input
          v-model="extensionKeyword"
          type="search"
          placeholder="筛选导入器、脚本或元数据拓展"
        />
      </label>

      <div class="extensions-workbench__list">
        <article v-for="plugin in filteredPlugins" :key="plugin.pluginId" class="extensions-workbench__card">
          <div class="extensions-workbench__card-head">
            <strong>{{ plugin.name }}</strong>
            <span class="asset-card__pill" :class="{ 'asset-card__pill--ghost': !plugin.enabled }">
              {{ plugin.enabled ? "已启用" : "未启用" }}
            </span>
          </div>
          <p class="extensions-workbench__card-desc">{{ plugin.description }}</p>
          <div class="settings-list__chips">
            <span class="workspace-hints__chip">{{ plugin.kind }}</span>
            <span v-for="capability in plugin.capabilities" :key="capability" class="workspace-hints__chip">
              {{ capability }}
            </span>
          </div>
        </article>
      </div>
    </div>
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

  <Teleport to="body">
    <Transition name="modal">
      <div
        v-if="copyTargetDialogOpen"
        class="modal-overlay"
        role="dialog"
        aria-modal="true"
        aria-label="复制到文件夹"
        @click.self="cancelCopyTarget()"
      >
        <div class="modal-card dialog-card copy-target-dialog">
          <div class="dialog-card__header">
            <Copy :size="14" aria-hidden="true" />
            <span>复制到文件夹</span>
          </div>
          <div class="dialog-card__body copy-target-dialog__body">
            <label class="dialog-field">
              <span>目标目录</span>
              <input v-model="copyTargetPath" type="text" placeholder="留空表示根目录" />
            </label>
          </div>
          <div class="dialog-card__actions">
            <button type="button" class="ghost" :disabled="isMutatingFiles" @click="cancelCopyTarget()">
              取消
            </button>
            <button type="button" class="primary" :disabled="isMutatingFiles" @click="submitCopyTarget()">
              {{ isMutatingFiles ? "处理中..." : "复制" }}
            </button>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>

  <Teleport to="body">
    <Transition name="modal">
      <div
        v-if="currentHardlinkCandidate"
        class="modal-overlay"
        role="dialog"
        aria-modal="true"
        aria-label="加入硬链接关联"
        @click.self="skipCurrentHardlinkCandidate()"
      >
        <div class="modal-card dialog-card hardlink-candidate-dialog">
          <div class="dialog-card__header">
            <Copy :size="14" aria-hidden="true" />
            <span>加入硬链接关联</span>
          </div>
          <div class="dialog-card__body hardlink-candidate-dialog__body">
            <p>{{ hardlinkCandidateMessage(currentHardlinkCandidate) }}</p>
            <div class="hardlink-candidate-dialog__paths">
              <span>{{ currentHardlinkCandidate.existingPath }}</span>
              <span>{{ currentHardlinkCandidate.newPath }}</span>
            </div>
          </div>
          <div class="dialog-card__actions">
            <button type="button" class="ghost" :disabled="isMutatingFiles" @click="skipCurrentHardlinkCandidate()">
              跳过
            </button>
            <button type="button" class="primary" :disabled="isMutatingFiles" @click="confirmCurrentHardlinkCandidate()">
              {{ isMutatingFiles ? "处理中..." : "加入关联" }}
            </button>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>

  <Teleport to="body">
    <Transition name="modal">
      <div
        v-if="exportDialogRepository"
        class="modal-overlay"
        role="dialog"
        aria-modal="true"
        aria-label="导出资源库"
        @click.self="closeExportDialog()"
      >
        <div class="modal-card dialog-card repository-export-dialog">
          <div class="dialog-card__header">
            <Download :size="14" aria-hidden="true" />
            <span>导出资源库</span>
            <button
              type="button"
              class="repository-export-dialog__close"
              title="关闭"
              aria-label="关闭导出配置"
              :disabled="isExporting"
              @click="closeExportDialog()"
            >
              <X :size="13" aria-hidden="true" />
            </button>
          </div>

          <div class="dialog-card__body repository-export-dialog__body">
            <div class="repository-export-dialog__repo">
              <strong>{{ exportDialogRepository.name }}</strong>
              <span>{{ exportDialogRepository.path }}</span>
            </div>

            <div class="segmented repository-export-dialog__tabs">
              <button
                type="button"
                :class="{ 'is-active': exportTarget === 'archive' }"
                :disabled="isExporting"
                @click="exportTarget = 'archive'"
              >
                <Archive :size="13" aria-hidden="true" />
                压缩包
              </button>
              <button
                type="button"
                :class="{ 'is-active': exportTarget === 'git' }"
                :disabled="isExporting"
                @click="exportTarget = 'git'"
              >
                <GitBranch :size="13" aria-hidden="true" />
                Git
              </button>
            </div>

            <template v-if="exportTarget === 'archive'">
              <div class="repository-export-dialog__grid">
                <label class="dialog-field">
                  <span>格式</span>
                  <select v-model="exportArchiveFormat" :disabled="isExporting">
                    <option value="zip">zip</option>
                    <option value="7z">7z</option>
                    <option value="tar">tar</option>
                  </select>
                </label>

                <label class="dialog-field">
                  <span>压缩</span>
                  <select v-model="exportCompression" :disabled="isExporting">
                    <option value="none">不压缩</option>
                    <option value="fast">快速</option>
                    <option value="balanced">均衡</option>
                    <option value="maximum">最大</option>
                  </select>
                </label>
              </div>

              <label class="repository-export-dialog__toggle">
                <input v-model="exportEncrypt" type="checkbox" :disabled="isExporting" />
                <span>加密压缩包</span>
              </label>

              <label v-if="exportEncrypt" class="dialog-field">
                <span>密码</span>
                <input
                  v-model="exportPassword"
                  type="password"
                  placeholder="用于压缩包加密"
                  :disabled="isExporting"
                  @keydown.enter.prevent="submitExportDialog"
                />
              </label>
            </template>

            <template v-else>
              <div class="repository-export-dialog__grid">
                <label class="dialog-field">
                  <span>远端</span>
                  <input
                    v-model="exportGitRemote"
                    type="text"
                    placeholder="origin"
                    :disabled="isExporting"
                  />
                </label>

                <label class="dialog-field">
                  <span>分支</span>
                  <input
                    v-model="exportGitBranch"
                    type="text"
                    placeholder="默认当前分支"
                    :disabled="isExporting"
                  />
                </label>
              </div>

              <label class="dialog-field">
                <span>提交信息</span>
                <input
                  v-model="exportGitMessage"
                  type="text"
                  placeholder="导出资源库"
                  :disabled="isExporting"
                  @keydown.enter.prevent="submitExportDialog"
                />
              </label>
            </template>

            <p v-if="exportDialogError" class="repository-add-popover__error">
              {{ exportDialogError }}
            </p>
          </div>

          <div class="dialog-card__actions">
            <button type="button" class="ghost" :disabled="isExporting" @click="closeExportDialog()">
              取消
            </button>
            <button type="button" class="primary" :disabled="isExporting" @click="submitExportDialog">
              <LoaderCircle v-if="isExporting" class="spin" :size="13" aria-hidden="true" />
              {{ isExporting ? "处理中" : exportActionLabel }}
            </button>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>
