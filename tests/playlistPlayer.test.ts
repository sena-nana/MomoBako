import { afterEach, describe, expect, it, vi } from "vitest";
import { waitFor } from "@testing-library/vue";
import { usePlaylistPlayer } from "../src/composables/usePlaylistPlayer";
import { clearPreviewPluginRegistry, syncRegisteredPreviewPluginManifests } from "../src/plugins/sdk";
import { listPlugins } from "../src/services/repositoryApi";
import type { PlaylistDetail, PlaylistSummary } from "../src/types/repository";

function playlist(fileClass: "image" | "audio" | "video"): PlaylistSummary {
  const playerTypeId = fileClass === "image"
    ? "momobako.playlist.image-slideshow"
    : fileClass === "audio"
      ? "momobako.playlist.audio-sequence"
      : "momobako.playlist.video-sequence";
  const playerLabel = fileClass === "image" ? "图片幻灯片" : fileClass === "audio" ? "音频顺序播放" : "视频顺序播放";
  return {
    playlistId: `playlist-${fileClass}`,
    repoId: "repo-main-001",
    name: `${playerLabel} Test`,
    playerTypeId,
    playerPluginId: "momobako.preview.media",
    playerLabel,
    fileClass,
    itemCount: 2,
    sortOrder: 0,
    createdAt: "2026-06-05T00:18:00Z",
    updatedAt: "2026-06-05T00:18:00Z",
  };
}

function playlistDetail(fileClass: "image" | "audio" | "video"): PlaylistDetail {
  const extension = fileClass === "image" ? "png" : fileClass === "audio" ? "mp3" : "mp4";
  const summary = playlist(fileClass);
  return {
    playlist: summary,
    items: [1, 2].map((index) => ({
      playlistItemId: `${fileClass}-item-${index}`,
      playlistId: summary.playlistId,
      assetId: `${fileClass}-asset-${index}`,
      path: `Media/${fileClass}-${index}.${extension}`,
      filename: `${fileClass}-${index}.${extension}`,
      extension,
      thumbnailPath: null,
      status: "ready",
      statusReason: null,
      sortOrder: index - 1,
      addedAt: "2026-06-05T00:18:00Z",
    })),
  };
}

async function registerMediaPlugin() {
  await syncRegisteredPreviewPluginManifests(await listPlugins());
}

afterEach(() => {
  vi.useRealTimers();
  clearPreviewPluginRegistry();
  usePlaylistPlayer().resetPlayerState();
});

describe("usePlaylistPlayer", () => {
  it("uses configured image duration and pauses slideshow timers", async () => {
    vi.useFakeTimers();
    await registerMediaPlugin();
    const player = usePlaylistPlayer();
    const mount = document.createElement("div");
    document.body.append(mount);
    player.attachMountTarget(mount);

    await player.updatePlaybackSettings({ imageDurationMs: 2000 });
    await player.setActivePlaylist("repo-main-001", playlistDetail("image"), "image-item-1", { autoPlay: true });

    expect(player.durationMs.value).toBe(2000);
    expect(player.currentItemId.value).toBe("image-item-1");
    await vi.advanceTimersByTimeAsync(1000);
    expect(player.currentTimeMs.value).toBe(1000);

    await player.setPlaybackState({ isPlaying: false });
    await vi.advanceTimersByTimeAsync(3000);
    expect(player.currentItemId.value).toBe("image-item-1");

    await player.setPlaybackState({ isPlaying: true });
    await vi.advanceTimersByTimeAsync(1000);
    await waitFor(() => {
      expect(player.currentItemId.value).toBe("image-item-2");
    });
    expect(localStorage.getItem("momobako.playbackSettings")).toContain("\"imageDurationMs\":2000");
  });

  it("moves the current runtime between hidden and visible mount targets", async () => {
    await registerMediaPlugin();
    const player = usePlaylistPlayer();
    const hiddenMount = document.createElement("div");
    const visibleMount = document.createElement("div");
    document.body.append(hiddenMount, visibleMount);
    player.attachMountTarget(hiddenMount);

    await player.setActivePlaylist("repo-main-001", playlistDetail("image"), "image-item-1");
    expect(hiddenMount.querySelector("[data-path='Media/image-1.png']")).toBeInTheDocument();

    player.attachVisibleMountTarget(visibleMount);
    await waitFor(() => {
      expect(visibleMount.querySelector("[data-path='Media/image-1.png']")).toBeInTheDocument();
    });
    expect(hiddenMount.querySelector("[data-path='Media/image-1.png']")).toBeNull();

    await player.setPlaybackState({ currentTimeMs: 1200 });
    player.attachVisibleMountTarget(null);
    await waitFor(() => {
      expect(hiddenMount.querySelector("[data-path='Media/image-1.png']")).toBeInTheDocument();
    });
    expect(player.currentTimeMs.value).toBe(1200);
  });

  it("supports video seek, volume, object fit and ended navigation", async () => {
    await registerMediaPlugin();
    const player = usePlaylistPlayer();
    const mount = document.createElement("div");
    document.body.append(mount);
    player.attachMountTarget(mount);

    await player.updatePlaybackSettings({ objectFit: "cover" });
    await player.setActivePlaylist("repo-main-001", playlistDetail("video"), "video-item-1");

    const video = mount.querySelector<HTMLVideoElement>("video");
    expect(video).toBeInstanceOf(HTMLVideoElement);
    expect(video?.controls).toBe(false);
    expect(video?.hasAttribute("controls")).toBe(false);
    expect(video?.style.objectFit).toBe("cover");

    await player.setPlaybackState({ currentTimeMs: 42000, volume: 0.35 });
    expect(video?.currentTime).toBe(42);
    expect(video?.volume).toBe(0.35);

    video?.dispatchEvent(new Event("ended"));
    await waitFor(() => {
      expect(player.currentItemId.value).toBe("video-item-2");
    });
  });

  it("keeps audio runtime controlled by the workspace player bar", async () => {
    await registerMediaPlugin();
    const player = usePlaylistPlayer();
    const mount = document.createElement("div");
    document.body.append(mount);
    player.attachMountTarget(mount);

    await player.setActivePlaylist("repo-main-001", playlistDetail("audio"), "audio-item-1");

    const audio = mount.querySelector<HTMLAudioElement>("audio");
    expect(audio).toBeInstanceOf(HTMLAudioElement);
    expect(audio?.controls).toBe(false);
    expect(audio?.hasAttribute("controls")).toBe(false);
    expect(mount.querySelector(".media-preview__download-progress")).toBeNull();
  });
});
