import { copyFileSync, existsSync, mkdirSync, rmSync } from "node:fs";
import { basename, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const __dirname = dirname(fileURLToPath(import.meta.url));
const root = join(__dirname, "..");
const builtinPluginsRoot = join(root, "plugins", "builtin");
const tauriBuiltinResourceRoot = join(
  root,
  "src-tauri",
  "resources",
  "plugins",
  "builtin",
);
const cargoCommand = process.platform === "win32" ? "cargo.exe" : "cargo";

const plugins = [
  {
    name: "local-filesystem",
    packageDir: join(builtinPluginsRoot, "local-filesystem"),
    manifestPath: join(builtinPluginsRoot, "local-filesystem", "Cargo.toml"),
    packageName: "momobako_builtin_local_filesystem",
  },
];

const runtimePluginPackages = [
  { name: "cloud-drive", files: ["manifest.json"] },
  { name: "filesystem-watcher", files: ["manifest.json"] },
  { name: "local-filesystem", files: ["manifest.json"] },
  { name: "media-preview", files: ["manifest.json", "preview.ts"] },
  { name: "metadata-provider", files: ["manifest.json"] },
  { name: "three-model-preview", files: ["manifest.json", "preview.ts"] },
  { name: "vector-index", files: ["manifest.json"] },
  { name: "webdav", files: ["manifest.json"] },
];

function dynamicLibraryFileName(packageName) {
  if (process.platform === "win32") return `${packageName}.dll`;
  if (process.platform === "darwin") return `lib${packageName}.dylib`;
  return `lib${packageName}.so`;
}

function copyRuntimePluginFile(pluginName, fileName) {
  const source = join(builtinPluginsRoot, pluginName, fileName);
  const target = join(tauriBuiltinResourceRoot, pluginName, fileName);
  if (!existsSync(source)) {
    console.error(`[build-builtin-plugins] missing runtime file: ${source}`);
    process.exit(1);
  }
  mkdirSync(dirname(target), { recursive: true });
  copyFileSync(source, target);
}

for (const plugin of plugins) {
  const result = spawnSync(
    cargoCommand,
    ["build", "--release", "--manifest-path", plugin.manifestPath],
    {
      cwd: root,
      stdio: "inherit",
    },
  );
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }

  const fileName = dynamicLibraryFileName(plugin.packageName);
  const source = join(
    plugin.packageDir,
    "target",
    "release",
    fileName,
  );
  const target = join(plugin.packageDir, fileName);
  if (!existsSync(source)) {
    console.error(`[build-builtin-plugins] missing built library: ${source}`);
    process.exit(1);
  }
  copyFileSync(source, target);
  console.log(`[build-builtin-plugins] ${plugin.name}: ${basename(target)}`);
}

rmSync(tauriBuiltinResourceRoot, { recursive: true, force: true });
for (const plugin of runtimePluginPackages) {
  for (const fileName of plugin.files) {
    copyRuntimePluginFile(plugin.name, fileName);
  }
}
for (const plugin of plugins) {
  const fileName = dynamicLibraryFileName(plugin.packageName);
  copyRuntimePluginFile(plugin.name, fileName);
}
console.log(
  `[build-builtin-plugins] staged resources: ${tauriBuiltinResourceRoot}`,
);
