import { onUnmounted, ref, type ComputedRef } from "vue";
import type { EntryActionDialogRequest, EntryActionDialogResultMap } from "../../plugins/sdk";
import type { RepositorySummary } from "../../types/repository";

type EntryActionRepositoryDialogOptions = {
  repositories: ComputedRef<RepositorySummary[]>;
  activeRepoId: ComputedRef<string | null>;
};

export function useEntryActionRepositoryDialog(options: EntryActionRepositoryDialogOptions) {
  const entryActionRepositoryDialogOpen = ref(false);
  const entryActionRepositoryDialogTitle = ref("选择目标资源库");
  const entryActionRepositoryCandidates = ref<RepositorySummary[]>([]);
  let resolvePendingDialog: ((value: RepositorySummary | null) => void) | null = null;

  async function openEntryActionDialog<TKind extends keyof EntryActionDialogResultMap>(
    request: Extract<EntryActionDialogRequest, { kind: TKind }>,
  ): Promise<EntryActionDialogResultMap[TKind]> {
    const dialogRequest = request as EntryActionDialogRequest;
    if (dialogRequest.kind === "directory") {
      const selected = await openDirectoryDialog(dialogRequest.title ?? "选择目录", dialogRequest.defaultPath ?? null);
      return selected as EntryActionDialogResultMap[TKind];
    }

    const candidates = options.repositories.value.filter((repository) => {
      if (dialogRequest.requireReady !== false && repository.status !== "ready") return false;
      if (dialogRequest.requireWritable && !repository.backend.capabilities.includes("write")) return false;
      if (dialogRequest.backendPluginIds?.length && !dialogRequest.backendPluginIds.includes(repository.backend.pluginId)) return false;
      if (dialogRequest.backendKinds?.length && !dialogRequest.backendKinds.includes(repository.backend.kind)) return false;
      if (options.activeRepoId.value && repository.repoId === options.activeRepoId.value) return false;
      return true;
    });

    if (!candidates.length) {
      return null as EntryActionDialogResultMap[TKind];
    }
    if (candidates.length === 1) {
      return candidates[0] as EntryActionDialogResultMap[TKind];
    }

    if (resolvePendingDialog) {
      resolvePendingDialog(null);
      resolvePendingDialog = null;
    }
    entryActionRepositoryDialogTitle.value = dialogRequest.title ?? "选择目标资源库";
    entryActionRepositoryCandidates.value = candidates;
    entryActionRepositoryDialogOpen.value = true;

    return await new Promise<EntryActionDialogResultMap[TKind]>((resolve) => {
      resolvePendingDialog = resolve as (value: RepositorySummary | null) => void;
    });
  }

  function closeEntryActionRepositoryDialog(result: RepositorySummary | null = null) {
    entryActionRepositoryDialogOpen.value = false;
    entryActionRepositoryCandidates.value = [];
    const resolve = resolvePendingDialog;
    resolvePendingDialog = null;
    resolve?.(result);
  }

  onUnmounted(() => {
    closeEntryActionRepositoryDialog();
  });

  return {
    entryActionRepositoryDialogCandidates: entryActionRepositoryCandidates,
    entryActionRepositoryDialogOpen,
    entryActionRepositoryDialogTitle,
    closeEntryActionRepositoryDialog,
    openEntryActionDialog,
  };
}

async function openDirectoryDialog(title: string, defaultPath: string | null) {
  const { open } = await import("@tauri-apps/plugin-dialog");
  const selected = await open({
    title,
    directory: true,
    multiple: false,
    defaultPath: defaultPath ?? undefined,
  });
  return typeof selected === "string" && selected.trim() ? selected : null;
}
