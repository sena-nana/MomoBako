import {
  copyEntries,
  createDirectory,
  createFile,
  deleteEntry,
  importEntries,
  moveEntries,
  mutateTrash,
  openRepositoryPath,
  renameEntry,
  revealRepositoryPath,
  startExternalFileDrag,
} from "../../services/repositoryApi";
import type {
  FileBrowserSnapshot,
  FileDeleteMode,
} from "../../types/repository";
import {
  activePanel,
  activeRepoId,
  activeSnapshot,
  currentDirectoryPath,
  error,
  fileBrowser,
  isMutatingFiles,
  selectedFilePath,
} from "./state";
import { visibleEntries } from "./selectors";
import {
  cancelOperationProgress,
  finishOperationProgress,
  startOperationProgress,
  updateOperationProgress,
} from "./tasks";
import {
  applyFileBrowserSnapshot,
  entryNameFromPath,
  getDefaultFileBrowserSelection,
  joinActiveRepositoryPath,
  loadFileBrowserForDirectory,
} from "./files";
import {
  applyWorkspaceSelection,
  clearWorkspaceSelection,
  normalizeSelectionPaths,
} from "./selection";
import {
  refreshWorkspaceAfterMutation,
  type WorkspaceRefreshPlan,
} from "./refresh";

function defaultDirectoryRefreshPlan(paths: string[]): WorkspaceRefreshPlan["directory"] {
  if (activePanel.value === "deleted") return "trash";
  const selectedPaths = new Set(paths);
  const includesDirectory = visibleEntries.value.some((entry) => (
    entry.kind === "directory" && selectedPaths.has(entry.path)
  ));
  return includesDirectory ? "currentWithTree" : "current";
}

function resolveBatchMutationPrimaryPath(excludedPaths: string[]) {
  const excluded = new Set(excludedPaths);
  return visibleEntries.value.find((entry) => !excluded.has(entry.path))?.path ?? null;
}

async function refreshAfterFileMutation(repoId: string, plan: WorkspaceRefreshPlan) {
  await refreshWorkspaceAfterMutation(repoId, plan, loadFileBrowserForDirectory);
}

async function finishFileTransfer(
  repoId: string,
  snapshot: FileBrowserSnapshot,
  sourcePaths: string[],
) {
  applyFileBrowserSnapshot(snapshot);
  const sourceNames = new Set(sourcePaths.map(entryNameFromPath));
  const nextSelection = snapshot.entries
    .filter((entry) => sourceNames.has(entry.name))
    .map((entry) => entry.path);
  if (nextSelection.length) {
    applyWorkspaceSelection(nextSelection, nextSelection[0], nextSelection[0]);
  }
  await refreshAfterFileMutation(repoId, { hardlinkCandidates: true, repositorySnapshot: true });
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
    await refreshAfterFileMutation(activeRepoId.value, { repositorySnapshot: true });
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
    const createdPath = snapshot.entries.find((entry) => entry.name === name)?.path ?? null;
    if (createdPath) {
      applyWorkspaceSelection([createdPath], createdPath, createdPath);
    }
    await refreshAfterFileMutation(activeRepoId.value, { repositorySnapshot: true });
    return snapshot;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    return null;
  } finally {
    isMutatingFiles.value = false;
  }
}

export async function importEntriesToWorkspace(sourcePaths: string[], parentPath = currentDirectoryPath.value) {
  const repoId = activeRepoId.value;
  if (!repoId || !sourcePaths.length) return null;
  error.value = null;
  const progressId = startOperationProgress(
    "导入文件",
    `准备导入 ${sourcePaths.length} 个条目`,
    { initial: 8 },
  );
  try {
    updateOperationProgress(progressId, { detail: "导入文件到当前资源库", value: 24 });
    const snapshot = await importEntries({
      repoId,
      parentPath,
      sourcePaths,
    });
    updateOperationProgress(progressId, { detail: "刷新文件索引", value: 84 });
    await finishFileTransfer(repoId, snapshot, sourcePaths);
    finishOperationProgress(progressId);
    return snapshot;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    cancelOperationProgress(progressId);
    return null;
  }
}

export async function copyWorkspaceEntries(sourcePaths: string[], parentPath = currentDirectoryPath.value) {
  const repoId = activeRepoId.value;
  if (!repoId || !sourcePaths.length) return null;
  isMutatingFiles.value = true;
  error.value = null;
  const progressId = startOperationProgress("复制文件", `准备复制 ${sourcePaths.length} 个条目`, { initial: 8 });
  try {
    updateOperationProgress(progressId, { detail: "创建硬链接或复制文件", value: 32 });
    const snapshot = await copyEntries({
      repoId,
      sourcePaths,
      parentPath,
      mode: "hardlinkPreferred",
    });
    updateOperationProgress(progressId, { detail: "刷新文件索引", value: 84 });
    await finishFileTransfer(repoId, snapshot, sourcePaths);
    finishOperationProgress(progressId);
    return snapshot;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    cancelOperationProgress(progressId);
    return null;
  } finally {
    isMutatingFiles.value = false;
  }
}

export async function moveWorkspaceEntries(sourcePaths: string[], parentPath: string) {
  const repoId = activeRepoId.value;
  const nextPaths = normalizeSelectionPaths(sourcePaths);
  if (!repoId || !nextPaths.length) return null;

  isMutatingFiles.value = true;
  error.value = null;
  const progressId = startOperationProgress("移动文件", `准备移动 ${nextPaths.length} 个条目`, { initial: 8 });
  try {
    updateOperationProgress(progressId, { detail: "移动到目标文件夹", value: 36 });
    const snapshot = await moveEntries({
      repoId,
      sourcePaths: nextPaths,
      parentPath,
    });
    updateOperationProgress(progressId, { detail: "刷新文件索引", value: 82 });

    if (currentDirectoryPath.value === parentPath) {
      applyFileBrowserSnapshot(snapshot);
      const movedNames = new Set(nextPaths.map(entryNameFromPath));
      const nextSelection = snapshot.entries
        .filter((entry) => movedNames.has(entry.name))
        .map((entry) => entry.path);
      if (nextSelection.length) {
        applyWorkspaceSelection(nextSelection, nextSelection[0], nextSelection[0]);
      }
      await refreshAfterFileMutation(repoId, { repositorySnapshot: true });
    } else {
      const nextPrimaryPath = resolveBatchMutationPrimaryPath(nextPaths);
      applyWorkspaceSelection(nextPrimaryPath ? [nextPrimaryPath] : [], nextPrimaryPath, nextPrimaryPath);
      await refreshAfterFileMutation(repoId, {
        directory: defaultDirectoryRefreshPlan(nextPaths),
        repositorySnapshot: true,
      });
    }

    finishOperationProgress(progressId);
    return snapshot;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    cancelOperationProgress(progressId);
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
    const renamedPath = snapshot.entries.find((entry) => entry.name === newName)?.path ?? null;
    if (renamedPath) {
      applyWorkspaceSelection([renamedPath], renamedPath, renamedPath);
    }
    await refreshAfterFileMutation(activeRepoId.value, { repositorySnapshot: true });
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
    await refreshAfterFileMutation(activeRepoId.value, { repositorySnapshot: true });
    return snapshot;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    return null;
  } finally {
    isMutatingFiles.value = false;
  }
}

export async function deleteWorkspaceEntries(paths: string[], mode?: FileDeleteMode) {
  const repoId = activeRepoId.value;
  const nextPaths = normalizeSelectionPaths(paths);
  if (!repoId || !nextPaths.length) return null;

  isMutatingFiles.value = true;
  error.value = null;
  const progressId = startOperationProgress("删除文件", `准备处理 ${nextPaths.length} 个条目`, { initial: 10 });
  try {
    const deleteMode = mode ?? (activePanel.value === "deleted" ? "permanentDelete" : undefined);
    const nextPrimaryPath = resolveBatchMutationPrimaryPath(nextPaths);
    for (const [index, path] of nextPaths.entries()) {
      updateOperationProgress(progressId, {
        detail: `正在处理 ${entryNameFromPath(path)}`,
        value: Math.round(((index + 1) / nextPaths.length) * 72),
      });
      await deleteEntry({
        repoId,
        path,
        mode: deleteMode,
      });
    }
    applyWorkspaceSelection(nextPrimaryPath ? [nextPrimaryPath] : [], nextPrimaryPath, nextPrimaryPath);
    await refreshAfterFileMutation(repoId, {
      directory: defaultDirectoryRefreshPlan(nextPaths),
      repositorySnapshot: true,
    });
    finishOperationProgress(progressId);
    return fileBrowser.value;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    cancelOperationProgress(progressId);
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
    await refreshAfterFileMutation(activeRepoId.value, {
      repositorySnapshot: true,
      repositorySummary: true,
    });
    return snapshot;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    return null;
  } finally {
    isMutatingFiles.value = false;
  }
}

export async function restoreTrashEntries(paths: string[]) {
  const repoId = activeRepoId.value;
  const nextPaths = normalizeSelectionPaths(paths);
  if (!repoId || !nextPaths.length) return null;

  isMutatingFiles.value = true;
  error.value = null;
  const progressId = startOperationProgress("还原文件", `准备还原 ${nextPaths.length} 个条目`, { initial: 10 });
  try {
    for (const [index, path] of nextPaths.entries()) {
      updateOperationProgress(progressId, {
        detail: `正在还原 ${entryNameFromPath(path)}`,
        value: Math.round(((index + 1) / nextPaths.length) * 72),
      });
      await mutateTrash({
        repoId,
        action: "restore",
        path,
      });
    }
    clearWorkspaceSelection();
    await refreshAfterFileMutation(repoId, {
      directory: "trash",
      repositorySnapshot: true,
      repositorySummary: true,
    });
    finishOperationProgress(progressId);
    return fileBrowser.value;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    cancelOperationProgress(progressId);
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
    await refreshAfterFileMutation(activeRepoId.value, {
      repositorySnapshot: true,
      repositorySummary: true,
    });
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
    await refreshAfterFileMutation(activeRepoId.value, {
      repositorySnapshot: true,
      repositorySummary: true,
    });
    return snapshot;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    return null;
  } finally {
    isMutatingFiles.value = false;
  }
}

export async function openWorkspaceEntry(path: string) {
  const absolutePath = joinActiveRepositoryPath(path);
  if (!absolutePath) return;
  error.value = null;
  try {
    await openRepositoryPath(absolutePath);
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
  }
}

export async function revealWorkspaceEntry(path: string) {
  const absolutePath = joinActiveRepositoryPath(path);
  if (!absolutePath) return;
  error.value = null;
  try {
    await revealRepositoryPath(absolutePath);
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
  }
}

export async function startWorkspaceEntryDrag(path: string, icon?: string) {
  return startWorkspaceEntriesDrag([path], icon);
}

export async function startWorkspaceEntriesDrag(paths: string[], icon?: string) {
  if (fileBrowser.value?.specialLocation === "trash") return false;
  if (activeSnapshot.value?.repository.backend.kind !== "filesystem") return false;
  const absolutePaths = normalizeSelectionPaths(paths)
    .map((path) => joinActiveRepositoryPath(path))
    .filter((path): path is string => Boolean(path));
  if (!absolutePaths.length) return false;

  error.value = null;
  try {
    await startExternalFileDrag(absolutePaths, icon);
    return true;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    return false;
  }
}
