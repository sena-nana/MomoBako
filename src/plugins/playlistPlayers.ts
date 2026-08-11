import type { PlaylistItem } from "../types/repository";
import {
  AUDIO_PLAYER_CAPABILITY_ID,
  getPreferredPlaylistPlayerPluginId,
  getRegisteredPlaylistPlayerByType,
  listRegisteredPlaylistPlayerImplementations,
  listRegisteredPlaylistPlayers,
  resolveRegisteredPlaylistPlayerByType,
  setPreferredPlaylistPlayerPluginId,
  type RegisteredPlaylistPlayer,
} from "./sdk";

export const AUDIO_PLAYLIST_PLAYER_TYPE_ID = "momobako.playlist.audio-sequence";

export function listPlaylistPlayers() {
  return listRegisteredPlaylistPlayers();
}

export function getPlaylistPlayerByType(playerTypeId: string | null | undefined) {
  if (!playerTypeId) return null;
  return getRegisteredPlaylistPlayerByType(playerTypeId);
}

export function listAudioPlayerImplementations() {
  return listRegisteredPlaylistPlayerImplementations(AUDIO_PLAYER_CAPABILITY_ID);
}

export function getDefaultAudioPlayerPluginId() {
  return getPreferredPlaylistPlayerPluginId(AUDIO_PLAYER_CAPABILITY_ID);
}

export function setDefaultAudioPlayerPluginId(pluginId: string | null) {
  setPreferredPlaylistPlayerPluginId(AUDIO_PLAYER_CAPABILITY_ID, pluginId);
}

export function getAudioPlayerResolution() {
  return resolveRegisteredPlaylistPlayerByType(AUDIO_PLAYLIST_PLAYER_TYPE_ID);
}

export function findPlaylistPlayersForExtension(extension?: string | null): RegisteredPlaylistPlayer[] {
  const normalized = (extension ?? "").trim().toLowerCase();
  if (!normalized) return [];
  return listRegisteredPlaylistPlayers().filter((player) => player.supportedExtensions.includes(normalized));
}

export function playlistPlayerSupportsItem(
  player: Pick<RegisteredPlaylistPlayer, "supportedExtensions"> | null | undefined,
  item: Pick<PlaylistItem, "extension"> | null | undefined,
) {
  if (!player || !item?.extension) return false;
  return player.supportedExtensions.includes(item.extension.toLowerCase());
}
