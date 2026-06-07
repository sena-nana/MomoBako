import { fireEvent, render, screen } from "@testing-library/vue";
import { createMemoryHistory } from "vue-router";
import { describe, expect, it } from "vitest";
import App from "../src/App.vue";
import { createTemplateRouter } from "../src/router";

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
});
