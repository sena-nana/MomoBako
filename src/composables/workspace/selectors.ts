import { computed, type ComputedRef } from "vue";
import type { FileBrowserEntry, RepositoryBackendOption } from "../../types/repository";
import {
  activeRepoId,
  activeSnapshot,
  currentDirectoryPath,
  fileBrowser,
  filters,
  plugins,
  repositories,
  selectedFilePath,
} from "./state";

export type WorkspaceSelectors = {
  activeRepository: ComputedRef<(typeof repositories.value)[number] | null>;
  fileBrowserEntryMap: ComputedRef<ReadonlyMap<string, FileBrowserEntry>>;
  selectedEntry: ComputedRef<FileBrowserEntry | null>;
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
  new Map((fileBrowser.value?.entries ?? []).map((entry) => [entry.path, entry]))
));

export const selectedEntry = computed(() => {
  const path = selectedFilePath.value;
  return path ? fileBrowserEntryMap.value.get(path) ?? null : null;
});

export const directoryEntries = computed(() => (
  (fileBrowser.value?.entries ?? []).filter((entry) => entry.kind === "directory")
));

export const fileEntries = computed(() => (
  (fileBrowser.value?.entries ?? []).filter((entry) => entry.kind === "file")
));

export const hasSplitFileGroups = computed(() => (
  directoryEntries.value.length > 0 && fileEntries.value.length > 0
));

export const libraryOverview = computed(() => activeSnapshot.value?.overview ?? null);

export const activeFilterCount = computed(() => (
  filters.value.tags.length +
  filters.value.formats.length +
  filters.value.colors.length +
  filters.value.shapes.length +
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
    .filter((plugin) => ["filesystem", "webdav", "cloud"].includes(plugin.kind))
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
