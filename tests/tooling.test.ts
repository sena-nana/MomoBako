import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

describe("单应用模板工具链", () => {
  it("根 package.json 直接提供单应用脚本，不包含 workspace", () => {
    const pkg = JSON.parse(readFileSync(resolve("package.json"), "utf-8"));

    expect(pkg.workspaces).toBeUndefined();
    expect(pkg.packageManager).toBe("yarn@4.14.1");
    expect(pkg.scripts).toMatchObject({
      dev: "vite",
      build: "vue-tsc --noEmit && vite build",
      test: "vitest run",
      tauri: "tauri",
      "tauri:dev": "tauri dev",
      "tauri:build": "tauri build",
      verify: "yarn test && yarn build && cargo check --manifest-path src-tauri/Cargo.toml",
    });
  });

  it("只保留通用 Tauri/Vue 依赖，不包含 Lilia agent 业务依赖", () => {
    const pkg = JSON.parse(readFileSync(resolve("package.json"), "utf-8"));
    const deps = { ...pkg.dependencies, ...pkg.devDependencies };

    expect(deps.vue).toBeDefined();
    expect(deps["vue-router"]).toBeDefined();
    expect(deps["@tauri-apps/api"]).toBeDefined();
    expect(deps["@tauri-apps/plugin-store"]).toBeDefined();
    expect(deps["@anthropic-ai/claude-agent-sdk"]).toBeUndefined();
    expect(deps["@openai/codex-sdk"]).toBeUndefined();
    expect(deps["@modelcontextprotocol/sdk"]).toBeUndefined();
    expect(deps["@lilia/contracts"]).toBeUndefined();
    expect(deps.zod).toBeUndefined();
  });

  it("Rust 端只新增通用窗口状态 store 插件", () => {
    const cargo = readFileSync(resolve("src-tauri/Cargo.toml"), "utf-8");

    expect(cargo).toContain('tauri-plugin-store = "2"');
    expect(cargo).not.toContain("rusqlite");
    expect(cargo).not.toContain("r2d2");
    expect(cargo).not.toContain("reqwest");
  });
});
