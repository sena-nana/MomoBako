import { ensureThumbnail } from "../../services/repositoryApi";
import { getPreviewPluginForEntry } from "../../plugins/previewPlugins";
import type { FileBrowserEntry, FileBrowserSnapshot, ThumbnailResponse } from "../../types/repository";
import { activeRepoId, currentDirectoryPath, error, fileBrowser, fileBrowserDerived, selectedFilePath } from "./state";
import { yieldToUi } from "./scheduler";

const THUMBNAIL_LOAD_CONCURRENCY = 3;
const NETEASE_PROVIDER_ID = "netease-cloud-music";

export type ThumbnailQueueItem = {
  repoId: string;
  directoryPath: string;
  entry: FileBrowserEntry;
};

let thumbnailDirectoryToken = 0;
const queuedThumbnailKeys = new Set<string>();

export function invalidateThumbnailQueue() {
  thumbnailDirectoryToken += 1;
  queuedThumbnailKeys.clear();
}

export function loadThumbnailsForSnapshot(snapshot: FileBrowserSnapshot) {
  if (snapshot.specialLocation === "trash") return;
  const token = ++thumbnailDirectoryToken;
  queuedThumbnailKeys.clear();
  const queue = createThumbnailQueue(snapshot.repoId, snapshot.currentPath, snapshot.entries);
  let cursor = 0;

  async function worker() {
    while (cursor < queue.length) {
      const item = queue[cursor++];
      await yieldToUi();
      await loadThumbnailForQueueItem(item, token);
    }
  }

  void Promise.all(
    Array.from({ length: Math.min(THUMBNAIL_LOAD_CONCURRENCY, queue.length) }, () => worker()),
  );
}

export function loadThumbnailsForEntries(repoId: string, directoryPath: string, entries: FileBrowserEntry[]) {
  const token = thumbnailDirectoryToken;
  const queue = createThumbnailQueue(repoId, directoryPath, entries);
  let cursor = 0;

  async function worker() {
    while (cursor < queue.length) {
      const item = queue[cursor++];
      await yieldToUi();
      await loadThumbnailForQueueItem(item, token);
    }
  }

  void Promise.all(
    Array.from({ length: Math.min(THUMBNAIL_LOAD_CONCURRENCY, queue.length) }, () => worker()),
  );
}

function createThumbnailQueue(repoId: string, directoryPath: string, entries: FileBrowserEntry[]) {
  return entries
    .filter((entry) => entry.kind === "file" && !entry.thumbnailPath)
    .filter((entry) => {
      const key = `${repoId}:${directoryPath}:${entry.path}`;
      if (queuedThumbnailKeys.has(key)) return false;
      queuedThumbnailKeys.add(key);
      return true;
    })
    .map((entry) => ({
      repoId,
      directoryPath,
      entry,
    }))
    .sort((left, right) => thumbnailPriority(left.entry) - thumbnailPriority(right.entry));
}

function thumbnailPriority(entry: FileBrowserEntry) {
  if (entry.path === selectedFilePath.value) return 0;
  if (!entry.path.includes("/")) return 1;
  return 2;
}

async function loadThumbnailForQueueItem(item: ThumbnailQueueItem, token: number) {
  try {
    const response = await ensureThumbnail({
      repoId: item.repoId,
      path: item.entry.path,
      action: "ensure",
    });
    if (!response.thumbnailPath) {
      const sourceResponse = await loadRemoteSourceThumbnailForQueueItem(item, token);
      if (sourceResponse) return;
      await loadGeneratedThumbnailForQueueItem(item, token);
      return;
    }
    if (token !== thumbnailDirectoryToken) return;
    applyThumbnailResponse(response, item.directoryPath);
    return;
  } catch {
    return;
  }
}

function neteaseCoverUrl(entry: FileBrowserEntry) {
  if (entry.sourcePayload?.provider !== NETEASE_PROVIDER_ID) return "";
  const value = entry.sourcePayload?.coverUrl;
  return typeof value === "string" && /^https?:\/\//i.test(value) ? value : "";
}

async function loadRemoteSourceThumbnailForQueueItem(item: ThumbnailQueueItem, token: number) {
  const sourceUrl = neteaseCoverUrl(item.entry);
  if (!sourceUrl) return null;
  try {
    const response = await ensureThumbnail({
      repoId: item.repoId,
      path: item.entry.path,
      action: "save",
      sourceUrl,
    });
    if (!response.thumbnailPath || token !== thumbnailDirectoryToken) return response;
    applyThumbnailResponse(response, item.directoryPath);
    return response;
  } catch {
    return null;
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

  const patchEntry = (item: FileBrowserEntry): FileBrowserEntry => (
    item.path === response.path && item.kind === response.kind
      ? {
          ...item,
          assetId: response.assetId || item.assetId,
          thumbnailPath: response.thumbnailPath ?? null,
          thumbnailCustom: response.thumbnailCustom,
          metadata: response.metadata ?? item.metadata,
        }
      : item
  );

  fileBrowser.value = {
    ...current,
    entries: current.entries.map(patchEntry),
  };

  if (!fileBrowserDerived.value.entryMap.has(response.path)) return;

  fileBrowserDerived.value = {
    entryMap: new Map(
      Array.from(fileBrowserDerived.value.entryMap.entries(), ([path, item]) => [path, patchEntry(item)]),
    ),
    directories: fileBrowserDerived.value.directories.map(patchEntry),
    files: fileBrowserDerived.value.files.map(patchEntry),
    visibleEntries: fileBrowserDerived.value.visibleEntries.map(patchEntry),
  };
}

type WorkspaceThumbnailSaveRequest =
  | {
      action: "save";
      sourcePath: string;
    }
  | {
      action: "save";
      sourceUrl: string;
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

export function setWorkspaceEntryThumbnailFromUrl(path: string, sourceUrl: string) {
  return mutateWorkspaceEntryThumbnail(path, {
    action: "save",
    sourceUrl,
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
