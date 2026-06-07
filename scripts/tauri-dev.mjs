import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { createServer } from "node:net";
import { delimiter, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const DEFAULT_PORT = 1420;
const MAX_PORT_ATTEMPTS = 100;
const __dirname = dirname(fileURLToPath(import.meta.url));

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

function yarnCommand() {
  const searchDirs = [
    dirname(process.execPath),
    ...(process.env.PATH ?? "").split(delimiter),
  ];

  for (const dir of searchDirs) {
    const corepackYarn = join(
      dir,
      "node_modules",
      "corepack",
      "dist",
      "yarn.js",
    );
    if (existsSync(corepackYarn)) {
      return {
        command: process.execPath,
        args: [corepackYarn],
      };
    }
  }

  return {
    command: process.platform === "win32" ? "yarn.cmd" : "yarn",
    args: [],
  };
}

const yarn = yarnCommand();
const args = [
  ...yarn.args,
  "tauri",
  "dev",
  "--config",
  config,
  ...process.argv.slice(2),
];

console.log(`[tauri-dev] frontend dev server: ${devUrl}`);

try {
  const child = spawn(yarn.command, args, {
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
  console.error(`[tauri-dev] failed to start Tauri dev: ${error.message}`);
  process.exit(1);
}
