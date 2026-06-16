import { afterEach, describe, expect, it } from "vitest";
import { getPreviewPluginForEntry, listPreviewPlugins } from "../src/plugins/previewPlugins";
import { getPlaylistPlayerByType, listPlaylistPlayers } from "../src/plugins/playlistPlayers";
import { clearPreviewPluginRegistry, syncRegisteredPreviewPluginManifests } from "../src/plugins/sdk";
import { listPlugins } from "../src/services/repositoryApi";
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
    clearPreviewPluginRegistry();
  });

  it("routes VRM files to the built-in 3D model preview", async () => {
    await syncRegisteredPreviewPluginManifests(await listPlugins());
    const plugin = getPreviewPluginForEntry(fileEntry("vrm"));

    expect(plugin?.pluginId).toBe("momobako.preview.three-model");
    expect(listPreviewPlugins().some((item) => item.supportedExtensions.includes("vrm"))).toBe(true);
  });

  it("routes STL, 3MF, and BLEND files to the built-in 3D model preview", async () => {
    await syncRegisteredPreviewPluginManifests(await listPlugins());
    expect(getPreviewPluginForEntry(fileEntry("stl"))?.pluginId).toBe("momobako.preview.three-model");
    expect(getPreviewPluginForEntry(fileEntry("3mf"))?.pluginId).toBe("momobako.preview.three-model");
    expect(getPreviewPluginForEntry(fileEntry("blend"))?.pluginId).toBe("momobako.preview.three-model");
    const modelPlugin = listPreviewPlugins().find((plugin) => plugin.pluginId === "momobako.preview.three-model");
    expect(modelPlugin?.supportedExtensions).toContain("stl");
    expect(modelPlugin?.supportedExtensions).toContain("3mf");
    expect(modelPlugin?.supportedExtensions).toContain("blend");
  });

  it("routes image, video, and audio files to the built-in media preview", async () => {
    await syncRegisteredPreviewPluginManifests(await listPlugins());
    expect(getPreviewPluginForEntry(fileEntry("png"))?.pluginId).toBe("momobako.preview.media");
    expect(getPreviewPluginForEntry(fileEntry("mp4"))?.pluginId).toBe("momobako.preview.media");
    expect(getPreviewPluginForEntry(fileEntry("mp3"))?.pluginId).toBe("momobako.preview.media");
    const mediaPlugin = listPreviewPlugins().find((plugin) => plugin.pluginId === "momobako.preview.media");
    expect(mediaPlugin?.supportedExtensions).toContain("png");
    expect(mediaPlugin?.supportedExtensions).toContain("webm");
    expect(mediaPlugin?.supportedExtensions).toContain("wav");
  });

  it("registers built-in playlist player types from the media plugin manifest", async () => {
    await syncRegisteredPreviewPluginManifests(await listPlugins());

    expect(getPlaylistPlayerByType("momobako.playlist.image-slideshow")?.fileClass).toBe("image");
    expect(getPlaylistPlayerByType("momobako.playlist.audio-sequence")?.fileClass).toBe("audio");
    expect(getPlaylistPlayerByType("momobako.playlist.video-sequence")?.fileClass).toBe("video");
    expect(listPlaylistPlayers()).toHaveLength(3);
  });

  it("skips disabled media playlist players after manifest sync", async () => {
    await syncRegisteredPreviewPluginManifests(await listPlugins());
    const manifests = listPreviewPlugins().map((plugin) => (
      plugin.pluginId === "momobako.preview.media"
        ? {
            ...plugin.manifest!,
            enabled: false,
            status: "disabled" as const,
          }
        : plugin.manifest!
    ));

    await syncRegisteredPreviewPluginManifests(manifests);

    expect(getPlaylistPlayerByType("momobako.playlist.image-slideshow")).toBeNull();
    expect(getPlaylistPlayerByType("momobako.playlist.audio-sequence")).toBeNull();
    expect(getPlaylistPlayerByType("momobako.playlist.video-sequence")).toBeNull();
    expect(listPlaylistPlayers()).toHaveLength(0);
  });

  it("routes text and markdown files to the built-in text preview", async () => {
    await syncRegisteredPreviewPluginManifests(await listPlugins());
    expect(getPreviewPluginForEntry(fileEntry("txt"))?.pluginId).toBe("momobako.preview.text");
    expect(getPreviewPluginForEntry(fileEntry("md"))?.pluginId).toBe("momobako.preview.text");
    const textPlugin = listPreviewPlugins().find((plugin) => plugin.pluginId === "momobako.preview.text");
    expect(textPlugin?.supportedExtensions).toContain("markdown");
    expect(textPlugin?.generateThumbnail).toBeTypeOf("function");
  });

  it("routes Office and PDF files to the built-in document preview", async () => {
    await syncRegisteredPreviewPluginManifests(await listPlugins());
    expect(getPreviewPluginForEntry(fileEntry("pdf"))?.pluginId).toBe("momobako.preview.office");
    expect(getPreviewPluginForEntry(fileEntry("docx"))?.pluginId).toBe("momobako.preview.office");
    expect(getPreviewPluginForEntry(fileEntry("xlsx"))?.pluginId).toBe("momobako.preview.office");
    expect(getPreviewPluginForEntry(fileEntry("pptx"))?.pluginId).toBe("momobako.preview.office");
    expect(getPreviewPluginForEntry(fileEntry("docm"))?.pluginId).toBe("momobako.preview.office");
    expect(getPreviewPluginForEntry(fileEntry("xlsm"))?.pluginId).toBe("momobako.preview.office");
    expect(getPreviewPluginForEntry(fileEntry("pptm"))?.pluginId).toBe("momobako.preview.office");
    expect(getPreviewPluginForEntry(fileEntry("csv"))?.pluginId).toBe("momobako.preview.text");
    const officePlugin = listPreviewPlugins().find((plugin) => plugin.pluginId === "momobako.preview.office");
    expect(officePlugin?.supportedExtensions).toContain("doc");
    expect(officePlugin?.supportedExtensions).toContain("ppt");
    expect(officePlugin?.generateThumbnail).toBeTypeOf("function");
  });

  it("routes archive files to the archive preview plugin", async () => {
    await syncRegisteredPreviewPluginManifests(await listPlugins());
    expect(getPreviewPluginForEntry(fileEntry("zip"))?.pluginId).toBe("momobako.preview.archive");
    expect(getPreviewPluginForEntry(fileEntry("cbz"))?.pluginId).toBe("momobako.preview.archive");
    expect(getPreviewPluginForEntry(fileEntry("7z"))?.pluginId).toBe("momobako.preview.archive");
    expect(getPreviewPluginForEntry(fileEntry("rar"))?.pluginId).toBe("momobako.preview.archive");
    expect(getPreviewPluginForEntry(fileEntry("cbr"))?.pluginId).toBe("momobako.preview.archive");
  });

  it("exposes office preview extensions through plugin manifests instead of host-side static imports", async () => {
    await syncRegisteredPreviewPluginManifests(await listPlugins());
    const officePlugin = listPreviewPlugins().find((plugin) => plugin.pluginId === "momobako.preview.office");

    expect(officePlugin).toBeDefined();
    expect(officePlugin?.supportedExtensions).toContain("pdf");
    expect(officePlugin?.supportedExtensions).toContain("docx");
    expect(officePlugin?.supportedExtensions).toContain("xlsx");
    expect(officePlugin?.supportedExtensions).toContain("pptx");
    expect(officePlugin?.supportedExtensions).toContain("docm");
    expect(officePlugin?.supportedExtensions).toContain("xlsm");
    expect(officePlugin?.supportedExtensions).toContain("pptm");
  });

  it("skips disabled preview plugins after manifest sync", async () => {
    await syncRegisteredPreviewPluginManifests(await listPlugins());
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

    await syncRegisteredPreviewPluginManifests(manifests);

    expect(getPreviewPluginForEntry(fileEntry("mp4"))).toBeNull();
    expect(getPreviewPluginForEntry(fileEntry("mp3"))).toBeNull();
    expect(getPreviewPluginForEntry(fileEntry("vrm"))?.pluginId).toBe("momobako.preview.three-model");
  });
});
