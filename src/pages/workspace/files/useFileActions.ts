import { computed, ref, watch, type ComputedRef } from "vue";
import type {
  EagleLibraryImportResponse,
  FileBrowserEntry,
  FileBrowserSnapshot,
  FileDeleteMode,
  HardlinkCandidate,
  HardlinkConfirmResponse,
} from "../../../types/repository";
import { normalizeRepositoryRelativePath } from "../../../composables/workspace/paths";

type WorkspaceFileActionsOptions = {
  currentFileEntry: ComputedRef<FileBrowserEntry | null>;
  fileBrowser: ComputedRef<FileBrowserSnapshot | null>;
  hasMultipleSelection: ComputedRef<boolean>;
  hardlinkCandidates: ComputedRef<HardlinkCandidate[]>;
  isTrashPanel: ComputedRef<boolean>;
  selectedFilePathSet: ComputedRef<ReadonlySet<string>>;
  selectedFilePaths: ComputedRef<string[]>;
  confirmWorkspaceHardlinkCandidate: (candidateId: string) => Promise<HardlinkConfirmResponse | null>;
  copyWorkspaceEntries: (sourcePaths: string[], parentPath?: string) => Promise<FileBrowserSnapshot | null>;
  createFileInWorkspace: (name: string) => Promise<FileBrowserSnapshot | null>;
  deleteWorkspaceEntries: (paths: string[], mode?: FileDeleteMode) => Promise<FileBrowserSnapshot | null>;
  deleteWorkspaceEntry: (path: string, mode?: FileDeleteMode) => Promise<FileBrowserSnapshot | null>;
  emptyTrash: () => Promise<FileBrowserSnapshot | null>;
  importArchiveEntriesToWorkspace: (archivePath: string, parentPath?: string) => Promise<FileBrowserSnapshot | null>;
  importEagleLibraryToWorkspace: (
    libraryPath: string,
    mode: "copy" | "move",
    parentPath?: string,
  ) => Promise<EagleLibraryImportResponse | null>;
  importEntriesToWorkspace: (sourcePaths: string[], parentPath?: string) => Promise<FileBrowserSnapshot | null>;
  openDirectory: (path: string) => void;
  openWorkspaceEntry: (path: string) => Promise<void>;
  renameWorkspaceEntry: (path: string, newName: string) => Promise<FileBrowserSnapshot | null>;
  restoreAllTrashEntries: () => Promise<FileBrowserSnapshot | null>;
  restoreTrashEntries: (paths: string[]) => Promise<FileBrowserSnapshot | null>;
  restoreTrashEntry: (path: string) => Promise<FileBrowserSnapshot | null>;
  revealWorkspaceEntry: (path: string) => Promise<void>;
};

export function useFileActions(options: WorkspaceFileActionsOptions) {
  const createFileName = ref("");
  const renameValue = ref("");
  const renameTargetPath = ref<string | null>(null);
  const pendingCopySourcePaths = ref<string[]>([]);
  const copyTargetDialogOpen = ref(false);
  const copyTargetPath = ref("");
  const skippedHardlinkCandidateIds = ref<Set<string>>(new Set());

  const currentHardlinkCandidate = computed(() => (
    options.hardlinkCandidates.value.find((candidate) => !skippedHardlinkCandidateIds.value.has(candidate.candidateId)) ?? null
  ));

  watch([options.currentFileEntry, options.hasMultipleSelection], ([entry, multiple]) => {
    if (multiple || (renameTargetPath.value && renameTargetPath.value !== entry?.path)) {
      renameTargetPath.value = null;
      renameValue.value = "";
    }
  });

  async function handleCreateFile() {
    if (options.isTrashPanel.value) return;
    if (!createFileName.value.trim()) return;
    const snapshot = await options.createFileInWorkspace(createFileName.value.trim());
    if (snapshot) {
      createFileName.value = "";
    }
  }

  function startRenameEntry(entry: FileBrowserEntry) {
    renameTargetPath.value = entry.path;
    renameValue.value = entry.name;
  }

  function startRenameSelected() {
    if (!options.currentFileEntry.value) return;
    startRenameEntry(options.currentFileEntry.value);
  }

  async function submitRenameSelected() {
    if (!renameTargetPath.value || !renameValue.value.trim()) return;
    const snapshot = await options.renameWorkspaceEntry(renameTargetPath.value, renameValue.value.trim());
    if (snapshot) {
      renameTargetPath.value = null;
      renameValue.value = "";
    }
  }

  async function deleteSelectedEntry() {
    if (!options.selectedFilePaths.value.length) return;
    if (options.selectedFilePaths.value.length > 1) {
      await options.deleteWorkspaceEntries(options.selectedFilePaths.value, options.isTrashPanel.value ? "permanentDelete" : undefined);
      return;
    }
    if (!options.currentFileEntry.value) return;
    await options.deleteWorkspaceEntry(options.currentFileEntry.value.path, options.isTrashPanel.value ? "permanentDelete" : undefined);
  }

  async function deleteContextSelection(entry: FileBrowserEntry, contextSelectionPaths: string[]) {
    if (contextSelectionPaths.length > 1) {
      await options.deleteWorkspaceEntries(contextSelectionPaths, options.isTrashPanel.value ? "permanentDelete" : undefined);
      return;
    }
    await options.deleteWorkspaceEntry(entry.path, options.isTrashPanel.value ? "permanentDelete" : undefined);
  }

  function openCopyTargetDialog(entry: FileBrowserEntry) {
    if (options.isTrashPanel.value) return;
    pendingCopySourcePaths.value = options.selectedFilePathSet.value.has(entry.path)
      ? [...options.selectedFilePaths.value]
      : [entry.path];
    copyTargetPath.value = options.fileBrowser.value?.currentPath ?? "";
    copyTargetDialogOpen.value = true;
  }

  async function submitCopyTarget() {
    const paths = pendingCopySourcePaths.value;
    if (!paths.length) return;
    const targetPath = normalizeRepositoryRelativePath(copyTargetPath.value);
    const snapshot = await options.copyWorkspaceEntries(paths, targetPath);
    if (snapshot) {
      cancelCopyTarget();
    }
  }

  function cancelCopyTarget() {
    copyTargetDialogOpen.value = false;
    pendingCopySourcePaths.value = [];
    copyTargetPath.value = "";
  }

  async function confirmCurrentHardlinkCandidate() {
    const candidate = currentHardlinkCandidate.value;
    if (!candidate) return;
    const response = await options.confirmWorkspaceHardlinkCandidate(candidate.candidateId);
    if (response) {
      skippedHardlinkCandidateIds.value.delete(candidate.candidateId);
    }
  }

  function skipCurrentHardlinkCandidate() {
    const candidate = currentHardlinkCandidate.value;
    if (!candidate) return;
    skippedHardlinkCandidateIds.value = new Set([
      ...skippedHardlinkCandidateIds.value,
      candidate.candidateId,
    ]);
  }

  async function restoreSelectedEntry() {
    if (!options.selectedFilePaths.value.length || !options.isTrashPanel.value) return;
    if (options.selectedFilePaths.value.length > 1) {
      await options.restoreTrashEntries(options.selectedFilePaths.value);
      return;
    }
    if (!options.currentFileEntry.value) return;
    await options.restoreTrashEntry(options.currentFileEntry.value.path);
  }

  async function restoreContextSelection(entry: FileBrowserEntry, contextSelectionPaths: string[]) {
    if (!options.isTrashPanel.value) return;
    if (contextSelectionPaths.length > 1) {
      await options.restoreTrashEntries(contextSelectionPaths);
      return;
    }
    await options.restoreTrashEntry(entry.path);
  }

  async function handleRestoreAllTrash() {
    if (!options.isTrashPanel.value) return;
    await options.restoreAllTrashEntries();
  }

  async function handleEmptyTrash() {
    if (!options.isTrashPanel.value) return;
    await options.emptyTrash();
  }

  async function openSelectedEntry() {
    if (options.isTrashPanel.value) return;
    if (!options.currentFileEntry.value) return;
    if (options.currentFileEntry.value.kind === "directory") {
      options.openDirectory(options.currentFileEntry.value.path);
      return;
    }
    await options.openWorkspaceEntry(options.currentFileEntry.value.path);
  }

  async function revealSelectedEntry() {
    if (options.isTrashPanel.value) return;
    if (!options.currentFileEntry.value) return;
    await options.revealWorkspaceEntry(options.currentFileEntry.value.path);
  }

  async function handleImportFolder() {
    if (options.isTrashPanel.value) return;
    const sourcePath = await openDirectoryDialog("选择导入文件夹");
    if (!sourcePath) return;
    await options.importEntriesToWorkspace([sourcePath]);
  }

  async function handleImportZip() {
    if (options.isTrashPanel.value) return;
    const archivePath = await openZipDialog("选择 ZIP 压缩包");
    if (!archivePath) return;
    await options.importArchiveEntriesToWorkspace(archivePath);
  }

  async function handleImportEagleCopy() {
    await handleImportEagle("copy");
  }

  async function handleImportEagleMove() {
    await handleImportEagle("move");
  }

  async function handleImportEagle(mode: "copy" | "move") {
    if (options.isTrashPanel.value) return;
    const libraryPath = await openDirectoryDialog("选择 EagleLibrary 目录");
    if (!libraryPath) return;
    await options.importEagleLibraryToWorkspace(libraryPath, mode);
  }

  return {
    copyTargetDialogOpen,
    copyTargetPath,
    createFileName,
    currentHardlinkCandidate,
    renameTargetPath,
    renameValue,
    cancelCopyTarget,
    confirmCurrentHardlinkCandidate,
    deleteContextSelection,
    deleteSelectedEntry,
    handleCreateFile,
    handleEmptyTrash,
    handleImportEagleCopy,
    handleImportEagleMove,
    handleImportFolder,
    handleImportZip,
    handleRestoreAllTrash,
    openCopyTargetDialog,
    openSelectedEntry,
    restoreContextSelection,
    restoreSelectedEntry,
    revealSelectedEntry,
    skipCurrentHardlinkCandidate,
    startRenameEntry,
    startRenameSelected,
    submitCopyTarget,
    submitRenameSelected,
  };
}

async function openDirectoryDialog(title: string) {
  const { open } = await import("@tauri-apps/plugin-dialog");
  const selected = await open({
    title,
    directory: true,
    multiple: false,
  });
  return typeof selected === "string" && selected.trim() ? selected : null;
}

async function openZipDialog(title: string) {
  const { open } = await import("@tauri-apps/plugin-dialog");
  const selected = await open({
    title,
    directory: false,
    multiple: false,
    filters: [{ name: "ZIP", extensions: ["zip"] }],
  });
  return typeof selected === "string" && selected.trim() ? selected : null;
}
