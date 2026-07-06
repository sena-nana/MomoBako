import {
  copyEntries,
  createDirectory,
  createFile,
  deleteEntry,
  importArchiveEntries,
  importEagleLibrary,
  importEntries,
  moveEntries,
  mutateTrash,
  openRepositoryPath,
  recordEntryAccess,
  renameEntry,
  revealRepositoryPath,
  startExternalFileDrag,
} from "../../services/repositoryApi";
import type {
  FileBrowserEntry,
  FileBrowserSnapshot,
  FileDeleteMode,
  EagleImportMode,
} from "../../types/repository";
import {
  activeLibraryCategory,
  activePanel,
  activeRepoId,
  activeSnapshot,
  currentDirectoryPath,
  error,
  fileBrowser,
  isMutatingFiles,
  selectedFilePaths,
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
import { emitSystemLogSilently } from "../../services/systemLog";

function defaultDirectoryRefreshPlan(paths: string[]): WorkspaceRefreshPlan["directory"] {
  if (activePanel.value === "trash") return "trash";
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

function isLibraryCategoryView() {
  return activePanel.value === "files" && activeLibraryCategory.value !== "all";
}

function canApplyDirectorySnapshot(snapshot: FileBrowserSnapshot) {
  return !isLibraryCategoryView() || currentDirectoryPath.value === snapshot.currentPath;
}

type WorkspaceOpenTarget = string | Pick<FileBrowserEntry, "isVirtual" | "localAbsolutePath" | "path">;

function hasExplicitWorkspaceSelection() {
  return Boolean(selectedFilePath.value || selectedFilePaths.value.length);
}

function resolveWorkspaceEntryPath(target: WorkspaceOpenTarget) {
  if (typeof target === "string") {
    return {
      absolutePath: joinActiveRepositoryPath(target),
      relativePath: target,
    };
  }
  if (target.localAbsolutePath?.trim()) {
    return {
      absolutePath: target.localAbsolutePath,
      relativePath: target.path,
    };
  }
  if (target.isVirtual || activeSnapshot.value?.repository.backend.kind !== "filesystem") {
    return {
      absolutePath: null,
      relativePath: target.path,
    };
  }
  return {
    absolutePath: joinActiveRepositoryPath(target.path),
    relativePath: target.path,
  };
}

async function refreshAfterFileMutation(repoId: string, plan: WorkspaceRefreshPlan) {
  await refreshWorkspaceAfterMutation(repoId, plan, loadFileBrowserForDirectory);
}

async function finishFileTransfer(
  repoId: string,
  snapshot: FileBrowserSnapshot,
  sourcePaths: string[],
) {
  if (canApplyDirectorySnapshot(snapshot)) {
    applyFileBrowserSnapshot(snapshot);
    const sourceNames = new Set(sourcePaths.map(entryNameFromPath));
    const nextSelection = snapshot.entries
      .filter((entry) => sourceNames.has(entry.name))
      .map((entry) => entry.path);
    if (nextSelection.length) {
      applyWorkspaceSelection(nextSelection, nextSelection[0], nextSelection[0]);
    }
  }
  await refreshAfterFileMutation(repoId, { hardlinkCandidates: true, repositorySnapshot: true });
}

async function finishWorkspaceImport(
  repoId: string,
  snapshot: FileBrowserSnapshot,
  previousPathSet: ReadonlySet<string>,
  parentPath: string,
) {
  if (canApplyDirectorySnapshot(snapshot)) {
    applyFileBrowserSnapshot(snapshot);
    const nextSelection = snapshot.entries
      .filter((entry) => parent_relative_path(entry.path) === parentPath)
      .filter((entry) => !previousPathSet.has(entry.path))
      .map((entry) => entry.path);
    if (nextSelection.length) {
      applyWorkspaceSelection(nextSelection, nextSelection[0], nextSelection[0]);
    }
  }
  await refreshAfterFileMutation(repoId, { hardlinkCandidates: true, repositorySnapshot: true });
}

export async function createDirectoryInWorkspace(name: string, parentPath = currentDirectoryPath.value) {
  if (!activeRepoId.value) return null;
  emitSystemLogSilently("info", {
    category: "file",
    action: "createDirectoryStart",
    message: "开始创建文件夹。",
    repoId: activeRepoId.value,
    context: { name, parentPath },
  });
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
    emitSystemLogSilently("info", {
      category: "file",
      action: "createDirectorySuccess",
      message: "文件夹创建完成。",
      repoId: activeRepoId.value,
      context: { name, parentPath },
    });
    return snapshot;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    emitSystemLogSilently("error", {
      category: "file",
      action: "createDirectoryFailed",
      message: "文件夹创建失败。",
      repoId: activeRepoId.value,
      context: { name, parentPath, error: error.value },
    });
    return null;
  } finally {
    isMutatingFiles.value = false;
  }
}

export async function createFileInWorkspace(name: string, parentPath = currentDirectoryPath.value) {
  if (!activeRepoId.value) return null;
  emitSystemLogSilently("info", {
    category: "file",
    action: "createFileStart",
    message: "开始创建文件。",
    repoId: activeRepoId.value,
    context: { name, parentPath },
  });
  isMutatingFiles.value = true;
  error.value = null;
  const hadExplicitSelection = hasExplicitWorkspaceSelection();
  try {
    const snapshot = await createFile({
      repoId: activeRepoId.value,
      parentPath,
      name,
    });
    applyFileBrowserSnapshot(snapshot);
    const createdPath = snapshot.entries.find((entry) => entry.name === name)?.path ?? null;
    if (createdPath && hadExplicitSelection) {
      applyWorkspaceSelection([createdPath], createdPath, createdPath);
    }
    await refreshAfterFileMutation(activeRepoId.value, { repositorySnapshot: true });
    emitSystemLogSilently("info", {
      category: "file",
      action: "createFileSuccess",
      message: "文件创建完成。",
      repoId: activeRepoId.value,
      context: { name, parentPath, path: createdPath },
    });
    return snapshot;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    emitSystemLogSilently("error", {
      category: "file",
      action: "createFileFailed",
      message: "文件创建失败。",
      repoId: activeRepoId.value,
      context: { name, parentPath, error: error.value },
    });
    return null;
  } finally {
    isMutatingFiles.value = false;
  }
}

export async function importEntriesToWorkspace(sourcePaths: string[], parentPath = currentDirectoryPath.value) {
  const repoId = activeRepoId.value;
  if (!repoId || !sourcePaths.length) return null;
  emitSystemLogSilently("info", {
    category: "file",
    action: "importStart",
    message: "开始导入条目。",
    repoId,
    context: { parentPath, count: sourcePaths.length },
  });
  error.value = null;
  const hadExplicitSelection = hasExplicitWorkspaceSelection();
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
    await finishFileTransfer(repoId, snapshot, hadExplicitSelection ? sourcePaths : []);
    emitSystemLogSilently("info", {
      category: "file",
      action: "importSuccess",
      message: "条目导入完成。",
      repoId,
      context: { parentPath, count: sourcePaths.length },
    });
    finishOperationProgress(progressId);
    return snapshot;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    emitSystemLogSilently("error", {
      category: "file",
      action: "importFailed",
      message: "条目导入失败。",
      repoId,
      context: { parentPath, count: sourcePaths.length, error: error.value },
    });
    cancelOperationProgress(progressId);
    return null;
  }
}

export async function importArchiveEntriesToWorkspace(
  archivePath: string,
  parentPath = currentDirectoryPath.value,
) {
  const repoId = activeRepoId.value;
  if (!repoId || !archivePath.trim()) return null;
  emitSystemLogSilently("info", {
    category: "file",
    action: "importArchiveStart",
    message: "开始导入压缩包。",
    repoId,
    context: { archivePath, parentPath },
  });
  isMutatingFiles.value = true;
  error.value = null;
  const hadExplicitSelection = hasExplicitWorkspaceSelection();
  const previousPathSet = new Set((fileBrowser.value?.entries ?? []).map((entry) => entry.path));
  const progressId = startOperationProgress("导入 ZIP", "准备解压并导入 ZIP", { initial: 8 });
  try {
    updateOperationProgress(progressId, { detail: "预检压缩包条目", value: 24 });
    const snapshot = await importArchiveEntries({
      repoId,
      parentPath,
      archivePath,
    });
    updateOperationProgress(progressId, { detail: "刷新文件索引", value: 84 });
    await finishWorkspaceImport(
      repoId,
      snapshot,
      hadExplicitSelection ? previousPathSet : new Set([...previousPathSet, ...snapshot.entries.map((entry) => entry.path)]),
      parentPath ?? "",
    );
    emitSystemLogSilently("info", {
      category: "file",
      action: "importArchiveSuccess",
      message: "压缩包导入完成。",
      repoId,
      context: { archivePath, parentPath },
    });
    finishOperationProgress(progressId);
    return snapshot;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    emitSystemLogSilently("error", {
      category: "file",
      action: "importArchiveFailed",
      message: "压缩包导入失败。",
      repoId,
      context: { archivePath, parentPath, error: error.value },
    });
    cancelOperationProgress(progressId);
    return null;
  } finally {
    isMutatingFiles.value = false;
  }
}

export async function importEagleLibraryToWorkspace(
  libraryPath: string,
  mode: EagleImportMode,
  parentPath = currentDirectoryPath.value,
) {
  const repoId = activeRepoId.value;
  if (!repoId || !libraryPath.trim()) return null;
  emitSystemLogSilently("info", {
    category: "file",
    action: "importEagleStart",
    message: "开始导入 Eagle 资源库。",
    repoId,
    context: { libraryPath, mode, parentPath },
  });
  isMutatingFiles.value = true;
  error.value = null;
  const hadExplicitSelection = hasExplicitWorkspaceSelection();
  const previousPathSet = new Set((fileBrowser.value?.entries ?? []).map((entry) => entry.path));
  const progressId = startOperationProgress(
    mode === "move" ? "剪切导入 Eagle" : "复制导入 Eagle",
    "准备转换 EagleLibrary",
    { initial: 8 },
  );
  try {
    updateOperationProgress(progressId, { detail: "转换 EagleLibrary", value: 24 });
    const response = await importEagleLibrary({
      repoId,
      parentPath,
      libraryPath,
      mode,
    });
    updateOperationProgress(progressId, { detail: "刷新文件索引", value: 84 });
    await finishWorkspaceImport(
      repoId,
      response.snapshot,
      hadExplicitSelection ? previousPathSet : new Set([...previousPathSet, ...response.snapshot.entries.map((entry) => entry.path)]),
      parentPath ?? "",
    );
    emitSystemLogSilently("info", {
      category: "file",
      action: "importEagleSuccess",
      message: "Eagle 资源库导入完成。",
      repoId,
      context: { libraryPath, mode, parentPath },
    });
    finishOperationProgress(progressId);
    return response;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    emitSystemLogSilently("error", {
      category: "file",
      action: "importEagleFailed",
      message: "Eagle 资源库导入失败。",
      repoId,
      context: { libraryPath, mode, parentPath, error: error.value },
    });
    cancelOperationProgress(progressId);
    return null;
  } finally {
    isMutatingFiles.value = false;
  }
}

export async function copyWorkspaceEntries(sourcePaths: string[], parentPath = currentDirectoryPath.value) {
  const repoId = activeRepoId.value;
  if (!repoId || !sourcePaths.length) return null;
  emitSystemLogSilently("info", {
    category: "file",
    action: "copyStart",
    message: "开始复制条目。",
    repoId,
    context: { sourcePaths, parentPath },
  });
  isMutatingFiles.value = true;
  error.value = null;
  const hadExplicitSelection = hasExplicitWorkspaceSelection();
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
    await finishFileTransfer(repoId, snapshot, hadExplicitSelection ? sourcePaths : []);
    emitSystemLogSilently("info", {
      category: "file",
      action: "copySuccess",
      message: "条目复制完成。",
      repoId,
      context: { sourcePaths, parentPath },
    });
    finishOperationProgress(progressId);
    return snapshot;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    emitSystemLogSilently("error", {
      category: "file",
      action: "copyFailed",
      message: "条目复制失败。",
      repoId,
      context: { sourcePaths, parentPath, error: error.value },
    });
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
  emitSystemLogSilently("info", {
    category: "file",
    action: "moveStart",
    message: "开始移动条目。",
    repoId,
    context: { sourcePaths: nextPaths, parentPath },
  });

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

    emitSystemLogSilently("info", {
      category: "file",
      action: "moveSuccess",
      message: "条目移动完成。",
      repoId,
      context: { sourcePaths: nextPaths, parentPath },
    });
    finishOperationProgress(progressId);
    return snapshot;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    emitSystemLogSilently("error", {
      category: "file",
      action: "moveFailed",
      message: "条目移动失败。",
      repoId,
      context: { sourcePaths: nextPaths, parentPath, error: error.value },
    });
    cancelOperationProgress(progressId);
    return null;
  } finally {
    isMutatingFiles.value = false;
  }
}

export async function renameWorkspaceEntry(path: string, newName: string) {
  if (!activeRepoId.value) return null;
  emitSystemLogSilently("info", {
    category: "file",
    action: "renameStart",
    message: "开始重命名条目。",
    repoId: activeRepoId.value,
    context: { path, newName },
  });
  isMutatingFiles.value = true;
  error.value = null;
  try {
    const snapshot = await renameEntry({
      repoId: activeRepoId.value,
      path,
      newName,
    });
    const renamedPath = snapshot.entries.find((entry) => entry.name === newName)?.path ?? null;
    if (canApplyDirectorySnapshot(snapshot)) {
      applyFileBrowserSnapshot(snapshot);
    }
    if (renamedPath) {
      applyWorkspaceSelection([renamedPath], renamedPath, renamedPath);
    }
    await refreshAfterFileMutation(activeRepoId.value, { repositorySnapshot: true });
    emitSystemLogSilently("info", {
      category: "file",
      action: "renameSuccess",
      message: "条目重命名完成。",
      repoId: activeRepoId.value,
      context: { path, newName, renamedPath },
    });
    return snapshot;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    emitSystemLogSilently("error", {
      category: "file",
      action: "renameFailed",
      message: "条目重命名失败。",
      repoId: activeRepoId.value,
      context: { path, newName, error: error.value },
    });
    return null;
  } finally {
    isMutatingFiles.value = false;
  }
}

export async function deleteWorkspaceEntry(path: string, mode?: FileDeleteMode) {
  if (!activeRepoId.value) return null;
  emitSystemLogSilently("warn", {
    category: "file",
    action: "deleteStart",
    message: "开始删除条目。",
    repoId: activeRepoId.value,
    context: { path, mode: mode ?? null },
  });
  isMutatingFiles.value = true;
  error.value = null;
  try {
    const deleteMode = mode ?? (activePanel.value === "trash" ? "permanentDelete" : undefined);
    const snapshot = await deleteEntry({
      repoId: activeRepoId.value,
      path,
      mode: deleteMode,
    });
    if (canApplyDirectorySnapshot(snapshot)) {
      applyFileBrowserSnapshot(snapshot);
    } else if (selectedFilePath.value === path) {
      applyWorkspaceSelection([], null, null);
    }
    await refreshAfterFileMutation(activeRepoId.value, { repositorySnapshot: true });
    emitSystemLogSilently("warn", {
      category: "file",
      action: "deleteSuccess",
      message: "条目删除完成。",
      repoId: activeRepoId.value,
      context: { path, mode: deleteMode ?? null },
    });
    return snapshot;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    emitSystemLogSilently("error", {
      category: "file",
      action: "deleteFailed",
      message: "条目删除失败。",
      repoId: activeRepoId.value,
      context: { path, mode: mode ?? null, error: error.value },
    });
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
    const deleteMode = mode ?? (activePanel.value === "trash" ? "permanentDelete" : undefined);
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
    applyWorkspaceSelection([], null, null);
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
    applyFileBrowserSnapshot(snapshot);
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
    applyWorkspaceSelection([], null, null);
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
  emitSystemLogSilently("warn", {
    category: "file",
    action: "emptyTrashStart",
    message: "开始清空回收站。",
    repoId: activeRepoId.value,
  });
  isMutatingFiles.value = true;
  error.value = null;
  try {
    const snapshot = await mutateTrash({
      repoId: activeRepoId.value,
      action: "empty",
    });
    applyFileBrowserSnapshot(snapshot);
    applyWorkspaceSelection([], null, null);
    await refreshAfterFileMutation(activeRepoId.value, {
      repositorySnapshot: true,
      repositorySummary: true,
    });
    emitSystemLogSilently("warn", {
      category: "file",
      action: "emptyTrashSuccess",
      message: "回收站已清空。",
      repoId: activeRepoId.value,
    });
    return snapshot;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    emitSystemLogSilently("error", {
      category: "file",
      action: "emptyTrashFailed",
      message: "清空回收站失败。",
      repoId: activeRepoId.value,
      context: { error: error.value },
    });
    return null;
  } finally {
    isMutatingFiles.value = false;
  }
}

export async function openWorkspaceEntry(target: WorkspaceOpenTarget) {
  const repoId = activeRepoId.value;
  const { absolutePath, relativePath } = resolveWorkspaceEntryPath(target);
  if (!repoId || !absolutePath) return;
  error.value = null;
  try {
    await recordEntryAccess({ repoId, path: relativePath });
    await openRepositoryPath(absolutePath);
    emitSystemLogSilently("info", {
      category: "file",
      action: "openExternal",
      message: "已用系统程序打开条目。",
      repoId,
      context: { path: relativePath, absolutePath },
    });
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    emitSystemLogSilently("error", {
      category: "file",
      action: "openExternalFailed",
      message: "打开条目失败。",
      repoId,
      context: { path: relativePath, absolutePath, error: error.value },
    });
  }
}

export async function revealWorkspaceEntry(target: WorkspaceOpenTarget) {
  const { absolutePath } = resolveWorkspaceEntryPath(target);
  if (!absolutePath) return;
  error.value = null;
  try {
    await revealRepositoryPath(absolutePath);
    emitSystemLogSilently("info", {
      category: "file",
      action: "revealExternal",
      message: "已在系统文件管理器中定位条目。",
      repoId: activeRepoId.value,
      context: { absolutePath },
    });
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    emitSystemLogSilently("error", {
      category: "file",
      action: "revealExternalFailed",
      message: "定位条目失败。",
      repoId: activeRepoId.value,
      context: { absolutePath, error: error.value },
    });
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

function parent_relative_path(path: string) {
  const normalized = path.replace(/\\/g, "/");
  const index = normalized.lastIndexOf("/");
  return index >= 0 ? normalized.slice(0, index) : "";
}
