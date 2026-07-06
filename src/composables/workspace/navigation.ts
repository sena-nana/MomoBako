import {
  getAssetDetail,
  getRepositorySnapshot,
  syncRepository,
} from "../../services/repositoryApi";
import { emitSystemLogSilently } from "../../services/systemLog";
import {
  activeLibraryCategory,
  activeAssetDetail,
  activeAssetId,
  activePanel,
  activePreviewPath,
  activePlaylistDetail,
  activePlaylistId,
  activeRepoId,
  activeSmartFolderId,
  activeSnapshot,
  currentDirectoryPath,
  error,
  fileBrowser,
  isLoadingFileBrowser,
  isLoadingAssetDetail,
  isLoadingSnapshot,
  lastSyncResult,
  playlists,
  repositories,
  smartFolderResult,
  type WorkspaceLibraryCategoryKey,
  type WorkspacePanelKey,
} from "./state";
import { resetSearchState } from "./search";
import {
  cancelOperationProgress,
  finishOperationProgress,
  startOperationProgress,
  updateOperationProgress,
} from "./tasks";
import { loadFileBrowserForDirectory } from "./files";
import {
  applyRepositorySnapshotAsPresetRoot,
  ensureRepositoryWorkspace as ensureRepositoryWorkspaceLifecycle,
  failWorkspaceStartup,
  finishWorkspaceStartup,
  loadRepositoryRootDirectoryImmediately,
  loadRepositories as loadRepositoriesLifecycle,
  queueRepositoryBackgroundLoads,
  rememberLastActiveRepository,
  refreshActiveRepositoryWorkspaceSilently as refreshActiveRepositoryWorkspaceSilentlyLifecycle,
  resetActiveRepositoryContent,
  setWorkspaceStartupProgress,
} from "./lifecycle";
import { loadSystemLogsInWorkspace } from "./logs";
import { loadSettingsData } from "./settings";

export async function selectRepository(repoId: string) {
  if (!repoId) return;

  emitSystemLogSilently("info", {
    category: "repository",
    action: "selectStart",
    message: "开始切换资源库。",
    repoId,
  });
  const isSwitchingRepository = activeRepoId.value !== repoId;
  const previousDirectoryPath = !isSwitchingRepository ? currentDirectoryPath.value : "";
  const previousRepoId = activeRepoId.value;
  if (isSwitchingRepository) {
    setWorkspaceStartupProgress(1, "切换资源库");
    resetActiveRepositoryContent(previousRepoId);
    activeRepoId.value = repoId;
    isLoadingFileBrowser.value = true;
  }
  isLoadingSnapshot.value = true;
  error.value = null;
  const progressId = isSwitchingRepository
    ? startOperationProgress("加载资源库", "读取资源库快照", { initial: 10, indeterminate: true })
    : null;

  try {
    const repository = repositories.value.find((item) => item.repoId === repoId);
    if (repository?.status === "missing") {
      activeRepoId.value = repoId;
      resetActiveRepositoryContent();
      rememberLastActiveRepository(repoId);
      if (isSwitchingRepository) {
        resetSearchState();
        finishWorkspaceStartup();
      }
      if (progressId) {
        finishOperationProgress(progressId);
      }
      emitSystemLogSilently("warn", {
        category: "repository",
        action: "selectMissing",
        message: "资源库处于缺失状态。",
        repoId,
      });
      return;
    }

    if (isSwitchingRepository) {
      setWorkspaceStartupProgress(2, "扫描资源库文件");
    }
    if (progressId) {
      updateOperationProgress(progressId, { detail: "扫描资源库文件", value: 28 });
    }
    lastSyncResult.value = await syncRepository({ repoId });

    if (isSwitchingRepository) {
      setWorkspaceStartupProgress(3, "读取仓库摘要");
      await applyRepositorySnapshotAsPresetRoot(repoId, selectAsset);
    } else {
      const snapshot = await getRepositorySnapshot(repoId);
      activeRepoId.value = repoId;
      activeSnapshot.value = snapshot;
      playlists.value = snapshot.playlists ?? [];

      const defaultAssetId = activeAssetId.value && snapshot.assets.some((item) => item.assetId === activeAssetId.value)
        ? activeAssetId.value
        : snapshot.assets[0]?.assetId ?? null;
      activeAssetId.value = defaultAssetId;
      activeAssetDetail.value = null;

      if (defaultAssetId) {
        void selectAsset(defaultAssetId);
      }
    }
    rememberLastActiveRepository(repoId);
    if (progressId) {
      updateOperationProgress(progressId, { detail: "加载资源索引", value: 46 });
    }
    if (isSwitchingRepository) {
      resetSearchState();
      activeLibraryCategory.value = "all";
      activePlaylistId.value = null;
      activePlaylistDetail.value = null;
      activePreviewPath.value = null;
      activeSmartFolderId.value = null;
      smartFolderResult.value = null;
    }

    const initialDirectoryPath = isSwitchingRepository ? "" : previousDirectoryPath;
    currentDirectoryPath.value = initialDirectoryPath;
    if (!isSwitchingRepository) {
      const browserSnapshot = await loadFileBrowserForDirectory(initialDirectoryPath, { includeTree: false });
      if (!browserSnapshot && previousDirectoryPath) {
        currentDirectoryPath.value = "";
        await loadFileBrowserForDirectory("", { includeTree: false });
      }
    }
    if (!isSwitchingRepository && previousDirectoryPath && currentDirectoryPath.value !== previousDirectoryPath) {
      currentDirectoryPath.value = "";
    }
    if (isSwitchingRepository) {
      setWorkspaceStartupProgress(4, "读取首屏目录");
      await loadRepositoryRootDirectoryImmediately(repoId);
    }
    queueRepositoryBackgroundLoads(repoId);
    if (isSwitchingRepository) {
      finishWorkspaceStartup();
    }
    if (progressId) {
      finishOperationProgress(progressId);
    }
    emitSystemLogSilently("info", {
      category: "repository",
      action: "selectSuccess",
      message: "资源库切换完成。",
      repoId,
      context: { switched: isSwitchingRepository },
    });
  } catch (cause) {
    const message = cause instanceof Error ? cause.message : String(cause);
    error.value = message;
    emitSystemLogSilently("error", {
      category: "repository",
      action: "selectFailed",
      message: "资源库切换失败。",
      repoId,
      context: { error: message },
    });
    if (isSwitchingRepository) {
      failWorkspaceStartup(message);
    }
    if (progressId) {
      cancelOperationProgress(progressId);
    }
  } finally {
    isLoadingSnapshot.value = false;
    if (isSwitchingRepository) {
      isLoadingFileBrowser.value = false;
    }
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

export function setActivePanel(panel: WorkspacePanelKey) {
  activePanel.value = panel;
  if (panel === "files" && activeRepoId.value && fileBrowser.value?.specialLocation === "trash") {
    void loadFileBrowserForDirectory("", { includeTree: false });
  }
  if (panel === "trash" && activeRepoId.value) {
    void loadFileBrowserForDirectory("", { specialLocation: "trash" });
  }
  if (panel === "logs") {
    void loadSystemLogsInWorkspace();
  }
}

export function setActiveLibraryCategory(category: WorkspaceLibraryCategoryKey) {
  activeLibraryCategory.value = category;
}

export function setActivePreviewPath(path: string | null) {
  activePreviewPath.value = path;
}

export function loadRepositories() {
  return loadRepositoriesLifecycle(selectRepository);
}

export function ensureRepositoryWorkspace() {
  return ensureRepositoryWorkspaceLifecycle(selectAsset, loadSettingsData);
}

export function refreshRepositoryWorkspace() {
  return loadRepositories();
}

export function refreshActiveRepositoryWorkspaceSilently() {
  return refreshActiveRepositoryWorkspaceSilentlyLifecycle();
}
