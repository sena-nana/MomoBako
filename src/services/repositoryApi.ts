import { Channel, invoke } from "@tauri-apps/api/core";
import { openPath, revealItemInDir } from "@tauri-apps/plugin-opener";
import type {
  ApiDesignSnapshot,
  AssetDetail,
  CacheSnapshot,
  BinaryFileWriteRequest,
  BinaryFileWriteResponse,
  FileBrowserRequest,
  FileBrowserSnapshot,
  FileCopyRequest,
  FileCreateRequest,
  FileImportRequest,
  FileDeleteRequest,
  FileMoveRequest,
  FilePreviewSourceResponse,
  FileReadRequest,
  FileRenameRequest,
  HardlinkCandidateResponse,
  HardlinkConfirmRequest,
  HardlinkConfirmResponse,
  MetadataUpdateRequest,
  MetadataUpdateResponse,
  PluginCallRequest,
  PluginCallResponse,
  PluginArchiveReadRequest,
  PluginArchiveTextResponse,
  PluginEnabledRequest,
  PluginInstallRequest,
  RepositoryExportRequest,
  RepositoryExportResponse,
  RepositoryFolderRequest,
  RepositoryRelocateRequest,
  PluginManifest,
  PluginMutationResponse,
  RepositoryMutationRequest,
  RepositoryMutationResponse,
  RepositorySnapshot,
  RepositorySummary,
  RevisionActionRequest,
  RevisionActionResponse,
  SearchRequest,
  SearchResponse,
  SmartFolderMutationRequest,
  SmartFolderMutationResponse,
  SmartFolderResultSnapshot,
  SmartFolderTreeNode,
  SmartFolderUpdateRequest,
  SyncRequest,
  SyncResult,
  ThumbnailRequest,
  ThumbnailResponse,
  TrashMutationRequest,
} from "../types/repository";

type ExternalFileDragEvent = {
  result: "Dropped" | "Cancel";
  cursorPos: {
    x: number;
    y: number;
  };
};

const fallbackFileDragIcon = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAEAAAABACAYAAACqaXHeAAAA1klEQVR4nO2aMQ6DMAxFf6n//7JzSbqVKAWyJE6RMXBpZRmCcL7ugPfr7YkNQKkAqQBYAGABBkKr+S2q681+RjvfbWAA4FO9VxOAQ3VFcvLaKQwwqgbgtI2kQNdEAZpWAJalYBWIVgD62AhgGwhqgVAFYqCx+WK4QClyHb1ZAMKoHDCu5TgBsK4IuM0EnJbx0B5oEGh96F/Nh78qfm83pkQ+ZpA6lAyoo9CRPz39QLLm9YkA8C1yNEioOl4H8NZuTkAmFK5e4Z4A1UkaIBUAC4AFwALg3AE5mFG5Q1UzmgAAAABJRU5ErkJggg==";

export function listRepositories() {
  return invoke<RepositorySummary[]>("list_repositories");
}

export function getRepositorySnapshot(repoId: string) {
  return invoke<RepositorySnapshot>("get_repository_snapshot", { repoId });
}

export function getAssetDetail(repoId: string, assetId: string) {
  return invoke<AssetDetail>("get_asset_detail", { repoId, assetId });
}

export function searchAssets(request: SearchRequest) {
  return invoke<SearchResponse>("search_assets", { request });
}

export function updateAssetMetadata(request: MetadataUpdateRequest) {
  return invoke<MetadataUpdateResponse>("update_asset_metadata", { request });
}

export function getFileBrowser(request: FileBrowserRequest) {
  return invoke<FileBrowserSnapshot>("get_file_browser", { request });
}

export function listSmartFolders(repoId: string) {
  return invoke<SmartFolderTreeNode[]>("list_smart_folders", { repoId });
}

export function createSmartFolder(request: SmartFolderMutationRequest) {
  return invoke<SmartFolderMutationResponse>("create_smart_folder", { request });
}

export function updateSmartFolder(request: SmartFolderUpdateRequest) {
  return invoke<SmartFolderMutationResponse>("update_smart_folder", { request });
}

export function deleteSmartFolder(repoId: string, smartFolderId: string) {
  return invoke<SmartFolderMutationResponse>("delete_smart_folder", { repoId, smartFolderId });
}

export function querySmartFolder(repoId: string, smartFolderId: string) {
  return invoke<SmartFolderResultSnapshot>("query_smart_folder", { repoId, smartFolderId });
}

export function readFile(request: FileReadRequest) {
  return invoke<number[]>("read_file", { request });
}

export function preparePreviewFileSource(request: FileReadRequest) {
  return invoke<FilePreviewSourceResponse>("prepare_preview_file_source", { request });
}

export function callPlugin<T = unknown>(request: PluginCallRequest) {
  return invoke<PluginCallResponse<T>>("call_plugin", { request });
}

export function readPluginArchiveText(request: PluginArchiveReadRequest) {
  return invoke<PluginArchiveTextResponse>("read_plugin_archive_text", { request });
}

export function writeBinaryFile(request: BinaryFileWriteRequest) {
  return invoke<BinaryFileWriteResponse>("write_binary_file", { request });
}

export function createDirectory(request: FileCreateRequest) {
  return invoke<FileBrowserSnapshot>("create_directory", { request });
}

export function createFile(request: FileCreateRequest) {
  return invoke<FileBrowserSnapshot>("create_file", { request });
}

export function importEntries(request: FileImportRequest) {
  return invoke<FileBrowserSnapshot>("import_entries", { request });
}

export function copyEntries(request: FileCopyRequest) {
  return invoke<FileBrowserSnapshot>("copy_entries", { request });
}

export function moveEntries(request: FileMoveRequest) {
  return invoke<FileBrowserSnapshot>("move_entries", { request });
}

export function renameEntry(request: FileRenameRequest) {
  return invoke<FileBrowserSnapshot>("rename_entry", { request });
}

export function deleteEntry(request: FileDeleteRequest) {
  return invoke<FileBrowserSnapshot>("delete_entry", { request });
}

export function mutateTrash(request: TrashMutationRequest) {
  return invoke<FileBrowserSnapshot>("mutate_trash", { request });
}

export function createRepository(request: RepositoryMutationRequest) {
  return invoke<RepositoryMutationResponse>("create_repository", { request });
}

export function importRepository(request: RepositoryMutationRequest) {
  return invoke<RepositoryMutationResponse>("import_repository", { request });
}

export function attachRepositoryFolder(request: RepositoryFolderRequest) {
  return invoke<RepositoryMutationResponse>("attach_repository_folder", { request });
}

export function deleteRepository(repoId: string) {
  return invoke<void>("delete_repository", { repoId });
}

export function relocateRepository(request: RepositoryRelocateRequest) {
  return invoke<RepositoryMutationResponse>("relocate_repository", { request });
}

export function exportRepository(request: RepositoryExportRequest) {
  return invoke<RepositoryExportResponse>("export_repository", { request });
}

export function syncRepository(request: SyncRequest) {
  return invoke<SyncResult>("sync_repository", { request });
}

export function listHardlinkCandidates(repoId: string) {
  return invoke<HardlinkCandidateResponse>("list_hardlink_candidates", { repoId });
}

export function confirmHardlinkCandidate(request: HardlinkConfirmRequest) {
  return invoke<HardlinkConfirmResponse>("confirm_hardlink_candidate", { request });
}

export function ensureThumbnail(request: ThumbnailRequest) {
  return invoke<ThumbnailResponse>("ensure_thumbnail", { request });
}

export function undoLastRevision(request: RevisionActionRequest) {
  return invoke<RevisionActionResponse>("undo_last_revision", { request });
}

export function redoLastRevision(request: RevisionActionRequest) {
  return invoke<RevisionActionResponse>("redo_last_revision", { request });
}

export function listPlugins() {
  return invoke<PluginManifest[]>("list_plugins");
}

export function setPluginEnabled(request: PluginEnabledRequest) {
  return invoke<PluginMutationResponse>("set_plugin_enabled", { request });
}

export function deletePlugin(pluginId: string) {
  return invoke<PluginMutationResponse>("delete_plugin", { pluginId });
}

export function installPluginFromArchive(request: PluginInstallRequest) {
  return invoke<PluginMutationResponse>("install_plugin_from_archive", { request });
}

export function getCacheSnapshot() {
  return invoke<CacheSnapshot>("get_cache_snapshot");
}

export function getApiDesignSnapshot() {
  return invoke<ApiDesignSnapshot>("get_api_design_snapshot");
}

export function startExternalFileDrag(paths: string[], icon = fallbackFileDragIcon) {
  const onEvent = new Channel<ExternalFileDragEvent>();
  return invoke<void>("plugin:drag|start_drag", {
    item: paths,
    image: icon,
    options: { mode: "copy" },
    onEvent,
  });
}

export function openRepositoryPath(path: string) {
  return openPath(path);
}

export function revealRepositoryPath(path: string) {
  return revealItemInDir(path);
}
