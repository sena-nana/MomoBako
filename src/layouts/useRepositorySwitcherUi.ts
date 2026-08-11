import { computed, onBeforeUnmount, onMounted, ref, type ComputedRef } from "vue";
import { open } from "@tauri-apps/plugin-dialog";
import type { RouteLocationNormalizedLoadedGeneric, Router } from "vue-router";
import type { RepositoryBackendOption, RepositorySummary } from "../types/repository";
import { supportsLocalRepositoryRoot } from "../utils/pluginTaxonomy";

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
    repoId?: string,
    options?: {
      skipInitialSync?: boolean;
    },
  ) => Promise<unknown>;
  openRepositoryDeleteDialog: (repoId: string) => void;
  repositories: ComputedRef<RepositorySummary[]>;
  repositoryBackendOptions: ComputedRef<RepositoryBackendOption[]>;
  route: RouteLocationNormalizedLoadedGeneric;
  router: Router;
  selectRepository: (repoId: string) => Promise<unknown>;
};

const localFilesystemPluginId = "momobako.local-filesystem";
const eagleSourcePluginId = "momobako.source.eagle-library";

function formatAddRepositoryBackendLabel(pluginId: string, fallback: string) {
  if (pluginId === localFilesystemPluginId) return "本地文件夹";
  if (pluginId === eagleSourcePluginId) return "Eagle Library";
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
  const addRepositoryPopoverPosition = ref({ left: 0, top: 0, width: 0, anchorX: 0, anchorY: 0 });
  const addRepositoryPopoverRef = ref<HTMLElement | null>(null);
  const repositorySwitcherButtonRef = ref<HTMLElement | null>(null);
  const backendPluginId = ref("");
  const backendName = ref("");
  const backendUrl = ref("");
  const backendUsername = ref("");
  const backendPassword = ref("");
  const backendRoot = ref("");
  const isSubmittingBackend = ref(false);
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

  function preferredBackendPluginId() {
    return options.repositoryBackendOptions.value.find((item) => item.enabled)?.pluginId ?? "";
  }

  function preferredLocalBackendPluginId() {
    return options.repositoryBackendOptions.value.find((item) => item.enabled && supportsLocalRepositoryRoot(item))?.pluginId ?? "";
  }

  function resetBackendForm(pluginId = preferredBackendPluginId()) {
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
    const anchorWidth = Math.min(current.width, width);
    return {
      left,
      top,
      width,
      anchorX: current.left + anchorWidth / 2,
      anchorY: current.bottom,
    };
  }

  function showAddRepositoryMenu() {
    if (!isSubmittingBackend.value) {
      resetBackendForm();
    }
    addRepositoryPopoverMode.value = "addMenu";
  }

  function openAddRepositoryMenu(anchor?: RepositoryPopoverAnchor | null) {
    showAddRepositoryMenu();
    addRepositoryPopoverPosition.value = getPopoverPosition(anchor, "addMenu");
  }

  function openRepositorySwitcherFromEvent(event: MouseEvent) {
    if (isSubmittingBackend.value) return;
    if (addRepositoryPopoverMode.value === "switcher") return;
    addRepositoryError.value = "";
    addRepositoryPopoverMode.value = "switcher";
    addRepositoryPopoverPosition.value = getPopoverPosition(getAnchorFromElement(event.currentTarget), "switcher");
  }

  function showAddRepositoryMenuFromSwitcher() {
    if (isSubmittingBackend.value) return;
    showAddRepositoryMenu();
  }

  function closeAddRepositoryPopover() {
    if (isSubmittingBackend.value) return;
    addRepositoryPopoverMode.value = "closed";
  }

  function selectRepositoryFromList(repoId: string) {
    if (isSubmittingBackend.value) return;
    void options.selectRepository(repoId).then(() => {
      addRepositoryPopoverMode.value = "closed";
      if (options.route.path === "/settings") {
        void options.router.push("/");
      }
    });
  }

  function deleteActiveRepositoryFromMenu() {
    if (!options.activeRepoId.value || isSubmittingBackend.value) return;
    addRepositoryError.value = "";
    addRepositoryPopoverMode.value = "closed";
    options.openRepositoryDeleteDialog(options.activeRepoId.value);
    if (options.route.path === "/settings") {
      void options.router.push("/");
    }
  }

  async function createLocalRepositoryFromPath(path: string, fallbackPosition = addRepositoryPopoverPosition.value) {
    const nextPath = path.trim();
    if (!nextPath) return false;
    backendPluginId.value = preferredLocalBackendPluginId() || backendPluginId.value;
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

  async function chooseEagleLibraryAndCreate() {
    addRepositoryError.value = "";
    const previousPosition = addRepositoryPopoverPosition.value;
    addRepositoryPopoverMode.value = "closed";
    const selected = await open({
      directory: true,
      multiple: false,
      title: "选择 Eagle Library 目录",
    });
    const nextPath = typeof selected === "string" ? selected.trim() : "";
    if (!nextPath) return;
    const segments = nextPath.split(/[\\/]/).filter(Boolean);
    const rawName = segments[segments.length - 1] || "Eagle Library";
    const repoName = rawName.replace(/\.library$/i, "") || "Eagle Library";
    isSubmittingBackend.value = true;
    try {
      await options.createNewRepository(repoName, nextPath, eagleSourcePluginId);
      addRepositoryPopoverMode.value = "closed";
    } catch (cause) {
      addRepositoryError.value = cause instanceof Error ? cause.message : String(cause);
      addRepositoryPopoverPosition.value = previousPosition;
      addRepositoryPopoverMode.value = "addMenu";
    } finally {
      isSubmittingBackend.value = false;
    }
  }

  async function selectBackend(pluginId: string) {
    if (isSubmittingBackend.value) return;
    const backend = options.repositoryBackendOptions.value.find((item) => item.pluginId === pluginId);
    if (!backend?.enabled) return;
    resetBackendForm(pluginId);
    if (pluginId === eagleSourcePluginId) {
      await chooseEagleLibraryAndCreate();
      return;
    }
    if (supportsLocalRepositoryRoot(backend)) {
      await chooseLocalFolderAndCreate();
      return;
    }
    if (backend.capabilities.includes("authentication")) {
      addRepositoryPopoverMode.value = "closed";
      await options.router.push({ path: "/settings", query: { plugin: pluginId } });
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
    if (addRepositoryPopoverMode.value === "closed" || isSubmittingBackend.value) return;
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
