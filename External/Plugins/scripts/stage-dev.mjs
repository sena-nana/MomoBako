import { copyFileSync, existsSync, mkdirSync, readdirSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const pluginsRoot = resolve(__dirname, "..");
const repoRoot = resolve(pluginsRoot, "..", "..");
const packagesRoot = join(pluginsRoot, ".packages");
const targetRoot = process.argv[2]?.trim();
const runtimeRoot = targetRoot
  ? resolve(targetRoot, "plugins")
  : join(repoRoot, ".service-data", "plugins");

rmSync(runtimeRoot, { recursive: true, force: true });
mkdirSync(runtimeRoot, { recursive: true });

if (existsSync(packagesRoot)) {
  for (const name of readdirSync(packagesRoot)) {
    if (!name.endsWith(".momoplug")) continue;
    copyFileSync(join(packagesRoot, name), join(runtimeRoot, name));
    console.log(`[stage-external-plugins] staged ${name}`);
  }
}
