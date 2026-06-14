import {
  getAssetDetail,
  getRepositorySnapshot,
} from "../../services/repositoryApi";
import {
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
  isLoadingAssetDetail,
  isLoadingSnapshot,
  playlists,
  repositories,
  smartFolderResult,
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
  ensureRepositoryWorkspace as ensureRepositoryWorkspaceLifecycle,
  loadRepositories as loadRepositoriesLifecycle,
  queueRepositoryBackgroundLoads,
  resetActiveRepositoryContent,
} from "./lifecycle";
import { loadSettingsData } from "./settings";

export async function selectRepository(repoId: string) {
  if (!repoId) return;

  const isSwitchingRepository = activeRepoId.value !== repoId;
  const previousDirectoryPath = !isSwitchingRepository ? currentDirectoryPath.value : "";
  isLoadingSnapshot.value = true;
  error.value = null;
  const progressId = startOperationProgress("加载资源库", "读取资源库快照", { initial: 10, indeterminate: true });

  try {
    const repository = repositories.value.find((item) => item.repoId === repoId);
    if (repository?.status === "missing") {
      activeRepoId.value = repoId;
      resetActiveRepositoryContent();
      if (isSwitchingRepository) {
        resetSearchState();
      }
      finishOperationProgress(progressId);
      return;
    }

    const snapshot = await getRepositorySnapshot(repoId);
    updateOperationProgress(progressId, { detail: "加载资源索引", value: 46 });
    activeRepoId.value = repoId;
    activeSnapshot.value = snapshot;
    playlists.value = snapshot.playlists ?? [];
    if (isSwitchingRepository) {
      resetSearchState();
      activePlaylistId.value = null;
      activePlaylistDetail.value = null;
      activePreviewPath.value = null;
      activeSmartFolderId.value = null;
      smartFolderResult.value = null;
    }

    const defaultAssetId = activeAssetId.value && snapshot.assets.some((item) => item.assetId === activeAssetId.value)
      ? activeAssetId.value
      : snapshot.assets[0]?.assetId ?? null;

    activeAssetId.value = defaultAssetId;
    activeAssetDetail.value = null;

    const initialDirectoryPath = isSwitchingRepository ? "" : previousDirectoryPath;
    currentDirectoryPath.value = initialDirectoryPath;
    const browserSnapshot = await loadFileBrowserForDirectory(initialDirectoryPath, { includeTree: false });
    if (!browserSnapshot && !isSwitchingRepository && previousDirectoryPath) {
      currentDirectoryPath.value = "";
      await loadFileBrowserForDirectory("", { includeTree: false });
    }
    if (defaultAssetId) {
      void selectAsset(defaultAssetId);
    }
    queueRepositoryBackgroundLoads(repoId);
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

export function setActivePanel(panel: WorkspacePanelKey) {
  activePanel.value = panel;
  if (panel === "files" && activeRepoId.value && fileBrowser.value?.specialLocation === "trash") {
    void loadFileBrowserForDirectory("", { includeTree: false });
  }
  if (panel === "deleted" && activeRepoId.value) {
    void loadFileBrowserForDirectory("", { specialLocation: "trash" });
  }
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
