import { computed, ref, watch, type ComputedRef } from "vue";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import type { RepositorySummary } from "../../../types/repository";

type MissingRepositoryAction = "relocating" | null;

type MissingRepositoryActionsOptions = {
  activeRepoId: ComputedRef<string | null>;
  activeRepository: ComputedRef<RepositorySummary | null>;
  configureNeteaseRepositoryCache?: (repoId: string, path: string) => Promise<unknown>;
  isDeletingRepository: ComputedRef<boolean>;
  openRepositoryDeleteDialog: (repoId: string) => void;
  refreshRepositoryWorkspaceSilently: () => Promise<unknown>;
  relocateMissingRepository: (repoId: string, path: string) => Promise<unknown>;
};

export function useMissingRepositoryActions(options: MissingRepositoryActionsOptions) {
  const missingRepositoryError = ref("");
  const missingRepositoryAction = ref<MissingRepositoryAction>(null);

  const isMissingRepositoryBusy = computed(() => (
    missingRepositoryAction.value !== null
    || options.isDeletingRepository.value
  ));
  const isRepairingMissingRepository = computed(() => missingRepositoryAction.value === "relocating");
  const isDeletingMissingRepository = computed(() => options.isDeletingRepository.value);
  const isNeteaseCacheMissing = computed(() => (
    options.activeRepository.value?.backend.pluginId === "momobako.source.netease-cloud-music"
    && options.activeRepository.value.localCache?.status !== "ready"
  ));

  watch(options.activeRepoId, () => {
    missingRepositoryError.value = "";
  });

  async function chooseMissingRepositoryPath() {
    if (!options.activeRepoId.value || isMissingRepositoryBusy.value) return;
    missingRepositoryError.value = "";
    const selected = await openDialog({
      title: isNeteaseCacheMissing.value ? "指定网易云缓存目录" : "重定向资源库位置",
      directory: true,
      multiple: false,
    });
    if (typeof selected !== "string" || !selected.trim()) return;

    missingRepositoryAction.value = "relocating";
    try {
      if (isNeteaseCacheMissing.value) {
        if (!options.configureNeteaseRepositoryCache) {
          throw new Error("缺少网易云缓存目录配置能力");
        }
        await options.configureNeteaseRepositoryCache(options.activeRepoId.value, selected);
      } else {
        await options.relocateMissingRepository(options.activeRepoId.value, selected);
      }
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
      await options.refreshRepositoryWorkspaceSilently();
    } catch (cause) {
      missingRepositoryError.value = cause instanceof Error ? cause.message : String(cause);
    }
  }

  function openMissingRepositoryDeleteDialog() {
    if (!options.activeRepoId.value || isMissingRepositoryBusy.value) return;
    missingRepositoryError.value = "";
    options.openRepositoryDeleteDialog(options.activeRepoId.value);
  }

  return {
    missingRepositoryError,
    isMissingRepositoryBusy,
    isRepairingMissingRepository,
    isDeletingMissingRepository,
    chooseMissingRepositoryPath,
    refreshMissingRepository,
    openMissingRepositoryDeleteDialog,
  };
}
