import { fireEvent, render, screen } from "@testing-library/vue";
import { beforeEach, describe, expect, it, vi } from "vitest";
import PluginManagerPanel from "../src/components/PluginManagerPanel.vue";
import type { PluginManifest } from "../src/types/repository";

const plugins = vi.hoisted(() => ({ value: [] as PluginManifest[] }));
const error = vi.hoisted(() => ({ value: "" }));
const deletePluginConfigValueInWorkspace = vi.hoisted(() => vi.fn());
const loadPluginConfigInWorkspace = vi.hoisted(() => vi.fn());
const openPluginDataDirectoryInWorkspace = vi.hoisted(() => vi.fn());
const setPluginConfigValueInWorkspace = vi.hoisted(() => vi.fn());

vi.mock("../src/composables/useRepositoryWorkspace", () => ({
  useWorkspaceSettings: () => ({
    plugins,
    deletePluginConfigValueInWorkspace,
    deletePluginInWorkspace: vi.fn(),
    installPluginArchiveInWorkspace: vi.fn(),
    loadPluginConfigInWorkspace,
    loadSettingsData: vi.fn(),
    openPluginDataDirectoryInWorkspace,
    setPluginConfigValueInWorkspace,
    setPluginEnabledInWorkspace: vi.fn(),
  }),
  useWorkspaceProgress: () => ({
    isLoadingSettingsData: false,
    isManagingPlugins: false,
    error,
  }),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
}));

function manifest(input: Partial<PluginManifest> & Pick<PluginManifest, "pluginId" | "name">): PluginManifest {
  return {
    pluginId: input.pluginId,
    name: input.name,
    version: "0.1.0",
    kind: "metadata",
    category: "service",
    description: "Test plugin.",
    capabilities: ["metadata"],
    enabled: true,
    sdk: "backend",
    source: "user",
    runtime: "manifest-only",
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
    ...input,
  };
}

describe("PluginManagerPanel", () => {
  beforeEach(() => {
    plugins.value = [];
    error.value = "";
    deletePluginConfigValueInWorkspace.mockReset();
    loadPluginConfigInWorkspace.mockReset();
    openPluginDataDirectoryInWorkspace.mockReset();
    setPluginConfigValueInWorkspace.mockReset();
    loadPluginConfigInWorkspace.mockResolvedValue({
      pluginId: "user.configurable",
      dataDirectory: "C:/MomoBako/.service-data/plugin-data/user-configurable",
      schema: {},
      values: {},
    });
  });

  it("shows dependency, permission, disable reason, and degraded feedback", () => {
    plugins.value = [
      manifest({
        pluginId: "user.dependent",
        name: "Dependent Plugin",
        permissions: ["readMetadata", "network"],
        requires: ["user.provider"],
        optional: ["user.optional-helper"],
        status: "ready",
        degraded: true,
        degradationReason: "可选依赖不可用，部分能力降级：Optional Helper。",
        dependencyStatus: {
          required: [
            {
              pluginId: "user.provider",
              name: "Provider",
              status: "ready",
              enabled: true,
              available: true,
            },
          ],
          optional: [
            {
              pluginId: "user.optional-helper",
              name: "Optional Helper",
              status: "disabled",
              enabled: false,
              available: false,
            },
          ],
          missingRequired: [],
          missingOptional: [],
          disabledRequired: [],
          disabledOptional: ["Optional Helper"],
        },
      }),
      manifest({
        pluginId: "user.missing-required",
        name: "Missing Required",
        enabled: false,
        status: "unavailable",
        disableReason: "缺少必需依赖：Provider。",
      }),
    ];

    render(PluginManagerPanel);

    expect(screen.getByText("降级运行")).toBeInTheDocument();
    expect(screen.getByText("可选依赖不可用，部分能力降级：Optional Helper。")).toBeInTheDocument();
    expect(screen.getByText("必需 Provider · 可用")).toBeInTheDocument();
    expect(screen.getByText("可选 Optional Helper · 未启用")).toBeInTheDocument();
    expect(screen.getByText("readMetadata")).toBeInTheDocument();
    expect(screen.getByText("network")).toBeInTheDocument();
    expect(screen.getByText("缺少必需依赖：Provider。")).toBeInTheDocument();
  });

  it("opens the selected plugin settings directory from the settings panel", async () => {
    openPluginDataDirectoryInWorkspace.mockResolvedValue({
      pluginId: "user.configurable",
      path: "C:/MomoBako/.service-data/plugin-data/user-configurable",
    });
    plugins.value = [
      manifest({
        pluginId: "user.configurable",
        name: "Configurable Plugin",
      }),
    ];

    render(PluginManagerPanel);
    await fireEvent.click(screen.getByRole("button", { name: "设置" }));
    await fireEvent.click(await screen.findByRole("button", { name: "打开目录" }));

    expect(loadPluginConfigInWorkspace).toHaveBeenCalledWith("user.configurable");
    expect(openPluginDataDirectoryInWorkspace).toHaveBeenCalledWith("user.configurable");
    expect(screen.getByText("已打开“Configurable Plugin”设置目录。")).toBeInTheDocument();
  });

  it("renders manifest settings schema and saves key-value config", async () => {
    loadPluginConfigInWorkspace.mockResolvedValue({
      pluginId: "user.configurable",
      dataDirectory: "C:/MomoBako/.service-data/plugin-data/user-configurable",
      schema: {},
      values: {
        apiKey: "old-key",
        enabled: true,
      },
    });
    setPluginConfigValueInWorkspace.mockResolvedValue({
      pluginId: "user.configurable",
      dataDirectory: "C:/MomoBako/.service-data/plugin-data/user-configurable",
      schema: {},
      values: {
        apiKey: "new-key",
        enabled: true,
      },
    });
    plugins.value = [
      manifest({
        pluginId: "user.configurable",
        name: "Configurable Plugin",
        contributes: {
          settings: {
            fields: [
              {
                key: "apiKey",
                label: "API Key",
                type: "string",
              },
              {
                key: "enabled",
                label: "Enabled",
                type: "boolean",
              },
            ],
          },
        },
      }),
    ];

    render(PluginManagerPanel);
    await fireEvent.click(screen.getByRole("button", { name: "设置" }));
    const input = await screen.findByDisplayValue("old-key");
    await fireEvent.update(input, "new-key");
    await fireEvent.change(input);

    expect(setPluginConfigValueInWorkspace).toHaveBeenCalledWith(
      "user.configurable",
      "apiKey",
      "new-key",
    );
    expect(screen.getByText("插件设置已保存。")).toBeInTheDocument();
  });
});
