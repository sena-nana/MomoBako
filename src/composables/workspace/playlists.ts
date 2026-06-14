import {
  addPlaylistItems,
  addPlaylistItemsByPaths,
  createPlaylist,
  deletePlaylist,
  getPlaylistDetail,
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
import {
  activePanel,
  activePlaylistDetail,
  activePlaylistId,
  activeRepoId,
  playlistMemberships,
  playlists,
} from "./state";

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

  const details = (await Promise.all(
    playlistItems.map(async (playlist) => {
      try {
        return await getPlaylistDetail(repoId, playlist.playlistId);
      } catch {
        return null;
      }
    }),
  )).filter((detail): detail is PlaylistDetail => Boolean(detail));

  rebuildPlaylistMemberships(details);

  if (activePlaylistId.value) {
    activePlaylistDetail.value = details.find((detail) => detail.playlist.playlistId === activePlaylistId.value) ?? null;
  }

  return details;
}

export async function refreshPlaylists(repoId = activeRepoId.value) {
  if (!repoId) return [];
  const items = await listPlaylists(repoId);
  if (activeRepoId.value !== repoId) {
    return items;
  }
  playlists.value = items;
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
  activePlaylistDetail.value = await getPlaylistDetail(activeRepoId.value, playlistId);
  return activePlaylistDetail.value;
}

export async function createPlaylistInWorkspace(request: Omit<PlaylistMutationRequest, "repoId">) {
  if (!activeRepoId.value) return null;
  const response = await createPlaylist({ ...request, repoId: activeRepoId.value });
  playlists.value = response.playlists;
  await syncPlaylistMemberships(activeRepoId.value, response.playlists);
  if (response.playlist?.playlistId) {
    await selectPlaylist(response.playlist.playlistId);
  }
  return response;
}

export async function deletePlaylistInWorkspace(playlistId: string) {
  if (!activeRepoId.value) return null;
  const response = await deletePlaylist(activeRepoId.value, playlistId);
  playlists.value = response.playlists;
  await syncPlaylistMemberships(activeRepoId.value, response.playlists);
  if (activePlaylistId.value === playlistId) {
    activePlaylistId.value = null;
    activePlaylistDetail.value = null;
    if (activePanel.value === "playlist") activePanel.value = "files";
  }
  return response;
}

export async function addPlaylistItemsInWorkspace(playlistId: string, assetIds: string[]) {
  if (!activeRepoId.value) return null;
  const detail = await addPlaylistItems({
    repoId: activeRepoId.value,
    playlistId,
    assetIds,
  });
  activePlaylistDetail.value = detail;
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
  activePlaylistDetail.value = detail;
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
  activePlaylistDetail.value = detail;
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
  activePlaylistDetail.value = detail;
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
