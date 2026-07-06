/**
 * 前端统一日志助手，负责补齐来源与位置信息。
 */
import type {
  SystemLogLevel,
  SystemLogLocation,
  SystemLogRecord,
  SystemLogSourceKind,
} from "../types/repository";
import { writeSystemLog } from "./repositoryApi";

type FrontendSystemLogOptions = {
  category: string;
  action: string;
  message: string;
  context?: Record<string, unknown>;
  repoId?: string | null;
  pluginId?: string | null;
  sourceKind?: SystemLogSourceKind | string;
  sourceLabel?: string | null;
  location?: SystemLogLocation | null;
  stackOffset?: number;
};

/**
 * 从调用栈提取日志产生位置，方便界面展示模块与行号。
 */
function parseStackLocation(stackOffset = 2): SystemLogLocation | null {
  const stack = new Error().stack?.split("\n") ?? [];
  const line = stack[stackOffset]?.trim();
  if (!line) return null;
  const match = line.match(/(?:at\s+.*?\()?(.+):(\d+):(\d+)\)?$/);
  if (!match) {
    return {
      modulePath: line.replace(/^at\s+/, ""),
    };
  }
  return {
    modulePath: line.replace(/^at\s+/, ""),
    file: match[1],
    line: Number(match[2]),
  };
}

function normalizeLocation(
  explicitLocation?: SystemLogLocation | null,
  stackOffset?: number,
) {
  const fallback = parseStackLocation(stackOffset);
  return {
    modulePath: explicitLocation?.modulePath ?? fallback?.modulePath ?? null,
    file: explicitLocation?.file ?? fallback?.file ?? null,
    line: explicitLocation?.line ?? fallback?.line ?? null,
  };
}

/**
 * 统一从前端写入结构化日志。
 */
export async function emitSystemLog(
  level: SystemLogLevel | string,
  options: FrontendSystemLogOptions,
): Promise<SystemLogRecord | null> {
  try {
    return await writeSystemLog({
      level,
      category: options.category,
      action: options.action,
      message: options.message,
      context: options.context,
      repoId: options.repoId,
      pluginId: options.pluginId,
      sourceKind: options.sourceKind ?? "frontend-host",
      sourceLabel: options.sourceLabel ?? "MomoBako UI",
      location: normalizeLocation(options.location, options.stackOffset ?? 3),
    });
  } catch {
    return null;
  }
}

export function emitSystemLogSilently(
  level: SystemLogLevel | string,
  options: FrontendSystemLogOptions,
) {
  void emitSystemLog(level, options);
}
