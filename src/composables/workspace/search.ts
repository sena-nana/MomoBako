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

export function resetSearchState() {
  searchQuery.value = "";
  searchResults.value = [];
  filters.value = createInitialFilters();
  isFilterBarOpen.value = false;
}

function normalizeFilterValues(values: string[]) {
  return Array.from(new Set(values.map((value) => value.trim()).filter(Boolean)));
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

function parsePathPrefixesInput(value: string) {
  return value
    .split(/\n|[,，]/)
    .map((item) => item.trim().replace(/\\/g, "/").replace(/^\/+|\/+$/g, ""))
    .filter(Boolean);
}

export function buildSearchRequest(query = searchQuery.value): SearchRequest {
  const nextFilters = filters.value;
  const metadataFilters = [
    ...nextFilters.colors.map((value) => ({ key: "color", value })),
    ...nextFilters.shapes.map((value) => ({ key: "shape", value })),
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
    return;
  }

  isSearching.value = true;
  error.value = null;

  try {
    const response = await searchAssets(normalizedRequest);
    searchResults.value = response.results;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
  } finally {
    isSearching.value = false;
  }
}

export function runFilteredSearch() {
  if (!activeRepoId.value && hasActiveFilters.value) {
    searchResults.value = [];
    return Promise.resolve();
  }
  return runSearch(buildSearchRequest());
}
