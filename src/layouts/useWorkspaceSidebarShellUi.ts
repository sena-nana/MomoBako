import { computed, type ComputedRef } from "vue";
import type { RepositorySummary, RepositorySyncProgress } from "../types/repository";

type WorkspaceSidebarShellUiOptions = {
  activeRepository: ComputedRef<RepositorySummary | null>;
  syncProgress: ComputedRef<RepositorySyncProgress>;
};

export function useWorkspaceSidebarShellUi(options: WorkspaceSidebarShellUiOptions) {
  const isActiveRepositoryMissing = computed(() => options.activeRepository.value?.status === "missing");
  const isShowingSyncProgress = computed(() => (
    options.syncProgress.value.phase === "scanning" ||
    options.syncProgress.value.phase === "writing" ||
    options.syncProgress.value.phase === "refreshing"
  ));

  return {
    isActiveRepositoryMissing,
    isShowingSyncProgress,
  };
}
