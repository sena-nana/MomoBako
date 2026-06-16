import { computed, onBeforeUnmount, onMounted, ref, type ComputedRef } from "vue";
import { open } from "@tauri-apps/plugin-dialog";
import type { RouteLocationNormalizedLoadedGeneric, Router } from "vue-router";
import {
  callPlugin,
  configureNeteaseRepositoryCache,
  syncRepository,
  updateRepositoryBackendConfig,
} from "../services/repositoryApi";
import type { RepositoryBackendOption, RepositorySummary } from "../types/repository";

export type RepositoryPopoverMode = "closed" | "switcher" | "addMenu" | "form" | "neteaseLogin";
export type RepositoryPopoverAnchor = {
  left: number;
  bottom: number;
  width: number;
};
type AddRepositoryRequestDetail = {
  anchor?: RepositoryPopoverAnchor;
};
type NeteaseReloginRequestDetail = {
  repoId?: string;
  accountId?: string | number;
  anchor?: RepositoryPopoverAnchor;
};
type NeteaseQrSession = {
  unikey?: string;
  qrurl?: string;
  qrimg?: string | null;
};
type NeteaseLoginResult = {
  code?: number;
  message?: string | null;
  backendConfig?: Record<string, unknown>;
  account?: {
    id?: string | number;
    userName?: string | null;
  } | null;
  profile?: {
    nickname?: string | null;
  } | null;
};

type RepositorySwitcherUiOptions = {
  activeRepoId: ComputedRef<string | null>;
  attachRepository: (path: string) => Promise<unknown>;
  createNewRepository: (
    name: string,
    path: string,
    backendPluginId?: string,
    backendConfig?: Record<string, unknown>,
    repoId?: string,
    options?: {
      skipInitialSync?: boolean;
    },
  ) => Promise<unknown>;
  removeRepository: (repoId: string) => Promise<unknown>;
  repositories: ComputedRef<RepositorySummary[]>;
  repositoryBackendOptions: ComputedRef<RepositoryBackendOption[]>;
  refreshRepositoryWorkspace: () => Promise<unknown>;
  route: RouteLocationNormalizedLoadedGeneric;
  router: Router;
  selectRepository: (repoId: string) => Promise<unknown>;
};

const localFilesystemPluginId = "momobako.local-filesystem";
const neteaseSourcePluginId = "momobako.source.netease-cloud-music";

function formatAddRepositoryBackendLabel(pluginId: string, fallback: string) {
  if (pluginId === localFilesystemPluginId) return "本地文件夹";
  if (pluginId === neteaseSourcePluginId) return "网易云音乐";
  if (pluginId === "momobako.cloud-drive") return "云盘";
  return fallback;
}

function getAnchorFromElement(element: EventTarget | null): RepositoryPopoverAnchor | null {
  if (!(element instanceof HTMLElement)) return null;
  const rect = element.getBoundingClientRect();
  return {
    left: rect.left,
    bottom: rect.bottom,
    width: rect.width,
  };
}

export function useRepositorySwitcherUi(options: RepositorySwitcherUiOptions) {
  const addRepositoryPopoverMode = ref<RepositoryPopoverMode>("closed");
  const addRepositoryPopoverPosition = ref({ left: 0, top: 0, width: 0 });
  const addRepositoryPopoverRef = ref<HTMLElement | null>(null);
  const repositorySwitcherButtonRef = ref<HTMLElement | null>(null);
  const backendPluginId = ref(localFilesystemPluginId);
  const backendName = ref("");
  const backendUrl = ref("");
  const backendUsername = ref("");
  const backendPassword = ref("");
  const backendRoot = ref("");
  const isSubmittingBackend = ref(false);
  const isRemovingRepository = ref(false);
  const isConfirmingRepositoryDelete = ref(false);
  const addRepositoryError = ref("");
  const neteaseQrSession = ref<NeteaseQrSession | null>(null);
  const neteaseLoginMessage = ref("");
  const neteaseLoginTargetRepoId = ref<string | null>(null);
  const neteaseExpectedAccountId = ref<string | null>(null);
  const neteaseCachePath = ref("");

  const backendOptions = computed(() => options.repositoryBackendOptions.value.map((item) => ({
    value: item.pluginId,
    label: formatAddRepositoryBackendLabel(item.pluginId, item.name),
    enabled: item.enabled,
  })));
  const selectedBackend = computed(() => (
    options.repositoryBackendOptions.value.find((item) => item.pluginId === backendPluginId.value)
    ?? options.repositoryBackendOptions.value.find((item) => item.enabled)
    ?? null
  ));
  const backendSubmitDisabled = computed(() => {
    if (!selectedBackend.value?.enabled) {
      return true;
    }
    return !backendUrl.value.trim();
  });

  function resetBackendForm(pluginId = options.repositoryBackendOptions.value.find((item) => item.enabled)?.pluginId ?? localFilesystemPluginId) {
    backendPluginId.value = pluginId;
    backendName.value = "";
    backendUrl.value = "";
    backendUsername.value = "";
    backendPassword.value = "";
    backendRoot.value = "";
    addRepositoryError.value = "";
  }

  function getPopoverWidth(mode = addRepositoryPopoverMode.value) {
    if (mode === "switcher") return 280;
    if (mode === "addMenu") return 160;
    if (mode === "neteaseLogin") return 340;
    return 320;
  }

  function getPopoverPosition(anchor?: RepositoryPopoverAnchor | null, mode = addRepositoryPopoverMode.value) {
    const fallback = {
      left: 16,
      bottom: 44,
      width: getPopoverWidth(mode),
    };
    const current = anchor ?? fallback;
    const width = mode === "switcher" ? current.width : getPopoverWidth(mode);
    const maxLeft = Math.max(8, window.innerWidth - width - 8);
    const left = Math.max(8, Math.min(current.left, maxLeft));
    const top = Math.max(8, Math.min(current.bottom + 6, window.innerHeight - 80));
    return { left, top, width };
  }

  function showAddRepositoryMenu() {
    if (!isSubmittingBackend.value) {
      resetBackendForm();
    }
    isConfirmingRepositoryDelete.value = false;
    addRepositoryPopoverMode.value = "addMenu";
  }

  function openAddRepositoryMenu(anchor?: RepositoryPopoverAnchor | null) {
    showAddRepositoryMenu();
    addRepositoryPopoverPosition.value = getPopoverPosition(anchor, "addMenu");
  }

  function openRepositorySwitcherFromEvent(event: MouseEvent) {
    if (isSubmittingBackend.value || isRemovingRepository.value) return;
    if (addRepositoryPopoverMode.value === "switcher") return;
    addRepositoryError.value = "";
    isConfirmingRepositoryDelete.value = false;
    addRepositoryPopoverMode.value = "switcher";
    addRepositoryPopoverPosition.value = getPopoverPosition(getAnchorFromElement(event.currentTarget), "switcher");
  }

  function showAddRepositoryMenuFromSwitcher() {
    if (isSubmittingBackend.value || isRemovingRepository.value) return;
    showAddRepositoryMenu();
  }

  function closeAddRepositoryPopover() {
    if (isSubmittingBackend.value || isRemovingRepository.value) return;
    addRepositoryPopoverMode.value = "closed";
    isConfirmingRepositoryDelete.value = false;
  }

  function selectRepositoryFromList(repoId: string) {
    if (isSubmittingBackend.value || isRemovingRepository.value) return;
    isConfirmingRepositoryDelete.value = false;
    void options.selectRepository(repoId).then(() => {
      addRepositoryPopoverMode.value = "closed";
      if (options.route.path === "/settings") {
        void options.router.push("/");
      }
    });
  }

  async function deleteActiveRepositoryFromMenu() {
    if (!options.activeRepoId.value || isSubmittingBackend.value || isRemovingRepository.value) return;
    if (!isConfirmingRepositoryDelete.value) {
      isConfirmingRepositoryDelete.value = true;
      return;
    }
    isRemovingRepository.value = true;
    addRepositoryError.value = "";
    try {
      await options.removeRepository(options.activeRepoId.value);
      addRepositoryPopoverMode.value = "closed";
      isConfirmingRepositoryDelete.value = false;
      if (options.route.path === "/settings") {
        void options.router.push("/");
      }
    } catch (cause) {
      addRepositoryError.value = cause instanceof Error ? cause.message : String(cause);
    } finally {
      isRemovingRepository.value = false;
    }
  }

  async function createLocalRepositoryFromPath(path: string, fallbackPosition = addRepositoryPopoverPosition.value) {
    const nextPath = path.trim();
    if (!nextPath) return false;
    backendPluginId.value = localFilesystemPluginId;
    addRepositoryError.value = "";
    isSubmittingBackend.value = true;
    try {
      await options.attachRepository(nextPath);
      addRepositoryPopoverMode.value = "closed";
      return true;
    } catch (cause) {
      addRepositoryError.value = cause instanceof Error ? cause.message : String(cause);
      addRepositoryPopoverPosition.value = fallbackPosition;
      addRepositoryPopoverMode.value = "addMenu";
      console.error("failed to create repository backend", cause);
      return false;
    } finally {
      isSubmittingBackend.value = false;
    }
  }

  function accountIdFromValue(value: unknown) {
    const text = String(value ?? "").trim();
    return text ? text : null;
  }

  function accountIdFromRepoId(repoId?: string | null) {
    const match = String(repoId ?? "").match(/^netease-cloud-music-(.+)$/);
    return match?.[1] ?? null;
  }

  function backendConfigWithSyncTime(backendConfig: Record<string, unknown>, accountId: string) {
    return {
      ...backendConfig,
      accountId,
      lastSyncAt: new Date().toISOString(),
    };
  }

  function neteaseRepositoryName(result: NeteaseLoginResult, accountId: string) {
    return result.profile?.nickname
      || result.account?.userName
      || `网易云音乐 ${accountId}`;
  }

  function syncNeteaseRepositoryInBackground(repoId: string) {
    void (async () => {
      try {
        await syncRepository({ repoId });
        await options.refreshRepositoryWorkspace();
      } catch (cause) {
        const message = cause instanceof Error ? cause.message : String(cause);
        console.error(`failed to sync netease repository ${repoId}`, cause);
        addRepositoryError.value = message;
      }
    })();
  }

  async function createNeteaseQrSession() {
    isSubmittingBackend.value = true;
    addRepositoryError.value = "";
    neteaseLoginMessage.value = "正在创建二维码...";
    try {
      const response = await callPlugin<NeteaseQrSession>({
        pluginId: neteaseSourcePluginId,
        method: "auth.createQrSession",
        payload: { qrimg: true, timestamp: Date.now() },
      });
      neteaseQrSession.value = response.payload ?? null;
      neteaseLoginMessage.value = "请使用网易云音乐扫码登录，完成后点击检查扫码结果。";
    } catch (cause) {
      addRepositoryError.value = cause instanceof Error ? cause.message : String(cause);
      neteaseLoginMessage.value = "";
    } finally {
      isSubmittingBackend.value = false;
    }
  }

  async function openNeteaseLoginFlow(detail?: NeteaseReloginRequestDetail | null) {
    if (isSubmittingBackend.value || isRemovingRepository.value) return;
    resetBackendForm(neteaseSourcePluginId);
    neteaseQrSession.value = null;
    neteaseLoginMessage.value = "";
    neteaseLoginTargetRepoId.value = detail?.repoId?.trim() || null;
    neteaseExpectedAccountId.value = accountIdFromValue(detail?.accountId)
      ?? accountIdFromRepoId(detail?.repoId)
      ?? null;
    neteaseCachePath.value = "";
    addRepositoryPopoverMode.value = "neteaseLogin";
    addRepositoryPopoverPosition.value = getPopoverPosition(detail?.anchor, "neteaseLogin");
    await createNeteaseQrSession();
  }

  async function pollNeteaseQrSession() {
    const key = neteaseQrSession.value?.unikey;
    if (!key || isSubmittingBackend.value) return;
    const cachePath = neteaseCachePath.value.trim();
    if (!cachePath) {
      addRepositoryError.value = "请先选择网易云资源库的本地缓存目录。";
      return;
    }
    isSubmittingBackend.value = true;
    addRepositoryError.value = "";
    neteaseLoginMessage.value = "正在检查扫码结果...";
    try {
      const response = await callPlugin<NeteaseLoginResult>({
        pluginId: neteaseSourcePluginId,
        method: "auth.pollQrSession",
        payload: { key, timestamp: Date.now(), persistSession: false },
      });
      const result = response.payload ?? {};
      if (!result.backendConfig) {
        neteaseLoginMessage.value = result.message || "还没有确认登录，请在手机端确认后再检查。";
        return;
      }
      const accountId = accountIdFromValue(result.backendConfig.accountId ?? result.account?.id);
      if (!accountId) {
        throw new Error("扫码成功但未返回账号 ID");
      }
      if (neteaseExpectedAccountId.value && neteaseExpectedAccountId.value !== accountId) {
        throw new Error("扫码账号与当前资源库不一致。请使用原账号重新登录，或通过“添加资源库”创建新的网易云账号库。");
      }

      const repoId = neteaseLoginTargetRepoId.value || `netease-cloud-music-${accountId}`;
      const backendConfig = backendConfigWithSyncTime({
        ...result.backendConfig,
        sourceUri: `netease-cloud-music://account/${accountId}`,
        localCachePath: cachePath,
      }, accountId);
      const existing = options.repositories.value.find((repo) => repo.repoId === repoId);
      if (existing) {
        await updateRepositoryBackendConfig({ repoId, backendConfig });
        if (existing.localCache?.status !== "ready" || existing.path !== cachePath) {
          await configureNeteaseRepositoryCache({
            repoId,
            path: cachePath,
            migrateLegacyCache: true,
          });
        }
        await options.selectRepository(repoId);
        neteaseLoginMessage.value = `已更新登录状态，正在后台同步歌单：${existing.name}`;
        addRepositoryPopoverMode.value = "closed";
        syncNeteaseRepositoryInBackground(repoId);
        return;
      }

      const name = neteaseRepositoryName(result, accountId);
      await options.createNewRepository(
        name,
        cachePath,
        neteaseSourcePluginId,
        backendConfig,
        repoId,
        { skipInitialSync: true },
      );
      await options.selectRepository(repoId);
      neteaseLoginMessage.value = `已创建资源库，正在后台同步歌单：${name}`;
      addRepositoryPopoverMode.value = "closed";
      syncNeteaseRepositoryInBackground(repoId);
    } catch (cause) {
      addRepositoryError.value = cause instanceof Error ? cause.message : String(cause);
      neteaseLoginMessage.value = "";
    } finally {
      isSubmittingBackend.value = false;
    }
  }

  async function chooseLocalFolderAndCreate() {
    addRepositoryError.value = "";
    const previousPosition = addRepositoryPopoverPosition.value;
    addRepositoryPopoverMode.value = "closed";
    const selected = await open({
      directory: true,
      multiple: false,
      title: "选择资源库文件夹",
    });
    if (typeof selected === "string" && selected.trim()) {
      await createLocalRepositoryFromPath(selected, previousPosition);
    }
  }

  async function selectBackend(pluginId: string) {
    if (isSubmittingBackend.value) return;
    const backend = options.repositoryBackendOptions.value.find((item) => item.pluginId === pluginId);
    if (!backend?.enabled) return;
    resetBackendForm(pluginId);
    if (pluginId === localFilesystemPluginId) {
      await chooseLocalFolderAndCreate();
      return;
    }
    if (pluginId === neteaseSourcePluginId) {
      await openNeteaseLoginFlow();
      return;
    }
    backendPluginId.value = pluginId;
    addRepositoryPopoverMode.value = "form";
  }

  async function chooseNeteaseCacheFolder() {
    if (isSubmittingBackend.value) return;
    addRepositoryError.value = "";
    const selected = await open({
      directory: true,
      multiple: false,
      title: "选择网易云缓存目录",
    });
    if (typeof selected === "string" && selected.trim()) {
      neteaseCachePath.value = selected.trim();
    }
  }

  async function submitAddRepositoryForm() {
    if (backendSubmitDisabled.value) return;
    isSubmittingBackend.value = true;
    addRepositoryError.value = "";
    try {
      const name = backendName.value.trim() || selectedBackend.value?.name || "新资源库";
      const path = backendUrl.value.trim();
      await options.createNewRepository(name, path, backendPluginId.value, {
        baseUrl: path,
        username: backendUsername.value.trim() || undefined,
        password: backendPassword.value.trim() || undefined,
        rootPath: backendRoot.value.trim() || "",
      });
      addRepositoryPopoverMode.value = "closed";
    } catch (cause) {
      addRepositoryError.value = cause instanceof Error ? cause.message : String(cause);
      console.error("failed to create repository backend", cause);
    } finally {
      isSubmittingBackend.value = false;
    }
  }

  function handleAddRepositoryRequest(event: Event) {
    const detail = (event as CustomEvent<AddRepositoryRequestDetail>).detail;
    openAddRepositoryMenu(detail?.anchor);
  }

  function handleNeteaseReloginRequest(event: Event) {
    const detail = (event as CustomEvent<NeteaseReloginRequestDetail>).detail;
    void openNeteaseLoginFlow(detail);
  }

  function handleDocumentKeydown(event: KeyboardEvent) {
    if (event.key === "Escape" && addRepositoryPopoverMode.value !== "closed") {
      closeAddRepositoryPopover();
    }
  }

  function handleDocumentPointerDown(event: PointerEvent) {
    if (addRepositoryPopoverMode.value === "closed" || isSubmittingBackend.value || isRemovingRepository.value) return;
    const target = event.target as Node | null;
    if (target && addRepositoryPopoverRef.value?.contains(target)) return;
    if (target && repositorySwitcherButtonRef.value?.contains(target)) return;
    closeAddRepositoryPopover();
  }

  onMounted(() => {
    window.addEventListener("momo:add-repository", handleAddRepositoryRequest);
    window.addEventListener("momo:netease-relogin", handleNeteaseReloginRequest);
    document.addEventListener("keydown", handleDocumentKeydown);
    document.addEventListener("pointerdown", handleDocumentPointerDown, true);
  });

  onBeforeUnmount(() => {
    window.removeEventListener("momo:add-repository", handleAddRepositoryRequest);
    window.removeEventListener("momo:netease-relogin", handleNeteaseReloginRequest);
    document.removeEventListener("keydown", handleDocumentKeydown);
    document.removeEventListener("pointerdown", handleDocumentPointerDown, true);
  });

  return {
    addRepositoryError,
    addRepositoryPopoverMode,
    addRepositoryPopoverPosition,
    addRepositoryPopoverRef,
    backendName,
    backendOptions,
    backendPassword,
    backendPluginId,
    backendRoot,
    backendSubmitDisabled,
    backendUrl,
    backendUsername,
    closeAddRepositoryPopover,
    createLocalRepositoryFromPath,
    deleteActiveRepositoryFromMenu,
    isConfirmingRepositoryDelete,
    isRemovingRepository,
    isSubmittingBackend,
    neteaseLoginMessage,
    neteaseQrSession,
    neteaseCachePath,
    chooseNeteaseCacheFolder,
    openRepositorySwitcherFromEvent,
    pollNeteaseQrSession,
    createNeteaseQrSession,
    repositorySwitcherButtonRef,
    selectedBackend,
    selectBackend,
    selectRepositoryFromList,
    showAddRepositoryMenuFromSwitcher,
    submitAddRepositoryForm,
  };
}
