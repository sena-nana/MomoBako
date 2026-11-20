import { existsSync, readdirSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

function externalRuntimeRoutes() {
  const source = readFileSync(resolve("src-backend/src/services/runtime/external_api.rs"), "utf-8");
  return [...source.matchAll(/\(&Method::([A-Za-z]+),\s*"([^"]+)"\)\s*=>/g)]
    .map((match) => `${match[1].toUpperCase()} ${match[2]}`)
    .filter((route) => route.includes(" /external/v1/"));
}

function externalDefinitionRoutes() {
  const source = readFileSync(resolve("src-backend/src/services/repository/api_design.rs"), "utf-8");
  return [...source.matchAll(/external_api_definition\(\s*"([^"]+)",\s*"([^"]+)"/g)]
    .map((match) => `${match[1].toUpperCase()} ${match[2]}`);
}

function pluginDirs() {
  return readdirSync(resolve("External/Plugins"), { withFileTypes: true })
    .filter(
      (entry) =>
        entry.isDirectory() && existsSync(resolve("External/Plugins", entry.name, "manifest.json")),
    )
    .map((entry) => entry.name)
    .sort();
}

function backendPluginMethods(pluginDir: string) {
  const sourcePath = resolve("External/Plugins", pluginDir, "src/lib.rs");
  if (!existsSync(sourcePath)) return [];
  const source = readFileSync(sourcePath, "utf-8");
  return [...source.matchAll(/"([^"]+)"\s*=>/g)]
    .map((match) => match[1])
    .filter((method) => method.includes("."));
}

function manifestApiMethods(pluginDir: string) {
  const manifest = JSON.parse(
    readFileSync(resolve("External/Plugins", pluginDir, "manifest.json"), "utf-8"),
  ) as {
    contributes?: {
      apiTests?: Array<{ method?: string }>;
      provider?: { lookup?: { action?: string } };
      metadataDefaults?: { action?: string };
    };
  };
  const contributes = manifest.contributes;
  return [
    ...(contributes?.apiTests ?? []).map((test) => test.method),
    contributes?.provider?.lookup?.action,
    contributes?.metadataDefaults?.action,
  ].filter((method): method is string => Boolean(method?.trim()));
}

function manifestRuntime(pluginDir: string) {
  const manifest = JSON.parse(
    readFileSync(resolve("External/Plugins", pluginDir, "manifest.json"), "utf-8"),
  ) as { runtime?: string };
  return manifest.runtime;
}

describe("API design snapshot contract", () => {
  it("lists only transports a client outside the host can call", () => {
    const source = readFileSync(resolve("src-backend/src/services/repository/api_design.rs"), "utf-8");
    const transports = [...source.matchAll(/transport: "([^"]+)"/g)].map((match) => match[1]);

    expect(transports.sort()).toEqual(["external-http", "plugin-call"]);
    expect(source).not.toMatch(/tauri/i);
  });

  it("keeps all external HTTP routes represented in the API snapshot definitions", () => {
    const expected = externalRuntimeRoutes();
    const covered = new Set(externalDefinitionRoutes());

    expect(expected.filter((route) => !covered.has(route))).toEqual([]);
  });

  it("keeps every backend manifest API declaration represented by its dispatch table", () => {
    const missing = pluginDirs().flatMap((pluginDir) => {
      const dispatchMethods = new Set(backendPluginMethods(pluginDir));
      return manifestApiMethods(pluginDir)
        .filter((method) => !dispatchMethods.has(method))
        .map((method) => `${pluginDir}:${method}`);
    });

    expect(missing).toEqual([]);
  });

  it("keeps manifest-only plugins free of callable API declarations", () => {
    // manifest-only 插件没有可执行体，声明的方法谁也接不住，调用只会报插件不可用。
    const callable = pluginDirs()
      .filter((pluginDir) => manifestRuntime(pluginDir) === "manifest-only")
      .flatMap((pluginDir) => manifestApiMethods(pluginDir).map((method) => `${pluginDir}:${method}`));

    expect(callable).toEqual([]);
  });
});
