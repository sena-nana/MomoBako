import { build as esbuild } from "esbuild";
import { cpSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
import { parse } from "smol-toml";
import {
  assertDeclaredCompanionArtifacts,
  refreshPluginArtifactHashes,
  resolvePluginPackagePath,
  writePluginPackageEnvelope,
} from "./plugin-package-manifest.ts";

const __dirname = dirname(fileURLToPath(import.meta.url));
const pluginsRoot = resolve(__dirname, "..");
const repoRoot = resolve(pluginsRoot, "..", "..");
const distRoot = join(pluginsRoot, ".dist");
const cargoCommand = process.platform === "win32" ? "cargo.exe" : "cargo";
const rustcCommand = process.platform === "win32" ? "rustc.exe" : "rustc";
const rawArguments = process.argv.slice(2);
const profileArgumentIndex = rawArguments.indexOf("--profile");
const buildProfile = profileArgumentIndex >= 0 ? rawArguments[profileArgumentIndex + 1] : "release";
if (!buildProfile || buildProfile.startsWith("--")) {
  throw new Error("--profile requires a Cargo profile name");
}
const requestedPluginIds = new Set(
  rawArguments.filter((value, index) => (
    profileArgumentIndex < 0
    || (index !== profileArgumentIndex && index !== profileArgumentIndex + 1)
  )),
);
const cargoOutputProfile = buildProfile === "dev" ? "debug" : buildProfile;

function cargoBuildEnvironment() {
  const environment = { ...process.env };
  if (process.env.MOMO_USE_SCCACHE === "1") {
    const sccache = spawnSync("sccache", ["--version"], { encoding: "utf8" });
    if (sccache.status !== 0) {
      throw new Error("MOMO_USE_SCCACHE=1 requires sccache on PATH");
    }
    environment.RUSTC_WRAPPER = "sccache";
  }
  if (process.platform === "win32") {
    const lintList = spawnSync(rustcCommand, ["-W", "help"], { encoding: "utf8" });
    if (lintList.status === 0 && lintList.stdout.includes("linker-messages")) {
      // 中文 MSVC 会把正常的导入库提示归入 linker-messages；仅在支持该 lint 时关闭误报。
      environment.RUSTFLAGS = [environment.RUSTFLAGS?.trim(), "-A linker-messages"]
        .filter(Boolean)
        .join(" ");
    }
  }
  return environment;
}

const cargoEnvironment = cargoBuildEnvironment();

function rustTargetTriple() {
  const result = spawnSync(rustcCommand, ["-vV"], { encoding: "utf8" });
  if (result.status !== 0) throw new Error("failed to determine the Rust host target triple");
  const hostLine = result.stdout.split(/\r?\n/).find((line) => line.startsWith("host: "));
  if (!hostLine) throw new Error("rustc -vV did not report a host target triple");
  return hostLine.slice("host: ".length).trim();
}

const targetTriple = rustTargetTriple();

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

function nativePluginArtifact(manifest) {
  const libraryName = manifest.entry?.backend?.library;
  if (!libraryName) {
    throw new Error(`native plugin is missing entry.backend.library: ${manifest.pluginId}`);
  }
  const fileName = dynamicLibraryFileName(libraryName);
  const builtLibraryPath = join(repoRoot, "target", cargoOutputProfile, fileName);
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

function companionNativeArtifact(pluginDir, definition) {
  if (!definition.path) {
    throw new Error(`companion native artifact is missing package path: ${pluginDir}`);
  }
  const fileName = definition.path;
  const builtBinaryPath = join(repoRoot, "target", cargoOutputProfile, binaryFileName(definition.binaryName));
  if (!existsSync(builtBinaryPath)) {
    throw new Error(`missing built binary: ${builtBinaryPath}`);
  }
  return {
    fileName,
    builtBinaryPath,
  };
}

function writeDistributionManifest(outputDir) {
  const path = join(outputDir, "manifest.json");
  const manifest = readJson(path);
  manifest.compat = { ...(manifest.compat ?? {}), sdkVersion: "2" };
  writeFileSync(path, `${JSON.stringify(manifest, null, 2)}\n`, "utf8");
}

function processPluginArtifact(record) {
  const binaryName = record.project.build?.binaryName;
  const artifactPath = record.project.build?.artifactPath;
  if (!binaryName || !artifactPath) {
    throw new Error(`process plugin requires build.binaryName and build.artifactPath: ${record.pluginDir}`);
  }
  const builtBinaryPath = join(repoRoot, "target", cargoOutputProfile, binaryFileName(binaryName));
  if (!existsSync(builtBinaryPath)) throw new Error(`missing built process runner: ${builtBinaryPath}`);
  return { artifactPath, builtBinaryPath };
}

function cargoPackageName(manifestPath) {
  const manifest = parse(readFileSync(manifestPath, "utf8"));
  const packageName = manifest.package?.name;
  if (typeof packageName !== "string" || packageName.trim() === "") {
    throw new Error(`Cargo manifest is missing package.name: ${manifestPath}`);
  }
  return packageName;
}

/** 所有原生插件共享一次 Cargo 解析与编译图，统一复用根 target 与 Cargo.lock。 */
function buildNativeWorkspace(records) {
  const packageNames = new Set();
  for (const record of records) {
    packageNames.add(cargoPackageName(nativeManifestPath(record.pluginDir, record.project)));
    for (const definition of record.companionDefinitions) {
      packageNames.add(cargoPackageName(join(record.pluginDir, definition.manifestPath)));
    }
  }
  if (packageNames.size === 0) return;

  const argumentsList = [
    "build",
    "--locked",
    "--manifest-path",
    join(repoRoot, "Cargo.toml"),
    "--profile",
    buildProfile,
  ];
  for (const packageName of packageNames) argumentsList.push("--package", packageName);
  const result = spawnSync(cargoCommand, argumentsList, {
    cwd: repoRoot,
    env: cargoEnvironment,
    stdio: "inherit",
  });
  if (result.status !== 0) process.exit(result.status ?? 1);
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

function distributionExcludePaths(project) {
  const excluded = new Set([
    ...(project.build?.excludePaths ?? []),
    "Cargo.lock",
    "Cargo.toml",
    "build.rs",
    "plugin.project.json",
  ]);
  if (project.build?.sourceDir) {
    excluded.add(project.build.sourceDir);
  } else if (["cargo-native", "cargo-process", "frontend-module"].includes(project.build?.type)) {
    excluded.add("src");
  }
  for (const companion of project.build?.companionArtifacts ?? []) {
    const manifestDirectory = dirname(companion.manifestPath).replace(/\\/g, "/");
    if (manifestDirectory !== ".") excluded.add(manifestDirectory);
  }
  return [...excluded];
}

const preparedPlugins = [];
const nativeBuilds = [];

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
  copyPluginProject(pluginDir, outputDir, pluginDir, distributionExcludePaths(project));
  writeDistributionManifest(outputDir);

  const buildType = project.build?.type;
  if (buildType === "frontend-module") {
    const sourcePath = frontendSourcePath(pluginDir, project);
    if (existsSync(sourcePath)) {
      await buildFrontendPlugin(pluginDir, outputDir, manifest, project);
    } else {
      console.log(`[build-external-plugins] skipped compile for ${name.name}: missing frontend source ${sourcePath}`);
    }
  }

  if (buildType === "cargo-native" || buildType === "cargo-process") {
    const cargoManifestPath = nativeManifestPath(pluginDir, project);
    if (existsSync(cargoManifestPath)) {
      const companionDefinitions = project.build?.companionArtifacts ?? [];
      assertDeclaredCompanionArtifacts(
        outputDir,
        companionDefinitions.map((definition) => definition.path),
      );
      nativeBuilds.push({ pluginDir, outputDir, manifest, project, companionDefinitions });
    } else {
      console.log(`[build-external-plugins] skipped compile for ${name.name}: missing native manifest ${cargoManifestPath}`);
    }
  }

  preparedPlugins.push({ name: name.name, outputDir });
}

buildNativeWorkspace(nativeBuilds);
for (const record of nativeBuilds) {
  if (record.project.build?.type === "cargo-process") {
    const { artifactPath, builtBinaryPath } = processPluginArtifact(record);
    const outputPath = resolvePluginPackagePath(record.outputDir, artifactPath, "process plugin artifact");
    mkdirSync(dirname(outputPath), { recursive: true });
    cpSync(builtBinaryPath, outputPath);
  } else {
    const { fileName, builtLibraryPath } = nativePluginArtifact(record.manifest);
    cpSync(builtLibraryPath, join(record.outputDir, fileName));
  }
  for (const definition of record.companionDefinitions) {
    const { fileName: extraFileName, builtBinaryPath } = companionNativeArtifact(record.pluginDir, definition);
    const outputPath = resolvePluginPackagePath(record.outputDir, extraFileName, "companion build artifact");
    mkdirSync(dirname(outputPath), { recursive: true });
    cpSync(builtBinaryPath, outputPath);
  }
}

for (const plugin of preparedPlugins) {
  refreshPluginArtifactHashes(plugin.outputDir);
  writePluginPackageEnvelope(plugin.outputDir, targetTriple);
  console.log(`[build-external-plugins] prepared ${plugin.name}`);
}
