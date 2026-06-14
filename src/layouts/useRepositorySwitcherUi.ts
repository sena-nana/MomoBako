import { computed, onBeforeUnmount, onMounted, ref, type ComputedRef } from "vue";
import { open } from "@tauri-apps/plugin-dialog";
import type { RouteLocationNormalizedLoadedGeneric, Router } from "vue-router";
import type { RepositoryBackendOption } from "../types/repository";

export type RepositoryPopoverMode = "closed" | "switcher" | "addMenu" | "form";
export type RepositoryPopoverAnchor = {
  left: number;
  bottom: number;
  width: number;
};
type AddRepositoryRequestDetail = {
  anchor?: RepositoryPopoverAnchor;
};

type RepositorySwitcherUiOptions = {
  activeRepoId: ComputedRef<string | null>;
  attachRepository: (path: string) => Promise<unknown>;
  createNewRepository: (
    name: string,
    path: string,
    backendPluginId?: string,
    backendConfig?: Record<string, unknown>,
  ) => Promise<unknown>;
  removeRepository: (repoId: string) => Promise<unknown>;
  repositoryBackendOptions: ComputedRef<RepositoryBackendOption[]>;
  route: RouteLocationNormalizedLoadedGeneric;
  router: Router;
  selectRepository: (repoId: string) => Promise<unknown>;
};

const localFilesystemPluginId = "momobako.local-filesystem";

function formatAddRepositoryBackendLabel(pluginId: string, fallback: string) {
  if (pluginId === localFilesystemPluginId) return "本地文件夹";
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
    backendPluginId.value = pluginId;
    addRepositoryPopoverMode.value = "form";
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
    document.addEventListener("keydown", handleDocumentKeydown);
    document.addEventListener("pointerdown", handleDocumentPointerDown, true);
  });

  onBeforeUnmount(() => {
    window.removeEventListener("momo:add-repository", handleAddRepositoryRequest);
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
    openRepositorySwitcherFromEvent,
    repositorySwitcherButtonRef,
    selectedBackend,
    selectBackend,
    selectRepositoryFromList,
    showAddRepositoryMenuFromSwitcher,
    submitAddRepositoryForm,
  };
}
