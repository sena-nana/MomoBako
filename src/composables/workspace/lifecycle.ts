import {
  getFileBrowser,
  getRepositorySnapshot,
  listPlaylists,
  listHardlinkCandidates,
  listRepositoryActions,
  listSmartFolders,
  listRepositories,
} from "../../services/repositoryApi";
import type { RepositorySummary } from "../../types/repository";
import {
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
import { applyFileBrowserSnapshot } from "./files";
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
import { invalidateThumbnailQueue } from "./thumbnails";
import { scheduleIdleTask } from "./scheduler";
import { syncPlaylistMemberships } from "./playlists";

let startupPromise: Promise<void> | null = null;

export function resetWorkspaceSelection() {
  activeRepoId.value = null;
  resetActiveRepositoryContent();
}

export function resetActiveRepositoryContent() {
  invalidateThumbnailQueue();
  activeSnapshot.value = null;
  activeAssetId.value = null;
  activeAssetDetail.value = null;
  activePreviewPath.value = null;
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

function setStartupLoadingFlags(value: boolean) {
  isLoadingRepositories.value = value;
  isLoadingSnapshot.value = value;
  isLoadingFileBrowser.value = value;
  isLoadingSettingsData.value = value;
}

async function loadInitialRepository(
  items: RepositorySummary[],
  selectAsset: (assetId: string) => Promise<unknown>,
) {
  if (!items.length) {
    resetWorkspaceSelection();
    return;
  }

  const nextRepoId = activeRepoId.value && items.some((item) => item.repoId === activeRepoId.value)
    ? activeRepoId.value
    : items[0].repoId;
  const nextRepository = items.find((item) => item.repoId === nextRepoId);

  if (nextRepository?.status === "missing") {
    activeRepoId.value = nextRepoId;
    resetActiveRepositoryContent();
    return;
  }

  setStartupProgress(2, "读取仓库摘要");
  const snapshot = await getRepositorySnapshot(nextRepoId);
  activeRepoId.value = nextRepoId;
  activeSnapshot.value = snapshot;
  playlists.value = snapshot.playlists ?? [];

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
    includeTree: false,
    offset: 0,
    limit: FILE_BROWSER_INITIAL_PAGE_SIZE,
  });
  const playlistItems = snapshot.playlists ?? await listPlaylists(nextRepoId);
  playlists.value = playlistItems;
  await syncPlaylistMemberships(nextRepoId, playlistItems);
  applyFileBrowserSnapshot(browserSnapshot);

  if (defaultAssetId) {
    void selectAsset(defaultAssetId);
  }
  queueRepositoryBackgroundLoads(nextRepoId);
}

export function queueRepositoryBackgroundLoads(repoId: string) {
  scheduleIdleTask(() => {
    void loadRepositorySecondaryData(repoId);
  }, 250);
}

async function loadRepositorySecondaryData(repoId: string) {
  const [
    playlistItems,
    smartFolderItems,
    actionItems,
    hardlinkResponse,
    treeSnapshot,
  ] = await Promise.allSettled([
    listPlaylists(repoId),
    listSmartFolders(repoId),
    listRepositoryActions(repoId),
    listHardlinkCandidates(repoId),
    getFileBrowser({
      repoId,
      directoryPath: currentDirectoryPath.value,
      includeTree: true,
    }),
  ]);

  if (activeRepoId.value !== repoId) return;

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
  if (treeSnapshot.status === "fulfilled" && treeSnapshot.value.tree) {
    fileTree.value = treeSnapshot.value.tree;
  }
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
      resetWorkspaceSelection();
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

    try {
      setStartupProgress(1, "加载仓库列表");
      const items = await listRepositories();
      repositories.value = items;

      await loadInitialRepository(items, selectAsset);

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

export function resetRepositoryWorkspaceForTests() {
  repositories.value = [];
  resetWorkspaceSelection();
  activePanel.value = "files";
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
  startupPromise = null;
}
