import { afterEach, describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/vue";
import {
  clearPreviewPluginRegistry,
  syncRegisteredFrontendPluginManifests,
} from "../src/plugins/sdk";
import { getPluginSettingsPage, listPluginSettingsPages } from "../src/plugins/settingsPages";
import type { PluginManifest } from "../src/types/repository";
import { getInvokeCalls, getPluginCallCalls } from "./setupTests";
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
});
