import { computed, ref } from "vue";
import {
  attachRepositoryFolder,
  createDirectory,
  createFile,
  createRepository,
  deleteEntry,
  deleteRepository,
  ensureThumbnail,
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
  mutateTrash,
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
  FileBrowserEntry,
  FileBrowserSnapshot,
  FileTreeNode,
  FileDeleteMode,
  PluginManifest,
  RepositoryExportRequest,
  RepositoryExportResponse,
  RepositoryBackendOption,
  RepositorySyncProgress,
  RepositorySnapshot,
  RepositorySummary,
  SearchHit,
  SearchRequest,
  SyncResult,
  ThumbnailResponse,
  WorkspaceStartupState,
} from "../types/repository";

export type WorkspacePanelKey = "libraries" | "files" | "deleted" | "search" | "extensions";

export type WorkspaceOperationProgress = {
  label: string;
  detail: string;
  value: number;
  indeterminate: boolean;
};

const STARTUP_TOTAL_STEPS = 3;
const SYNC_TOTAL_STEPS = 3;
const THUMBNAIL_LOAD_CONCURRENCY = 3;

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
const workspaceStartup = ref<WorkspaceStartupState>(createInitialWorkspaceStartup());
const syncProgress = ref<RepositorySyncProgress>(createInitialSyncProgress());
let startupPromise: Promise<void> | null = null;
let thumbnailLoadToken = 0;

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
    activeAssetDetail.value = null;

    currentDirectoryPath.value = "";
    await loadFileBrowserForDirectory("", { includeTree: true });
    if (defaultAssetId) {
      void selectAsset(defaultAssetId);
    }
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
  specialLocation?: "trash";
};

function getDefaultFileBrowserSelection(snapshot: FileBrowserSnapshot) {
  return snapshot.entries.find((entry) => entry.kind === "file")?.path
    ?? snapshot.entries[0]?.path
    ?? null;
}

function applyFileBrowserSnapshot(snapshot: FileBrowserSnapshot) {
  const displaySnapshot = {
    ...snapshot,
    entries: snapshot.entries.map((entry) => ({ ...entry })),
  };
  fileBrowser.value = displaySnapshot;
  if (displaySnapshot.tree) {
    fileTree.value = displaySnapshot.tree;
  }
  currentDirectoryPath.value = displaySnapshot.currentPath;

  const hasCurrentSelection = selectedFilePath.value
    && displaySnapshot.entries.some((entry) => entry.path === selectedFilePath.value);
  selectedFilePath.value = hasCurrentSelection ? selectedFilePath.value : getDefaultFileBrowserSelection(displaySnapshot);
  void loadThumbnailsForSnapshot(displaySnapshot);
}

async function loadThumbnailsForSnapshot(snapshot: FileBrowserSnapshot) {
  if (snapshot.specialLocation === "trash") return;
  const token = ++thumbnailLoadToken;
  const files = snapshot.entries.filter((entry) => entry.kind === "file" && !entry.thumbnailPath);
  let cursor = 0;

  async function worker() {
    while (cursor < files.length) {
      const entry = files[cursor++];
      await loadThumbnailForEntry(snapshot, entry, token);
    }
  }

  await Promise.all(
    Array.from({ length: Math.min(THUMBNAIL_LOAD_CONCURRENCY, files.length) }, () => worker()),
  );
}

async function loadThumbnailForEntry(snapshot: FileBrowserSnapshot, entry: FileBrowserEntry, token: number) {
  try {
    const response = await ensureThumbnail({
      repoId: snapshot.repoId,
      path: entry.path,
      action: "ensure",
    });
    if (!response.thumbnailPath || token !== thumbnailLoadToken) return;
    applyThumbnailResponse(response, snapshot.currentPath);
  } catch {
    return;
  }
}

function applyThumbnailResponse(response: ThumbnailResponse, expectedDirectoryPath = currentDirectoryPath.value) {
  const current = fileBrowser.value;
  if (!current || current.repoId !== response.repoId || current.currentPath !== expectedDirectoryPath) return;
  if (!current.entries.some((item) => item.path === response.path && item.kind === response.kind)) return;

  fileBrowser.value = {
    ...current,
    entries: current.entries.map((item) => (
      item.path === response.path && item.kind === response.kind
        ? {
            ...item,
            assetId: response.assetId || item.assetId,
            thumbnailPath: response.thumbnailPath ?? null,
            thumbnailCustom: response.thumbnailCustom,
          }
        : item
    )),
  };
}

export async function loadFileBrowserForDirectory(directoryPath = "", options: FileBrowserLoadOptions = {}) {
  if (!activeRepoId.value) return null;

  const includeTree = options.includeTree ?? false;
  const specialLocation = options.specialLocation ?? (activePanel.value === "deleted" ? "trash" : undefined);
  isLoadingFileBrowser.value = true;
  error.value = null;
  const progressId = startOperationProgress(
    specialLocation === "trash" ? "读取回收站" : includeTree ? "读取文件树" : "读取目录",
    directoryPath ? `正在读取 ${directoryPath}` : specialLocation === "trash" ? "正在读取回收站" : "正在读取根目录",
    { initial: 14, indeterminate: true },
  );
  try {
    const snapshot = await getFileBrowser({
      repoId: activeRepoId.value,
      directoryPath,
      includeTree,
      specialLocation,
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
    const deleteMode = mode ?? (activePanel.value === "deleted" ? "permanentDelete" : undefined);
    const snapshot = await deleteEntry({
      repoId: activeRepoId.value,
      path,
      mode: deleteMode,
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

export async function restoreTrashEntry(path: string) {
  if (!activeRepoId.value) return null;
  isMutatingFiles.value = true;
  error.value = null;
  try {
    const snapshot = await mutateTrash({
      repoId: activeRepoId.value,
      action: "restore",
      path,
    });
    const shouldSelectDefault = selectedFilePath.value === path;
    applyFileBrowserSnapshot(snapshot);
    if (shouldSelectDefault) {
      selectedFilePath.value = getDefaultFileBrowserSelection(snapshot);
    }
    await refreshRepositorySnapshot(activeRepoId.value);
    await refreshRepositorySummaries();
    return snapshot;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    return null;
  } finally {
    isMutatingFiles.value = false;
  }
}

export async function restoreAllTrashEntries() {
  if (!activeRepoId.value) return null;
  isMutatingFiles.value = true;
  error.value = null;
  try {
    const snapshot = await mutateTrash({
      repoId: activeRepoId.value,
      action: "restoreAll",
    });
    applyFileBrowserSnapshot(snapshot);
    selectedFilePath.value = getDefaultFileBrowserSelection(snapshot);
    await refreshRepositorySnapshot(activeRepoId.value);
    await refreshRepositorySummaries();
    return snapshot;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    return null;
  } finally {
    isMutatingFiles.value = false;
  }
}

export async function emptyTrash() {
  if (!activeRepoId.value) return null;
  isMutatingFiles.value = true;
  error.value = null;
  try {
    const snapshot = await mutateTrash({
      repoId: activeRepoId.value,
      action: "empty",
    });
    applyFileBrowserSnapshot(snapshot);
    selectedFilePath.value = getDefaultFileBrowserSelection(snapshot);
    await refreshRepositorySnapshot(activeRepoId.value);
    await refreshRepositorySummaries();
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
  thumbnailLoadToken += 1;
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
  activeAssetDetail.value = null;

  setStartupProgress(3, "读取首屏目录");
  currentDirectoryPath.value = "";
  const browserSnapshot = await getFileBrowser({
    repoId: nextRepoId,
    directoryPath: "",
    includeTree: true,
  });
  applyFileBrowserSnapshot(browserSnapshot);

  if (defaultAssetId) {
    void selectAsset(defaultAssetId);
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

export async function setWorkspaceEntryThumbnail(path: string, sourcePath: string) {
  if (!activeRepoId.value || fileBrowser.value?.specialLocation === "trash") return null;
  error.value = null;
  try {
    const response = await ensureThumbnail({
      repoId: activeRepoId.value,
      path,
      action: "save",
      sourcePath,
    });
    applyThumbnailResponse(response);
    return response;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    return null;
  }
}

export async function setWorkspaceEntryThumbnailFromBytes(path: string, imageBytes: number[], mediaType?: string) {
  if (!activeRepoId.value || fileBrowser.value?.specialLocation === "trash") return null;
  error.value = null;
  try {
    const response = await ensureThumbnail({
      repoId: activeRepoId.value,
      path,
      action: "save",
      imageBytes,
      mediaType,
    });
    applyThumbnailResponse(response);
    return response;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    return null;
  }
}

export async function saveGeneratedWorkspaceEntryThumbnail(path: string, imageBytes: number[], mediaType?: string) {
  if (!activeRepoId.value || fileBrowser.value?.specialLocation === "trash") return null;
  error.value = null;
  try {
    const response = await ensureThumbnail({
      repoId: activeRepoId.value,
      path,
      action: "saveGenerated",
      imageBytes,
      mediaType,
    });
    applyThumbnailResponse(response);
    return response;
  } catch {
    return null;
  }
}

export async function clearWorkspaceEntryThumbnail(path: string) {
  if (!activeRepoId.value || fileBrowser.value?.specialLocation === "trash") return null;
  error.value = null;
  try {
    const response = await ensureThumbnail({
      repoId: activeRepoId.value,
      path,
      action: "clear",
    });
    applyThumbnailResponse(response);
    return response;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    return null;
  }
}

export async function refreshWorkspaceEntryThumbnail(path: string) {
  if (!activeRepoId.value || fileBrowser.value?.specialLocation === "trash") return null;
  error.value = null;
  try {
    const response = await ensureThumbnail({
      repoId: activeRepoId.value,
      path,
      action: "refresh",
    });
    applyThumbnailResponse(response);
    return response;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    return null;
  }
}

export function setActivePanel(panel: WorkspacePanelKey) {
  activePanel.value = panel;
  if (panel === "files" && activeRepoId.value && fileBrowser.value?.specialLocation === "trash") {
    void loadFileBrowserForDirectory("", { includeTree: true });
  }
  if (panel === "deleted" && activeRepoId.value) {
    void loadFileBrowserForDirectory("", { specialLocation: "trash" });
  }
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
  setSyncProgress("scanning", "扫描仓库文件", 1);

  try {
    const previousDirectoryPath = currentDirectoryPath.value;
    const result = await syncRepository({ repoId: activeRepoId.value });
    updateOperationProgress(progressId, {
      detail: `已扫描 ${result.scannedFiles} 个文件`,
      value: 72,
      indeterminate: false,
    });
    setSyncProgress("writing", "写入索引结果", 2);
    lastSyncResult.value = result;
    await refreshRepositorySnapshot(activeRepoId.value);
    await refreshRepositorySummaries();
    setSyncProgress("refreshing", "刷新仓库视图", 3);
    if (activePanel.value === "files") {
      await loadFileBrowserForDirectory(previousDirectoryPath, { includeTree: true });
    } else if (activePanel.value === "deleted") {
      await loadFileBrowserForDirectory(previousDirectoryPath, { specialLocation: "trash" });
    }
    setSyncProgress("complete", "同步完成", 3);
    finishOperationProgress(progressId);
    return result;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    cancelOperationProgress(progressId);
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
  const progressId = startOperationProgress("刷新文件树", "同步并读取目录结构", { initial: 12 });
  setSyncProgress("scanning", "扫描文件夹结构", 1);
  try {
    const result = await syncRepository({ repoId: activeRepoId.value });
    updateOperationProgress(progressId, { detail: `已扫描 ${result.scannedFiles} 个文件`, value: 58 });
    setSyncProgress("writing", "写入索引结果", 2);
    lastSyncResult.value = result;
    await refreshRepositorySnapshot(activeRepoId.value);
    setSyncProgress("refreshing", "刷新文件夹树", 3);
    const snapshot = await loadFileBrowserForDirectory(
      currentDirectoryPath.value,
      activePanel.value === "deleted" ? { specialLocation: "trash" } : { includeTree: true },
    );
    setSyncProgress("complete", "刷新完成", 3);
    finishOperationProgress(progressId);
    return snapshot;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    cancelOperationProgress(progressId);
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

      workspaceStartup.value = {
        status: "ready",
        stepLabel: "加载完成",
        currentStep: STARTUP_TOTAL_STEPS,
        totalSteps: STARTUP_TOTAL_STEPS,
        percent: 100,
        error: null,
      };
      void loadSettingsData();
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
    operationProgress: computed(() => operationProgress.value),
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
    restoreTrashEntry,
    restoreAllTrashEntries,
    emptyTrash,
    openWorkspaceEntry,
    revealWorkspaceEntry,
    selectWorkspaceEntry,
    setWorkspaceEntryThumbnail,
    setWorkspaceEntryThumbnailFromBytes,
    saveGeneratedWorkspaceEntryThumbnail,
    clearWorkspaceEntryThumbnail,
    refreshWorkspaceEntryThumbnail,
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
