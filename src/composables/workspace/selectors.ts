import { computed, type ComputedRef } from "vue";
import type { FileBrowserEntry, RepositoryBackendOption } from "../../types/repository";
import { isSourcePlugin } from "../../utils/pluginTaxonomy";
import {
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
  fileBrowserEntryMap: ComputedRef<ReadonlyMap<string, FileBrowserEntry>>;
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

export const fileBrowserEntryMap = computed<ReadonlyMap<string, FileBrowserEntry>>(() => (
  fileBrowserDerived.value.entryMap
));

export const selectedEntry = computed(() => {
  const path = selectedFilePath.value;
  return path ? fileBrowserEntryMap.value.get(path) ?? null : null;
});

export const directoryEntries = computed(() => (
  fileBrowserDerived.value.directories
));

export const fileEntries = computed(() => (
  fileBrowserDerived.value.files
));

export const visibleEntries = computed(() => (
  fileBrowserDerived.value.visibleEntries
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
  directoryEntries.value.length > 0 && fileEntries.value.length > 0
));

export const libraryOverview = computed(() => activeSnapshot.value?.overview ?? null);

export const activeFilterCount = computed(() => (
  filters.value.tags.length +
  filters.value.formats.length +
  filters.value.colors.length +
  filters.value.shapes.length +
  filters.value.excludeTags.length +
  filters.value.excludeFormats.length +
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
      enabled: plugin.enabled,
    }));
}

export const repositoryBackendOptions = computed(() => repositoryBackendOptionsFromPlugins());
