import { fireEvent, render, screen } from "@testing-library/vue";
import { describe, expect, it, vi } from "vitest";
import type { FileBrowserEntry } from "../src/types/repository";
import WorkspaceFilesSurface from "../src/pages/workspace/WorkspaceFilesSurface.vue";

function fileEntry(): FileBrowserEntry {
  return {
    path: "Music/demo-track.mp3",
    name: "demo-track.mp3",
    kind: "file",
    extension: "mp3",
    sizeBytes: 2048,
    sizeLabel: "2 KB",
    modifiedAt: "2026-07-03T12:00:00Z",
    assetId: "asset-demo-track",
    status: "ready",
    metadata: {},
  };
}

function renderSurface() {
  const entry = fileEntry();
  return render(WorkspaceFilesSurface, {
    props: {
      activeFileEntries: [entry],
      allEntries: [entry],
      activeRepoId: "repo-main-001",
      availableTags: [],
      breadcrumbs: [],
      canDeleteSelected: true,
      canDragEntries: false,
      canImport: true,
      canOpenSelected: true,
      canRenameSelected: true,
      canRestoreSelected: false,
      canClearRecentHistory: false,
      currentFileEntry: entry,
      currentDirectoryDisplayName: "Music",
      currentDirectoryPath: "Music",
      currentLibraryExtensions: [],
      directoryEntries: [],
      displayModeClass: "files-list__files--list",
      displayModeOptions: [{ value: "list", label: "列表" }],
      dropTargetPath: null,
      entryDeletedAtLabel: () => null,
      entryModifiedAtLabel: () => "2026/7/3 20:00:00",
      error: null,
      fileEntryContextMenu: () => [],
      fileItemStyle: () => ({}),
      fileTone: () => "rgb(20, 20, 20)",
      hardlinkStateLabel: () => "",
      hasSplitFileGroups: false,
      isAudioEntry: () => true,
      isDragActive: false,
      isDraggingFiles: false,
      isLoadingFileBrowser: false,
      isLoadingFileBrowserMore: false,
      isModelEntry: () => false,
      isMutatingFiles: false,
      isClearingRecentHistory: false,
      isRecentView: false,
      isReadOnlyVirtual: false,
      isSavingMetadata: false,
      isTrashPanel: false,
      isVirtualView: false,
      isVideoEntry: () => false,
      hasMoreEntries: false,
      libraryExtensions: [],
      openSelectedLabel: "打开",
      previewFileEntry: null,
      previewLibraryExtensions: [],
      previewPlugin: null,
      renameTargetPath: entry.path,
      renameValue: "demo-track.mp3",
      saveCoverThumbnail: vi.fn(),
      saveMetadata: vi.fn(),
      selectedEntries: [entry],
      selectedFilePath: entry.path,
      selectedFilePaths: [entry.path],
      showWorkspacePlayer: false,
      statusLabel: (status: string) => status,
      tagGroups: [],
      thumbnailPalette: () => [],
      thumbnailSrc: () => null,
      virtualSubline: "",
      virtualTitle: "分类视图",
      workspacePlayerBarHandlers: {
        togglePlay: vi.fn(),
        previous: vi.fn(),
        next: vi.fn(),
        cycleMode: vi.fn(),
        openQueue: vi.fn(),
        openPreview: vi.fn(),
        setVolume: vi.fn(),
        selectQueueItem: vi.fn(),
        seek: vi.fn(),
        setImageDuration: vi.fn(),
        setObjectFit: vi.fn(),
      },
      workspacePlayerBarProps: {
        item: null,
        playerLabel: null,
        fileClass: null,
        supportsSeek: false,
        supportsVolume: false,
        canPlay: false,
        mode: "listLoop",
        currentTimeMs: 0,
        durationMs: 0,
        volume: 1,
        imageDurationMs: 5000,
        objectFit: "contain",
        isPlaying: false,
        errorMessage: null,
        queueOpen: false,
        queueItems: [],
        currentItemId: null,
      },
      createFileName: "",
      fileDisplayMode: "list",
      "onUpdate:createFileName": vi.fn(),
      "onUpdate:fileDisplayMode": vi.fn(),
      "onUpdate:renameValue": vi.fn(),
    },
    global: {
      stubs: {
        FileBrowserPanel: { template: "<div class='stub-browser'></div>" },
        FilePreviewPane: { template: "<div class='stub-preview'></div>" },
        WorkspacePlayerBar: { template: "<div class='stub-player'></div>" },
      },
    },
  });
}

describe("WorkspaceFilesSurface", () => {
  it("重命名时显示应用内弹窗，并支持 Esc / Enter 事件", async () => {
    const view = renderSurface();

    expect(screen.getByRole("dialog", { name: "重命名文件" })).toBeInTheDocument();

    const input = screen.getByPlaceholderText("输入新的文件名");
    await fireEvent.keyDown(input, { key: "Escape" });
    await fireEvent.keyDown(input, { key: "Enter" });

    expect(view.emitted("cancelRename")).toHaveLength(1);
    expect(view.emitted("submitRename")).toHaveLength(1);
  });
});
