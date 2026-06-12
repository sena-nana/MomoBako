import type { FileBrowserEntry } from "../../types/repository";

export type AsmrPlaylistItem = {
  repoId: string;
  path: string;
  title: string;
  workTitle: string;
  status: string;
  workRoot?: string;
  trackPath?: string;
  assetId?: string | null;
};

export function asmrMetadataText(entry: FileBrowserEntry, key: string) {
  const value = entry.metadata?.[key];
  if (typeof value === "string") return value;
  if (typeof value === "number" || typeof value === "boolean") return String(value);
  return "";
}

export function listeningStatusLabel(status: string) {
  switch (status) {
    case "unlistened":
      return "未收听";
    case "listening":
      return "收听中";
    case "listened":
      return "已听完";
    default:
      return status;
  }
}

export function isAsmrAudioEntry(entry: FileBrowserEntry) {
  const metadata = entry.metadata ?? {};
  return entry.kind === "file" && metadata.asmrEntryKind === "audio" && (
    metadata.libraryKind === "asmr" || Boolean(metadata.workId) || Boolean(metadata.rjCode)
  );
}

export function createAsmrPlaylistItem(repoId: string, entry: FileBrowserEntry): AsmrPlaylistItem | null {
  if (!repoId || !isAsmrAudioEntry(entry)) return null;
  return {
    repoId,
    path: entry.path,
    title: asmrMetadataText(entry, "trackTitle") || entry.name,
    workTitle: asmrMetadataText(entry, "workTitle"),
    status: listeningStatusLabel(asmrMetadataText(entry, "listeningStatus")),
    workRoot: asmrMetadataText(entry, "workRoot") || undefined,
    trackPath: asmrMetadataText(entry, "trackPath") || undefined,
    assetId: entry.assetId ?? null,
  };
}

export function sortAsmrAudioEntries(entries: FileBrowserEntry[]) {
  return [...entries].sort((left, right) => (
    (asmrMetadataText(left, "trackPath") || left.path).localeCompare(
      asmrMetadataText(right, "trackPath") || right.path,
      "zh-CN",
      { numeric: true },
    )
  ));
}

export function asmrWorkAudioEntries(entry: FileBrowserEntry, entries: FileBrowserEntry[]) {
  const workRoot = asmrMetadataText(entry, "workRoot");
  if (!workRoot) return isAsmrAudioEntry(entry) ? [entry] : [];
  return sortAsmrAudioEntries(entries.filter((item) => (
    isAsmrAudioEntry(item) && asmrMetadataText(item, "workRoot") === workRoot
  )));
}

function itemKey(item: Pick<AsmrPlaylistItem, "repoId" | "path">) {
  return `${item.repoId}\u0000${item.path}`;
}

function isStoredPlaylistItem(value: unknown): value is AsmrPlaylistItem {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return false;
  const item = value as Record<string, unknown>;
  return typeof item.path === "string" && item.path.trim().length > 0;
}

export function parseStoredAsmrPlaylist(raw: string | null): AsmrPlaylistItem[] {
  if (!raw) return [];
  try {
    const parsed = JSON.parse(raw) as unknown;
    if (!Array.isArray(parsed)) return [];
    return parsed
      .map((item): AsmrPlaylistItem | null => {
        if (typeof item === "string" && item.trim()) {
          return {
            repoId: "",
            path: item,
            title: item.split(/[\\/]/).pop() || item,
            workTitle: "",
            status: "",
          };
        }
        if (!isStoredPlaylistItem(item)) return null;
        return {
          repoId: typeof item.repoId === "string" ? item.repoId : "",
          path: item.path,
          title: typeof item.title === "string" && item.title ? item.title : item.path.split(/[\\/]/).pop() || item.path,
          workTitle: typeof item.workTitle === "string" ? item.workTitle : "",
          status: typeof item.status === "string" ? item.status : "",
          workRoot: typeof item.workRoot === "string" ? item.workRoot : undefined,
          trackPath: typeof item.trackPath === "string" ? item.trackPath : undefined,
          assetId: typeof item.assetId === "string" ? item.assetId : null,
        };
      })
      .filter((item): item is AsmrPlaylistItem => Boolean(item));
  } catch {
    return [];
  }
}

export function mergeAsmrPlaylistEntries(
  current: AsmrPlaylistItem[],
  repoId: string,
  entries: FileBrowserEntry[],
) {
  if (!repoId) return current;
  const next = [...current];
  const existing = new Set(next.map(itemKey));
  for (const entry of entries) {
    const item = createAsmrPlaylistItem(repoId, entry);
    if (!item || existing.has(itemKey(item))) continue;
    next.push(item);
    existing.add(itemKey(item));
  }
  return next;
}

export function resolveActiveAsmrPlaylist(
  items: AsmrPlaylistItem[],
  repoId: string | null,
  entriesByPath: ReadonlyMap<string, FileBrowserEntry>,
) {
  if (!repoId) return [];
  return items
    .filter((item) => item.repoId === repoId || !item.repoId)
    .map((item) => {
      const entry = entriesByPath.get(item.path);
      return entry ? createAsmrPlaylistItem(repoId, entry) ?? item : { ...item, repoId: item.repoId || repoId };
    });
}

export function hasAsmrPlaylistEntry(items: AsmrPlaylistItem[], repoId: string, path: string) {
  return items.some((item) => (item.repoId === repoId || !item.repoId) && item.path === path);
}

export function clearAsmrPlaylistForRepo(items: AsmrPlaylistItem[], repoId: string | null) {
  if (!repoId) return items;
  return items.filter((item) => item.repoId !== repoId && item.repoId);
}
