import { spawn } from "node:child_process";
import { createServer } from "node:net";

const DEFAULT_PORT = 1420;
const MAX_PORT_ATTEMPTS = 100;

function canListen(port) {
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

async function findAvailablePort(startPort) {
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
const yarn = process.platform === "win32" ? "yarn.cmd" : "yarn";
const args = ["tauri", "dev", "--config", config, ...process.argv.slice(2)];

console.log(`[tauri-dev] frontend dev server: ${devUrl}`);

const child = spawn(yarn, args, {
  stdio: "inherit",
});

child.on("exit", (code) => {
  process.exit(code ?? 1);
});

child.on("error", (error) => {
  console.error(`[tauri-dev] failed to start Tauri dev: ${error.message}`);
  process.exit(1);
});
