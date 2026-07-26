import type {
  DownloaderPlaylistProgressEvent,
  DownloaderPlaylistRequest,
  PlaylistDetail,
  PlaylistItemsAddRequest,
  PlaylistItemsByPathsAddRequest,
  PlaylistItemsOrderRequest,
  PlaylistMembershipIndex,
  PlaylistMembershipRequest,
  PlaylistMembershipSnapshot,
  PlaylistMutationRequest,
  PlaylistMutationResponse,
  PlaylistSummary,
  PlaylistUpdateRequest,
} from "../../types/repository";
import { invokeCommand } from "./core";
import { runMutsukiTask } from "../mutsukiTasks";

export function listPlaylists(repoId: string) {
  return invokeCommand<PlaylistSummary[]>("list_playlists", { repoId });
}

export function listPlaylistMemberships(repoId: string) {
  return invokeCommand<PlaylistMembershipIndex>("list_playlist_memberships", { repoId });
}

export function createPlaylist(request: PlaylistMutationRequest) {
  return invokeCommand<PlaylistMutationResponse>("create_playlist", { request });
}

export function updatePlaylist(request: PlaylistUpdateRequest) {
  return invokeCommand<PlaylistMutationResponse>("update_playlist", { request });
}

export function deletePlaylist(repoId: string, playlistId: string) {
  return invokeCommand<PlaylistMutationResponse>("delete_playlist", { repoId, playlistId });
}

export function getPlaylistDetail(repoId: string, playlistId: string) {
  return invokeCommand<PlaylistDetail>("get_playlist_detail", { repoId, playlistId });
}

export function addPlaylistItems(request: PlaylistItemsAddRequest) {
  return invokeCommand<PlaylistDetail>("add_playlist_items", { request });
}

export function addPlaylistItemsByPaths(request: PlaylistItemsByPathsAddRequest) {
  return invokeCommand<PlaylistDetail>("add_playlist_items_by_paths", { request });
}

export function reorderPlaylistItems(request: PlaylistItemsOrderRequest) {
  return invokeCommand<PlaylistDetail>("reorder_playlist_items", { request });
}

export function removePlaylistItem(request: {
  repoId: string;
  playlistId: string;
  playlistItemId: string;
}) {
  return invokeCommand<PlaylistDetail>("remove_playlist_item", { request });
}

export function setPlaylistMembership(request: PlaylistMembershipRequest) {
  return invokeCommand<PlaylistMembershipSnapshot>("set_playlist_membership", { request });
}

export function downloadPlaylistWithProgress(
  request: DownloaderPlaylistRequest,
  onEvent: (event: DownloaderPlaylistProgressEvent) => void,
  signal?: AbortSignal,
) {
  return runMutsukiTask<Record<string, unknown>, DownloaderPlaylistProgressEvent>(
    "momobako.playlist.download",
    request,
    onEvent,
    signal,
  );
}
