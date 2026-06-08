import { describe, expect, it } from "vitest";
import { getPreviewPluginForEntry, listPreviewPlugins } from "../src/plugins/previewPlugins";
import type { FileBrowserEntry } from "../src/types/repository";

function fileEntry(extension: string): FileBrowserEntry {
  return {
    path: `Characters/asset.${extension}`,
    name: `asset.${extension}`,
    kind: "file",
    extension,
    sizeBytes: 1024,
    sizeLabel: "1 KB",
    modifiedAt: "2026-06-05T00:18:00Z",
    assetId: "asset-vrm",
    status: "synced",
    thumbnailPath: null,
    thumbnailCustom: false,
    metadata: {},
  };
}

describe("previewPlugins", () => {
  it("routes VRM files to the built-in 3D model preview", () => {
    const plugin = getPreviewPluginForEntry(fileEntry("vrm"));

    expect(plugin?.pluginId).toBe("builtin.three-model-preview");
    expect(listPreviewPlugins().some((item) => item.supportedExtensions.includes("vrm"))).toBe(true);
  });

  it("routes video and audio files to the built-in media preview", () => {
    expect(getPreviewPluginForEntry(fileEntry("mp4"))?.pluginId).toBe("builtin.media-preview");
    expect(getPreviewPluginForEntry(fileEntry("mp3"))?.pluginId).toBe("builtin.media-preview");
    const mediaPlugin = listPreviewPlugins().find((plugin) => plugin.pluginId === "builtin.media-preview");
    expect(mediaPlugin?.supportedExtensions).toContain("webm");
    expect(mediaPlugin?.supportedExtensions).toContain("wav");
  });
});
