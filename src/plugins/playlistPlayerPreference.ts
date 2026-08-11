/** 播放器实现选择与官方回退策略。 */
import type { RegisteredPlaylistPlayer } from "./sdk";

const PLAYER_PREFERENCE_STORAGE_KEY = "momobako.playlistPlayerPreferences.v1";
export const AUDIO_PLAYER_CAPABILITY_ID = "momobako.player.audio";
export const OFFICIAL_AUDIO_PLAYER_PLUGIN_ID = "momobako.player.audio";

export type PlaylistPlayerResolution = {
  player: RegisteredPlaylistPlayer | null;
  capabilityId: string;
  preferredPluginId: string | null;
  fallbackUsed: boolean;
};

export function playerCapabilityId(player: RegisteredPlaylistPlayer) {
  return player.capabilityId?.trim() || defaultCapabilityForPlayerType(player.playerTypeId);
}

export function getStoredPlayerPreference(capabilityId: string): string | null {
  return readPreferences()[capabilityId.trim()] ?? null;
}

export function setStoredPlayerPreference(capabilityId: string, pluginId: string | null) {
  const normalizedCapabilityId = capabilityId.trim();
  if (!normalizedCapabilityId) throw new Error("播放器能力标识不能为空");
  const preferences = readPreferences();
  if (pluginId?.trim()) {
    preferences[normalizedCapabilityId] = pluginId.trim();
  } else {
    delete preferences[normalizedCapabilityId];
  }
  writePreferences(preferences);
}

/** 按显式选择、官方默认的顺序解析；音频不会被未选择的第三方实现接管。 */
export function resolvePlayer(
  playerTypeId: string,
  candidates: RegisteredPlaylistPlayer[],
): PlaylistPlayerResolution {
  const capabilityId = resolveCapabilityId(playerTypeId, candidates);
  const preferredPluginId = getStoredPlayerPreference(capabilityId);
  if (!candidates.length) {
    return { player: null, capabilityId, preferredPluginId, fallbackUsed: Boolean(preferredPluginId) };
  }
  const preferred = preferredPluginId
    ? candidates.find((player) => player.pluginId === preferredPluginId)
    : null;
  const officialPluginId = officialPlayerPluginId(capabilityId);
  const official = officialPluginId
    ? candidates.find((player) => player.pluginId === officialPluginId)
    : null;
  const player = preferred
    ?? official
    ?? (capabilityId === AUDIO_PLAYER_CAPABILITY_ID ? null : candidates[0])
    ?? null;
  return {
    player,
    capabilityId,
    preferredPluginId,
    fallbackUsed: Boolean(preferredPluginId && player?.pluginId !== preferredPluginId),
  };
}

function readPreferences(): Record<string, string> {
  if (typeof localStorage === "undefined") return {};
  try {
    const parsed = JSON.parse(localStorage.getItem(PLAYER_PREFERENCE_STORAGE_KEY) ?? "{}");
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return {};
    return Object.fromEntries(Object.entries(parsed).filter((entry): entry is [string, string] => (
      Boolean(entry[0].trim()) && typeof entry[1] === "string" && Boolean(entry[1].trim())
    )));
  } catch (error) {
    console.error("[playlist-player] failed to read player preferences", error);
    return {};
  }
}

function writePreferences(preferences: Record<string, string>) {
  if (typeof localStorage === "undefined") return;
  try {
    localStorage.setItem(PLAYER_PREFERENCE_STORAGE_KEY, JSON.stringify(preferences));
  } catch (error) {
    console.error("[playlist-player] failed to persist player preferences", error);
  }
}

function defaultCapabilityForPlayerType(playerTypeId: string) {
  return playerTypeId === "momobako.playlist.audio-sequence"
    ? AUDIO_PLAYER_CAPABILITY_ID
    : `playlist-player:${playerTypeId}`;
}

function resolveCapabilityId(playerTypeId: string, candidates: RegisteredPlaylistPlayer[]) {
  const stableDefault = defaultCapabilityForPlayerType(playerTypeId);
  return candidates.find((player) => playerCapabilityId(player) === stableDefault)
    ? stableDefault
    : candidates[0] ? playerCapabilityId(candidates[0]) : stableDefault;
}

function officialPlayerPluginId(capabilityId: string) {
  return capabilityId === AUDIO_PLAYER_CAPABILITY_ID ? OFFICIAL_AUDIO_PLAYER_PLUGIN_ID : null;
}
