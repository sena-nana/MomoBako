import { computed, ref, watch, type ComputedRef } from "vue";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

type MissingRepositoryAction = "relocating" | "deleting" | null;

type MissingRepositoryActionsOptions = {
  activeRepoId: ComputedRef<string | null>;
  refreshRepositoryWorkspace: () => Promise<unknown>;
  relocateMissingRepository: (repoId: string, path: string) => Promise<unknown>;
  removeRepository: (repoId: string) => Promise<unknown>;
};

export function useMissingRepositoryActions(options: MissingRepositoryActionsOptions) {
  const missingRepositoryError = ref("");
  const missingRepositoryAction = ref<MissingRepositoryAction>(null);
  const showMissingRepositoryDeleteDialog = ref(false);

  const isMissingRepositoryBusy = computed(() => missingRepositoryAction.value !== null);
  const isRepairingMissingRepository = computed(() => missingRepositoryAction.value === "relocating");
  const isDeletingMissingRepository = computed(() => missingRepositoryAction.value === "deleting");

  watch(options.activeRepoId, () => {
    missingRepositoryError.value = "";
    showMissingRepositoryDeleteDialog.value = false;
  });

  async function chooseMissingRepositoryPath() {
    if (!options.activeRepoId.value || isMissingRepositoryBusy.value) return;
    missingRepositoryError.value = "";
    const selected = await openDialog({
      title: "重定向资源库位置",
      directory: true,
      multiple: false,
    });
    if (typeof selected !== "string" || !selected.trim()) return;

    missingRepositoryAction.value = "relocating";
    try {
      await options.relocateMissingRepository(options.activeRepoId.value, selected);
      missingRepositoryError.value = "";
    } catch (cause) {
      missingRepositoryError.value = cause instanceof Error ? cause.message : String(cause);
    } finally {
      missingRepositoryAction.value = null;
    }
  }

  async function refreshMissingRepository() {
    if (isMissingRepositoryBusy.value) return;
    missingRepositoryError.value = "";
    try {
      await options.refreshRepositoryWorkspace();
    } catch (cause) {
      missingRepositoryError.value = cause instanceof Error ? cause.message : String(cause);
    }
  }

  function openMissingRepositoryDeleteDialog() {
    if (!options.activeRepoId.value || isMissingRepositoryBusy.value) return;
    missingRepositoryError.value = "";
    showMissingRepositoryDeleteDialog.value = true;
  }

  function closeMissingRepositoryDeleteDialog() {
    if (isDeletingMissingRepository.value) return;
    showMissingRepositoryDeleteDialog.value = false;
  }

  async function confirmMissingRepositoryDelete() {
    if (!options.activeRepoId.value) return;
    missingRepositoryAction.value = "deleting";
    missingRepositoryError.value = "";
    try {
      await options.removeRepository(options.activeRepoId.value);
      showMissingRepositoryDeleteDialog.value = false;
    } catch (cause) {
      missingRepositoryError.value = cause instanceof Error ? cause.message : String(cause);
    } finally {
      missingRepositoryAction.value = null;
    }
  }

  return {
    missingRepositoryError,
    isMissingRepositoryBusy,
    isRepairingMissingRepository,
    isDeletingMissingRepository,
    showMissingRepositoryDeleteDialog,
    chooseMissingRepositoryPath,
    refreshMissingRepository,
    openMissingRepositoryDeleteDialog,
    closeMissingRepositoryDeleteDialog,
    confirmMissingRepositoryDelete,
  };
}
