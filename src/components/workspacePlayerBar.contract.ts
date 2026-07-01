/**
 * 播放器栏共享契约。
 * 统一维护视图层与页面绑定层之间的 props 和事件类型，避免重复定义。
 */
import type { PlaylistPlayerObjectFit } from "../plugins/sdk";
import type { PlaylistItem, PlaylistPlaybackMode } from "../types/repository";

export type WorkspacePlayerBarProps = {
  item: PlaylistItem | null;
  playerLabel?: string | null;
  fileClass?: string | null;
  supportsSeek?: boolean;
  supportsVolume?: boolean;
  canPlay?: boolean;
  mode: PlaylistPlaybackMode;
  currentTimeMs: number;
  durationMs: number;
  volume: number;
  imageDurationMs: number;
  objectFit: PlaylistPlayerObjectFit;
  isPlaying: boolean;
  errorMessage?: string | null;
  queueOpen: boolean;
  queueItems: PlaylistItem[];
  currentItemId: string | null;
};

export type WorkspacePlayerBarEmitMap = {
  togglePlay: [];
  previous: [];
  next: [];
  cycleMode: [];
  openQueue: [];
  openPreview: [];
  setVolume: [value: number];
  selectQueueItem: [playlistItemId: string];
  seek: [timeMs: number];
  setImageDuration: [value: number];
  setObjectFit: [value: PlaylistPlayerObjectFit];
};

export type WorkspacePlayerBarHandlers = {
  [TEvent in keyof WorkspacePlayerBarEmitMap]:
    (...args: WorkspacePlayerBarEmitMap[TEvent]) => void | Promise<unknown>;
};
