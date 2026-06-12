import { ref } from "vue";
import { convertFileSrc } from "@tauri-apps/api/core";
import type { FileBrowserEntry } from "../../types/repository";
import { getPreviewPluginForEntry } from "../../plugins/previewPlugins";
import { isAudioExtension, isVideoExtension } from "../../utils/filePreviewExtensions";
import { metadataPalette } from "../../utils/fileMetadata";
import { extractPaletteFromImageElement } from "./thumbnailUi";

export function useWorkspacePreviewUi() {
  const failedThumbnailPaths = ref<Set<string>>(new Set());
  const thumbnailAspectRatios = ref<Record<string, number>>({});
  const thumbnailPalettes = ref<Record<string, string[]>>({});

  function isVideoEntry(entry: FileBrowserEntry) {
    return isVideoExtension(entry.extension);
  }

  function isAudioEntry(entry: FileBrowserEntry) {
    return isAudioExtension(entry.extension);
  }

  function isModelEntry(entry: FileBrowserEntry) {
    return Boolean(getPreviewPluginForEntry(entry));
  }

  function thumbnailSrc(entry: FileBrowserEntry) {
    if (!entry.thumbnailPath || failedThumbnailPaths.value.has(entry.path)) return null;
    return convertFileSrc(entry.thumbnailPath);
  }

  function markThumbnailFailed(entry: FileBrowserEntry) {
    failedThumbnailPaths.value = new Set([...failedThumbnailPaths.value, entry.path]);
  }

  function updateThumbnailAspectRatio(entry: FileBrowserEntry, event: Event) {
    const image = event.currentTarget as HTMLImageElement | null;
    if (!image?.naturalWidth || !image.naturalHeight) return;
    const aspectRatio = image.naturalWidth / image.naturalHeight;
    if (!Number.isFinite(aspectRatio) || aspectRatio <= 0) return;
    thumbnailAspectRatios.value = {
      ...thumbnailAspectRatios.value,
      [entry.path]: Math.min(Math.max(aspectRatio, 0.55), 2.4),
    };
    const palette = extractPaletteFromImageElement(image);
    if (palette.length) {
      thumbnailPalettes.value = {
        ...thumbnailPalettes.value,
        [entry.path]: palette,
      };
    }
  }

  function fileItemStyle(entry: FileBrowserEntry) {
    return {
      "--file-thumb-aspect": String(thumbnailAspectRatios.value[entry.path] ?? 1),
    };
  }

  function thumbnailPaletteColors(entry: FileBrowserEntry) {
    const metadataColors = metadataPalette(entry.metadata);
    if (metadataColors.length) return metadataColors;
    return thumbnailPalettes.value[entry.path] ?? [];
  }

  function resetThumbnailFailure(path: string) {
    const next = new Set(failedThumbnailPaths.value);
    next.delete(path);
    failedThumbnailPaths.value = next;
  }

  return {
    fileItemStyle,
    isAudioEntry,
    isModelEntry,
    isVideoEntry,
    markThumbnailFailed,
    resetThumbnailFailure,
    thumbnailPaletteColors,
    thumbnailSrc,
    updateThumbnailAspectRatio,
  };
}
