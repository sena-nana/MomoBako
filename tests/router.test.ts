import { fireEvent, render, screen, waitFor } from "@testing-library/vue";
import { createMemoryHistory } from "vue-router";
import { describe, expect, it } from "vitest";
import App from "../src/App.vue";
import { createTemplateRouter } from "../src/router";
import { getInvokeCalls, seedMockRepository } from "./setupTests";

async function renderAt(path: string) {
  const router = createTemplateRouter(createMemoryHistory());
  await router.push(path);
  await router.isReady();

  render(App, {
    global: {
      plugins: [router],
    },
  });
}

describe("基础路由", () => {
  it("默认首页在无仓库时显示建库引导", async () => {
    await renderAt("/");

    expect(await screen.findByRole("heading", { level: 1, name: "还没有可用资源库" })).toBeInTheDocument();
    expect(screen.getByText(/在左侧“资源库列表”点击 `\+` 选择文件夹/)).toBeInTheDocument();
  });

  it("侧边栏只保留左下角一个设置入口", async () => {
    await renderAt("/");

    expect(screen.getAllByRole("link", { name: "设置" })).toHaveLength(1);
  });

  it("设置页只保留通用偏好与应用信息", async () => {
    await renderAt("/settings");

    expect(await screen.findByRole("heading", { name: "设置" })).toBeInTheDocument();
    expect(screen.getByText("外观")).toBeInTheDocument();
    expect(screen.queryByText(/Claude|Codex|CC-Switch|agent/i)).toBeNull();
  });

  it("侧栏切换选项卡时主界面同步切换", async () => {
    await renderAt("/");

    await fireEvent.click(screen.getByRole("button", { name: "搜索" }));
    expect(await screen.findByRole("heading", { level: 1, name: "搜索结果" })).toBeInTheDocument();

    await fireEvent.click(screen.getByRole("button", { name: "拓展" }));
    expect(await screen.findByRole("heading", { level: 1, name: "文件系统与插件" })).toBeInTheDocument();
  });

  it("从设置页点击侧栏选项会返回主工作区", async () => {
    await renderAt("/settings");

    await fireEvent.click(screen.getByRole("button", { name: "搜索" }));
    expect(await screen.findByRole("heading", { level: 1, name: "搜索结果" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { level: 1, name: "设置" })).toBeNull();
  });

  it("未知路由回到首页", async () => {
    await renderAt("/missing");

    expect(await screen.findByRole("button", { name: "资源库" })).toBeInTheDocument();
    expect(screen.getAllByRole("link", { name: "设置" })).toHaveLength(1);
  });

  it("切换目录只读取当前目录，刷新按钮才重建文件夹树", async () => {
    seedMockRepository();
    await renderAt("/");

    await fireEvent.click(screen.getByRole("button", { name: "刷新资源库" }));
    await fireEvent.click(await screen.findByRole("button", { name: "文件管理" }));
    expect(await screen.findByText("Campaigns")).toBeInTheDocument();

    let browserCalls = getInvokeCalls("get_file_browser");
    expect(browserCalls.at(-1)?.args).toMatchObject({
      request: {
        directoryPath: "",
        includeTree: true,
      },
    });

    await fireEvent.click(screen.getAllByText("Campaigns")[0]);
    expect((await screen.findAllByText("Summer")).length).toBeGreaterThan(0);
    browserCalls = getInvokeCalls("get_file_browser");
    expect(browserCalls.at(-1)?.args).toMatchObject({
      request: {
        directoryPath: "Campaigns",
        includeTree: false,
      },
    });

    await fireEvent.click(screen.getByRole("button", { name: "刷新文件夹树" }));
    await waitFor(() => {
      expect(getInvokeCalls("sync_repository").at(-1)?.args).toMatchObject({
        request: {
          repoId: "repo-main-001",
        },
      });
    });
    await waitFor(() => {
      browserCalls = getInvokeCalls("get_file_browser");
      expect(browserCalls.at(-1)?.args).toMatchObject({
        request: {
          directoryPath: "Campaigns",
          includeTree: true,
        },
      });
    });
  });
});
