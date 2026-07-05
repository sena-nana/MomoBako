import { configureRepositoryWorkspaceActions } from "./repositories";
import {
  loadRepositories,
  selectRepository,
} from "./navigation";

export type {
  WorkspaceFilterState,
  WorkspaceLibraryCategoryKey,
  WorkspacePanelKey,
} from "./state";
export type {
  WorkspaceOperationProgress,
} from "./tasks";

export { resetRepositoryWorkspaceForTests } from "./lifecycle";
export {
  ensureRepositoryWorkspace,
  loadRepositories,
  refreshActiveRepositoryWorkspaceSilently,
  refreshRepositoryWorkspace,
  selectAsset,
  selectRepository,
  setActiveLibraryCategory,
  setActivePanel,
  setActivePreviewPath,
} from "./navigation";
export {
  useRepositoryWorkspace,
  useWorkspaceAssetMetadata,
  useWorkspaceActions,
  useWorkspaceFiles,
  useWorkspaceNavigation,
  useWorkspacePlaylists,
  useWorkspaceProgress,
  useWorkspaceRepository,
  useWorkspaceSearch,
  useWorkspaceSelection,
  useWorkspaceSettings,
  useWorkspaceSmartFolders,
} from "./facade";
export {
  deletePluginConfigValueInWorkspace,
  deletePluginInWorkspace,
  installPluginArchiveInWorkspace,
  loadPluginConfigInWorkspace,
  loadSettingsData,
  openPluginDataDirectoryInWorkspace,
  setPluginConfigValueInWorkspace,
  setPluginEnabledInWorkspace,
} from "./settings";
export {
  attachRepository,
  closeRepositoryDeleteDialog,
  confirmRepositoryDelete,
  configureNeteaseRepositoryCacheInWorkspace,
  createNewRepository,
  exportCurrentRepository,
  importExistingRepository,
  openRepositoryDeleteDialog,
  relocateMissingRepository,
  removeRepository,
} from "./repositories";
export {
  createSmartFolderInWorkspace,
  deleteSmartFolderInWorkspace,
  refreshSmartFolders,
  selectSmartFolder,
  updateSmartFolderInWorkspace,
} from "./smartFolders";
export {
  refreshRepositoryActions,
  runActiveRepositoryAction,
  selectRepositoryAction,
} from "./repositoryActions";
export {
  redoAssetRevision,
  saveAssetMetadata,
  undoAssetRevision,
} from "./assetMetadata";
export {
  clearDraggedWorkspaceState,
  clearWorkspaceSelection,
  selectWorkspaceEntries,
  selectWorkspaceEntry,
  setDragHoverFolderPath,
  setDraggedWorkspacePaths,
  setExternalDragActive,
  setInternalDragActive,
} from "./selection";
export {
  copyWorkspaceEntries,
  createDirectoryInWorkspace,
  createFileInWorkspace,
  deleteWorkspaceEntries,
  deleteWorkspaceEntry,
  emptyTrash,
  importArchiveEntriesToWorkspace,
  importEagleLibraryToWorkspace,
  importEntriesToWorkspace,
  moveWorkspaceEntries,
  openWorkspaceEntry,
  renameWorkspaceEntry,
  restoreAllTrashEntries,
  restoreTrashEntries,
  restoreTrashEntry,
  revealWorkspaceEntry,
  startWorkspaceEntriesDrag,
  startWorkspaceEntryDrag,
} from "./fileOperations";
export {
  confirmWorkspaceHardlinkCandidate,
  refreshFileBrowserTree,
  syncActiveRepository,
} from "./sync";
export {
  addPlaylistItemsInWorkspace,
  createPlaylistInWorkspace,
  deletePlaylistInWorkspace,
  refreshPlaylists,
  removePlaylistItemInWorkspace,
  reorderPlaylistItemsInWorkspace,
  selectPlaylist,
  setPlaylistMembershipInWorkspace,
} from "./playlists";
export {
  clearWorkspaceEntryThumbnail,
  refreshWorkspaceEntryThumbnail,
  saveGeneratedWorkspaceEntryThumbnail,
  setWorkspaceEntryThumbnail,
  setWorkspaceEntryThumbnailFromBytes,
  setWorkspaceEntryThumbnailFromUrl,
} from "./thumbnails";
export {
  clearFilters,
  runFilteredSearch,
  runSearch,
  setFilterBarOpen,
  setMinimumRatingFilter,
  toggleFilterBar,
  toggleFilterValue,
  updateFilters,
} from "./search";
export {
  refreshHardlinkCandidates,
} from "./refresh";

configureRepositoryWorkspaceActions({
  loadRepositories,
  selectRepository,
});
