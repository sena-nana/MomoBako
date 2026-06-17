import { fireEvent, render, screen, waitFor, within } from "@testing-library/vue";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { h } from "vue";
import type { FileBrowserEntry } from "../src/types/repository";

const playlistPlayerMock = vi.hoisted(() => ({
  activeRepoId: null as string | null,
  currentItem: null as { path: string } | null,
  activeFileClass: null as string | null,
  isPlaying: false,
  playEntry: vi.fn(),
  attachVisibleMountTarget: vi.fn(),
  setPlaybackState: vi.fn(),
}));

vi.mock("../src/composables/usePlaylistPlayer", () => ({
  usePlaylistPlayer: () => ({
    activeRepoId: { get value() { return playlistPlayerMock.activeRepoId; } },
    currentItem: { get value() { return playlistPlayerMock.currentItem; } },
    activeFileClass: { get value() { return playlistPlayerMock.activeFileClass; } },
    isPlaying: { get value() { return playlistPlayerMock.isPlaying; } },
    playEntry: playlistPlayerMock.playEntry,
    attachVisibleMountTarget: playlistPlayerMock.attachVisibleMountTarget,
    setPlaybackState: playlistPlayerMock.setPlaybackState,
  }),
}));

import FilePreviewPane from "../src/pages/workspace/preview/FilePreviewPane.vue";

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
  isAudioEntry?: (entry: FileBrowserEntry) => boolean;
  isVideoEntry?: (entry: FileBrowserEntry) => boolean;
} = {}) {
  const entry = options.entry ?? asmrEntry("Voice/RJ123456 Rain Voice/01.mp3", "01 intro");
  const secondEntry = asmrEntry("Voice/RJ123456 Rain Voice/02.mp3", "02 rain");
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
      isVideoEntry: options.isVideoEntry ?? (() => false),
      isAudioEntry: options.isAudioEntry ?? (() => true),
      hardlinkStateLabel: () => "",
      statusLabel: (status: string) => status,
      isSavingMetadata: false,
      availableTags: [],
      tagGroups: [],
      playlistEntries: options.playlistEntries ?? [
        entry,
        secondEntry,
      ],
      libraryExtensions: [
        {
          pluginId: "test.library",
          pluginName: "Test Library",
          libraryKind: "test",
          label: "Test",
          matchEntry: () => true,
          previewPanel: {
            name: "LibraryPreviewPanelStub",
            props: ["entries", "previewEntry"],
            setup(panelProps: { entries: FileBrowserEntry[]; previewEntry: (entry: FileBrowserEntry) => void }) {
              return () => h("section", { "aria-label": "库扩展预览" }, [
                h("button", { type: "button", onClick: () => panelProps.previewEntry(panelProps.entries[1]) }, panelProps.entries[1].metadata?.trackTitle as string),
              ]);
            },
          },
        },
      ],
      thumbnailPalette: () => [],
      saveMetadata: vi.fn(),
    },
  });
}

beforeEach(() => {
  playlistPlayerMock.activeRepoId = null;
  playlistPlayerMock.currentItem = null;
  playlistPlayerMock.activeFileClass = null;
  playlistPlayerMock.isPlaying = false;
  playlistPlayerMock.playEntry.mockReset();
  playlistPlayerMock.attachVisibleMountTarget.mockReset();
  playlistPlayerMock.setPlaybackState.mockReset();
});

describe("FilePreviewPane library extensions", () => {
  it("渲染库扩展预览面板并转发预览回调", async () => {
    const { emitted } = renderPane();

    const previewPanel = screen.getByRole("region", { name: "库扩展预览" });
    expect(previewPanel).toHaveTextContent("02 rain");
    await fireEvent.click(within(previewPanel).getByText("02 rain"));
    expect(emitted("preview")?.[0][0]).toMatchObject({
      path: "Voice/RJ123456 Rain Voice/02.mp3",
    });
  });
});

describe("FilePreviewPane unified media playback", () => {
  it("音频预览进入后直接调用统一播放且不显示本地播放按钮", async () => {
    const entry = asmrEntry("Voice/RJ123456 Rain Voice/01.mp3", "01 intro");
    renderPane({ entry });

    expect(screen.queryByRole("button", { name: "播放" })).not.toBeInTheDocument();
    expect(screen.getByLabelText("当前播放画面")).toBeInTheDocument();
    expect(document.querySelector(".files-preview-page__preview input[type='range']")).toBeNull();
    await waitFor(() => {
      expect(playlistPlayerMock.playEntry).toHaveBeenCalledWith("repo-main-001", entry);
    });
    expect(playlistPlayerMock.playEntry).toHaveBeenCalledTimes(1);
    expect(playlistPlayerMock.attachVisibleMountTarget).toHaveBeenLastCalledWith(expect.any(HTMLElement));
  });

  it("当前条目已在统一播放器中播放时只挂载画面不重复插入", async () => {
    const entry = asmrEntry("Voice/RJ123456 Rain Voice/01.mp3", "01 intro");
    playlistPlayerMock.activeRepoId = "repo-main-001";
    playlistPlayerMock.currentItem = { path: entry.path };

    renderPane({ entry });

    expect(screen.getByLabelText("当前播放画面")).toBeInTheDocument();
    expect(playlistPlayerMock.playEntry).not.toHaveBeenCalled();
    await waitFor(() => {
      expect(playlistPlayerMock.attachVisibleMountTarget).toHaveBeenLastCalledWith(expect.any(HTMLElement));
    });
  });

  it("预览页卸载时只解绑可见挂载点", async () => {
    const { unmount } = renderPane();
    await waitFor(() => {
      expect(playlistPlayerMock.attachVisibleMountTarget).toHaveBeenLastCalledWith(expect.any(HTMLElement));
    });

    unmount();

    expect(playlistPlayerMock.attachVisibleMountTarget).toHaveBeenLastCalledWith(null);
    expect(playlistPlayerMock.setPlaybackState).not.toHaveBeenCalled();
  });

  it("切换到新的音频预览会再次进入统一播放", async () => {
    const firstEntry = asmrEntry("Voice/RJ123456 Rain Voice/01.mp3", "01 intro");
    const secondEntry = asmrEntry("Voice/RJ123456 Rain Voice/02.mp3", "02 rain");
    const view = renderPane({ entry: firstEntry });
    await waitFor(() => {
      expect(playlistPlayerMock.playEntry).toHaveBeenCalledWith("repo-main-001", firstEntry);
    });

    await view.rerender({ entry: secondEntry });

    await waitFor(() => {
      expect(playlistPlayerMock.playEntry).toHaveBeenCalledWith("repo-main-001", secondEntry);
    });
    expect(playlistPlayerMock.playEntry).toHaveBeenCalledTimes(2);
  });
});
