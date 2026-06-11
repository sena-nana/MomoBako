export function splitListInput(value: string) {
  return Array.from(new Set(
    value
      .split(/[,，\n]/)
      .map((item) => item.trim())
      .filter(Boolean),
  ));
}

export function normalizeFilterValues(values: string[]) {
  return Array.from(new Set(values.map((value) => value.trim()).filter(Boolean)));
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

export function parsePathPrefixesInput(value: string) {
  return splitListInput(value)
    .map((item) => item.replace(/\\/g, "/").replace(/^\/+|\/+$/g, ""))
    .filter(Boolean);
}
