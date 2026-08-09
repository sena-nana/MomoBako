import { build as esbuild } from "esbuild";
import { cpSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
import {
  assertDeclaredCompanionArtifacts,
  refreshPluginArtifactHashes,
  resolvePluginPackagePath,
} from "./plugin-package-manifest.ts";

const __dirname = dirname(fileURLToPath(import.meta.url));
const pluginsRoot = resolve(__dirname, "..");
const repoRoot = resolve(pluginsRoot, "..", "..");
const distRoot = join(pluginsRoot, ".dist");
const cargoCommand = process.platform === "win32" ? "cargo.exe" : "cargo";
const rustcCommand = process.platform === "win32" ? "rustc.exe" : "rustc";
const requestedPluginIds = new Set(process.argv.slice(2).filter(Boolean));

function cargoBuildEnvironment() {
  if (process.platform !== "win32") return process.env;

  const environment = { ...process.env };
  const lintList = spawnSync(rustcCommand, ["-W", "help"], { encoding: "utf8" });
  if (lintList.status === 0 && lintList.stdout.includes("linker-messages")) {
    // 中文 MSVC 会把正常的导入库提示归入 linker-messages；仅在支持该 lint 时关闭误报。
    environment.RUSTFLAGS = [environment.RUSTFLAGS?.trim(), "-A linker-messages"]
      .filter(Boolean)
      .join(" ");
  }
  return environment;
}

const cargoEnvironment = cargoBuildEnvironment();

function readJson(path) {
  return JSON.parse(readFileSync(path, "utf-8"));
}

function dynamicLibraryFileName(libraryName) {
  if (process.platform === "win32") return `${libraryName}.dll`;
  if (process.platform === "darwin") return `lib${libraryName}.dylib`;
  return `lib${libraryName}.so`;
}

function createManifestOnlyProject(manifest) {
  return {
    pluginId: manifest.pluginId,
    build: {
      type: "manifest-only",
    },
  };
}

function pluginMatchesRequest(name, manifest, project) {
  if (requestedPluginIds.size === 0) return true;
  return [name, manifest.pluginId, project.pluginId]
    .filter(Boolean)
    .some((id) => requestedPluginIds.has(id));
}

function frontendSourcePath(pluginDir, project) {
  const sourceDir = project.build?.sourceDir ?? "src";
  const sourceEntry = project.build?.sourceEntry ?? join(sourceDir, "register.js");
  return join(pluginDir, sourceEntry);
}

function nativeManifestPath(pluginDir, project) {
  return join(pluginDir, project.build?.manifestPath ?? "Cargo.toml");
}

async function buildFrontendPlugin(pluginDir, outputDir, manifest, project) {
  const frontendEntry = manifest.entry?.frontend?.module;
  if (!frontendEntry) {
    throw new Error(`frontend plugin is missing entry.frontend.module: ${pluginDir}`);
  }

  const sourcePath = frontendSourcePath(pluginDir, project);
  const entryPath = join(outputDir, frontendEntry);
  mkdirSync(dirname(entryPath), { recursive: true });

  await esbuild({
    absWorkingDir: pluginDir,
    entryPoints: [sourcePath],
    outfile: entryPath,
    bundle: true,
    format: "esm",
    platform: "browser",
    target: "es2022",
    charset: "utf8",
    sourcemap: false,
    minify: false,
    logLevel: "silent",
    legalComments: "none",
  });
}

function buildNativePlugin(pluginDir, manifest, project) {
  const manifestPath = nativeManifestPath(pluginDir, project);
  const result = spawnSync(
    cargoCommand,
    ["build", "--release", "--manifest-path", manifestPath],
    {
      cwd: repoRoot,
      env: cargoEnvironment,
      stdio: "inherit",
    },
  );
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }

  const libraryName = manifest.entry?.backend?.library;
  if (!libraryName) {
    throw new Error(`native plugin is missing entry.backend.library: ${pluginDir}`);
  }
  const fileName = dynamicLibraryFileName(libraryName);
  const builtLibraryPath = join(pluginDir, "target", "release", fileName);
  if (!existsSync(builtLibraryPath)) {
    throw new Error(`missing built library: ${builtLibraryPath}`);
  }
  return {
    fileName,
    builtLibraryPath,
  };
}

function binaryFileName(binaryName) {
  if (process.platform === "win32") return `${binaryName}.exe`;
  return binaryName;
}

function buildCompanionNativeArtifact(pluginDir, definition) {
  if (!definition.path) {
    throw new Error(`companion native artifact is missing package path: ${pluginDir}`);
  }
  const manifestPath = join(pluginDir, definition.manifestPath);
  const binaryRoot = dirname(manifestPath);
  const result = spawnSync(
    cargoCommand,
    ["build", "--release", "--manifest-path", manifestPath],
    {
      cwd: repoRoot,
      env: cargoEnvironment,
      stdio: "inherit",
    },
  );
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
  const fileName = definition.path;
  const builtBinaryPath = join(binaryRoot, "target", "release", binaryFileName(definition.binaryName));
  if (!existsSync(builtBinaryPath)) {
    throw new Error(`missing built binary: ${builtBinaryPath}`);
  }
  return {
    fileName,
    builtBinaryPath,
  };
}

function shouldSkipCopy(name) {
  return ["target", ".dist", ".packages", "node_modules"].includes(name);
}

function isExcludedRelativePath(relativePath, excludePaths = []) {
  const normalized = relativePath.replace(/\\/g, "/");
  return excludePaths.some((candidate) => {
    const normalizedCandidate = candidate.replace(/\\/g, "/");
    return normalized === normalizedCandidate || normalized.startsWith(`${normalizedCandidate}/`);
  });
}

function copyPluginProject(sourceDir, outputDir, rootDir = sourceDir, excludePaths = []) {
  mkdirSync(outputDir, { recursive: true });
  for (const entry of readdirSync(sourceDir, { withFileTypes: true })) {
    if (shouldSkipCopy(entry.name)) continue;
    const sourcePath = join(sourceDir, entry.name);
    const targetPath = join(outputDir, entry.name);
    const relativePath = sourcePath.slice(rootDir.length + 1).replace(/\\/g, "/");
    if (isExcludedRelativePath(relativePath, excludePaths)) continue;
    if (entry.isDirectory()) {
      copyPluginProject(sourcePath, targetPath, rootDir, excludePaths);
      continue;
    }
    cpSync(sourcePath, targetPath);
  }
}

if (requestedPluginIds.size === 0) {
  rmSync(distRoot, { recursive: true, force: true });
  mkdirSync(distRoot, { recursive: true });
} else {
  mkdirSync(distRoot, { recursive: true });
}

for (const name of readdirSync(pluginsRoot, { withFileTypes: true })) {
  if (!name.isDirectory()) continue;
  if (name.name.startsWith(".") || ["_sdk", "scripts", "node_modules"].includes(name.name)) continue;
  const pluginDir = join(pluginsRoot, name.name);
  const manifestPath = join(pluginDir, "manifest.json");
  const projectPath = join(pluginDir, "plugin.project.json");
  if (!existsSync(manifestPath)) continue;

  const manifest = readJson(manifestPath);
  const project = existsSync(projectPath)
    ? readJson(projectPath)
    : createManifestOnlyProject(manifest);
  if (!pluginMatchesRequest(name.name, manifest, project)) {
    continue;
  }
  const outputDir = join(distRoot, name.name);
  rmSync(outputDir, { recursive: true, force: true });
  mkdirSync(outputDir, { recursive: true });
  // 分发产物只保留插件自身运行时文件，开发 SDK 继续留在源码目录参与编译。
  copyPluginProject(pluginDir, outputDir, pluginDir, project.build?.excludePaths ?? []);

  const buildType = project.build?.type;
  if (buildType === "frontend-module") {
    const sourcePath = frontendSourcePath(pluginDir, project);
    if (existsSync(sourcePath)) {
      await buildFrontendPlugin(pluginDir, outputDir, manifest, project);
    } else {
      console.log(`[build-external-plugins] skipped compile for ${name.name}: missing frontend source ${sourcePath}`);
    }
  }

  if (buildType === "cargo-native") {
    const cargoManifestPath = nativeManifestPath(pluginDir, project);
    if (existsSync(cargoManifestPath)) {
      const companionDefinitions = project.build?.companionArtifacts ?? [];
      assertDeclaredCompanionArtifacts(
        outputDir,
        companionDefinitions.map((definition) => definition.path),
      );
      const { fileName, builtLibraryPath } = buildNativePlugin(pluginDir, manifest, project);
      cpSync(builtLibraryPath, join(outputDir, fileName));
      for (const definition of companionDefinitions) {
        const { fileName: extraFileName, builtBinaryPath } = buildCompanionNativeArtifact(pluginDir, definition);
        const outputPath = resolvePluginPackagePath(outputDir, extraFileName, "companion build artifact");
        mkdirSync(dirname(outputPath), { recursive: true });
        cpSync(builtBinaryPath, outputPath);
      }
    } else {
      console.log(`[build-external-plugins] skipped compile for ${name.name}: missing native manifest ${cargoManifestPath}`);
    }
  }

  refreshPluginArtifactHashes(outputDir);
  console.log(`[build-external-plugins] prepared ${name.name}`);
}
