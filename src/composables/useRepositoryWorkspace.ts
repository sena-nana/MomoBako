import { computed, ref } from "vue";
import {
  attachRepositoryFolder,
  createDirectory,
  createFile,
  createRepository,
  deleteEntry,
  deleteRepository,
  exportRepository,
  getApiDesignSnapshot,
  getAssetDetail,
  getCacheSnapshot,
  getFileBrowser,
  getRepositorySnapshot,
  importRepository,
  listPlugins,
  listRepositories,
  openRepositoryPath,
  redoLastRevision,
  renameEntry,
  revealRepositoryPath,
  searchAssets,
  syncRepository,
  undoLastRevision,
  updateAssetMetadata,
} from "../services/repositoryApi";
import type {
  ApiDesignSnapshot,
  AssetDetail,
  CacheSnapshot,
  FileBrowserSnapshot,
  FileDeleteMode,
  PluginManifest,
  RepositoryBackendOption,
  RepositorySnapshot,
  RepositorySummary,
  SearchHit,
  SearchRequest,
  SyncResult,
} from "../types/repository";

export type WorkspacePanelKey = "libraries" | "files" | "search" | "extensions";

const repositories = ref<RepositorySummary[]>([]);
const activeRepoId = ref<string | null>(null);
const activeSnapshot = ref<RepositorySnapshot | null>(null);
const activeAssetId = ref<string | null>(null);
const activeAssetDetail = ref<AssetDetail | null>(null);
const activePanel = ref<WorkspacePanelKey>("libraries");
const currentDirectoryPath = ref("");
const fileBrowser = ref<FileBrowserSnapshot | null>(null);
const selectedFilePath = ref<string | null>(null);
const searchQuery = ref("");
const searchResults = ref<SearchHit[]>([]);
const lastSyncResult = ref<SyncResult | null>(null);
const plugins = ref<PluginManifest[]>([]);
const cacheSnapshot = ref<CacheSnapshot | null>(null);
const apiDesign = ref<ApiDesignSnapshot | null>(null);
const isLoadingRepositories = ref(false);
const isLoadingSnapshot = ref(false);
const isLoadingAssetDetail = ref(false);
const isLoadingFileBrowser = ref(false);
const isSearching = ref(false);
const isSavingMetadata = ref(false);
const isSyncing = ref(false);
const isMutatingFiles = ref(false);
const isLoadingSettingsData = ref(false);
const error = ref<string | null>(null);
const bootstrapped = ref(false);

function repositoryBackendOptionsFromPlugins(items: PluginManifest[]): RepositoryBackendOption[] {
  return items
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

async function loadRepositories() {
  isLoadingRepositories.value = true;
  error.value = null;

  try {
    const items = await listRepositories();
    repositories.value = items;

    if (!items.length) {
      activeRepoId.value = null;
      activeSnapshot.value = null;
      activeAssetId.value = null;
      activeAssetDetail.value = null;
      fileBrowser.value = null;
      currentDirectoryPath.value = "";
      selectedFilePath.value = null;
      return;
    }

    const nextRepoId = activeRepoId.value && items.some((item) => item.repoId === activeRepoId.value)
      ? activeRepoId.value
      : items[0].repoId;

    await selectRepository(nextRepoId);
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
  } finally {
    isLoadingRepositories.value = false;
  }
}

export async function selectRepository(repoId: string) {
  if (!repoId) return;

  isLoadingSnapshot.value = true;
  error.value = null;

  try {
    const snapshot = await getRepositorySnapshot(repoId);
    activeRepoId.value = repoId;
    activeSnapshot.value = snapshot;

    const defaultAssetId = activeAssetId.value && snapshot.assets.some((item) => item.assetId === activeAssetId.value)
      ? activeAssetId.value
      : snapshot.assets[0]?.assetId ?? null;

    activeAssetId.value = defaultAssetId;
    if (defaultAssetId) {
      await selectAsset(defaultAssetId);
    } else {
      activeAssetDetail.value = null;
    }

    currentDirectoryPath.value = "";
    await loadFileBrowserForDirectory("");
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
  } finally {
    isLoadingSnapshot.value = false;
  }
}

export async function selectAsset(assetId: string) {
  if (!assetId || !activeRepoId.value) return;

  isLoadingAssetDetail.value = true;
  error.value = null;

  try {
    activeAssetId.value = assetId;
    activeAssetDetail.value = await getAssetDetail(activeRepoId.value, assetId);
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
  } finally {
    isLoadingAssetDetail.value = false;
  }
}

export async function loadFileBrowserForDirectory(directoryPath = "") {
  if (!activeRepoId.value) return null;

  isLoadingFileBrowser.value = true;
  error.value = null;
  try {
    const snapshot = await getFileBrowser({
      repoId: activeRepoId.value,
      directoryPath,
    });
    fileBrowser.value = snapshot;
    currentDirectoryPath.value = snapshot.currentPath;

    const hasCurrentSelection = selectedFilePath.value
      && snapshot.entries.some((entry) => entry.path === selectedFilePath.value);
    const defaultSelection = snapshot.entries.find((entry) => entry.kind === "file")?.path
      ?? snapshot.entries[0]?.path
      ?? null;
    selectedFilePath.value = hasCurrentSelection ? selectedFilePath.value : defaultSelection;
    return snapshot;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    return null;
  } finally {
    isLoadingFileBrowser.value = false;
  }
}

export async function createDirectoryInWorkspace(name: string, parentPath = currentDirectoryPath.value) {
  if (!activeRepoId.value) return null;
  isMutatingFiles.value = true;
  error.value = null;
  try {
    const snapshot = await createDirectory({
      repoId: activeRepoId.value,
      parentPath,
      name,
    });
    fileBrowser.value = snapshot;
    currentDirectoryPath.value = snapshot.currentPath;
    await refreshRepositorySnapshot(activeRepoId.value);
    return snapshot;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    return null;
  } finally {
    isMutatingFiles.value = false;
  }
}

export async function createFileInWorkspace(name: string, parentPath = currentDirectoryPath.value) {
  if (!activeRepoId.value) return null;
  isMutatingFiles.value = true;
  error.value = null;
  try {
    const snapshot = await createFile({
      repoId: activeRepoId.value,
      parentPath,
      name,
    });
    fileBrowser.value = snapshot;
    currentDirectoryPath.value = snapshot.currentPath;
    selectedFilePath.value = snapshot.entries.find((entry) => entry.name === name)?.path ?? selectedFilePath.value;
    await refreshRepositorySnapshot(activeRepoId.value);
    return snapshot;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    return null;
  } finally {
    isMutatingFiles.value = false;
  }
}

export async function renameWorkspaceEntry(path: string, newName: string) {
  if (!activeRepoId.value) return null;
  isMutatingFiles.value = true;
  error.value = null;
  try {
    const snapshot = await renameEntry({
      repoId: activeRepoId.value,
      path,
      newName,
    });
    fileBrowser.value = snapshot;
    currentDirectoryPath.value = snapshot.currentPath;
    selectedFilePath.value = snapshot.entries.find((entry) => entry.name === newName)?.path ?? selectedFilePath.value;
    await refreshRepositorySnapshot(activeRepoId.value);
    return snapshot;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    return null;
  } finally {
    isMutatingFiles.value = false;
  }
}

export async function deleteWorkspaceEntry(path: string, mode?: FileDeleteMode) {
  if (!activeRepoId.value) return null;
  isMutatingFiles.value = true;
  error.value = null;
  try {
    const snapshot = await deleteEntry({
      repoId: activeRepoId.value,
      path,
      mode,
    });
    fileBrowser.value = snapshot;
    currentDirectoryPath.value = snapshot.currentPath;
    if (selectedFilePath.value === path) {
      selectedFilePath.value = snapshot.entries.find((entry) => entry.kind === "file")?.path
        ?? snapshot.entries[0]?.path
        ?? null;
    }
    await refreshRepositorySnapshot(activeRepoId.value);
    return snapshot;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    return null;
  } finally {
    isMutatingFiles.value = false;
  }
}

export async function openWorkspaceEntry(path: string) {
  if (!activeSnapshot.value) return;
  const absolutePath = joinAbsolutePath(activeSnapshot.value.repository.path, path);
  await openRepositoryPath(absolutePath);
}

export async function revealWorkspaceEntry(path: string) {
  if (!activeSnapshot.value) return;
  const absolutePath = joinAbsolutePath(activeSnapshot.value.repository.path, path);
  await revealRepositoryPath(absolutePath);
}

export function selectWorkspaceEntry(path: string) {
  selectedFilePath.value = path;
}

export function setActivePanel(panel: WorkspacePanelKey) {
  activePanel.value = panel;
}

export async function runSearch(request: SearchRequest) {
  searchQuery.value = request.query;
  if (!request.query.trim() && !request.tag && !request.metadataKey && request.minRating == null) {
    searchResults.value = [];
    return;
  }

  isSearching.value = true;
  error.value = null;

  try {
    const response = await searchAssets(request);
    searchResults.value = response.results;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
  } finally {
    isSearching.value = false;
  }
}

export async function saveAssetMetadata(metadata: Record<string, unknown>) {
  if (!activeRepoId.value || !activeAssetDetail.value) return null;

  isSavingMetadata.value = true;
  error.value = null;

  try {
    const response = await updateAssetMetadata({
      repoId: activeRepoId.value,
      assetId: activeAssetDetail.value.summary.assetId,
      expectedVersion: activeAssetDetail.value.summary.version,
      metadata,
      source: "desktop",
    });

    applyAssetResponse(response);
    return response;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    return null;
  } finally {
    isSavingMetadata.value = false;
  }
}

export async function syncActiveRepository() {
  if (!activeRepoId.value) return null;

  isSyncing.value = true;
  error.value = null;

  try {
    const result = await syncRepository({ repoId: activeRepoId.value });
    lastSyncResult.value = result;
    await loadRepositories();
    if (activePanel.value === "files") {
      await loadFileBrowserForDirectory(currentDirectoryPath.value);
    }
    return result;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    return null;
  } finally {
    isSyncing.value = false;
  }
}

export async function undoAssetRevision() {
  if (!activeRepoId.value || !activeAssetId.value) return null;

  try {
    const response = await undoLastRevision({
      repoId: activeRepoId.value,
      assetId: activeAssetId.value,
    });
    applyAssetResponse(response);
    return response;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    return null;
  }
}

export async function redoAssetRevision() {
  if (!activeRepoId.value || !activeAssetId.value) return null;

  try {
    const response = await redoLastRevision({
      repoId: activeRepoId.value,
      assetId: activeAssetId.value,
    });
    applyAssetResponse(response);
    return response;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    return null;
  }
}

export async function createNewRepository(
  name: string,
  path: string,
  backendPluginId?: string,
  backendConfig?: Record<string, unknown>,
) {
  await createRepository({ name, path, backendPluginId, backendConfig });
  await loadRepositories();
}

export async function importExistingRepository(name: string, path: string) {
  await importRepository({ name, path });
  await loadRepositories();
}

export async function attachRepository(path: string) {
  await attachRepositoryFolder({ path });
  await loadRepositories();
}

export async function removeRepository(repoId: string) {
  await deleteRepository(repoId);
  await loadRepositories();
}

export async function exportCurrentRepository() {
  if (!activeRepoId.value) return null;
  return exportRepository(activeRepoId.value);
}

export async function loadSettingsData() {
  isLoadingSettingsData.value = true;

  try {
    const [pluginItems, cache, api] = await Promise.all([
      listPlugins(),
      getCacheSnapshot(),
      getApiDesignSnapshot(),
    ]);
    plugins.value = pluginItems;
    cacheSnapshot.value = cache;
    apiDesign.value = api;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
  } finally {
    isLoadingSettingsData.value = false;
  }
}

export function getRepositoryBackendOptions() {
  return repositoryBackendOptionsFromPlugins(plugins.value);
}

function applyAssetResponse(response: { asset: AssetDetail }) {
  activeAssetDetail.value = response.asset;
  activeAssetId.value = response.asset.summary.assetId;

  if (!activeSnapshot.value) return;

  activeSnapshot.value = {
    ...activeSnapshot.value,
    assets: activeSnapshot.value.assets.map((asset) => (
      asset.assetId === response.asset.summary.assetId ? response.asset.summary : asset
    )),
    recentRevisionCount: activeSnapshot.value.recentRevisionCount + 1,
  };
}

async function refreshRepositorySnapshot(repoId: string) {
  const snapshot = await getRepositorySnapshot(repoId);
  activeSnapshot.value = snapshot;
}

function joinAbsolutePath(rootPath: string, relativePath: string) {
  if (!relativePath) return rootPath;
  const normalizedRoot = rootPath.replace(/[\\/]+$/, "");
  return `${normalizedRoot}/${relativePath}`;
}

export async function ensureRepositoryWorkspace() {
  if (bootstrapped.value) return;
  bootstrapped.value = true;
  await Promise.all([loadRepositories(), loadSettingsData()]);
}

export function refreshRepositoryWorkspace() {
  return loadRepositories();
}

export function useRepositoryWorkspace() {
  return {
    repositories: computed(() => repositories.value),
    activeRepoId: computed(() => activeRepoId.value),
    activeSnapshot: computed(() => activeSnapshot.value),
    activeAssetId: computed(() => activeAssetId.value),
    activeAssetDetail: computed(() => activeAssetDetail.value),
    activePanel: computed(() => activePanel.value),
    currentDirectoryPath: computed(() => currentDirectoryPath.value),
    fileBrowser: computed(() => fileBrowser.value),
    selectedFilePath: computed(() => selectedFilePath.value),
    searchQuery: computed(() => searchQuery.value),
    searchResults: computed(() => searchResults.value),
    lastSyncResult: computed(() => lastSyncResult.value),
    plugins: computed(() => plugins.value),
    repositoryBackendOptions: computed(() => getRepositoryBackendOptions()),
    cacheSnapshot: computed(() => cacheSnapshot.value),
    apiDesign: computed(() => apiDesign.value),
    isLoadingRepositories: computed(() => isLoadingRepositories.value),
    isLoadingSnapshot: computed(() => isLoadingSnapshot.value),
    isLoadingAssetDetail: computed(() => isLoadingAssetDetail.value),
    isLoadingFileBrowser: computed(() => isLoadingFileBrowser.value),
    isSearching: computed(() => isSearching.value),
    isSavingMetadata: computed(() => isSavingMetadata.value),
    isSyncing: computed(() => isSyncing.value),
    isMutatingFiles: computed(() => isMutatingFiles.value),
    isLoadingSettingsData: computed(() => isLoadingSettingsData.value),
    isBusy: computed(() => (
      isLoadingRepositories.value ||
      isLoadingSnapshot.value ||
      isLoadingAssetDetail.value
    )),
    error: computed(() => error.value),
    ensureRepositoryWorkspace,
    refreshRepositoryWorkspace,
    selectRepository,
    selectAsset,
    loadFileBrowserForDirectory,
    createDirectoryInWorkspace,
    createFileInWorkspace,
    renameWorkspaceEntry,
    deleteWorkspaceEntry,
    openWorkspaceEntry,
    revealWorkspaceEntry,
    selectWorkspaceEntry,
    setActivePanel,
    runSearch,
    saveAssetMetadata,
    syncActiveRepository,
    undoAssetRevision,
    redoAssetRevision,
    createNewRepository,
    importExistingRepository,
    attachRepository,
    removeRepository,
    exportCurrentRepository,
    loadSettingsData,
  };
}
