import type { PlaylistItem } from "../types/repository";
import {
  getRegisteredPlaylistPlayerByType,
  listRegisteredPlaylistPlayers,
  type RegisteredPlaylistPlayer,
} from "./sdk";

export function listPlaylistPlayers() {
  return listRegisteredPlaylistPlayers();
}

export function getPlaylistPlayerByType(playerTypeId: string | null | undefined) {
  if (!playerTypeId) return null;
  return getRegisteredPlaylistPlayerByType(playerTypeId);
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
