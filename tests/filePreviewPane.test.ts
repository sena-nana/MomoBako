import { fireEvent, render, screen, within } from "@testing-library/vue";
import { describe, expect, it, vi } from "vitest";
import { h } from "vue";
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
      isVideoEntry: () => false,
      isAudioEntry: () => true,
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
