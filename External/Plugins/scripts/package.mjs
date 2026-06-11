import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { join, relative, resolve } from "node:path";
import { dirname } from "node:path";
import { fileURLToPath } from "node:url";
import JSZip from "jszip";

const __dirname = dirname(fileURLToPath(import.meta.url));
const pluginsRoot = resolve(__dirname, "..");
const distRoot = join(pluginsRoot, ".dist");
const packagesRoot = join(pluginsRoot, ".packages");
const requestedPluginIds = new Set(process.argv.slice(2).filter(Boolean));

function addDirectory(zip, sourceDir, baseDir) {
  for (const entry of readdirSync(sourceDir, { withFileTypes: true })) {
    const absolutePath = join(sourceDir, entry.name);
    const archivePath = relative(baseDir, absolutePath).replace(/\\/g, "/");
    if (entry.isDirectory()) {
      zip.folder(archivePath);
      addDirectory(zip, absolutePath, baseDir);
      continue;
    }
    if (entry.isFile()) {
      zip.file(archivePath, readFileSync(absolutePath), {
        binary: true,
        date: statSync(absolutePath).mtime,
      });
    }
  }
}

rmSync(packagesRoot, { recursive: true, force: true });
mkdirSync(packagesRoot, { recursive: true });
let packagedCount = 0;
let expectedCount = 0;

for (const name of readdirSync(distRoot, { withFileTypes: true })) {
  if (!name.isDirectory()) continue;
  const pluginDir = join(distRoot, name.name);
  const manifestPath = join(pluginDir, "manifest.json");
  if (!existsSync(manifestPath)) continue;
  const manifest = JSON.parse(readFileSync(manifestPath, "utf-8"));
  if (
    requestedPluginIds.size === 0
      ? ["example", "template"].includes(name.name)
      : !requestedPluginIds.has(name.name) && !requestedPluginIds.has(manifest.pluginId)
  ) {
    continue;
  }
  expectedCount += 1;
  const packageName = `${name.name}-${manifest.version}.momoplug`;
  const packagePath = join(packagesRoot, packageName);
  const zip = new JSZip();

  addDirectory(zip, pluginDir, distRoot);
  const archive = await zip.generateAsync({
    type: "nodebuffer",
    compression: "DEFLATE",
    compressionOptions: {
      level: 9,
    },
  });
  writeFileSync(packagePath, archive);
  packagedCount += 1;

  console.log(`[package-external-plugins] packaged ${packageName}`);
}

if (packagedCount === 0) {
  throw new Error("no plugin build outputs found to package");
}
if (packagedCount !== expectedCount) {
  throw new Error(`packaged plugin count mismatch: expected ${expectedCount}, got ${packagedCount}`);
}
