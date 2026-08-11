import { computed, ref, type ComputedRef } from "vue";
import type {
  WorkspacePlayerBarHandlers,
  WorkspacePlayerBarProps,
} from "../../../components/workspacePlayerBar.contract";
import { getPlaylistPlayerByType } from "../../../plugins/playlistPlayers";
import type { PlaylistDetail, PlaylistItem } from "../../../types/repository";
import type { usePlaylistPlayer } from "../../../composables/usePlaylistPlayer";
import type { PlaylistPlayerObjectFit } from "../../../plugins/sdk";
import { resolveThumbnailSrc } from "../../../utils/thumbnailSrc";

type PlaylistPlayerController = ReturnType<typeof usePlaylistPlayer>;

type WorkspacePlayerUiOptions = {
  activePlaylistDetail: ComputedRef<PlaylistDetail | null>;
  activePlaylistId: ComputedRef<string | null>;
  activeRepoId: ComputedRef<string | null>;
  playlistPlayer: PlaylistPlayerController;
  removePlaylistItemInWorkspace: (playlistId: string, playlistItemId: string) => Promise<unknown>;
  reorderPlaylistItemsInWorkspace: (playlistId: string, itemIds: string[]) => Promise<unknown>;
  selectWorkspaceEntry: (path: string) => void;
  setActiveLibraryCategory: (category: "all") => void;
  setActivePanel: (panel: "files") => void;
  setActivePreviewPath: (path: string | null) => void;
  setPreviewFilePath: (path: string | null) => void;
};

export function playlistItemThumbnailSrc(item: PlaylistItem) {
  return resolveThumbnailSrc(item.thumbnailPath);
}

export function playlistItemToFileEntry(item: PlaylistItem) {
  return {
    path: item.path,
    name: item.filename,
    kind: "file" as const,
    extension: item.extension,
    assetId: item.assetId,
    status: item.status,
    thumbnailPath: item.thumbnailPath,
    isVirtual: item.isVirtual,
    providerId: item.providerId,
    providerItemId: item.providerItemId,
    sourcePayload: item.sourcePayload,
    metadata: item.metadata ?? undefined,
    localAbsolutePath: item.localAbsolutePath,
  };
}

export function usePlayerUi(options: WorkspacePlayerUiOptions) {
  const playlistDragItemId = ref<string | null>(null);

  const activePlaylistPlayer = computed(() => getPlaylistPlayerByType(options.activePlaylistDetail.value?.playlist.playerTypeId));
  const workspacePlayerDefinition = computed(() => (
    options.playlistPlayer.currentPlayerDefinition.value
  ));
  const showWorkspacePlayer = computed(() => Boolean(options.activeRepoId.value));
  const playerQueueItems = computed<PlaylistItem[]>(() => (
    (options.playlistPlayer.queueItems.value ?? []).map((item) => ({
      ...item,
      thumbnailPath: resolveThumbnailSrc(item.thumbnailPath),
    }))
  ));
  const currentPlayerItem = computed<PlaylistItem | null>(() => {
    const currentId = options.playlistPlayer.currentItemId.value;
    return currentId
      ? playerQueueItems.value.find((item) => item.playlistItemId === currentId) ?? null
      : null;
  });
  const playlistStatusLabel = computed(() => {
    if (!options.activePlaylistDetail.value) return "";
    return `${options.activePlaylistDetail.value.playlist.playerLabel} · ${options.activePlaylistDetail.value.items.length} 项`;
  });

  function openPlaylistItemPreview(item: PlaylistItem) {
    options.setActiveLibraryCategory("all");
    options.setPreviewFilePath(item.path);
    options.setActivePreviewPath(item.path);
    options.selectWorkspaceEntry(item.path);
    options.setActivePanel("files");
  }

  function openCurrentPlayerPreview() {
    const item = options.playlistPlayer.currentItem.value;
    if (!item) return;
    openPlaylistItemPreview(item);
  }

  function cycleWorkspacePlayerMode() {
    const nextMode = options.playlistPlayer.mode.value === "listLoop"
      ? "shuffle"
      : options.playlistPlayer.mode.value === "shuffle"
        ? "singleLoop"
        : "listLoop";
    void options.playlistPlayer.setPlaybackState({ mode: nextMode });
  }

  async function playPlaylistFromItem(item?: PlaylistItem | null) {
    if (!options.activeRepoId.value || !options.activePlaylistDetail.value) return;
    const startItemId = item?.playlistItemId
      ?? options.activePlaylistDetail.value.items.find((entry) => entry.status === "ready")?.playlistItemId
      ?? options.activePlaylistDetail.value.items[0]?.playlistItemId
      ?? null;
    await options.playlistPlayer.setActivePlaylist(options.activeRepoId.value, options.activePlaylistDetail.value, startItemId, { autoPlay: true });
  }

  async function removePlaylistItem(item: PlaylistItem) {
    if (!options.activePlaylistId.value) return;
    await options.removePlaylistItemInWorkspace(options.activePlaylistId.value, item.playlistItemId);
  }

  function handlePlaylistDragStart(item: PlaylistItem) {
    playlistDragItemId.value = item.playlistItemId;
  }

  async function handlePlaylistDrop(item: PlaylistItem) {
    if (!options.activePlaylistId.value || !options.activePlaylistDetail.value || !playlistDragItemId.value) return;
    const sourceId = playlistDragItemId.value;
    if (sourceId === item.playlistItemId) {
      playlistDragItemId.value = null;
      return;
    }
    const items = [...options.activePlaylistDetail.value.items];
    const sourceIndex = items.findIndex((entry) => entry.playlistItemId === sourceId);
    const targetIndex = items.findIndex((entry) => entry.playlistItemId === item.playlistItemId);
    if (sourceIndex < 0 || targetIndex < 0) {
      playlistDragItemId.value = null;
      return;
    }
    const [moved] = items.splice(sourceIndex, 1);
    items.splice(targetIndex, 0, moved);
    playlistDragItemId.value = null;
    await options.reorderPlaylistItemsInWorkspace(options.activePlaylistId.value, items.map((entry) => entry.playlistItemId));
  }

  const workspacePlayerBarProps = computed<WorkspacePlayerBarProps>(() => ({
    item: currentPlayerItem.value,
    playerLabel: options.playlistPlayer.currentPlayerLabel.value,
    fileClass: workspacePlayerDefinition.value?.fileClass,
    supportsSeek: workspacePlayerDefinition.value?.supportsSeek ?? false,
    supportsVolume: workspacePlayerDefinition.value?.supportsVolume ?? false,
    canPlay: options.playlistPlayer.canPlay.value,
    mode: options.playlistPlayer.mode.value,
    currentTimeMs: options.playlistPlayer.currentTimeMs.value,
    durationMs: options.playlistPlayer.durationMs.value,
    volume: options.playlistPlayer.volume.value,
    imageDurationMs: options.playlistPlayer.playbackSettings.value.imageDurationMs,
    objectFit: options.playlistPlayer.playbackSettings.value.objectFit,
    isPlaying: options.playlistPlayer.isPlaying.value,
    errorMessage: options.playlistPlayer.errorMessage.value,
    queueOpen: options.playlistPlayer.queueOpen.value,
    queueItems: playerQueueItems.value,
    currentItemId: options.playlistPlayer.currentItemId.value,
  }));

  const workspacePlayerBarHandlers: WorkspacePlayerBarHandlers = {
    togglePlay: () => options.playlistPlayer.setPlaybackState({ isPlaying: !options.playlistPlayer.isPlaying.value }),
    previous: () => options.playlistPlayer.playPrevious(),
    next: () => options.playlistPlayer.playNext(false),
    cycleMode: cycleWorkspacePlayerMode,
    openQueue: () => options.playlistPlayer.setQueueOpen(!options.playlistPlayer.queueOpen.value),
    openPreview: openCurrentPlayerPreview,
    setVolume: (value: number) => options.playlistPlayer.setPlaybackState({ volume: value }),
    selectQueueItem: (playlistItemId: string) => options.playlistPlayer.playItem(playlistItemId, true),
    seek: (timeMs: number) => options.playlistPlayer.setPlaybackState({ currentTimeMs: timeMs }),
    setImageDuration: (imageDurationMs: number) => options.playlistPlayer.updatePlaybackSettings({ imageDurationMs }),
    setObjectFit: (objectFit: PlaylistPlayerObjectFit) => options.playlistPlayer.updatePlaybackSettings({ objectFit }),
  };

  return {
    activePlaylistPlayer,
    handlePlaylistDragStart,
    handlePlaylistDrop,
    openCurrentPlayerPreview,
    openPlaylistItemPreview,
    playlistStatusLabel,
    playPlaylistFromItem,
    playerQueueItems,
    removePlaylistItem,
    showWorkspacePlayer,
    workspacePlayerBarHandlers,
    workspacePlayerBarProps,
  };
}
