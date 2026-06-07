import { invoke } from "@tauri-apps/api/core";
import { openPath, revealItemInDir } from "@tauri-apps/plugin-opener";
import type {
  ApiDesignSnapshot,
  AssetDetail,
  CacheSnapshot,
  FileBrowserRequest,
  FileBrowserSnapshot,
  FileCreateRequest,
  FileImportRequest,
  FileDeleteRequest,
  FileReadRequest,
  FileRenameRequest,
  MetadataUpdateRequest,
  MetadataUpdateResponse,
  RepositoryFolderRequest,
  PluginManifest,
  RepositoryMutationRequest,
  RepositoryMutationResponse,
  RepositorySnapshot,
  RepositorySummary,
  RevisionActionRequest,
  RevisionActionResponse,
  SearchRequest,
  SearchResponse,
  SyncRequest,
  SyncResult,
} from "../types/repository";

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

export function readFile(request: FileReadRequest) {
  return invoke<number[]>("read_file", { request });
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

export function renameEntry(request: FileRenameRequest) {
  return invoke<FileBrowserSnapshot>("rename_entry", { request });
}

export function deleteEntry(request: FileDeleteRequest) {
  return invoke<FileBrowserSnapshot>("delete_entry", { request });
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

export function exportRepository(repoId: string) {
  return invoke<RepositoryMutationResponse>("export_repository", { repoId });
}

export function syncRepository(request: SyncRequest) {
  return invoke<SyncResult>("sync_repository", { request });
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

export function getCacheSnapshot() {
  return invoke<CacheSnapshot>("get_cache_snapshot");
}

export function getApiDesignSnapshot() {
  return invoke<ApiDesignSnapshot>("get_api_design_snapshot");
}

export function openRepositoryPath(path: string) {
  return openPath(path);
}

export function revealRepositoryPath(path: string) {
  return revealItemInDir(path);
}
