import { computed, ref } from "vue";

export type TaskProgress = {
  id: string;
  label: string;
  detail: string;
  value: number;
  indeterminate: boolean;
  source?: string;
  updatedAt: number;
};

const tasks = ref<TaskProgress[]>([]);

function normalizeValue(value: number) {
  return Math.max(0, Math.min(100, Math.round(value)));
}

export function upsertTask(task: Omit<TaskProgress, "updatedAt">) {
  const nextTask: TaskProgress = {
    ...task,
    value: normalizeValue(task.value),
    updatedAt: Date.now(),
  };
  const index = tasks.value.findIndex((item) => item.id === task.id);
  if (index === -1) {
    tasks.value = [nextTask, ...tasks.value];
    return;
  }
  tasks.value = tasks.value.map((item, currentIndex) => (
    currentIndex === index ? nextTask : item
  ));
}

export function removeTask(id: string) {
  tasks.value = tasks.value.filter((item) => item.id !== id);
}

export function useTaskCenter() {
  return {
    tasks: computed(() => tasks.value),
    upsertTask,
    removeTask,
  };
}
