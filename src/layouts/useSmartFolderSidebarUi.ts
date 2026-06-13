import { computed, onBeforeUnmount, ref, shallowRef, watch, type ComputedRef } from "vue";
import { scheduleIdleTask } from "../composables/workspace/scheduler";
import {
  formatDateFiltersInput,
  formatExcludeDateFiltersInput,
  formatExcludeMetadataFiltersInput,
  formatExcludeNumberFiltersInput,
  formatMetadataFiltersInput,
  formatNumberFiltersInput,
  joinListInput,
  parseDateFiltersInput,
  parseMetadataFiltersInput,
  parseNumberFiltersInput,
  splitListInput,
} from "../composables/workspace/filterInputs";
import {
  flattenSmartFolders,
  smartFolderAncestry,
  smartFolderMapFromFlatList,
} from "./smartFolderTree";
import type {
  SmartFolder,
  SmartFolderFilter,
  SmartFolderMutationRequest,
  SmartFolderMutationResponse,
  SmartFolderUpdateRequest,
  SmartFolderTreeNode,
} from "../types/repository";

type SmartFolderSidebarUiOptions = {
  activeSmartFolderId: ComputedRef<string | null>;
  createSmartFolderInWorkspace: (request: Omit<SmartFolderMutationRequest, "repoId">) => Promise<SmartFolderMutationResponse | null>;
  deleteSmartFolderInWorkspace: (smartFolderId: string) => Promise<SmartFolderMutationResponse | null>;
  isMutatingSmartFolder: ComputedRef<boolean>;
  smartFolders: ComputedRef<SmartFolderTreeNode[]>;
  updateSmartFolderInWorkspace: (request: Omit<SmartFolderUpdateRequest, "repoId">) => Promise<SmartFolderMutationResponse | null>;
};

export function useSmartFolderSidebarUi(options: SmartFolderSidebarUiOptions) {
  const expandedSmartFolderIds = ref<string[]>([]);
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
  const smartFolderExcludeQuery = ref("");
  const smartFolderExcludePathPrefixes = ref("");
  const smartFolderExcludeTags = ref("");
  const smartFolderExcludeFormats = ref("");
  const smartFolderExcludeMetadataFilters = ref("");
  const smartFolderExcludeNumberFilters = ref("");
  const smartFolderExcludeDateFilters = ref("");
  const smartFolderNumberFilters = ref("");
  const smartFolderDateFilters = ref("");
  const smartFolderSortField = ref("");
  const smartFolderSortDirection = ref<"asc" | "desc">("asc");
  const smartFolderLimit = ref("");
  const showSmartFolderDeleteDialog = ref(false);
  const pendingDeleteSmartFolderId = ref("");
  const pendingDeleteSmartFolderLabel = ref("");
  const flatSmartFolders = shallowRef<SmartFolder[]>([]);
  const smartFolderById = shallowRef<ReadonlyMap<string, SmartFolder>>(new Map());
  let cancelSmartFolderFlatten: (() => void) | null = null;

  const expandedSmartFolderIdSet = computed(() => new Set(expandedSmartFolderIds.value));
  const smartFolderDialogTitle = computed(() => (
    smartFolderDialogMode.value === "create" ? "新建智能文件夹" : "编辑智能文件夹"
  ));
  const smartFolderDialogActionLabel = computed(() => (
    smartFolderDialogMode.value === "create" ? "创建" : "保存"
  ));
  const smartFolderDialogDisabled = computed(() => !smartFolderName.value.trim() || options.isMutatingSmartFolder.value);

  watch(
    options.smartFolders,
    (nodes) => {
      cancelSmartFolderFlatten?.();
      cancelSmartFolderFlatten = scheduleIdleTask(() => {
        const folders = flattenSmartFolders(nodes);
        flatSmartFolders.value = folders;
        smartFolderById.value = smartFolderMapFromFlatList(folders);
      }, 200);
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
    options.activeSmartFolderId,
    (id) => {
      if (!id) return;
      const path = smartFolderAncestry(id, smartFolderById.value);
      if (!path.length) return;
      const next = new Set(expandedSmartFolderIds.value);
      for (const item of path.slice(0, -1)) {
        next.add(item.smartFolderId);
      }
      expandedSmartFolderIds.value = Array.from(next);
    },
    { immediate: true },
  );

  onBeforeUnmount(() => {
    cancelSmartFolderFlatten?.();
  });

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
      excludeQuery: smartFolderExcludeQuery.value.trim() || undefined,
      excludePathPrefixes: splitListInput(smartFolderExcludePathPrefixes.value),
      excludeTags: splitListInput(smartFolderExcludeTags.value),
      excludeFormats: splitListInput(smartFolderExcludeFormats.value),
      excludeMetadataFilters: parseMetadataFiltersInput(smartFolderExcludeMetadataFilters.value),
      excludeNumberFilters: parseNumberFiltersInput(smartFolderExcludeNumberFilters.value),
      excludeDateFilters: parseDateFiltersInput(smartFolderExcludeDateFilters.value),
      numberFilters: parseNumberFiltersInput(smartFolderNumberFilters.value),
      dateFilters: parseDateFiltersInput(smartFolderDateFilters.value),
      sort: smartFolderSortField.value.trim()
        ? { field: smartFolderSortField.value.trim(), direction: smartFolderSortDirection.value }
        : undefined,
      limit: Number.isFinite(limit) && limit > 0 ? limit : undefined,
    };
  }

  function findSmartFolder(id: string) {
    return smartFolderById.value.get(id) ?? null;
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
    smartFolderExcludeQuery.value = "";
    smartFolderExcludePathPrefixes.value = "";
    smartFolderExcludeTags.value = "";
    smartFolderExcludeFormats.value = "";
    smartFolderExcludeMetadataFilters.value = "";
    smartFolderExcludeNumberFilters.value = "";
    smartFolderExcludeDateFilters.value = "";
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
    smartFolderExcludeQuery.value = folder.filter.excludeQuery ?? "";
    smartFolderExcludePathPrefixes.value = joinListInput(folder.filter.excludePathPrefixes);
    smartFolderExcludeTags.value = joinListInput(folder.filter.excludeTags);
    smartFolderExcludeFormats.value = joinListInput(folder.filter.excludeFormats);
    smartFolderExcludeMetadataFilters.value = formatExcludeMetadataFiltersInput(folder.filter);
    smartFolderExcludeNumberFilters.value = formatExcludeNumberFiltersInput(folder.filter);
    smartFolderExcludeDateFilters.value = formatExcludeDateFiltersInput(folder.filter);
    smartFolderNumberFilters.value = formatNumberFiltersInput(folder.filter);
    smartFolderDateFilters.value = formatDateFiltersInput(folder.filter);
    smartFolderSortField.value = folder.filter.sort?.field ?? "";
    smartFolderSortDirection.value = folder.filter.sort?.direction === "desc" ? "desc" : "asc";
    smartFolderLimit.value = folder.filter.limit == null ? "" : String(folder.filter.limit);
    showSmartFolderDialog.value = true;
  }

  function closeSmartFolderDialog() {
    if (options.isMutatingSmartFolder.value) return;
    showSmartFolderDialog.value = false;
  }

  async function submitSmartFolderDialog() {
    const name = smartFolderName.value.trim();
    if (!name) return;
    const filter = buildSmartFolderFilter();
    const parentId = smartFolderParentId.value || undefined;
    const response = smartFolderDialogMode.value === "create"
      ? await options.createSmartFolderInWorkspace({ parentId, name, filter })
      : await options.updateSmartFolderInWorkspace({
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
    if (options.isMutatingSmartFolder.value) return;
    showSmartFolderDeleteDialog.value = false;
  }

  async function confirmDeleteSmartFolder() {
    const response = await options.deleteSmartFolderInWorkspace(pendingDeleteSmartFolderId.value);
    if (response) {
      showSmartFolderDeleteDialog.value = false;
    }
  }

  return {
    closeSmartFolderDeleteDialog,
    closeSmartFolderDialog,
    confirmDeleteSmartFolder,
    expandedSmartFolderIdSet,
    flatSmartFolders,
    openCreateSmartFolderDialog,
    openDeleteSmartFolderDialog,
    openEditSmartFolderDialog,
    pendingDeleteSmartFolderLabel,
    showSmartFolderDeleteDialog,
    showSmartFolderDialog,
    smartFolderColors,
    smartFolderDateFilters,
    smartFolderDialogActionLabel,
    smartFolderDialogDisabled,
    smartFolderDialogTitle,
    smartFolderExcludeDateFilters,
    smartFolderExcludeFormats,
    smartFolderExcludeMetadataFilters,
    smartFolderExcludeNumberFilters,
    smartFolderExcludePathPrefixes,
    smartFolderExcludeQuery,
    smartFolderExcludeTags,
    smartFolderFormats,
    smartFolderLimit,
    smartFolderMatchMode,
    smartFolderMetadataFilters,
    smartFolderMinRating,
    smartFolderName,
    smartFolderNumberFilters,
    smartFolderParentId,
    smartFolderPathPrefix,
    smartFolderQuery,
    smartFolderShapes,
    smartFolderSortDirection,
    smartFolderSortField,
    smartFolderTags,
    smartFolderTargetId,
    submitSmartFolderDialog,
    toggleSmartFolderExpansion,
  };
}
