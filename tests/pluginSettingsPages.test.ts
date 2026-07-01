import { afterEach, describe, expect, it } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/vue";
import {
  clearPreviewPluginRegistry,
  syncRegisteredFrontendPluginManifests,
} from "../src/plugins/sdk";
import { getPluginSettingsPage, listPluginSettingsPages } from "../src/plugins/settingsPages";
import type { PluginManifest } from "../src/types/repository";
import {
  getInvokeCalls,
  mockPluginCallResponse,
  getPluginCallCalls,
  seedMockPlugins,
  seedMockRepositories,
} from "./setupTests";
import { pluginManifest } from "./fixtures/repositoryFixtures";

function settingsPageManifest(enabled = true): PluginManifest {
  return {
    pluginId: "user.settings-page",
    name: "Settings Plugin",
    version: "0.1.0",
    kind: "service",
    category: "service",
    description: "Test settings page.",
    capabilities: ["settings"],
    enabled,
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
    contributes: {
      settings: {
        settingsPage: {
          label: "Settings Page",
        },
      },
    },
    compat: { sdkVersion: "1", legacyPluginIds: [] },
    status: enabled ? "ready" : "disabled",
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

function neteaseSettingsManifest(): PluginManifest {
  return pluginManifest(
    "momobako.library.netease-cloud-music",
    [],
    "Netease Cloud Music Library",
    "0.1.0",
    "library-kind",
    "netease-cloud-music",
    "网易云音乐前端扩展。",
    ["library", "entry-actions", "settings"],
    true,
    "frontend",
    "vue-module",
    "user",
  );
}

function officeConvertSettingsManifest(): PluginManifest {
  return {
    pluginId: "momobako.service.office-convert",
    name: "Office Convert Service",
    version: "0.1.0",
    kind: "office-convert",
    category: "service",
    description: "Office 转换服务设置页。",
    capabilities: ["office", "convert", "settings"],
    enabled: true,
    sdk: "frontend",
    entry: {
      frontend: {
        module: "dist/register.js",
        export: "register",
      },
    },
    source: "builtin",
    runtime: "vue-module",
    permissions: [],
    requires: [],
    optional: [],
    hooks: [],
    contributes: {
      settings: {
        settingsPage: {
          label: "Office 转换",
        },
      },
    },
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

function downloaderSettingsManifest(): PluginManifest {
  return {
    pluginId: "momobako.service.downloader",
    name: "Download Service",
    version: "0.1.0",
    kind: "download",
    category: "service",
    description: "下载服务设置页。",
    capabilities: ["download", "settings"],
    enabled: true,
    sdk: "frontend",
    entry: {
      frontend: {
        module: "dist/register.js",
        export: "register",
      },
    },
    source: "builtin",
    runtime: "vue-module",
    permissions: [],
    requires: [],
    optional: [],
    hooks: [],
    contributes: {
      settings: {
        settingsPage: {
          label: "下载服务",
        },
      },
    },
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

describe("plugin settings pages", () => {
  afterEach(() => {
    clearPreviewPluginRegistry();
  });

  it("registers custom settings pages and hides disabled plugin pages", async () => {
    await syncRegisteredFrontendPluginManifests([settingsPageManifest()]);

    expect(getPluginSettingsPage("user.settings-page")?.label).toBe("Settings Page");
    expect(getPluginSettingsPage("user.settings-page")?.component).toBeDefined();
    expect(listPluginSettingsPages().map((page) => page.pluginId)).toEqual(["user.settings-page"]);

    await syncRegisteredFrontendPluginManifests([settingsPageManifest(false)]);

    expect(getPluginSettingsPage("user.settings-page")).toBeNull();
    expect(listPluginSettingsPages()).toHaveLength(0);
  });

  it("keeps netease account login out of plugin-global settings", async () => {
    const manifest = neteaseSettingsManifest();

    await syncRegisteredFrontendPluginManifests([manifest]);
    const page = getPluginSettingsPage(manifest.pluginId);
    expect(page?.component).toBeDefined();

    render(page!.component, {
      props: {
        manifest,
      },
    });

    expect(await screen.findByText("添加资源库时扫码登录")).toBeInTheDocument();
    expect(screen.getByText("每个网易云账号一个资源库")).toBeInTheDocument();
    expect(screen.getByText("在对应资源库中操作，不保存在插件全局配置")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "二维码登录" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "创建/刷新资源库" })).not.toBeInTheDocument();
    expect(getInvokeCalls("create_repository")).toHaveLength(0);
    expect(getInvokeCalls("set_plugin_config_value")).toHaveLength(0);
    expect(getPluginCallCalls("momobako.source.netease-cloud-music")).toHaveLength(0);
  });

  it("renders office convert status page and triggers runtime actions", async () => {
    const manifest = officeConvertSettingsManifest();
    seedMockRepositories([{
      repoId: "repo-main-001",
      name: "Mock Anime Repo",
      path: "C:/Mock/AnimeAssets",
      backend: {
        pluginId: "momobako.local-filesystem",
        kind: "filesystem",
        name: "Local Filesystem",
        capabilities: ["browse"],
      },
      status: "ready",
      assetCount: 12,
      updatedAt: "2026-07-01T08:00:00Z",
    }]);
    seedMockPlugins([manifest]);

    await syncRegisteredFrontendPluginManifests([manifest]);
    const page = getPluginSettingsPage(manifest.pluginId);
    expect(page?.component).toBeDefined();

    render(page!.component, {
      props: {
        manifest,
      },
    });

    expect(await screen.findByText("运行状态与缓存")).toBeInTheDocument();
    expect(await screen.findByText(/WINWORD\.EXE/)).toBeInTheDocument();
    expect((await screen.findAllByText("Mock Anime Repo")).length).toBeGreaterThan(0);
    expect(getPluginCallCalls("momobako.service.office-convert", "officeConvert.getRuntimeStatus")).toHaveLength(1);
    expect(getInvokeCalls("list_repositories")).toHaveLength(1);

    await fireEvent.click(screen.getByRole("button", { name: "运行自检" }));
    await waitFor(() => {
      expect(getPluginCallCalls("momobako.service.office-convert", "officeConvert.runRuntimeSelfCheck").length).toBeGreaterThan(0);
    });

    await fireEvent.click(screen.getByRole("button", { name: "清理缓存" }));
    await waitFor(() => {
      expect(getPluginCallCalls("momobako.service.office-convert", "officeConvert.clearPreviewCache").length).toBeGreaterThan(0);
    });

    expect(await screen.findByText("已清理 0 个缓存文件")).toBeInTheDocument();
    expect(screen.getByText("转换模式与自动下载选项沿用下方插件通用配置字段保存。")).toBeInTheDocument();
  });

  it("allows shutting down a running office daemon and refreshes runtime status", async () => {
    const manifest = officeConvertSettingsManifest();
    let runtimeStatusReads = 0;
    let daemonRunning = true;
    mockPluginCallResponse("momobako.service.office-convert", "officeConvert.getRuntimeStatus", () => {
      runtimeStatusReads += 1;
      return {
        converterMode: "auto",
        autoDownloadLibreOffice: true,
        bundledDownloadUrl: "https://example.test/libreoffice.msi",
        microsoftOffice: {
          available: true,
          path: "C:/Program Files/Microsoft Office/root/Office16/WINWORD.EXE",
        },
        libreofficeSystem: {
          available: false,
          reason: "未探测到系统 LibreOffice 安装。",
        },
        libreofficeBundle: {
          available: true,
          path: "C:/MomoBako/.service-data/plugin-data/momobako-service-office-convert/runtime/program/soffice.exe",
          version: "25.8.3",
        },
        daemon: daemonRunning
          ? {
              running: true,
              healthy: true,
              helperType: "bundled",
              pid: 23119,
              sofficeReady: true,
              updatedAt: "2026-07-01T08:00:00Z",
            }
          : {
              running: false,
              healthy: false,
              helperType: "bundled",
              error: "守护进程已停止",
              updatedAt: "2026-07-01T08:02:00Z",
            },
      };
    });
    mockPluginCallResponse("momobako.service.office-convert", "officeConvert.shutdownDaemon", () => {
      daemonRunning = false;
      return {
        stopped: true,
        pid: 23119,
      };
    });
    seedMockRepositories([{
      repoId: "repo-main-001",
      name: "Mock Anime Repo",
      path: "C:/Mock/AnimeAssets",
      backend: {
        pluginId: "momobako.local-filesystem",
        kind: "filesystem",
        name: "Local Filesystem",
        capabilities: ["browse"],
      },
      status: "ready",
      assetCount: 12,
      updatedAt: "2026-07-01T08:00:00Z",
    }]);
    seedMockPlugins([manifest]);

    await syncRegisteredFrontendPluginManifests([manifest]);
    const page = getPluginSettingsPage(manifest.pluginId);
    expect(page?.component).toBeDefined();

    render(page!.component, {
      props: {
        manifest,
      },
    });

    expect(await screen.findByText(/WINWORD\.EXE/)).toBeInTheDocument();
    expect(await screen.findByText("PID 23119 | Soffice 已就绪 | 更新于 2026-07-01T08:00:00Z")).toBeInTheDocument();

    const shutdownButton = screen.getByRole("button", { name: "关闭守护进程" });
    expect(shutdownButton).toBeEnabled();

    await fireEvent.click(shutdownButton);

    await waitFor(() => {
      expect(getPluginCallCalls("momobako.service.office-convert", "officeConvert.shutdownDaemon")).toHaveLength(1);
    });
    await waitFor(() => {
      expect(getPluginCallCalls("momobako.service.office-convert", "officeConvert.getRuntimeStatus").length).toBeGreaterThanOrEqual(2);
    });

    expect(await screen.findByText("LibreOffice 守护进程已关闭")).toBeInTheDocument();
    expect(await screen.findByText("守护进程已停止")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "关闭守护进程" })).toBeDisabled();
  });

  it("renders downloader runtime status page", async () => {
    const manifest = downloaderSettingsManifest();
    seedMockPlugins([manifest]);

    await syncRegisteredFrontendPluginManifests([manifest]);
    const page = getPluginSettingsPage(manifest.pluginId);
    expect(page?.component).toBeDefined();

    render(page!.component, {
      props: {
        manifest,
      },
    });

    expect(await screen.findByText("aria2 运行状态")).toBeInTheDocument();
    expect(await screen.findByText("http://127.0.0.1:6800/jsonrpc")).toBeInTheDocument();
    expect(getPluginCallCalls("momobako.service.downloader", "downloader.getRuntimeStatus")).toHaveLength(1);
  });
});
