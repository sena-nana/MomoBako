import { afterEach, describe, expect, it } from "vitest";
import { clearPreviewPluginRegistry, syncRegisteredFrontendPluginManifests } from "../src/plugins/sdk";
import { getPluginSettingsPage, listPluginSettingsPages } from "../src/plugins/settingsPages";
import type { PluginManifest } from "../src/types/repository";

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
});
