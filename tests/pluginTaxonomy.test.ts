import { afterEach, describe, expect, it } from "vitest";
import { repositoryBackendOptionsFromPlugins } from "../src/composables/workspace/selectors";
import { plugins } from "../src/composables/workspace/state";
import type { PluginManifest } from "../src/types/repository";
import { pluginCategory, pluginCategoryForKind, pluginCategoryLabel } from "../src/utils/pluginTaxonomy";

function manifest(input: Partial<PluginManifest> & Pick<PluginManifest, "pluginId" | "kind">): PluginManifest {
  return {
    name: input.pluginId,
    version: "0.1.0",
    description: "Test plugin.",
    capabilities: [],
    enabled: true,
    ...input,
  };
}

describe("plugin taxonomy", () => {
  afterEach(() => {
    plugins.value = [];
  });

  it("marks only runtime-available source plugins as creatable repository backend options", () => {
    plugins.value = [
      manifest({
        pluginId: "momobako.local-filesystem",
        kind: "filesystem",
        category: "source",
        sdk: "backend",
        runtime: "native-dylib",
        status: "ready",
      }),
      manifest({
        pluginId: "momobako.source.custom",
        kind: "remote-mount",
        category: "source",
        sdk: "backend",
        runtime: "manifest-only",
        status: "ready",
      }),
      manifest({
        pluginId: "momobako.library.audio",
        kind: "library-kind",
        category: "library-kind",
      }),
    ];

    const options = repositoryBackendOptionsFromPlugins();
    expect(options.map((plugin) => plugin.pluginId)).toEqual(["momobako.local-filesystem", "momobako.source.custom"]);
    expect(options.map((plugin) => [plugin.pluginId, plugin.enabled])).toEqual([
      ["momobako.local-filesystem", true],
      ["momobako.source.custom", false],
    ]);
  });

  it("keeps legacy filesystem kinds attachable without category", () => {
    plugins.value = [
      manifest({ pluginId: "momobako.webdav", kind: "webdav", sdk: "backend", runtime: "manifest-only" }),
      manifest({ pluginId: "momobako.cloud-drive", kind: "cloud", sdk: "backend", runtime: "manifest-only" }),
      manifest({ pluginId: "momobako.preview.media", kind: "preview" }),
    ];

    const options = repositoryBackendOptionsFromPlugins();
    expect(options.map((plugin) => plugin.pluginId)).toEqual(["momobako.webdav", "momobako.cloud-drive"]);
    expect(options.every((plugin) => !plugin.enabled)).toBe(true);
  });

  it("centralizes category fallback and labels", () => {
    expect(pluginCategoryForKind("webdav")).toBe("source");
    expect(pluginCategoryForKind("parser")).toBe("parser");
    expect(pluginCategoryForKind("metadata")).toBe("service");
    expect(pluginCategory(manifest({ pluginId: "momobako.preview.media", kind: "preview" }))).toBe("preview");
    expect(pluginCategoryLabel("library-kind")).toBe("库类型");
  });
});
