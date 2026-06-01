import { render, screen } from "@testing-library/vue";
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
  it("默认首页显示模板占位内容", async () => {
    await renderAt("/");

    expect(
      await screen.findByRole("heading", { level: 1, name: "Tauri 应用模板" }),
    ).toBeInTheDocument();
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

  it("未知路由回到首页", async () => {
    await renderAt("/missing");

    expect(await screen.findByText("从这里开始替换成你的业务页面。")).toBeInTheDocument();
  });
});
