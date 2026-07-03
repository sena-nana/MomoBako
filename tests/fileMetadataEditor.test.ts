import { fireEvent, render, screen, within } from "@testing-library/vue";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { h, computed, reactive, ref, watch } from "vue";
import FileMetadataEditor from "../src/pages/workspace/files/FileMetadataEditor.vue";
import type { RegisteredLibraryExtension } from "../src/plugins/sdk";
import type { FileBrowserEntry } from "../src/types/repository";
import { callPlugin } from "../src/services/repositoryApi";
import { register as registerAsmrLibrary } from "../External/Plugins/library-asmr/src/register";
import { getOpenerCalls } from "./setupTests";

const writeText = vi.fn<[(text: string) => Promise<void>]>();

function createEntry(metadata: Record<string, unknown>): FileBrowserEntry {
  return {
    path: "References/imported-image.png",
    name: "imported-image.png",
    kind: "file",
    extension: "png",
    sizeBytes: 4096,
    sizeLabel: "4 KB",
    modifiedAt: "2026-06-05T00:18:00Z",
    assetId: "asset-source-01",
    status: "synced",
    tags: [],
    metadata,
  };
}

function asmrLibraryExtension(): RegisteredLibraryExtension {
  let extension: RegisteredLibraryExtension | null = null;
  registerAsmrLibrary({
    vue: { h, computed, reactive, ref, watch },
    callPlugin,
    onPluginEvent: vi.fn(() => vi.fn()),
    registerLibraryExtension(definition: RegisteredLibraryExtension) {
      extension = {
        ...definition,
        pluginId: "momobako.library.asmr",
        pluginName: "ASMR Library",
      };
      return extension;
    },
  });
  if (!extension) throw new Error("ASMR library extension was not registered");
  return extension;
}

function renderEditor(entry: FileBrowserEntry, saveMetadata = vi.fn(), saveCoverThumbnail = vi.fn()) {
  return render(FileMetadataEditor, {
    props: {
      entry,
      isSaving: false,
      availableTags: [],
      tagGroups: [],
      repoId: "repo-main-001",
      playlistEntries: [entry],
      libraryExtensions: [asmrLibraryExtension()],
      saveMetadata,
      saveCoverThumbnail,
    },
  });
}

describe("FileMetadataEditor", () => {
  beforeEach(() => {
    writeText.mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });
  });

  it("显示来源 metadata 并支持打开和复制链接", async () => {
    renderEditor(createEntry({
      originTitle: "Reference Board Item",
      sourceUrl: "https://example.test/assets/imported-image.png",
      originReferrer: "momoapp://library/reference-board",
    }));

    const sourceTitleRow = screen.getByText("来源标题").closest(".asset-meta__row");
    const sourceUrlRow = screen.getByText("来源链接").closest(".asset-meta__row");
    expect(sourceTitleRow).not.toBeNull();
    expect(sourceUrlRow).not.toBeNull();
    expect(sourceTitleRow).toHaveTextContent("Reference Board Item");
    expect(sourceUrlRow).toHaveTextContent("https://example.test/assets/imported-image.png");
    expect(screen.queryByText("momoapp://library/reference-board")).toBeNull();

    const sourceUrlButtons = within(sourceUrlRow as HTMLElement).getAllByRole("button");
    await fireEvent.click(sourceUrlButtons[0]);
    await fireEvent.click(sourceUrlButtons[1]);

    expect(getOpenerCalls("openUrl").at(-1)).toMatchObject({
      path: "https://example.test/assets/imported-image.png",
    });
    expect(writeText).toHaveBeenCalledWith("https://example.test/assets/imported-image.png");
  });

  it("将注释和链接编辑区排在技术 metadata 之前", () => {
    const { container } = renderEditor(createEntry({
      comment: "整理来源和待办",
      link: "https://example.test/task/imported-image",
      addedToLibraryAt: "2026-06-05T00:18:00Z",
    }));

    const commentRow = screen.getByText("注释").closest(".asset-meta__row");
    const linkRow = screen.getByText("链接").closest(".asset-meta__row");
    const addedRow = screen.getByText("添加到资源库").closest(".asset-meta__row");

    expect(commentRow).toBeInstanceOf(HTMLElement);
    expect(linkRow).toBeInstanceOf(HTMLElement);
    expect(addedRow).toBeInstanceOf(HTMLElement);
    expect(container.querySelector(".file-metadata-card")).toContainElement(commentRow as HTMLElement);
    expect((commentRow as HTMLElement).compareDocumentPosition(addedRow as HTMLElement)).toBe(Node.DOCUMENT_POSITION_FOLLOWING);
    expect((linkRow as HTMLElement).compareDocumentPosition(addedRow as HTMLElement)).toBe(Node.DOCUMENT_POSITION_FOLLOWING);
  });

  it("显示 ASMR 作品 metadata 专用区", () => {
    renderEditor(createEntry({
      libraryKind: "asmr",
      workId: "RJ123456",
      rjCode: "RJ123456",
      workTitle: "Rain Voice",
      workRoot: "Voice/RJ123456 Rain Voice",
      trackTitle: "01 intro.mp3",
      circle: "Blue Circle",
      voiceActors: ["Aoi", "Momo"],
      scenarioTags: ["睡眠", "耳语"],
      lyricStatus: "local",
      listeningStatus: "listening",
      listeningProgress: 42,
      trackDurationMs: 180000,
      rateAverage: 4.8,
      reviewCount: 12,
    }));

    const asmrRegion = screen.getByRole("region", { name: "ASMR 信息" });
    expect(asmrRegion).toHaveTextContent("RJ123456");
    expect(asmrRegion).toHaveTextContent("Rain Voice");
    expect(asmrRegion).toHaveTextContent("Blue Circle");
    expect(asmrRegion).toHaveTextContent("Aoi，Momo");
    expect(asmrRegion).toHaveTextContent("收听中 · 42%");
    expect(asmrRegion).toHaveTextContent("3:00");
    expect(asmrRegion).toHaveTextContent("4.8");
  });

  it("显示并应用 ASMR provider 候选，同时跳过人工字段", async () => {
    const saveMetadata = vi.fn().mockResolvedValue(undefined);
    renderEditor(createEntry({
      libraryKind: "asmr",
      workId: "RJ123456",
      workTitle: "Rain Voice",
      providerCandidates: [
        {
          source: "dlsite",
          confidence: "external-id",
          fields: {
            workTitle: "Rain Voice Deluxe",
            circle: "Blue Circle",
            voiceActors: ["Aoi", "Momo"],
            rating: 5,
            comment: "manual note",
            listeningStatus: "listened",
          },
        },
      ],
    }), saveMetadata);

    const candidateRegion = screen.getByRole("region", { name: "ASMR 元数据候选" });
    expect(candidateRegion).toHaveTextContent("dlsite");
    expect(candidateRegion).toHaveTextContent("workTitle=Rain Voice Deluxe");
    expect(candidateRegion).toHaveTextContent("voiceActors=Aoi，Momo");
    expect(candidateRegion).toHaveTextContent("跳过 rating，comment，listeningStatus");

    await fireEvent.click(within(candidateRegion).getByText("应用"));

    expect(saveMetadata).toHaveBeenCalledWith(expect.objectContaining({
      path: "References/imported-image.png",
    }), {
      workTitle: "Rain Voice Deluxe",
      circle: "Blue Circle",
      voiceActors: ["Aoi", "Momo"],
    });
  });

  it("支持将 ASMR provider 候选封面保存为条目缩略图", async () => {
    const saveMetadata = vi.fn().mockResolvedValue(undefined);
    const saveCoverThumbnail = vi.fn().mockResolvedValue(undefined);
    renderEditor(createEntry({
      libraryKind: "asmr",
      workId: "RJ123456",
      workTitle: "Rain Voice",
      providerCandidates: [
        {
          source: "dlsite",
          confidence: "external-id",
          fields: {
            workTitle: "Rain Voice Deluxe",
            coverUrl: "https://img.example.test/RJ123456.jpg",
          },
        },
      ],
    }), saveMetadata, saveCoverThumbnail);

    const candidateRegion = screen.getByRole("region", { name: "ASMR 元数据候选" });
    await fireEvent.click(within(candidateRegion).getByText("封面"));

    expect(saveCoverThumbnail).toHaveBeenCalledWith(
      "References/imported-image.png",
      "https://img.example.test/RJ123456.jpg",
    );
    expect(saveMetadata).not.toHaveBeenCalled();
  });

  it("支持手动导入 ASMR provider 候选 JSON", async () => {
    const saveMetadata = vi.fn().mockResolvedValue(undefined);
    renderEditor(createEntry({
      libraryKind: "asmr",
      workId: "RJ123456",
      workTitle: "Rain Voice",
    }), saveMetadata);

    const candidateRegion = screen.getByRole("region", { name: "ASMR 元数据候选" });
    await fireEvent.click(within(candidateRegion).getByText("导入"));
    await fireEvent.update(within(candidateRegion).getByLabelText("ASMR 候选 JSON"), JSON.stringify({
      source: "dlsite",
      confidence: "manual",
      fields: {
        workTitle: "Rain Voice Deluxe",
        circle: "Blue Circle",
      },
    }));
    await fireEvent.click(within(candidateRegion).getByText("导入候选"));

    expect(saveMetadata).toHaveBeenCalledWith(expect.objectContaining({
      path: "References/imported-image.png",
    }), {
      providerCandidates: [
        {
          source: "dlsite",
          confidence: "manual",
          fields: {
            workTitle: "Rain Voice Deluxe",
            circle: "Blue Circle",
          },
        },
      ],
    });
  });

  it("支持通过后端 provider 抓取 ASMR 候选", async () => {
    const saveMetadata = vi.fn().mockResolvedValue(undefined);
    renderEditor(createEntry({
      libraryKind: "asmr",
      workId: "RJ123456",
      rjCode: "RJ123456",
      workTitle: "Rain Voice",
    }), saveMetadata);

    const candidateRegion = screen.getByRole("region", { name: "ASMR 元数据候选" });
    await fireEvent.click(within(candidateRegion).getByText("导入"));
    await fireEvent.click(within(candidateRegion).getByText("抓取候选"));

    expect(saveMetadata).toHaveBeenCalledWith(expect.objectContaining({
      path: "References/imported-image.png",
    }), {
      providerCandidates: [
        {
          source: "dlsite",
          confidence: "external-id",
          fields: {
            workId: "RJ123456",
            rjCode: "RJ123456",
            workTitle: "Fetched Rain Voice",
            circle: "Fetched Circle",
          },
        },
      ],
    });
  });
});
