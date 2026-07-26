import { searchAssets } from "../../services/repositoryApi";
import type { SearchRequest } from "../../types/repository";
import {
  activeRepoId,
  createInitialFilters,
  error,
  filters,
  isFilterBarOpen,
  isSearching,
  searchQuery,
  searchResults,
} from "./state";
import { hasActiveFilters } from "./selectors";
import {
  normalizeFilterValues,
  parseDateFiltersInput,
  parseMetadataFiltersInput,
  parseNumberFiltersInput,
  parsePathPrefixesInput,
} from "./filterInputs";

let latestSearchRequestId = 0;

/** 生成新的搜索请求序号，使此前仍在执行的请求失效。 */
function nextSearchRequestId() {
  latestSearchRequestId += 1;
  return latestSearchRequestId;
}

export function resetSearchState() {
  nextSearchRequestId();
  searchQuery.value = "";
  searchResults.value = [];
  filters.value = createInitialFilters();
  isFilterBarOpen.value = false;
  isSearching.value = false;
}

export function buildSearchRequest(query = searchQuery.value): SearchRequest {
  const nextFilters = filters.value;
  const metadataFilters = [
    ...nextFilters.colors.map((value) => ({ key: "color", value })),
    ...nextFilters.shapes.map((value) => ({ key: "shape", value })),
    ...parseMetadataFiltersInput(nextFilters.metadataFilters),
  ];
  const sortField = nextFilters.sortField.trim();

  return {
    query,
    repoId: hasActiveFilters.value ? activeRepoId.value ?? undefined : undefined,
    excludeQuery: nextFilters.excludeQuery.trim() || undefined,
    tags: normalizeFilterValues(nextFilters.tags),
    formats: normalizeFilterValues(nextFilters.formats),
    metadataFilters,
    excludeTags: normalizeFilterValues(nextFilters.excludeTags),
    excludeFormats: normalizeFilterValues(nextFilters.excludeFormats),
    excludeMetadataFilters: parseMetadataFiltersInput(nextFilters.excludeMetadataFilters),
    excludePathPrefixes: parsePathPrefixesInput(nextFilters.excludePathPrefixes),
    excludeNumberFilters: parseNumberFiltersInput(nextFilters.excludeNumberFilters),
    excludeDateFilters: parseDateFiltersInput(nextFilters.excludeDateFilters),
    numberFilters: parseNumberFiltersInput(nextFilters.numberFilters),
    dateFilters: parseDateFiltersInput(nextFilters.dateFilters),
    matchMode: nextFilters.matchMode === "or" ? "or" : undefined,
    sort: sortField ? { field: sortField, direction: nextFilters.sortDirection } : undefined,
    limit: nextFilters.limit ?? undefined,
    minRating: nextFilters.minRating ?? undefined,
  };
}

function hasSearchCriteria(request: SearchRequest) {
  return Boolean(
    request.query.trim() ||
    request.tag ||
    (request.tags?.length ?? 0) > 0 ||
    request.metadataKey ||
    (request.metadataFilters?.length ?? 0) > 0 ||
    (request.excludeTags?.length ?? 0) > 0 ||
    (request.excludeFormats?.length ?? 0) > 0 ||
    Boolean(request.excludeQuery?.trim()) ||
    (request.excludePathPrefixes?.length ?? 0) > 0 ||
    (request.excludeMetadataFilters?.length ?? 0) > 0 ||
    (request.excludeNumberFilters?.length ?? 0) > 0 ||
    (request.excludeDateFilters?.length ?? 0) > 0 ||
    (request.numberFilters?.length ?? 0) > 0 ||
    (request.dateFilters?.length ?? 0) > 0 ||
    (request.formats?.length ?? 0) > 0 ||
    request.sort != null ||
    request.limit != null ||
    request.minRating != null,
  );
}

export function setFilterBarOpen(open: boolean) {
  isFilterBarOpen.value = open;
}

export function toggleFilterBar() {
  isFilterBarOpen.value = !isFilterBarOpen.value;
}

function updateFilterList(
  key: "tags" | "formats" | "colors" | "shapes" | "excludeTags" | "excludeFormats",
  value: string,
  enabled: boolean,
) {
  const normalizedValue = value.trim();
  if (!normalizedValue) return;
  const current = filters.value[key];
  const next = enabled
    ? normalizeFilterValues([...current, normalizedValue])
    : current.filter((item) => item !== normalizedValue);
  filters.value = {
    ...filters.value,
    [key]: next,
  };
}

export function toggleFilterValue(key: "tags" | "formats" | "colors" | "shapes" | "excludeTags" | "excludeFormats", value: string) {
  const current = filters.value[key];
  updateFilterList(key, value, !current.includes(value));
}

export function setMinimumRatingFilter(value: number | null) {
  filters.value = {
    ...filters.value,
    minRating: value == null || value <= 0 ? null : value,
  };
}

export function updateFilters(patch: Partial<typeof filters.value>) {
  filters.value = {
    ...filters.value,
    ...patch,
  };
}

export function clearFilters() {
  filters.value = createInitialFilters();
}

export async function runSearch(request: SearchRequest) {
  const requestId = nextSearchRequestId();
  searchQuery.value = request.query;
  const filterRequest = buildSearchRequest(request.query);
  const repoId = hasActiveFilters.value ? activeRepoId.value ?? undefined : undefined;
  const normalizedRequest: SearchRequest = {
    ...request,
    repoId: request.repoId ?? repoId,
    tags: request.tags ?? filterRequest.tags,
    formats: request.formats ?? filterRequest.formats,
    metadataFilters: request.metadataFilters ?? filterRequest.metadataFilters,
    excludeTags: request.excludeTags ?? filterRequest.excludeTags,
    excludeFormats: request.excludeFormats ?? filterRequest.excludeFormats,
    excludeQuery: request.excludeQuery ?? filterRequest.excludeQuery,
    excludePathPrefixes: request.excludePathPrefixes ?? filterRequest.excludePathPrefixes,
    excludeMetadataFilters: request.excludeMetadataFilters ?? filterRequest.excludeMetadataFilters,
    excludeNumberFilters: request.excludeNumberFilters ?? filterRequest.excludeNumberFilters,
    excludeDateFilters: request.excludeDateFilters ?? filterRequest.excludeDateFilters,
    numberFilters: request.numberFilters ?? filterRequest.numberFilters,
    dateFilters: request.dateFilters ?? filterRequest.dateFilters,
    matchMode: request.matchMode ?? filterRequest.matchMode,
    sort: request.sort ?? filterRequest.sort,
    limit: request.limit ?? filterRequest.limit,
    minRating: request.minRating ?? filterRequest.minRating,
  };

  if (!hasSearchCriteria(normalizedRequest)) {
    searchResults.value = [];
    isSearching.value = false;
    return;
  }

  isSearching.value = true;
  error.value = null;

  try {
    const response = await searchAssets(normalizedRequest);
    if (requestId !== latestSearchRequestId) return;
    searchResults.value = response.results;
  } catch (cause) {
    if (requestId !== latestSearchRequestId) return;
    error.value = cause instanceof Error ? cause.message : String(cause);
  } finally {
    if (requestId === latestSearchRequestId) {
      isSearching.value = false;
    }
  }
}

export function runFilteredSearch() {
  if (!activeRepoId.value && hasActiveFilters.value) {
    nextSearchRequestId();
    searchResults.value = [];
    isSearching.value = false;
    return Promise.resolve();
  }
  return runSearch(buildSearchRequest());
}
