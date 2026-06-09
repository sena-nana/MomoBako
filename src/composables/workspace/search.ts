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

export function buildSearchRequest(query = searchQuery.value): SearchRequest {
  const nextFilters = filters.value;
  const metadataFilters = [
    ...nextFilters.colors.map((value) => ({ key: "color", value })),
    ...nextFilters.shapes.map((value) => ({ key: "shape", value })),
  ];

  return {
    query,
    repoId: hasActiveFilters.value ? activeRepoId.value ?? undefined : undefined,
    tags: normalizeFilterValues(nextFilters.tags),
    formats: normalizeFilterValues(nextFilters.formats),
    metadataFilters,
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
    (request.formats?.length ?? 0) > 0 ||
    request.minRating != null,
  );
}

export function setFilterBarOpen(open: boolean) {
  isFilterBarOpen.value = open;
}

export function toggleFilterBar() {
  isFilterBarOpen.value = !isFilterBarOpen.value;
}

function updateFilterList(key: "tags" | "formats" | "colors" | "shapes", value: string, enabled: boolean) {
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

export function toggleFilterValue(key: "tags" | "formats" | "colors" | "shapes", value: string) {
  const current = filters.value[key];
  updateFilterList(key, value, !current.includes(value));
}

export function setMinimumRatingFilter(value: number | null) {
  filters.value = {
    ...filters.value,
    minRating: value == null || value <= 0 ? null : value,
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
