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
  importEntries,
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
  FileTreeNode,
  FileDeleteMode,
  PluginManifest,
  RepositoryBackendOption,
  RepositorySyncProgress,
  RepositorySnapshot,
  RepositorySummary,
  SearchHit,
  SearchRequest,
  SyncResult,
  WorkspaceStartupState,
} from "../types/repository";

export type WorkspacePanelKey = "libraries" | "files" | "search" | "extensions";

const STARTUP_TOTAL_STEPS = 4;
const SYNC_TOTAL_STEPS = 3;

function createInitialWorkspaceStartup(): WorkspaceStartupState {
  return {
    status: "idle",
    stepLabel: "准备加载仓库",
    currentStep: 0,
    totalSteps: STARTUP_TOTAL_STEPS,
    percent: 0,
    error: null,
  };
}

function createInitialSyncProgress(): RepositorySyncProgress {
  return {
    phase: "idle",
    label: "",
    current: 0,
    total: SYNC_TOTAL_STEPS,
    percent: 0,
  };
}

const repositories = ref<RepositorySummary[]>([]);
const activeRepoId = ref<string | null>(null);
const activeSnapshot = ref<RepositorySnapshot | null>(null);
const activeAssetId = ref<string | null>(null);
const activeAssetDetail = ref<AssetDetail | null>(null);
const activePanel = ref<WorkspacePanelKey>("files");
const currentDirectoryPath = ref("");
const fileBrowser = ref<FileBrowserSnapshot | null>(null);
const fileTree = ref<FileTreeNode[]>([]);
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
const workspaceStartup = ref<WorkspaceStartupState>(createInitialWorkspaceStartup());
const syncProgress = ref<RepositorySyncProgress>(createInitialSyncProgress());
let startupPromise: Promise<void> | null = null;

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
      fileTree.value = [];
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

async function refreshRepositorySummaries() {
  const items = await listRepositories();
  repositories.value = items;
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
    await loadFileBrowserForDirectory("", { includeTree: true });
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

type FileBrowserLoadOptions = {
  includeTree?: boolean;
};

function getDefaultFileBrowserSelection(snapshot: FileBrowserSnapshot) {
  return snapshot.entries.find((entry) => entry.kind === "file")?.path
    ?? snapshot.entries[0]?.path
    ?? null;
}

function applyFileBrowserSnapshot(snapshot: FileBrowserSnapshot) {
  fileBrowser.value = snapshot;
  if (snapshot.tree) {
    fileTree.value = snapshot.tree;
  }
  currentDirectoryPath.value = snapshot.currentPath;

  const hasCurrentSelection = selectedFilePath.value
    && snapshot.entries.some((entry) => entry.path === selectedFilePath.value);
  selectedFilePath.value = hasCurrentSelection ? selectedFilePath.value : getDefaultFileBrowserSelection(snapshot);
}

export async function loadFileBrowserForDirectory(directoryPath = "", options: FileBrowserLoadOptions = {}) {
  if (!activeRepoId.value) return null;

  const includeTree = options.includeTree ?? false;
  isLoadingFileBrowser.value = true;
  error.value = null;
  try {
    const snapshot = await getFileBrowser({
      repoId: activeRepoId.value,
      directoryPath,
      includeTree,
    });
    applyFileBrowserSnapshot(snapshot);
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
    applyFileBrowserSnapshot(snapshot);
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
    applyFileBrowserSnapshot(snapshot);
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

export async function importEntriesToWorkspace(sourcePaths: string[], parentPath = currentDirectoryPath.value) {
  if (!activeRepoId.value || !sourcePaths.length) return null;
  isMutatingFiles.value = true;
  error.value = null;
  try {
    const snapshot = await importEntries({
      repoId: activeRepoId.value,
      parentPath,
      sourcePaths,
    });
    applyFileBrowserSnapshot(snapshot);
    selectedFilePath.value = snapshot.entries.find((entry) => sourcePaths.some((sourcePath) => (
      sourcePath.replace(/\\/g, "/").endsWith(`/${entry.name}`) || sourcePath.replace(/\\/g, "/") === entry.name
    )))?.path ?? selectedFilePath.value;
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
    applyFileBrowserSnapshot(snapshot);
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
    const shouldSelectDefault = selectedFilePath.value === path;
    applyFileBrowserSnapshot(snapshot);
    if (shouldSelectDefault) {
      selectedFilePath.value = getDefaultFileBrowserSelection(snapshot);
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
  error.value = null;
  try {
    await openRepositoryPath(absolutePath);
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
  }
}

function resetWorkspaceSelection() {
  activeRepoId.value = null;
  activeSnapshot.value = null;
  activeAssetId.value = null;
  activeAssetDetail.value = null;
  fileBrowser.value = null;
  fileTree.value = [];
  currentDirectoryPath.value = "";
  selectedFilePath.value = null;
}

function setStartupProgress(currentStep: number, stepLabel: string) {
  const totalSteps = workspaceStartup.value.totalSteps || 4;
  workspaceStartup.value = {
    status: "loading",
    stepLabel,
    currentStep,
    totalSteps,
    percent: Math.round((currentStep / totalSteps) * 100),
    error: null,
  };
}

function setSyncProgress(
  phase: RepositorySyncProgress["phase"],
  label: string,
  current: number,
  total = SYNC_TOTAL_STEPS,
) {
  syncProgress.value = {
    phase,
    label,
    current,
    total,
    percent: Math.round((current / total) * 100),
  };
}

function setStartupLoadingFlags(value: boolean) {
  isLoadingRepositories.value = value;
  isLoadingSnapshot.value = value;
  isLoadingFileBrowser.value = value;
  isLoadingSettingsData.value = value;
}

async function loadInitialRepository(items: RepositorySummary[]) {
  if (!items.length) {
    resetWorkspaceSelection();
    return;
  }

  const nextRepoId = activeRepoId.value && items.some((item) => item.repoId === activeRepoId.value)
    ? activeRepoId.value
    : items[0].repoId;

  setStartupProgress(2, "读取仓库摘要");
  const snapshot = await getRepositorySnapshot(nextRepoId);
  activeRepoId.value = nextRepoId;
  activeSnapshot.value = snapshot;

  const defaultAssetId = activeAssetId.value && snapshot.assets.some((item) => item.assetId === activeAssetId.value)
    ? activeAssetId.value
    : snapshot.assets[0]?.assetId ?? null;

  activeAssetId.value = defaultAssetId;
  if (defaultAssetId) {
    activeAssetDetail.value = await getAssetDetail(nextRepoId, defaultAssetId);
  } else {
    activeAssetDetail.value = null;
  }

  setStartupProgress(3, "读取首屏目录");
  currentDirectoryPath.value = "";
  const browserSnapshot = await getFileBrowser({
    repoId: nextRepoId,
    directoryPath: "",
    includeTree: true,
  });
  applyFileBrowserSnapshot(browserSnapshot);
}

export async function revealWorkspaceEntry(path: string) {
  if (!activeSnapshot.value) return;
  const absolutePath = joinAbsolutePath(activeSnapshot.value.repository.path, path);
  error.value = null;
  try {
    await revealRepositoryPath(absolutePath);
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
  }
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
  setSyncProgress("scanning", "扫描仓库文件", 1);

  try {
    const previousDirectoryPath = currentDirectoryPath.value;
    const result = await syncRepository({ repoId: activeRepoId.value });
    setSyncProgress("writing", "写入索引结果", 2);
    lastSyncResult.value = result;
    await refreshRepositorySnapshot(activeRepoId.value);
    await refreshRepositorySummaries();
    setSyncProgress("refreshing", "刷新仓库视图", 3);
    if (activePanel.value === "files") {
      await loadFileBrowserForDirectory(previousDirectoryPath, { includeTree: true });
    }
    setSyncProgress("complete", "同步完成", 3);
    return result;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    setSyncProgress("error", error.value, 3);
    return null;
  } finally {
    isSyncing.value = false;
  }
}

export async function refreshFileBrowserTree() {
  if (!activeRepoId.value) return null;

  isLoadingFileBrowser.value = true;
  error.value = null;
  setSyncProgress("scanning", "扫描文件夹结构", 1);
  try {
    const result = await syncRepository({ repoId: activeRepoId.value });
    setSyncProgress("writing", "写入索引结果", 2);
    lastSyncResult.value = result;
    await refreshRepositorySnapshot(activeRepoId.value);
    setSyncProgress("refreshing", "刷新文件夹树", 3);
    const snapshot = await loadFileBrowserForDirectory(currentDirectoryPath.value, { includeTree: true });
    setSyncProgress("complete", "刷新完成", 3);
    return snapshot;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    setSyncProgress("error", error.value, 3);
    return null;
  } finally {
    isLoadingFileBrowser.value = false;
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

type SettingsDataLoadOptions = {
  failFast?: boolean;
};

export async function loadSettingsData(options: SettingsDataLoadOptions = {}) {
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
    if (options.failFast) {
      throw cause;
    }
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
  const normalizedRoot = trimTrailingPathSeparators(rootPath);
  const normalizedRelative = relativePath
    .trim()
    .replace(/^[\\/]+|[\\/]+$/g, "")
    .split(/[\\/]+/)
    .filter(Boolean);
  if (!normalizedRelative.length) return normalizedRoot;

  const separator = normalizedRoot.includes("\\") ? "\\" : "/";
  if (/^[A-Za-z]:[\\/]$/.test(normalizedRoot)) {
    return `${normalizedRoot}${normalizedRelative.join(separator)}`;
  }
  return `${normalizedRoot}${separator}${normalizedRelative.join(separator)}`;
}

function trimTrailingPathSeparators(path: string) {
  const trimmed = path.trim();
  if (/^[A-Za-z]:[\\/]$/.test(trimmed)) return trimmed;
  return trimmed.replace(/[\\/]+$/, "") || trimmed;
}

export function ensureRepositoryWorkspace() {
  if (workspaceStartup.value.status === "ready") return;
  if (startupPromise) return startupPromise;

  startupPromise = (async () => {
    workspaceStartup.value = { ...createInitialWorkspaceStartup(), status: "loading" };
    error.value = null;
    setStartupLoadingFlags(true);

    try {
      setStartupProgress(1, "加载仓库列表");
      const items = await listRepositories();
      repositories.value = items;

      await loadInitialRepository(items);

      setStartupProgress(4, "加载插件与设置");
      await loadSettingsData({ failFast: true });

      workspaceStartup.value = {
        status: "ready",
        stepLabel: "加载完成",
        currentStep: STARTUP_TOTAL_STEPS,
        totalSteps: STARTUP_TOTAL_STEPS,
        percent: 100,
        error: null,
      };
    } catch (cause) {
      const message = cause instanceof Error ? cause.message : String(cause);
      error.value = message;
      workspaceStartup.value = {
        ...workspaceStartup.value,
        status: "error",
        stepLabel: "加载失败",
        error: message,
      };
    } finally {
      setStartupLoadingFlags(false);
      startupPromise = null;
    }
  })();

  return startupPromise;
}

export function refreshRepositoryWorkspace() {
  return loadRepositories();
}

export function resetRepositoryWorkspaceForTests() {
  repositories.value = [];
  resetWorkspaceSelection();
  activePanel.value = "files";
  searchQuery.value = "";
  searchResults.value = [];
  lastSyncResult.value = null;
  plugins.value = [];
  cacheSnapshot.value = null;
  apiDesign.value = null;
  error.value = null;
  isLoadingRepositories.value = false;
  isLoadingSnapshot.value = false;
  isLoadingAssetDetail.value = false;
  isLoadingFileBrowser.value = false;
  isSearching.value = false;
  isSavingMetadata.value = false;
  isSyncing.value = false;
  isMutatingFiles.value = false;
  isLoadingSettingsData.value = false;
  workspaceStartup.value = createInitialWorkspaceStartup();
  syncProgress.value = createInitialSyncProgress();
  startupPromise = null;
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
    fileTree: computed(() => fileTree.value),
    selectedFilePath: computed(() => selectedFilePath.value),
    searchQuery: computed(() => searchQuery.value),
    searchResults: computed(() => searchResults.value),
    lastSyncResult: computed(() => lastSyncResult.value),
    plugins: computed(() => plugins.value),
    repositoryBackendOptions: computed(() => getRepositoryBackendOptions()),
    cacheSnapshot: computed(() => cacheSnapshot.value),
    apiDesign: computed(() => apiDesign.value),
    workspaceStartup: computed(() => workspaceStartup.value),
    syncProgress: computed(() => syncProgress.value),
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
    refreshFileBrowserTree,
    createDirectoryInWorkspace,
    createFileInWorkspace,
    importEntriesToWorkspace,
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
