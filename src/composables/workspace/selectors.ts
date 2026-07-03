import { computed, type ComputedRef } from "vue";
import type { FileBrowserEntry, RepositoryBackendOption } from "../../types/repository";
import { isRepositoryBackendRuntimeAvailable, isSourcePlugin } from "../../utils/pluginTaxonomy";
import {
  activeLibraryCategory,
  activePanel,
  activeRepoId,
  activeSnapshot,
  currentDirectoryPath,
  fileBrowserDerived,
  filters,
  plugins,
  repositories,
  selectedFilePaths,
  selectedFilePath,
} from "./state";

export type WorkspaceSelectors = {
  activeRepository: ComputedRef<(typeof repositories.value)[number] | null>;
  activeLibraryCategoryLabel: ComputedRef<string>;
  fileBrowserEntryMap: ComputedRef<ReadonlyMap<string, FileBrowserEntry>>;
  isLibraryCategoryVirtualView: ComputedRef<boolean>;
  libraryCategorySummary: ComputedRef<string>;
  visibleEntries: ComputedRef<FileBrowserEntry[]>;
  selectedEntry: ComputedRef<FileBrowserEntry | null>;
  selectedEntries: ComputedRef<FileBrowserEntry[]>;
  selectedFilePathSet: ComputedRef<ReadonlySet<string>>;
  hasMultipleSelection: ComputedRef<boolean>;
  directoryEntries: ComputedRef<FileBrowserEntry[]>;
  fileEntries: ComputedRef<FileBrowserEntry[]>;
  hasSplitFileGroups: ComputedRef<boolean>;
  activeFilterCount: ComputedRef<number>;
  hasActiveFilters: ComputedRef<boolean>;
  repositoryBackendOptions: ComputedRef<RepositoryBackendOption[]>;
};

export const activeRepository = computed(() => (
  repositories.value.find((item) => item.repoId === activeRepoId.value) ?? null
));

function compareDescendingDate(left: string | null | undefined, right: string | null | undefined) {
  const leftValue = left ?? "";
  const rightValue = right ?? "";
  return rightValue.localeCompare(leftValue);
}

function assetSummaryToFileEntry(asset: NonNullable<typeof activeSnapshot.value>["assets"][number]): FileBrowserEntry {
  return {
    path: asset.path,
    name: asset.filename,
    kind: "file",
    extension: asset.extension,
    sizeBytes: asset.sizeBytes,
    sizeLabel: asset.sizeLabel,
    modifiedAt: asset.modifiedAt,
    assetId: asset.assetId,
    status: asset.status,
    thumbnailPath: asset.thumbnailPath ?? null,
    hardlinkGroupId: asset.hardlinkGroupId ?? null,
    hardlinkState: asset.hardlinkState ?? null,
    tags: asset.tags,
    aliasPaths: [asset.path],
    metadata: {},
    isVirtual: asset.isVirtual ?? false,
    providerId: asset.providerId ?? null,
    providerItemId: asset.providerItemId ?? null,
    sourcePayload: asset.sourcePayload ?? null,
    localAbsolutePath: asset.localAbsolutePath ?? null,
  };
}

export const activeLibraryCategoryLabel = computed(() => {
  switch (activeLibraryCategory.value) {
    case "uncategorized":
      return "未分类";
    case "untagged":
      return "未标签";
    case "recent":
      return "最近使用";
    default:
      return "全部";
  }
});

export const isLibraryCategoryVirtualView = computed(() => activeLibraryCategory.value !== "all");
const isLibraryCategoryFilesView = computed(() => (
  activePanel.value === "files" && isLibraryCategoryVirtualView.value
));

const activeLibraryAssets = computed(() => (
  (activeSnapshot.value?.assets ?? []).filter((asset) => asset.status !== "deleted")
));

const libraryCategoryEntries = computed<FileBrowserEntry[]>(() => {
  const assets = activeLibraryAssets.value;
  const filteredAssets = activeLibraryCategory.value === "uncategorized"
    ? assets.filter((asset) => !asset.path.includes("/"))
    : activeLibraryCategory.value === "untagged"
      ? assets.filter((asset) => asset.tags.length === 0)
      : activeLibraryCategory.value === "recent"
        ? assets
          .filter((asset) => Boolean(asset.lastAccessedAt))
          .slice()
          .sort((left, right) => (
            compareDescendingDate(left.lastAccessedAt, right.lastAccessedAt)
            || compareDescendingDate(left.modifiedAt, right.modifiedAt)
            || left.filename.localeCompare(right.filename, "zh-CN")
          ))
        : assets;

  return filteredAssets.map(assetSummaryToFileEntry);
});

export const libraryCategorySummary = computed(() => {
  if (activeLibraryCategory.value === "all") return "";
  const count = libraryCategoryEntries.value.length;
  if (activeLibraryCategory.value === "recent") {
    return `按最近访问时间排序，共 ${count} 项。`;
  }
  return `${activeLibraryCategoryLabel.value}共 ${count} 项。`;
});

export const fileBrowserEntryMap = computed<ReadonlyMap<string, FileBrowserEntry>>(() => (
  isLibraryCategoryFilesView.value
    ? new Map(libraryCategoryEntries.value.map((entry) => [entry.path, entry]))
    : fileBrowserDerived.value.entryMap
));

export const selectedEntry = computed(() => {
  const path = selectedFilePath.value;
  return path ? fileBrowserEntryMap.value.get(path) ?? null : null;
});

export const directoryEntries = computed(() => (
  isLibraryCategoryFilesView.value ? [] : fileBrowserDerived.value.directories
));

export const fileEntries = computed(() => (
  isLibraryCategoryFilesView.value ? libraryCategoryEntries.value : fileBrowserDerived.value.files
));

export const visibleEntries = computed(() => (
  isLibraryCategoryFilesView.value ? libraryCategoryEntries.value : fileBrowserDerived.value.visibleEntries
));

export const selectedFilePathSet = computed<ReadonlySet<string>>(() => (
  new Set(selectedFilePaths.value)
));

export const selectedEntries = computed(() => (
  selectedFilePaths.value
    .map((path) => fileBrowserEntryMap.value.get(path) ?? null)
    .filter((entry): entry is FileBrowserEntry => Boolean(entry))
));

export const hasMultipleSelection = computed(() => selectedEntries.value.length > 1);

export const hasSplitFileGroups = computed(() => (
  !isLibraryCategoryFilesView.value && directoryEntries.value.length > 0 && fileEntries.value.length > 0
));

export const libraryOverview = computed(() => activeSnapshot.value?.overview ?? null);

export const activeFilterCount = computed(() => (
  filters.value.tags.length +
  filters.value.formats.length +
  filters.value.colors.length +
  filters.value.shapes.length +
  filters.value.excludeTags.length +
  filters.value.excludeFormats.length +
  (filters.value.metadataFilters.trim() ? 1 : 0) +
  (filters.value.excludeMetadataFilters.trim() ? 1 : 0) +
  (filters.value.numberFilters.trim() ? 1 : 0) +
  (filters.value.dateFilters.trim() ? 1 : 0) +
  (filters.value.matchMode === "or" ? 1 : 0) +
  (filters.value.sortField.trim() ? 1 : 0) +
  (filters.value.limit == null ? 0 : 1) +
  (filters.value.minRating == null ? 0 : 1)
));

export const hasActiveFilters = computed(() => activeFilterCount.value > 0);

export const breadcrumbSegments = computed(() => {
  const currentPath = currentDirectoryPath.value;
  const segments = currentPath ? currentPath.split("/") : [];
  return segments.map((segment, index) => ({
    label: segment,
    path: segments.slice(0, index + 1).join("/"),
  }));
});

export function repositoryBackendOptionsFromPlugins(): RepositoryBackendOption[] {
  return plugins.value
    .filter(isSourcePlugin)
    .map((plugin) => ({
      pluginId: plugin.pluginId,
      kind: plugin.kind,
      name: plugin.name,
      capabilities: plugin.capabilities,
      description: plugin.description,
      enabled: isRepositoryBackendRuntimeAvailable(plugin),
    }));
}

export const repositoryBackendOptions = computed(() => repositoryBackendOptionsFromPlugins());
