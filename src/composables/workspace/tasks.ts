import { ref } from "vue";
import type { RepositorySyncProgress } from "../../types/repository";

export type WorkspaceOperationProgress = {
  label: string;
  detail: string;
  value: number;
  indeterminate: boolean;
};

export const SYNC_TOTAL_STEPS = 3;

export const operationProgress = ref<WorkspaceOperationProgress | null>(null);
export const syncProgress = ref<RepositorySyncProgress>(createInitialSyncProgress());

let operationProgressTimer: number | null = null;
let operationProgressId = 0;

export function createInitialSyncProgress(): RepositorySyncProgress {
  return {
    phase: "idle",
    label: "",
    current: 0,
    total: SYNC_TOTAL_STEPS,
    percent: 0,
  };
}

function stopOperationProgressTimer() {
  if (operationProgressTimer === null) return;
  window.clearInterval(operationProgressTimer);
  operationProgressTimer = null;
}

export function startOperationProgress(
  label: string,
  detail: string,
  options: { initial?: number; ceiling?: number; indeterminate?: boolean } = {},
) {
  const id = ++operationProgressId;
  const ceiling = options.ceiling ?? 88;
  stopOperationProgressTimer();
  operationProgress.value = {
    label,
    detail,
    value: options.initial ?? 8,
    indeterminate: options.indeterminate ?? false,
  };

  operationProgressTimer = window.setInterval(() => {
    if (id !== operationProgressId || !operationProgress.value) return;
    const current = operationProgress.value.value;
    const increment = current < 35 ? 5 : current < 70 ? 3 : 1;
    operationProgress.value = {
      ...operationProgress.value,
      value: Math.min(ceiling, current + increment),
    };
  }, 220);

  return id;
}

export function updateOperationProgress(id: number, patch: Partial<WorkspaceOperationProgress>) {
  if (id !== operationProgressId || !operationProgress.value) return;
  operationProgress.value = {
    ...operationProgress.value,
    ...patch,
    value: patch.value == null ? operationProgress.value.value : Math.max(0, Math.min(100, patch.value)),
  };
}

export function finishOperationProgress(id: number) {
  if (id !== operationProgressId) return;
  stopOperationProgressTimer();
  if (operationProgress.value) {
    operationProgress.value = {
      ...operationProgress.value,
      value: 100,
      indeterminate: false,
    };
  }
  window.setTimeout(() => {
    if (id === operationProgressId) {
      operationProgress.value = null;
    }
  }, 180);
}

export function cancelOperationProgress(id: number) {
  if (id !== operationProgressId) return;
  stopOperationProgressTimer();
  operationProgress.value = null;
}

export function setSyncProgress(
  phase: RepositorySyncProgress["phase"],
  label: string,
  current: number,
  total = SYNC_TOTAL_STEPS,
) {
  syncProgress.value = {
    phase,
    label,
    current,
    total,
    percent: Math.round((current / total) * 100),
  };
}
