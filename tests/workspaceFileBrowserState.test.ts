// 验证文件浏览派生状态在缩略图异步回填后仍然保留最新条目数据。
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { resetRepositoryWorkspaceForTests } from "../src/composables/useRepositoryWorkspace";
import * as repositoryApi from "../src/services/repositoryApi";
import {
  appendFileBrowserSnapshot,
  applyFileBrowserSnapshot,
  loadFileBrowserForDirectory,
  waitForFileBrowserDerivedState,
} from "../src/composables/workspace/files";
import * as thumbnails from "../src/composables/workspace/thumbnails";
import {
  activePanel,
  activeRepoId,
  createEmptyFileBrowserDerivedState,
  currentDirectoryPath,
  fileBrowser,
  fileBrowserDerived,
  selectedFilePath,
  selectedFilePaths,
  selectionAnchorPath,
} from "../src/composables/workspace/state";
import { applyThumbnailResponse } from "../src/composables/workspace/thumbnails";
import type { FileBrowserSnapshot } from "../src/types/repository";

function resetFileBrowserState() {
  resetRepositoryWorkspaceForTests();
  currentDirectoryPath.value = "";
  fileBrowser.value = null;
  fileBrowserDerived.value = createEmptyFileBrowserDerivedState();
  selectedFilePath.value = null;
  selectedFilePaths.value = [];
  selectionAnchorPath.value = null;
}

function createFileDirectorySnapshot(options: {
  repoId?: string;
  currentPath?: string;
  startIndex?: number;
  count: number;
  totalEntries?: number;
  loadedCount?: number;
  nextOffset?: number | null;
  hasMore?: boolean;
}): FileBrowserSnapshot {
  const startIndex = options.startIndex ?? 1;
  const entries = Array.from({ length: options.count }, (_, index) => {
    const entryIndex = startIndex + index;
    return {
      path: `${options.currentPath ?? "图库"}/文件-${entryIndex}.png`,
      name: `文件-${entryIndex}.png`,
      kind: "file" as const,
      extension: "png",
      sizeBytes: entryIndex * 1024,
      sizeLabel: `${entryIndex} KB`,
      modifiedAt: "2026-07-02T00:00:00Z",
      assetId: `asset-${entryIndex}`,
      status: "ready" as const,
      thumbnailPath: null,
      thumbnailCustom: false,
      hardlinkGroupId: null,
      hardlinkState: null,
      tags: [],
      aliasPaths: [],
      folderMetadata: null,
      metadata: {},
      isVirtual: false,
      providerId: null,
      providerItemId: null,
      sourcePayload: null,
      localAbsolutePath: `C:/Mock/${entryIndex}.png`,
    };
  });

  return {
    repoId: options.repoId ?? "repo-files-1",
    rootPath: "C:/Mock",
    backendPluginId: "momobako.local-filesystem",
    backendKind: "filesystem",
    cacheState: "ready",
    indexedAt: "2026-07-02T00:00:00Z",
    currentPath: options.currentPath ?? "图库",
    totalEntries: options.totalEntries ?? entries.length,
    loadedCount: options.loadedCount ?? entries.length,
    nextOffset: options.nextOffset ?? null,
    hasMore: options.hasMore ?? false,
    tree: [],
    entries,
  };
}

function createLargeNeteaseDirectorySnapshot(): FileBrowserSnapshot {
  const entries = Array.from({ length: 620 }, (_, index) => ({
    path: `创建的歌单/歌单-${index + 1}`,
    name: `歌单-${index + 1}`,
    kind: "directory" as const,
    extension: null,
    sizeBytes: null,
    sizeLabel: null,
    modifiedAt: "2026-06-14T00:00:00Z",
    assetId: null,
    status: null,
    thumbnailPath: null,
    thumbnailCustom: false,
    hardlinkGroupId: null,
    hardlinkState: null,
    tags: [],
    aliasPaths: [],
    folderMetadata: null,
    metadata: {
      provider: "netease-cloud-music",
      entryKind: "playlist-folder",
      playlistId: 9000 + index,
    },
    isVirtual: true,
    providerId: "netease-cloud-music",
    providerItemId: String(9000 + index),
    sourcePayload: {
      provider: "netease-cloud-music",
      entryKind: "playlist-folder",
      playlistId: 9000 + index,
      playlistName: `歌单-${index + 1}`,
      playlistCoverUrl: `https://example.test/playlist-${9000 + index}.jpg`,
    },
    localAbsolutePath: null,
  }));

  return {
    repoId: "netease-cloud-music-123456",
    rootPath: "netease-cloud-music://account/123456",
    backendPluginId: "momobako.source.netease-cloud-music",
    backendKind: "netease-cloud-music",
    cacheState: "ready",
    indexedAt: null,
    currentPath: "创建的歌单",
    totalEntries: entries.length,
    loadedCount: entries.length,
    nextOffset: null,
    hasMore: false,
    tree: [],
    entries,
  };
}

beforeEach(() => {
  vi.spyOn(thumbnails, "loadThumbnailsForEntries").mockImplementation(() => {});
  vi.spyOn(thumbnails, "loadThumbnailsForSnapshot").mockImplementation(() => {});
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("workspace file browser derived state", () => {
  it("目录封面异步回填后不会被旧快照覆盖", async () => {
    resetFileBrowserState();
    const snapshot = createLargeNeteaseDirectorySnapshot();
    const targetPath = snapshot.entries[511]?.path ?? "";
    const targetThumbnailPath = "C:/Mock/Thumbs/创建的歌单__歌单-512.jpg";

    applyFileBrowserSnapshot(snapshot);
    applyThumbnailResponse({
      repoId: snapshot.repoId,
      path: targetPath,
      assetId: "asset-ncm-playlist-512",
      kind: "directory",
      thumbnailPath: targetThumbnailPath,
      thumbnailCustom: true,
    });

    await waitForFileBrowserDerivedState();

    expect(
      fileBrowser.value?.entries.find((entry) => entry.path === targetPath)?.thumbnailPath,
    ).toBe(targetThumbnailPath);
    expect(
      fileBrowserDerived.value.directories.find((entry) => entry.path === targetPath)?.thumbnailPath,
    ).toBe(targetThumbnailPath);
  });

  it("空选中加载目录后保持空选中", async () => {
    resetFileBrowserState();
    const snapshot = createFileDirectorySnapshot({ count: 12 });

    applyFileBrowserSnapshot(snapshot);
    await waitForFileBrowserDerivedState();

    expect(selectedFilePath.value).toBeNull();
    expect(selectedFilePaths.value).toEqual([]);
  });

  it("当前选中项消失后保持空选中，不晋升相邻条目", async () => {
    resetFileBrowserState();
    const initialSnapshot = createFileDirectorySnapshot({ count: 3 });
    applyFileBrowserSnapshot(initialSnapshot);
    selectedFilePath.value = initialSnapshot.entries[0]?.path ?? null;
    selectedFilePaths.value = selectedFilePath.value ? [selectedFilePath.value] : [];

    const refreshedSnapshot = {
      ...initialSnapshot,
      entries: initialSnapshot.entries.slice(1),
      loadedCount: 2,
      totalEntries: 2,
    };
    applyFileBrowserSnapshot(refreshedSnapshot);
    await waitForFileBrowserDerivedState();

    expect(selectedFilePath.value).toBeNull();
    expect(selectedFilePaths.value).toEqual([]);
  });

  it("静默刷新当前目录时会保留已加载页并继续显示列表", async () => {
    resetFileBrowserState();
    activeRepoId.value = "repo-files-1";
    activePanel.value = "files";

    const initialSnapshot = createFileDirectorySnapshot({
      count: 160,
      totalEntries: 240,
      loadedCount: 160,
      nextOffset: 160,
      hasMore: true,
    });
    applyFileBrowserSnapshot(initialSnapshot);
    await waitForFileBrowserDerivedState();

    const refreshedSnapshot = createFileDirectorySnapshot({
      count: 160,
      totalEntries: 240,
      loadedCount: 160,
      nextOffset: 160,
      hasMore: true,
    });
    const getFileBrowserSpy = vi.spyOn(repositoryApi, "getFileBrowser").mockResolvedValue(refreshedSnapshot);

    const refreshPromise = loadFileBrowserForDirectory(initialSnapshot.currentPath, { silent: true });

    expect(getFileBrowserSpy).toHaveBeenCalledWith(expect.objectContaining({
      repoId: "repo-files-1",
      directoryPath: initialSnapshot.currentPath,
      offset: 0,
      limit: 160,
    }));
    expect(fileBrowserDerived.value.files).toHaveLength(160);

    await refreshPromise;
    await waitForFileBrowserDerivedState();

    expect(fileBrowser.value?.entries).toHaveLength(160);
    expect(fileBrowserDerived.value.files).toHaveLength(160);
  });

  it("追加分页遇到重叠条目时会按唯一条目继续推进偏移", async () => {
    resetFileBrowserState();
    activeRepoId.value = "repo-files-1";
    activePanel.value = "files";

    const firstPage = createFileDirectorySnapshot({
      count: 80,
      totalEntries: 200,
      loadedCount: 80,
      nextOffset: 80,
      hasMore: true,
    });
    applyFileBrowserSnapshot(firstPage);
    await waitForFileBrowserDerivedState();

    const secondPage = createFileDirectorySnapshot({
      startIndex: 61,
      count: 80,
      totalEntries: 200,
      loadedCount: 160,
      nextOffset: 160,
      hasMore: true,
    });
    appendFileBrowserSnapshot(secondPage);
    await waitForFileBrowserDerivedState();

    expect(fileBrowser.value?.entries).toHaveLength(140);
    expect(fileBrowser.value?.loadedCount).toBe(140);
    expect(fileBrowser.value?.nextOffset).toBe(140);
    expect(fileBrowser.value?.hasMore).toBe(true);

    const finalPage = createFileDirectorySnapshot({
      startIndex: 141,
      count: 60,
      totalEntries: 200,
      loadedCount: 200,
      nextOffset: null,
      hasMore: false,
    });
    const getFileBrowserSpy = vi.spyOn(repositoryApi, "getFileBrowser").mockResolvedValue(finalPage);

    await loadFileBrowserForDirectory(firstPage.currentPath, { append: true });
    await waitForFileBrowserDerivedState();

    expect(getFileBrowserSpy).toHaveBeenCalledWith(expect.objectContaining({
      repoId: "repo-files-1",
      directoryPath: firstPage.currentPath,
      offset: 140,
      limit: 160,
    }));
    expect(fileBrowser.value?.entries).toHaveLength(200);
    expect(fileBrowser.value?.loadedCount).toBe(200);
    expect(fileBrowser.value?.nextOffset).toBeNull();
    expect(fileBrowser.value?.hasMore).toBe(false);
  });
});
