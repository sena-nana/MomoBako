import { computed, ref, watch, type ComputedRef } from "vue";
import { getPreviewPluginForEntry } from "../../plugins/previewPlugins";
import { getRegisteredLibraryExtensionsForEntry, listRegisteredLibraryExtensions } from "../../plugins/sdk";
import type {
  AssetDetail,
  FileBrowserEntry,
  FileBrowserSnapshot,
  PlaylistDetail,
  RepositorySnapshot,
  SearchHit,
  SmartFolderResultSnapshot,
} from "../../types/repository";
import type { WorkspaceLibraryCategoryKey, WorkspacePanelKey } from "../../composables/useRepositoryWorkspace";

export type FileDisplayMode = "adaptive" | "masonry" | "grid" | "list";

type WorkspaceViewStateOptions = {
  activeAssetDetail: ComputedRef<AssetDetail | null>;
  activeLibraryCategory: ComputedRef<WorkspaceLibraryCategoryKey>;
  activeLibraryCategoryLabel: ComputedRef<string>;
  activePanel: ComputedRef<WorkspacePanelKey>;
  activePlaylistDetail: ComputedRef<PlaylistDetail | null>;
  activePreviewPath: ComputedRef<string | null>;
  activeRepositoryStatus: ComputedRef<string | null | undefined>;
  activeSnapshot: ComputedRef<RepositorySnapshot | null>;
  directoryEntries: ComputedRef<FileBrowserEntry[]>;
  fileBrowser: ComputedRef<FileBrowserSnapshot | null>;
  fileBrowserEntryMap: ComputedRef<ReadonlyMap<string, FileBrowserEntry>>;
  fileEntries: ComputedRef<FileBrowserEntry[]>;
  hasMultipleSelection: ComputedRef<boolean>;
  hasSplitFileGroups: ComputedRef<boolean>;
  playlistPreviewEntryMap: ComputedRef<ReadonlyMap<string, FileBrowserEntry>>;
  searchResults: ComputedRef<SearchHit[]>;
  selectedEntries: ComputedRef<FileBrowserEntry[]>;
  selectedEntry: ComputedRef<FileBrowserEntry | null>;
  selectedFilePath: ComputedRef<string | null>;
  smartFolderResult: ComputedRef<SmartFolderResultSnapshot | null>;
  isLibraryCategoryVirtualView: ComputedRef<boolean>;
  isLoadingFileBrowser: ComputedRef<boolean>;
  isLoadingSmartFolder: ComputedRef<boolean>;
  libraryCategorySummary: ComputedRef<string>;
};

export const fileDisplayModeStorageKey = "momobako.fileDisplayMode";
export const fileDisplayModeOptions: Array<{ value: FileDisplayMode; label: string }> = [
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

function withActiveAssetMetadata(entry: FileBrowserEntry | null, assetDetail: AssetDetail | null) {
  if (!entry || !assetDetail || entry.assetId !== assetDetail.summary.assetId) return entry;
  return {
    ...entry,
    metadata: {
      ...(entry.metadata ?? {}),
      ...Object.fromEntries(assetDetail.metadata.map((item) => [item.key, item.value])),
    },
  };
}

export function useWorkspaceViewState(options: WorkspaceViewStateOptions) {
  const previewFilePath = ref<string | null>(null);
  const fileDisplayMode = ref<FileDisplayMode>(readInitialFileDisplayMode());

  const hasRepository = computed(() => Boolean(options.activeSnapshot.value));
  const isMissingRepository = computed(() => options.activeRepositoryStatus.value === "missing");
  const isFilesPanel = computed(() => options.activePanel.value === "files");
  const isTrashPanel = computed(() => options.activePanel.value === "trash");
  const isSearchPanel = computed(() => options.activePanel.value === "search");
  const isSmartFolderPanel = computed(() => options.activePanel.value === "smartFolder");
  const isActionsPanel = computed(() => options.activePanel.value === "actions");
  const isExtensionsPanel = computed(() => options.activePanel.value === "extensions");
  const isPlaylistPanel = computed(() => options.activePanel.value === "playlist");
  const isLibraryCategoryView = computed(() => (
    isFilesPanel.value && options.isLibraryCategoryVirtualView.value
  ));
  const isVirtualView = computed(() => isSmartFolderPanel.value || isLibraryCategoryView.value);
  const isFileBrowserPanel = computed(() => isFilesPanel.value || isTrashPanel.value || isSmartFolderPanel.value);
  const smartFolderEntryMap = computed<ReadonlyMap<string, FileBrowserEntry>>(() => (
    new Map((options.smartFolderResult.value?.results ?? []).map((entry) => [entry.path, entry]))
  ));
  const currentFileEntry = computed(() => {
    if (isSmartFolderPanel.value) {
      return withActiveAssetMetadata(
        options.selectedFilePath.value ? smartFolderEntryMap.value.get(options.selectedFilePath.value) ?? null : null,
        options.activeAssetDetail.value,
      );
    }
    return withActiveAssetMetadata(options.selectedEntry.value, options.activeAssetDetail.value);
  });
  const isRepositoryWritable = computed(() => (
    hasRepository.value
    && !isMissingRepository.value
    && Boolean(options.activeSnapshot.value?.repository.backend.capabilities.includes("write"))
  ));
  const canRenameSelected = computed(() => options.selectedEntries.value.length === 1 && isRepositoryWritable.value && !isTrashPanel.value && !isSmartFolderPanel.value);
  const canOpenSelected = computed(() => options.selectedEntries.value.length === 1 && !isMissingRepository.value && !isTrashPanel.value);
  const canDeleteSelected = computed(() => options.selectedEntries.value.length > 0 && isRepositoryWritable.value && !isSmartFolderPanel.value);
  const canRestoreSelected = computed(() => options.selectedEntries.value.length > 0 && isRepositoryWritable.value && isTrashPanel.value);
  const canDragEntries = computed(() => isRepositoryWritable.value && !isTrashPanel.value && !isSmartFolderPanel.value && options.fileBrowser.value?.backendKind === "filesystem");
  const openSelectedLabel = computed(() => currentFileEntry.value?.kind === "directory" ? "进入" : "查看");
  const previewFileEntry = computed(() => {
    if (!previewFilePath.value) return null;
    const activeEntryMap = isSmartFolderPanel.value ? smartFolderEntryMap.value : options.fileBrowserEntryMap.value;
    return activeEntryMap.get(previewFilePath.value)
      ?? options.fileBrowserEntryMap.value.get(previewFilePath.value)
      ?? smartFolderEntryMap.value.get(previewFilePath.value)
      ?? options.playlistPreviewEntryMap.value.get(previewFilePath.value)
      ?? null;
  });
  const previewPlugin = computed(() => getPreviewPluginForEntry(previewFileEntry.value));
  const libraryExtensions = computed(() => listRegisteredLibraryExtensions());
  const previewLibraryExtensions = computed(() => getRegisteredLibraryExtensionsForEntry(previewFileEntry.value));
  const currentLibraryExtensions = computed(() => getRegisteredLibraryExtensionsForEntry(currentFileEntry.value));
  const fileDisplayModeClass = computed(() => `files-list__files--${fileDisplayMode.value}`);
  const activeDirectoryEntries = computed(() => (isVirtualView.value ? [] : options.directoryEntries.value));
  const activeFileEntries = computed(() => (isSmartFolderPanel.value ? options.smartFolderResult.value?.results ?? [] : options.fileEntries.value));
  const hasActiveSplitFileGroups = computed(() => (
    isVirtualView.value ? false : options.hasSplitFileGroups.value
  ));
  const isActiveBrowserLoading = computed(() => (
    isSmartFolderPanel.value ? options.isLoadingSmartFolder.value : options.isLoadingFileBrowser.value
  ));
  const smartFolderSummary = computed(() => {
    if (!options.smartFolderResult.value) return "";
    const filter = options.smartFolderResult.value.inheritedFilter;
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
    return `${options.smartFolderResult.value.results.length} 条结果${parts.length ? ` · ${parts.join(" · ")}` : ""}`;
  });
  const activeLibrarySearchShortcuts = computed(() => {
    const entries = isSmartFolderPanel.value ? options.smartFolderResult.value?.results ?? [] : options.fileEntries.value;
    const results = options.searchResults.value;
    return libraryExtensions.value.flatMap((extension) => {
      if (!extension.searchShortcuts?.length) return [];
      const hasMatches = entries.some((entry) => extension.matchEntry(entry))
        || results.some((result) => extension.matchEntry(searchHitToFileEntry(result)));
      return hasMatches ? extension.searchShortcuts.map((shortcut) => ({ extension, shortcut })) : [];
    });
  });

  watch(fileDisplayMode, (mode) => {
    try {
      localStorage.setItem(fileDisplayModeStorageKey, mode);
    } catch {
      return;
    }
  });

  watch(options.selectedFilePath, (path) => {
    if (!previewFilePath.value) return;
    if (options.activePreviewPath.value === previewFilePath.value) return;
    if (previewFilePath.value !== path) {
      previewFilePath.value = null;
    }
  });

  watch(options.activePreviewPath, (path) => {
    previewFilePath.value = path;
  });

  watch(options.hasMultipleSelection, (multiple) => {
    if (multiple) {
      previewFilePath.value = null;
    }
  });

  function setPreviewFilePath(path: string | null) {
    previewFilePath.value = path;
  }

  return {
    activeLibraryCategory: options.activeLibraryCategory,
    activeLibraryCategoryLabel: options.activeLibraryCategoryLabel,
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
    isVirtualView,
    isMissingRepository,
    isPlaylistPanel,
    isReadOnlyVirtualView: isSmartFolderPanel,
    isRepositoryWritable,
    isSearchPanel,
    isSmartFolderPanel,
    isTrashPanel,
    openSelectedLabel,
    previewFileEntry,
    previewFilePath,
    previewLibraryExtensions,
    previewPlugin,
    setPreviewFilePath,
    virtualViewSummary: computed(() => (
      isSmartFolderPanel.value ? smartFolderSummary.value : options.libraryCategorySummary.value
    )),
    virtualViewTitle: computed(() => (
      isSmartFolderPanel.value
        ? options.smartFolderResult.value?.smartFolder.name ?? "智能文件夹"
        : options.activeLibraryCategoryLabel.value
    )),
    smartFolderSummary,
  };
}
