import { render, screen, waitFor } from "@testing-library/vue";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { getPreviewPluginForEntry } from "../src/plugins/previewPlugins";
import { clearPreviewPluginRegistry, syncRegisteredPreviewPluginManifests } from "../src/plugins/sdk";
import { listPlugins } from "../src/services/repositoryApi";
import { getInvokeCalls } from "./setupTests";

const audioEntry = {
  path: "Music/theme-song.mp3",
  name: "theme-song.mp3",
  kind: "file" as const,
  extension: "mp3",
  sizeBytes: 4096,
  sizeLabel: "4 KB",
  modifiedAt: "2026-06-05T00:18:00Z",
  assetId: "asset-audio-01",
  status: "synced",
  thumbnailPath: "C:/Mock/Thumbs/Music__theme-song.mp3.jpg",
  thumbnailCustom: false,
  metadata: {},
};

describe("MediaPreview", () => {
  beforeEach(async () => {
    document.body.innerHTML = "";
    clearPreviewPluginRegistry();
    await syncRegisteredPreviewPluginManifests(await listPlugins());
  });

  it("为音频预览读取同名 lrc 歌词并显示在右侧面板", async () => {
    const plugin = getPreviewPluginForEntry(audioEntry);
    expect(plugin).not.toBeNull();

    render(plugin!.component, {
      props: {
        repoId: "repo-main-001",
        entry: audioEntry,
      },
    });

    await waitFor(() => {
      expect(getInvokeCalls("prepare_preview_file_source").at(-1)?.args).toMatchObject({
        request: {
          repoId: "repo-main-001",
          path: "Music/theme-song.mp3",
        },
      });
    });
    await waitFor(() => {
      expect(getInvokeCalls("read_file").at(-1)?.args).toMatchObject({
        request: {
          repoId: "repo-main-001",
          path: "Music/theme-song.lrc",
        },
      });
    });

    const lyricsRegion = screen.getByRole("region", { name: "歌词面板" });
    await waitFor(() => {
      expect(lyricsRegion).toHaveTextContent("Mock lyric line 1");
      expect(lyricsRegion).toHaveTextContent("Mock lyric line 2");
    });

    const lyricButtons = screen.getAllByRole("button").filter((element) => (
      element.classList.contains("media-preview__audio-lyric")
    ));
    expect(lyricButtons).toHaveLength(2);
  });

  it("同名 lrc 不存在时显示暂无歌词", async () => {
    const plugin = getPreviewPluginForEntry({
      ...audioEntry,
      path: "Music/no-lyrics-track.mp3",
      name: "no-lyrics-track.mp3",
    });
    expect(plugin).not.toBeNull();

    render(plugin!.component, {
      props: {
        repoId: "repo-main-001",
        entry: {
          ...audioEntry,
          path: "Music/no-lyrics-track.mp3",
          name: "no-lyrics-track.mp3",
        },
      },
    });

    await waitFor(() => {
      expect(getInvokeCalls("read_file").at(-1)?.args).toMatchObject({
        request: {
          repoId: "repo-main-001",
          path: "Music/no-lyrics-track.lrc",
        },
      });
    });

    expect(await screen.findByText("暂无歌词")).toBeInTheDocument();
  });

  it("根据播放时间高亮当前歌词行", async () => {
    const plugin = getPreviewPluginForEntry(audioEntry);
    expect(plugin).not.toBeNull();

    const { container } = render(plugin!.component, {
      props: {
        repoId: "repo-main-001",
        entry: audioEntry,
      },
    });

    await waitFor(() => {
      expect(container.querySelectorAll(".media-preview__audio-lyric").length).toBe(2);
    });

    const audio = container.querySelector("audio");
    expect(audio).toBeInstanceOf(HTMLAudioElement);

    Object.defineProperty(audio as HTMLAudioElement, "currentTime", {
      configurable: true,
      value: 20.2,
    });
    audio?.dispatchEvent(new Event("timeupdate"));

    await waitFor(() => {
      const activeLine = container.querySelector(".media-preview__audio-lyric.is-active");
      expect(activeLine).toHaveTextContent("Mock lyric line 2");
    });
  });

  it("ASMR 音频保存时长和收听进度", async () => {
    const plugin = getPreviewPluginForEntry({
      ...audioEntry,
      metadata: {
        libraryKind: "asmr",
        workId: "RJ123456",
        asmrEntryKind: "audio",
      },
    });
    expect(plugin).not.toBeNull();
    const saveMetadata = vi.fn().mockResolvedValue(null);

    const { container } = render(plugin!.component, {
      props: {
        repoId: "repo-main-001",
        entry: {
          ...audioEntry,
          metadata: {
            libraryKind: "asmr",
            workId: "RJ123456",
            asmrEntryKind: "audio",
          },
        },
        saveMetadata,
      },
    });

    await waitFor(() => {
      expect(container.querySelector("audio")).toBeInstanceOf(HTMLAudioElement);
    });
    const audio = container.querySelector("audio") as HTMLAudioElement;
    Object.defineProperty(audio, "duration", { configurable: true, value: 120 });
    Object.defineProperty(audio, "currentTime", { configurable: true, value: 0 });
    audio.dispatchEvent(new Event("loadedmetadata"));

    await waitFor(() => {
      expect(saveMetadata).toHaveBeenCalledWith(
        expect.objectContaining({ path: "Music/theme-song.mp3" }),
        { trackDurationMs: 120000 },
      );
    });
    saveMetadata.mockClear();

    Object.defineProperty(audio, "currentTime", { configurable: true, value: 30 });
    audio.dispatchEvent(new Event("pause"));

    await waitFor(() => {
      expect(saveMetadata).toHaveBeenCalledWith(
        expect.objectContaining({ path: "Music/theme-song.mp3" }),
        expect.objectContaining({
          listeningProgress: 25,
          listeningStatus: "listening",
          trackDurationMs: 120000,
          trackPositionMs: 30000,
        }),
      );
    });
  });

  it("普通音频不会写入 ASMR 收听进度", async () => {
    const plugin = getPreviewPluginForEntry(audioEntry);
    expect(plugin).not.toBeNull();
    const saveMetadata = vi.fn().mockResolvedValue(null);

    const { container } = render(plugin!.component, {
      props: {
        repoId: "repo-main-001",
        entry: audioEntry,
        saveMetadata,
      },
    });

    await waitFor(() => {
      expect(container.querySelector("audio")).toBeInstanceOf(HTMLAudioElement);
    });
    const audio = container.querySelector("audio") as HTMLAudioElement;
    Object.defineProperty(audio, "duration", { configurable: true, value: 120 });
    Object.defineProperty(audio, "currentTime", { configurable: true, value: 30 });
    audio.dispatchEvent(new Event("pause"));

    expect(saveMetadata).not.toHaveBeenCalled();
  });
});
