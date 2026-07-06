/**
 * 工作区日志状态编排，负责日志首屏加载与实时订阅。
 */
import {
  clearSystemLogs,
  listSystemLogs,
  onSystemLogRecord,
} from "../../services/repositoryApi";
import type { SystemLogPage, SystemLogRecord } from "../../types/repository";
import {
  isClearingLogs,
  isLoadingLogs,
  systemLogs,
} from "./state";

let unlistenSystemLogs: (() => Promise<void> | void) | null = null;
let systemLogSubscriptionPromise: Promise<void> | null = null;

function sortSystemLogs(records: SystemLogRecord[]) {
  return [...records].sort((left, right) => (
    right.timestamp.localeCompare(left.timestamp)
    || right.id.localeCompare(left.id)
  ));
}

function mergeSystemLogs(records: SystemLogRecord[]) {
  const merged = new Map(systemLogs.value.map((record) => [record.id, record]));
  for (const record of records) {
    merged.set(record.id, record);
  }
  systemLogs.value = sortSystemLogs([...merged.values()]).slice(0, 500);
}

export async function ensureSystemLogSubscription() {
  if (unlistenSystemLogs || systemLogSubscriptionPromise) return;
  systemLogSubscriptionPromise = onSystemLogRecord((record) => {
    mergeSystemLogs([record]);
  })
    .then((unlisten) => {
      unlistenSystemLogs = unlisten;
    })
    .finally(() => {
      systemLogSubscriptionPromise = null;
    });
}

/**
 * 加载工作区日志面板首屏数据。
 */
export async function loadSystemLogsInWorkspace(limit = 200): Promise<SystemLogPage> {
  isLoadingLogs.value = true;
  try {
    await ensureSystemLogSubscription();
    const page = await listSystemLogs({ limit });
    systemLogs.value = sortSystemLogs(page.records);
    return page;
  } finally {
    isLoadingLogs.value = false;
  }
}

/**
 * 清空工作区日志缓存与后端持久化文件。
 */
export async function clearSystemLogsInWorkspace() {
  isClearingLogs.value = true;
  try {
    await clearSystemLogs();
    systemLogs.value = [];
  } finally {
    isClearingLogs.value = false;
  }
}

export function resetSystemLogsForTests() {
  systemLogs.value = [];
  isLoadingLogs.value = false;
  isClearingLogs.value = false;
  void unlistenSystemLogs?.();
  unlistenSystemLogs = null;
  systemLogSubscriptionPromise = null;
}
