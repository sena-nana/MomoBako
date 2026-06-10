import type { MetadataTagGroup } from "../types/repository";

const hexColorPattern = /^#[0-9A-F]{6}$/i;

export function metadataString(metadata: Record<string, unknown> | undefined, key: string) {
  const value = metadata?.[key];
  return typeof value === "string" ? value : "";
}

export function metadataNumber(metadata: Record<string, unknown> | undefined, key: string) {
  const value = metadata?.[key];
  if (typeof value !== "number" || !Number.isFinite(value)) return 0;
  return Math.min(Math.max(Math.round(value), 0), 5);
}

export function metadataPalette(metadata: Record<string, unknown> | undefined) {
  const value = metadata?.thumbnailPalette;
  if (!Array.isArray(value)) return [];
  return value
    .filter((item): item is string => typeof item === "string" && hexColorPattern.test(item))
    .slice(0, 5);
}

export function metadataTagGroups(metadata: Record<string, unknown> | undefined): MetadataTagGroup[] {
  const value = metadata?.tagGroups;
  if (!Array.isArray(value)) return [];

  const tags = value.flatMap((item) => {
    if (typeof item === "string") {
      return item.trim() ? [item.trim()] : [];
    }
    if (!item || typeof item !== "object") return [];

    const legacyItem = item as { label?: unknown; tags?: unknown };
    const nestedTags = Array.isArray(legacyItem.tags)
      ? legacyItem.tags
          .filter((tag): tag is string => typeof tag === "string")
          .map((tag) => tag.trim())
          .filter(Boolean)
      : [];
    if (nestedTags.length) return nestedTags;

    const label = typeof legacyItem.label === "string" ? legacyItem.label.trim() : "";
    return label ? [label] : [];
  });

  return Array.from(new Set(tags));
}

export function formatMetadataDate(value: string) {
  if (!value) return "未记录";
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString("zh-CN");
}
