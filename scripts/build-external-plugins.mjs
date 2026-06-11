import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const __dirname = dirname(fileURLToPath(import.meta.url));
const root = resolve(__dirname, "..");
const pluginsRoot = resolve(root, "External", "Plugins");
const result = process.platform === "win32"
  ? spawnSync("powershell.exe", ["-NoProfile", "-Command", "yarn build"], {
      cwd: pluginsRoot,
      stdio: "inherit",
    })
  : spawnSync("corepack", ["yarn", "build"], {
      cwd: pluginsRoot,
      stdio: "inherit",
    });

if (result.status !== 0) {
  process.exit(result.status ?? 1);
}
