import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { describe, expect, it } from "vitest";

import { resolveYarnCommand } from "../scripts/yarn-command.ts";

function read(path: string) {
  return readFileSync(resolve(path), "utf-8");
}

/** 插件目录里的 JavaScript 源码；构建产物、依赖和 Rust 编译目录不算。 */
function javascriptSources(directory: string): string[] {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) {
      return ["node_modules", "target", "dist"].includes(entry.name) ? [] : javascriptSources(path);
    }
    return /\.(?:js|mjs|cjs|ts|vue)$/.test(entry.name) ? [path] : [];
  });
}

function expectNoSourceResidue(content: string) {
  expect(content).not.toContain("plugins/builtin/");
  expect(content).not.toContain("scripts/build-builtin-plugins.mjs");
}

/** 校验 workflow 的 Rust 缓存跟随根 Cargo workspace：只缓存根 target，key 由 rustc 版本和根 Cargo.lock 决定。 */
function expectWorkspaceRustCache(workflow: string, cachePrefixes: string[]) {
  const steps = workflow.replace(/\r\n/g, "\n").split(/\n\s+- name: /);
  const toolchainSteps = steps.filter((step) => step.includes("uses: dtolnay/rust-toolchain@"));
  const cacheSteps = steps.filter((step) => step.includes("uses: actions/cache@"));

  expect(workflow).toContain("CARGO_INCREMENTAL: 0");
  expect(toolchainSteps).toHaveLength(cachePrefixes.length);
  expect(cacheSteps).toHaveLength(cachePrefixes.length);
  for (const step of toolchainSteps) {
    expect(step).toContain("id: toolchain");
  }
  cacheSteps.forEach((step, index) => {
    const restoreKey = `\${{ runner.os }}-${cachePrefixes[index]}-\${{ steps.toolchain.outputs.cachekey }}-`;

    expect(step).toMatch(/^\s+target$/m);
    expect(step).not.toMatch(/\/target$/m);
    expect(step).toContain(`key: ${restoreKey}\${{ hashFiles('Cargo.lock') }}`);
    expect(step.match(/restore-keys: \|\n\s+(.+)/)?.[1]).toBe(restoreKey);
  });
}

/** 取根 Cargo.toml 里 workspace members 的显式路径；改成通配写法时成员目录校验会先失败。 */
function cargoWorkspaceMembers(manifest: string) {
  const members = manifest.match(/^members\s*=\s*\[([^\]]*)\]/m)?.[1] ?? "";
  return [...members.matchAll(/"([^"]+)"/g)].map((match) => match[1]);
}

describe("MomoBako 工具链", () => {
  it("package.json 提供文档脚本并保留插件验证链路", () => {
    const pkg = JSON.parse(read("package.json"));
    const deps = { ...pkg.dependencies, ...pkg.devDependencies };

    expect(pkg.packageManager).toMatch(/^yarn@4\.17\.1\+sha512\./);
    expect(pkg.scripts).toMatchObject({
      "docs:dev": "vitepress dev docs",
      "docs:build": "vitepress build docs",
      "docs:preview": "vitepress preview docs",
      "plugins:build": "node scripts/build-external-plugins.mjs",
      "plugins:package": "node scripts/package-external-plugins.mjs",
      "test:backend": "cargo test -p momobako-backend --locked",
      "nana:build": "cargo build -p momobako-nana --bin momobako-nana --profile dist --locked",
      "nana:package": "node scripts/nana-package.ts",
      verify: "yarn typecheck:node-scripts && yarn test && cargo check -p momobako-backend -p momobako-nana --all-targets --locked",
    });
    expect(pkg.dependencies).toBeUndefined();
    expect(Object.keys(deps).sort()).toEqual(["@types/node", "typescript", "vitepress", "vitest"]);
    for (const script of ["dev", "build", "preview", "plugins:stage:dev", "icons:tauri"]) {
      expect(pkg.scripts[script], script).toBeUndefined();
    }
    expect(Object.keys(pkg.scripts).filter((script) => script.startsWith("tauri"))).toEqual([]);
    for (const path of [
      "src",
      "public",
      "index.html",
      "vite.config.ts",
      "tsconfig.json",
      "tsconfig.node.json",
      "app.config.json",
      // 旧检出可能在 src-tauri 下留着被忽略的本地开发数据（.service-data），按外壳自己的文件判断。
      "src-tauri/Cargo.toml",
      "src-tauri/tauri.conf.json",
      "scripts/tauri-dev.ts",
      "scripts/stage-external-plugins.mjs",
      "External/Plugins/scripts/stage-dev.mjs",
    ]) {
      expect(existsSync(resolve(path)), `${path} 是 Vue 前端或 Tauri 外壳的文件，应删除`).toBe(false);
    }
  });

  it("External/Plugins 提供标准开发入口并与主线源码隔离", () => {
    const devGuide = read("External/Plugins/dev.md");
    const pluginsPkg = JSON.parse(read("External/Plugins/package.json"));
    const templateManifest = JSON.parse(read("External/Plugins/template/backend-rust/manifest.json"));
    const exampleManifest = JSON.parse(read("External/Plugins/example/backend-rust/manifest.json"));

    expect(devGuide).toContain("External/Plugins/");
    expect(devGuide).toContain("External/Plugins/package.json");
    expect(devGuide).toContain("<serviceRoot>/plugins");
    expect(devGuide).toContain(".momoplug");
    expect(devGuide).toContain("manifest-only");
    expect(devGuide).not.toContain("register(ctx)");
    expect(devGuide).toContain("Mutsuki ABI v2");
    expect(pluginsPkg.scripts).toMatchObject({
      build: "node scripts/build.mjs",
      package: "yarn typecheck:node-scripts && node scripts/package.ts",
    });
    expect(pluginsPkg.scripts["stage:dev"]).toBeUndefined();
    expect(templateManifest).toMatchObject({ sdk: "backend", runtime: "native-dylib" });
    expect(exampleManifest).toMatchObject({ sdk: "backend", runtime: "native-dylib" });
    expect(pluginsPkg.dependencies.esbuild).toBeUndefined();
  });

  it("External/Plugins 的插件都是后端形态，不带前端代码", () => {
    const root = resolve("External/Plugins");
    const pluginDirs = readdirSync(root, { withFileTypes: true })
      .filter((entry) => entry.isDirectory() && existsSync(join(root, entry.name, "manifest.json")))
      .map((entry) => entry.name);

    expect(pluginDirs.length).toBeGreaterThan(0);
    for (const dir of pluginDirs) {
      const manifest = JSON.parse(read(`External/Plugins/${dir}/manifest.json`));
      expect(manifest.sdk, dir).toBe("backend");
      expect(["native-dylib", "process", "manifest-only"], dir).toContain(manifest.runtime);
      expect(manifest.entry?.frontend, dir).toBeUndefined();
      expect(manifest.contributes?.preview, dir).toBeUndefined();
      if (existsSync(join(root, dir, "plugin.project.json"))) {
        const project = JSON.parse(read(`External/Plugins/${dir}/plugin.project.json`));
        expect(["cargo-native", "cargo-process", "manifest-only"], dir).toContain(project.build?.type);
      }
      expect(javascriptSources(join(root, dir)), dir).toEqual([]);
    }
  });

  it("Yarn 启动器优先复用当前项目入口并在独立执行时回退 Corepack", () => {
    expect(resolveYarnCommand({
      npmExecPath: "/tooling/yarn",
      platform: "linux",
    })).toEqual({
      command: "/tooling/yarn",
      args: [],
    });
    expect(resolveYarnCommand({
      comSpec: "C:\\Windows\\System32\\cmd.exe",
      npmExecPath: "C:\\Users\\ADMINI~1\\AppData\\Local\\Temp\\xfs-a96bedc1\\yarn",
      platform: "win32",
    })).toEqual({
      command: "C:\\Windows\\System32\\cmd.exe",
      args: [
        "/d",
        "/s",
        "/c",
        "C:\\Users\\ADMINI~1\\AppData\\Local\\Temp\\xfs-a96bedc1\\yarn.cmd",
      ],
    });
    expect(resolveYarnCommand({
      comSpec: "C:\\Windows\\System32\\cmd.exe",
      npmExecPath: "C:\\tooling\\yarn.cmd",
      platform: "win32",
    })).toEqual({
      command: "C:\\Windows\\System32\\cmd.exe",
      args: ["/d", "/s", "/c", "C:\\tooling\\yarn.cmd"],
    });
    expect(resolveYarnCommand({
      comSpec: "C:\\Windows\\System32\\cmd.exe",
      npmExecPath: "C:\\tooling\\yarn.js",
      platform: "win32",
    })).toEqual({
      command: "C:\\Windows\\System32\\cmd.exe",
      args: ["/d", "/s", "/c", "corepack.cmd", "yarn"],
    });
    expect(resolveYarnCommand({
      comSpec: "C:\\Windows\\System32\\cmd.exe",
      npmExecPath: "",
      platform: "win32",
    })).toEqual({
      command: "C:\\Windows\\System32\\cmd.exe",
      args: ["/d", "/s", "/c", "corepack.cmd", "yarn"],
    });
  });

  it("GitHub CI 使用 MomoBako 验证、后端单测和文档构建配置", () => {
    const ci = read(".github/workflows/ci.yml");

    expect(ci).toContain("Verify MomoBako");
    expect(ci).toContain("Build External Plugins");
    expect(ci).toContain("corepack yarn test:backend");
    expect(ci).toContain("corepack yarn verify");
    expect(ci).toContain("working-directory: External/Plugins");
    expect(ci).toContain("corepack yarn package");
    expect(ci).toContain("corepack yarn docs:build");
    expectNoSourceResidue(ci);
  });

  it("GitHub release 用 nana-packager 打 Windows 包并发布草稿", () => {
    const release = read(".github/workflows/release.yml");

    expect(release).toContain("Publish Windows Release");
    expect(release).not.toContain("tauri-action");
    expect(release).not.toContain("src-tauri");
    expect(release).not.toContain("corepack yarn verify");
    expect(release).toContain("corepack yarn test:backend");
    expect(release).toContain("Install external plugin dependencies");
    expect(release).toMatch(
      /name: Package external plugins[\s\S]+run: corepack yarn nana:build[\s\S]+run: corepack yarn nana:package /,
    );
    expect(release).toContain('--expect-tag "${{ steps.release_ref.outputs.tag }}"');
    expect(release).toMatch(/gh release create \$env:RELEASE_TAG @assets --draft --title "MomoBako \$env:RELEASE_TAG"/);
    expect(release).toContain("Upload external plugin packages");
    expect(release).toContain("path: External/Plugins/.packages/*.momoplug");
  });

  it("GitHub workflow 的 Rust 缓存跟随根 Cargo workspace 并按 rustc 版本分代", () => {
    expectWorkspaceRustCache(read(".github/workflows/ci.yml"), ["cargo", "external-plugins-cargo"]);
    expectWorkspaceRustCache(read(".github/workflows/release.yml"), ["cargo"]);
  });

  it("Cargo workspace 成员共用根 Cargo.lock，成员目录不再各带一份", () => {
    const members = cargoWorkspaceMembers(read("Cargo.toml"));

    expect(existsSync(resolve("Cargo.lock"))).toBe(true);
    expect(members).toEqual(expect.arrayContaining(["src-backend", "src-nana", "External/Plugins/_sdk/mutsuki-rust"]));
    expect(members).not.toContain("src-tauri");
    for (const member of members) {
      expect(existsSync(resolve(member, "Cargo.toml")), `${member} 不是 Cargo 包目录`).toBe(true);
      expect(existsSync(resolve(member, "Cargo.lock")), `${member}/Cargo.lock 不会被 cargo 读取，应删除`).toBe(false);
    }
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
});
