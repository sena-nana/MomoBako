import { computed, ref, watch } from "vue";
import type {
  FileBrowserEntry,
  PlaylistItem,
  PlaylistFileClass,
  PlaylistDetail,
  PlaylistPlaybackMode,
} from "../types/repository";
import { getPlaylistPlayerByType, listPlaylistPlayers } from "../plugins/playlistPlayers";
import type {
  PlaylistPlayerObjectFit,
  PlaylistPlayerController,
  PlaylistPlayerRuntimeApi,
  PlaylistPlayerRuntimeEvent,
  PlaylistPlayerRuntimeSettings,
} from "../plugins/sdk";

export type PlaylistPlayerSettings = {
  imageDurationMs: number;
  objectFit: PlaylistPlayerObjectFit;
};

type PlayerSession = {
  repoId: string;
  playlistId: string;
  playerTypeId: string;
  currentItemId: string;
  currentTimeMs: number;
  durationMs: number;
  mode: PlaylistPlaybackMode;
  volume: number;
  isPlaying: boolean;
};

type PlaybackQueueItem = PlaylistItem & {
  runtimePlayerTypeId: string;
  playerLabel: string;
  fileClass: PlaylistFileClass;
  transient?: boolean;
};

const storageKeyPrefix = "momobako.playbackSession";
const settingsStorageKey = "momobako.playbackSettings";
const defaultSettings: PlaylistPlayerSettings = {
  imageDurationMs: 5000,
  objectFit: "contain",
};
const activeRepoId = ref<string | null>(null);
const activePlaylist = ref<PlaylistDetail | null>(null);
const playbackQueue = ref<PlaybackQueueItem[]>([]);
const currentItemId = ref<string | null>(null);
const currentTimeMs = ref(0);
const durationMs = ref(0);
const mode = ref<PlaylistPlaybackMode>("listLoop");
const volume = ref(1);
const isPlaying = ref(false);
const errorMessage = ref<string | null>(null);
const canPlay = ref(false);
const queueOpen = ref(false);
const fallbackMountTarget = ref<HTMLElement | null>(null);
const visibleMountTarget = ref<HTMLElement | null>(null);
const runtime = ref<PlaylistPlayerRuntimeApi | null>(null);
const runtimeController = ref<PlaylistPlayerController | null>(null);
const runtimeMountTarget = ref<HTMLElement | null>(null);
const runtimePlayerTypeId = ref<string | null>(null);
const shuffledOrder = ref<string[]>([]);
const playbackHistory = ref<string[]>([]);
const playbackSettings = ref<PlaylistPlayerSettings>(readPlaybackSettings());

function storageKey(repoId: string) {
  return `${storageKeyPrefix}:${repoId}`;
}

function normalizeImageDurationMs(value: unknown) {
  if (typeof value !== "number" || !Number.isFinite(value)) return defaultSettings.imageDurationMs;
  return Math.min(30000, Math.max(2000, Math.round(value)));
}

function normalizeObjectFit(value: unknown): PlaylistPlayerObjectFit {
  return value === "cover" ? "cover" : "contain";
}

function readPlaybackSettings(): PlaylistPlayerSettings {
  try {
    const raw = localStorage.getItem(settingsStorageKey);
    if (!raw) return { ...defaultSettings };
    const parsed = JSON.parse(raw) as Partial<PlaylistPlayerSettings>;
    return {
      imageDurationMs: normalizeImageDurationMs(parsed.imageDurationMs),
      objectFit: normalizeObjectFit(parsed.objectFit),
    };
  } catch {
    return { ...defaultSettings };
  }
}

function persistPlaybackSettings(settings = playbackSettings.value) {
  try {
    localStorage.setItem(settingsStorageKey, JSON.stringify(settings));
  } catch {
    /* ignore */
  }
}

function runtimeSettings(): PlaylistPlayerRuntimeSettings {
  return {
    imageDurationMs: playbackSettings.value.imageDurationMs,
    objectFit: playbackSettings.value.objectFit,
  };
}

async function configureRuntime() {
  await runtime.value?.configure?.(runtimeSettings());
}

function activeMountTarget() {
  return visibleMountTarget.value ?? fallbackMountTarget.value;
}

function moveMountedRuntimeNode(target: HTMLElement | null) {
  const source = runtimeMountTarget.value;
  if (!target || !source) return false;
  if (target === source) return true;
  if (runtimeController.value) {
    runtimeController.value.mountTarget = target;
  }
  if (!source.childNodes.length) {
    runtimeMountTarget.value = target;
    return true;
  }
  target.replaceChildren(...Array.from(source.childNodes));
  runtimeMountTarget.value = target;
  return true;
}

function currentPlayerFileClass(): PlaylistFileClass | null {
  return currentPlayer()?.fileClass ?? activePlaylist.value?.playlist.fileClass ?? null;
}

function currentSession(): PlayerSession | null {
  if (!activeRepoId.value || !activePlaylist.value || !currentItemId.value) return null;
  const item = currentItem();
  if (!item || item.transient) return null;
  return {
    repoId: activeRepoId.value,
    playlistId: activePlaylist.value.playlist.playlistId,
    playerTypeId: activePlaylist.value.playlist.playerTypeId,
    currentItemId: currentItemId.value,
    currentTimeMs: currentTimeMs.value,
    durationMs: durationMs.value,
    mode: mode.value,
    volume: volume.value,
    isPlaying: isPlaying.value,
  };
}

function persistSession() {
  const session = currentSession();
  if (!session) return;
  try {
    localStorage.setItem(storageKey(session.repoId), JSON.stringify(session));
  } catch {
    /* ignore */
  }
}

function clearSession(repoId = activeRepoId.value) {
  if (!repoId) return;
  try {
    localStorage.removeItem(storageKey(repoId));
  } catch {
    /* ignore */
  }
}

function readSession(repoId: string): PlayerSession | null {
  try {
    const raw = localStorage.getItem(storageKey(repoId));
    if (!raw) return null;
    return JSON.parse(raw) as PlayerSession;
  } catch {
    return null;
  }
}

function queueItemFromPlaylistItem(item: PlaylistItem, playlist = activePlaylist.value): PlaybackQueueItem {
  const playerTypeId = playlist?.playlist.playerTypeId ?? "";
  return {
    ...item,
    runtimePlayerTypeId: playerTypeId,
    playerLabel: playlist?.playlist.playerLabel ?? "",
    fileClass: playlist?.playlist.fileClass ?? "",
  };
}

function findPlayerForEntry(entry: FileBrowserEntry) {
  const extension = (entry.extension ?? entry.name.split(".").pop() ?? "").toLowerCase();
  return listPlaylistPlayers().find((player) => (
    player.supportedExtensions.map((item) => item.toLowerCase()).includes(extension)
    && (player.fileClass === "audio" || player.fileClass === "video")
  )) ?? null;
}

function transientItemFromEntry(entry: FileBrowserEntry, playerTypeId: string): PlaybackQueueItem {
  const player = getPlaylistPlayerByType(playerTypeId);
  const now = new Date().toISOString();
  const extension = (entry.extension ?? entry.name.split(".").pop() ?? "").toLowerCase();
  return {
    playlistItemId: `transient:${entry.assetId ?? entry.path}:${Date.now()}`,
    playlistId: activePlaylist.value?.playlist.playlistId ?? "__transient__",
    assetId: entry.assetId ?? entry.path,
    path: entry.path,
    filename: entry.name,
    extension,
    thumbnailPath: entry.thumbnailPath ?? null,
    status: "ready",
    statusReason: null,
    sortOrder: -1,
    addedAt: now,
    isVirtual: entry.isVirtual,
    providerId: entry.providerId,
    providerItemId: entry.providerItemId,
    sourcePayload: entry.sourcePayload,
    localAbsolutePath: entry.localAbsolutePath,
    runtimePlayerTypeId: playerTypeId,
    playerLabel: player?.label ?? "",
    fileClass: player?.fileClass ?? "",
    transient: true,
  };
}

function syncQueueFromActivePlaylist() {
  const persistentItems = (activePlaylist.value?.items ?? []).map((item) => queueItemFromPlaylistItem(item));
  const transientItems = playbackQueue.value.filter((item) => item.transient);
  playbackQueue.value = [...persistentItems, ...transientItems.filter((item) => (
    currentItemId.value === item.playlistItemId || playbackHistory.value.includes(item.playlistItemId)
  ))];
}

function pruneInactiveTransientItems() {
  const currentId = currentItemId.value;
  const removedIds = new Set(playbackQueue.value
    .filter((item) => item.transient && item.playlistItemId !== currentId)
    .map((item) => item.playlistItemId));
  if (!removedIds.size) return;
  playbackQueue.value = playbackQueue.value.filter((item) => !removedIds.has(item.playlistItemId));
  playbackHistory.value = playbackHistory.value.filter((itemId) => !removedIds.has(itemId));
  shuffledOrder.value = shuffledOrder.value.filter((itemId) => !removedIds.has(itemId));
}

function readyItems() {
  return playbackQueue.value.filter((item) => item.status === "ready");
}

function currentItem() {
  return playbackQueue.value.find((item) => item.playlistItemId === currentItemId.value) ?? null;
}

function ensureShuffleOrder() {
  const items = readyItems().map((item) => item.playlistItemId);
  if (!items.length) {
    shuffledOrder.value = [];
    return;
  }
  const current = currentItemId.value;
  const rest = items.filter((itemId) => itemId !== current);
  for (let index = rest.length - 1; index > 0; index -= 1) {
    const nextIndex = Math.floor(Math.random() * (index + 1));
    [rest[index], rest[nextIndex]] = [rest[nextIndex], rest[index]];
  }
  shuffledOrder.value = current ? [current, ...rest] : rest;
}

async function disposeRuntime() {
  if (!runtime.value) return;
  await runtime.value.dispose?.();
  runtime.value = null;
  runtimeController.value = null;
  runtimeMountTarget.value = null;
  runtimePlayerTypeId.value = null;
}

function setError(message: string | null) {
  errorMessage.value = message;
  persistSession();
}

function currentPlayer() {
  return getPlaylistPlayerByType(currentItem()?.runtimePlayerTypeId ?? activePlaylist.value?.playlist.playerTypeId);
}

function handleRuntimeEvent(event: PlaylistPlayerRuntimeEvent) {
  if (event.type === "state") {
    if (event.canPlay !== undefined) canPlay.value = event.canPlay;
    if (event.isPlaying !== undefined) isPlaying.value = event.isPlaying;
    persistSession();
    return;
  }
  if (event.type === "time") {
    currentTimeMs.value = event.currentTimeMs;
    if (event.durationMs !== undefined) durationMs.value = event.durationMs;
    persistSession();
    return;
  }
  if (event.type === "error") {
    setError(event.message);
    isPlaying.value = false;
    return;
  }
  if (event.type === "ended") {
    void playNext(true);
  }
}

async function ensureRuntimeLoaded() {
  const player = currentPlayer();
  const target = activeMountTarget();
  const playerTypeId = currentItem()?.runtimePlayerTypeId ?? activePlaylist.value?.playlist.playerTypeId ?? null;
  if (!player || !target || !activeRepoId.value) {
    canPlay.value = false;
    await disposeRuntime();
    return false;
  }
  if (runtime.value && runtimePlayerTypeId.value !== playerTypeId) {
    await disposeRuntime();
  }
  if (!runtime.value) {
    const controller = {
      mountTarget: target,
      repoId: activeRepoId.value,
      onEvent: handleRuntimeEvent,
    };
    runtimeController.value = controller;
    runtime.value = await player.createRuntime(controller);
    runtimeMountTarget.value = target;
    runtimePlayerTypeId.value = playerTypeId;
    await configureRuntime();
  } else {
    moveMountedRuntimeNode(target);
  }
  return true;
}

async function loadCurrentItem(autoPlay = false) {
  const item = currentItem();
  const player = currentPlayer();
  if (!item || !player) {
    canPlay.value = false;
    setError(player ? "当前没有可播放条目" : "缺少对应播放插件");
    return;
  }
  if (item.status !== "ready") {
    canPlay.value = false;
    setError(item.statusReason ?? "当前条目不可播放");
    return;
  }
  const ready = await ensureRuntimeLoaded();
  if (!ready || !runtime.value) {
    setError("缺少对应播放插件");
    return;
  }
  setError(null);
  await configureRuntime();
  await runtime.value.load(item);
  if (player.supportsVolume) {
    await runtime.value.setVolume?.(volume.value);
  }
  if (player.supportsSeek && currentTimeMs.value > 0) {
    await runtime.value.seek?.(currentTimeMs.value);
  }
  if (autoPlay || isPlaying.value) {
    await runtime.value.play();
    isPlaying.value = true;
  }
  persistSession();
}

async function playItem(playlistItemId: string, autoPlay = true) {
  currentItemId.value = playlistItemId;
  currentTimeMs.value = 0;
  durationMs.value = 0;
  if (playbackHistory.value[playbackHistory.value.length - 1] !== playlistItemId) {
    playbackHistory.value = [...playbackHistory.value, playlistItemId];
  }
  await loadCurrentItem(autoPlay);
}

async function playEntry(repoId: string, entry: FileBrowserEntry) {
  const player = findPlayerForEntry(entry);
  if (!player) {
    setError("没有可用于播放此媒体的插件");
    return false;
  }
  if (activeRepoId.value && activeRepoId.value !== repoId) {
    await stop(false);
    activePlaylist.value = null;
    currentItemId.value = null;
    playbackQueue.value = [];
    playbackHistory.value = [];
    shuffledOrder.value = [];
  }
  activeRepoId.value = repoId;
  const item = transientItemFromEntry(entry, player.playerTypeId);
  const currentId = currentItemId.value;
  const withoutSamePath = playbackQueue.value.filter((queueItem) => (
    !queueItem.transient || queueItem.path !== entry.path
  ));
  const currentIndex = currentId
    ? withoutSamePath.findIndex((queueItem) => queueItem.playlistItemId === currentId)
    : -1;
  if (currentIndex >= 0) {
    withoutSamePath.splice(currentIndex + 1, 0, item);
  } else {
    withoutSamePath.push(item);
  }
  playbackQueue.value = withoutSamePath;
  if (mode.value === "shuffle" && currentId) {
    const order = shuffledOrder.value.filter((itemId) => (
      playbackQueue.value.some((queueItem) => queueItem.playlistItemId === itemId)
    ));
    const orderIndex = order.findIndex((itemId) => itemId === currentId);
    if (orderIndex >= 0) {
      order.splice(orderIndex + 1, 0, item.playlistItemId);
      shuffledOrder.value = order;
    }
  }
  await playItem(item.playlistItemId, true);
  return true;
}

function nextReadyItemId(naturalEnd = false) {
  const items = readyItems();
  if (!items.length) return null;
  const currentId = currentItemId.value;
  if (mode.value === "singleLoop" && naturalEnd && currentId) {
    return currentId;
  }
  if (mode.value === "shuffle") {
    if (!shuffledOrder.value.length || !shuffledOrder.value.includes(currentId ?? "")) {
      ensureShuffleOrder();
    }
    const currentIndex = shuffledOrder.value.findIndex((itemId) => itemId === currentId);
    const nextItemId = shuffledOrder.value[currentIndex + 1];
    if (nextItemId) return nextItemId;
    ensureShuffleOrder();
    return shuffledOrder.value[0] ?? null;
  }
  const currentIndex = items.findIndex((item) => item.playlistItemId === currentId);
  if (currentIndex < 0) return items[0]?.playlistItemId ?? null;
  const nextItem = items[currentIndex + 1];
  if (nextItem) return nextItem.playlistItemId;
  return mode.value === "listLoop" ? items[0]?.playlistItemId ?? null : null;
}

function previousReadyItemId() {
  if (mode.value === "shuffle" && playbackHistory.value.length > 1) {
    return playbackHistory.value[playbackHistory.value.length - 2] ?? null;
  }
  const items = readyItems();
  const currentIndex = items.findIndex((item) => item.playlistItemId === currentItemId.value);
  if (currentIndex > 0) return items[currentIndex - 1]?.playlistItemId ?? null;
  return items[items.length - 1]?.playlistItemId ?? null;
}

async function playNext(naturalEnd = false) {
  const previousItem = currentItem();
  const nextItemId = nextReadyItemId(naturalEnd);
  if (!nextItemId) {
    isPlaying.value = false;
    if (naturalEnd) pruneInactiveTransientItems();
    persistSession();
    return;
  }
  await playItem(nextItemId, true);
  if (naturalEnd || previousItem?.transient) {
    pruneInactiveTransientItems();
  }
}

async function playPrevious() {
  const previousItemId = previousReadyItemId();
  if (!previousItemId) return;
  playbackHistory.value = playbackHistory.value.slice(0, -1);
  await playItem(previousItemId, true);
}

async function setPlaybackState(next: Partial<Pick<PlayerSession, "currentItemId" | "currentTimeMs" | "durationMs" | "mode" | "volume" | "isPlaying">>) {
  const previousMode = mode.value;
  if (next.mode !== undefined) mode.value = next.mode;
  if (next.currentItemId !== undefined && next.currentItemId !== currentItemId.value) {
    await playItem(next.currentItemId, next.isPlaying ?? isPlaying.value);
    return;
  }
  if (next.currentTimeMs !== undefined) {
    currentTimeMs.value = next.currentTimeMs;
    await runtime.value?.seek?.(next.currentTimeMs);
  }
  if (next.durationMs !== undefined) durationMs.value = next.durationMs;
  if (next.volume !== undefined) {
    volume.value = next.volume;
    await runtime.value?.setVolume?.(next.volume);
  }
  if (next.isPlaying !== undefined) {
    isPlaying.value = next.isPlaying;
    if (next.isPlaying) {
      if (runtime.value && canPlay.value && currentItem()) {
        await runtime.value.play();
      } else {
        await loadCurrentItem(true);
      }
    } else {
      await runtime.value?.pause();
    }
  }
  if (next.mode !== undefined && next.mode !== previousMode && next.mode === "shuffle") {
    ensureShuffleOrder();
  }
  persistSession();
}

async function updatePlaybackSettings(next: Partial<PlaylistPlayerSettings>) {
  playbackSettings.value = {
    imageDurationMs: normalizeImageDurationMs(next.imageDurationMs ?? playbackSettings.value.imageDurationMs),
    objectFit: normalizeObjectFit(next.objectFit ?? playbackSettings.value.objectFit),
  };
  persistPlaybackSettings();
  await configureRuntime();
  if (currentPlayerFileClass() === "image") {
    durationMs.value = playbackSettings.value.imageDurationMs;
    if (currentTimeMs.value > durationMs.value) {
      currentTimeMs.value = durationMs.value;
    }
    persistSession();
  }
}

async function stop(clearStoredSession = true) {
  isPlaying.value = false;
  currentTimeMs.value = 0;
  durationMs.value = 0;
  canPlay.value = false;
  queueOpen.value = false;
  playbackHistory.value = [];
  shuffledOrder.value = [];
  playbackQueue.value = activePlaylist.value
    ? activePlaylist.value.items.map((item) => queueItemFromPlaylistItem(item))
    : [];
  setError(null);
  await runtime.value?.pause();
  await disposeRuntime();
  if (clearStoredSession) {
    clearSession();
  }
}

async function setActivePlaylist(repoId: string, playlist: PlaylistDetail | null, startItemId?: string | null, options: { autoPlay?: boolean; restore?: boolean } = {}) {
  if (activeRepoId.value && activeRepoId.value !== repoId) {
    await stop();
  }
  activeRepoId.value = repoId;
  activePlaylist.value = playlist;
  playbackHistory.value = [];
  shuffledOrder.value = [];
  playbackQueue.value = playlist ? playlist.items.map((item) => queueItemFromPlaylistItem(item, playlist)) : [];
  if (!playlist) {
    currentItemId.value = null;
    return;
  }

  const session = options.restore ? readSession(repoId) : null;
  currentItemId.value = startItemId
    ?? session?.currentItemId
    ?? playlist.items.find((item) => item.status === "ready")?.playlistItemId
    ?? playlist.items[0]?.playlistItemId
    ?? null;
  currentTimeMs.value = session?.currentTimeMs ?? 0;
  durationMs.value = session?.durationMs ?? 0;
  mode.value = session?.mode ?? mode.value;
  volume.value = session?.volume ?? volume.value;
  isPlaying.value = session?.isPlaying ?? Boolean(options.autoPlay);
  ensureShuffleOrder();
  await loadCurrentItem(Boolean(options.autoPlay || session?.isPlaying));
}

async function restoreSession(repoId: string, playlist: PlaylistDetail | null) {
  if (!playlist) {
    clearSession(repoId);
    return false;
  }
  const session = readSession(repoId);
  if (!session) return false;
  const player = getPlaylistPlayerByType(session.playerTypeId);
  const item = playlist.items.find((entry) => entry.playlistItemId === session.currentItemId);
  if (!player || !item || item.status !== "ready" || playlist.playlist.playlistId !== session.playlistId) {
    clearSession(repoId);
    return false;
  }
  await setActivePlaylist(repoId, playlist, session.currentItemId, { restore: true });
  return true;
}

async function moveRuntimeToActiveMountTarget() {
  if (!runtime.value || !activeRepoId.value || !currentItemId.value) return;
  const target = activeMountTarget();
  if (!target) return;
  if (moveMountedRuntimeNode(target)) return;
  const wasPlaying = isPlaying.value;
  const resumeTimeMs = currentTimeMs.value;
  await disposeRuntime();
  const ready = await ensureRuntimeLoaded();
  if (!ready || !runtime.value) return;
  const item = currentItem();
  if (!item || item.status !== "ready") return;
  await configureRuntime();
  await runtime.value.load(item);
  if (currentPlayer()?.supportsVolume) {
    await runtime.value.setVolume?.(volume.value);
  }
  if (currentPlayer()?.supportsSeek || currentPlayerFileClass() === "image") {
    await runtime.value.seek?.(resumeTimeMs);
  }
  if (wasPlaying) {
    await runtime.value.play();
  }
}

function attachMountTarget(element: HTMLElement | null) {
  fallbackMountTarget.value = element;
  void moveRuntimeToActiveMountTarget();
}

function attachVisibleMountTarget(element: HTMLElement | null) {
  visibleMountTarget.value = element;
  void moveRuntimeToActiveMountTarget();
}

function resetPlayerState() {
  activeRepoId.value = null;
  activePlaylist.value = null;
  playbackQueue.value = [];
  currentItemId.value = null;
  currentTimeMs.value = 0;
  durationMs.value = 0;
  mode.value = "listLoop";
  volume.value = 1;
  isPlaying.value = false;
  errorMessage.value = null;
  canPlay.value = false;
  queueOpen.value = false;
  fallbackMountTarget.value = null;
  visibleMountTarget.value = null;
  runtime.value = null;
  runtimeController.value = null;
  runtimeMountTarget.value = null;
  runtimePlayerTypeId.value = null;
  shuffledOrder.value = [];
  playbackHistory.value = [];
  playbackSettings.value = readPlaybackSettings();
}

watch(
  () => activePlaylist.value?.items.map((item) => `${item.playlistItemId}:${item.status}`).join("|") ?? "",
  () => {
    syncQueueFromActivePlaylist();
    if (mode.value === "shuffle") ensureShuffleOrder();
  },
);

export function usePlaylistPlayer() {
  return {
    activeRepoId: computed(() => activeRepoId.value),
    activePlaylist: computed(() => activePlaylist.value),
    queueItems: computed(() => playbackQueue.value),
    currentItem: computed(() => currentItem()),
    currentItemId: computed(() => currentItemId.value),
    currentTimeMs: computed(() => currentTimeMs.value),
    durationMs: computed(() => durationMs.value),
    mode: computed(() => mode.value),
    volume: computed(() => volume.value),
    playbackSettings: computed(() => playbackSettings.value),
    activeFileClass: computed(() => currentPlayerFileClass()),
    currentPlayerLabel: computed(() => currentItem()?.playerLabel ?? activePlaylist.value?.playlist.playerLabel ?? null),
    currentPlayerDefinition: computed(() => currentPlayer()),
    isPlaying: computed(() => isPlaying.value),
    canPlay: computed(() => canPlay.value),
    errorMessage: computed(() => errorMessage.value),
    queueOpen: computed(() => queueOpen.value),
    attachMountTarget,
    attachVisibleMountTarget,
    setQueueOpen(value: boolean) {
      queueOpen.value = value;
    },
    setActivePlaylist,
    restoreSession,
    setPlaybackState,
    updatePlaybackSettings,
    setError,
    playItem,
    playEntry,
    playPrevious,
    playNext,
    stop,
    clearSession,
    persistSession,
    resetPlayerState,
  };
}
