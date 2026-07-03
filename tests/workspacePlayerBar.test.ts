import { render, screen } from "@testing-library/vue";
import { describe, expect, it, vi } from "vitest";
import WorkspacePlayerBar from "../src/components/WorkspacePlayerBar.vue";

function renderPlayerBar() {
  return render(WorkspacePlayerBar, {
    props: {
      item: {
        playlistItemId: "item-current",
        playlistId: "playlist-audio",
        assetId: "asset-current",
        path: "Playlist/night-drive.mp3",
        filename: "night-drive.mp3",
        extension: "mp3",
        thumbnailPath: null,
        status: "ready",
        statusReason: null,
        sortOrder: 0,
        addedAt: "2026-07-03T12:00:00Z",
      },
      playerLabel: "音频顺序播放",
      fileClass: "audio",
      supportsSeek: true,
      supportsVolume: true,
      canPlay: true,
      mode: "listLoop",
      currentTimeMs: 30000,
      durationMs: 180000,
      volume: 0.45,
      imageDurationMs: 5000,
      objectFit: "contain",
      isPlaying: true,
      errorMessage: null,
      queueOpen: true,
      queueItems: [
        {
          playlistItemId: "item-current",
          playlistId: "playlist-audio",
          assetId: "asset-current",
          path: "Playlist/night-drive.mp3",
          filename: "night-drive.mp3",
          extension: "mp3",
          thumbnailPath: null,
          status: "ready",
          statusReason: null,
          sortOrder: 0,
          addedAt: "2026-07-03T12:00:00Z",
        },
        {
          playlistItemId: "item-next",
          playlistId: "playlist-audio",
          assetId: "asset-next",
          path: "Playlist/dawn-loop.mp3",
          filename: "dawn-loop.mp3",
          extension: "mp3",
          thumbnailPath: null,
          status: "ready",
          statusReason: null,
          sortOrder: 1,
          addedAt: "2026-07-03T12:01:00Z",
        },
      ],
      currentItemId: "item-current",
      togglePlay: vi.fn(),
      previous: vi.fn(),
      next: vi.fn(),
      cycleMode: vi.fn(),
      openQueue: vi.fn(),
      openPreview: vi.fn(),
      setVolume: vi.fn(),
      selectQueueItem: vi.fn(),
      seek: vi.fn(),
      setImageDuration: vi.fn(),
      setObjectFit: vi.fn(),
    },
  });
}

describe("WorkspacePlayerBar", () => {
  it("播放栏和队列标题显示去扩展名名称", () => {
    renderPlayerBar();

    expect(screen.getByText("正在播放 night-drive")).toBeInTheDocument();
    expect(screen.getByText("dawn-loop")).toBeInTheDocument();
    expect(screen.queryByText("night-drive.mp3")).toBeNull();
    expect(screen.queryByText("dawn-loop.mp3")).toBeNull();
    expect(screen.getAllByText("音频").length).toBeGreaterThan(0);
  });

  it("进度条和音量条在可播放时保持可用", () => {
    renderPlayerBar();

    const sliders = screen.getAllByRole("slider");
    expect(sliders).toHaveLength(2);
    expect(sliders[0]).toBeEnabled();
    expect(sliders[1]).toBeEnabled();
  });
});
