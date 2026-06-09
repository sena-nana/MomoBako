import { afterEach, describe, expect, it } from "vitest";
import { getPreviewPluginForEntry, listPreviewPlugins } from "../src/plugins/previewPlugins";
import { syncRegisteredPreviewPluginManifests } from "../src/plugins/sdk";
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
  afterEach(() => {
    syncRegisteredPreviewPluginManifests([]);
  });

  it("routes VRM files to the built-in 3D model preview", () => {
    const plugin = getPreviewPluginForEntry(fileEntry("vrm"));

    expect(plugin?.pluginId).toBe("momobako.preview.three-model");
    expect(listPreviewPlugins().some((item) => item.supportedExtensions.includes("vrm"))).toBe(true);
  });

  it("routes video and audio files to the built-in media preview", () => {
    expect(getPreviewPluginForEntry(fileEntry("mp4"))?.pluginId).toBe("momobako.preview.media");
    expect(getPreviewPluginForEntry(fileEntry("mp3"))?.pluginId).toBe("momobako.preview.media");
    const mediaPlugin = listPreviewPlugins().find((plugin) => plugin.pluginId === "momobako.preview.media");
    expect(mediaPlugin?.supportedExtensions).toContain("webm");
    expect(mediaPlugin?.supportedExtensions).toContain("wav");
  });

  it("skips disabled preview plugins after manifest sync", () => {
    const manifests = listPreviewPlugins().map((plugin) => (
      plugin.pluginId === "momobako.preview.media"
        ? {
            ...(plugin.manifest ?? {
              pluginId: plugin.pluginId,
              name: plugin.name,
            }),
            enabled: false,
            status: "disabled" as const,
          }
        : plugin.manifest!
    ));

    syncRegisteredPreviewPluginManifests(manifests);

    expect(getPreviewPluginForEntry(fileEntry("mp4"))).toBeNull();
    expect(getPreviewPluginForEntry(fileEntry("mp3"))).toBeNull();
    expect(getPreviewPluginForEntry(fileEntry("vrm"))?.pluginId).toBe("momobako.preview.three-model");
  });
});
