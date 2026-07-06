/**
 * 系统日志相关的 Tauri 仓库 API 封装。
 */
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  SystemLogPage,
  SystemLogQuery,
  SystemLogRecord,
  SystemLogWriteRequest,
} from "../../types/repository";
import { invokeCommand } from "./core";

const SYSTEM_LOG_EVENT = "system://log-recorded";

/**
 * 读取系统日志分页数据。
 */
export function listSystemLogs(query?: SystemLogQuery) {
  return invokeCommand<SystemLogPage>("list_system_logs", { query });
}

/**
 * 写入一条系统日志。
 */
export function writeSystemLog(request: SystemLogWriteRequest) {
  return invokeCommand<SystemLogRecord>("write_system_log", request);
}

/**
 * 清空系统日志。
 */
export function clearSystemLogs() {
  return invokeCommand<void>("clear_system_logs");
}

/**
 * 订阅实时日志事件。
 */
export function onSystemLogRecord(
  listener: (record: SystemLogRecord) => void,
): Promise<UnlistenFn> {
  return listen<SystemLogRecord>(SYSTEM_LOG_EVENT, ({ payload }) => {
    listener(payload);
  });
}
