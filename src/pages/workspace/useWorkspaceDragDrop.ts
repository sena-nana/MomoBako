import { onMounted, onUnmounted, ref, type ComputedRef } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { UnlistenFn } from "@tauri-apps/api/event";
import type {
  FileBrowserEntry,
  FileBrowserSnapshot,
  RepositorySnapshot,
} from "../../types/repository";
import {
  joinRepositoryPath,
  normalizeFilesystemPath,
  repositoryPathParts,
  trimTrailingPathSeparators,
} from "../../composables/workspace/paths";
import {
  internalWorkspaceDragDistance,
  normalizeWorkspaceMovePaths,
  resolveWorkspaceDropTarget,
  shouldDelegateToExternalDrag as shouldDelegateToExternalWorkspaceDrag,
} from "./dragBehavior";
import { createExternalDragIcon } from "./thumbnailUi";

type InternalWorkspaceDragSession = {
  startX: number;
  startY: number;
  lastX: number;
  lastY: number;
  entry: FileBrowserEntry;
  delegatedToExternalDrag: boolean;
};

type WorkspaceDragDropOptions = {
  activeRepoId: ComputedRef<string | null>;
  activeSnapshot: ComputedRef<RepositorySnapshot | null>;
  canDragEntries: ComputedRef<boolean>;
  currentDirectoryPath: ComputedRef<string>;
  dragHoverFolderPath: ComputedRef<string | null>;
  draggedWorkspacePaths: ComputedRef<string[]>;
  hasRepository: ComputedRef<boolean>;
  isFilesPanel: ComputedRef<boolean>;
  isInternalDragActive: ComputedRef<boolean>;
  isMissingRepository: ComputedRef<boolean>;
  isRepositoryWritable: ComputedRef<boolean>;
  isTrashPanel: ComputedRef<boolean>;
  selectedFilePathSet: ComputedRef<ReadonlySet<string>>;
  selectedFilePath: ComputedRef<string | null>;
  selectedFilePaths: ComputedRef<string[]>;
  attachRepository: (path: string) => Promise<unknown>;
  clearDraggedWorkspaceState: () => void;
  importEntriesToWorkspace: (sourcePaths: string[], parentPath?: string) => Promise<FileBrowserSnapshot | null>;
  moveWorkspaceEntries: (sourcePaths: string[], parentPath: string) => Promise<FileBrowserSnapshot | null>;
  selectWorkspaceEntries: (
    paths: string[],
    options?: { primaryPath?: string | null; anchorPath?: string | null },
  ) => void;
  setDragHoverFolderPath: (path: string | null) => void;
  setDraggedWorkspacePaths: (paths: string[]) => void;
  setExternalDragActive: (value: boolean) => void;
  setInternalDragActive: (value: boolean) => void;
  startWorkspaceEntriesDrag: (paths: string[], icon?: string) => Promise<boolean>;
};

const externalDragSwitchDistance = 72;

function getDroppedSourcePaths(event: DragEvent) {
  return Array.from(event.dataTransfer?.files ?? [])
    .map((file) => (file as File & { path?: string }).path ?? "")
    .filter((path) => path.trim().length > 0);
}

function isNestedDragLeave(event: DragEvent) {
  const currentTarget = event.currentTarget as HTMLElement | null;
  const relatedTarget = event.relatedTarget as Node | null;
  return Boolean(currentTarget && relatedTarget && currentTarget.contains(relatedTarget));
}

export function useWorkspaceDragDrop(options: WorkspaceDragDropOptions) {
  const isDraggingFiles = ref(false);
  const isDraggingRepositoryFolder = ref(false);
  const emptyRepositoryError = ref("");
  const internalWorkspaceDragSession = ref<InternalWorkspaceDragSession | null>(null);
  let dragDropUnlisten: UnlistenFn | null = null;

  function startInternalWorkspaceDrag(paths: string[]) {
    options.setDraggedWorkspacePaths(paths);
    options.setInternalDragActive(true);
  }

  function finishInternalWorkspaceDrag() {
    internalWorkspaceDragSession.value = null;
    options.clearDraggedWorkspaceState();
    options.setDragHoverFolderPath(null);
  }

  function resolveFolderDropTarget(clientX: number, clientY: number) {
    return resolveWorkspaceDropTarget(document, clientX, clientY, options.currentDirectoryPath.value);
  }

  function updateInternalWorkspaceHover(clientX: number, clientY: number) {
    const nextTargetPath = resolveFolderDropTarget(clientX, clientY);
    if (nextTargetPath == null) {
      options.setDragHoverFolderPath(null);
      return null;
    }
    options.setDragHoverFolderPath(nextTargetPath);
    return nextTargetPath;
  }

  function shouldDelegateToExternalDrag(clientX: number, clientY: number) {
    const session = internalWorkspaceDragSession.value;
    if (!session) return false;
    return shouldDelegateToExternalWorkspaceDrag(
      session,
      clientX,
      clientY,
      { width: window.innerWidth, height: window.innerHeight },
      externalDragSwitchDistance,
    );
  }

  async function delegateToExternalWorkspaceDrag() {
    const session = internalWorkspaceDragSession.value;
    if (!session || session.delegatedToExternalDrag || !options.draggedWorkspacePaths.value.length) return;
    internalWorkspaceDragSession.value = {
      ...session,
      delegatedToExternalDrag: true,
    };
    const dragPaths = [...options.draggedWorkspacePaths.value];
    const icon = createExternalDragIcon(session.entry);
    finishInternalWorkspaceDrag();
    await options.startWorkspaceEntriesDrag(dragPaths, icon);
  }

  function normalizeWorkspaceDragPaths(targetPath: string) {
    return normalizeWorkspaceMovePaths(options.draggedWorkspacePaths.value, targetPath);
  }

  async function moveDraggedWorkspaceEntries(targetPath: string) {
    const sourcePaths = normalizeWorkspaceDragPaths(targetPath);
    finishInternalWorkspaceDrag();
    if (!sourcePaths.length || options.isTrashPanel.value) return;
    await options.moveWorkspaceEntries(sourcePaths, targetPath);
  }

  function isInternalWorkspaceDragEvent(event: DragEvent) {
    return options.isInternalDragActive.value
      || Array.from(event.dataTransfer?.types ?? []).includes("application/x-momobako-entry");
  }

  async function handleExternalPathsDrop(paths: string[]) {
    options.setExternalDragActive(false);
    const targetPath = options.dragHoverFolderPath.value ?? options.currentDirectoryPath.value;
    options.setDragHoverFolderPath(null);

    if (options.isTrashPanel.value || !options.activeSnapshot.value) return;

    const repoRoot = options.activeSnapshot.value.repository.path;
    const filteredPaths = paths.filter((sourcePath) => {
      const normalizedSourcePath = trimTrailingPathSeparators(sourcePath);
      if (!normalizedSourcePath) return false;
      const segments = repositoryPathParts(normalizedSourcePath);
      const name = segments[segments.length - 1];
      if (!name) return false;
      const targetAbsolutePath = joinRepositoryPath(repoRoot, targetPath, name);
      return normalizeFilesystemPath(targetAbsolutePath) !== normalizeFilesystemPath(normalizedSourcePath);
    });

    if (!filteredPaths.length) return;
    await options.importEntriesToWorkspace(filteredPaths, targetPath);
  }

  async function createRepositoryFromFolder(path: string) {
    const nextPath = path.trim();
    if (!nextPath) return;
    emptyRepositoryError.value = "";
    try {
      await options.attachRepository(nextPath);
    } catch (cause) {
      emptyRepositoryError.value = cause instanceof Error ? cause.message : String(cause);
    }
  }

  function handleEntryDragStart(entry: FileBrowserEntry, event: PointerEvent) {
    if (!options.canDragEntries.value) return;
    const dragPaths = options.selectedFilePathSet.value.has(entry.path)
      ? options.selectedFilePaths.value
      : [entry.path];
    if (!options.selectedFilePathSet.value.has(entry.path)) {
      options.selectWorkspaceEntries([entry.path], { primaryPath: entry.path, anchorPath: entry.path });
    }
    startInternalWorkspaceDrag(dragPaths);
    internalWorkspaceDragSession.value = {
      startX: event.clientX,
      startY: event.clientY,
      lastX: event.clientX,
      lastY: event.clientY,
      entry,
      delegatedToExternalDrag: false,
    };
    updateInternalWorkspaceHover(event.clientX, event.clientY);
  }

  function handleEntryDragMove(event: PointerEvent) {
    if (!options.isInternalDragActive.value) return;
    if (internalWorkspaceDragSession.value) {
      internalWorkspaceDragSession.value = {
        ...internalWorkspaceDragSession.value,
        lastX: event.clientX,
        lastY: event.clientY,
      };
    }
    updateInternalWorkspaceHover(event.clientX, event.clientY);
    if (shouldDelegateToExternalDrag(event.clientX, event.clientY)) {
      void delegateToExternalWorkspaceDrag();
    }
  }

  async function handleEntryDragEnd(event: PointerEvent | null) {
    const session = internalWorkspaceDragSession.value;
    if (!session) {
      finishInternalWorkspaceDrag();
      return;
    }
    if (session.delegatedToExternalDrag) {
      finishInternalWorkspaceDrag();
      return;
    }

    const targetPath = event ? updateInternalWorkspaceHover(event.clientX, event.clientY) : options.dragHoverFolderPath.value;
    if (!targetPath) {
      finishInternalWorkspaceDrag();
      return;
    }
    await moveDraggedWorkspaceEntries(targetPath);
  }

  function handleBoxSelection(paths: string[], mode: "replace" | "append") {
    if (mode === "append") {
      if (!paths.length) return;
      const nextPaths = Array.from(new Set([...options.selectedFilePaths.value, ...paths]));
      options.selectWorkspaceEntries(nextPaths, {
        primaryPath: options.selectedFilePath.value ?? paths[0] ?? null,
        anchorPath: options.selectedFilePath.value ?? paths[0] ?? null,
      });
      return;
    }

    if (!paths.length) {
      options.selectWorkspaceEntries([]);
      return;
    }
    options.selectWorkspaceEntries(paths, { primaryPath: paths[0], anchorPath: paths[0] });
  }

  function handleFolderDropHover(path: string) {
    if (options.isTrashPanel.value) return;
    options.setDragHoverFolderPath(path);
  }

  function handleFolderDropLeave(path: string) {
    if (options.dragHoverFolderPath.value === path) {
      options.setDragHoverFolderPath(null);
    }
  }

  async function handleFolderDrop(path: string, event: DragEvent) {
    if (isInternalWorkspaceDragEvent(event)) {
      await moveDraggedWorkspaceEntries(path);
      return;
    }

    const sourcePaths = getDroppedSourcePaths(event);
    if (!sourcePaths.length) return;
    options.setExternalDragActive(false);
    isDraggingFiles.value = false;
    options.setDragHoverFolderPath(null);
    await options.importEntriesToWorkspace(sourcePaths, path);
  }

  function handleWindowPointerLeave(event: PointerEvent) {
    const session = internalWorkspaceDragSession.value;
    if (!session || session.delegatedToExternalDrag || !options.isInternalDragActive.value) return;
    internalWorkspaceDragSession.value = {
      ...session,
      lastX: event.clientX,
      lastY: event.clientY,
    };
    if (internalWorkspaceDragDistance(internalWorkspaceDragSession.value) >= externalDragSwitchDistance) {
      void delegateToExternalWorkspaceDrag();
    }
  }

  function handleWindowBlur() {
    const session = internalWorkspaceDragSession.value;
    if (!session || session.delegatedToExternalDrag || !options.isInternalDragActive.value) return;
    if (internalWorkspaceDragDistance(session) >= externalDragSwitchDistance) {
      void delegateToExternalWorkspaceDrag();
    }
  }

  function handleDragOver(event: DragEvent) {
    if (!options.isRepositoryWritable.value || !options.isFilesPanel.value) return;
    event.preventDefault();
    if (isInternalWorkspaceDragEvent(event)) {
      if (event.dataTransfer) {
        event.dataTransfer.dropEffect = "move";
      }
      return;
    }
    if (event.dataTransfer) {
      event.dataTransfer.dropEffect = "copy";
    }
    options.setExternalDragActive(true);
    isDraggingFiles.value = true;
  }

  function handleDragLeave(event: DragEvent) {
    if (isNestedDragLeave(event)) return;
    if (options.isInternalDragActive.value) return;
    options.setExternalDragActive(false);
    options.setDragHoverFolderPath(null);
    isDraggingFiles.value = false;
  }

  async function handleDrop(event: DragEvent) {
    event.preventDefault();
    if (isInternalWorkspaceDragEvent(event)) {
      await moveDraggedWorkspaceEntries(options.dragHoverFolderPath.value ?? options.currentDirectoryPath.value);
      return;
    }
    options.setExternalDragActive(false);
    isDraggingFiles.value = false;
    if (!options.isRepositoryWritable.value || options.isTrashPanel.value) return;
    const sourcePaths = getDroppedSourcePaths(event);
    if (!sourcePaths.length) return;
    await handleExternalPathsDrop(sourcePaths);
  }

  function handleEmptyRepositoryDragOver(event: DragEvent) {
    if (options.activeRepoId.value || options.hasRepository.value) return;
    event.preventDefault();
    if (event.dataTransfer) {
      event.dataTransfer.dropEffect = "copy";
    }
    isDraggingRepositoryFolder.value = true;
  }

  function handleEmptyRepositoryDragLeave(event: DragEvent) {
    if (isNestedDragLeave(event)) return;
    isDraggingRepositoryFolder.value = false;
  }

  async function handleEmptyRepositoryDrop(event: DragEvent) {
    event.preventDefault();
    isDraggingRepositoryFolder.value = false;
    if (options.activeRepoId.value || options.hasRepository.value) return;
    const [path] = getDroppedSourcePaths(event);
    if (path) {
      await createRepositoryFromFolder(path);
    }
  }

  onMounted(() => {
    window.addEventListener("pointerleave", handleWindowPointerLeave);
    window.addEventListener("blur", handleWindowBlur);
    try {
      const currentWindow = getCurrentWindow();
      currentWindow.onDragDropEvent(({ payload }) => {
        if (!options.hasRepository.value && !options.isMissingRepository.value) {
          if (payload.type === "enter" || payload.type === "over") {
            isDraggingRepositoryFolder.value = true;
            return;
          }
          if (payload.type === "leave") {
            isDraggingRepositoryFolder.value = false;
            return;
          }
          isDraggingRepositoryFolder.value = false;
          if (payload.paths.length) {
            void createRepositoryFromFolder(payload.paths[0]);
          }
          return;
        }
        if (!options.isRepositoryWritable.value || !options.isFilesPanel.value) return;
        if (payload.type === "enter" || payload.type === "over") {
          options.setExternalDragActive(true);
          isDraggingFiles.value = true;
          return;
        }
        if (payload.type === "leave") {
          options.setExternalDragActive(false);
          options.setDragHoverFolderPath(null);
          isDraggingFiles.value = false;
          return;
        }
        options.setExternalDragActive(false);
        isDraggingFiles.value = false;
        if (payload.paths.length) {
          void handleExternalPathsDrop(payload.paths);
        }
      }).then((unlisten) => {
        dragDropUnlisten = unlisten;
      }).catch(() => {
        dragDropUnlisten = null;
      });
    } catch {
      dragDropUnlisten = null;
    }
  });

  onUnmounted(() => {
    dragDropUnlisten?.();
    window.removeEventListener("pointerleave", handleWindowPointerLeave);
    window.removeEventListener("blur", handleWindowBlur);
    options.setExternalDragActive(false);
    options.setDragHoverFolderPath(null);
    options.clearDraggedWorkspaceState();
    isDraggingFiles.value = false;
    isDraggingRepositoryFolder.value = false;
  });

  return {
    emptyRepositoryError,
    isDraggingFiles,
    isDraggingRepositoryFolder,
    handleBoxSelection,
    handleDragLeave,
    handleDragOver,
    handleDrop,
    handleEmptyRepositoryDragLeave,
    handleEmptyRepositoryDragOver,
    handleEmptyRepositoryDrop,
    handleEntryDragEnd,
    handleEntryDragMove,
    handleEntryDragStart,
    handleFolderDrop,
    handleFolderDropHover,
    handleFolderDropLeave,
  };
}
