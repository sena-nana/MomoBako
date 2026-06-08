import { fireEvent, render, screen, waitFor } from "@testing-library/vue";
import { createMemoryHistory } from "vue-router";
import { describe, expect, it } from "vitest";
import App from "../src/App.vue";
import { installContextMenu } from "../src/composables/useContextMenu";
import { resetRepositoryWorkspaceForTests, useRepositoryWorkspace } from "../src/composables/useRepositoryWorkspace";
import { vContextMenu } from "../src/directives/contextMenu";
import { createTemplateRouter } from "../src/router";
import {
  createDirectoryOnNextSync,
  delayNextInvoke,
  failNextInvoke,
  failNextOpenerCall,
  getInvokeCalls,
  getOpenerCalls,
  seedCrossRepositorySearchHit,
  seedMockRepository,
  setMockSavePath,
  seedMockRepositoryPath,
  selectMockFile,
  selectMockFolder,
} from "./setupTests";

async function renderApp() {
  resetRepositoryWorkspaceForTests();
  installContextMenu();
  const router = createTemplateRouter(createMemoryHistory());
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
}

async function renderAppWithoutStartupPreload() {
  resetRepositoryWorkspaceForTests();
  installContextMenu();
  const router = createTemplateRouter(createMemoryHistory());
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

function shellElement() {
  const shell = document.querySelector(".shell");
  expect(shell).toBeInstanceOf(HTMLElement);
  return shell as HTMLElement;
}

function pointerEvent(type: string, clientX: number) {
  return new PointerEvent(type, {
    bubbles: true,
    button: 0,
    clientX,
    pointerId: 1,
  });
}

describe("文件管理冒烟", () => {
  it("启动加载显示进度，完成后再显示仓库内容", async () => {
    seedMockRepository();
    const delay = delayNextInvoke("list_repositories");
    await renderAppWithoutStartupPreload();

    expect(screen.getByRole("heading", { name: "加载仓库列表" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "资源库" })).not.toBeInTheDocument();

    delay.resolve();
    expect(await screen.findByRole("button", { name: "资源库" })).toBeInTheDocument();
    expect((await screen.findAllByText("Campaigns")).length).toBeGreaterThan(0);
    expect(screen.queryByRole("heading", { name: "加载仓库列表" })).not.toBeInTheDocument();
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

    await fireEvent.click(screen.getByRole("button", { name: "添加资源库" }));
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
    expect(settingsButton.compareDocumentPosition(extensionsButton)).toBe(Node.DOCUMENT_POSITION_FOLLOWING);

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

  it("通过导出弹窗提交资源库压缩包选项", async () => {
    seedMockRepository();
    setMockSavePath("C:/Mock/Exports/Momo.zip");
    const workspace = useRepositoryWorkspace();
    await renderApp();
    workspace.setActivePanel("libraries");

    await fireEvent.click(await screen.findByRole("button", { name: "导出" }));
    expect(await screen.findByRole("dialog", { name: "导出资源库" })).toBeInTheDocument();
    await fireEvent.click(screen.getByRole("button", { name: "导出压缩包" }));

    await waitFor(() => {
      const exportCalls = getInvokeCalls("export_repository");
      expect(exportCalls.at(-1)?.args).toMatchObject({
        request: {
          repoId: "repo-main-001",
          target: "archive",
          archive: {
            format: "zip",
            outputPath: "C:/Mock/Exports/Momo.zip",
            compression: "balanced",
            encrypt: false,
          },
        },
      });
    });
  });
});
