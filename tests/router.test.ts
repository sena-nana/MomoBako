import { fireEvent, render, screen, waitFor } from "@testing-library/vue";
import { createMemoryHistory } from "vue-router";
import { describe, expect, it } from "vitest";
import App from "../src/App.vue";
import { useRepositoryWorkspace } from "../src/composables/useRepositoryWorkspace";
import { createTemplateRouter } from "../src/router";
import {
  createDirectoryOnNextSync,
  getInvokeCalls,
  seedMockRepository,
} from "./setupTests";

async function renderApp() {
  const router = createTemplateRouter(createMemoryHistory());
  await router.push("/");
  await router.isReady();

  render(App, {
    global: {
      plugins: [router],
    },
  });
}

describe("文件管理冒烟", () => {
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
});
