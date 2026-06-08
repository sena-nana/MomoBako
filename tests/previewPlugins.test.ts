import { describe, expect, it } from "vitest";
import { getPreviewPluginForEntry, listPreviewPlugins } from "../src/plugins/previewPlugins";
import type { FileBrowserEntry } from "../src/types/repository";

function modelEntry(extension: string): FileBrowserEntry {
  return {
    path: `Characters/avatar.${extension}`,
    name: `avatar.${extension}`,
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
    const plugin = getPreviewPluginForEntry(modelEntry("vrm"));

    expect(plugin?.pluginId).toBe("builtin.three-model-preview");
    expect(listPreviewPlugins()[0]?.supportedExtensions).toContain("vrm");
  });
});
