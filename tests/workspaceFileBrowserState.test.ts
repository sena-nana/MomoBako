// 验证文件浏览派生状态在缩略图异步回填后仍然保留最新条目数据。
import { describe, expect, it } from "vitest";
import { resetRepositoryWorkspaceForTests } from "../src/composables/useRepositoryWorkspace";
import { applyFileBrowserSnapshot, waitForFileBrowserDerivedState } from "../src/composables/workspace/files";
import { createEmptyFileBrowserDerivedState, currentDirectoryPath, fileBrowser, fileBrowserDerived, selectedFilePath, selectedFilePaths, selectionAnchorPath } from "../src/composables/workspace/state";
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
});
