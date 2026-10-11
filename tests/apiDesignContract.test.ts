import { existsSync, readFileSync } from "node:fs";
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

function backendPluginMethods(pluginDir: string) {
  const sourcePath = resolve("External/Plugins", pluginDir, "src/lib.rs");
  if (!existsSync(sourcePath)) return [];
  const source = readFileSync(sourcePath, "utf-8");
  return [...source.matchAll(/"([^"]+)"\s*=>/g)]
    .map((match) => match[1])
    .filter((method) => method.includes("."));
}

function embeddedLocalFilesystemMethods() {
  const source = readFileSync(resolve("src-backend/src/services/repository/mod.rs"), "utf-8");
  const fallback = source.match(/fn call_builtin_local_filesystem\([\s\S]*?\n}\n\nfn list_backend_files/)?.[0] ?? "";
  return [...fallback.matchAll(/"([^"]+)"\s*=>/g)]
    .map((match) => match[1])
    .filter((method) => method.startsWith("filesystem."));
}

function apiTestMethods(pluginDir: string) {
  const manifest = JSON.parse(
    readFileSync(resolve("External/Plugins", pluginDir, "manifest.json"), "utf-8"),
  ) as { contributes?: { apiTests?: Array<{ method?: string }> } };
  return (manifest.contributes?.apiTests ?? [])
    .map((test) => test.method)
    .filter(Boolean);
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

  it("keeps backend plugin call methods represented in manifest apiTests", () => {
    const pluginDirs = [
      "local-filesystem",
      "office-convert",
      "parser-asmr-folder",
      "service-downloader",
      "service-provider-asmr-one",
      "service-provider-dlsite",
    ];

    const missing = pluginDirs.flatMap((pluginDir) => {
      const covered = new Set(apiTestMethods(pluginDir));
      return backendPluginMethods(pluginDir)
        .filter((method) => !covered.has(method))
        .map((method) => `${pluginDir}:${method}`);
    });

    const localFilesystemCovered = new Set(apiTestMethods("local-filesystem"));
    const fallbackMissing = embeddedLocalFilesystemMethods()
      .filter((method) => !localFilesystemCovered.has(method))
      .map((method) => `embedded-local-filesystem:${method}`);

    expect([...missing, ...fallbackMissing]).toEqual([]);
  });
});
