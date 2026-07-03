import { render, screen, within } from "@testing-library/vue";
import { describe, expect, it, vi } from "vitest";
import FileBrowserPanel from "../src/pages/workspace/files/FileBrowserPanel.vue";
import type { RegisteredLibraryExtension } from "../src/plugins/sdk";
import type { FileBrowserEntry } from "../src/types/repository";
import { fileSummary, matchAsmrEntry } from "../External/Plugins/library-asmr/src/asmrLibrary";

function asmrEntry(): FileBrowserEntry {
  return {
    path: "Voice/RJ123456 Rain Voice/01.mp3",
    name: "01.mp3",
    kind: "file",
    extension: "mp3",
    sizeBytes: 1024,
    sizeLabel: "1 KB",
    modifiedAt: "2026-06-05T00:18:00Z",
    assetId: "asset-rj123456-01",
    status: "synced",
    metadata: {
      libraryKind: "asmr",
      workId: "RJ123456",
      rjCode: "RJ123456",
      workTitle: "Rain Voice",
      trackTitle: "01 intro",
      lyricStatus: "local",
      listeningStatus: "listening",
      listeningProgress: 42,
      asmrEntryKind: "audio",
    },
  };
}

function mockDirectoryEntry(): FileBrowserEntry {
  return {
    path: "Voice",
    name: "Voice",
    kind: "directory",
    extension: null,
    sizeBytes: null,
    sizeLabel: null,
    modifiedAt: "2026-06-05T00:18:00Z",
    assetId: null,
    status: null,
    metadata: {},
  };
}

function renderPanel(entry = asmrEntry(), overrides: Record<string, unknown> = {}) {
  const libraryExtension: RegisteredLibraryExtension = {
    pluginId: "momobako.library.asmr",
    pluginName: "ASMR Library",
    libraryKind: "asmr",
    label: "ASMR",
    matchEntry: matchAsmrEntry,
    fileSummary,
  };
  return render(FileBrowserPanel, {
    props: {
      breadcrumbs: [],
      canDragEntries: false,
      canDeleteSelected: true,
      canOpenSelected: true,
      canRenameSelected: true,
      canRestoreSelected: false,
      currentDirectoryDisplayName: "Voice",
      currentDirectoryPath: "Voice",
      currentFileEntry: entry,
      allEntries: [entry],
      directoryEntries: [],
      displayModeClass: "files-list__files--list",
      displayModeOptions: [{ value: "list", label: "列表" }],
      dropTargetPath: null,
      entryDeletedAtLabel: () => null,
      entryModifiedAtLabel: () => "2026/6/5 08:18:00",
      error: null,
      fileEntries: [entry],
      fileEntryContextMenu: () => [],
      fileItemStyle: () => ({}),
      fileTone: () => "rgb(20, 20, 20)",
      hardlinkStateLabel: () => "",
      hasSplitFileGroups: false,
      isAudioEntry: () => true,
      isDraggingFiles: false,
      isDragActive: false,
      isLoadingFileBrowser: false,
      isModelEntry: () => false,
      isMutatingFiles: false,
      isTrashPanel: false,
      isVideoEntry: () => false,
      openSelectedLabel: "打开",
      renameTargetPath: null,
      selectedEntries: [entry],
      selectedFilePaths: [entry.path],
      selectedFilePath: entry.path,
      statusLabel: (status: string) => status,
      thumbnailSrc: () => null,
      isSavingMetadata: false,
      availableTags: [],
      tagGroups: [],
      thumbnailPalette: () => [],
      saveMetadata: vi.fn(),
      libraryExtensions: [libraryExtension],
      createFileName: "",
      fileDisplayMode: "list",
      renameValue: "",
      ...overrides,
    },
  });
}

describe("FileBrowserPanel ASMR summary", () => {
  it("在文件列表和详情中显示 ASMR 作品、收听进度和歌词状态", () => {
    renderPanel();

    expect(screen.getAllByText(/RJ123456 · Rain Voice · 收听中 · 42% · 歌词/).length).toBeGreaterThan(0);
    expect(screen.getByText("作品")).toBeInTheDocument();
    expect(screen.getAllByText("Rain Voice").length).toBeGreaterThan(0);
    expect(screen.getAllByText("音轨").length).toBeGreaterThan(0);
    expect(screen.getAllByText("01 intro").length).toBeGreaterThan(0);
    expect(screen.getByText("收听")).toBeInTheDocument();
    expect(screen.getAllByText("收听中 · 42%").length).toBeGreaterThan(0);
    expect(screen.getAllByText("歌词").length).toBeGreaterThan(0);
    expect(screen.getAllByText("local").length).toBeGreaterThan(0);
  });

  it("最近使用视图在展示方式右侧显示清空记录按钮", () => {
    renderPanel(asmrEntry(), {
      canClearRecentHistory: true,
      isRecentView: true,
    });

    expect(screen.getByRole("button", { name: "清空记录" })).toBeInTheDocument();
  });

  it("普通目录视图不显示清空记录按钮", () => {
    renderPanel();

    expect(screen.queryByRole("button", { name: "清空记录" })).toBeNull();
  });

  it("详情标题与文件列表标题显示去扩展名名称，并移除同步状态", () => {
    renderPanel();

    expect(screen.getAllByText("01").length).toBeGreaterThan(0);
    expect(screen.queryByText("01.mp3")).toBeNull();
    expect(screen.queryByText("synced")).toBeNull();
  });

  it("无选中文件时显示当前文件夹信息", () => {
    const fileEntry = asmrEntry();
    const directoryEntry = mockDirectoryEntry();
    renderPanel(fileEntry, {
      currentFileEntry: null,
      selectedEntries: [],
      selectedFilePaths: [],
      selectedFilePath: null,
      directoryEntries: [directoryEntry],
      fileEntries: [fileEntry],
      allEntries: [directoryEntry, fileEntry],
      currentDirectoryDisplayName: "主资源库",
      currentDirectoryPath: "",
    });

    expect(screen.getByRole("heading", { name: "主资源库" })).toBeInTheDocument();
    expect(screen.getAllByText("根目录").length).toBeGreaterThan(0);
    expect(screen.getByText("直属文件")).toBeInTheDocument();
    expect(screen.getByText("直属子文件夹")).toBeInTheDocument();
    expect(screen.getByText("当前视图总条目")).toBeInTheDocument();
    const detailCard = screen.getByRole("heading", { name: "主资源库" }).closest(".files-detail__card");
    expect(detailCard).toBeInstanceOf(HTMLElement);
    const values = within(detailCard as HTMLElement).getAllByText(/^[12]$/).map((element) => element.textContent);
    expect(values).toEqual(["1", "1", "2"]);
  });
});
