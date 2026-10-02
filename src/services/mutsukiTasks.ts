/**
 * Mutsuki task handle、事件与取消的前端适配。
 */
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { TaskProgressSnapshot } from "../types/repository";

/** 将旧版进度事件归一化为宿主统一的任务快照。 */
export function toTaskProgressSnapshot(
  taskId: string,
  protocolId: string,
  progress: Record<string, unknown>,
  status: TaskProgressSnapshot["status"] = "running",
): TaskProgressSnapshot {
  const numberValue = (key: string) => {
    const value = progress[key];
    return typeof value === "number" && Number.isFinite(value) ? value : null;
  };
  const stringValue = (key: string) => {
    const value = progress[key];
    return typeof value === "string" ? value : null;
  };
  const percent = numberValue("percent") ?? numberValue("percentage") ?? null;
  return {
    taskId,
    protocolId,
    status,
    phase: stringValue("phase"),
    label: stringValue("label") ?? stringValue("message"),
    current: numberValue("current") ?? numberValue("completed"),
    total: numberValue("total"),
    percent: percent == null ? null : Math.max(0, Math.min(100, percent)),
    error: stringValue("error"),
    updatedAt: new Date().toISOString(),
  };
}

type RuntimeEvent = {
  name: string;
  attributes?: Record<string, unknown>;
};

type MutsukiEvent = {
  type: string;
  events?: MutsukiEvent[];
  task_id?: string;
  event?: RuntimeEvent;
};

type EventEnvelope = {
  payload: MutsukiEvent;
};

type TaskRun = {
  task_id: string;
};

type TaskResult = {
  outcome?: {
    status: string;
    output?: unknown;
    error?: unknown;
    reason?: string | null;
  } | null;
};

function taskId() {
  const suffix = globalThis.crypto?.randomUUID?.()
    ?? `${Date.now()}-${Math.random().toString(16).slice(2)}`;
  return `momobako-task:${suffix}`;
}

function visitEvents(event: MutsukiEvent, visit: (event: MutsukiEvent) => void) {
  if (event.type === "batch" && event.events) {
    event.events.forEach((child) => visitEvents(child, visit));
    return;
  }
  visit(event);
}

function progressPayload<T>(event: RuntimeEvent): T | null {
  if (event.name !== "momobako.task.progress") return null;
  const raw = event.attributes?.payload;
  if (typeof raw !== "string") return null;
  try {
    const envelope = JSON.parse(raw) as { progress?: T };
    return envelope.progress ?? null;
  } catch {
    return null;
  }
}

function abortError() {
  return new DOMException("Mutsuki task aborted.", "AbortError");
}

/**
 * 启动任务后按 handle 过滤 DomainEvent；AbortSignal 会进入 Core cancellation。
 */
export async function runMutsukiTask<TResult, TProgress = never>(
  protocolId: string,
  payload: unknown,
  onProgress?: (progress: TProgress) => void,
  signal?: AbortSignal,
): Promise<TResult> {
  if (signal?.aborted) {
    throw abortError();
  }
  const requestedTaskId = taskId();
  let registeredTaskId: string | null = null;
  let cancelPromise: Promise<void> | null = null;
  let unlisten: (() => void) | null = null;
  const requestCancellation = () => {
    if (!registeredTaskId || cancelPromise) return;
    cancelPromise = invoke("mutsuki_cancel_task", {
      request: { task_id: registeredTaskId, reason: "frontend aborted" },
    }).then(
      () => undefined,
      () => undefined,
    );
  };
  signal?.addEventListener("abort", requestCancellation, { once: true });
  if (signal?.aborted) requestCancellation();
  try {
    unlisten = await listen<EventEnvelope>("mutsuki://event", ({ payload: envelope }) => {
      if (signal?.aborted) return;
      visitEvents(envelope.payload, (event) => {
        if (
          event.type !== "task"
          || event.task_id !== requestedTaskId
          || !event.event
        ) return;
        const progress = progressPayload<TProgress>(event.event);
        if (progress !== null) onProgress?.(progress);
      });
    });
    if (signal?.aborted) throw abortError();

    let run: TaskRun;
    try {
      run = await invoke<TaskRun>("mutsuki_start_task", {
        request: {
          protocol_id: protocolId,
          payload,
          task_id: requestedTaskId,
        },
      });
    } catch (error) {
      if (signal?.aborted) throw abortError();
      throw error;
    }
    registeredTaskId = run.task_id;
    if (signal?.aborted) requestCancellation();
    if (cancelPromise) await cancelPromise;

    const result = await invoke<TaskResult>("mutsuki_task_result", {
      request: { task_id: run.task_id },
    });
    if (signal?.aborted) {
      requestCancellation();
      if (cancelPromise) await cancelPromise;
      throw abortError();
    }
    const outcome = result.outcome;
    if (outcome?.status === "completed") return outcome.output as TResult;
    if (outcome?.status === "failed") {
      throw new Error(`Mutsuki task failed: ${JSON.stringify(outcome.error)}`);
    }
    if (outcome?.status === "cancelled") throw abortError();
    throw new Error(`Mutsuki task ended with ${outcome?.status ?? "no outcome"}: ${outcome?.reason ?? ""}`);
  } finally {
    signal?.removeEventListener("abort", requestCancellation);
    unlisten?.();
  }
}
