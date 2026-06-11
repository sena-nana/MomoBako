import { afterEach, describe, expect, it } from "vitest";
import {
  isPptxPreviewExtension,
  isVueOfficePreviewExtension,
} from "../src/plugins/officePreview/officeExtensions";
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

  it("routes STL, 3MF, and BLEND files to the built-in 3D model preview", () => {
    expect(getPreviewPluginForEntry(fileEntry("stl"))?.pluginId).toBe("momobako.preview.three-model");
    expect(getPreviewPluginForEntry(fileEntry("3mf"))?.pluginId).toBe("momobako.preview.three-model");
    expect(getPreviewPluginForEntry(fileEntry("blend"))?.pluginId).toBe("momobako.preview.three-model");
    const modelPlugin = listPreviewPlugins().find((plugin) => plugin.pluginId === "momobako.preview.three-model");
    expect(modelPlugin?.supportedExtensions).toContain("stl");
    expect(modelPlugin?.supportedExtensions).toContain("3mf");
    expect(modelPlugin?.supportedExtensions).toContain("blend");
  });

  it("routes image, video, and audio files to the built-in media preview", () => {
    expect(getPreviewPluginForEntry(fileEntry("png"))?.pluginId).toBe("momobako.preview.media");
    expect(getPreviewPluginForEntry(fileEntry("mp4"))?.pluginId).toBe("momobako.preview.media");
    expect(getPreviewPluginForEntry(fileEntry("mp3"))?.pluginId).toBe("momobako.preview.media");
    const mediaPlugin = listPreviewPlugins().find((plugin) => plugin.pluginId === "momobako.preview.media");
    expect(mediaPlugin?.supportedExtensions).toContain("png");
    expect(mediaPlugin?.supportedExtensions).toContain("webm");
    expect(mediaPlugin?.supportedExtensions).toContain("wav");
  });

  it("routes text and markdown files to the built-in text preview", () => {
    expect(getPreviewPluginForEntry(fileEntry("txt"))?.pluginId).toBe("momobako.preview.text");
    expect(getPreviewPluginForEntry(fileEntry("md"))?.pluginId).toBe("momobako.preview.text");
    const textPlugin = listPreviewPlugins().find((plugin) => plugin.pluginId === "momobako.preview.text");
    expect(textPlugin?.supportedExtensions).toContain("markdown");
    expect(textPlugin?.generateThumbnail).toBeTypeOf("function");
  });

  it("routes Office and PDF files to the built-in document preview", () => {
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

  it("uses vue-office only for docx, xlsx, and pdf document previews", () => {
    expect(isVueOfficePreviewExtension("docx")).toBe(true);
    expect(isVueOfficePreviewExtension("xlsx")).toBe(true);
    expect(isVueOfficePreviewExtension("pdf")).toBe(true);
    expect(isVueOfficePreviewExtension("docm")).toBe(false);
    expect(isVueOfficePreviewExtension("xlsm")).toBe(false);
    expect(isVueOfficePreviewExtension("pptx")).toBe(false);
  });

  it("uses pptx-preview only for pptx presentation previews", () => {
    expect(isPptxPreviewExtension("pptx")).toBe(true);
    expect(isPptxPreviewExtension("pptm")).toBe(false);
    expect(isPptxPreviewExtension("ppsx")).toBe(false);
    expect(isPptxPreviewExtension("docx")).toBe(false);
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
