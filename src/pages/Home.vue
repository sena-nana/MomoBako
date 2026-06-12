<script setup lang="ts">
import { computed, nextTick, onUnmounted, ref, watch } from "vue";
import {
  AlertTriangle,
  X,
} from "lucide-vue-next";
import ConfirmDialog from "../components/ConfirmDialog.vue";
import { useRepositoryWorkspace } from "../composables/useRepositoryWorkspace";
import { getPreviewPluginForEntry } from "../plugins/previewPlugins";
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

const {
  activePanel,
  activeAssetId,
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
  startWorkspaceEntriesDrag,
  selectWorkspaceEntry,
  selectWorkspaceEntries,
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
const isFileBrowserPanel = computed(() => isFilesPanel.value || isTrashPanel.value || isSmartFolderPanel.value);
const smartFolderEntryMap = computed<ReadonlyMap<string, FileBrowserEntry>>(() => (
  new Map((smartFolderResult.value?.results ?? []).map((entry) => [entry.path, entry]))
));
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
const previewFileEntry = computed(() => (
  previewFilePath.value
    ? (isSmartFolderPanel.value ? smartFolderEntryMap.value : fileBrowserEntryMap.value).get(previewFilePath.value) ?? null
    : null
));
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
const ratingFilterOptions = [1, 2, 3, 4, 5];
const {
  colorFilterInput,
  shapeFilterInput,
  excludeQueryInput,
  excludePathPrefixesInput,
  excludeTagsInput,
  excludeFormatsInput,
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
  previewFilePath.value = entry.path;
}

function exitPreview() {
  previewFilePath.value = null;
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
    </template>

    <FileBrowserPanel
      v-else
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
