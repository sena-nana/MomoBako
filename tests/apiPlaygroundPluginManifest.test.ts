import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

function readManifest<T = Record<string, unknown>>(): T {
  return JSON.parse(readFileSync(resolve("External/Plugins/api-playground/manifest.json"), "utf-8")) as T;
}

describe("API Playground plugin manifest", () => {
  it("puts the host's built-in API Playground page on the extension page", () => {
    const manifest = readManifest<{
      pluginId: string;
      sdk: string;
      runtime: string;
      entry: Record<string, unknown>;
      permissions: string[];
      hooks: Array<{ slot: string; action: string }>;
      contributes: {
        toolPages: Array<{
          toolPageId: string;
          label: string;
          description: string;
          order: number;
        }>;
      };
    }>();

    expect(manifest.pluginId).toBe("momobako.tool.api-playground");
    expect(manifest.sdk).toBe("backend");
    expect(manifest.runtime).toBe("manifest-only");
    expect(manifest.entry).toEqual({});
    expect(manifest.contributes.toolPages[0]).toMatchObject({
      toolPageId: "momobako.tool.api-playground",
      label: "API Playground",
      order: 10,
    });
    expect(manifest.permissions).toEqual(expect.arrayContaining([
      "network:localhost",
      "external-api:read",
      "external-api:write",
    ]));
    expect(manifest.hooks).toEqual(expect.arrayContaining([
      expect.objectContaining({ slot: "toolPage", action: "tool.apiPlayground.open" }),
    ]));
  });
});
