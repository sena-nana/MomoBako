import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { runYarn } from "./yarn-command.ts";

const __dirname = dirname(fileURLToPath(import.meta.url));
const root = resolve(__dirname, "..");
const pluginsRoot = resolve(root, "External", "Plugins");
const result = runYarn(["stage:dev"], {
  cwd: pluginsRoot,
  stdio: "inherit",
});

if (result.status !== 0) {
  process.exit(result.status ?? 1);
}
