import {
  confirmHardlinkCandidate,
  syncRepository,
} from "../../services/repositoryApi";
import type { HardlinkConfirmResponse } from "../../types/repository";
import {
  activePanel,
  activeRepoId,
  error,
  fileBrowser,
  isLoadingFileBrowser,
  isMutatingFiles,
  isSyncing,
  lastSyncResult,
} from "./state";
import {
  refreshHardlinkCandidates,
  refreshWorkspaceAfterMutation,
  type WorkspaceRefreshPlan,
} from "./refresh";
import { loadFileBrowserForDirectory } from "./files";
import {
  cancelOperationProgress,
  finishOperationProgress,
  setSyncProgress,
  startOperationProgress,
  updateOperationProgress,
} from "./tasks";

async function refreshWorkspace(repoId: string, plan: WorkspaceRefreshPlan) {
  await refreshWorkspaceAfterMutation(repoId, plan, loadFileBrowserForDirectory);
}

export async function confirmWorkspaceHardlinkCandidate(candidateId: string): Promise<HardlinkConfirmResponse | null> {
  const repoId = activeRepoId.value;
  if (!repoId) return null;
  isMutatingFiles.value = true;
  error.value = null;
  try {
    const response = await confirmHardlinkCandidate({
      repoId,
      candidateId,
    });
    await refreshWorkspace(repoId, {
      directory: fileBrowser.value && !fileBrowser.value.specialLocation ? "current" : undefined,
      hardlinkCandidates: true,
      repositorySnapshot: true,
    });
    return response;
  } catch (cause) {
    const confirmError = cause instanceof Error ? cause.message : String(cause);
    try {
      await refreshHardlinkCandidates(repoId);
    } catch {
      // Keep the confirmation error visible if the follow-up refresh also fails.
    }
    error.value = confirmError;
    return null;
  } finally {
    isMutatingFiles.value = false;
  }
}

export async function syncActiveRepository() {
  if (!activeRepoId.value) return null;

  isSyncing.value = true;
  error.value = null;
  const progressId = startOperationProgress("同步资源库", "扫描文件变化", { initial: 10 });
  setSyncProgress("scanning", "扫描仓库文件", 1);

  try {
    const result = await syncRepository({ repoId: activeRepoId.value });
    updateOperationProgress(progressId, {
      detail: `已扫描 ${result.scannedFiles} 个文件`,
      value: 72,
      indeterminate: false,
    });
    setSyncProgress("writing", "写入索引结果", 2);
    lastSyncResult.value = result;
    setSyncProgress("refreshing", "刷新仓库视图", 3);
    await refreshWorkspace(activeRepoId.value, {
      directory: activePanel.value === "files"
        ? "currentWithTree"
        : activePanel.value === "deleted" ? "trash" : undefined,
      hardlinkCandidates: true,
      repositorySnapshot: true,
      repositorySummary: true,
    });
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
    setSyncProgress("refreshing", "刷新文件夹树", 3);
    await refreshWorkspace(activeRepoId.value, {
      directory: activePanel.value === "deleted" ? "trash" : "currentWithTree",
      hardlinkCandidates: true,
      repositorySnapshot: true,
    });
    setSyncProgress("complete", "刷新完成", 3);
    finishOperationProgress(progressId);
    return fileBrowser.value;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    cancelOperationProgress(progressId);
    setSyncProgress("error", error.value, 3);
    return null;
  } finally {
    isLoadingFileBrowser.value = false;
  }
}
