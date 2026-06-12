import { fireEvent, render, screen, within } from "@testing-library/vue";
import { beforeEach, describe, expect, it, vi } from "vitest";
import FileMetadataEditor from "../src/pages/workspace/FileMetadataEditor.vue";
import type { FileBrowserEntry } from "../src/types/repository";
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

function renderEditor(entry: FileBrowserEntry) {
  return render(FileMetadataEditor, {
    props: {
      entry,
      isSaving: false,
      availableTags: [],
      tagGroups: [],
      saveMetadata: vi.fn(),
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

    const sourceRegion = screen.getByRole("region", { name: "来源信息" });
    expect(sourceRegion).toHaveTextContent("Reference Board Item");
    expect(sourceRegion).toHaveTextContent("https://example.test/assets/imported-image.png");
    expect(sourceRegion).toHaveTextContent("momoapp://library/reference-board");

    const sourceUrlRow = within(sourceRegion).getByText("原始链接").closest(".asset-meta__row");
    expect(sourceUrlRow).not.toBeNull();
    const sourceUrlButtons = within(sourceUrlRow as HTMLElement).getAllByRole("button");
    await fireEvent.click(sourceUrlButtons[0]);
    await fireEvent.click(sourceUrlButtons[1]);

    expect(getOpenerCalls("openUrl").at(-1)).toMatchObject({
      path: "https://example.test/assets/imported-image.png",
    });
    expect(writeText).toHaveBeenCalledWith("https://example.test/assets/imported-image.png");
  });
});
