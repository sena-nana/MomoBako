// 统一解析项目锁定的 Yarn 入口，避免依赖 Node 安装目录中的 Corepack 私有路径。
import { spawn, spawnSync, type SpawnOptions, type SpawnSyncOptions } from "node:child_process";

export interface YarnCommandOptions {
  comSpec?: string;
  npmExecPath?: string;
  platform?: NodeJS.Platform;
}

export interface YarnCommand {
  command: string;
  args: string[];
}

/** 优先复用当前 Yarn 进程入口，独立执行时通过显式安装的 Corepack 启动。 */
export function resolveYarnCommand({
  comSpec = process.env.ComSpec ?? "cmd.exe",
  npmExecPath = process.env.npm_execpath,
  platform = process.platform,
}: YarnCommandOptions = {}): YarnCommand {
  const yarnEntry = npmExecPath || (platform === "win32" ? "corepack.cmd" : "corepack");

  if (platform === "win32" && /\.(?:cmd|bat)$/i.test(yarnEntry)) {
    return {
      command: comSpec,
      args: ["/d", "/s", "/c", yarnEntry, ...(npmExecPath ? [] : ["yarn"])],
    };
  }

  return {
    command: yarnEntry,
    args: npmExecPath ? [] : ["yarn"],
  };
}

/** 同步执行嵌套 Yarn 项目命令，并保留调用方的工作目录和 stdio。 */
export function runYarn(
  args: readonly string[],
  options: SpawnSyncOptions,
) {
  const yarn = resolveYarnCommand();
  return spawnSync(yarn.command, [...yarn.args, ...args], options);
}

/** 启动长生命周期 Yarn 子进程，供 Tauri 开发服务器使用。 */
export function spawnYarn(
  args: readonly string[],
  options: SpawnOptions,
) {
  const yarn = resolveYarnCommand();
  return spawn(yarn.command, [...yarn.args, ...args], options);
}
