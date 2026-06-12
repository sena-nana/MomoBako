import {
  createSmartFolder,
  deleteSmartFolder,
  listSmartFolders,
  querySmartFolder,
  updateSmartFolder,
} from "../../services/repositoryApi";
import type {
  SmartFolderFilter,
  SmartFolderMutationRequest,
  SmartFolderTreeNode,
  SmartFolderUpdateRequest,
} from "../../types/repository";
import {
  activePanel,
  activeRepoId,
  activeSmartFolderId,
  error,
  isLoadingSmartFolder,
  isMutatingSmartFolder,
  selectedFilePath,
  smartFolderResult,
  smartFolders,
} from "./state";

function smartFolderTreeContains(items: SmartFolderTreeNode[], smartFolderId: string | null): boolean {
  if (!smartFolderId) return false;
  return items.some((item) => (
    item.smartFolderId === smartFolderId || smartFolderTreeContains(item.children, smartFolderId)
  ));
}

function normalizeSmartFolderFilter(filter: SmartFolderFilter): SmartFolderFilter {
  const normalizeList = (items?: string[]) => {
    const values = Array.from(new Set((items ?? []).map((item) => item.trim()).filter(Boolean)));
    return values.length ? values : undefined;
  };
  const normalizeMetadataFilters = (items = filter.metadataFilters) => items
    ?.map((item) => ({ key: item.key.trim(), value: item.value.trim() }))
    .filter((item) => item.key && item.value);
  const numberFilters = filter.numberFilters
    ?.map((item) => ({ key: item.key.trim(), min: item.min, max: item.max }))
    .filter((item) => item.key && (item.min != null || item.max != null));
  const excludeNumberFilters = filter.excludeNumberFilters
    ?.map((item) => ({ key: item.key.trim(), min: item.min, max: item.max }))
    .filter((item) => item.key && (item.min != null || item.max != null));
  const dateFilters = filter.dateFilters
    ?.map((item) => ({ key: item.key.trim(), from: item.from?.trim() || undefined, to: item.to?.trim() || undefined }))
    .filter((item) => item.key && (item.from || item.to));
  const excludeDateFilters = filter.excludeDateFilters
    ?.map((item) => ({ key: item.key.trim(), from: item.from?.trim() || undefined, to: item.to?.trim() || undefined }))
    .filter((item) => item.key && (item.from || item.to));
  const sortField = filter.sort?.field.trim();
  return {
    query: filter.query?.trim() || undefined,
    pathPrefix: filter.pathPrefix?.trim() || undefined,
    excludeQuery: filter.excludeQuery?.trim() || undefined,
    excludePathPrefixes: normalizeList(filter.excludePathPrefixes),
    tags: normalizeList(filter.tags),
    formats: normalizeList(filter.formats),
    colors: normalizeList(filter.colors),
    shapes: normalizeList(filter.shapes),
    metadataFilters: normalizeMetadataFilters()?.length ? normalizeMetadataFilters() : undefined,
    excludeTags: normalizeList(filter.excludeTags),
    excludeFormats: normalizeList(filter.excludeFormats),
    excludeMetadataFilters: normalizeMetadataFilters(filter.excludeMetadataFilters)?.length
      ? normalizeMetadataFilters(filter.excludeMetadataFilters)
      : undefined,
    excludeNumberFilters: excludeNumberFilters?.length ? excludeNumberFilters : undefined,
    excludeDateFilters: excludeDateFilters?.length ? excludeDateFilters : undefined,
    numberFilters: numberFilters?.length ? numberFilters : undefined,
    dateFilters: dateFilters?.length ? dateFilters : undefined,
    minRating: filter.minRating && filter.minRating > 0 ? filter.minRating : undefined,
    matchMode: filter.matchMode === "or" ? "or" : undefined,
    sort: sortField ? { field: sortField, direction: filter.sort?.direction === "desc" ? "desc" : "asc" } : undefined,
    limit: filter.limit && filter.limit > 0 ? filter.limit : undefined,
  };
}

export async function refreshSmartFolders(repoId = activeRepoId.value) {
  if (!repoId) {
    smartFolders.value = [];
    return [];
  }
  const items = await listSmartFolders(repoId);
  if (activeRepoId.value === repoId) {
    smartFolders.value = items;
  }
  return items;
}

export async function selectSmartFolder(smartFolderId: string) {
  const repoId = activeRepoId.value;
  if (!repoId || !smartFolderId) return null;
  activePanel.value = "smartFolder";
  activeSmartFolderId.value = smartFolderId;
  isLoadingSmartFolder.value = true;
  error.value = null;
  try {
    const snapshot = await querySmartFolder(repoId, smartFolderId);
    if (activeRepoId.value !== repoId || activeSmartFolderId.value !== smartFolderId) {
      return snapshot;
    }
    smartFolderResult.value = snapshot;
    selectedFilePath.value = snapshot.results[0]?.path ?? null;
    return snapshot;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    return null;
  } finally {
    isLoadingSmartFolder.value = false;
  }
}

export async function createSmartFolderInWorkspace(request: Omit<SmartFolderMutationRequest, "repoId">) {
  const repoId = activeRepoId.value;
  if (!repoId) return null;
  isMutatingSmartFolder.value = true;
  error.value = null;
  try {
    const response = await createSmartFolder({
      ...request,
      repoId,
      parentId: request.parentId || undefined,
      filter: normalizeSmartFolderFilter(request.filter),
    });
    smartFolders.value = response.smartFolders;
    if (response.smartFolder) {
      await selectSmartFolder(response.smartFolder.smartFolderId);
    }
    return response;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    return null;
  } finally {
    isMutatingSmartFolder.value = false;
  }
}

export async function updateSmartFolderInWorkspace(request: Omit<SmartFolderUpdateRequest, "repoId">) {
  const repoId = activeRepoId.value;
  if (!repoId) return null;
  isMutatingSmartFolder.value = true;
  error.value = null;
  try {
    const response = await updateSmartFolder({
      ...request,
      repoId,
      parentId: request.parentId || undefined,
      filter: normalizeSmartFolderFilter(request.filter),
    });
    smartFolders.value = response.smartFolders;
    if (activeSmartFolderId.value === request.smartFolderId) {
      await selectSmartFolder(request.smartFolderId);
    }
    return response;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    return null;
  } finally {
    isMutatingSmartFolder.value = false;
  }
}

export async function deleteSmartFolderInWorkspace(smartFolderId: string) {
  const repoId = activeRepoId.value;
  if (!repoId) return null;
  isMutatingSmartFolder.value = true;
  error.value = null;
  try {
    const response = await deleteSmartFolder(repoId, smartFolderId);
    smartFolders.value = response.smartFolders;
    if (!smartFolderTreeContains(response.smartFolders, activeSmartFolderId.value)) {
      activeSmartFolderId.value = null;
      smartFolderResult.value = null;
      activePanel.value = "files";
    }
    return response;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    return null;
  } finally {
    isMutatingSmartFolder.value = false;
  }
}
