import { computed, onBeforeUnmount, ref, watch, type ComputedRef } from "vue";
import { normalizeWorkspaceMovePaths } from "../pages/workspace/dragBehavior";
import { scheduleIdleTask } from "../composables/workspace/scheduler";
import type { FileBrowserSnapshot, FileDeleteMode, FileTreeNode } from "../types/repository";

type FolderSidebarUiOptions = {
  activeRepoId: ComputedRef<string | null>;
  activePanel: ComputedRef<string>;
  clearDraggedWorkspaceState: () => void;
  createDirectoryInWorkspace: (name: string, parentPath?: string) => Promise<FileBrowserSnapshot | null>;
  currentDirectoryPath: ComputedRef<string>;
  deleteWorkspaceEntry: (path: string, mode?: FileDeleteMode) => Promise<FileBrowserSnapshot | null>;
  dragHoverFolderPath: ComputedRef<string | null>;
  draggedWorkspacePaths: ComputedRef<string[]>;
  fileTree: ComputedRef<FileTreeNode[]>;
  importEntriesToWorkspace: (sourcePaths: string[], targetPath?: string) => Promise<FileBrowserSnapshot | null>;
  isExternalDragActive: ComputedRef<boolean>;
  isInternalDragActive: ComputedRef<boolean>;
  isMutatingFiles: ComputedRef<boolean>;
  loadFileBrowserForDirectory: (directoryPath?: string) => Promise<unknown>;
  moveWorkspaceEntries: (sourcePaths: string[], targetPath: string) => Promise<FileBrowserSnapshot | null>;
  renameWorkspaceEntry: (path: string, nextName: string) => Promise<FileBrowserSnapshot | null>;
  setActivePanel: (panel: "files") => void;
  setDragHoverFolderPath: (path: string | null) => void;
};

function getDroppedSourcePaths(event: DragEvent) {
  return Array.from(event.dataTransfer?.files ?? [])
    .map((file) => (file as File & { path?: string }).path ?? "")
    .filter((path) => path.trim().length > 0);
}

export function useFolderSidebarUi(options: FolderSidebarUiOptions) {
  const expandedFolderPaths = ref<string[]>([]);
  const showFolderDialog = ref(false);
  const folderDialogMode = ref<"create" | "rename">("create");
  const folderDialogParentPath = ref("");
  const folderDialogTargetPath = ref("");
  const folderDialogLabel = ref("");
  const folderDialogValue = ref("");
  const showFolderDeleteDialog = ref(false);
  const pendingDeleteFolderPath = ref("");
  const pendingDeleteFolderLabel = ref("");

  const isTrashPanel = computed(() => options.activePanel.value === "deleted");
  const isFolderDragActive = computed(() => options.isExternalDragActive.value || options.isInternalDragActive.value);
  const expandedFolderPathSet = computed(() => new Set(expandedFolderPaths.value));
  const fileTreeNodes = computed(() => options.fileTree.value);
  const folderDialogTitle = computed(() => (
    folderDialogMode.value === "create" ? "新建文件夹" : "重命名文件夹"
  ));
  const folderDialogActionLabel = computed(() => (
    folderDialogMode.value === "create" ? "创建" : "保存"
  ));
  const folderDialogPlaceholder = computed(() => (
    folderDialogMode.value === "create" ? "输入文件夹名称" : "输入新的文件夹名称"
  ));
  const folderDialogDisabled = computed(() => !folderDialogValue.value.trim() || options.isMutatingFiles.value);

  let folderHoverSwitchTimer: number | null = null;
  let pendingHoverFolderPath: string | null = null;
  let cancelFolderPathValidation: (() => void) | null = null;

  function openFolder(path: string) {
    options.setActivePanel("files");
    void options.loadFileBrowserForDirectory(path);
  }

  function ensureFolderExpanded(path: string) {
    if (!path) return;
    const next = new Set(expandedFolderPaths.value);
    next.add(path);
    expandedFolderPaths.value = Array.from(next);
  }

  function clearFolderHoverTimer() {
    if (folderHoverSwitchTimer != null) {
      window.clearTimeout(folderHoverSwitchTimer);
      folderHoverSwitchTimer = null;
    }
    pendingHoverFolderPath = null;
  }

  function clearFolderDragHover() {
    clearFolderHoverTimer();
    options.setDragHoverFolderPath(null);
  }

  function handleFolderDragHover(path: string) {
    if (!options.activeRepoId.value || isTrashPanel.value || !isFolderDragActive.value) return;
    options.setDragHoverFolderPath(path);
    if (pendingHoverFolderPath === path) return;

    clearFolderHoverTimer();
    pendingHoverFolderPath = path;
    folderHoverSwitchTimer = window.setTimeout(() => {
      ensureFolderExpanded(path);
      openFolder(path);
      folderHoverSwitchTimer = null;
      pendingHoverFolderPath = null;
    }, 450);
  }

  function handleFolderDragLeave(path: string) {
    if (options.dragHoverFolderPath.value === path) {
      options.setDragHoverFolderPath(null);
    }
    if (pendingHoverFolderPath === path) {
      clearFolderHoverTimer();
    }
  }

  async function handleFolderDrop(path: string, event: DragEvent) {
    clearFolderDragHover();

    if (!options.activeRepoId.value || isTrashPanel.value) {
      options.clearDraggedWorkspaceState();
      return;
    }

    if (options.isInternalDragActive.value && options.draggedWorkspacePaths.value.length) {
      const sourcePaths = normalizeWorkspaceMovePaths(options.draggedWorkspacePaths.value, path);
      if (!sourcePaths.length) {
        options.clearDraggedWorkspaceState();
        return;
      }
      await options.moveWorkspaceEntries(sourcePaths, path);
      options.clearDraggedWorkspaceState();
      return;
    }

    const sourcePaths = getDroppedSourcePaths(event);
    if (sourcePaths.length) {
      await options.importEntriesToWorkspace(sourcePaths, path);
    }
  }

  function toggleFolderExpansion(path: string) {
    const next = new Set(expandedFolderPaths.value);
    if (next.has(path)) {
      next.delete(path);
    } else {
      next.add(path);
    }
    expandedFolderPaths.value = Array.from(next);
  }

  function openCreateFolderDialog(parentPath = "") {
    folderDialogMode.value = "create";
    folderDialogParentPath.value = parentPath;
    folderDialogTargetPath.value = "";
    folderDialogLabel.value = "";
    folderDialogValue.value = "";
    showFolderDialog.value = true;
  }

  function openRenameFolderDialog(path: string, label: string) {
    folderDialogMode.value = "rename";
    folderDialogParentPath.value = "";
    folderDialogTargetPath.value = path;
    folderDialogLabel.value = label;
    folderDialogValue.value = label;
    showFolderDialog.value = true;
  }

  function closeFolderDialog() {
    if (options.isMutatingFiles.value) return;
    showFolderDialog.value = false;
  }

  async function submitFolderDialog() {
    const value = folderDialogValue.value.trim();
    if (!value) return;

    if (folderDialogMode.value === "create") {
      const snapshot = await options.createDirectoryInWorkspace(value, folderDialogParentPath.value);
      if (snapshot) {
        ensureFolderExpanded(folderDialogParentPath.value);
        showFolderDialog.value = false;
      }
      return;
    }

    const snapshot = await options.renameWorkspaceEntry(folderDialogTargetPath.value, value);
    if (snapshot) {
      showFolderDialog.value = false;
    }
  }

  function openDeleteFolderDialog(path: string, label: string) {
    pendingDeleteFolderPath.value = path;
    pendingDeleteFolderLabel.value = label;
    showFolderDeleteDialog.value = true;
  }

  function closeDeleteFolderDialog() {
    if (options.isMutatingFiles.value) return;
    showFolderDeleteDialog.value = false;
  }

  async function confirmDeleteFolder(mode: FileDeleteMode) {
    const snapshot = await options.deleteWorkspaceEntry(pendingDeleteFolderPath.value, mode);
    if (snapshot) {
      showFolderDeleteDialog.value = false;
    }
  }

  watch(
    fileTreeNodes,
    (nodes) => {
      cancelFolderPathValidation?.();
      cancelFolderPathValidation = scheduleIdleTask(() => {
        const validPaths = new Set<string>([""]);
        const collectPaths = (items: FileTreeNode[]) => {
          for (const item of items) {
            validPaths.add(item.path);
            collectPaths(item.children);
          }
        };
        collectPaths(nodes);
        expandedFolderPaths.value = expandedFolderPaths.value.filter((path) => validPaths.has(path));
      }, 200);
    },
  );

  watch(
    options.currentDirectoryPath,
    (path) => {
      if (path == null) return;
      const segments = path ? path.split("/") : [];
      let cursor = "";
      for (const segment of segments) {
        cursor = cursor ? `${cursor}/${segment}` : segment;
        ensureFolderExpanded(cursor);
      }
    },
    { immediate: true },
  );

  watch(isFolderDragActive, (active) => {
    if (!active) {
      clearFolderDragHover();
    }
  });

  onBeforeUnmount(() => {
    clearFolderDragHover();
    cancelFolderPathValidation?.();
  });

  return {
    closeDeleteFolderDialog,
    closeFolderDialog,
    confirmDeleteFolder,
    expandedFolderPathSet,
    fileTreeNodes,
    folderDialogActionLabel,
    folderDialogDisabled,
    folderDialogLabel,
    folderDialogMode,
    folderDialogParentPath,
    folderDialogPlaceholder,
    folderDialogTitle,
    folderDialogValue,
    handleFolderDragHover,
    handleFolderDragLeave,
    handleFolderDrop,
    isFolderDragActive,
    isTrashPanel,
    openCreateFolderDialog,
    openDeleteFolderDialog,
    openFolder,
    openRenameFolderDialog,
    pendingDeleteFolderLabel,
    showFolderDeleteDialog,
    showFolderDialog,
    submitFolderDialog,
    toggleFolderExpansion,
  };
}
