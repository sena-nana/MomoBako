<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch, type Component } from "vue";
import { open } from "@tauri-apps/plugin-dialog";
import { RouterLink, useRoute, useRouter } from "vue-router";
import {
  Archive,
  Bookmark,
  Check,
  ChevronsUpDown,
  File,
  FolderTree,
  LoaderCircle,
  Plus,
  Puzzle,
  RefreshCw,
  Settings,
  Tag,
  Trash2,
  X,
} from "lucide-vue-next";
import FolderTreeNode from "../components/FolderTreeNode.vue";
import SmartFolderTreeNode from "../components/SmartFolderTreeNode.vue";
import TaskPopover from "../components/TaskPopover.vue";
import { normalizeWorkspaceMovePaths } from "../pages/workspace/dragBehavior";
import { useRepositoryWorkspace, type WorkspacePanelKey } from "../composables/useRepositoryWorkspace";
import type { FileDeleteMode, RepositoryShortcut, SmartFolder, SmartFolderFilter, SmartFolderTreeNode as SmartFolderTreeNodeType } from "../types/repository";

type PanelKey = Exclude<WorkspacePanelKey, "files" | "search" | "smartFolder">;
type ShortcutKey = "all" | "processing" | "untagged" | "deleted";
type ShortcutItem = {
  id: ShortcutKey;
  label: string;
  count: number;
  icon: Component;
};

type RepositoryPopoverMode = "closed" | "switcher" | "addMenu" | "form";
type RepositoryPopoverAnchor = {
  left: number;
  bottom: number;
  width: number;
};
type AddRepositoryRequestDetail = {
  anchor?: RepositoryPopoverAnchor;
};

const addRepositoryPopoverMode = ref<RepositoryPopoverMode>("closed");
const addRepositoryPopoverPosition = ref({ left: 0, top: 0, width: 0 });
const addRepositoryPopoverRef = ref<HTMLElement | null>(null);
const localFilesystemPluginId = "momobako.local-filesystem";
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
const expandedFolderPaths = ref<string[]>([]);
const expandedSmartFolderIds = ref<string[]>([]);
const showFolderDialog = ref(false);
const folderDialogMode = ref<"create" | "rename">("create");
const folderDialogParentPath = ref("");
const folderDialogTargetPath = ref("");
const folderDialogLabel = ref("");
const folderDialogValue = ref("");
const showFolderDeleteDialog = ref(false);
const pendingDeleteFolderPath = ref("");
const pendingDeleteFolderLabel = ref("");
const showSmartFolderDialog = ref(false);
const smartFolderDialogMode = ref<"create" | "edit">("create");
const smartFolderTargetId = ref("");
const smartFolderParentId = ref("");
const smartFolderName = ref("");
const smartFolderQuery = ref("");
const smartFolderPathPrefix = ref("");
const smartFolderFormats = ref("");
const smartFolderTags = ref("");
const smartFolderColors = ref("");
const smartFolderShapes = ref("");
const smartFolderMinRating = ref("");
const smartFolderMetadataFilters = ref("");
const smartFolderMatchMode = ref<"and" | "or">("and");
const smartFolderExcludeTags = ref("");
const smartFolderExcludeFormats = ref("");
const smartFolderExcludeMetadataFilters = ref("");
const smartFolderNumberFilters = ref("");
const smartFolderDateFilters = ref("");
const smartFolderSortField = ref("");
const smartFolderSortDirection = ref<"asc" | "desc">("asc");
const smartFolderLimit = ref("");
const showSmartFolderDeleteDialog = ref(false);
const pendingDeleteSmartFolderId = ref("");
const pendingDeleteSmartFolderLabel = ref("");
const route = useRoute();
const router = useRouter();

const {
  repositories,
  activeRepository,
  repositoryBackendOptions,
  activePanel,
  activeRepoId,
  activeSmartFolderId,
  activeSnapshot,
  currentDirectoryPath,
  dragHoverFolderPath,
  draggedWorkspacePaths,
  fileTree,
  smartFolders,
  syncProgress,
  isExternalDragActive,
  isInternalDragActive,
  isBusy,
  isLoadingFileBrowser,
  isMutatingFiles,
  isMutatingSmartFolder,
  error,
  refreshFileBrowserTree,
  selectRepository,
  selectSmartFolder,
  selectWorkspaceEntry,
  setActivePanel,
  loadFileBrowserForDirectory,
  createSmartFolderInWorkspace,
  updateSmartFolderInWorkspace,
  deleteSmartFolderInWorkspace,
  createDirectoryInWorkspace,
  importEntriesToWorkspace,
  moveWorkspaceEntries,
  renameWorkspaceEntry,
  deleteWorkspaceEntry,
  createNewRepository,
  attachRepository,
  removeRepository,
  clearDraggedWorkspaceState,
  setDragHoverFolderPath,
} = useRepositoryWorkspace();

const shortcuts = computed<ShortcutItem[]>(() => {
  const assets = activeSnapshot.value?.assets ?? [];
  return [
    { id: "all", label: "全部", count: assets.length, icon: Archive },
    { id: "processing", label: "处理中", count: assets.filter((item) => item.status === "processing").length, icon: FolderTree },
    { id: "untagged", label: "未标签", count: assets.filter((item) => item.tags.length === 0).length, icon: Tag },
    { id: "deleted", label: "已删除", count: 0, icon: Trash2 },
  ];
});

const isTrashPanel = computed(() => activePanel.value === "deleted");
const quickAccess = computed(() => activeSnapshot.value?.quickAccess ?? []);
const isActiveRepositoryMissing = computed(() => activeRepository.value?.status === "missing");
const isFolderDragActive = computed(() => isExternalDragActive.value || isInternalDragActive.value);
const expandedFolderPathSet = computed(() => new Set(expandedFolderPaths.value));
const fileTreeNodes = computed(() => fileTree.value);
const backendOptions = computed(() => repositoryBackendOptions.value.map((item) => ({
  value: item.pluginId,
  label: formatAddRepositoryBackendLabel(item.pluginId, item.name),
  enabled: item.enabled,
})));
const selectedBackend = computed(() => (
  repositoryBackendOptions.value.find((item) => item.pluginId === backendPluginId.value)
  ?? repositoryBackendOptions.value[0]
  ?? null
));
const backendSubmitDisabled = computed(() => {
  if (!selectedBackend.value?.enabled) {
    return true;
  }
  return !backendUrl.value.trim();
});
const folderDialogTitle = computed(() => (
  folderDialogMode.value === "create" ? "新建文件夹" : "重命名文件夹"
));
const folderDialogActionLabel = computed(() => (
  folderDialogMode.value === "create" ? "创建" : "保存"
));
const folderDialogPlaceholder = computed(() => (
  folderDialogMode.value === "create" ? "输入文件夹名称" : "输入新的文件夹名称"
));
const folderDialogDisabled = computed(() => !folderDialogValue.value.trim() || isMutatingFiles.value);
const flatSmartFolders = computed(() => flattenSmartFolders(smartFolders.value));
const smartFolderById = computed(() => new Map(flatSmartFolders.value.map((item) => [item.smartFolderId, item])));
const expandedSmartFolderIdSet = computed(() => new Set(expandedSmartFolderIds.value));
const smartFolderDialogTitle = computed(() => (
  smartFolderDialogMode.value === "create" ? "新建智能文件夹" : "编辑智能文件夹"
));
const smartFolderDialogActionLabel = computed(() => (
  smartFolderDialogMode.value === "create" ? "创建" : "保存"
));
const smartFolderDialogDisabled = computed(() => !smartFolderName.value.trim() || isMutatingSmartFolder.value);
const isShowingSyncProgress = computed(() => (
  syncProgress.value.phase === "scanning" ||
  syncProgress.value.phase === "writing" ||
  syncProgress.value.phase === "refreshing"
));

let folderHoverSwitchTimer: number | null = null;
let pendingHoverFolderPath: string | null = null;

watch(
  fileTreeNodes,
  (nodes) => {
    const validPaths = new Set<string>([""]);
    const collectPaths = (items: typeof nodes) => {
      for (const item of items) {
        validPaths.add(item.path);
        collectPaths(item.children);
      }
    };
    collectPaths(nodes);
    expandedFolderPaths.value = expandedFolderPaths.value.filter((path) => validPaths.has(path));
  },
  { deep: true },
);

watch(
  currentDirectoryPath,
  (path) => {
    if (path == null) return;
    const segments = path ? path.split("/") : [];
    const nextExpanded = new Set(expandedFolderPaths.value);
    let cursor = "";
    for (const segment of segments) {
      cursor = cursor ? `${cursor}/${segment}` : segment;
      nextExpanded.add(cursor);
    }
    expandedFolderPaths.value = Array.from(nextExpanded);
  },
  { immediate: true },
);

watch(
  flatSmartFolders,
  (folders) => {
    const validIds = new Set(folders.map((item) => item.smartFolderId));
    expandedSmartFolderIds.value = expandedSmartFolderIds.value.filter((id) => validIds.has(id));
  },
);

watch(
  activeSmartFolderId,
  (id) => {
    if (!id) return;
    const path = smartFolderAncestry(id);
    if (!path.length) return;
    const next = new Set(expandedSmartFolderIds.value);
    for (const item of path.slice(0, -1)) {
      next.add(item.smartFolderId);
    }
    expandedSmartFolderIds.value = Array.from(next);
  },
  { immediate: true },
);

function splitListInput(value: string) {
  return Array.from(new Set(
    value
      .split(/[,，\n]/)
      .map((item) => item.trim())
      .filter(Boolean),
  ));
}

function joinListInput(values?: string[]) {
  return values?.join("，") ?? "";
}

function parseMetadataFiltersInput(value: string) {
  return value
    .split(/\n|[,，]/)
    .map((item) => item.trim())
    .filter(Boolean)
    .flatMap((item) => {
      const index = item.indexOf("=");
      if (index < 0) return [];
      const key = item.slice(0, index).trim();
      const filterValue = item.slice(index + 1).trim();
      return key && filterValue ? [{ key, value: filterValue }] : [];
    });
}

function formatMetadataFiltersInput(filter: SmartFolderFilter) {
  return filter.metadataFilters?.map((item) => `${item.key}=${item.value}`).join("\n") ?? "";
}

function formatExcludeMetadataFiltersInput(filter: SmartFolderFilter) {
  return filter.excludeMetadataFilters?.map((item) => `${item.key}=${item.value}`).join("\n") ?? "";
}

function parseNumberFiltersInput(value: string) {
  const parseRangeBound = (text: string) => {
    const trimmed = text.trim();
    if (!trimmed) return undefined;
    const number = Number(trimmed);
    return Number.isFinite(number) ? number : undefined;
  };

  return value
    .split(/\n|[,，]/)
    .map((item) => item.trim())
    .filter(Boolean)
    .flatMap((item) => {
      const [key, range] = item.split("=");
      if (!key?.trim() || !range?.trim()) return [];
      const [minText, maxText] = range.split("..").map((part) => part.trim());
      const min = parseRangeBound(minText ?? "");
      const max = parseRangeBound(maxText ?? "");
      return [{
        key: key.trim(),
        min,
        max,
      }].filter((filter) => filter.min != null || filter.max != null);
    });
}

function formatNumberFiltersInput(filter: SmartFolderFilter) {
  return filter.numberFilters?.map((item) => `${item.key}=${item.min ?? ""}..${item.max ?? ""}`).join("\n") ?? "";
}

function parseDateFiltersInput(value: string) {
  return value
    .split(/\n|[,，]/)
    .map((item) => item.trim())
    .filter(Boolean)
    .flatMap((item) => {
      const [key, range] = item.split("=");
      if (!key?.trim() || !range?.trim()) return [];
      const [from, to] = range.split("..").map((part) => part.trim());
      return [{
        key: key.trim(),
        from: from || undefined,
        to: to || undefined,
      }].filter((filter) => filter.from || filter.to);
    });
}

function formatDateFiltersInput(filter: SmartFolderFilter) {
  return filter.dateFilters?.map((item) => `${item.key}=${item.from ?? ""}..${item.to ?? ""}`).join("\n") ?? "";
}

function buildSmartFolderFilter(): SmartFolderFilter {
  const minRating = Number(smartFolderMinRating.value);
  const limit = Number(smartFolderLimit.value);
  return {
    query: smartFolderQuery.value.trim() || undefined,
    pathPrefix: smartFolderPathPrefix.value.trim() || undefined,
    formats: splitListInput(smartFolderFormats.value),
    tags: splitListInput(smartFolderTags.value),
    colors: splitListInput(smartFolderColors.value),
    shapes: splitListInput(smartFolderShapes.value),
    minRating: Number.isFinite(minRating) && minRating > 0 ? minRating : undefined,
    metadataFilters: parseMetadataFiltersInput(smartFolderMetadataFilters.value),
    matchMode: smartFolderMatchMode.value,
    excludeTags: splitListInput(smartFolderExcludeTags.value),
    excludeFormats: splitListInput(smartFolderExcludeFormats.value),
    excludeMetadataFilters: parseMetadataFiltersInput(smartFolderExcludeMetadataFilters.value),
    numberFilters: parseNumberFiltersInput(smartFolderNumberFilters.value),
    dateFilters: parseDateFiltersInput(smartFolderDateFilters.value),
    sort: smartFolderSortField.value.trim()
      ? { field: smartFolderSortField.value.trim(), direction: smartFolderSortDirection.value }
      : undefined,
    limit: Number.isFinite(limit) && limit > 0 ? limit : undefined,
  };
}

function flattenSmartFolders(nodes: SmartFolderTreeNodeType[]): SmartFolder[] {
  return nodes.flatMap((node) => [
    node,
    ...flattenSmartFolders(node.children),
  ]);
}

function findSmartFolder(id: string) {
  return smartFolderById.value.get(id) ?? null;
}

function smartFolderAncestry(id: string) {
  const path: SmartFolder[] = [];
  let current = findSmartFolder(id);
  while (current) {
    path.unshift(current);
    current = current.parentId
      ? findSmartFolder(current.parentId)
      : null;
  }
  return path;
}

watch(isFolderDragActive, (active) => {
  if (!active) {
    clearFolderDragHover();
  }
});

function selectPanel(next: PanelKey) {
  setActivePanel(next);
  if (route.path === "/settings") {
    void router.push("/");
  }
}

function selectShortcut(id: ShortcutKey) {
  if (isActiveRepositoryMissing.value) return;
  if (id === "deleted") {
    selectPanel("deleted");
    return;
  }
  setActivePanel("files");
  if (route.path === "/settings") {
    void router.push("/");
  }
}

function shortcutIcon(shortcut: RepositoryShortcut) {
  if (shortcut.targetKind === "smartFolder") return Bookmark;
  if (shortcut.targetKind === "file") return File;
  return FolderTree;
}

function parentPathFor(path: string) {
  const normalized = path.replace(/\\/g, "/").replace(/^\/+|\/+$/g, "");
  const index = normalized.lastIndexOf("/");
  return index > 0 ? normalized.slice(0, index) : "";
}

async function openQuickAccess(shortcut: RepositoryShortcut) {
  if (isActiveRepositoryMissing.value) return;
  if (route.path === "/settings") {
    void router.push("/");
  }
  if (shortcut.targetKind === "smartFolder" && shortcut.targetId) {
    await selectSmartFolder(shortcut.targetId);
    return;
  }
  if (shortcut.targetKind === "file" && shortcut.targetPath) {
    setActivePanel("files");
    await loadFileBrowserForDirectory(parentPathFor(shortcut.targetPath));
    selectWorkspaceEntry(shortcut.targetPath);
    return;
  }
  if (shortcut.targetPath != null) {
    openFolder(shortcut.targetPath);
  }
}

function selectRepositoryFromList(repoId: string) {
  if (isSubmittingBackend.value || isRemovingRepository.value) return;
  isConfirmingRepositoryDelete.value = false;
  void selectRepository(repoId).then(() => {
    addRepositoryPopoverMode.value = "closed";
    if (route.path === "/settings") {
      void router.push("/");
    }
  });
}

function formatAddRepositoryBackendLabel(pluginId: string, fallback: string) {
  if (pluginId === localFilesystemPluginId || pluginId === "builtin.local-filesystem") return "本地文件夹";
  if (pluginId === "momobako.cloud-drive" || pluginId === "builtin.cloud-drive") return "云盘";
  return fallback;
}

function resetBackendForm(pluginId = repositoryBackendOptions.value[0]?.pluginId ?? localFilesystemPluginId) {
  backendPluginId.value = pluginId;
  backendName.value = "";
  backendUrl.value = "";
  backendUsername.value = "";
  backendPassword.value = "";
  backendRoot.value = "";
  addRepositoryError.value = "";
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

async function selectBackend(pluginId: string) {
  if (isSubmittingBackend.value) return;
  const backend = repositoryBackendOptions.value.find((item) => item.pluginId === pluginId);
  if (!backend?.enabled) return;
  resetBackendForm(pluginId);
  if (pluginId === localFilesystemPluginId) {
    await chooseLocalFolderAndCreate();
    return;
  }
  backendPluginId.value = pluginId;
  addRepositoryPopoverMode.value = "form";
}

async function deleteActiveRepositoryFromMenu() {
  if (!activeRepoId.value || isSubmittingBackend.value || isRemovingRepository.value) return;
  if (!isConfirmingRepositoryDelete.value) {
    isConfirmingRepositoryDelete.value = true;
    return;
  }
  isRemovingRepository.value = true;
  addRepositoryError.value = "";
  try {
    await removeRepository(activeRepoId.value);
    addRepositoryPopoverMode.value = "closed";
    isConfirmingRepositoryDelete.value = false;
    if (route.path === "/settings") {
      void router.push("/");
    }
  } catch (cause) {
    addRepositoryError.value = cause instanceof Error ? cause.message : String(cause);
  } finally {
    isRemovingRepository.value = false;
  }
}

function openFolder(path: string) {
  setActivePanel("files");
  void loadFileBrowserForDirectory(path);
}

function ensureFolderExpanded(path: string) {
  if (!path) return;
  const next = new Set(expandedFolderPaths.value);
  next.add(path);
  expandedFolderPaths.value = Array.from(next);
}

function clearFolderHoverTimer() {
  if (folderHoverSwitchTimer != null) {
    window.clearTimeout(folderHoverSwitchTimer);
    folderHoverSwitchTimer = null;
  }
  pendingHoverFolderPath = null;
}

function clearFolderDragHover() {
  clearFolderHoverTimer();
  setDragHoverFolderPath(null);
}

function handleFolderDragHover(path: string) {
  if (!activeRepoId.value || isTrashPanel.value || !isFolderDragActive.value) return;
  setDragHoverFolderPath(path);
  if (pendingHoverFolderPath === path) return;

  clearFolderHoverTimer();
  pendingHoverFolderPath = path;
  folderHoverSwitchTimer = window.setTimeout(() => {
    ensureFolderExpanded(path);
    openFolder(path);
    folderHoverSwitchTimer = null;
    pendingHoverFolderPath = null;
  }, 450);
}

function handleFolderDragLeave(path: string) {
  if (dragHoverFolderPath.value === path) {
    setDragHoverFolderPath(null);
  }
  if (pendingHoverFolderPath === path) {
    clearFolderHoverTimer();
  }
}

function getDroppedSourcePaths(event: DragEvent) {
  return Array.from(event.dataTransfer?.files ?? [])
    .map((file) => (file as File & { path?: string }).path ?? "")
    .filter((path) => path.trim().length > 0);
}

async function handleFolderDrop(path: string, event: DragEvent) {
  clearFolderDragHover();

  if (!activeRepoId.value || isTrashPanel.value) {
    clearDraggedWorkspaceState();
    return;
  }

  if (isInternalDragActive.value && draggedWorkspacePaths.value.length) {
    const sourcePaths = normalizeWorkspaceMovePaths(draggedWorkspacePaths.value, path);
    if (!sourcePaths.length) {
      clearDraggedWorkspaceState();
      return;
    }
    await moveWorkspaceEntries(sourcePaths, path);
    clearDraggedWorkspaceState();
    return;
  }

  const sourcePaths = getDroppedSourcePaths(event);
  if (sourcePaths.length) {
    await importEntriesToWorkspace(sourcePaths, path);
  }
}

function toggleFolderExpansion(path: string) {
  const next = new Set(expandedFolderPaths.value);
  if (next.has(path)) {
    next.delete(path);
  } else {
    next.add(path);
  }
  expandedFolderPaths.value = Array.from(next);
}

function toggleSmartFolderExpansion(smartFolderId: string) {
  const next = new Set(expandedSmartFolderIds.value);
  if (next.has(smartFolderId)) {
    next.delete(smartFolderId);
  } else {
    next.add(smartFolderId);
  }
  expandedSmartFolderIds.value = Array.from(next);
}

function openSmartFolder(smartFolderId: string) {
  if (route.path === "/settings") {
    void router.push("/");
  }
  void selectSmartFolder(smartFolderId);
}

function resetSmartFolderDialog(parentId = "") {
  smartFolderTargetId.value = "";
  smartFolderParentId.value = parentId;
  smartFolderName.value = "";
  smartFolderQuery.value = "";
  smartFolderPathPrefix.value = "";
  smartFolderFormats.value = "";
  smartFolderTags.value = "";
  smartFolderColors.value = "";
  smartFolderShapes.value = "";
  smartFolderMinRating.value = "";
  smartFolderMetadataFilters.value = "";
  smartFolderMatchMode.value = "and";
  smartFolderExcludeTags.value = "";
  smartFolderExcludeFormats.value = "";
  smartFolderExcludeMetadataFilters.value = "";
  smartFolderNumberFilters.value = "";
  smartFolderDateFilters.value = "";
  smartFolderSortField.value = "";
  smartFolderSortDirection.value = "asc";
  smartFolderLimit.value = "";
}

function openCreateSmartFolderDialog(parentId = "") {
  smartFolderDialogMode.value = "create";
  resetSmartFolderDialog(parentId);
  showSmartFolderDialog.value = true;
}

function openEditSmartFolderDialog(smartFolderId: string) {
  const folder = findSmartFolder(smartFolderId);
  if (!folder) return;
  smartFolderDialogMode.value = "edit";
  smartFolderTargetId.value = folder.smartFolderId;
  smartFolderParentId.value = folder.parentId ?? "";
  smartFolderName.value = folder.name;
  smartFolderQuery.value = folder.filter.query ?? "";
  smartFolderPathPrefix.value = folder.filter.pathPrefix ?? "";
  smartFolderFormats.value = joinListInput(folder.filter.formats);
  smartFolderTags.value = joinListInput(folder.filter.tags);
  smartFolderColors.value = joinListInput(folder.filter.colors);
  smartFolderShapes.value = joinListInput(folder.filter.shapes);
  smartFolderMinRating.value = folder.filter.minRating == null ? "" : String(folder.filter.minRating);
  smartFolderMetadataFilters.value = formatMetadataFiltersInput(folder.filter);
  smartFolderMatchMode.value = folder.filter.matchMode === "or" ? "or" : "and";
  smartFolderExcludeTags.value = joinListInput(folder.filter.excludeTags);
  smartFolderExcludeFormats.value = joinListInput(folder.filter.excludeFormats);
  smartFolderExcludeMetadataFilters.value = formatExcludeMetadataFiltersInput(folder.filter);
  smartFolderNumberFilters.value = formatNumberFiltersInput(folder.filter);
  smartFolderDateFilters.value = formatDateFiltersInput(folder.filter);
  smartFolderSortField.value = folder.filter.sort?.field ?? "";
  smartFolderSortDirection.value = folder.filter.sort?.direction === "desc" ? "desc" : "asc";
  smartFolderLimit.value = folder.filter.limit == null ? "" : String(folder.filter.limit);
  showSmartFolderDialog.value = true;
}

function closeSmartFolderDialog() {
  if (isMutatingSmartFolder.value) return;
  showSmartFolderDialog.value = false;
}

async function submitSmartFolderDialog() {
  const name = smartFolderName.value.trim();
  if (!name) return;
  const filter = buildSmartFolderFilter();
  const parentId = smartFolderParentId.value || undefined;
  const response = smartFolderDialogMode.value === "create"
    ? await createSmartFolderInWorkspace({ parentId, name, filter })
    : await updateSmartFolderInWorkspace({
      smartFolderId: smartFolderTargetId.value,
      parentId,
      name,
      filter,
    });
  if (!response) return;
  if (parentId) {
    const next = new Set(expandedSmartFolderIds.value);
    next.add(parentId);
    expandedSmartFolderIds.value = Array.from(next);
  }
  showSmartFolderDialog.value = false;
}

function openDeleteSmartFolderDialog(smartFolderId: string, label: string) {
  pendingDeleteSmartFolderId.value = smartFolderId;
  pendingDeleteSmartFolderLabel.value = label;
  showSmartFolderDeleteDialog.value = true;
}

function closeSmartFolderDeleteDialog() {
  if (isMutatingSmartFolder.value) return;
  showSmartFolderDeleteDialog.value = false;
}

async function confirmDeleteSmartFolder() {
  const response = await deleteSmartFolderInWorkspace(pendingDeleteSmartFolderId.value);
  if (response) {
    showSmartFolderDeleteDialog.value = false;
  }
}

function openCreateFolderDialog(parentPath = "") {
  folderDialogMode.value = "create";
  folderDialogParentPath.value = parentPath;
  folderDialogTargetPath.value = "";
  folderDialogLabel.value = "";
  folderDialogValue.value = "";
  showFolderDialog.value = true;
}

function openRenameFolderDialog(path: string, label: string) {
  folderDialogMode.value = "rename";
  folderDialogParentPath.value = "";
  folderDialogTargetPath.value = path;
  folderDialogLabel.value = label;
  folderDialogValue.value = label;
  showFolderDialog.value = true;
}

function closeFolderDialog() {
  if (isMutatingFiles.value) return;
  showFolderDialog.value = false;
}

async function submitFolderDialog() {
  const value = folderDialogValue.value.trim();
  if (!value) return;

  if (folderDialogMode.value === "create") {
    const snapshot = await createDirectoryInWorkspace(value, folderDialogParentPath.value);
    if (snapshot) {
      if (folderDialogParentPath.value) {
        const next = new Set(expandedFolderPaths.value);
        next.add(folderDialogParentPath.value);
        expandedFolderPaths.value = Array.from(next);
      }
      showFolderDialog.value = false;
    }
    return;
  }

  const snapshot = await renameWorkspaceEntry(folderDialogTargetPath.value, value);
  if (snapshot) {
    showFolderDialog.value = false;
  }
}

function openDeleteFolderDialog(path: string, label: string) {
  pendingDeleteFolderPath.value = path;
  pendingDeleteFolderLabel.value = label;
  showFolderDeleteDialog.value = true;
}

function closeDeleteFolderDialog() {
  if (isMutatingFiles.value) return;
  showFolderDeleteDialog.value = false;
}

async function confirmDeleteFolder(mode: FileDeleteMode) {
  const snapshot = await deleteWorkspaceEntry(pendingDeleteFolderPath.value, mode);
  if (snapshot) {
    showFolderDeleteDialog.value = false;
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

async function createLocalRepositoryFromPath(path: string, fallbackPosition = addRepositoryPopoverPosition.value) {
  const nextPath = path.trim();
  if (!nextPath) return false;
  backendPluginId.value = localFilesystemPluginId;
  addRepositoryError.value = "";
  isSubmittingBackend.value = true;
  try {
    await attachRepository(nextPath);
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

async function submitAddRepositoryForm() {
  if (backendSubmitDisabled.value) return;
  isSubmittingBackend.value = true;
  addRepositoryError.value = "";
  try {
    const name = backendName.value.trim() || selectedBackend.value?.name || "新资源库";
    const path = backendUrl.value.trim();
    await createNewRepository(name, path, backendPluginId.value, {
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
  clearFolderDragHover();
  window.removeEventListener("momo:add-repository", handleAddRepositoryRequest);
  document.removeEventListener("keydown", handleDocumentKeydown);
  document.removeEventListener("pointerdown", handleDocumentPointerDown, true);
});
</script>

<template>
  <aside class="secondary-panel secondary-panel--workspace">
    <div class="workspace-sidebar">
      <section class="workspace-sidebar__top" aria-label="资源库与视图">
        <div class="workspace-sidebar__repo-head">
          <button
            ref="repositorySwitcherButtonRef"
            type="button"
            class="workspace-sidebar__repo-current"
            :title="activeRepository?.path ?? '添加资源库'"
            aria-haspopup="menu"
            :aria-expanded="addRepositoryPopoverMode === 'switcher'"
            aria-label="资源库"
            @click="openRepositorySwitcherFromEvent"
          >
            <span>{{ activeRepository?.name ?? "无资源库" }}</span>
            <ChevronsUpDown :size="13" aria-hidden="true" />
          </button>
        </div>
      </section>

      <section class="workspace-sidebar__files" aria-label="文件管理">
        <div v-if="error" class="workspace-state workspace-state--error">
          {{ error }}
        </div>

        <div v-else-if="isBusy" class="workspace-state">
          <LoaderCircle class="spin" :size="16" aria-hidden="true" />
          正在同步仓库状态
        </div>

        <div v-else-if="isShowingSyncProgress" class="workspace-state workspace-state--progress">
          <LoaderCircle class="spin" :size="16" aria-hidden="true" />
          <span>{{ syncProgress.label }}</span>
          <span class="workspace-state__percent">{{ syncProgress.percent }}%</span>
        </div>

        <section class="workspace-group">
          <div class="workspace-shortcuts">
            <button
              v-for="item in shortcuts"
              :key="item.id"
              type="button"
              class="workspace-shortcuts__item"
              :class="{ 'is-active': activePanel === item.id || (item.id === 'all' && activePanel === 'files') }"
              :disabled="isActiveRepositoryMissing"
              @click="selectShortcut(item.id)"
            >
              <span class="workspace-shortcuts__label">
                <component :is="item.icon" :size="15" aria-hidden="true" />
                {{ item.label }}
              </span>
              <span class="workspace-shortcuts__count">{{ item.count }}</span>
            </button>
          </div>
        </section>

        <section v-if="quickAccess.length" class="workspace-group">
          <div class="workspace-group__header">
            <span>快捷访问</span>
          </div>
          <div class="workspace-shortcuts">
            <button
              v-for="item in quickAccess"
              :key="item.shortcutId"
              type="button"
              class="workspace-shortcuts__item"
              :disabled="isActiveRepositoryMissing"
              :title="item.targetPath ?? item.targetId ?? item.label"
              @click="openQuickAccess(item)"
            >
              <span class="workspace-shortcuts__label">
                <component :is="shortcutIcon(item)" :size="15" aria-hidden="true" />
                {{ item.label }}
              </span>
            </button>
          </div>
        </section>

        <section class="workspace-group workspace-group--tree">
          <div class="workspace-group__header">
            <span>文件夹</span>
            <div class="workspace-group__actions">
              <button
                type="button"
                class="workspace-tree-action"
              :disabled="!activeRepoId || isActiveRepositoryMissing || isMutatingFiles || isTrashPanel"
                title="在当前目录新建文件夹"
                aria-label="在当前目录新建文件夹"
                @click="openCreateFolderDialog(currentDirectoryPath)"
              >
                <Plus :size="13" aria-hidden="true" />
              </button>
              <button
                type="button"
                class="workspace-tree-action"
              :disabled="!activeRepoId || isActiveRepositoryMissing || isLoadingFileBrowser"
                title="刷新文件夹树"
                aria-label="刷新文件夹树"
                @click="refreshFileBrowserTree"
              >
                <RefreshCw v-if="!isLoadingFileBrowser" :size="13" aria-hidden="true" />
                <LoaderCircle v-else class="spin" :size="13" aria-hidden="true" />
              </button>
            </div>
          </div>
          <div v-if="!activeRepoId" class="workspace-empty workspace-empty--compact">
            <p class="workspace-empty__text">先选择或添加一个资源库。</p>
          </div>
          <div v-else-if="isActiveRepositoryMissing" class="workspace-empty workspace-empty--compact">
            <p class="workspace-empty__text">资源库文件夹丢失，请先在主视图修复。</p>
          </div>
          <div v-else class="workspace-folder-tree">
            <FolderTreeNode
              v-if="!isTrashPanel"
              v-for="node in fileTreeNodes"
              :key="node.path"
              :node="node"
              :current-path="currentDirectoryPath"
              :expanded-paths="expandedFolderPathSet"
              :depth="1"
              :drop-target-path="dragHoverFolderPath"
              :is-drag-active="isFolderDragActive"
              :is-mutating="isMutatingFiles"
              @toggle="toggleFolderExpansion"
              @open="openFolder"
              @create="openCreateFolderDialog"
              @rename="openRenameFolderDialog"
              @delete="openDeleteFolderDialog"
              @hover-folder="handleFolderDragHover"
              @leave-folder="handleFolderDragLeave"
              @drop-folder="handleFolderDrop"
            />
          </div>
          <div v-if="activeRepoId && !isActiveRepositoryMissing && (isTrashPanel || !fileTreeNodes.length) && !isLoadingFileBrowser" class="workspace-empty workspace-empty--compact">
            <p class="workspace-empty__text">{{ isTrashPanel ? "回收站条目在主视图中管理。" : "当前仓库还没有子文件夹。" }}</p>
          </div>
        </section>

        <section class="workspace-group workspace-group--tree">
          <div class="workspace-group__header">
            <span>智能文件夹</span>
            <div class="workspace-group__actions">
              <button
                type="button"
                class="workspace-tree-action"
                :disabled="!activeRepoId || isActiveRepositoryMissing || isMutatingSmartFolder"
                title="新建智能文件夹"
                aria-label="新建智能文件夹"
                @click="openCreateSmartFolderDialog()"
              >
                <Plus :size="13" aria-hidden="true" />
              </button>
            </div>
          </div>
          <div v-if="!activeRepoId" class="workspace-empty workspace-empty--compact">
            <p class="workspace-empty__text">先选择或添加一个资源库。</p>
          </div>
          <div v-else-if="isActiveRepositoryMissing" class="workspace-empty workspace-empty--compact">
            <p class="workspace-empty__text">资源库修复后可继续使用智能文件夹。</p>
          </div>
          <div v-else-if="smartFolders.length" class="workspace-folder-tree">
            <SmartFolderTreeNode
              v-for="node in smartFolders"
              :key="node.smartFolderId"
              :node="node"
              :active-id="activeSmartFolderId"
              :expanded-ids="expandedSmartFolderIdSet"
              :depth="1"
              :is-mutating="isMutatingSmartFolder"
              @toggle="toggleSmartFolderExpansion"
              @open="openSmartFolder"
              @create="openCreateSmartFolderDialog"
              @edit="openEditSmartFolderDialog"
              @delete="openDeleteSmartFolderDialog"
            />
          </div>
          <div v-else class="workspace-empty workspace-empty--compact">
            <p class="workspace-empty__text">还没有智能文件夹。</p>
          </div>
        </section>
      </section>

      <footer class="workspace-sidebar__footer" aria-label="辅助入口">
        <RouterLink
          to="/settings"
          class="workspace-footer__btn"
          active-class="is-active"
          title="设置"
          aria-label="设置"
        >
          <Settings :size="14" aria-hidden="true" />
        </RouterLink>
        <button
          type="button"
          class="workspace-footer__btn"
          :class="{ 'is-active': activePanel === 'extensions' && route.path !== '/settings' }"
          title="拓展"
          aria-label="拓展"
          @click="selectPanel('extensions')"
        >
          <Puzzle :size="14" aria-hidden="true" />
        </button>
        <TaskPopover />
      </footer>
    </div>
  </aside>

  <Teleport to="body">
    <Transition name="panel">
      <section
        v-if="addRepositoryPopoverMode !== 'closed'"
        ref="addRepositoryPopoverRef"
        class="repository-add-popover"
        :class="{
          'ctx-menu': addRepositoryPopoverMode === 'addMenu',
          'repository-add-popover--menu': addRepositoryPopoverMode === 'addMenu',
          'repository-add-popover--switcher': addRepositoryPopoverMode === 'switcher',
        }"
        :style="{
          left: `${addRepositoryPopoverPosition.left}px`,
          top: `${addRepositoryPopoverPosition.top}px`,
          width: addRepositoryPopoverMode === 'switcher' ? `${addRepositoryPopoverPosition.width}px` : undefined,
        }"
        :aria-label="addRepositoryPopoverMode === 'switcher' ? '切换资源库' : '添加资源库'"
      >
        <template v-if="addRepositoryPopoverMode === 'switcher'">
          <div class="repository-switcher__list" role="menu" aria-label="资源库列表">
            <button
              v-for="library in repositories"
              :key="library.repoId"
              type="button"
              class="repository-switcher__item"
              :class="{ 'is-active': activeRepoId === library.repoId, 'is-missing': library.status === 'missing' }"
              :title="`${library.name}\n${library.path}`"
              :aria-label="`切换资源库 ${library.name}`"
              :disabled="isRemovingRepository || isSubmittingBackend"
              @click="selectRepositoryFromList(library.repoId)"
            >
              <span class="repository-switcher__check">
                <Check v-if="activeRepoId === library.repoId" :size="13" aria-hidden="true" />
              </span>
              <span class="repository-switcher__main">
                <strong>{{ library.name }}</strong>
                <span v-if="library.status === 'missing'" class="repository-switcher__status">丢失</span>
              </span>
            </button>
          </div>

          <div class="repository-switcher__actions">
            <button
              type="button"
              class="ctx-menu__item"
              :disabled="isSubmittingBackend || isRemovingRepository"
              @click="showAddRepositoryMenuFromSwitcher"
            >
              <Plus :size="14" aria-hidden="true" />
              <span class="ctx-menu__label">添加资源库</span>
            </button>
            <button
              type="button"
              class="ctx-menu__item ctx-menu__item--danger"
              :class="{ 'ctx-menu__item--pending': isConfirmingRepositoryDelete }"
              :disabled="!activeRepoId || isSubmittingBackend || isRemovingRepository"
              @click="deleteActiveRepositoryFromMenu"
            >
              <LoaderCircle v-if="isRemovingRepository" class="spin" :size="14" aria-hidden="true" />
              <Trash2 v-else :size="14" aria-hidden="true" />
              <span class="ctx-menu__label">
                {{ isConfirmingRepositoryDelete ? "确认删除当前资源库" : "删除当前资源库" }}
              </span>
            </button>
          </div>

          <p v-if="addRepositoryError" class="repository-add-popover__error">
            {{ addRepositoryError }}
          </p>
        </template>

        <template v-else-if="addRepositoryPopoverMode === 'addMenu'">
          <button
            v-for="option in backendOptions"
            :key="option.value"
            type="button"
            class="ctx-menu__item"
            :disabled="isSubmittingBackend || !option.enabled"
            @click="selectBackend(String(option.value))"
          >
            {{ option.label }}
          </button>

          <p v-if="addRepositoryError" class="repository-add-popover__error">
            {{ addRepositoryError }}
          </p>
        </template>

        <template v-else>
          <header class="repository-add-popover__header">
            <span>{{ selectedBackend?.name ?? "添加资源库" }}</span>
            <button
              type="button"
              class="repository-add-popover__close"
              title="关闭"
              aria-label="关闭添加资源库"
              :disabled="isSubmittingBackend"
              @click="closeAddRepositoryPopover"
            >
              <X :size="13" aria-hidden="true" />
            </button>
          </header>

          <div class="repository-add-popover__body">
            <p class="repository-add-popover__summary">
              {{ selectedBackend?.description ?? "填写资源库配置。" }}
            </p>

            <label class="repository-add-popover__field">
              <span>资源库名称</span>
              <input v-model="backendName" type="text" placeholder="可选，默认使用后端名称" :disabled="isSubmittingBackend" />
            </label>

            <label class="repository-add-popover__field">
              <span>服务地址</span>
              <input v-model="backendUrl" type="url" placeholder="https://example.com/dav/" :disabled="isSubmittingBackend" />
            </label>
            <label class="repository-add-popover__field">
              <span>根目录</span>
              <input v-model="backendRoot" type="text" placeholder="/assets/anime" :disabled="isSubmittingBackend" />
            </label>
            <label class="repository-add-popover__field">
              <span>用户名</span>
              <input v-model="backendUsername" type="text" placeholder="可选" :disabled="isSubmittingBackend" />
            </label>
            <label class="repository-add-popover__field">
              <span>密码 / Token</span>
              <input v-model="backendPassword" type="password" placeholder="可选" :disabled="isSubmittingBackend" />
            </label>
            <p class="repository-add-popover__note">
              当前仅完成后端配置入口与服务端抽象。远端适配器尚未实现，请先用于配置演进与契约联调。
            </p>

            <p v-if="addRepositoryError" class="repository-add-popover__error">
              {{ addRepositoryError }}
            </p>
          </div>

          <div class="repository-add-popover__actions">
            <button type="button" class="ghost" :disabled="isSubmittingBackend" @click="addRepositoryPopoverMode = 'addMenu'">
              返回
            </button>
            <button type="button" class="ghost" :disabled="isSubmittingBackend" @click="closeAddRepositoryPopover">
              取消
            </button>
            <button
              type="button"
              class="primary"
              :disabled="isSubmittingBackend || backendSubmitDisabled"
              @click="submitAddRepositoryForm"
            >
              <LoaderCircle v-if="isSubmittingBackend" class="spin" :size="13" aria-hidden="true" />
              {{ isSubmittingBackend ? "创建中" : "创建" }}
            </button>
          </div>
        </template>
      </section>
    </Transition>
  </Teleport>

  <Teleport to="body">
    <Transition name="modal">
      <div
        v-if="showSmartFolderDialog"
        class="modal-overlay"
        role="dialog"
        aria-modal="true"
        :aria-label="smartFolderDialogTitle"
        @click.self="closeSmartFolderDialog"
      >
        <div class="modal-card dialog-card smart-folder-dialog">
          <div class="dialog-card__header">
            <span>{{ smartFolderDialogTitle }}</span>
          </div>
          <div class="dialog-card__body smart-folder-dialog__body">
            <label class="dialog-field">
              <span>名称</span>
              <input
                v-model="smartFolderName"
                type="text"
                placeholder="例如 高评分 PSD"
                @keydown.enter.prevent="submitSmartFolderDialog"
              />
            </label>

            <label class="dialog-field">
              <span>父级</span>
              <select v-model="smartFolderParentId">
                <option value="">顶层智能文件夹</option>
                <option
                  v-for="folder in flatSmartFolders.filter((item) => item.smartFolderId !== smartFolderTargetId)"
                  :key="folder.smartFolderId"
                  :value="folder.smartFolderId"
                >
                  {{ folder.name }}
                </option>
              </select>
            </label>

            <div class="smart-folder-dialog__grid">
              <label class="dialog-field">
                <span>关键词</span>
                <input v-model="smartFolderQuery" type="text" placeholder="文件名、标签或元数据" />
              </label>
              <label class="dialog-field">
                <span>路径前缀</span>
                <input v-model="smartFolderPathPrefix" type="text" placeholder="Campaigns/Summer" />
              </label>
              <label class="dialog-field">
                <span>格式</span>
                <input v-model="smartFolderFormats" type="text" placeholder="psd，png" />
              </label>
              <label class="dialog-field">
                <span>标签</span>
                <input v-model="smartFolderTags" type="text" placeholder="封面，主视觉" />
              </label>
              <label class="dialog-field">
                <span>颜色</span>
                <input v-model="smartFolderColors" type="text" placeholder="红色，绿色" />
              </label>
              <label class="dialog-field">
                <span>形状</span>
                <input v-model="smartFolderShapes" type="text" placeholder="方形，横版" />
              </label>
              <label class="dialog-field">
                <span>最低评分</span>
                <input v-model="smartFolderMinRating" type="number" min="1" max="5" step="1" placeholder="4" />
              </label>
              <label class="dialog-field">
                <span>匹配方式</span>
                <select v-model="smartFolderMatchMode">
                  <option value="and">全部匹配</option>
                  <option value="or">任一匹配</option>
                </select>
              </label>
            </div>

            <label class="dialog-field">
              <span>元数据键值</span>
              <textarea
                v-model="smartFolderMetadataFilters"
                rows="3"
                placeholder="artist=demo&#10;source=reference"
              />
            </label>

            <div class="smart-folder-dialog__grid">
              <label class="dialog-field">
                <span>排除标签</span>
                <input v-model="smartFolderExcludeTags" type="text" placeholder="草稿，临时" />
              </label>
              <label class="dialog-field">
                <span>排除格式</span>
                <input v-model="smartFolderExcludeFormats" type="text" placeholder="gif，webp" />
              </label>
              <label class="dialog-field">
                <span>排序字段</span>
                <input v-model="smartFolderSortField" type="text" placeholder="modifiedAt / rating / metadata.width" />
              </label>
              <label class="dialog-field">
                <span>排序方向</span>
                <select v-model="smartFolderSortDirection">
                  <option value="asc">升序</option>
                  <option value="desc">降序</option>
                </select>
              </label>
              <label class="dialog-field">
                <span>结果数量</span>
                <input v-model="smartFolderLimit" type="number" min="1" step="1" placeholder="100" />
              </label>
            </div>

            <label class="dialog-field">
              <span>排除元数据</span>
              <textarea
                v-model="smartFolderExcludeMetadataFilters"
                rows="2"
                placeholder="status=archived"
              />
            </label>

            <label class="dialog-field">
              <span>数值范围</span>
              <textarea
                v-model="smartFolderNumberFilters"
                rows="2"
                placeholder="width=1024..4096&#10;originalSizeBytes=..10485760"
              />
            </label>

            <label class="dialog-field">
              <span>日期范围</span>
              <textarea
                v-model="smartFolderDateFilters"
                rows="2"
                placeholder="fileCreatedAt=2024-01-01T00:00:00Z..2024-12-31T23:59:59Z"
              />
            </label>
          </div>
          <div class="dialog-card__actions">
            <button type="button" class="ghost" :disabled="isMutatingSmartFolder" @click="closeSmartFolderDialog">
              取消
            </button>
            <button
              type="button"
              class="primary"
              :disabled="smartFolderDialogDisabled"
              @click="submitSmartFolderDialog"
            >
              <LoaderCircle v-if="isMutatingSmartFolder" class="spin" :size="13" aria-hidden="true" />
              {{ smartFolderDialogActionLabel }}
            </button>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>

  <Teleport to="body">
    <Transition name="modal">
      <div
        v-if="showSmartFolderDeleteDialog"
        class="modal-overlay"
        role="dialog"
        aria-modal="true"
        aria-label="删除智能文件夹"
        @click.self="closeSmartFolderDeleteDialog"
      >
        <div class="modal-card dialog-card folder-delete-dialog">
          <div class="dialog-card__header dialog-card__header--danger">
            <span>删除智能文件夹</span>
          </div>
          <div class="dialog-card__body folder-delete-dialog__body">
            <p>将删除“{{ pendingDeleteSmartFolderLabel }}”及其子智能文件夹。实际文件和真实目录不会被删除。</p>
          </div>
          <div class="dialog-card__actions">
            <button type="button" class="ghost" :disabled="isMutatingSmartFolder" @click="closeSmartFolderDeleteDialog">
              取消
            </button>
            <button type="button" class="ghost danger" :disabled="isMutatingSmartFolder" @click="confirmDeleteSmartFolder">
              删除
            </button>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>

  <Teleport to="body">
    <Transition name="modal">
      <div
        v-if="showFolderDialog"
        class="modal-overlay"
        role="dialog"
        aria-modal="true"
        :aria-label="folderDialogTitle"
        @click.self="closeFolderDialog"
      >
        <div class="modal-card dialog-card folder-dialog">
          <div class="dialog-card__header">
            <span>{{ folderDialogTitle }}</span>
          </div>
          <div class="dialog-card__body folder-dialog__body">
            <p class="folder-dialog__summary">
              {{
                folderDialogMode === "create"
                  ? `将在 ${folderDialogParentPath || "根目录"} 下创建新文件夹。`
                  : `正在重命名 ${folderDialogLabel}。`
              }}
            </p>
            <label class="dialog-field">
              <span>文件夹名称</span>
              <input
                v-model="folderDialogValue"
                type="text"
                :placeholder="folderDialogPlaceholder"
                @keydown.enter.prevent="submitFolderDialog"
              />
            </label>
          </div>
          <div class="dialog-card__actions">
            <button type="button" class="ghost" :disabled="isMutatingFiles" @click="closeFolderDialog">
              取消
            </button>
            <button
              type="button"
              class="primary"
              :disabled="folderDialogDisabled"
              @click="submitFolderDialog"
            >
              {{ folderDialogActionLabel }}
            </button>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>

  <Teleport to="body">
    <Transition name="modal">
      <div
        v-if="showFolderDeleteDialog"
        class="modal-overlay"
        role="dialog"
        aria-modal="true"
        aria-label="处理文件夹"
        @click.self="closeDeleteFolderDialog"
      >
        <div class="modal-card dialog-card folder-delete-dialog">
          <div class="dialog-card__header dialog-card__header--danger">
            <span>处理文件夹</span>
          </div>
          <div class="dialog-card__body folder-delete-dialog__body">
            <p>将处理文件夹“{{ pendingDeleteFolderLabel }}”。请选择内部内容的处理方式。</p>
            <div class="folder-delete-dialog__options">
              <button
                type="button"
                class="folder-delete-dialog__option"
                :disabled="isMutatingFiles"
                @click="confirmDeleteFolder('moveToParent')"
              >
                <strong>转移到上级目录</strong>
                <span>保留内部文件和子文件夹，只删除当前这一层目录。</span>
              </button>
              <button
                type="button"
                class="folder-delete-dialog__option folder-delete-dialog__option--danger"
                :disabled="isMutatingFiles"
                @click="confirmDeleteFolder('delete')"
              >
                <strong>移入回收站</strong>
                <span>将该目录及其全部内容移入回收站，可在回收站中还原。</span>
              </button>
            </div>
          </div>
          <div class="dialog-card__actions">
            <button type="button" class="ghost" :disabled="isMutatingFiles" @click="closeDeleteFolderDialog">
              取消
            </button>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>
