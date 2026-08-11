import type {
  AssetDetail,
  RepositoryDeleteRequest,
  NeteaseRepositoryCacheConfigureRequest,
  NeteaseRepositoryCacheConfigureResponse,
  SourceRepositoryCacheConfigureRequest,
  SourceRepositoryCacheConfigureResponse,
  RepositoryBackendConfigUpdateRequest,
  RepositoryExportRequest,
  RepositoryExportResponse,
  RepositoryFolderRequest,
  RepositoryMutationRequest,
  RepositoryMutationResponse,
  RepositoryRelocateRequest,
  RepositorySnapshot,
  RepositorySummary,
  SyncRequest,
  SyncResult,
} from "../../types/repository";
import { invokeCommand } from "./core";

export function listRepositories() {
  return invokeCommand<RepositorySummary[]>("list_repositories");
}

export function getRepositorySnapshot(repoId: string) {
  return invokeCommand<RepositorySnapshot>("get_repository_snapshot", { repoId });
}

export function getAssetDetail(repoId: string, assetId: string) {
  return invokeCommand<AssetDetail>("get_asset_detail", { repoId, assetId });
}

export function createRepository(request: RepositoryMutationRequest) {
  return invokeCommand<RepositoryMutationResponse>("create_repository", { request });
}

export function importRepository(request: RepositoryMutationRequest) {
  return invokeCommand<RepositoryMutationResponse>("import_repository", { request });
}

export function attachRepositoryFolder(request: RepositoryFolderRequest) {
  return invokeCommand<RepositoryMutationResponse>("attach_repository_folder", { request });
}

export function deleteRepository(request: RepositoryDeleteRequest) {
  return invokeCommand<void>("delete_repository", { request });
}

export function relocateRepository(request: RepositoryRelocateRequest) {
  return invokeCommand<RepositoryMutationResponse>("relocate_repository", { request });
}

export function updateRepositoryBackendConfig(request: RepositoryBackendConfigUpdateRequest) {
  return invokeCommand<RepositoryMutationResponse>("update_repository_backend_config", { request });
}

export function configureNeteaseRepositoryCache(request: NeteaseRepositoryCacheConfigureRequest) {
  return invokeCommand<NeteaseRepositoryCacheConfigureResponse>(
    "configure_netease_repository_cache",
    { request },
  );
}

/** 配置需要本地缓存的 Source 仓库；当前宿主命令同时负责迁移旧网易云缓存。 */
export function configureSourceRepositoryCache(request: SourceRepositoryCacheConfigureRequest) {
  return invokeCommand<SourceRepositoryCacheConfigureResponse>(
    "configure_netease_repository_cache",
    { request },
  );
}

export function exportRepository(request: RepositoryExportRequest) {
  return invokeCommand<RepositoryExportResponse>("export_repository", { request });
}

export function syncRepository(request: SyncRequest) {
  return invokeCommand<SyncResult>("sync_repository", { request });
}
