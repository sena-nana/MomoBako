/** 从通用元数据读取音频展示信息，不依赖具体来源的 payload。 */

export function displayNameWithoutExtension(name, extension) {
  const normalizedName = typeof name === "string" ? name.trim() : "";
  const normalizedExtension = typeof extension === "string" ? extension.trim() : "";
  if (!normalizedName || !normalizedExtension) return normalizedName;
  const suffix = `.${normalizedExtension}`;
  return normalizedName.toLowerCase().endsWith(suffix.toLowerCase())
    ? normalizedName.slice(0, -suffix.length)
    : normalizedName;
}

export function audioDisplayMetadata(entry) {
  const metadata = entry?.metadata ?? {};
  const artists = Array.isArray(metadata.artists)
    ? metadata.artists.filter((value) => typeof value === "string" && value.trim()).join(" / ")
    : stringValue(metadata.artist);
  return {
    title: stringValue(metadata.title) || displayNameWithoutExtension(entry?.name ?? entry?.filename ?? "", entry?.extension),
    artist: artists,
    album: stringValue(metadata.album),
    coverArt: stringValue(metadata.coverArt),
  };
}

export function resolveArtworkUrl(path, fileSrc) {
  if (!path) return "";
  if (/^https?:\/\//i.test(path)) return path;
  return fileSrc(path);
}

function stringValue(value) {
  return typeof value === "string" && value.trim() ? value.trim() : "";
}
