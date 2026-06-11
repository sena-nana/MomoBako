import { ensureThumbnail } from "../../services/repositoryApi";
import { getPreviewPluginForEntry } from "../../plugins/previewPlugins";
import type { FileBrowserEntry, FileBrowserSnapshot, ThumbnailResponse } from "../../types/repository";
import { activeRepoId, currentDirectoryPath, error, fileBrowser } from "./state";

const THUMBNAIL_LOAD_CONCURRENCY = 3;

export type ThumbnailQueueItem = {
  repoId: string;
  directoryPath: string;
  entry: FileBrowserEntry;
};

let thumbnailDirectoryToken = 0;

export function invalidateThumbnailQueue() {
  thumbnailDirectoryToken += 1;
}

export function loadThumbnailsForSnapshot(snapshot: FileBrowserSnapshot) {
  if (snapshot.specialLocation === "trash") return;
  const token = ++thumbnailDirectoryToken;
  const queue: ThumbnailQueueItem[] = snapshot.entries
    .filter((entry) => entry.kind === "file" && !entry.thumbnailPath)
    .map((entry) => ({
      repoId: snapshot.repoId,
      directoryPath: snapshot.currentPath,
      entry,
    }));
  let cursor = 0;

  async function worker() {
    while (cursor < queue.length) {
      const item = queue[cursor++];
      await loadThumbnailForQueueItem(item, token);
    }
  }

  void Promise.all(
    Array.from({ length: Math.min(THUMBNAIL_LOAD_CONCURRENCY, queue.length) }, () => worker()),
  );
}

async function loadThumbnailForQueueItem(item: ThumbnailQueueItem, token: number) {
  try {
    const response = await ensureThumbnail({
      repoId: item.repoId,
      path: item.entry.path,
      action: "ensure",
    });
    if (!response.thumbnailPath) {
      await loadGeneratedThumbnailForQueueItem(item, token);
      return;
    }
    if (token !== thumbnailDirectoryToken) return;
    applyThumbnailResponse(response, item.directoryPath);
  } catch {
    await loadGeneratedThumbnailForQueueItem(item, token);
  }
}

async function loadGeneratedThumbnailForQueueItem(item: ThumbnailQueueItem, token: number) {
  const generator = getPreviewPluginForEntry(item.entry)?.generateThumbnail;
  if (!generator) return;
  try {
    const thumbnail = await generator({
      repoId: item.repoId,
      entry: item.entry,
    });
    if (!thumbnail || token !== thumbnailDirectoryToken) return;
    const response = await ensureThumbnail({
      repoId: item.repoId,
      path: item.entry.path,
      action: "saveGenerated",
      imageBytes: thumbnail.bytes,
      mediaType: thumbnail.mediaType,
    });
    if (!response.thumbnailPath || token !== thumbnailDirectoryToken) return;
    applyThumbnailResponse(response, item.directoryPath);
  } catch {
    return;
  }
}

export function applyThumbnailResponse(response: ThumbnailResponse, expectedDirectoryPath = currentDirectoryPath.value) {
  const current = fileBrowser.value;
  if (!current || current.repoId !== response.repoId || current.currentPath !== expectedDirectoryPath) return;
  if (!current.entries.some((item) => item.path === response.path && item.kind === response.kind)) return;

  fileBrowser.value = {
    ...current,
    entries: current.entries.map((item) => (
      item.path === response.path && item.kind === response.kind
        ? {
            ...item,
            assetId: response.assetId || item.assetId,
            thumbnailPath: response.thumbnailPath ?? null,
            thumbnailCustom: response.thumbnailCustom,
            metadata: response.metadata ?? item.metadata,
          }
        : item
    )),
  };
}

type WorkspaceThumbnailSaveRequest =
  | {
      action: "save";
      sourcePath: string;
    }
  | {
      action: "save" | "saveGenerated";
      imageBytes: number[];
      mediaType?: string;
    }
  | {
      action: "clear" | "refresh";
    };

async function mutateWorkspaceEntryThumbnail(path: string, request: WorkspaceThumbnailSaveRequest, reportError = true) {
  if (!activeRepoId.value || fileBrowser.value?.specialLocation === "trash") return null;
  error.value = null;
  try {
    const response = await ensureThumbnail({
      repoId: activeRepoId.value,
      path,
      ...request,
    });
    applyThumbnailResponse(response);
    return response;
  } catch (cause) {
    if (reportError) {
      error.value = cause instanceof Error ? cause.message : String(cause);
    }
    return null;
  }
}

export function setWorkspaceEntryThumbnail(path: string, sourcePath: string) {
  return mutateWorkspaceEntryThumbnail(path, {
    action: "save",
    sourcePath,
  });
}

export function setWorkspaceEntryThumbnailFromBytes(path: string, imageBytes: number[], mediaType?: string) {
  return mutateWorkspaceEntryThumbnail(path, {
    action: "save",
    imageBytes,
    mediaType,
  });
}

export function saveGeneratedWorkspaceEntryThumbnail(path: string, imageBytes: number[], mediaType?: string) {
  return mutateWorkspaceEntryThumbnail(path, {
    action: "saveGenerated",
    imageBytes,
    mediaType,
  }, false);
}

export function clearWorkspaceEntryThumbnail(path: string) {
  return mutateWorkspaceEntryThumbnail(path, { action: "clear" });
}

export function refreshWorkspaceEntryThumbnail(path: string) {
  return mutateWorkspaceEntryThumbnail(path, { action: "refresh" });
}
