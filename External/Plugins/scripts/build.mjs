import { build as esbuild } from "esbuild";
import { cpSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const __dirname = dirname(fileURLToPath(import.meta.url));
const pluginsRoot = resolve(__dirname, "..");
const repoRoot = resolve(pluginsRoot, "..", "..");
const distRoot = join(pluginsRoot, ".dist");
const sdkRoot = join(pluginsRoot, "_sdk");
const cargoCommand = process.platform === "win32" ? "cargo.exe" : "cargo";
const requestedPluginIds = new Set(process.argv.slice(2).filter(Boolean));

function readJson(path) {
  return JSON.parse(readFileSync(path, "utf-8"));
}

function dynamicLibraryFileName(libraryName) {
  if (process.platform === "win32") return `${libraryName}.dll`;
  if (process.platform === "darwin") return `lib${libraryName}.dylib`;
  return `lib${libraryName}.so`;
}

async function buildFrontendPlugin(pluginDir, outputDir, manifest, project) {
  const frontendEntry = manifest.entry?.frontend?.module;
  if (!frontendEntry) {
    throw new Error(`frontend plugin is missing entry.frontend.module: ${pluginDir}`);
  }

  const sourceDir = project.build?.sourceDir ?? "src";
  const sourceEntry = project.build?.sourceEntry ?? join(sourceDir, "register.js");
  const sourcePath = join(pluginDir, sourceEntry);
  if (!existsSync(sourcePath)) {
    throw new Error(`missing frontend plugin source: ${sourcePath}`);
  }

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

function buildNativePlugin(pluginDir, manifest) {
  const manifestPath = join(pluginDir, "Cargo.toml");
  if (!existsSync(manifestPath)) {
    throw new Error(`missing Cargo.toml for native plugin: ${pluginDir}`);
  }
  const result = spawnSync(
    cargoCommand,
    ["build", "--release", "--manifest-path", manifestPath],
    {
      cwd: repoRoot,
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

function copyPluginProject(pluginDir, outputDir) {
  for (const entry of readdirSync(pluginDir, { withFileTypes: true })) {
    if (["target", ".dist", ".packages", "node_modules"].includes(entry.name)) continue;
    if (entry.isDirectory()) {
      cpSync(join(pluginDir, entry.name), join(outputDir, entry.name), { recursive: true });
      continue;
    }
    cpSync(join(pluginDir, entry.name), join(outputDir, entry.name));
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
  if (!existsSync(projectPath)) {
    throw new Error(`missing plugin.project.json: ${pluginDir}`);
  }

  const manifest = readJson(manifestPath);
  const project = readJson(projectPath);
  if (requestedPluginIds.size > 0 && !requestedPluginIds.has(name.name) && !requestedPluginIds.has(project.pluginId)) {
    continue;
  }
  const outputDir = join(distRoot, name.name);
  rmSync(outputDir, { recursive: true, force: true });
  mkdirSync(outputDir, { recursive: true });
  copyPluginProject(pluginDir, outputDir);

  if (existsSync(sdkRoot)) {
    cpSync(sdkRoot, join(outputDir, "_sdk"), { recursive: true });
  }

  if (project.build?.type === "frontend-module") {
    await buildFrontendPlugin(pluginDir, outputDir, manifest, project);
  }

  if (project.build?.type === "cargo-native") {
    const { fileName, builtLibraryPath } = buildNativePlugin(pluginDir, manifest);
    cpSync(builtLibraryPath, join(outputDir, fileName));
  }

  console.log(`[build-external-plugins] prepared ${name.name}`);
}
