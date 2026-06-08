import { fireEvent, render, screen, waitFor } from "@testing-library/vue";
import { createMemoryHistory } from "vue-router";
import { describe, expect, it } from "vitest";
import App from "../src/App.vue";
import { useRepositoryWorkspace } from "../src/composables/useRepositoryWorkspace";
import { createTemplateRouter } from "../src/router";
import {
  createDirectoryOnNextSync,
  failNextOpenerCall,
  getInvokeCalls,
  getOpenerCalls,
  seedMockRepository,
  setMockSavePath,
  seedMockRepositoryPath,
  selectMockFolder,
} from "./setupTests";

async function renderApp() {
  const router = createTemplateRouter(createMemoryHistory());
  await router.push("/");
  await router.isReady();
  await useRepositoryWorkspace().refreshRepositoryWorkspace();

  render(App, {
    global: {
      plugins: [router],
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
    workspace.setActivePanel("libraries");
    await renderApp();

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
