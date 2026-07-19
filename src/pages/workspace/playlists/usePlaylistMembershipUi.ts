import type { ComputedRef } from "vue";
import type { ContextMenuItem } from "../../../ui";
import { getPlaylistPlayerByType } from "../../../plugins/playlistPlayers";
import type { FileBrowserEntry, PlaylistSummary } from "../../../types/repository";

type WorkspacePlaylistMembershipUiOptions = {
  playlistMemberships: ComputedRef<Record<string, string[]>>;
  playlists: ComputedRef<PlaylistSummary[]>;
  addPlaylistItemsByPathsInWorkspace: (playlistId: string, paths: string[]) => Promise<unknown>;
  setPlaylistMembershipInWorkspace: (assetId: string, playlistIds: string[]) => Promise<unknown>;
};

export function usePlaylistMembershipUi(options: WorkspacePlaylistMembershipUiOptions) {
  function compatiblePlaylistsForEntry(entry: FileBrowserEntry) {
    if (entry.kind === "directory") {
      return options.playlists.value;
    }
    const extension = (entry.extension ?? "").toLowerCase();
    if (!extension) return [];
    return options.playlists.value.filter((playlist) => {
      const player = getPlaylistPlayerByType(playlist.playerTypeId);
      return Boolean(player?.supportedExtensions.includes(extension));
    });
  }

  async function togglePlaylistMembership(entry: FileBrowserEntry, playlistId: string) {
    if (!entry.assetId) return;
    const currentMemberships = options.playlistMemberships.value[entry.assetId] ?? [];
    const nextMemberships = currentMemberships.includes(playlistId)
      ? currentMemberships.filter((item) => item !== playlistId)
      : [...currentMemberships, playlistId];
    await options.setPlaylistMembershipInWorkspace(entry.assetId, nextMemberships);
  }

  async function addPathsToPlaylist(entry: FileBrowserEntry, playlistId: string) {
    await options.addPlaylistItemsByPathsInWorkspace(playlistId, [entry.path]);
  }

  function playlistMenuItems(entry: FileBrowserEntry): ContextMenuItem[] {
    const canToggleMembership = entry.kind === "file" && Boolean(entry.assetId) && !entry.isVirtual;
    return compatiblePlaylistsForEntry(entry).map((playlist) => ({
      id: `playlist-${playlist.playlistId}`,
      label: playlist.name,
      checked: canToggleMembership
        ? (options.playlistMemberships.value[entry.assetId ?? ""] ?? []).includes(playlist.playlistId)
        : undefined,
      onSelect: () => canToggleMembership
        ? togglePlaylistMembership(entry, playlist.playlistId)
        : addPathsToPlaylist(entry, playlist.playlistId),
    }));
  }

  return {
    playlistMenuItems,
  };
}
