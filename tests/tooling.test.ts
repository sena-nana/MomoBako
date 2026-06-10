import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

function read(path: string) {
  return readFileSync(resolve(path), "utf-8");
}

function expectNoSourceResidue(content: string) {
  expect(content).not.toContain("Tauri Template");
  expect(content).not.toContain("Lilia");
}

describe("MomoBako 工具链", () => {
  it("package.json 提供文档脚本并保留插件验证链路", () => {
    const pkg = JSON.parse(read("package.json"));
    const deps = { ...pkg.dependencies, ...pkg.devDependencies };

    expect(pkg.packageManager).toBe("yarn@4.14.1");
    expect(pkg.scripts).toMatchObject({
      "docs:dev": "vitepress dev docs",
      "docs:build": "vitepress build docs",
      "docs:preview": "vitepress preview docs",
      "plugins:build": "node scripts/build-builtin-plugins.mjs",
      verify:
        "yarn test && yarn build && yarn plugins:build && cargo check --manifest-path src-tauri/Cargo.toml",
    });
    expect(deps.vitepress).toBeDefined();
  });

  it("GitHub CI 使用 MomoBako 验证和文档构建配置", () => {
    const ci = read(".github/workflows/ci.yml");

    expect(ci).toContain("Verify MomoBako");
    expect(ci).toContain("corepack yarn verify");
    expect(ci).toContain("corepack yarn docs:build");
    expect(ci).toContain("src-tauri/target");
    expect(ci).toContain("plugins/backend-sdk/target");
    expect(ci).toContain("plugins/builtin/local-filesystem/target");
    expectNoSourceResidue(ci);
  });

  it("GitHub release 发布 MomoBako Windows draft bundle", () => {
    const release = read(".github/workflows/release.yml");

    expect(release).toContain("Publish Windows Release");
    expect(release).toContain("corepack yarn verify");
    expect(release).toContain("projectPath: .");
    expect(release).toContain("tauriScript: corepack yarn tauri");
    expect(release).toContain("releaseName: MomoBako");
    expect(release).toContain("releaseDraft: true");
    expectNoSourceResidue(release);
  });

  it("GitHub Pages 使用文档产物和 Pages 权限", () => {
    const pages = read(".github/workflows/pages.yml");

    expect(pages).toContain("pages: write");
    expect(pages).toContain("id-token: write");
    expect(pages).toContain("corepack yarn docs:build");
    expect(pages).toContain("docs/.vitepress/dist");
    expect(pages).not.toContain("enablement: true");
  });

  it("GitHub Issue 模板使用 MomoBako 领域字段", () => {
    const bug = read(".github/ISSUE_TEMPLATE/bug_report.yml");
    const feature = read(".github/ISSUE_TEMPLATE/feature_request.yml");
    const combined = `${bug}\n${feature}`;

    expect(combined).toContain("MomoBako 版本 / commit");
    expect(combined).toContain("资源库 / 数据模型");
    expect(combined).toContain("文件同步 / 导入导出");
    expect(combined).toContain("预览 / 缩略图");
    expect(combined).toContain("插件");
    expect(combined).toContain("Tauri / Rust 后端");
    expect(combined).toContain("构建 / 发布");
    expect(combined).toContain("UI / 主题");
    expectNoSourceResidue(combined);
  });

  it("VitePress 文档站使用现有 MomoBako 文档入口", () => {
    const config = read("docs/.vitepress/config.ts");
    const index = read("docs/index.md");

    expect(config).toContain('title: "MomoBako"');
    expect(config).toContain('link: "/architecture"');
    expect(config).toContain('link: "/api-design"');
    expect(config).toContain('link: "/design/style-standard"');
    expect(index).toContain("[架构](./architecture.md)");
    expect(index).toContain("[API 设计](./api-design.md)");
    expect(index).toContain("[样式标准](./design/style-standard.md)");
  });
});
