import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/vue";
import { createMemoryHistory } from "vue-router";
import { describe, expect, it, vi } from "vitest";
import App from "../src/App.vue";
import { installContextMenu } from "../src/composables/useContextMenu";
import { resetRepositoryWorkspaceForTests, useRepositoryWorkspace } from "../src/composables/useRepositoryWorkspace";
import { usePlaylistPlayer } from "../src/composables/usePlaylistPlayer";
import { vContextMenu } from "../src/directives/contextMenu";
import { createMomoBakoRouter } from "../src/router";
import {
  createDirectoryOnNextSync,
  delayNextInvoke,
  failNextInvoke,
  failNextOpenerCall,
  getRelocatedRepositoryPath,
  getInvokeCalls,
  getOpenerCalls,
  seedCrossRepositorySearchHit,
  seedMissingMockRepository,
  seedMixedMockRepositories,
  seedMockPlaylists,
  seedMockPlugins,
  seedMockRepository,
  seedMockRepositoryPath,
  selectMockFile,
  selectMockFolder,
} from "./setupTests";
import { getPreviewPluginForEntry } from "../src/plugins/previewPlugins";
import type { FileBrowserEntry, PlaylistDetail, PlaylistSummary } from "../src/types/repository";
import { createMockPlugins } from "./fixtures/repositoryFixtures";

async function renderApp() {
  resetRepositoryWorkspaceForTests();
  usePlaylistPlayer().resetPlayerState();
  installContextMenu();
  const router = createMomoBakoRouter(createMemoryHistory());
  await router.push("/");
  await router.isReady();
  await useRepositoryWorkspace().ensureRepositoryWorkspace();

  render(App, {
    global: {
      plugins: [router],
      directives: {
        "context-menu": vContextMenu,
      },
    },
  });
  await waitForCurrentWorkspaceView();
}

async function renderAppWithoutStartupPreload() {
  resetRepositoryWorkspaceForTests();
  usePlaylistPlayer().resetPlayerState();
  installContextMenu();
  const router = createMomoBakoRouter(createMemoryHistory());
  await router.push("/");
  await router.isReady();

  render(App, {
    global: {
      plugins: [router],
      directives: {
        "context-menu": vContextMenu,
      },
    },
  });
}

async function waitForCurrentWorkspaceView() {
  await waitFor(() => {
    const workspace = useRepositoryWorkspace();
    const selector = workspace.activeSnapshot.value
      ? workspace.activePanel.value === "search"
          ? ".search-workbench"
          : workspace.activePanel.value === "extensions"
            ? ".extensions-workbench"
            : ".files-browser, .files-preview-page__body"
      : workspace.activeRepository.value?.status === "missing"
        ? ".missing-repository-page"
        : ".empty-state-page";
    expect(document.querySelector(selector)).toBeInTheDocument();
  });
}

function shellElement() {
  const shell = document.querySelector(".shell");
  expect(shell).toBeInstanceOf(HTMLElement);
  return shell as HTMLElement;
}

function pointerEvent(type: string, clientX: number) {
  return new PointerEvent(type, {
    bubbles: true,
    button: 0,
    buttons: 1,
    clientX,
    pointerId: 1,
  });
}

function workspaceBrowser() {
  const browser = document.querySelector(".files-browser");
  expect(browser).toBeInstanceOf(HTMLElement);
  return browser as HTMLElement;
}

function fileListItems() {
  return Array.from(document.querySelectorAll<HTMLElement>(".files-list__item[data-entry-path]"));
}

function fileListItem(path: string) {
  const item = fileListItems().find((element) => element.dataset.entryPath === path);
  expect(item).toBeInstanceOf(HTMLElement);
  return item as HTMLElement;
}

function setPlaybackSession(repoId: string, session: Record<string, unknown>) {
  window.localStorage.setItem(`momobako.playbackSession:${repoId}`, JSON.stringify(session));
}

function audioPlaylist(repoId = "repo-main-001"): PlaylistSummary {
  return {
    playlistId: "playlist-mock",
    repoId,
    name: "Mock Playlist",
    playerTypeId: "momobako.playlist.audio-sequence",
    playerPluginId: "momobako.preview.media",
    playerLabel: "音频顺序播放",
    fileClass: "audio",
    itemCount: 1,
    sortOrder: 0,
    createdAt: "2026-06-05T00:18:00Z",
    updatedAt: "2026-06-05T00:18:00Z",
  };
}

function audioPlaylistDetail(repoId = "repo-main-001"): PlaylistDetail {
  return {
    playlist: audioPlaylist(repoId),
    items: [{
      playlistItemId: "playlist-item-mock",
      playlistId: "playlist-mock",
      assetId: "asset-01",
      path: "asset-01.mp3",
      filename: "asset-01.mp3",
      extension: "mp3",
      thumbnailPath: null,
      status: "ready",
      statusReason: null,
      sortOrder: 0,
      addedAt: "2026-06-05T00:18:00Z",
    }],
  };
}

function folderTreeItem(path: string) {
  const item = Array.from(document.querySelectorAll<HTMLElement>(".workspace-folder-tree__item"))
    .find((element) => element.textContent?.includes(path.split("/").at(-1) ?? path));
  expect(item).toBeInstanceOf(HTMLElement);
  return item as HTMLElement;
}

function setElementRect(element: HTMLElement, rect: { left: number; top: number; right: number; bottom: number }) {
  element.getBoundingClientRect = () => ({
    ...rect,
    width: rect.right - rect.left,
    height: rect.bottom - rect.top,
    x: rect.left,
    y: rect.top,
    toJSON: () => rect,
  } as DOMRect);
}

function previewEntry(extension: string): FileBrowserEntry {
  return {
    path: `Preview/asset.${extension}`,
    name: `asset.${extension}`,
    kind: "file",
    extension,
    sizeBytes: 1024,
    sizeLabel: "1 KB",
    modifiedAt: "2026-06-05T00:18:00Z",
    assetId: `asset-${extension}`,
    status: "synced",
    thumbnailPath: null,
    thumbnailCustom: false,
    metadata: {},
  };
}

describe("文件管理冒烟", () => {
  it("启动加载显示进度，完成后再显示仓库内容", async () => {
    seedMockRepository();
    const delay = delayNextInvoke("list_repositories");
    await renderAppWithoutStartupPreload();

    expect(screen.getByRole("heading", { name: "加载仓库列表" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "资源库" })).not.toBeInTheDocument();

    delay.resolve();
    await waitFor(() => {
      expect(screen.queryByRole("heading", { name: "加载仓库列表" })).not.toBeInTheDocument();
    });
    await waitForCurrentWorkspaceView();
    expect((await screen.findAllByText("Campaigns")).length).toBeGreaterThan(0);
  });

  it("启动加载并发调用复用同一条链路", async () => {
    seedMockRepository();
    resetRepositoryWorkspaceForTests();
    const workspace = useRepositoryWorkspace();
    const delay = delayNextInvoke("list_repositories");

    const first = workspace.ensureRepositoryWorkspace();
    const second = workspace.ensureRepositoryWorkspace();
    expect(first).toBe(second);
    expect(getInvokeCalls("list_repositories")).toHaveLength(1);

    delay.resolve();
    await first;
    expect(getInvokeCalls("list_repositories")).toHaveLength(1);
    expect(workspace.workspaceStartup.value.status).toBe("ready");
  });

  it("启动失败显示错误并允许重试", async () => {
    seedMockRepository();
    failNextInvoke("list_repositories", "仓库注册表读取失败");
    await renderAppWithoutStartupPreload();

    expect(await screen.findByText("仓库注册表读取失败")).toBeInTheDocument();
    await fireEvent.click(screen.getByRole("button", { name: "重试" }));

    expect(await screen.findByRole("button", { name: "资源库" })).toBeInTheDocument();
    expect(getInvokeCalls("list_repositories")).toHaveLength(2);
  });

  it("启动时当前资源库丢失，进入工作区并显示修复操作", async () => {
    seedMissingMockRepository();
    await renderApp();

    expect(await screen.findByRole("heading", { name: "主资源库" })).toBeInTheDocument();
    expect(screen.getByText("资源库丢失")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "重定向" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "删除资源库" })).toBeInTheDocument();
    expect(await screen.findByRole("button", { name: "资源库" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /全部/ })).toBeDisabled();
    expect(screen.getByRole("button", { name: /已删除/ })).toBeDisabled();
    expect(screen.getByRole("button", { name: "刷新文件夹树" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "新建智能文件夹" })).toBeDisabled();
    expect(getInvokeCalls("get_repository_snapshot")).toHaveLength(0);
  });

  it("资源库切换器标记丢失资源库，并允许切换到丢失态", async () => {
    seedMixedMockRepositories();
    await renderApp();

    expect((await screen.findAllByText("Reference")).length).toBeGreaterThan(0);
    await fireEvent.click(screen.getByRole("button", { name: "资源库" }));
    expect(await screen.findByText("丢失")).toBeInTheDocument();
    await fireEvent.click(await screen.findByRole("button", { name: "切换资源库 主资源库" }));

    expect(await screen.findByText("资源库丢失")).toBeInTheDocument();
    const snapshotCalls = getInvokeCalls("get_repository_snapshot").filter((call) => call.args?.repoId === "repo-main-001");
    expect(snapshotCalls).toHaveLength(0);
  });

  it("重定向丢失资源库失败时保留丢失态并显示错误", async () => {
    seedMissingMockRepository();
    selectMockFolder("C:/Mock/DifferentRepo");
    await renderApp();

    await fireEvent.click(screen.getByRole("button", { name: "重定向" }));

    expect(await screen.findByText("selected folder belongs to a different repository")).toBeInTheDocument();
    expect(screen.getByText("资源库丢失")).toBeInTheDocument();
    expect(getInvokeCalls("relocate_repository").at(-1)?.args).toMatchObject({
      request: {
        repoId: "repo-main-001",
        path: "C:/Mock/DifferentRepo",
      },
    });
  });

  it("重定向丢失资源库成功后加载仓库内容", async () => {
    seedMissingMockRepository();
    selectMockFolder(getRelocatedRepositoryPath());
    await renderApp();

    await fireEvent.click(screen.getByRole("button", { name: "重定向" }));

    await waitFor(() => {
      expect(getInvokeCalls("relocate_repository").at(-1)?.args).toMatchObject({
        request: {
          repoId: "repo-main-001",
          path: getRelocatedRepositoryPath(),
        },
      });
    });
    expect((await screen.findAllByText("Campaigns")).length).toBeGreaterThan(0);
    expect(screen.queryByText("资源库丢失")).not.toBeInTheDocument();
    expect(getInvokeCalls("get_repository_snapshot").at(-1)?.args).toMatchObject({
      repoId: "repo-main-001",
    });
  });

  it("启动后会恢复当前资源库的播放会话且不切走文件浏览页", async () => {
    seedMockRepository();
    setPlaybackSession("repo-main-001", {
      repoId: "repo-main-001",
      playlistId: "playlist-mock",
      playerTypeId: "momobako.playlist.audio-sequence",
      currentItemId: "playlist-item-mock",
      currentTimeMs: 12000,
      durationMs: 180000,
      mode: "listLoop",
      volume: 0.6,
      isPlaying: false,
    });

    await renderApp();

    await waitFor(() => {
      expect(getInvokeCalls("get_playlist_detail").at(-1)?.args).toMatchObject({
        repoId: "repo-main-001",
        playlistId: "playlist-mock",
      });
    });
    expect(document.querySelector(".files-browser")).toBeInTheDocument();
    expect(document.querySelector(".workspace-player")).toBeInTheDocument();
    expect(screen.getByText("asset-01.mp3")).toBeInTheDocument();
    expect(screen.getByText(/音频顺序播放/)).toBeInTheDocument();
  });

  it("删除丢失资源库会移除注册并切到剩余资源库", async () => {
    seedMixedMockRepositories();
    await renderApp();

    await fireEvent.click(screen.getByRole("button", { name: "资源库" }));
    await fireEvent.click(await screen.findByRole("button", { name: "切换资源库 主资源库" }));
    await screen.findByText("资源库丢失");
    await fireEvent.click(screen.getByRole("button", { name: "删除资源库" }));
    const dialog = await screen.findByRole("dialog", { name: "删除丢失资源库" });
    await fireEvent.click(within(dialog).getByRole("button", { name: "删除" }));

    await waitFor(() => {
      expect(getInvokeCalls("delete_repository").at(-1)?.args).toMatchObject({
        repoId: "repo-main-001",
      });
    });
    expect((await screen.findAllByText("Reference")).length).toBeGreaterThan(0);
    expect(screen.queryByText("资源库丢失")).not.toBeInTheDocument();
  });

  it("手动同步时展示同步进度", async () => {
    seedMockRepository();
    const workspace = useRepositoryWorkspace();
    workspace.setActivePanel("files");
    await renderApp();

    const delay = delayNextInvoke("sync_repository");
    const syncPromise = workspace.syncActiveRepository();

    expect(await screen.findByText("扫描仓库文件")).toBeInTheDocument();
    expect(await screen.findByText("33%")).toBeInTheDocument();

    delay.resolve();
    await syncPromise;
    expect(workspace.syncProgress.value.phase).toBe("complete");
  });

  it("目录内容先显示灰色占位，再异步补充缩略图", async () => {
    seedMockRepository();
    const workspace = useRepositoryWorkspace();
    workspace.setActivePanel("files");
    const delay = delayNextInvoke("ensure_thumbnail");

    await renderApp();

    expect(screen.getAllByText("cover-final.psd").length).toBeGreaterThan(0);
    expect(workspace.fileBrowser.value?.entries.find((entry) => entry.path === "cover-final.psd")?.thumbnailPath ?? null).toBeNull();
    expect(getInvokeCalls("ensure_thumbnail")).toHaveLength(1);

    delay.resolve();
    await waitFor(() => {
      expect(workspace.fileBrowser.value?.entries.find((entry) => entry.path === "cover-final.psd")?.thumbnailPath)
        .toBe("C:/Mock/Thumbs/cover-final.psd.jpg");
    });
  });

  it("支持切换并记住素材展示方式", async () => {
    seedMockRepository();
    const workspace = useRepositoryWorkspace();
    workspace.setActivePanel("files");

    await renderApp();

    const displayModeSelect = await screen.findByLabelText("素材展示方式");
    expect(displayModeSelect).toHaveValue("adaptive");

    await fireEvent.update(displayModeSelect, "list");
    expect(localStorage.getItem("momobako.fileDisplayMode")).toBe("list");
    expect((await screen.findAllByText("227.9 MB")).length).toBeGreaterThan(0);
    expect((await screen.findAllByText("已同步")).length).toBeGreaterThan(0);

    cleanup();
    localStorage.setItem("momobako.fileDisplayMode", "masonry");
    await renderApp();
    expect(await screen.findByLabelText("素材展示方式")).toHaveValue("masonry");
    const masonryList = document.querySelector<HTMLElement>(".files-list__files--masonry");
    expect(masonryList).toBeInTheDocument();

    cleanup();
    localStorage.setItem("momobako.fileDisplayMode", "invalid-mode");
    await renderApp();
    expect(await screen.findByLabelText("素材展示方式")).toHaveValue("adaptive");
  });

  it("自适应展示方式使用缩略图自然比例调整素材宽度", async () => {
    seedMockRepository();
    const workspace = useRepositoryWorkspace();
    workspace.setActivePanel("files");

    await renderApp();

    await waitFor(() => {
      expect(workspace.fileBrowser.value?.entries.find((entry) => entry.path === "cover-final.psd")?.thumbnailPath)
        .toBe("C:/Mock/Thumbs/cover-final.psd.jpg");
    });

    const thumbnail = document.querySelector<HTMLImageElement>(".files-list__item--file .files-list__preview img");
    expect(thumbnail).not.toBeNull();
    Object.defineProperty(thumbnail, "naturalWidth", { configurable: true, value: 1600 });
    Object.defineProperty(thumbnail, "naturalHeight", { configurable: true, value: 900 });

    await fireEvent.load(thumbnail as HTMLImageElement);

    const fileItem = thumbnail?.closest<HTMLElement>(".files-list__item--file");
    expect(fileItem?.style.getPropertyValue("--file-thumb-aspect")).toBe(String(1600 / 900));
  });

  it("目录单击只选中，双击才进入目录", async () => {
    seedMockRepository();
    const workspace = useRepositoryWorkspace();
    workspace.setActivePanel("files");
    await renderApp();

    await fireEvent.click(fileListItem("Campaigns"));
    expect(workspace.currentDirectoryPath.value).toBe("");
    expect(workspace.selectedFilePaths.value).toEqual(["Campaigns"]);

    await fireEvent.dblClick(fileListItem("Campaigns"));
    await waitFor(() => {
      expect(getInvokeCalls("get_file_browser").at(-1)?.args).toMatchObject({
        request: {
          directoryPath: "Campaigns",
        },
      });
    });
    expect(workspace.currentDirectoryPath.value).toBe("Campaigns");
    expect(fileListItem("Campaigns/Summer")).toBeInTheDocument();
  });

  it("Eagle 快捷访问可以定位文件夹和文件", async () => {
    seedMockRepository();
    const workspace = useRepositoryWorkspace();
    workspace.setActivePanel("files");
    await renderApp();

    await fireEvent.click(screen.getByRole("button", { name: /快捷 Campaigns/ }));
    await waitFor(() => {
      expect(getInvokeCalls("get_file_browser").at(-1)?.args).toMatchObject({
        request: {
          directoryPath: "Campaigns",
        },
      });
    });
    expect(workspace.currentDirectoryPath.value).toBe("Campaigns");

    await fireEvent.click(screen.getByRole("button", { name: /封面文件/ }));
    await waitFor(() => {
      expect(getInvokeCalls("get_file_browser").at(-1)?.args).toMatchObject({
        request: {
          directoryPath: "Campaigns/Summer",
        },
      });
    });
    expect(workspace.currentDirectoryPath.value).toBe("Campaigns/Summer");
    expect(workspace.selectedFilePaths.value).toEqual(["Campaigns/Summer/cover-final.psd"]);
  });

  it("文件和文件夹详情展示 Eagle 迁移字段", async () => {
    seedMockRepository();
    const workspace = useRepositoryWorkspace();
    workspace.setActivePanel("files");
    await renderApp();

    await fireEvent.click(fileListItem("Campaigns"));
    expect(await screen.findByText("受保护")).toBeInTheDocument();
    expect(screen.getByText("项目归档密码提示")).toBeInTheDocument();

    await fireEvent.click(fileListItem("cover-final.psd"));
    expect(await screen.findByDisplayValue("最终版封面，保留可编辑图层。")).toBeInTheDocument();
    expect(screen.getByDisplayValue("https://example.test/source/cover")).toBeInTheDocument();
    expect(screen.getByText("1920 × 1080")).toBeInTheDocument();
    expect(screen.getAllByText("227.9 MB").length).toBeGreaterThan(0);
    expect(screen.getByText("Campaigns/Summer/cover-final.psd")).toBeInTheDocument();
    expect(screen.getByText("封面，主视觉，PSD")).toBeInTheDocument();
  });

  it("支持 Ctrl 多选、Shift 连选和框选", async () => {
    seedMockRepository();
    const workspace = useRepositoryWorkspace();
    workspace.setActivePanel("files");
    await renderApp();

    await fireEvent.click(fileListItem("Campaigns"));
    await fireEvent.click(fileListItem("cover-final.psd"), { ctrlKey: true });
    expect(workspace.selectedFilePaths.value).toEqual(["Campaigns", "cover-final.psd"]);

    await fireEvent.click(fileListItem("Campaigns"));
    await fireEvent.click(fileListItem("cover-final.psd"), { shiftKey: true });
    expect(workspace.selectedFilePaths.value).toEqual(["Campaigns", "cover-final.psd"]);

    const list = document.querySelector(".files-list");
    expect(list).toBeInstanceOf(HTMLElement);
    setElementRect(list as HTMLElement, { left: 0, top: 0, right: 320, bottom: 240 });
    setElementRect(fileListItem("Campaigns"), { left: 12, top: 12, right: 92, bottom: 92 });
    setElementRect(fileListItem("Backgrounds"), { left: 108, top: 12, right: 188, bottom: 92 });
    setElementRect(fileListItem("cover-final.psd"), { left: 204, top: 12, right: 284, bottom: 92 });

    await fireEvent.pointerDown(list as HTMLElement, {
      button: 0,
      clientX: 8,
      clientY: 8,
      pointerId: 2,
    });
    await fireEvent.pointerMove(list as HTMLElement, {
      buttons: 1,
      clientX: 190,
      clientY: 100,
      pointerId: 2,
    });
    await fireEvent.pointerUp(list as HTMLElement, {
      buttons: 0,
      clientX: 190,
      clientY: 100,
      pointerId: 2,
    });

    expect(workspace.selectedFilePaths.value).toEqual(["Backgrounds", "Campaigns"]);
  });

  it("多选后支持批量拖出和批量删除", async () => {
    seedMockRepository();
    const workspace = useRepositoryWorkspace();
    workspace.setActivePanel("files");
    await renderApp();

    await fireEvent.click(fileListItem("Campaigns"));
    await fireEvent.click(fileListItem("cover-final.psd"), { ctrlKey: true });

    await workspace.startWorkspaceEntriesDrag(workspace.selectedFilePaths.value);

    await waitFor(() => {
      expect(getInvokeCalls("plugin:drag|start_drag").at(-1)?.args).toMatchObject({
        item: [
          "C:/Mock/AnimeAssets/Campaigns",
          "C:/Mock/AnimeAssets/cover-final.psd",
        ],
      });
    });

    await fireEvent.click(screen.getByRole("button", { name: "批量删除" }));
    await waitFor(() => {
      expect(getInvokeCalls("delete_entry")).toHaveLength(2);
    });
  });

  it("切目录后丢弃旧目录返回的缩略图", async () => {
    seedMockRepository();
    const workspace = useRepositoryWorkspace();
    workspace.setActivePanel("files");
    const delay = delayNextInvoke("ensure_thumbnail");

    await renderApp();
    await workspace.loadFileBrowserForDirectory("Campaigns");
    delay.resolve();

    await waitFor(() => {
      expect(workspace.fileBrowser.value?.currentPath).toBe("Campaigns");
    });
    expect(workspace.fileBrowser.value?.entries.some((entry) => entry.path === "cover-final.psd")).toBe(false);
  });

  it("外部文件拖经文件夹树悬停后切换目录，再导入到该目录", async () => {
    vi.useFakeTimers();
    try {
      seedMockRepository();
      const workspace = useRepositoryWorkspace();
      workspace.setActivePanel("files");
      await renderApp();

      const browser = workspaceBrowser();
      await fireEvent.dragOver(browser, { dataTransfer: { files: [new File([], "new-shot.png")] } });
      await fireEvent.dragEnter(folderTreeItem("Campaigns"));
      await vi.advanceTimersByTimeAsync(460);

      await waitFor(() => {
        expect(getInvokeCalls("get_file_browser").at(-1)?.args).toMatchObject({
          request: {
            directoryPath: "Campaigns",
          },
        });
      });

      const importFile = new File([], "new-shot.png");
      Object.defineProperty(importFile, "path", {
        value: "C:/Import/new-shot.png",
      });
      await fireEvent.drop(browser, {
        dataTransfer: {
          files: [importFile],
        },
      });

      await waitFor(() => {
        expect(getInvokeCalls("import_entries").at(-1)?.args).toMatchObject({
          request: {
            parentPath: "Campaigns",
            sourcePaths: ["C:/Import/new-shot.png"],
          },
        });
      });
    } finally {
      vi.useRealTimers();
    }
  });

  it("拖回原目录原位置时静默跳过，混合批次仅导入有效条目", async () => {
    seedMockRepository();
    const workspace = useRepositoryWorkspace();
    workspace.setActivePanel("files");
    await renderApp();

    const browser = workspaceBrowser();
    const samePathFile = new File([], "cover-final.psd");
    Object.defineProperty(samePathFile, "path", {
      value: "C:/Mock/AnimeAssets/cover-final.psd",
    });

    const importCountBefore = getInvokeCalls("import_entries").length;
    await fireEvent.drop(browser, {
      dataTransfer: {
        files: [samePathFile],
      },
    });
    expect(getInvokeCalls("import_entries")).toHaveLength(importCountBefore);
    expect(workspace.error.value).toBeNull();

    const newFile = new File([], "new-shot.png");
    Object.defineProperty(newFile, "path", {
      value: "C:/Import/new-shot.png",
    });
    await fireEvent.drop(browser, {
      dataTransfer: {
        files: [samePathFile, newFile],
      },
    });

    await waitFor(() => {
      expect(getInvokeCalls("import_entries").at(-1)?.args).toMatchObject({
        request: {
          parentPath: "",
          sourcePaths: ["C:/Import/new-shot.png"],
        },
      });
    });
  });

  it("文件右键缩略图菜单支持选择自定义缩略图", async () => {
    seedMockRepository();
    selectMockFile("C:/Mock/ThumbSources/custom.png");
    const workspace = useRepositoryWorkspace();
    workspace.setActivePanel("files");
    await renderApp();

    const target = (await screen.findAllByText("cover-final.psd"))[0];
    await fireEvent.contextMenu(target);
    await fireEvent.click(await screen.findByRole("menuitem", { name: "自定义缩略图（选择文件）", hidden: true }));

    await waitFor(() => {
      expect(getInvokeCalls("ensure_thumbnail").at(-1)?.args).toMatchObject({
        request: {
          path: "cover-final.psd",
          action: "save",
          sourcePath: "C:/Mock/ThumbSources/custom.png",
        },
      });
    });
    expect(workspace.fileBrowser.value?.entries.find((entry) => entry.path === "cover-final.psd")?.thumbnailCustom).toBe(true);
  });

  it("侧栏选择本地文件夹时挂载已有目录而不是创建新仓库", async () => {
    seedMockRepository();
    selectMockFolder("C:/Mock/SelectedRepo");
    await renderApp();

    await fireEvent.click(screen.getByRole("button", { name: "资源库" }));
    await fireEvent.click(await screen.findByRole("button", { name: "添加资源库" }));
    await fireEvent.click(await screen.findByRole("button", { name: "本地文件夹" }));

    await waitFor(() => {
      expect(getInvokeCalls("attach_repository_folder").at(-1)?.args).toMatchObject({
        request: {
          path: "C:/Mock/SelectedRepo",
        },
      });
    });
    expect(getInvokeCalls("create_repository")).toHaveLength(0);
  });

  it("空状态拖入本地文件夹时挂载已有目录而不是创建新仓库", async () => {
    await renderApp();
    const dropZone = document.querySelector(".empty-state-page");
    expect(dropZone).toBeInstanceOf(HTMLElement);
    const folder = new File([], "EmptyRepo");
    Object.defineProperty(folder, "path", {
      value: "C:/Mock/EmptyRepo",
    });

    await fireEvent.drop(dropZone as HTMLElement, {
      dataTransfer: {
        files: [folder],
      },
    });

    await waitFor(() => {
      expect(getInvokeCalls("attach_repository_folder").at(-1)?.args).toMatchObject({
        request: {
          path: "C:/Mock/EmptyRepo",
        },
      });
    });
    expect(getInvokeCalls("create_repository")).toHaveLength(0);
  });

  it("没有仓库时也能在设置页管理插件", async () => {
    selectMockFile("C:/Mock/Plugins/sample-plugin.momoplug");
    const router = createMomoBakoRouter(createMemoryHistory());
    await router.push("/settings");
    await router.isReady();
    await useRepositoryWorkspace().ensureRepositoryWorkspace();

    render(App, {
      global: {
        plugins: [router],
        directives: {
          "context-menu": vContextMenu,
        },
      },
    });

    expect(await screen.findByRole("heading", { name: "插件管理" })).toBeInTheDocument();

    await fireEvent.click(screen.getByRole("button", { name: "从 .momoplug 安装" }));
    await waitFor(() => {
      expect(getInvokeCalls("install_plugin_from_archive").at(-1)?.args).toMatchObject({
        request: {
          packagePath: "C:/Mock/Plugins/sample-plugin.momoplug",
        },
      });
    });
    expect(await screen.findByText("插件已安装。")).toBeInTheDocument();
    const pluginCardTitle = await screen.findByText("Sample Plugin");
    const pluginCard = pluginCardTitle.closest(".extensions-workbench__card");
    expect(pluginCard).toBeInstanceOf(HTMLElement);

    await fireEvent.click(within(pluginCard as HTMLElement).getByRole("button", { name: "禁用" }));
    await waitFor(() => {
      expect(getInvokeCalls("set_plugin_enabled").at(-1)?.args).toMatchObject({
        request: {
          pluginId: "user.sample-plugin",
          enabled: false,
        },
      });
    });
    expect(await screen.findByText("插件已禁用。")).toBeInTheDocument();

    await fireEvent.click(within(pluginCard as HTMLElement).getByRole("button", { name: "删除" }));
    const deleteDialog = await screen.findByRole("dialog", { name: "删除插件" });
    await fireEvent.click(within(deleteDialog).getByRole("button", { name: "删除", exact: true }));
    await waitFor(() => {
      expect(getInvokeCalls("delete_plugin").at(-1)?.args).toMatchObject({
        pluginId: "user.sample-plugin",
      });
    });
    expect(await screen.findByText("插件已删除。")).toBeInTheDocument();
    expect(screen.queryByText("Sample Plugin")).not.toBeInTheDocument();
  });

  it("禁用预览插件后不再为对应文件分配预览组件", async () => {
    seedMockRepository();
    const workspace = useRepositoryWorkspace();
    await workspace.ensureRepositoryWorkspace();
    await workspace.loadSettingsData({ failFast: true });

    expect(getPreviewPluginForEntry(previewEntry("mp4"))?.pluginId).toBe("momobako.preview.media");
    expect(getPreviewPluginForEntry(previewEntry("vrm"))?.pluginId).toBe("momobako.preview.three-model");

    await workspace.setPluginEnabledInWorkspace("momobako.preview.media", false);
    await workspace.setPluginEnabledInWorkspace("momobako.preview.three-model", false);

    expect(getPreviewPluginForEntry(previewEntry("mp4"))).toBeNull();
    expect(getPreviewPluginForEntry(previewEntry("mp3"))).toBeNull();
    expect(getPreviewPluginForEntry(previewEntry("vrm"))).toBeNull();
  });

  it("播放插件缺失时播放集仍可见，但播放操作禁用", async () => {
    seedMockRepository();
    seedMockPlaylists([audioPlaylist()], {
      "playlist-mock": audioPlaylistDetail(),
    });
    seedMockPlugins(createMockPlugins().map((plugin) => (
      plugin.pluginId === "momobako.preview.media"
        ? { ...plugin, enabled: false, status: "disabled" as const }
        : plugin
    )));

    await renderApp();

    expect(await screen.findByText("Mock Playlist")).toBeInTheDocument();
    const sidebarPlayButtons = screen.getAllByRole("button", { name: "播放播放集" });
    expect(sidebarPlayButtons[0]).toBeDisabled();

    await fireEvent.click(screen.getByRole("button", { name: /Mock Playlist/ }));
    const heading = await screen.findByRole("heading", { name: "Mock Playlist" });
    expect(heading).toBeInTheDocument();
    expect(screen.getByText(/缺少对应播放插件/)).toBeInTheDocument();
    const playlistPage = heading.closest(".playlist-page__panel");
    expect(playlistPage).toBeInstanceOf(HTMLElement);
    const playButtons = within(playlistPage as HTMLElement).getAllByRole("button", { name: "播放" });
    expect(playButtons.every((button) => button.hasAttribute("disabled"))).toBe(true);
  });

  it("侧栏播放播放集只启动播放而不切换主窗体页面", async () => {
    seedMockRepository();
    seedMockPlaylists([audioPlaylist()], {
      "playlist-mock": audioPlaylistDetail(),
    });

    await renderApp();

    await fireEvent.click(await screen.findByRole("button", { name: "播放播放集" }));

    await waitFor(() => {
      expect(getInvokeCalls("get_playlist_detail").at(-1)?.args).toMatchObject({
        repoId: "repo-main-001",
        playlistId: "playlist-mock",
      });
    });
    expect(document.querySelector(".files-browser")).toBeInTheDocument();
    expect(document.querySelector(".playlist-page")).not.toBeInTheDocument();
    expect(document.querySelector(".workspace-player")).toBeInTheDocument();
    expect(screen.getByText("asset-01.mp3")).toBeInTheDocument();
  });

  it("文件右键菜单支持通过复选项设置播放集成员关系", async () => {
    seedMockRepository();
    const playlist: PlaylistSummary = {
      ...audioPlaylist(),
      name: "图片收藏",
      playerTypeId: "momobako.playlist.image-slideshow",
      playerLabel: "图片幻灯片",
      fileClass: "image",
    };
    const detail: PlaylistDetail = {
      playlist,
      items: [],
    };
    seedMockPlaylists([playlist], {
      [playlist.playlistId]: detail,
    });

    await renderApp();

    await fireEvent.doubleClick(fileListItem("Backgrounds"));
    const fileItem = fileListItem("Backgrounds/scene-forest-03.png");
    await fireEvent.contextMenu(fileItem);
    expect(await screen.findByText("添加到播放集")).toBeInTheDocument();

    await fireEvent.click(screen.getByRole("menuitem", { name: "图片收藏" }));
    await waitFor(() => {
      expect(getInvokeCalls("set_playlist_membership").at(-1)?.args).toMatchObject({
        request: {
          repoId: "repo-main-001",
          assetId: "asset-02",
          playlistIds: ["playlist-mock"],
        },
      });
    });
  });

  it("播放集列表支持拖拽排序并保存顺序", async () => {
    seedMockRepository();
    const playlist = audioPlaylist();
    seedMockPlaylists([playlist], {
      [playlist.playlistId]: {
        playlist: {
          ...playlist,
          itemCount: 2,
        },
        items: [
          {
            playlistItemId: "playlist-item-1",
            playlistId: playlist.playlistId,
            assetId: "asset-01",
            path: "asset-01.mp3",
            filename: "asset-01.mp3",
            extension: "mp3",
            thumbnailPath: null,
            status: "ready",
            statusReason: null,
            sortOrder: 0,
            addedAt: "2026-06-05T00:18:00Z",
          },
          {
            playlistItemId: "playlist-item-2",
            playlistId: playlist.playlistId,
            assetId: "asset-02",
            path: "asset-02.mp3",
            filename: "asset-02.mp3",
            extension: "mp3",
            thumbnailPath: null,
            status: "ready",
            statusReason: null,
            sortOrder: 1,
            addedAt: "2026-06-05T00:19:00Z",
          },
        ],
      },
    });

    await renderApp();

    await fireEvent.click(screen.getByRole("button", { name: /Mock Playlist/ }));
    const items = document.querySelectorAll(".playlist-page__item");
    expect(items).toHaveLength(2);

    await fireEvent.dragStart(items[0]);
    await fireEvent.drop(items[1]);

    await waitFor(() => {
      expect(getInvokeCalls("reorder_playlist_items").at(-1)?.args).toMatchObject({
        request: {
          repoId: "repo-main-001",
          playlistId: "playlist-mock",
          itemIds: ["playlist-item-2", "playlist-item-1"],
        },
      });
    });
  });

  it("当前播放项预览使用可见播放挂载并保存媒体显示设置", async () => {
    seedMockRepository();
    const playlist: PlaylistSummary = {
      ...audioPlaylist(),
      name: "图片播放",
      playerTypeId: "momobako.playlist.image-slideshow",
      playerLabel: "图片幻灯片",
      fileClass: "image",
      itemCount: 1,
    };
    seedMockPlaylists([playlist], {
      [playlist.playlistId]: {
        playlist,
        items: [{
          playlistItemId: "image-item-1",
          playlistId: playlist.playlistId,
          assetId: "asset-02",
          path: "cover-final.psd",
          filename: "scene-forest-03.png",
          extension: "png",
          thumbnailPath: null,
          status: "ready",
          statusReason: null,
          sortOrder: 0,
          addedAt: "2026-06-05T00:18:00Z",
        }],
      },
    });

    await renderApp();
    await fireEvent.click(screen.getByRole("button", { name: /图片播放/ }));
    const playlistPage = await screen.findByRole("heading", { name: "图片播放" });
    await fireEvent.click(within(playlistPage.closest(".playlist-page__panel") as HTMLElement).getAllByRole("button", { name: "播放" })[0]);

    expect(screen.getByRole("heading", { name: "图片播放" })).toBeInTheDocument();
    expect(document.querySelector(".files-preview-page__player-mount")).not.toBeInTheDocument();
    await fireEvent.click((playlistPage.closest(".playlist-page__panel") as HTMLElement).querySelector(".workspace-player__media") as HTMLElement);

    await waitFor(() => {
      expect(document.querySelector(".files-preview-page__player-mount .mock-playlist-runtime-image")).toBeInTheDocument();
    });
    expect(screen.getByRole("heading", { name: "cover-final.psd" })).toBeInTheDocument();

    await fireEvent.update(screen.getByLabelText("图片停留时长"), "7");
    await fireEvent.click(screen.getByRole("radio", { name: "填充" }));

    expect(localStorage.getItem("momobako.playbackSettings")).toContain("\"imageDurationMs\":7000");
    expect(localStorage.getItem("momobako.playbackSettings")).toContain("\"objectFit\":\"cover\"");
  });

  it("保留目录按需加载，并在结构变化后刷新文件夹树", async () => {
    seedMockRepository();
    const workspace = useRepositoryWorkspace();
    workspace.setActivePanel("files");
    await renderApp();

    expect(screen.getByRole("searchbox", { name: "全局搜索" })).toBeInTheDocument();
    expect(screen.queryByText("快捷方式")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: /全部/ })).toBeInTheDocument();
    const settingsButton = screen.getByRole("link", { name: "设置" });
    const extensionsButton = screen.getByRole("button", { name: "拓展" });

    await waitFor(() => {
      const browserCalls = getInvokeCalls("get_file_browser");
      expect(browserCalls.at(-1)?.args).toMatchObject({
        request: {
          directoryPath: "",
          includeTree: true,
        },
      });
    });

    await fireEvent.update(screen.getByRole("searchbox", { name: "全局搜索" }), "cover");
    await waitFor(() => {
      const searchCalls = getInvokeCalls("search_assets");
      expect(searchCalls.at(-1)?.args).toMatchObject({
        request: {
          query: "cover",
        },
      });
    });
    expect(await screen.findByRole("heading", { name: "搜索结果" })).toBeInTheDocument();
    expect(await screen.findByText("cover-final.psd")).toBeInTheDocument();

    await fireEvent.click(screen.getByRole("button", { name: "显示筛选栏" }));
    expect(await screen.findByLabelText("资源筛选")).toBeInTheDocument();
    await fireEvent.click(screen.getByRole("button", { name: "psd" }));
    await waitFor(() => {
      const searchCalls = getInvokeCalls("search_assets");
      expect(searchCalls.at(-1)?.args).toMatchObject({
        request: {
          query: "cover",
          repoId: "repo-main-001",
          formats: ["psd"],
        },
      });
    });

    await fireEvent.click(screen.getByRole("button", { name: "封面" }));
    await waitFor(() => {
      const searchCalls = getInvokeCalls("search_assets");
      expect(searchCalls.at(-1)?.args).toMatchObject({
        request: {
          repoId: "repo-main-001",
          formats: ["psd"],
          tags: ["封面"],
        },
      });
    });

    await fireEvent.click(screen.getByRole("button", { name: "5 星+" }));
    await waitFor(() => {
      const searchCalls = getInvokeCalls("search_assets");
      expect(searchCalls.at(-1)?.args).toMatchObject({
        request: {
          repoId: "repo-main-001",
          formats: ["psd"],
          tags: ["封面"],
          minRating: 5,
        },
      });
    });

    const hexColorButton = screen.getByRole("button", { name: "#336699" });
    expect(hexColorButton.style.getPropertyValue("--filter-swatch")).toBe("#336699");
    await fireEvent.click(hexColorButton);
    await waitFor(() => {
      const searchCalls = getInvokeCalls("search_assets");
      expect(searchCalls.at(-1)?.args).toMatchObject({
        request: {
          repoId: "repo-main-001",
          metadataFilters: [
            { key: "color", value: "#336699" },
          ],
        },
      });
    });

    await fireEvent.click(screen.getByRole("button", { name: "方形" }));
    await waitFor(() => {
      const searchCalls = getInvokeCalls("search_assets");
      expect(searchCalls.at(-1)?.args).toMatchObject({
        request: {
          repoId: "repo-main-001",
          metadataFilters: [
            { key: "color", value: "#336699" },
            { key: "shape", value: "方形" },
          ],
        },
      });
    });

    await fireEvent.update(screen.getByLabelText("排除标签"), "草稿，临时");
    await fireEvent.update(screen.getByLabelText("排除格式"), "gif");
    await fireEvent.update(screen.getByLabelText("排除元数据"), "status=archived");
    await fireEvent.update(screen.getByLabelText("数值范围"), "width=1024..4096, originalSizeBytes=..10485760");
    await fireEvent.update(screen.getByLabelText("日期范围"), "fileCreatedAt=2024-01-01T00:00:00Z..");
    await fireEvent.update(screen.getByLabelText("排序字段"), "metadata.width");
    await fireEvent.update(screen.getByLabelText("排序方向"), "desc");
    await fireEvent.update(screen.getByLabelText("结果数量"), "10");
    await fireEvent.click(screen.getByRole("button", { name: "应用" }));
    await waitFor(() => {
      const searchCalls = getInvokeCalls("search_assets");
      expect(searchCalls.at(-1)?.args).toMatchObject({
        request: {
          repoId: "repo-main-001",
          excludeTags: ["草稿", "临时"],
          excludeFormats: ["gif"],
          excludeMetadataFilters: [{ key: "status", value: "archived" }],
          numberFilters: [
            { key: "width", min: 1024, max: 4096 },
            { key: "originalSizeBytes", max: 10485760 },
          ],
          dateFilters: [{ key: "fileCreatedAt", from: "2024-01-01T00:00:00Z" }],
          sort: { field: "metadata.width", direction: "desc" },
          limit: 10,
        },
      });
    });

    await fireEvent.click(screen.getByRole("button", { name: "清除" }));
    await waitFor(() => {
      const searchCalls = getInvokeCalls("search_assets");
      expect(searchCalls.at(-1)?.args).toMatchObject({
        request: {
          query: "cover",
        },
      });
      expect(searchCalls.at(-1)?.args).not.toMatchObject({
        request: {
          repoId: "repo-main-001",
        },
      });
    });

    await fireEvent.click(extensionsButton);
    expect(await screen.findByRole("heading", { name: "文件系统与插件" })).toBeInTheDocument();

    workspace.setActivePanel("files");
    await screen.findAllByText("Campaigns");
    await fireEvent.click(screen.getAllByText("Campaigns")[0]);
    expect((await screen.findAllByText("Summer")).length).toBeGreaterThan(0);
    let browserCalls = getInvokeCalls("get_file_browser");
    expect(browserCalls.at(-1)?.args).toMatchObject({
      request: {
        directoryPath: "Campaigns",
        includeTree: false,
      },
    });

    await fireEvent.click(await screen.findByRole("button", { name: "在当前目录新建文件夹" }));
    await fireEvent.update(screen.getByPlaceholderText("输入文件夹名称"), "Layouts");
    await fireEvent.click(screen.getByRole("button", { name: "创建" }));
    expect((await screen.findAllByText("Layouts")).length).toBeGreaterThan(0);

    await workspace.importEntriesToWorkspace(["C:/Import/Storyboards"]);
    expect((await screen.findAllByText("Storyboards")).length).toBeGreaterThan(0);

    createDirectoryOnNextSync("ExternalSync");
    await workspace.syncActiveRepository();
    await waitFor(() => {
      browserCalls = getInvokeCalls("get_file_browser");
      expect(browserCalls.at(-1)?.args).toMatchObject({
        request: {
          directoryPath: "Campaigns",
          includeTree: true,
        },
      });
    });
    expect((await screen.findAllByText("ExternalSync")).length).toBeGreaterThan(0);

    await fireEvent.click(settingsButton);
    expect(await screen.findByRole("heading", { name: "设置" })).toBeInTheDocument();
  });

  it("点击跨仓库搜索结果后切到文件视图并预览命中文件", async () => {
    seedCrossRepositorySearchHit();
    await renderApp();

    await fireEvent.update(screen.getByRole("searchbox", { name: "全局搜索" }), "target");
    expect(await screen.findByRole("heading", { name: "搜索结果" })).toBeInTheDocument();

    await fireEvent.click(await screen.findByRole("button", { name: /target-preview\.png/ }));

    await waitFor(() => {
      expect(getInvokeCalls("get_repository_snapshot").at(-1)?.args).toMatchObject({
        repoId: "repo-alt-001",
      });
    });
    await waitFor(() => {
      expect(getInvokeCalls("get_file_browser").at(-1)?.args).toMatchObject({
        request: {
          repoId: "repo-alt-001",
          directoryPath: "Reference/Paint",
          includeTree: true,
        },
      });
    });
    await waitFor(() => {
      expect(getInvokeCalls("get_asset_detail").at(-1)?.args).toMatchObject({
        repoId: "repo-alt-001",
        assetId: "asset-alt-01",
      });
    });

    expect(screen.queryByRole("heading", { name: "搜索结果" })).not.toBeInTheDocument();
    expect(await screen.findByRole("heading", { name: "target-preview.png" })).toBeInTheDocument();
    expect(screen.getByText("Reference/Paint/target-preview.png")).toBeInTheDocument();
    await waitFor(() => {
      expect(getInvokeCalls("prepare_preview_file_source").at(-1)?.args).toMatchObject({
        request: {
          repoId: "repo-alt-001",
          path: "Reference/Paint/target-preview.png",
        },
      });
    });
    await waitFor(() => {
      const previewImage = document.querySelector<HTMLImageElement>(".media-preview__image");
      expect(previewImage?.getAttribute("src")).toBe(`http://127.0.0.1:49152/preview/${"0".repeat(64)}`);
    });
  });

  it("智能文件夹使用虚拟文件列表并只提供只读导航操作", async () => {
    seedMockRepository();
    await renderApp();

    await fireEvent.click(screen.getByRole("button", { name: "新建智能文件夹" }));
    const dialog = await screen.findByRole("dialog", { name: "新建智能文件夹" });
    await fireEvent.update(within(dialog).getByLabelText("名称"), "高评分封面");
    await fireEvent.update(within(dialog).getByLabelText("路径前缀"), "Campaigns");
    await fireEvent.update(within(dialog).getByLabelText("格式"), "psd");
    await fireEvent.update(within(dialog).getByLabelText("标签"), "封面");
    await fireEvent.update(within(dialog).getByLabelText("最低评分"), "5");
    await fireEvent.click(within(dialog).getByRole("button", { name: "创建" }));

    await waitFor(() => {
      expect(getInvokeCalls("create_smart_folder").at(-1)?.args).toMatchObject({
        request: {
          repoId: "repo-main-001",
          name: "高评分封面",
          filter: {
            pathPrefix: "Campaigns",
            formats: ["psd"],
            tags: ["封面"],
            minRating: 5,
          },
        },
      });
    });
    await waitFor(() => {
      expect(getInvokeCalls("query_smart_folder").at(-1)?.args).toMatchObject({
        repoId: "repo-main-001",
        smartFolderId: "smart-1",
      });
    });

    expect((await screen.findAllByText("高评分封面")).length).toBeGreaterThan(0);
    expect(await screen.findByText("智能文件夹不会改变实际目录。")).toBeInTheDocument();
    expect((await screen.findAllByText("cover-final.psd")).length).toBeGreaterThan(0);
    expect(screen.queryByRole("button", { name: "建文件" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "重命名" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "删除" })).not.toBeInTheDocument();
  });

  it("智能文件夹表单发送高级过滤字段", async () => {
    seedMockRepository();
    await renderApp();

    await fireEvent.click(screen.getByRole("button", { name: "新建智能文件夹" }));
    const dialog = await screen.findByRole("dialog", { name: "新建智能文件夹" });
    await fireEvent.update(within(dialog).getByLabelText("名称"), "Eagle 高级条件");
    await fireEvent.update(within(dialog).getByLabelText("标签"), "封面");
    await fireEvent.update(within(dialog).getByLabelText("匹配方式"), "or");
    await fireEvent.update(within(dialog).getByLabelText("排除标签"), "草稿");
    await fireEvent.update(within(dialog).getByLabelText("排除格式"), "gif，webp");
    await fireEvent.update(within(dialog).getByLabelText("排除元数据"), "status=archived");
    await fireEvent.update(within(dialog).getByLabelText("数值范围"), "width=1024..4096\noriginalSizeBytes=..10485760");
    await fireEvent.update(within(dialog).getByLabelText("日期范围"), "fileCreatedAt=2024-01-01T00:00:00Z..2024-12-31T23:59:59Z");
    await fireEvent.update(within(dialog).getByLabelText("排序字段"), "metadata.width");
    await fireEvent.update(within(dialog).getByLabelText("排序方向"), "desc");
    await fireEvent.update(within(dialog).getByLabelText("结果数量"), "20");
    await fireEvent.click(within(dialog).getByRole("button", { name: "创建" }));

    await waitFor(() => {
      expect(getInvokeCalls("create_smart_folder").at(-1)?.args).toMatchObject({
        request: {
          repoId: "repo-main-001",
          name: "Eagle 高级条件",
          filter: {
            tags: ["封面"],
            matchMode: "or",
            excludeTags: ["草稿"],
            excludeFormats: ["gif", "webp"],
            excludeMetadataFilters: [{ key: "status", value: "archived" }],
            numberFilters: [
              { key: "width", min: 1024, max: 4096 },
              { key: "originalSizeBytes", max: 10485760 },
            ],
            dateFilters: [{ key: "fileCreatedAt", from: "2024-01-01T00:00:00Z", to: "2024-12-31T23:59:59Z" }],
            sort: { field: "metadata.width", direction: "desc" },
            limit: 20,
          },
        },
      });
    });
  });

  it("使用本地绝对路径打开和定位文件，并展示 opener 失败信息", async () => {
    seedMockRepositoryPath("C:\\Mock\\AnimeAssets\\");
    const workspace = useRepositoryWorkspace();
    workspace.setActivePanel("files");
    await renderApp();

    await workspace.openWorkspaceEntry("Campaigns/Summer/cover.psd");
    expect(getOpenerCalls("openPath").at(-1)).toMatchObject({
      path: "C:\\Mock\\AnimeAssets\\Campaigns\\Summer\\cover.psd",
    });

    await workspace.revealWorkspaceEntry("Campaigns");
    expect(getOpenerCalls("revealItemInDir").at(-1)).toMatchObject({
      path: "C:\\Mock\\AnimeAssets\\Campaigns",
    });

    failNextOpenerCall("系统找不到指定的路径");
    await workspace.openWorkspaceEntry("missing.psd");
    expect((await screen.findAllByText("系统找不到指定的路径")).length).toBeGreaterThan(0);
  });

  it("支持折叠、拖拽调整和重置侧边栏宽度", async () => {
    seedMockRepository();
    const workspace = useRepositoryWorkspace();
    workspace.setActivePanel("files");
    await renderApp();

    const shell = shellElement();
    const toggleButton = screen.getByRole("button", { name: "折叠侧边栏" });
    const resizer = screen.getByRole("separator", { name: "拖动调整侧边栏宽度（双击恢复默认）" });

    expect(shell).not.toHaveClass("is-sidebar-collapsed");
    expect(shell.style.getPropertyValue("--sidebar-width")).toBe("276px");

    await fireEvent.click(toggleButton);
    expect(shell).toHaveClass("is-sidebar-collapsed");
    expect(localStorage.getItem("momobako.sidebarCollapsed")).toBe("1");
    expect(screen.getByRole("button", { name: "展开侧边栏" })).toBeInTheDocument();

    await fireEvent.click(screen.getByRole("button", { name: "展开侧边栏" }));
    expect(shell).not.toHaveClass("is-sidebar-collapsed");
    expect(localStorage.getItem("momobako.sidebarCollapsed")).toBe("0");

    resizer.dispatchEvent(pointerEvent("pointerdown", 276));
    window.dispatchEvent(pointerEvent("pointermove", 420));
    window.dispatchEvent(pointerEvent("pointerup", 420));
    await waitFor(() => {
      expect(shell.style.getPropertyValue("--sidebar-width")).toBe("420px");
    });
    expect(localStorage.getItem("momobako.sidebarWidth")).toBe("420");

    resizer.dispatchEvent(pointerEvent("pointerdown", 420));
    window.dispatchEvent(pointerEvent("pointermove", 1000));
    window.dispatchEvent(pointerEvent("pointerup", 1000));
    await waitFor(() => {
      expect(shell.style.getPropertyValue("--sidebar-width")).toBe("480px");
    });
    expect(localStorage.getItem("momobako.sidebarWidth")).toBe("480");

    await fireEvent.dblClick(resizer);
    expect(shell.style.getPropertyValue("--sidebar-width")).toBe("276px");
    expect(localStorage.getItem("momobako.sidebarWidth")).toBe("276");
  });

  it("通过资源库下拉菜单二次确认删除当前资源库", async () => {
    seedMockRepository();
    await renderApp();

    await fireEvent.click(await screen.findByRole("button", { name: "资源库" }));
    await fireEvent.click(await screen.findByRole("button", { name: "删除当前资源库" }));
    expect(screen.getByRole("button", { name: "确认删除当前资源库" })).toBeInTheDocument();
    expect(getInvokeCalls("delete_repository")).toHaveLength(0);
    await fireEvent.click(screen.getByRole("button", { name: "确认删除当前资源库" }));

    await waitFor(() => {
      expect(getInvokeCalls("delete_repository").at(-1)?.args).toMatchObject({
        repoId: "repo-main-001",
      });
    });
  });
});
