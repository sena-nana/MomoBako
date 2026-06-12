import { fireEvent, render, screen, within } from "@testing-library/vue";
import { describe, expect, it, vi } from "vitest";
import { h } from "vue";
import type { AsmrPlaylistItem } from "../src/pages/workspace/asmrPlaylist";
import FilePreviewPane from "../src/pages/workspace/FilePreviewPane.vue";
import type { FileBrowserEntry } from "../src/types/repository";

function asmrEntry(path: string, trackTitle: string): FileBrowserEntry {
  return {
    path,
    name: path.split("/").at(-1) ?? path,
    kind: "file",
    extension: "mp3",
    sizeBytes: 1024,
    sizeLabel: "1 KB",
    modifiedAt: "2026-06-05T00:18:00Z",
    assetId: path,
    status: "synced",
    metadata: {
      libraryKind: "asmr",
      workId: "RJ123456",
      rjCode: "RJ123456",
      workRoot: "Voice/RJ123456 Rain Voice",
      workTitle: "Rain Voice",
      trackPath: path,
      trackTitle,
      asmrEntryKind: "audio",
      listeningStatus: "listening",
    },
  };
}

function renderPane(options: {
  entry?: FileBrowserEntry;
  playlistEntries?: FileBrowserEntry[];
  asmrPlaylist?: AsmrPlaylistItem[];
} = {}) {
  const entry = options.entry ?? asmrEntry("Voice/RJ123456 Rain Voice/01.mp3", "01 intro");
  return render(FilePreviewPane, {
    props: {
      entry,
      plugin: {
        component: {
          name: "PreviewStub",
          render: () => h("div", "preview"),
        },
      },
      repoId: "repo-main-001",
      thumbnailSrc: () => null,
      isVideoEntry: () => false,
      isAudioEntry: () => true,
      hardlinkStateLabel: () => "",
      statusLabel: (status: string) => status,
      isSavingMetadata: false,
      availableTags: [],
      tagGroups: [],
      playlistEntries: options.playlistEntries ?? [
        entry,
        asmrEntry("Voice/RJ123456 Rain Voice/02.mp3", "02 rain"),
      ],
      asmrPlaylist: options.asmrPlaylist ?? [
        {
          repoId: "repo-main-001",
          path: entry.path,
          title: "01 intro",
          workTitle: "Rain Voice",
          status: "收听中",
        },
      ],
      thumbnailPalette: () => [],
      saveMetadata: vi.fn(),
    },
  });
}

describe("FilePreviewPane ASMR playlist", () => {
  it("显示作品队列和播放列表，并发出播放列表操作事件", async () => {
    const { emitted } = renderPane();

    const workQueue = screen.getByRole("region", { name: "ASMR 作品队列" });
    expect(workQueue).toHaveTextContent("02 rain");
    await fireEvent.click(within(workQueue).getByText("02 rain"));
    expect(emitted("preview")?.[0][0]).toMatchObject({
      path: "Voice/RJ123456 Rain Voice/02.mp3",
    });

    const playlist = screen.getByRole("region", { name: "ASMR 播放列表" });
    expect(playlist).toHaveTextContent("01 intro");
    await fireEvent.click(within(playlist).getByText("加入作品"));
    await fireEvent.click(within(playlist).getByText("随机"));
    await fireEvent.click(within(playlist).getByText("清空"));
    await fireEvent.click(within(playlist).getByText("01 intro"));

    expect(emitted("playlistAddWork")).toHaveLength(1);
    expect(emitted("playlistRandom")).toHaveLength(1);
    expect(emitted("playlistClear")).toHaveLength(1);
    expect(emitted("playlistSelect")?.[0]).toEqual(["Voice/RJ123456 Rain Voice/01.mp3"]);
  });
});
