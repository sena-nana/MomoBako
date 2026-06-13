const playlistStorageKey = "momobako.library.asmr.playlist.v1";

export function metadataText(entry, key) {
  const value = entry?.metadata?.[key];
  if (typeof value === "string") return value;
  if (typeof value === "number" || typeof value === "boolean") return String(value);
  return "";
}

export function metadataNumber(entry, key) {
  const value = entry?.metadata?.[key];
  if (typeof value === "number" && Number.isFinite(value)) return value;
  if (typeof value === "string" && value.trim() !== "") {
    const parsed = Number(value);
    return Number.isFinite(parsed) ? parsed : null;
  }
  return null;
}

export function matchAsmrEntry(entry) {
  const metadata = entry?.metadata ?? {};
  return metadata.libraryKind === "asmr" || Boolean(metadata.workId) || Boolean(metadata.rjCode);
}

export function listeningStatusLabel(status) {
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

export function isAsmrAudioEntry(entry) {
  const metadata = entry?.metadata ?? {};
  return entry?.kind === "file" && metadata.asmrEntryKind === "audio" && matchAsmrEntry(entry);
}

export function createPlaylistItem(repoId, entry) {
  if (!repoId || !isAsmrAudioEntry(entry)) return null;
  return {
    repoId,
    path: entry.path,
    title: metadataText(entry, "trackTitle") || entry.name,
    workTitle: metadataText(entry, "workTitle"),
    status: listeningStatusLabel(metadataText(entry, "listeningStatus")),
    workRoot: metadataText(entry, "workRoot") || undefined,
    trackPath: metadataText(entry, "trackPath") || undefined,
    assetId: entry.assetId ?? null,
  };
}

export function sortAudioEntries(entries) {
  return [...entries].sort((left, right) => (
    (metadataText(left, "trackPath") || left.path).localeCompare(
      metadataText(right, "trackPath") || right.path,
      "zh-CN",
      { numeric: true },
    )
  ));
}

export function workAudioEntries(entry, entries) {
  const workRoot = metadataText(entry, "workRoot");
  if (!workRoot) return isAsmrAudioEntry(entry) ? [entry] : [];
  return sortAudioEntries(entries.filter((item) => (
    isAsmrAudioEntry(item) && metadataText(item, "workRoot") === workRoot
  )));
}

function itemKey(item) {
  return `${item.repoId}\u0000${item.path}`;
}

function isStoredPlaylistItem(value) {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    && typeof value.path === "string" && value.path.trim().length > 0;
}

export function parseStoredPlaylist(raw) {
  if (!raw) return [];
  try {
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed
      .map((item) => {
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
      .filter(Boolean);
  } catch {
    return [];
  }
}

export function readStoredPlaylist() {
  try {
    return parseStoredPlaylist(localStorage.getItem(playlistStorageKey));
  } catch {
    return [];
  }
}

export function persistPlaylist(items) {
  try {
    localStorage.setItem(playlistStorageKey, JSON.stringify(items));
  } catch {
    return;
  }
}

export function mergePlaylistEntries(current, repoId, entries) {
  if (!repoId) return current;
  const next = [...current];
  const existing = new Set(next.map(itemKey));
  for (const entry of entries) {
    const item = createPlaylistItem(repoId, entry);
    if (!item || existing.has(itemKey(item))) continue;
    next.push(item);
    existing.add(itemKey(item));
  }
  return next;
}

export function resolveActivePlaylist(items, repoId, entriesByPath) {
  if (!repoId) return [];
  return items
    .filter((item) => item.repoId === repoId || !item.repoId)
    .map((item) => {
      const entry = entriesByPath.get(item.path);
      return entry ? createPlaylistItem(repoId, entry) ?? item : { ...item, repoId: item.repoId || repoId };
    });
}

export function hasPlaylistEntry(items, repoId, path) {
  return items.some((item) => (item.repoId === repoId || !item.repoId) && item.path === path);
}

export function clearPlaylistForRepo(items, repoId) {
  if (!repoId) return items;
  return items.filter((item) => item.repoId !== repoId && item.repoId);
}

export function fileSummary(entry) {
  if (!matchAsmrEntry(entry)) return null;
  const rjCode = metadataText(entry, "rjCode") || metadataText(entry, "workId");
  const workTitle = metadataText(entry, "workTitle") || metadataText(entry, "title");
  const trackTitle = metadataText(entry, "trackTitle");
  const listeningStatus = listeningStatusLabel(metadataText(entry, "listeningStatus"));
  const lyricStatus = metadataText(entry, "lyricStatus");
  const progress = metadataNumber(entry, "listeningProgress");
  const progressLabel = progress == null ? "" : `${Math.round(progress)}%`;
  const rows = [
    { label: "ASMR", value: rjCode || "作品" },
    { label: "作品", value: workTitle },
    { label: "音轨", value: trackTitle },
    { label: "收听", value: [listeningStatus, progressLabel].filter(Boolean).join(" · ") },
    { label: "歌词", value: lyricStatus },
  ].filter((row) => row.value);
  return {
    inline: [rjCode, workTitle, listeningStatus, progressLabel, lyricStatus ? "歌词" : ""].filter(Boolean).join(" · "),
    rows,
  };
}

export function buildListeningProgressMetadata(event, now = new Date()) {
  const durationMs = Math.max(0, Math.round(Number(event?.durationMs) || 0));
  const currentTimeMs = Math.max(0, Math.round(Number(event?.currentTimeMs) || 0));
  const progress = durationMs > 0 ? Math.min(100, Math.max(0, Math.round((currentTimeMs / durationMs) * 100))) : 0;
  const finished = event?.state === "ended" || (durationMs > 0 && progress >= 95);
  return {
    listeningProgress: finished ? 100 : progress,
    listeningStatus: finished ? "listened" : "listening",
    lastListenedAt: now.toISOString(),
    trackDurationMs: durationMs,
    trackPositionMs: finished ? durationMs : currentTimeMs,
  };
}

export function shouldPersistListeningProgress(event, previous = null, nowMs = Date.now()) {
  if (!isAsmrAudioEntry(event?.entry)) return false;
  if (event.state === "metadata") {
    const durationMs = Math.max(0, Math.round(Number(event.durationMs) || 0));
    return durationMs > 0 && event.entry.metadata?.trackDurationMs !== durationMs;
  }
  if (event.state === "ended") return true;
  if (event.state === "pause") return Math.max(0, Math.round(Number(event.currentTimeMs) || 0)) > 0;
  if (event.state !== "timeupdate") return false;
  const durationMs = Math.max(0, Math.round(Number(event.durationMs) || 0));
  const currentTimeMs = Math.max(0, Math.round(Number(event.currentTimeMs) || 0));
  if (durationMs <= 0 || currentTimeMs <= 0) return false;
  const currentSecond = Math.floor(currentTimeMs / 1000);
  return !previous
    || nowMs - previous.savedAtMs >= 15000
    || Math.abs(currentSecond - previous.savedSecond) >= 5;
}
