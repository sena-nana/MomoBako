import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

function read(path: string) {
  return readFileSync(resolve(path), "utf-8");
}

function expectNoSourceResidue(content: string) {
  expect(content).not.toContain("plugins/builtin/");
  expect(content).not.toContain("scripts/build-builtin-plugins.mjs");
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
      "plugins:build": "node scripts/build-external-plugins.mjs",
      "plugins:package": "node scripts/package-external-plugins.mjs",
      "plugins:stage:dev": "node scripts/stage-external-plugins.mjs",
      "tauri:dev:with-plugins": "yarn plugins:build && yarn plugins:package && yarn plugins:stage:dev && node scripts/tauri-dev.mjs",
      verify: "yarn test && yarn build && cargo check --manifest-path src-tauri/Cargo.toml",
    });
    expect(deps.vitepress).toBeDefined();
    expect(deps.jszip).toBeUndefined();
    expect(pkg.dependencies.three).toBeDefined();
    expect(pkg.dependencies["@pixiv/three-vrm"]).toBeDefined();
    expect(pkg.dependencies["@lilia/ui"]).toBe(
      "github:sena-nana/LiliaUI#workspace=@lilia/ui&commit=d8baeb0b97251ce973063c480f86b70f96b1ec07",
    );
    expect(pkg.dependencies["@lucide/vue"]).toBeDefined();
    expect(pkg.dependencies["lucide-vue-next"]).toBeUndefined();
    expect(pkg.devDependencies["@types/three"]).toBeDefined();
  });

  it("External/Plugins 提供标准开发入口并与主线源码隔离", () => {
    const devGuide = read("External/Plugins/dev.md");
    const pluginsPkg = JSON.parse(read("External/Plugins/package.json"));
    const templateManifest = read("External/Plugins/template/manifest.json");
    const templateProject = read("External/Plugins/template/plugin.project.json");
    const exampleManifest = read("External/Plugins/example/manifest.json");
    const exampleProject = read("External/Plugins/example/plugin.project.json");
    const tsconfig = read("tsconfig.json");

    expect(devGuide).toContain("External/Plugins/");
    expect(devGuide).toContain("External/Plugins/package.json");
    expect(devGuide).toContain("<serviceRoot>/plugins");
    expect(devGuide).toContain(".momoplug");
    expect(devGuide).toContain("register(ctx)");
    expect(devGuide).toContain("momobako_plugin_call");
    expect(pluginsPkg.scripts).toMatchObject({
      build: "node scripts/build.mjs",
      package: "node scripts/package.mjs",
      "stage:dev": "node scripts/stage-dev.mjs",
    });
    expect(read("External/Plugins/scripts/package.mjs")).toContain("no plugin build outputs found to package");
    expect(templateManifest).toContain("\"pluginId\"");
    expect(templateProject).toContain("\"build\"");
    expect(exampleManifest).toContain("\"pluginId\"");
    expect(exampleProject).toContain("\"build\"");
    expect(tsconfig).not.toContain("plugins/**/*.ts");
    expect(tsconfig).not.toContain("plugins/**/*.json");
  });

  it("GitHub CI 使用 MomoBako 验证和文档构建配置", () => {
    const ci = read(".github/workflows/ci.yml");

    expect(ci).toContain("Verify MomoBako");
    expect(ci).toContain("Build External Plugins");
    expect(ci).toContain("corepack yarn verify");
    expect(ci).toContain("working-directory: External/Plugins");
    expect(ci).toContain("corepack yarn package");
    expect(ci).toContain("corepack yarn docs:build");
    expect(ci).toContain("src-tauri/target");
    expectNoSourceResidue(ci);
  });

  it("GitHub release 发布 MomoBako Windows draft bundle", () => {
    const release = read(".github/workflows/release.yml");

    expect(release).toContain("Publish Windows Release");
    expect(release).toContain("corepack yarn verify");
    expect(release).toContain("Install external plugin dependencies");
    expect(release).toContain("Upload external plugin packages");
    expect(release).toContain("projectPath: .");
    expect(release).toContain("tauriScript: corepack yarn tauri");
    expect(release).toContain("releaseName: MomoBako");
    expect(release).toContain("releaseDraft: true");
  });

  it("GitHub Pages 使用文档产物和 Pages 权限", () => {
    const pages = read(".github/workflows/pages.yml");

    expect(pages).toContain("pages: write");
    expect(pages).toContain("id-token: write");
    expect(pages).toContain("corepack yarn docs:build");
    expect(pages).toContain("docs/.vitepress/dist");
    expect(pages).not.toContain("enablement: true");
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

  it("全局滚动条使用隐藏原生条和 overlay 显隐样式", () => {
    const styles = read("src/styles/index.css").replace(/\r\n/g, "\n");
    const main = read("src/main.ts");
    const scrollbars = read("src/ui/core/index.ts");

    expect(styles).toContain('@import "@lilia/ui/styles.css";');
    expect(main).toContain(
      'installGlobalScrollbarVisibility',
    );
    expect(main).toContain('from "./ui/core"');
    expect(scrollbars).toContain("installGlobalScrollbarVisibility");
    expect(scrollbars).toContain("uninstallGlobalScrollbarVisibility");
  });

  it("LiliaUI 字体资源随样式入口一起提供", () => {
    const fonts = [
      "public/fonts/noto-sans-sc-chinese-simplified-400-normal.woff2",
      "public/fonts/noto-sans-sc-chinese-simplified-500-normal.woff2",
      "public/fonts/noto-sans-sc-chinese-simplified-600-normal.woff2",
      "public/fonts/noto-sans-sc-chinese-simplified-700-normal.woff2",
    ];

    for (const fontPath of fonts) {
      expect(readFileSync(resolve(fontPath))).toBeDefined();
    }
  });
});
