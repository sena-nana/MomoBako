import { computed, ref, watch, type ComputedRef } from "vue";
import { callPlugin } from "../../../services/repositoryApi";
import type { FileBrowserSnapshot, RepositorySnapshot } from "../../../types/repository";

const NETEASE_SOURCE_PLUGIN_ID = "momobako.source.netease-cloud-music";

type WorkspaceNeteaseAuthOptions = {
  activeRepoId: ComputedRef<string | null>;
  activeSnapshot: ComputedRef<RepositorySnapshot | null>;
  fileBrowser: ComputedRef<FileBrowserSnapshot | null>;
};

export function useWorkspaceNeteaseAuthViewModel(options: WorkspaceNeteaseAuthOptions) {
  const neteaseLoginStatus = ref<{ loggedIn?: boolean; loginExpired?: boolean; error?: string | null } | null>(null);
  const isRefreshingNeteaseLogin = ref(false);

  const isActiveNeteaseRepository = computed(() => (
    options.activeSnapshot.value?.repository.backend.pluginId === NETEASE_SOURCE_PLUGIN_ID
  ));

  const activeNeteaseSourceConfig = computed(() => {
    const payload = [
      ...(options.fileBrowser.value?.entries ?? []),
      ...(options.activeSnapshot.value?.assets ?? []),
    ].map((entry) => entry.sourcePayload).find((item) => (
      item?.provider === "netease-cloud-music" && typeof item.accountCookie === "string"
    ));
    if (!payload) return null;
    return {
      cookie: payload.accountCookie,
      accountId: payload.accountId,
    };
  });

  const activeNeteaseLoginExpired = computed(() => {
    if (!isActiveNeteaseRepository.value) return false;
    if (neteaseLoginStatus.value?.loginExpired) return true;
    return (options.fileBrowser.value?.entries ?? []).some((entry) => (
      entry.sourcePayload?.loginExpired === true
      || entry.metadata?.loginExpired === true
    )) || (options.activeSnapshot.value?.assets ?? []).some((entry) => entry.sourcePayload?.loginExpired === true);
  });

  async function refreshActiveNeteaseLoginStatus() {
    if (!isActiveNeteaseRepository.value) {
      neteaseLoginStatus.value = null;
      return;
    }
    const config = activeNeteaseSourceConfig.value;
    if (!config) {
      neteaseLoginStatus.value = null;
      return;
    }
    isRefreshingNeteaseLogin.value = true;
    try {
      const response = await callPlugin<{
        loggedIn?: boolean;
        loginExpired?: boolean;
        error?: string;
      }>({
        pluginId: NETEASE_SOURCE_PLUGIN_ID,
        method: "auth.getLoginStatus",
        payload: { config },
      });
      neteaseLoginStatus.value = response.payload ?? null;
    } catch (cause) {
      neteaseLoginStatus.value = {
        loggedIn: false,
        loginExpired: true,
        error: cause instanceof Error ? cause.message : String(cause),
      };
    } finally {
      isRefreshingNeteaseLogin.value = false;
    }
  }

  function requestActiveNeteaseRelogin() {
    if (!options.activeRepoId.value) return;
    window.dispatchEvent(new CustomEvent("momo:netease-relogin", {
      detail: {
        repoId: options.activeRepoId.value,
        accountId: activeNeteaseSourceConfig.value?.accountId,
      },
    }));
  }

  watch(
    () => [options.activeRepoId.value, options.activeSnapshot.value?.repository.backend.pluginId] as const,
    () => {
      void refreshActiveNeteaseLoginStatus();
    },
    { immediate: true },
  );

  return {
    NETEASE_SOURCE_PLUGIN_ID,
    activeNeteaseLoginExpired,
    activeNeteaseSourceConfig,
    isActiveNeteaseRepository,
    isRefreshingNeteaseLogin,
    neteaseLoginStatus,
    refreshActiveNeteaseLoginStatus,
    requestActiveNeteaseRelogin,
  };
}
