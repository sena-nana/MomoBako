// 校验插件双清单与所有 artifact 后，将构建目录打包为可安装的 .momoplug 归档。
import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import JSZip from "jszip";
import {
  readMomoPluginManifest,
  validatePluginPackage,
} from "./plugin-package-manifest.ts";

const __dirname = dirname(fileURLToPath(import.meta.url));
const pluginsRoot = resolve(__dirname, "..");
const distRoot = join(pluginsRoot, ".dist");
const packagesRoot = join(pluginsRoot, ".packages");
const requestedPluginIds = new Set(process.argv.slice(2).filter(Boolean));

const DETERMINISTIC_ZIP_DATE = new Date("1980-01-01T00:00:00.000Z");

/** 按稳定路径顺序递归写入插件目录，并固定 ZIP 元数据时间。 */
function addDirectory(zip: JSZip, sourceDir: string, baseDir: string): void {
  const entries = readdirSync(sourceDir, { withFileTypes: true })
    .sort((left, right) => left.name.localeCompare(right.name, "en"));
  for (const entry of entries) {
    const absolutePath = join(sourceDir, entry.name);
    const archivePath = relative(baseDir, absolutePath).replace(/\\/g, "/");
    if (entry.isDirectory()) {
      addDirectory(zip, absolutePath, baseDir);
      continue;
    }
    if (entry.isFile()) {
      zip.file(archivePath, readFileSync(absolutePath), {
        binary: true,
        date: DETERMINISTIC_ZIP_DATE,
        createFolders: false,
      });
    }
  }
}

rmSync(packagesRoot, { recursive: true, force: true });
mkdirSync(packagesRoot, { recursive: true });
let packagedCount = 0;
let expectedCount = 0;

for (const name of readdirSync(distRoot, { withFileTypes: true }).sort((left, right) => left.name.localeCompare(right.name, "en"))) {
  if (!name.isDirectory()) continue;
  const pluginDir = join(distRoot, name.name);
  const manifestPath = join(pluginDir, "manifest.json");
  if (!existsSync(manifestPath)) continue;
  const manifest = readMomoPluginManifest(manifestPath);
  if (
    requestedPluginIds.size === 0
      ? ["example", "template"].includes(name.name)
      : !requestedPluginIds.has(name.name) && !requestedPluginIds.has(manifest.pluginId)
  ) {
    continue;
  }
  const { packageEnvelope } = validatePluginPackage(pluginDir);
  expectedCount += 1;
  const packageName = `${name.name}-${manifest.version}-${packageEnvelope.targetTriple}.momoplug`;
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
