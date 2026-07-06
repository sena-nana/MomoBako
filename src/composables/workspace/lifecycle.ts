import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  getFileBrowser,
  getRepositoryTree,
  getRepositorySnapshot,
  listPlaylists,
  listHardlinkCandidates,
  listRepositoryActions,
  listSmartFolders,
  listRepositories,
  syncRepository,
} from "../../services/repositoryApi";
import { emitSystemLogSilently } from "../../services/systemLog";
import type {
  RepositoryStructureUpdatedEvent,
  RepositorySummary,
} from "../../types/repository";
import {
  activeLibraryCategory,
  activeAssetDetail,
  activeAssetId,
  activePreviewPath,
  activePanel,
  activeRepoId,
  activeSnapshot,
  apiDesign,
  cacheSnapshot,
  FILE_BROWSER_INITIAL_PAGE_SIZE,
  createInitialWorkspaceStartup,
  currentDirectoryPath,
  createEmptyFileBrowserDerivedState,
  error,
  fileBrowser,
  fileBrowserDerived,
  fileTree,
  hardlinkCandidates,
  activePlaylistId,
  activePlaylistDetail,
  playlistMemberships,
  isLoadingAssetDetail,
  isLoadingFileBrowser,
  isLoadingFileBrowserMore,
  isLoadingSmartFolder,
  isLoadingRepositories,
  isLoadingSettingsData,
  isLoadingSnapshot,
  isManagingPlugins,
  isExternalDragActive,
  isInternalDragActive,
  isMutatingFiles,
  isMutatingSmartFolder,
  isSavingMetadata,
  isSearching,
  isSyncing,
  lastSyncResult,
  plugins,
  playlists,
  repositories,
  selectedFilePaths,
  selectionAnchorPath,
  selectedFilePath,
  activeSmartFolderId,
  smartFolderResult,
  smartFolders,
  STARTUP_TOTAL_STEPS,
  workspaceStartup,
  dragHoverFolderPath,
  draggedWorkspacePaths,
  repositoryActions,
  activeRepositoryActionId,
} from "./state";
import { applyFileBrowserSnapshot, buildPresetRootFileBrowserSnapshot } from "./files";
import { loadFileBrowserForDirectory } from "./files";
import { resetSearchState } from "./search";
import {
  cancelOperationProgress,
  createInitialSyncProgress,
  finishOperationProgress,
  setSyncProgress,
  startOperationProgress,
  syncProgress,
  updateOperationProgress,
} from "./tasks";
import { resetSystemLogsForTests } from "./logs";
import { invalidateThumbnailQueue } from "./thumbnails";
import { scheduleIdleTask } from "./scheduler";
import {
  clearPlaylistDetailCache,
  refreshPlaylists,
  primePlaylistDetailCache,
  selectPlaylist,
  syncPlaylistMemberships,
} from "./playlists";
import { refreshRepositoryActions } from "./repositoryActions";
import { refreshHardlinkCandidates, refreshRepositorySnapshot, refreshRepositorySummaries, refreshRepositoryTree } from "./refresh";
import { refreshSmartFolders, selectSmartFolder } from "./smartFolders";

let startupPromise: Promise<void> | null = null;
let repositoryBackgroundToken = 0;
let cancelRepositoryBackgroundTask: (() => void) | null = null;
let unlistenStructureUpdated: UnlistenFn | null = null;
let structureUpdatedListenerPromise: Promise<void> | null = null;
let startupTargetRepoId: string | null = null;
const LAST_ACTIVE_REPOSITORY_STORAGE_KEY = "momobako.lastActiveRepositoryId";
const STARTUP_LOG_CATEGORY = "workspace.startup";

function readLastActiveRepositoryId() {
  try {
    const repoId = window.localStorage.getItem(LAST_ACTIVE_REPOSITORY_STORAGE_KEY)?.trim();
    return repoId || null;
  } catch {
    return null;
  }
}

/**
 * 记录上次打开的资源库，供下次启动时直接恢复到用户离开前的工作区。
 */
export function rememberLastActiveRepository(repoId?: string | null) {
  try {
    if (!repoId?.trim()) {
      window.localStorage.removeItem(LAST_ACTIVE_REPOSITORY_STORAGE_KEY);
      return;
    }
    window.localStorage.setItem(LAST_ACTIVE_REPOSITORY_STORAGE_KEY, repoId.trim());
  } catch {
    /* 忽略无痕模式或配额限制 */
  }
}

async function handleRepositoryStructureUpdated(event: RepositoryStructureUpdatedEvent) {
  if (!activeRepoId.value || event.repoId !== activeRepoId.value) return;
  await refreshActiveRepositoryWorkspaceSilently({
    reason: event.reason,
    refreshCurrentPanel: true,
  });
}

function ensureStructureUpdatedListener() {
  if (unlistenStructureUpdated || structureUpdatedListenerPromise) return;
  structureUpdatedListenerPromise = listen<RepositoryStructureUpdatedEvent>(
    "repository://structure-updated",
    ({ payload }) => {
      void handleRepositoryStructureUpdated(payload);
    },
  )
    .then((unlisten) => {
      unlistenStructureUpdated = unlisten;
    })
    .finally(() => {
      structureUpdatedListenerPromise = null;
    });
}

export function resetWorkspaceSelection(options: { clearRememberedRepository?: boolean } = {}) {
  const previousRepoId = activeRepoId.value;
  activeRepoId.value = null;
  resetActiveRepositoryContent(previousRepoId);
  if (options.clearRememberedRepository) {
    rememberLastActiveRepository(null);
  }
}

export function resetActiveRepositoryContent(repoIdToClear = activeRepoId.value) {
  invalidateRepositoryBackgroundLoads();
  invalidateThumbnailQueue();
  clearPlaylistDetailCache(repoIdToClear);
  activeSnapshot.value = null;
  activeAssetId.value = null;
  activeAssetDetail.value = null;
  activePreviewPath.value = null;
  activeLibraryCategory.value = "all";
  fileBrowser.value = null;
  fileBrowserDerived.value = createEmptyFileBrowserDerivedState();
  fileTree.value = [];
  isLoadingFileBrowserMore.value = false;
  playlists.value = [];
  playlistMemberships.value = {};
  activePlaylistId.value = null;
  activePlaylistDetail.value = null;
  smartFolders.value = [];
  activeSmartFolderId.value = null;
  smartFolderResult.value = null;
  currentDirectoryPath.value = "";
  selectedFilePath.value = null;
  selectedFilePaths.value = [];
  selectionAnchorPath.value = null;
  isExternalDragActive.value = false;
  isInternalDragActive.value = false;
  draggedWorkspacePaths.value = [];
  dragHoverFolderPath.value = null;
  resetSearchState();
}

function nextRepositoryBackgroundToken() {
  repositoryBackgroundToken += 1;
  return repositoryBackgroundToken;
}

export function invalidateRepositoryBackgroundLoads() {
  repositoryBackgroundToken += 1;
  cancelRepositoryBackgroundTask?.();
  cancelRepositoryBackgroundTask = null;
}

type SilentRepositoryRefreshOptions = {
  reason?: RepositoryStructureUpdatedEvent["reason"] | "user";
  refreshCurrentPanel?: boolean;
};

async function syncSilentRepositoryBaseState(repoId: string) {
  const currentAssetId = activeAssetId.value;
  await Promise.all([
    refreshRepositorySummaries(),
    refreshRepositorySnapshot(repoId),
    refreshRepositoryTree(repoId),
    refreshPlaylists(repoId),
    refreshSmartFolders(repoId),
    refreshRepositoryActions(repoId),
    refreshHardlinkCandidates(repoId),
  ]);

  if (activeRepoId.value !== repoId) return false;

  const activeSummary = repositories.value.find((item) => item.repoId === repoId) ?? null;
  if (!activeSummary) {
    resetWorkspaceSelection();
    return false;
  }

  if (activeSummary.status === "missing") {
    resetActiveRepositoryContent();
    activeRepoId.value = repoId;
    return false;
  }

  if (currentAssetId && !activeSnapshot.value?.assets.some((item) => item.assetId === currentAssetId)) {
    activeAssetId.value = null;
    activeAssetDetail.value = null;
  }

  return true;
}

async function refreshCurrentPanelAfterSilentRefresh(repoId: string) {
  if (activeRepoId.value !== repoId) return;

  if (activePanel.value === "files") {
    await loadFileBrowserForDirectory(currentDirectoryPath.value, {
      includeTree: false,
      silent: true,
    });
    return;
  }

  if (activePanel.value === "trash") {
    await loadFileBrowserForDirectory(currentDirectoryPath.value, {
      specialLocation: "trash",
      silent: true,
    });
    return;
  }

  if (activePanel.value === "playlist" && activePlaylistId.value) {
    await selectPlaylist(activePlaylistId.value);
    return;
  }

  if (activePanel.value === "smartFolder" && activeSmartFolderId.value) {
    await selectSmartFolder(activeSmartFolderId.value);
  }
}

/**
 * 静默刷新当前资源库，更新侧栏与当前面板可见状态，不触发完整工作区重载。
 */
export async function refreshActiveRepositoryWorkspaceSilently(
  options: SilentRepositoryRefreshOptions = {},
) {
  const repoId = activeRepoId.value;
  if (!repoId) return null;

  const stillActive = await syncSilentRepositoryBaseState(repoId);
  if (!stillActive) return null;

  if (options.refreshCurrentPanel ?? true) {
    await refreshCurrentPanelAfterSilentRefresh(repoId);
  }

  return {
    repoId,
    reason: options.reason ?? "user",
  };
}

function isRepositoryBackgroundTokenActive(repoId: string, token: number) {
  return activeRepoId.value === repoId && repositoryBackgroundToken === token;
}

async function applyRepositorySnapshotState(
  repoId: string,
  selectAsset: (assetId: string) => Promise<unknown>,
) {
  const snapshot = await getRepositorySnapshot(repoId);
  activeRepoId.value = repoId;
  activeSnapshot.value = snapshot;
  const snapshotPlaylists = snapshot.playlists ?? [];
  playlists.value = snapshotPlaylists.length ? snapshotPlaylists : await listPlaylists(repoId);

  const defaultAssetId = activeAssetId.value && snapshot.assets.some((item) => item.assetId === activeAssetId.value)
    ? activeAssetId.value
    : snapshot.assets[0]?.assetId ?? null;
  activeAssetId.value = defaultAssetId;
  activeAssetDetail.value = null;

  applyFileBrowserSnapshot(buildPresetRootFileBrowserSnapshot(snapshot));
  currentDirectoryPath.value = "";

  if (defaultAssetId) {
    void selectAsset(defaultAssetId);
  }

  return { defaultAssetId, snapshot };
}

/**
 * 统一驱动首屏加载页，启动和切换资源库都复用同一套状态展示。
 */
export function setWorkspaceStartupProgress(currentStep: number, stepLabel: string, stepDetail = "") {
  const totalSteps = workspaceStartup.value.totalSteps || STARTUP_TOTAL_STEPS;
  workspaceStartup.value = {
    status: "loading",
    stepLabel,
    stepDetail,
    currentStep,
    totalSteps,
    percent: Math.round((currentStep / totalSteps) * 100),
    error: null,
  };
}

export function finishWorkspaceStartup() {
  workspaceStartup.value = {
    status: "ready",
    stepLabel: "加载完成",
    stepDetail: "工作区首屏已经准备完成。",
    currentStep: STARTUP_TOTAL_STEPS,
    totalSteps: STARTUP_TOTAL_STEPS,
    percent: 100,
    error: null,
  };
}

export function failWorkspaceStartup(message: string) {
  workspaceStartup.value = {
    ...workspaceStartup.value,
    status: "error",
    stepLabel: "加载失败",
    stepDetail: "资源库加载流程已停止，保留当前错误供重试。",
    error: message,
  };
}

function setStartupLoadingFlags(value: boolean) {
  isLoadingRepositories.value = value;
  isLoadingSnapshot.value = value;
  isLoadingFileBrowser.value = value;
  isLoadingSettingsData.value = value;
}

function emitStartupLog(
  level: "debug" | "info" | "warn" | "error",
  action: string,
  message: string,
  context?: Record<string, unknown>,
  repoId?: string | null,
) {
  emitSystemLogSilently(level, {
    category: STARTUP_LOG_CATEGORY,
    action,
    message,
    repoId,
    context,
  });
}

async function loadInitialRepository(
  items: RepositorySummary[],
  selectAsset: (assetId: string) => Promise<unknown>,
) {
  if (!items.length) {
    emitStartupLog("warn", "repositoryEmpty", "首屏启动未找到可加载的资源库。", {
      repositoryCount: 0,
    });
    resetWorkspaceSelection({ clearRememberedRepository: true });
    return;
  }

  const lastActiveRepoId = readLastActiveRepositoryId();
  const nextRepoId = activeRepoId.value && items.some((item) => item.repoId === activeRepoId.value)
    ? activeRepoId.value
    : lastActiveRepoId && items.some((item) => item.repoId === lastActiveRepoId)
      ? lastActiveRepoId
      : items[0].repoId;
  startupTargetRepoId = nextRepoId;
  const nextRepository = items.find((item) => item.repoId === nextRepoId);
  emitStartupLog("info", "repositorySelected", "首屏启动已选定资源库。", {
    repositoryCount: items.length,
    rememberedRepoId: lastActiveRepoId,
    repositoryStatus: nextRepository?.status ?? null,
    repositoryName: nextRepository?.name ?? null,
    repositoryPath: nextRepository?.path ?? null,
  }, nextRepoId);

  if (nextRepository?.status === "missing") {
    activeRepoId.value = nextRepoId;
    resetActiveRepositoryContent();
    rememberLastActiveRepository(nextRepoId);
    emitStartupLog("warn", "repositoryMissing", "首屏启动遇到缺失资源库。", {
      repositoryName: nextRepository.name,
      repositoryPath: nextRepository.path,
    }, nextRepoId);
    return;
  }

  setWorkspaceStartupProgress(2, "扫描资源库文件", "同步文件变化，更新新增、移动和删除记录。");
  emitStartupLog("info", "syncStart", "首屏启动开始同步文件变化。", {
    step: 2,
  }, nextRepoId);
  lastSyncResult.value = await syncRepository({ repoId: nextRepoId });
  emitStartupLog("info", "syncSuccess", "首屏启动文件变化同步完成。", {
    step: 2,
    scannedFiles: lastSyncResult.value.scannedFiles,
    createdAssets: lastSyncResult.value.createdAssets,
    updatedAssets: lastSyncResult.value.updatedAssets,
    deletedAssets: lastSyncResult.value.deletedAssets,
    createdEvents: lastSyncResult.value.createdEvents,
    hardlinkCandidates: lastSyncResult.value.hardlinkCandidates,
  }, nextRepoId);

  setWorkspaceStartupProgress(3, "读取仓库摘要", "读取资源库摘要、素材索引和默认预览对象。");
  emitStartupLog("info", "snapshotStart", "首屏启动开始读取资源库摘要。", {
    step: 3,
  }, nextRepoId);
  await applyRepositorySnapshotState(nextRepoId, selectAsset);
  emitStartupLog("info", "snapshotSuccess", "首屏启动资源库摘要读取完成。", {
    step: 3,
    assetCount: activeSnapshot.value?.assets.length ?? 0,
    playlistCount: playlists.value.length,
    defaultAssetId: activeAssetId.value,
  }, nextRepoId);
  rememberLastActiveRepository(nextRepoId);

  setWorkspaceStartupProgress(4, "读取首屏目录", "加载根目录、播放列表和首屏关联数据。");
  emitStartupLog("info", "firstScreenStart", "首屏启动开始加载首屏目录与关联数据。", {
    step: 4,
  }, nextRepoId);
  const playlistItems = playlists.value;
  playlists.value = playlistItems;
  await primePlaylistDetailCache(nextRepoId, playlistItems);
  await syncPlaylistMemberships(nextRepoId, playlistItems);
  queueRepositoryBackgroundLoads(nextRepoId);
  emitStartupLog("info", "firstScreenSuccess", "首屏启动首屏目录与关联数据加载完成。", {
    step: 4,
    playlistCount: playlistItems.length,
    currentDirectoryPath: currentDirectoryPath.value,
  }, nextRepoId);
}

async function loadRepositoryPrimaryDirectory(repoId: string, token: number) {
  const browserSnapshot = await getFileBrowser({
    repoId,
    directoryPath: "",
    includeTree: false,
    offset: 0,
    limit: FILE_BROWSER_INITIAL_PAGE_SIZE,
  });
  if (!isRepositoryBackgroundTokenActive(repoId, token) || currentDirectoryPath.value !== "") {
    return;
  }
  applyFileBrowserSnapshot(browserSnapshot);
}

async function loadRepositoryMetadataBackground(repoId: string, token: number) {
  const [
    playlistItems,
    smartFolderItems,
    actionItems,
    hardlinkResponse,
  ] = await Promise.allSettled([
    listPlaylists(repoId),
    listSmartFolders(repoId),
    listRepositoryActions(repoId),
    listHardlinkCandidates(repoId),
  ]);

  if (!isRepositoryBackgroundTokenActive(repoId, token)) return;

  if (playlistItems.status === "fulfilled") {
    playlists.value = playlistItems.value;
    await syncPlaylistMemberships(repoId, playlistItems.value);
  }
  if (smartFolderItems.status === "fulfilled") {
    smartFolders.value = smartFolderItems.value;
  }
  if (actionItems.status === "fulfilled") {
    repositoryActions.value = actionItems.value;
    if (activeRepositoryActionId.value && !actionItems.value.some((action) => action.actionId === activeRepositoryActionId.value)) {
      activeRepositoryActionId.value = null;
    }
    activeRepositoryActionId.value = activeRepositoryActionId.value ?? actionItems.value[0]?.actionId ?? null;
  }
  if (hardlinkResponse.status === "fulfilled") {
    hardlinkCandidates.value = hardlinkResponse.value.candidates;
  }
}

async function loadRepositoryStructureBackground(repoId: string, token: number) {
  const [treeSnapshot, rootDirectorySnapshot] = await Promise.allSettled([
    getRepositoryTree(repoId),
    getFileBrowser({
      repoId,
      directoryPath: "",
      includeTree: false,
      offset: 0,
      limit: FILE_BROWSER_INITIAL_PAGE_SIZE,
    }),
  ]);

  if (!isRepositoryBackgroundTokenActive(repoId, token)) return;

  if (treeSnapshot.status === "fulfilled") {
    fileTree.value = treeSnapshot.value.tree;
  }
  if (
    rootDirectorySnapshot.status === "fulfilled"
    && currentDirectoryPath.value === ""
  ) {
    applyFileBrowserSnapshot(rootDirectorySnapshot.value);
  }
}

export function queueRepositoryBackgroundLoads(repoId: string) {
  const token = nextRepositoryBackgroundToken();
  cancelRepositoryBackgroundTask?.();
  void loadRepositoryStructureBackground(repoId, token);
  cancelRepositoryBackgroundTask = scheduleIdleTask(() => {
    void loadRepositoryMetadataBackground(repoId, token);
  }, 250);
}

export async function applyRepositorySnapshotAsPresetRoot(
  repoId: string,
  selectAsset: (assetId: string) => Promise<unknown>,
) {
  return applyRepositorySnapshotState(repoId, selectAsset);
}

export async function loadRepositoryRootDirectoryImmediately(repoId: string) {
  const token = nextRepositoryBackgroundToken();
  await loadRepositoryPrimaryDirectory(repoId, token);
  return token;
}

export async function loadRepositories(
  selectRepository: (repoId: string) => Promise<unknown>,
) {
  isLoadingRepositories.value = true;
  error.value = null;
  const progressId = startOperationProgress("加载资源库", "读取已注册资源库", { initial: 12, indeterminate: true });

  try {
    const items = await listRepositories();
    updateOperationProgress(progressId, { detail: "加载资源库摘要", value: 38 });
    repositories.value = items;

    if (!items.length) {
      resetWorkspaceSelection({ clearRememberedRepository: true });
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

export function ensureRepositoryWorkspace(
  selectAsset: (assetId: string) => Promise<unknown>,
  loadSettingsData: () => Promise<unknown>,
) {
  if (workspaceStartup.value.status === "ready") return;
  if (startupPromise) return startupPromise;

  startupPromise = (async () => {
    workspaceStartup.value = { ...createInitialWorkspaceStartup(), status: "loading" };
    error.value = null;
    setStartupLoadingFlags(true);
    ensureStructureUpdatedListener();
    emitStartupLog("info", "startupStart", "首屏启动流程开始。", {
      totalSteps: STARTUP_TOTAL_STEPS,
      activeRepoId: activeRepoId.value,
      rememberedRepoId: readLastActiveRepositoryId(),
    });

    try {
      setWorkspaceStartupProgress(1, "加载仓库列表", "读取已注册资源库，并匹配上次打开的工作区。");
      emitStartupLog("info", "repositoryListStart", "首屏启动开始读取资源库列表。", {
        step: 1,
      });
      const items = await listRepositories();
      repositories.value = items;
      emitStartupLog("info", "repositoryListSuccess", "首屏启动资源库列表读取完成。", {
        step: 1,
        repositoryCount: items.length,
        readyRepositoryCount: items.filter((item) => item.status === "ready").length,
        missingRepositoryCount: items.filter((item) => item.status === "missing").length,
      });

      await loadInitialRepository(items, selectAsset);
      setWorkspaceStartupProgress(4, "读取首屏目录", "加载应用配置、插件设置和首屏辅助数据。");
      emitStartupLog("info", "settingsStart", "首屏启动开始加载应用配置与插件设置。", {
        step: 4,
        repoId: activeRepoId.value,
      }, activeRepoId.value);
      await loadSettingsData();
      emitStartupLog("info", "settingsSuccess", "首屏启动应用配置与插件设置加载完成。", {
        step: 4,
        repoId: activeRepoId.value,
      }, activeRepoId.value);

      finishWorkspaceStartup();
      emitStartupLog("info", "startupSuccess", "首屏启动流程完成。", {
        activeRepoId: activeRepoId.value,
        assetCount: activeSnapshot.value?.assets.length ?? 0,
        playlistCount: playlists.value.length,
      }, activeRepoId.value);
    } catch (cause) {
      const message = cause instanceof Error ? cause.message : String(cause);
      const failedRepoId = activeRepoId.value ?? startupTargetRepoId;
      error.value = message;
      failWorkspaceStartup(message);
      emitStartupLog("error", "startupFailed", "首屏启动流程失败。", {
        step: workspaceStartup.value.currentStep,
        stepLabel: workspaceStartup.value.stepLabel,
        error: message,
        activeRepoId: activeRepoId.value,
        targetRepoId: startupTargetRepoId,
      }, failedRepoId);
    } finally {
      setStartupLoadingFlags(false);
      startupTargetRepoId = null;
      startupPromise = null;
    }
  })();

  return startupPromise;
}

export function resetRepositoryWorkspaceForTests() {
  clearPlaylistDetailCache();
  repositories.value = [];
  resetWorkspaceSelection();
  activePanel.value = "files";
  activeLibraryCategory.value = "all";
  hardlinkCandidates.value = [];
  lastSyncResult.value = null;
  plugins.value = [];
  cacheSnapshot.value = null;
  apiDesign.value = null;
  error.value = null;
  isLoadingRepositories.value = false;
  isLoadingSnapshot.value = false;
  isLoadingAssetDetail.value = false;
  isLoadingFileBrowser.value = false;
  isLoadingFileBrowserMore.value = false;
  isLoadingSmartFolder.value = false;
  isSearching.value = false;
  isMutatingSmartFolder.value = false;
  isSavingMetadata.value = false;
  isSyncing.value = false;
  isMutatingFiles.value = false;
  isLoadingSettingsData.value = false;
  isManagingPlugins.value = false;
  workspaceStartup.value = createInitialWorkspaceStartup();
  syncProgress.value = createInitialSyncProgress();
  setSyncProgress("idle", "", 0);
  resetSystemLogsForTests();
  unlistenStructureUpdated?.();
  unlistenStructureUpdated = null;
  structureUpdatedListenerPromise = null;
  startupPromise = null;
}
