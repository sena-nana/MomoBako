import type { Ref } from "vue";

type WorkspaceFilterShortcutOptions = {
  dateFiltersInput: Ref<string>;
  excludeDateFiltersInput: Ref<string>;
  excludeFormatsInput: Ref<string>;
  excludeMetadataFiltersInput: Ref<string>;
  excludeNumberFiltersInput: Ref<string>;
  excludePathPrefixesInput: Ref<string>;
  excludeQueryInput: Ref<string>;
  excludeTagsInput: Ref<string>;
  isRepositoryWritable: Ref<boolean>;
  limitInput: Ref<string>;
  metadataFiltersInput: Ref<string>;
  numberFiltersInput: Ref<string>;
  sortDirectionInput: Ref<"asc" | "desc">;
  sortFieldInput: Ref<string>;
  runFilteredSearch: () => void | Promise<unknown>;
  setActivePanel: (panel: "search") => void;
  setFilterBarOpen: (open: boolean) => void;
  updateFilters: (patch: {
    metadataFilters: string;
    excludeQuery: string;
    excludePathPrefixes: string;
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

export function useWorkspaceFilterShortcuts(options: WorkspaceFilterShortcutOptions) {
  function applyMetadataFilterShortcut(metadataFilters: string, sortField = "", sortDirection: "asc" | "desc" = "asc") {
    if (!options.isRepositoryWritable.value) return;
    options.metadataFiltersInput.value = metadataFilters;
    options.excludeTagsInput.value = "";
    options.excludeFormatsInput.value = "";
    options.limitInput.value = "";
    options.sortFieldInput.value = sortField;
    options.sortDirectionInput.value = sortDirection;
    options.updateFilters({
      metadataFilters,
      excludeQuery: options.excludeQueryInput.value.trim(),
      excludePathPrefixes: options.excludePathPrefixesInput.value.trim(),
      excludeTags: [],
      excludeFormats: [],
      excludeMetadataFilters: options.excludeMetadataFiltersInput.value.trim(),
      excludeNumberFilters: options.excludeNumberFiltersInput.value.trim(),
      excludeDateFilters: options.excludeDateFiltersInput.value.trim(),
      numberFilters: options.numberFiltersInput.value.trim(),
      dateFilters: options.dateFiltersInput.value.trim(),
      sortField,
      sortDirection,
      limit: null,
    });
    options.setActivePanel("search");
    void options.runFilteredSearch();
  }

  function closeFilterBar() {
    options.setFilterBarOpen(false);
  }

  return {
    applyMetadataFilterShortcut,
    closeFilterBar,
  };
}
