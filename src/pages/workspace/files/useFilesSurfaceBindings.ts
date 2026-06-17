import { computed, type Component, type Ref } from "vue";
import type { ContextMenuItem } from "../../../composables/useContextMenu";
import type { RegisteredLibraryExtension } from "../../../plugins/sdk";
import type { FileBrowserEntry, RepositoryTagGroup, SmartFolderResultSnapshot } from "../../../types/repository";
import type { FileDisplayMode } from "../useWorkspaceViewState";
import type { WorkspacePlayerBarHandlers, WorkspacePlayerBarProps } from "../playlists/usePlayerUi";

type BreadcrumbSegment = {
  label: string;
  path: string;
};
type SelectionMode = "replace" | "toggle" | "range";
type BoxSelectionMode = "replace" | "append";

type ReadonlyRef<T> = Pick<Ref<T>, "value">;

export type WorkspaceFilesSurfaceBindingOptions = {
  activeFileEntries: ReadonlyRef<FileBrowserEntry[]>;
  activeRepoId: ReadonlyRef<string | null>;
  activeSnapshotTagGroups: ReadonlyRef<RepositoryTagGroup[]>;
  activeDirectoryEntries: ReadonlyRef<FileBrowserEntry[]>;
  breadcrumbSegments: ReadonlyRef<BreadcrumbSegment[]>;
  canDeleteSelected: ReadonlyRef<boolean>;
  canDragEntries: ReadonlyRef<boolean>;
  canOpenSelected: ReadonlyRef<boolean>;
  canRenameSelected: ReadonlyRef<boolean>;
  canRestoreSelected: ReadonlyRef<boolean>;
  currentFileEntry: ReadonlyRef<FileBrowserEntry | null>;
  currentLibraryExtensions: ReadonlyRef<RegisteredLibraryExtension[]>;
  dragHoverFolderPath: ReadonlyRef<string | null>;
  error: ReadonlyRef<string | null>;
  fileDisplayModeClass: ReadonlyRef<string>;
  fileDisplayModeOptions: Array<{ value: FileDisplayMode; label: string }>;
  fileEntryContextMenu: (entry: FileBrowserEntry) => ContextMenuItem[];
  fileItemStyle: (entry: FileBrowserEntry) => Record<string, string>;
  fileTone: (entry: FileBrowserEntry) => string;
  hardlinkStateLabel: (entry: FileBrowserEntry) => string;
  hasActiveSplitFileGroups: ReadonlyRef<boolean>;
  isAudioEntry: (entry: FileBrowserEntry) => boolean;
  isDragActive: ReadonlyRef<boolean>;
  isDraggingFiles: ReadonlyRef<boolean>;
  isActiveBrowserLoading: ReadonlyRef<boolean>;
  isModelEntry: (entry: FileBrowserEntry) => boolean;
  isMutatingFiles: ReadonlyRef<boolean>;
  isSmartFolderPanel: ReadonlyRef<boolean>;
  isSavingMetadata: ReadonlyRef<boolean>;
  isTrashPanel: ReadonlyRef<boolean>;
  isVideoEntry: (entry: FileBrowserEntry) => boolean;
  openSelectedLabel: ReadonlyRef<string>;
  previewFileEntry: ReadonlyRef<FileBrowserEntry | null>;
  previewLibraryExtensions: ReadonlyRef<RegisteredLibraryExtension[]>;
  previewPlugin: ReadonlyRef<{ component: Component } | null>;
  renameTargetPath: ReadonlyRef<string | null>;
  saveCoverThumbnail: (path: string, sourceUrl: string) => Promise<unknown>;
  saveMetadata: (entry: FileBrowserEntry, metadata: Record<string, unknown>) => Promise<unknown>;
  selectedEntries: ReadonlyRef<FileBrowserEntry[]>;
  selectedFilePath: ReadonlyRef<string | null>;
  selectedFilePaths: ReadonlyRef<string[]>;
  showWorkspacePlayer: ReadonlyRef<boolean>;
  smartFolderResult: ReadonlyRef<SmartFolderResultSnapshot | null>;
  smartFolderSummary: ReadonlyRef<string>;
  statusLabel: (status: string) => string;
  tagFilterOptions: ReadonlyRef<string[]>;
  thumbnailPalette: (entry: FileBrowserEntry) => string[];
  thumbnailSrc: (entry: FileBrowserEntry) => string | null;
  workspacePlayerBarHandlers: WorkspacePlayerBarHandlers;
  workspacePlayerBarProps: ReadonlyRef<WorkspacePlayerBarProps>;
  entryDeletedAtLabel: (entry: FileBrowserEntry) => string | null;
  entryModifiedAtLabel: (entry: FileBrowserEntry) => string;
  exitPreview: () => void;
  handleCreateFile: () => void | Promise<unknown>;
  deleteSelectedEntry: () => void | Promise<unknown>;
  handleDragLeave: (event: DragEvent) => void;
  handleDragOver: (event: DragEvent) => void;
  handleDrop: (event: DragEvent) => void | Promise<unknown>;
  handleFolderDrop: (path: string, event: DragEvent) => void | Promise<unknown>;
  handleEmptyTrash: () => void | Promise<unknown>;
  handleEntryDragEnd: (event: PointerEvent | null) => void;
  handleEntryDragMove: (event: PointerEvent) => void;
  handleEntryDragStart: (entry: FileBrowserEntry, event: PointerEvent) => void;
  handleFolderDropHover: (path: string) => void;
  handleFolderDropLeave: (path: string) => void;
  markThumbnailFailed: (entry: FileBrowserEntry) => void;
  openDirectory: (path: string) => void | Promise<unknown>;
  openWorkspaceEntry: (path: string) => void | Promise<unknown>;
  openSelectedEntry: () => void | Promise<unknown>;
  previewFileEntryByDoubleClick: (entry: FileBrowserEntry) => void;
  handleRestoreAllTrash: () => void | Promise<unknown>;
  restoreSelectedEntry: () => void | Promise<unknown>;
  revealWorkspaceEntry: (path: string) => void | Promise<unknown>;
  revealSelectedEntry: () => void | Promise<unknown>;
  handleBoxSelection: (paths: string[], mode: BoxSelectionMode) => void;
  selectFileEntry: (entry: FileBrowserEntry, mode: SelectionMode) => void;
  startRenameSelected: () => void;
  submitRenameSelected: () => void | Promise<unknown>;
  updateThumbnailAspectRatio: (entry: FileBrowserEntry, event: Event) => void;
};

export function useFilesSurfaceBindings(options: WorkspaceFilesSurfaceBindingOptions) {
  const filesSurfaceProps = computed(() => ({
    activeFileEntries: options.activeFileEntries.value,
    activeRepoId: options.activeRepoId.value,
    availableTags: options.tagFilterOptions.value,
    breadcrumbs: options.breadcrumbSegments.value,
    canDeleteSelected: options.canDeleteSelected.value,
    canDragEntries: options.canDragEntries.value,
    canOpenSelected: options.canOpenSelected.value,
    canRenameSelected: options.canRenameSelected.value,
    canRestoreSelected: options.canRestoreSelected.value,
    currentFileEntry: options.currentFileEntry.value,
    currentLibraryExtensions: options.currentLibraryExtensions.value,
    directoryEntries: options.activeDirectoryEntries.value,
    displayModeClass: options.fileDisplayModeClass.value,
    displayModeOptions: options.fileDisplayModeOptions,
    dropTargetPath: options.dragHoverFolderPath.value,
    entryDeletedAtLabel: options.entryDeletedAtLabel,
    entryModifiedAtLabel: options.entryModifiedAtLabel,
    error: options.error.value,
    fileEntryContextMenu: options.fileEntryContextMenu,
    fileItemStyle: options.fileItemStyle,
    fileTone: options.fileTone,
    hardlinkStateLabel: options.hardlinkStateLabel,
    hasSplitFileGroups: options.hasActiveSplitFileGroups.value,
    isAudioEntry: options.isAudioEntry,
    isDragActive: options.isDragActive.value,
    isDraggingFiles: options.isDraggingFiles.value,
    isLoadingFileBrowser: options.isActiveBrowserLoading.value,
    isModelEntry: options.isModelEntry,
    isMutatingFiles: options.isMutatingFiles.value,
    isReadOnlyVirtual: options.isSmartFolderPanel.value,
    isSavingMetadata: options.isSavingMetadata.value,
    isTrashPanel: options.isTrashPanel.value,
    isVideoEntry: options.isVideoEntry,
    libraryExtensions: options.currentLibraryExtensions.value,
    openSelectedLabel: options.openSelectedLabel.value,
    previewFileEntry: options.previewFileEntry.value,
    previewLibraryExtensions: options.previewLibraryExtensions.value,
    previewPlugin: options.previewPlugin.value,
    renameTargetPath: options.renameTargetPath.value,
    saveCoverThumbnail: options.saveCoverThumbnail,
    saveMetadata: options.saveMetadata,
    selectedEntries: options.selectedEntries.value,
    selectedFilePath: options.selectedFilePath.value,
    selectedFilePaths: options.selectedFilePaths.value,
    showWorkspacePlayer: options.showWorkspacePlayer.value,
    statusLabel: options.statusLabel,
    tagGroups: options.activeSnapshotTagGroups.value,
    thumbnailPalette: options.thumbnailPalette,
    thumbnailSrc: options.thumbnailSrc,
    virtualSubline: options.smartFolderSummary.value,
    virtualTitle: options.smartFolderResult.value?.smartFolder.name,
    workspacePlayerBarHandlers: options.workspacePlayerBarHandlers,
    workspacePlayerBarProps: options.workspacePlayerBarProps.value,
  }));

  const filesSurfaceHandlers = {
    back: options.exitPreview,
    createFile: options.handleCreateFile,
    deleteSelected: options.deleteSelectedEntry,
    dragLeave: options.handleDragLeave,
    dragOver: options.handleDragOver,
    drop: options.handleDrop,
    dropOnFolder: options.handleFolderDrop,
    emptyTrash: options.handleEmptyTrash,
    entryDragEnd: options.handleEntryDragEnd,
    entryDragMove: options.handleEntryDragMove,
    entryDragStart: options.handleEntryDragStart,
    hoverFolder: options.handleFolderDropHover,
    leaveFolder: options.handleFolderDropLeave,
    markThumbnailFailed: options.markThumbnailFailed,
    openDirectory: options.openDirectory,
    openEntry: options.openWorkspaceEntry,
    openSelected: options.openSelectedEntry,
    previewFile: options.previewFileEntryByDoubleClick,
    restoreAllTrash: options.handleRestoreAllTrash,
    restoreSelected: options.restoreSelectedEntry,
    revealEntry: options.revealWorkspaceEntry,
    revealSelected: options.revealSelectedEntry,
    selectEntries: options.handleBoxSelection,
    selectEntry: options.selectFileEntry,
    startRename: options.startRenameSelected,
    submitRename: options.submitRenameSelected,
    thumbnailError: options.markThumbnailFailed,
    thumbnailLoaded: options.updateThumbnailAspectRatio,
  };

  return {
    filesSurfaceHandlers,
    filesSurfaceProps,
  };
}
