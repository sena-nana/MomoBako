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
  RepositoryExportRequest,
  RepositoryExportResponse,
  RepositoryBackendOption,
  RepositorySnapshot,
  RepositorySummary,
  SearchHit,
  SearchRequest,
  SyncResult,
} from "../types/repository";

export type WorkspacePanelKey = "libraries" | "files" | "search" | "extensions";

export type WorkspaceOperationProgress = {
  label: string;
  detail: string;
  value: number;
  indeterminate: boolean;
};

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
const bootstrapped = ref(false);
const operationProgress = ref<WorkspaceOperationProgress | null>(null);
let operationProgressTimer: number | null = null;
let operationProgressId = 0;

function stopOperationProgressTimer() {
  if (operationProgressTimer === null) return;
  window.clearInterval(operationProgressTimer);
  operationProgressTimer = null;
}

function startOperationProgress(
  label: string,
  detail: string,
  options: { initial?: number; ceiling?: number; indeterminate?: boolean } = {},
) {
  const id = ++operationProgressId;
  const ceiling = options.ceiling ?? 88;
  stopOperationProgressTimer();
  operationProgress.value = {
    label,
    detail,
    value: options.initial ?? 8,
    indeterminate: options.indeterminate ?? false,
  };

  operationProgressTimer = window.setInterval(() => {
    if (id !== operationProgressId || !operationProgress.value) return;
    const current = operationProgress.value.value;
    const increment = current < 35 ? 5 : current < 70 ? 3 : 1;
    operationProgress.value = {
      ...operationProgress.value,
      value: Math.min(ceiling, current + increment),
    };
  }, 220);

  return id;
}

function updateOperationProgress(id: number, patch: Partial<WorkspaceOperationProgress>) {
  if (id !== operationProgressId || !operationProgress.value) return;
  operationProgress.value = {
    ...operationProgress.value,
    ...patch,
    value: patch.value == null ? operationProgress.value.value : Math.max(0, Math.min(100, patch.value)),
  };
}

function finishOperationProgress(id: number) {
  if (id !== operationProgressId) return;
  stopOperationProgressTimer();
  if (operationProgress.value) {
    operationProgress.value = {
      ...operationProgress.value,
      value: 100,
      indeterminate: false,
    };
  }
  window.setTimeout(() => {
    if (id === operationProgressId) {
      operationProgress.value = null;
    }
  }, 180);
}

function cancelOperationProgress(id: number) {
  if (id !== operationProgressId) return;
  stopOperationProgressTimer();
  operationProgress.value = null;
}

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
  const progressId = startOperationProgress("加载资源库", "读取已注册资源库", { initial: 12, indeterminate: true });

  try {
    const items = await listRepositories();
    updateOperationProgress(progressId, { detail: "加载资源库摘要", value: 38 });
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
      finishOperationProgress(progressId);
      return;
    }

    const nextRepoId = activeRepoId.value && items.some((item) => item.repoId === activeRepoId.value)
      ? activeRepoId.value
      : items[0].repoId;

    await selectRepository(nextRepoId);
    finishOperationProgress(progressId);
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    cancelOperationProgress(progressId);
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
  const progressId = startOperationProgress("加载资源库", "读取资源库快照", { initial: 10, indeterminate: true });

  try {
    const snapshot = await getRepositorySnapshot(repoId);
    updateOperationProgress(progressId, { detail: "加载资源索引", value: 46 });
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
    finishOperationProgress(progressId);
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    cancelOperationProgress(progressId);
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
  const progressId = startOperationProgress(
    includeTree ? "读取文件树" : "读取目录",
    directoryPath ? `正在读取 ${directoryPath}` : "正在读取根目录",
    { initial: 14, indeterminate: true },
  );
  try {
    const snapshot = await getFileBrowser({
      repoId: activeRepoId.value,
      directoryPath,
      includeTree,
    });
    updateOperationProgress(progressId, { detail: "整理目录条目", value: 92 });
    applyFileBrowserSnapshot(snapshot);
    finishOperationProgress(progressId);
    return snapshot;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    cancelOperationProgress(progressId);
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
  error.value = null;
  const progressId = startOperationProgress(
    "导入文件",
    `准备导入 ${sourcePaths.length} 个条目`,
    { initial: 8 },
  );
  try {
    updateOperationProgress(progressId, { detail: "复制文件到当前资源库", value: 24 });
    const snapshot = await importEntries({
      repoId: activeRepoId.value,
      parentPath,
      sourcePaths,
    });
    updateOperationProgress(progressId, { detail: "刷新文件索引", value: 84 });
    applyFileBrowserSnapshot(snapshot);
    selectedFilePath.value = snapshot.entries.find((entry) => sourcePaths.some((sourcePath) => (
      sourcePath.replace(/\\/g, "/").endsWith(`/${entry.name}`) || sourcePath.replace(/\\/g, "/") === entry.name
    )))?.path ?? selectedFilePath.value;
    await refreshRepositorySnapshot(activeRepoId.value);
    finishOperationProgress(progressId);
    return snapshot;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    cancelOperationProgress(progressId);
    return null;
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
  const progressId = startOperationProgress("同步资源库", "扫描文件变化", { initial: 10 });

  try {
    const previousDirectoryPath = currentDirectoryPath.value;
    const result = await syncRepository({ repoId: activeRepoId.value });
    updateOperationProgress(progressId, {
      detail: `已扫描 ${result.scannedFiles} 个文件`,
      value: 72,
      indeterminate: false,
    });
    lastSyncResult.value = result;
    await refreshRepositorySnapshot(activeRepoId.value);
    await refreshRepositorySummaries();
    if (activePanel.value === "files") {
      await loadFileBrowserForDirectory(previousDirectoryPath, { includeTree: true });
    }
    finishOperationProgress(progressId);
    return result;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    cancelOperationProgress(progressId);
    return null;
  } finally {
    isSyncing.value = false;
  }
}

export async function refreshFileBrowserTree() {
  if (!activeRepoId.value) return null;

  isLoadingFileBrowser.value = true;
  error.value = null;
  const progressId = startOperationProgress("刷新文件树", "同步并读取目录结构", { initial: 12 });
  try {
    const result = await syncRepository({ repoId: activeRepoId.value });
    updateOperationProgress(progressId, { detail: `已扫描 ${result.scannedFiles} 个文件`, value: 58 });
    lastSyncResult.value = result;
    await refreshRepositorySnapshot(activeRepoId.value);
    const snapshot = await loadFileBrowserForDirectory(currentDirectoryPath.value, { includeTree: true });
    finishOperationProgress(progressId);
    return snapshot;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    cancelOperationProgress(progressId);
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
  const progressId = startOperationProgress("创建资源库", "初始化资源库并扫描文件", { initial: 8 });
  try {
    await createRepository({ name, path, backendPluginId, backendConfig });
    await loadRepositories();
    finishOperationProgress(progressId);
  } catch (cause) {
    cancelOperationProgress(progressId);
    throw cause;
  }
}

export async function importExistingRepository(name: string, path: string) {
  const progressId = startOperationProgress("导入资源库", "读取资源库元数据并扫描文件", { initial: 8 });
  try {
    await importRepository({ name, path });
    await loadRepositories();
    finishOperationProgress(progressId);
  } catch (cause) {
    cancelOperationProgress(progressId);
    throw cause;
  }
}

export async function attachRepository(path: string) {
  const progressId = startOperationProgress("挂载资源库", "检查文件夹并读取索引", { initial: 8 });
  try {
    await attachRepositoryFolder({ path });
    await loadRepositories();
    finishOperationProgress(progressId);
  } catch (cause) {
    cancelOperationProgress(progressId);
    throw cause;
  }
}

export async function removeRepository(repoId: string) {
  await deleteRepository(repoId);
  await loadRepositories();
}

export async function exportCurrentRepository(
  request: Omit<RepositoryExportRequest, "repoId">,
): Promise<RepositoryExportResponse | null> {
  if (!activeRepoId.value) return null;

  error.value = null;
  const progressId = startOperationProgress(
    request.target === "git" ? "上传到 Git" : "导出资源库",
    request.target === "git" ? "准备提交并推送资源库" : "准备打包资源库文件",
    { initial: 8 },
  );

  try {
    const response = await exportRepository({
      ...request,
      repoId: activeRepoId.value,
    });
    updateOperationProgress(progressId, { detail: response.message, value: 92 });
    finishOperationProgress(progressId);
    return response;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    cancelOperationProgress(progressId);
    return null;
  }
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
    fileTree: computed(() => fileTree.value),
    selectedFilePath: computed(() => selectedFilePath.value),
    searchQuery: computed(() => searchQuery.value),
    searchResults: computed(() => searchResults.value),
    lastSyncResult: computed(() => lastSyncResult.value),
    plugins: computed(() => plugins.value),
    repositoryBackendOptions: computed(() => getRepositoryBackendOptions()),
    cacheSnapshot: computed(() => cacheSnapshot.value),
    apiDesign: computed(() => apiDesign.value),
    operationProgress: computed(() => operationProgress.value),
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
