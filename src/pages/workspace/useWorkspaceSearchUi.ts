import { computed, ref, shallowRef, watch, type ComputedRef } from "vue";
import { splitListInput } from "../../composables/workspace/filterInputs";
import type {
  FileBrowserSnapshot,
  RepositorySnapshot,
  SearchHit,
} from "../../types/repository";
import { scheduleIdleTask, yieldEvery } from "../../composables/workspace/scheduler";

type SearchFilterListKey = "tags" | "formats" | "colors" | "shapes";

type WorkspaceSearchUiOptions = {
  activeSnapshot: ComputedRef<RepositorySnapshot | null>;
  fileBrowser: ComputedRef<FileBrowserSnapshot | null>;
  hasActiveFilters: ComputedRef<boolean>;
  isRepositoryWritable: ComputedRef<boolean>;
  searchQuery: ComputedRef<string>;
  searchResults: ComputedRef<SearchHit[]>;
  clearFilters: () => void;
  runFilteredSearch: () => unknown;
  setActivePanel: (panel: "search") => void;
  setMinimumRatingFilter: (value: number | null) => void;
  toggleFilterValue: (key: SearchFilterListKey, value: string) => void;
  updateFilters: (filters: {
    excludeQuery: string;
    excludePathPrefixes: string;
    metadataFilters: string;
    excludeTags: string[];
    excludeFormats: string[];
    excludeMetadataFilters: string;
    excludeNumberFilters: string;
    excludeDateFilters: string;
    numberFilters: string;
    dateFilters: string;
    sortField: string;
    sortDirection: "asc" | "desc";
    limit: number | null;
  }) => void;
};

const filterColorMap: Record<string, string> = {
  red: "#e05252",
  green: "#4f9d69",
  blue: "#4c7bd9",
  yellow: "#d6a93f",
  purple: "#8b6bd6",
  pink: "#d66b9a",
  orange: "#d98b3d",
  black: "#333333",
  white: "#e8e8e8",
  gray: "#8c9299",
  grey: "#8c9299",
  红色: "#e05252",
  绿色: "#4f9d69",
  蓝色: "#4c7bd9",
  黄色: "#d6a93f",
  紫色: "#8b6bd6",
  粉色: "#d66b9a",
  橙色: "#d98b3d",
  黑色: "#333333",
  白色: "#e8e8e8",
  灰色: "#8c9299",
};

async function uniqueSortedAsync(values: Array<string | null | undefined>) {
  const unique = new Set<string>();
  for (let index = 0; index < values.length; index += 1) {
    const value = values[index]?.trim() ?? "";
    if (value) unique.add(value);
    await yieldEvery(index);
  }
  return Array.from(unique).sort((left, right) => left.localeCompare(right, "zh-CN"));
}

function searchResultFormat(result: SearchHit) {
  const filename = result.filename || result.path;
  const index = filename.lastIndexOf(".");
  return index >= 0 ? filename.slice(index + 1).toLowerCase() : "";
}

function metadataText(metadata: Record<string, unknown>, key: string) {
  const value = metadata[key];
  if (typeof value === "string") return value;
  if (typeof value === "number" || typeof value === "boolean") return String(value);
  return "";
}

export function useWorkspaceSearchUi(options: WorkspaceSearchUiOptions) {
  const colorFilterInput = ref("");
  const shapeFilterInput = ref("");
  const excludeQueryInput = ref("");
  const excludePathPrefixesInput = ref("");
  const excludeTagsInput = ref("");
  const excludeFormatsInput = ref("");
  const metadataFiltersInput = ref("");
  const excludeMetadataFiltersInput = ref("");
  const excludeNumberFiltersInput = ref("");
  const excludeDateFiltersInput = ref("");
  const numberFiltersInput = ref("");
  const dateFiltersInput = ref("");
  const sortFieldInput = ref("");
  const sortDirectionInput = ref<"asc" | "desc">("asc");
  const limitInput = ref("");
  const tagFilterOptions = shallowRef<string[]>([]);
  const formatFilterOptions = shallowRef<string[]>([]);
  const colorFilterOptions = shallowRef<string[]>([]);
  const shapeFilterOptions = shallowRef<string[]>([]);
  let filterOptionsToken = 0;
  let cancelFilterOptionBuild: (() => void) | null = null;

  async function rebuildFilterOptions(token: number) {
    const assets = options.activeSnapshot.value?.assets ?? [];
    const results = options.searchResults.value;
    const tags: Array<string | null | undefined> = [];
    const formats: Array<string | null | undefined> = [];
    const colors: Array<string | null | undefined> = [];
    const shapes: Array<string | null | undefined> = [];

    for (let index = 0; index < assets.length; index += 1) {
      const asset = assets[index];
      tags.push(...asset.tags);
      formats.push(asset.extension);
      await yieldEvery(index);
      if (token !== filterOptionsToken) return;
    }
    for (let index = 0; index < results.length; index += 1) {
      const result = results[index];
      tags.push(...result.tags);
      formats.push(searchResultFormat(result));
      colors.push(metadataText(result.metadata, "color"));
      shapes.push(metadataText(result.metadata, "shape"));
      await yieldEvery(index);
      if (token !== filterOptionsToken) return;
    }
    const entries = options.fileBrowser.value?.entries ?? [];
    for (let index = 0; index < entries.length; index += 1) {
      colors.push(metadataText(entries[index].metadata ?? {}, "color"));
      shapes.push(metadataText(entries[index].metadata ?? {}, "shape"));
      await yieldEvery(index);
      if (token !== filterOptionsToken) return;
    }

    const [nextTags, nextFormats, nextColors, nextShapes] = await Promise.all([
      uniqueSortedAsync(tags),
      uniqueSortedAsync(formats),
      uniqueSortedAsync(colors),
      uniqueSortedAsync(shapes),
    ]);
    if (token !== filterOptionsToken) return;
    tagFilterOptions.value = nextTags;
    formatFilterOptions.value = nextFormats;
    colorFilterOptions.value = nextColors;
    shapeFilterOptions.value = nextShapes;
  }

  watch(
    () => [
      options.activeSnapshot.value,
      options.fileBrowser.value,
      options.searchResults.value,
    ] as const,
    () => {
      filterOptionsToken += 1;
      const token = filterOptionsToken;
      cancelFilterOptionBuild?.();
      cancelFilterOptionBuild = scheduleIdleTask(() => {
        void rebuildFilterOptions(token);
      }, 300);
    },
    { immediate: true },
  );

  const searchResultScopeLabel = computed(() => (
    options.hasActiveFilters.value
      ? `${options.activeSnapshot.value?.repository.name ?? "当前资源库"}内筛选`
      : "全局搜索"
  ));

  const searchSummary = computed(() => {
    if (options.hasActiveFilters.value) {
      return options.searchQuery.value.trim()
        ? `当前资源库筛选: ${options.searchQuery.value}`
        : "按当前资源库筛选结果。";
    }
    if (options.searchQuery.value.trim()) {
      return `当前查询: ${options.searchQuery.value}`;
    }
    return "输入关键词、标签或评分条件后，这里会展示跨仓库结果。";
  });

  function toggleSearchFilter(key: SearchFilterListKey, value: string) {
    if (!options.isRepositoryWritable.value) return;
    options.toggleFilterValue(key, value);
    options.setActivePanel("search");
    void options.runFilteredSearch();
  }

  function submitMetadataFilterInput(key: "colors" | "shapes") {
    if (!options.isRepositoryWritable.value) return;
    const input = key === "colors" ? colorFilterInput : shapeFilterInput;
    const value = input.value.trim();
    if (!value) return;
    toggleSearchFilter(key, value);
    input.value = "";
  }

  function selectMinimumRating(value: number | null) {
    if (!options.isRepositoryWritable.value) return;
    options.setMinimumRatingFilter(value);
    options.setActivePanel("search");
    void options.runFilteredSearch();
  }

  function clearSearchFilters() {
    if (!options.isRepositoryWritable.value) return;
    options.clearFilters();
    colorFilterInput.value = "";
    shapeFilterInput.value = "";
    excludeQueryInput.value = "";
    excludePathPrefixesInput.value = "";
    excludeTagsInput.value = "";
    excludeFormatsInput.value = "";
    metadataFiltersInput.value = "";
    excludeMetadataFiltersInput.value = "";
    excludeNumberFiltersInput.value = "";
    excludeDateFiltersInput.value = "";
    numberFiltersInput.value = "";
    dateFiltersInput.value = "";
    sortFieldInput.value = "";
    sortDirectionInput.value = "asc";
    limitInput.value = "";
    options.setActivePanel("search");
    void options.runFilteredSearch();
  }

  function applyAdvancedSearchFilters() {
    if (!options.isRepositoryWritable.value) return;
    const limit = Number(limitInput.value);
    options.updateFilters({
      excludeQuery: excludeQueryInput.value.trim(),
      excludePathPrefixes: excludePathPrefixesInput.value.trim(),
      metadataFilters: metadataFiltersInput.value.trim(),
      excludeTags: splitListInput(excludeTagsInput.value),
      excludeFormats: splitListInput(excludeFormatsInput.value),
      excludeMetadataFilters: excludeMetadataFiltersInput.value.trim(),
      excludeNumberFilters: excludeNumberFiltersInput.value.trim(),
      excludeDateFilters: excludeDateFiltersInput.value.trim(),
      numberFilters: numberFiltersInput.value.trim(),
      dateFilters: dateFiltersInput.value.trim(),
      sortField: sortFieldInput.value.trim(),
      sortDirection: sortDirectionInput.value,
      limit: Number.isFinite(limit) && limit > 0 ? limit : null,
    });
    options.setActivePanel("search");
    void options.runFilteredSearch();
  }

  function searchResultRating(result: SearchHit) {
    const value = result.metadata.rating;
    return typeof value === "number" && value > 0 ? value : null;
  }

  function searchResultContext(result: SearchHit) {
    const rating = searchResultRating(result);
    return [
      searchResultFormat(result) || "文件",
      ...result.tags.slice(0, 3),
      metadataText(result.metadata, "color"),
      metadataText(result.metadata, "shape"),
      rating == null ? "" : `${rating} 星`,
    ].filter(Boolean);
  }

  function filterColorStyle(color: string) {
    const trimmed = color.trim();
    const hexColor = /^#[0-9a-f]{6}$/i.test(trimmed) ? trimmed : null;
    return {
      "--filter-swatch": hexColor ?? filterColorMap[color.toLowerCase()] ?? filterColorMap[color] ?? "var(--accent)",
    };
  }

  return {
    colorFilterInput,
    shapeFilterInput,
    excludeQueryInput,
    excludePathPrefixesInput,
    excludeTagsInput,
    excludeFormatsInput,
    metadataFiltersInput,
    excludeMetadataFiltersInput,
    excludeNumberFiltersInput,
    excludeDateFiltersInput,
    numberFiltersInput,
    dateFiltersInput,
    sortFieldInput,
    sortDirectionInput,
    limitInput,
    tagFilterOptions,
    formatFilterOptions,
    colorFilterOptions,
    shapeFilterOptions,
    searchResultScopeLabel,
    searchSummary,
    toggleSearchFilter,
    submitMetadataFilterInput,
    selectMinimumRating,
    clearSearchFilters,
    applyAdvancedSearchFilters,
    searchResultContext,
    filterColorStyle,
  };
}
