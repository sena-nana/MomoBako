/**
 * 插件 SDK 与资源库缓存预览源桥接测试。
 *
 * 验证 repositoryApi 会调用宿主缓存预览命令，
 * 并验证前端插件上下文暴露 prepareRepositoryCacheFilePreviewSource。
 */
import { afterEach, describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/vue";
import { clearPreviewPluginRegistry, syncRegisteredFrontendPluginManifests } from "../src/plugins/sdk";
import { getToolPage } from "../src/plugins/toolPages";
import { prepareRepositoryCacheFilePreviewSource } from "../src/services/repositoryApi";
import type { PluginManifest } from "../src/types/repository";
import { getInvokeCalls } from "./setupTests";

function repositoryCachePreviewToolManifest(): PluginManifest {
  return {
    pluginId: "user.repository-cache-preview-source-tool",
    name: "Repository Cache Preview Source Tool",
    version: "0.1.0",
    kind: "tool",
    category: "tool",
    description: "验证资源库缓存预览源桥接。",
    capabilities: ["tool", "preview-cache"],
    enabled: true,
    sdk: "frontend",
    entry: {
      frontend: {
        module: "dist/register.js",
        export: "register",
      },
    },
    source: "user",
    runtime: "vue-module",
    permissions: [],
    requires: [],
    optional: [],
    hooks: [],
    contributes: {},
    compat: { sdkVersion: "1", legacyPluginIds: [] },
    status: "ready",
    dependencyStatus: {
      required: [],
      optional: [],
      missingRequired: [],
      missingOptional: [],
      disabledRequired: [],
      disabledOptional: [],
    },
    degraded: false,
  };
}

describe("plugin sdk repository cache preview source bridge", () => {
  afterEach(() => {
    clearPreviewPluginRegistry();
  });

  it("calls the repository cache preview source tauri command with the request envelope", async () => {
    const response = await prepareRepositoryCacheFilePreviewSource({
      repoId: "repo-main-001",
      path: "C:/Mock/AnimeAssets/.momo/cache/office-preview/mock-preview.pdf",
      mediaType: "application/pdf",
    });

    expect(response).toMatchObject({
      repoId: "repo-main-001",
      path: "C:/Mock/AnimeAssets/.momo/cache/office-preview/mock-preview.pdf",
      token: "repository-cache-preview-token",
      sourceUrl: "http://127.0.0.1:49152/preview/repository-cache-preview-token",
      mediaType: "application/pdf",
    });
    expect(getInvokeCalls("prepare_repository_cache_file_preview_source").at(-1)?.args).toEqual({
      request: {
        repoId: "repo-main-001",
        path: "C:/Mock/AnimeAssets/.momo/cache/office-preview/mock-preview.pdf",
        mediaType: "application/pdf",
      },
    });
  });

  it("injects prepareRepositoryCacheFilePreviewSource into frontend plugin context", async () => {
    const manifest = repositoryCachePreviewToolManifest();

    await syncRegisteredFrontendPluginManifests([manifest]);
    const page = getToolPage("user.repository-cache-preview-source-tool");

    expect(page?.component).toBeDefined();

    render(page!.component, {
      props: {
        manifest,
      },
    });

    expect(await screen.findByText("context-ready")).toBeInTheDocument();
    expect(await screen.findByText("repository-cache-preview-token")).toBeInTheDocument();
    expect(
      await screen.findByText("http://127.0.0.1:49152/preview/repository-cache-preview-token"),
    ).toBeInTheDocument();
    expect(getInvokeCalls("prepare_repository_cache_file_preview_source").at(-1)?.args).toEqual({
      request: {
        repoId: "repo-main-001",
        path: "C:/Mock/AnimeAssets/.momo/cache/office-preview/tool-preview.pdf",
        mediaType: "application/pdf",
      },
    });
  });
});
