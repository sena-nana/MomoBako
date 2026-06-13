export function splitListInput(value: string) {
  return Array.from(new Set(
    value
      .split(/[,，\n]/)
      .map((item) => item.trim())
      .filter(Boolean),
  ));
}

export function joinListInput(values?: string[]) {
  return values?.join("，") ?? "";
}

export function normalizeFilterValues(values: string[]) {
  return Array.from(new Set(values.map((value) => value.trim()).filter(Boolean)));
}

export function formatMetadataFiltersInput(filter: { metadataFilters?: Array<{ key: string; value: string }> }) {
  return filter.metadataFilters?.map((item) => `${item.key}=${item.value}`).join("\n") ?? "";
}

export function formatExcludeMetadataFiltersInput(filter: { excludeMetadataFilters?: Array<{ key: string; value: string }> }) {
  return filter.excludeMetadataFilters?.map((item) => `${item.key}=${item.value}`).join("\n") ?? "";
}

export function parseMetadataFiltersInput(value: string) {
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

export function parseNumberFiltersInput(value: string) {
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
      return [({
        key: key.trim(),
        min,
        max,
      })].filter((filter) => filter.min != null || filter.max != null);
    });
}

export function formatNumberFiltersInput(filter: { numberFilters?: Array<{ key: string; min?: number; max?: number }> }) {
  return filter.numberFilters?.map((item) => `${item.key}=${item.min ?? ""}..${item.max ?? ""}`).join("\n") ?? "";
}

export function formatExcludeNumberFiltersInput(filter: { excludeNumberFilters?: Array<{ key: string; min?: number; max?: number }> }) {
  return filter.excludeNumberFilters?.map((item) => `${item.key}=${item.min ?? ""}..${item.max ?? ""}`).join("\n") ?? "";
}

export function parseDateFiltersInput(value: string) {
  return value
    .split(/\n|[,，]/)
    .map((item) => item.trim())
    .filter(Boolean)
    .flatMap((item) => {
      const [key, range] = item.split("=");
      if (!key?.trim() || !range?.trim()) return [];
      const [from, to] = range.split("..").map((part) => part.trim());
      return [({
        key: key.trim(),
        from: from || undefined,
        to: to || undefined,
      })].filter((filter) => filter.from || filter.to);
    });
}

export function formatDateFiltersInput(filter: { dateFilters?: Array<{ key: string; from?: string; to?: string }> }) {
  return filter.dateFilters?.map((item) => `${item.key}=${item.from ?? ""}..${item.to ?? ""}`).join("\n") ?? "";
}

export function formatExcludeDateFiltersInput(filter: { excludeDateFilters?: Array<{ key: string; from?: string; to?: string }> }) {
  return filter.excludeDateFilters?.map((item) => `${item.key}=${item.from ?? ""}..${item.to ?? ""}`).join("\n") ?? "";
}

export function parsePathPrefixesInput(value: string) {
  return splitListInput(value)
    .map((item) => item.replace(/\\/g, "/").replace(/^\/+|\/+$/g, ""))
    .filter(Boolean);
}
