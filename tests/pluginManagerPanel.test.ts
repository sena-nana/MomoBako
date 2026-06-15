import { within } from "@testing-library/dom";
import { fireEvent, render, screen } from "@testing-library/vue";
import { beforeEach, describe, expect, it, vi } from "vitest";
import PluginManagerPanel from "../src/components/PluginManagerPanel.vue";
import type { PluginHookExecutionRecord, PluginManifest } from "../src/types/repository";

const plugins = vi.hoisted(() => ({ value: [] as PluginManifest[] }));
const pluginHookExecutions = vi.hoisted(() => ({ value: [] as PluginHookExecutionRecord[] }));
const error = vi.hoisted(() => ({ value: "" }));
const deletePluginConfigValueInWorkspace = vi.hoisted(() => vi.fn());
const loadPluginConfigInWorkspace = vi.hoisted(() => vi.fn());
const openPluginDataDirectoryInWorkspace = vi.hoisted(() => vi.fn());
const setPluginConfigValueInWorkspace = vi.hoisted(() => vi.fn());

vi.mock("../src/composables/useRepositoryWorkspace", () => ({
  useWorkspaceSettings: () => ({
    plugins,
    pluginHookExecutions,
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
  function group(name: string) {
    return screen.getByRole("heading", { name }).closest(".plugin-manager__group") as HTMLElement;
  }

  beforeEach(() => {
    plugins.value = [];
    pluginHookExecutions.value = [];
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

  it("groups plugins by category while keeping card details visible", () => {
    plugins.value = [
      manifest({
        pluginId: "user.source",
        name: "Source Plugin",
        category: "source",
        kind: "filesystem",
      }),
      manifest({
        pluginId: "user.library",
        name: "Library Plugin",
        category: "library-kind",
        kind: "library-kind",
      }),
      manifest({
        pluginId: "user.parser",
        name: "Parser Plugin",
        category: "parser",
        kind: "parser",
      }),
      manifest({
        pluginId: "user.preview",
        name: "Preview Plugin",
        category: "preview",
        kind: "preview",
      }),
      manifest({
        pluginId: "user.service",
        name: "Service Plugin",
        category: "service",
        kind: "metadata",
        permissions: ["network"],
        requires: ["user.source"],
        hooks: [{ slot: "search", action: "index", label: "Index" }],
        dependencyStatus: {
          required: [
            {
              pluginId: "user.source",
              name: "Source Plugin",
              status: "ready",
              enabled: true,
              available: true,
            },
          ],
          optional: [],
          missingRequired: [],
          missingOptional: [],
          disabledRequired: [],
          disabledOptional: [],
        },
      }),
    ];

    render(PluginManagerPanel);

    const groups = [
      group("库来源"),
      group("库类型"),
      group("文件解析"),
      group("预览渲染"),
      group("基础服务"),
    ];

    expect(groups.map((group) => within(group as HTMLElement).getByText(/1 个插件/).textContent)).toEqual([
      "1 个插件",
      "1 个插件",
      "1 个插件",
      "1 个插件",
      "1 个插件",
    ]);

    expect(within(groups[0] as HTMLElement).getByText("Source Plugin")).toBeInTheDocument();
    expect(within(groups[1] as HTMLElement).getByText("Library Plugin")).toBeInTheDocument();
    expect(within(groups[2] as HTMLElement).getByText("Parser Plugin")).toBeInTheDocument();
    expect(within(groups[3] as HTMLElement).getByText("Preview Plugin")).toBeInTheDocument();

    const serviceGroup = groups[4] as HTMLElement;
    expect(within(serviceGroup).getByText("Service Plugin")).toBeInTheDocument();
    expect(within(serviceGroup).getByText("network")).toBeInTheDocument();
    expect(within(serviceGroup).getByText("必需 Source Plugin · 可用")).toBeInTheDocument();
    expect(within(serviceGroup).getByText("Index · search")).toBeInTheDocument();
  });

  it("shows recent hook execution records under declared hooks", () => {
    plugins.value = [
      manifest({
        pluginId: "user.service",
        name: "Service Plugin",
        hooks: [{ slot: "search", action: "service.search.index", label: "Index" }],
      }),
    ];
    pluginHookExecutions.value = [
      {
        executionId: "plugin-hook-1",
        pluginId: "user.service",
        hookSlot: "search",
        hookAction: "service.search.index",
        hookLabel: "Index",
        status: "success",
        message: "插件 Hook 已执行。",
        target: { query: "cover" },
        startedAt: "2026-06-14T10:12:00Z",
        finishedAt: "2026-06-14T10:12:01Z",
        runtime: null,
      },
    ];

    render(PluginManagerPanel);

    expect(screen.getByText("执行记录")).toBeInTheDocument();
    expect(screen.getByText("成功")).toBeInTheDocument();
    expect(screen.getAllByText("Index")[0]).toBeInTheDocument();
    expect(screen.getByText("插件 Hook 已执行。")).toBeInTheDocument();
  });

  it("does not render hook execution records when none exist", () => {
    plugins.value = [
      manifest({
        pluginId: "user.service",
        name: "Service Plugin",
        hooks: [{ slot: "search", action: "service.search.index", label: "Index" }],
      }),
    ];

    render(PluginManagerPanel);

    expect(screen.queryByText("执行记录")).not.toBeInTheDocument();
  });

  it("hides empty groups after filtering", async () => {
    plugins.value = [
      manifest({
        pluginId: "user.source",
        name: "Source Plugin",
        category: "source",
      }),
      manifest({
        pluginId: "user.parser",
        name: "Parser Plugin",
        category: "parser",
      }),
    ];

    render(PluginManagerPanel);

    const input = screen.getByRole("searchbox");
    await fireEvent.update(input, "parser");

    expect(screen.queryByRole("heading", { name: "库来源" })).not.toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "文件解析" })).toBeInTheDocument();
    expect(screen.getByText("Parser Plugin")).toBeInTheDocument();
    expect(screen.queryByText("Source Plugin")).not.toBeInTheDocument();
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
