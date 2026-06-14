import { afterEach, describe, expect, it } from "vitest";
import { clearPreviewPluginRegistry, syncRegisteredFrontendPluginManifests } from "../src/plugins/sdk";
import { getToolPage, listToolPages } from "../src/plugins/toolPages";
import { listPlugins } from "../src/services/repositoryApi";

describe("toolPages", () => {
  afterEach(() => {
    clearPreviewPluginRegistry();
  });

  it("registers API Playground as a plugin tool page", async () => {
    await syncRegisteredFrontendPluginManifests(await listPlugins());

    const pages = listToolPages();

    expect(pages.map((page) => page.toolPageId)).toContain("momobako.tool.api-playground");
    expect(getToolPage("momobako.tool.api-playground")?.label).toBe("API Playground");
    expect(getToolPage("momobako.tool.api-playground")?.component).toBeDefined();
  });

  it("hides tool pages when their plugin manifest is disabled", async () => {
    await syncRegisteredFrontendPluginManifests(await listPlugins());
    const manifests = listToolPages().map((page) => (
      page.pluginId === "momobako.tool.api-playground"
        ? {
            ...page.manifest!,
            enabled: false,
            status: "disabled" as const,
          }
        : page.manifest!
    ));

    await syncRegisteredFrontendPluginManifests(manifests);

    expect(getToolPage("momobako.tool.api-playground")).toBeNull();
    expect(listToolPages()).toHaveLength(0);
  });
});
