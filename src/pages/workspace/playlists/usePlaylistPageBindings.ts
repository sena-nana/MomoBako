import { computed, type ComputedRef } from "vue";
import type { WorkspacePlayerBarHandlers, WorkspacePlayerBarProps } from "../../../components/workspacePlayerBar.contract";
import type { PlaylistDetail, PlaylistItem } from "../../../types/repository";

type WorkspacePlaylistPageBindingOptions = {
  activePlaylistDetail: ComputedRef<PlaylistDetail | null>;
  hasPlayer: ComputedRef<boolean>;
  playlistItemThumbnailSrc: (item: PlaylistItem) => string | null;
  playlistStatusLabel: ComputedRef<string>;
  showWorkspacePlayer: ComputedRef<boolean>;
  workspacePlayerBarHandlers: WorkspacePlayerBarHandlers;
  workspacePlayerBarProps: ComputedRef<WorkspacePlayerBarProps>;
  handlePlaylistDragStart: (item: PlaylistItem) => void;
  handlePlaylistDrop: (item: PlaylistItem) => void | Promise<unknown>;
  openPlaylistItemPreview: (item: PlaylistItem) => void;
  playPlaylistFromItem: (item?: PlaylistItem | null) => void | Promise<unknown>;
  removePlaylistItem: (item: PlaylistItem) => void | Promise<unknown>;
};

export function usePlaylistPageBindings(options: WorkspacePlaylistPageBindingOptions) {
  const playlistPageProps = computed(() => ({
    activePlaylistDetail: options.activePlaylistDetail.value,
    hasPlayer: options.hasPlayer.value,
    playlistItemThumbnailSrc: options.playlistItemThumbnailSrc,
    playlistStatusLabel: options.playlistStatusLabel.value,
    showWorkspacePlayer: options.showWorkspacePlayer.value,
    workspacePlayerBarHandlers: options.workspacePlayerBarHandlers,
    workspacePlayerBarProps: options.workspacePlayerBarProps.value,
  }));

  const playlistPageHandlers = {
    dragStart: options.handlePlaylistDragStart,
    dropItem: options.handlePlaylistDrop,
    openPreview: options.openPlaylistItemPreview,
    play: options.playPlaylistFromItem,
    remove: options.removePlaylistItem,
  };

  return {
    playlistPageHandlers,
    playlistPageProps,
  };
}
