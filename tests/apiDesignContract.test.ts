import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { getApiDesignSnapshot } from "../src/services/repositoryApi";

function tauriHandlerCommands() {
  const source = readFileSync(resolve("src-tauri/src/lib.rs"), "utf-8");
  const handler = source.match(/tauri::generate_handler!\[([\s\S]*?)\]/)?.[1] ?? "";
  return handler
    .split(",")
    .map((command) => command.trim())
    .filter(Boolean);
}

function apiDefinitionCommands() {
  const source = readFileSync(resolve("src-tauri/src/services/repository/api_design.rs"), "utf-8");
  return [...source.matchAll(/tauri_api_definition\([^,]+,\s*"([^"]+)"/g)]
    .map((match) => match[1])
    .filter(Boolean);
}

function externalRuntimeRoutes() {
  const source = readFileSync(resolve("src-tauri/src/services/runtime/external_api.rs"), "utf-8");
  return [...source.matchAll(/\(&Method::([A-Za-z]+),\s*"([^"]+)"\)\s*=>/g)]
    .map((match) => `${match[1].toUpperCase()} ${match[2]}`)
    .filter((route) => route.includes(" /external/v1/"));
}

function externalDefinitionRoutes() {
  const source = readFileSync(resolve("src-tauri/src/services/repository/api_design.rs"), "utf-8");
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
  const source = readFileSync(resolve("src-tauri/src/services/repository/mod.rs"), "utf-8");
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
  it("keeps all Tauri commands represented in the API snapshot definitions", () => {
    const expected = tauriHandlerCommands();
    const covered = new Set(apiDefinitionCommands());

    expect(expected.filter((command) => !covered.has(command))).toEqual([]);
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

  it("exposes external, core, and plugin API transports to API Playground", async () => {
    const snapshot = await getApiDesignSnapshot();

    expect(snapshot.endpoints).toEqual(expect.arrayContaining([
      expect.objectContaining({
        transport: "external-http",
        path: "/external/v1/health",
      }),
      expect.objectContaining({
        transport: "tauri-command",
        command: "list_repositories",
      }),
      expect.objectContaining({
        transport: "plugin-call",
        pluginId: "momobako.service.provider.dlsite",
        pluginMethod: "provider.lookupMetadataCandidate",
      }),
      expect.objectContaining({
        transport: "plugin-call",
        pluginId: "momobako.local-filesystem",
        pluginMethod: "filesystem.listFiles",
      }),
      expect.objectContaining({
        transport: "tauri-command",
        command: "download_playlist_with_progress",
      }),
      expect.objectContaining({
        transport: "tauri-command",
        command: "prepare_repository_cache_file_preview_source",
      }),
      expect.objectContaining({
        transport: "plugin-call",
        pluginId: "momobako.service.office-convert",
        pluginMethod: "officeConvert.ensurePreviewPdf",
      }),
      expect.objectContaining({
        transport: "plugin-call",
        pluginId: "momobako.service.office-convert",
        pluginMethod: "officeConvert.getRuntimeStatus",
      }),
      expect.objectContaining({
        transport: "plugin-call",
        pluginId: "momobako.service.office-convert",
        pluginMethod: "officeConvert.clearPreviewCache",
      }),
      expect.objectContaining({
        transport: "plugin-call",
        pluginId: "momobako.service.office-convert",
        pluginMethod: "officeConvert.runRuntimeSelfCheck",
      }),
      expect.objectContaining({
        transport: "plugin-call",
        pluginId: "momobako.service.office-convert",
        pluginMethod: "officeConvert.shutdownDaemon",
      }),
      expect.objectContaining({
        transport: "plugin-call",
        pluginId: "momobako.service.downloader",
        pluginMethod: "downloader.ensureRuntime",
      }),
      expect.objectContaining({
        transport: "plugin-call",
        pluginId: "momobako.service.downloader",
        pluginMethod: "downloader.enqueueDownload",
      }),
      expect.objectContaining({
        transport: "plugin-call",
        pluginId: "momobako.service.downloader",
        pluginMethod: "downloader.awaitDownload",
      }),
      expect.objectContaining({
        transport: "plugin-call",
        pluginId: "momobako.service.downloader",
        pluginMethod: "downloader.removeDownload",
      }),
    ]));
  });
});
