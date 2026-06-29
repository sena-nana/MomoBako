import { computed, ref, type ComputedRef } from "vue";
import type { RouteLocationNormalizedLoadedGeneric, Router } from "vue-router";
import { listPlaylistPlayers } from "../plugins/playlistPlayers";
import { getPlaylistDetail } from "../services/repositoryApi";
import type { PlaylistDetail, PlaylistMutationRequest, PlaylistMutationResponse, PlaylistSummary } from "../types/repository";
import type { usePlaylistPlayer } from "../composables/usePlaylistPlayer";

type PlaylistPlayerController = ReturnType<typeof usePlaylistPlayer>;

type PlaylistSidebarUiOptions = {
  activePlaylistDetail: ComputedRef<PlaylistDetail | null>;
  activePlaylistId: ComputedRef<string | null>;
  activeRepoId: ComputedRef<string | null>;
  createPlaylistInWorkspace: (request: Omit<PlaylistMutationRequest, "repoId">) => Promise<PlaylistMutationResponse | null>;
  deletePlaylistInWorkspace: (playlistId: string) => Promise<PlaylistMutationResponse | null>;
  isActiveRepositoryMissing: ComputedRef<boolean>;
  playlistPlayer: PlaylistPlayerController;
  playlists: ComputedRef<PlaylistSummary[]>;
  refreshPlaylists: () => Promise<unknown>;
  route: RouteLocationNormalizedLoadedGeneric;
  router: Router;
  selectPlaylist: (playlistId: string) => Promise<unknown>;
};

export function usePlaylistSidebarUi(options: PlaylistSidebarUiOptions) {
  const showPlaylistDialog = ref(false);
  const playlistsExpanded = ref(false);
  const playlistName = ref("");
  const playlistPlayerTypeId = ref("");

  const availablePlaylistPlayers = computed(() => listPlaylistPlayers() ?? []);
  const playlistDialogDisabled = computed(() => !playlistName.value.trim() || !playlistPlayerTypeId.value);
  const playlistItems = computed(() => options.playlists.value ?? []);
  const activePlaylist = computed(() => playlistItems.value.find((item) => item.playlistId === options.activePlaylistId.value) ?? null);
  const availablePlaylistPlayerTypeIds = computed(() => new Set(availablePlaylistPlayers.value.map((player) => player.playerTypeId)));

  function togglePlaylistsExpanded() {
    playlistsExpanded.value = !playlistsExpanded.value;
  }

  function openPlaylistDialog() {
    if (!options.activeRepoId.value || options.isActiveRepositoryMissing.value) return;
    playlistName.value = "";
    playlistPlayerTypeId.value = availablePlaylistPlayers.value[0]?.playerTypeId ?? "";
    showPlaylistDialog.value = true;
  }

  function closePlaylistDialog() {
    showPlaylistDialog.value = false;
  }

  async function submitPlaylistDialog() {
    if (!options.activeRepoId.value || playlistDialogDisabled.value) return;
    const response = await options.createPlaylistInWorkspace({
      name: playlistName.value.trim(),
      playerTypeId: playlistPlayerTypeId.value,
    });
    if (response) {
      showPlaylistDialog.value = false;
    }
  }

  async function openPlaylist(playlistId: string) {
    if (options.route.path === "/settings") {
      await options.router.push("/");
    }
    await options.selectPlaylist(playlistId);
  }

  async function playPlaylist(playlist: PlaylistSummary) {
    const detail = options.activePlaylistDetail.value?.playlist.playlistId === playlist.playlistId
      ? options.activePlaylistDetail.value
      : options.activeRepoId.value ? await getPlaylistDetail(options.activeRepoId.value, playlist.playlistId) : null;
    if (!options.activeRepoId.value || !detail) return;
    const startItemId = detail.items.find((item) => item.status === "ready")?.playlistItemId ?? detail.items[0]?.playlistItemId ?? null;
    await options.playlistPlayer.setActivePlaylist(options.activeRepoId.value, detail, startItemId, { autoPlay: true });
  }

  async function removePlaylist(playlistId: string) {
    await options.deletePlaylistInWorkspace(playlistId);
    await options.refreshPlaylists();
  }

  return {
    activePlaylist,
    availablePlaylistPlayers,
    availablePlaylistPlayerTypeIds,
    closePlaylistDialog,
    openPlaylist,
    openPlaylistDialog,
    playlistDialogDisabled,
    playlistItems,
    playlistsExpanded,
    playlistName,
    playlistPlayerTypeId,
    playPlaylist,
    removePlaylist,
    showPlaylistDialog,
    submitPlaylistDialog,
    togglePlaylistsExpanded,
  };
}
