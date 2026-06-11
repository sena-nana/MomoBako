export function trimTrailingPathSeparators(path: string) {
  const trimmed = path.trim();
  if (/^[A-Za-z]:[\\/]$/.test(trimmed)) return trimmed;
  return trimmed.replace(/[\\/]+$/, "") || trimmed;
}

export function normalizeFilesystemPath(path: string) {
  return trimTrailingPathSeparators(path)
    .replace(/\//g, "\\")
    .toLowerCase();
}

export function repositoryPathParts(relativePath: string) {
  return relativePath
    .trim()
    .replace(/^[\\/]+|[\\/]+$/g, "")
    .split(/[\\/]+/)
    .filter(Boolean);
}

export function entryNameFromPath(path: string) {
  const segments = path.replace(/\\/g, "/").replace(/\/+$/g, "").split("/").filter(Boolean);
  return segments[segments.length - 1] ?? path;
}

export function joinRepositoryPath(rootPath: string, relativePath: string, name?: string) {
  const normalizedRoot = trimTrailingPathSeparators(rootPath);
  const parts = [
    ...repositoryPathParts(relativePath),
    ...(name ? [name] : []),
  ];
  if (!parts.length) return normalizedRoot;

  const separator = normalizedRoot.includes("\\") ? "\\" : "/";
  if (/^[A-Za-z]:[\\/]$/.test(normalizedRoot)) {
    return `${normalizedRoot}${parts.join(separator)}`;
  }
  return `${normalizedRoot}${separator}${parts.join(separator)}`;
}

export function normalizeRepositoryRelativePath(path: string) {
  return path.trim().replace(/\\/g, "/").replace(/^\/+|\/+$/g, "");
}
