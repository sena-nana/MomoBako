import { fireEvent, render, screen, waitFor } from "@testing-library/vue";
import { describe, expect, it, vi } from "vitest";
import type { Component } from "vue";
import type { FileBrowserEntry } from "../src/types/repository";

function archiveEntry(): FileBrowserEntry {
  return {
    path: "Books/volume.cbz",
    name: "volume.cbz",
    kind: "file",
    extension: "cbz",
    sizeBytes: 2048,
    sizeLabel: "2 KB",
    modifiedAt: "2026-06-05T00:18:00Z",
    assetId: "asset-archive",
    status: "synced",
    thumbnailPath: null,
    thumbnailCustom: false,
    metadata: {},
  };
}

describe("archive preview plugin", () => {
  it("lists archive directories and previews an internal text file", async () => {
    let component: Component | null = null;
    const callPlugin = vi.fn(async ({ method, payload }: { method: string; payload: Record<string, unknown> }) => {
      if (method === "archive.ensurePrepared") {
        return { payload: { rootPath: "C:/Temp/archive/root" } };
      }
      if (method === "archive.listDirectory") {
        return {
          payload: payload.directoryPath === "chapter"
            ? [
                {
                  path: "chapter/page.txt",
                  name: "page.txt",
                  kind: "file",
                  extension: "txt",
                  sizeBytes: 5,
                  previewable: true,
                },
              ]
            : [
                {
                  path: "chapter",
                  name: "chapter",
                  kind: "directory",
                  previewable: false,
                },
              ],
        };
      }
      if (method === "archive.prepareEntryPreview") {
        return {
          payload: {
            path: payload.entryPath,
            localPath: "C:/Temp/archive/root/chapter/page.txt",
            mediaType: "text/plain",
            sizeBytes: 5,
          },
        };
      }
      throw new Error(`unexpected method: ${method}`);
    });
    vi.stubGlobal("fetch", vi.fn(async () => ({
      ok: true,
      status: 200,
      arrayBuffer: async () => new TextEncoder().encode("hello").buffer,
    })));

    const { register } = await import("../External/Plugins/preview-archive/src/register.js");
    register({
      callPlugin,
      fileSrc: (path: string) => `asset://${path}`,
      preparePreviewFileSource: vi.fn(async () => ({
        repoId: "repo-main-001",
        path: "Books/volume.cbz",
        token: "0".repeat(64),
        sourceUrl: "http://127.0.0.1:49152/preview/archive",
        localPath: "C:/Books/volume.cbz",
        mediaType: "application/octet-stream",
        sizeBytes: 2048,
      })),
      registerPreview: ({ component: nextComponent }: { component: Component }) => {
        component = nextComponent;
        return {} as never;
      },
      vue: await import("vue"),
    });

    expect(component).not.toBeNull();
    render(component!, {
      props: {
        entry: archiveEntry(),
        repoId: "repo-main-001",
      },
    });

    const chapter = await screen.findByRole("button", { name: /chapter/ });
    await fireEvent.click(chapter);
    expect(screen.queryByRole("button", { name: /page\.txt/ })).not.toBeInTheDocument();
    await fireEvent.dblClick(chapter);
    const page = await screen.findByRole("button", { name: /page\.txt/ });
    await fireEvent.click(page);

    await waitFor(() => {
      expect(screen.getByText("hello")).toBeInTheDocument();
    });
    expect(callPlugin).toHaveBeenCalledWith(expect.objectContaining({
      method: "archive.listDirectory",
      payload: expect.objectContaining({ archivePath: "C:/Books/volume.cbz" }),
    }));
    expect(callPlugin).toHaveBeenCalledWith(expect.objectContaining({
      method: "archive.prepareEntryPreview",
      payload: expect.objectContaining({ entryPath: "chapter/page.txt" }),
    }));
  });
});
