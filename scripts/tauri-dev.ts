// 为 Tauri 开发模式选择空闲端口，并通过统一 Yarn 启动器运行桌面进程。
import { createServer } from "node:net";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { spawnYarn } from "./yarn-command.ts";

const DEFAULT_PORT = 1420;
const MAX_PORT_ATTEMPTS = 100;
const __dirname = dirname(fileURLToPath(import.meta.url));

/** 检查本机回环地址上的端口是否可绑定。 */
function canListen(port: number): Promise<boolean> {
  return new Promise((resolve) => {
    const server = createServer();

    server.once("error", () => {
      resolve(false);
    });
    server.once("listening", () => {
      server.close(() => resolve(true));
    });
    server.listen({ host: "127.0.0.1", port, exclusive: true });
  });
}

/** 在有限范围内选择第一个可用的前端开发端口。 */
async function findAvailablePort(startPort: number): Promise<number> {
  for (let offset = 0; offset < MAX_PORT_ATTEMPTS; offset += 1) {
    const port = startPort + offset;
    if (await canListen(port)) {
      return port;
    }
  }

  throw new Error(
    `No available frontend dev port found from ${startPort} to ${
      startPort + MAX_PORT_ATTEMPTS - 1
    }`,
  );
}

const port = await findAvailablePort(DEFAULT_PORT);
const devUrl = `http://127.0.0.1:${port}`;
const beforeDevCommand = `yarn vite --host 127.0.0.1 --port ${port} --strictPort`;
const config = JSON.stringify({
  build: {
    devUrl,
    beforeDevCommand,
  },
});
const args = [
  "tauri",
  "dev",
  "--config",
  config,
  ...process.argv.slice(2),
];

console.log(`[tauri-dev] frontend dev server: ${devUrl}`);

try {
  const child = spawnYarn(args, {
    stdio: "inherit",
    cwd: join(__dirname, ".."),
  });

  child.on("exit", (code) => {
    process.exit(code ?? 1);
  });

  child.on("error", (error) => {
    console.error(`[tauri-dev] failed to start Tauri dev: ${error.message}`);
    process.exit(1);
  });
} catch (error) {
  const message = error instanceof Error ? error.message : String(error);
  console.error(`[tauri-dev] failed to start Tauri dev: ${message}`);
  process.exit(1);
}
