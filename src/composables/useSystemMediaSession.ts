import { computed, watch, type WatchStopHandle } from "vue";
import type { usePlaylistPlayer } from "./usePlaylistPlayer";
import { resolveThumbnailSrc } from "../utils/thumbnailSrc";

type PlaylistPlayerController = ReturnType<typeof usePlaylistPlayer>;

function mediaSession() {
  return typeof navigator !== "undefined" ? navigator.mediaSession : undefined;
}

function artworkForItem(item: PlaylistPlayerController["currentItem"]["value"]) {
  const src = resolveThumbnailSrc(item?.thumbnailPath);
  if (!src) return [];
  return [{
    src,
    sizes: "512x512",
    type: "image/png",
  }];
}

function setActionHandler(
  session: MediaSession,
  action: MediaSessionAction,
  handler: MediaSessionActionHandler | null,
) {
  try {
    session.setActionHandler(action, handler);
  } catch {
    /* ignore unsupported actions */
  }
}

export function useSystemMediaSession(player: PlaylistPlayerController) {
  const session = mediaSession();
  const supported = Boolean(session && typeof MediaMetadata !== "undefined");
  const stopHandles: WatchStopHandle[] = [];

  if (!session || !supported) {
    return {
      supported,
      dispose: () => {},
    };
  }

  const metadata = computed(() => {
    const item = player.currentItem.value;
    const payload = item?.sourcePayload ?? {};
    const artists = Array.isArray(payload.artists)
      ? payload.artists.join(", ")
      : typeof payload.artist === "string"
        ? payload.artist
        : "";
    return {
      title: String(payload.songName ?? item?.filename ?? "MomoBako"),
      artist: artists,
      album: String(payload.albumName ?? ""),
      artwork: artworkForItem(item),
    };
  });

  stopHandles.push(watch(metadata, (value) => {
    session.metadata = new MediaMetadata(value);
  }, { immediate: true }));

  stopHandles.push(watch([
    player.isPlaying,
    player.currentTimeMs,
    player.durationMs,
    player.currentItem,
  ], () => {
    session.playbackState = player.isPlaying.value ? "playing" : "paused";
    if (session.setPositionState && player.durationMs.value > 0) {
      session.setPositionState({
        duration: player.durationMs.value / 1000,
        playbackRate: 1,
        position: Math.min(player.currentTimeMs.value, player.durationMs.value) / 1000,
      });
    }
  }, { immediate: true }));

  setActionHandler(session, "play", () => {
    void player.setPlaybackState({ isPlaying: true });
  });
  setActionHandler(session, "pause", () => {
    void player.setPlaybackState({ isPlaying: false });
  });
  setActionHandler(session, "previoustrack", () => {
    void player.playPrevious();
  });
  setActionHandler(session, "nexttrack", () => {
    void player.playNext(false);
  });
  setActionHandler(session, "seekto", (details) => {
    if (typeof details.seekTime === "number") {
      void player.setPlaybackState({ currentTimeMs: Math.round(details.seekTime * 1000) });
    }
  });
  setActionHandler(session, "stop", () => {
    void player.stop(false);
  });

  return {
    supported,
    dispose: () => {
      stopHandles.forEach((stop) => stop());
      session.metadata = null;
      session.playbackState = "none";
      for (const action of ["play", "pause", "previoustrack", "nexttrack", "seekto", "stop"] as const) {
        setActionHandler(session, action, null);
      }
    },
  };
}
