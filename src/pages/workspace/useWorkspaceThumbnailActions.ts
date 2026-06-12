import { open as openDialog } from "@tauri-apps/plugin-dialog";
import type { ComputedRef } from "vue";
import type { FileBrowserEntry } from "../../types/repository";

type ThumbnailMutationResponse = Promise<{ thumbnailPath?: string | null } | null>;

type WorkspaceThumbnailActionsOptions = {
  isTrashPanel: ComputedRef<boolean>;
  clearWorkspaceEntryThumbnail: (path: string) => ThumbnailMutationResponse;
  refreshWorkspaceEntryThumbnail: (path: string) => ThumbnailMutationResponse;
  resetThumbnailFailure: (path: string) => void;
  setWorkspaceEntryThumbnail: (path: string, thumbnailPath: string) => ThumbnailMutationResponse;
  setWorkspaceEntryThumbnailFromBytes: (path: string, bytes: number[], mediaType: string) => ThumbnailMutationResponse;
};

async function readClipboardImageBytes() {
  const items = await navigator.clipboard?.read?.();
  if (!items?.length) return null;

  for (const item of items) {
    const type = item.types.find((value) => value.startsWith("image/"));
    if (!type) continue;
    const blob = await item.getType(type);
    return {
      mediaType: type,
      bytes: Array.from(new Uint8Array(await blob.arrayBuffer())),
    };
  }

  return null;
}

export function useWorkspaceThumbnailActions(options: WorkspaceThumbnailActionsOptions) {
  async function chooseCustomThumbnail(entry: FileBrowserEntry) {
    if (options.isTrashPanel.value) return;
    const selected = await openDialog({
      title: "选择自定义缩略图",
      multiple: false,
      filters: [
        {
          name: "图片",
          extensions: ["png", "jpg", "jpeg", "webp", "bmp"],
        },
      ],
    });
    if (typeof selected !== "string") return;
    const response = await options.setWorkspaceEntryThumbnail(entry.path, selected);
    if (response?.thumbnailPath) options.resetThumbnailFailure(entry.path);
  }

  async function pasteCustomThumbnail(entry: FileBrowserEntry) {
    if (options.isTrashPanel.value) return;
    try {
      const image = await readClipboardImageBytes();
      if (!image) return;
      const response = await options.setWorkspaceEntryThumbnailFromBytes(entry.path, image.bytes, image.mediaType);
      if (response?.thumbnailPath) options.resetThumbnailFailure(entry.path);
    } catch {
      return;
    }
  }

  async function clearCustomThumbnail(entry: FileBrowserEntry) {
    if (options.isTrashPanel.value) return;
    const response = await options.clearWorkspaceEntryThumbnail(entry.path);
    options.resetThumbnailFailure(entry.path);
    if (!response?.thumbnailPath && entry.kind === "file") {
      await options.refreshWorkspaceEntryThumbnail(entry.path);
    }
  }

  async function refreshEntryThumbnail(entry: FileBrowserEntry) {
    if (options.isTrashPanel.value) return;
    const response = await options.refreshWorkspaceEntryThumbnail(entry.path);
    if (response?.thumbnailPath) options.resetThumbnailFailure(entry.path);
  }

  return {
    chooseCustomThumbnail,
    pasteCustomThumbnail,
    clearCustomThumbnail,
    refreshEntryThumbnail,
  };
}
