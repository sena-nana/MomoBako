import type { ComputedRef } from "vue";
import type { ContextMenuItem } from "../../composables/useContextMenu";
import { getPlaylistPlayerByType } from "../../plugins/playlistPlayers";
import type { FileBrowserEntry, PlaylistSummary } from "../../types/repository";

type WorkspacePlaylistMembershipUiOptions = {
  playlistMemberships: ComputedRef<Record<string, string[]>>;
  playlists: ComputedRef<PlaylistSummary[]>;
  setPlaylistMembershipInWorkspace: (assetId: string, playlistIds: string[]) => Promise<unknown>;
};

export function useWorkspacePlaylistMembershipUi(options: WorkspacePlaylistMembershipUiOptions) {
  function compatiblePlaylistsForEntry(entry: FileBrowserEntry) {
    if (entry.kind !== "file" || !entry.assetId) return [];
    const extension = (entry.extension ?? "").toLowerCase();
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

  function playlistMenuItems(entry: FileBrowserEntry): ContextMenuItem[] {
    return compatiblePlaylistsForEntry(entry).map((playlist) => ({
      id: `playlist-${playlist.playlistId}`,
      label: playlist.name,
      checked: (options.playlistMemberships.value[entry.assetId ?? ""] ?? []).includes(playlist.playlistId),
      onSelect: () => togglePlaylistMembership(entry, playlist.playlistId),
    }));
  }

  return {
    playlistMenuItems,
  };
}
