import {
  addPlaylistItems,
  addPlaylistItemsByPaths,
  createPlaylist,
  deletePlaylist,
  getPlaylistDetail,
  listPlaylistMemberships,
  listPlaylists,
  removePlaylistItem,
  reorderPlaylistItems,
  setPlaylistMembership,
} from "../../services/repositoryApi";
import type {
  PlaylistDetail,
  PlaylistMembershipSnapshot,
  PlaylistMutationRequest,
  PlaylistSummary,
} from "../../types/repository";
import { emitSystemLogSilently } from "../../services/systemLog";
import {
  activePanel,
  activePlaylistDetail,
  activePlaylistId,
  activeRepoId,
  playlistMemberships,
  playlists,
} from "./state";

const playlistDetailCache = new Map<string, PlaylistDetail>();

function playlistDetailCacheKey(repoId: string, playlistId: string) {
  return `${repoId}:${playlistId}`;
}

export function getCachedPlaylistDetail(repoId: string, playlistId: string) {
  return playlistDetailCache.get(playlistDetailCacheKey(repoId, playlistId)) ?? null;
}

export function cachePlaylistDetail(detail: PlaylistDetail) {
  playlistDetailCache.set(
    playlistDetailCacheKey(detail.playlist.repoId, detail.playlist.playlistId),
    detail,
  );
  return detail;
}

export function clearPlaylistDetailCache(repoId?: string | null) {
  if (!repoId) {
    playlistDetailCache.clear();
    return;
  }

  for (const key of playlistDetailCache.keys()) {
    if (key.startsWith(`${repoId}:`)) {
      playlistDetailCache.delete(key);
    }
  }
}

export async function primePlaylistDetailCache(repoId: string, playlistItems: PlaylistSummary[]) {
  if (!repoId || !playlistItems.length) return [];

  const details = await Promise.all(
    playlistItems.map((playlist) => getPlaylistDetail(repoId, playlist.playlistId)),
  );
  details.forEach(cachePlaylistDetail);
  return details;
}

export function rebuildPlaylistMemberships(details: PlaylistDetail[]) {
  const nextMemberships: Record<string, string[]> = {};

  for (const detail of details) {
    for (const item of detail.items) {
      if (!nextMemberships[item.assetId]) {
        nextMemberships[item.assetId] = [];
      }
      nextMemberships[item.assetId].push(detail.playlist.playlistId);
    }
  }

  playlistMemberships.value = nextMemberships;
}

export async function syncPlaylistMemberships(
  repoId: string,
  playlistItems: PlaylistSummary[] = playlists.value,
) {
  if (!repoId || !playlistItems.length) {
    playlistMemberships.value = {};
    if (activePlaylistId.value && !playlistItems.some((item) => item.playlistId === activePlaylistId.value)) {
      activePlaylistId.value = null;
      activePlaylistDetail.value = null;
    }
    return [];
  }

  try {
    const snapshot = await listPlaylistMemberships(repoId);
    playlistMemberships.value = snapshot.memberships;
  } catch {
    playlistMemberships.value = {};
  }

  if (activePlaylistId.value) {
    const activePlaylist = playlistItems.find((item) => item.playlistId === activePlaylistId.value);
    if (!activePlaylist) {
      activePlaylistDetail.value = null;
    } else if (activePlaylistDetail.value?.playlist.playlistId === activePlaylist.playlistId) {
      activePlaylistDetail.value = {
        ...activePlaylistDetail.value,
        playlist: activePlaylist,
      };
    }
  }

  return playlistMemberships.value;
}

export async function refreshPlaylists(repoId = activeRepoId.value) {
  if (!repoId) return [];
  const items = await listPlaylists(repoId);
  if (activeRepoId.value !== repoId) {
    return items;
  }
  playlists.value = items;
  if (!items.length) {
    clearPlaylistDetailCache(repoId);
  }
  await syncPlaylistMemberships(repoId, items);
  if (activePlaylistId.value && !items.some((item) => item.playlistId === activePlaylistId.value)) {
    activePlaylistId.value = null;
    activePlaylistDetail.value = null;
    if (activePanel.value === "playlist") {
      activePanel.value = "files";
    }
  }
  return items;
}

export async function selectPlaylist(playlistId: string) {
  if (!activeRepoId.value) return null;
  activePanel.value = "playlist";
  activePlaylistId.value = playlistId;
  const cachedDetail = getCachedPlaylistDetail(activeRepoId.value, playlistId);
  if (cachedDetail) {
    activePlaylistDetail.value = cachedDetail;
  }
  activePlaylistDetail.value = cachePlaylistDetail(
    await getPlaylistDetail(activeRepoId.value, playlistId),
  );
  return activePlaylistDetail.value;
}

export async function createPlaylistInWorkspace(request: Omit<PlaylistMutationRequest, "repoId">) {
  if (!activeRepoId.value) return null;
  emitSystemLogSilently("info", {
    category: "playlist",
    action: "createStart",
    message: "开始创建播放集。",
    repoId: activeRepoId.value,
    context: { name: request.name },
  });
  const response = await createPlaylist({ ...request, repoId: activeRepoId.value });
  playlists.value = response.playlists;
  await syncPlaylistMemberships(activeRepoId.value, response.playlists);
  if (response.playlist?.playlistId) {
    await selectPlaylist(response.playlist.playlistId);
  }
  emitSystemLogSilently("info", {
    category: "playlist",
    action: "createSuccess",
    message: "播放集创建完成。",
    repoId: activeRepoId.value,
    context: { playlistId: response.playlist?.playlistId ?? null, name: response.playlist?.name ?? request.name },
  });
  return response;
}

export async function deletePlaylistInWorkspace(playlistId: string) {
  if (!activeRepoId.value) return null;
  emitSystemLogSilently("warn", {
    category: "playlist",
    action: "deleteStart",
    message: "开始删除播放集。",
    repoId: activeRepoId.value,
    context: { playlistId },
  });
  const response = await deletePlaylist(activeRepoId.value, playlistId);
  playlists.value = response.playlists;
  await syncPlaylistMemberships(activeRepoId.value, response.playlists);
  if (activePlaylistId.value === playlistId) {
    activePlaylistId.value = null;
    activePlaylistDetail.value = null;
  if (activePanel.value === "playlist") activePanel.value = "files";
  }
  emitSystemLogSilently("warn", {
    category: "playlist",
    action: "deleteSuccess",
    message: "播放集删除完成。",
    repoId: activeRepoId.value,
    context: { playlistId },
  });
  return response;
}

export async function addPlaylistItemsInWorkspace(playlistId: string, assetIds: string[]) {
  if (!activeRepoId.value) return null;
  const detail = await addPlaylistItems({
    repoId: activeRepoId.value,
    playlistId,
    assetIds,
  });
  activePlaylistDetail.value = cachePlaylistDetail(detail);
  await refreshPlaylists(activeRepoId.value);
  return detail;
}

export async function addPlaylistItemsByPathsInWorkspace(playlistId: string, paths: string[]) {
  if (!activeRepoId.value || !paths.length) return null;
  const detail = await addPlaylistItemsByPaths({
    repoId: activeRepoId.value,
    playlistId,
    paths,
  });
  activePlaylistDetail.value = cachePlaylistDetail(detail);
  await refreshPlaylists(activeRepoId.value);
  return detail;
}

export async function reorderPlaylistItemsInWorkspace(playlistId: string, itemIds: string[]) {
  if (!activeRepoId.value) return null;
  const detail = await reorderPlaylistItems({
    repoId: activeRepoId.value,
    playlistId,
    itemIds,
  });
  activePlaylistDetail.value = cachePlaylistDetail(detail);
  await refreshPlaylists(activeRepoId.value);
  return detail;
}

export async function removePlaylistItemInWorkspace(playlistId: string, playlistItemId: string) {
  if (!activeRepoId.value) return null;
  const detail = await removePlaylistItem({
    repoId: activeRepoId.value,
    playlistId,
    playlistItemId,
  });
  activePlaylistDetail.value = cachePlaylistDetail(detail);
  await refreshPlaylists(activeRepoId.value);
  return detail;
}

export async function setPlaylistMembershipInWorkspace(assetId: string, playlistIds: string[]) {
  if (!activeRepoId.value) return null;
  const response: PlaylistMembershipSnapshot = await setPlaylistMembership({
    repoId: activeRepoId.value,
    assetId,
    playlistIds,
  });
  await refreshPlaylists(activeRepoId.value);
  return response;
}
