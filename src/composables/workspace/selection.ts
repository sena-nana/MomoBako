import {
  dragHoverFolderPath,
  draggedWorkspacePaths,
  isExternalDragActive,
  isInternalDragActive,
  selectedFilePath,
  selectedFilePaths,
  selectionAnchorPath,
} from "./state";
import { visibleEntries } from "./selectors";

export function normalizeSelectionPaths(paths: string[]) {
  return Array.from(new Set(
    paths
      .map((path) => path.trim())
      .filter(Boolean),
  ));
}

export function applyWorkspaceSelection(
  paths: string[],
  primaryPath: string | null = paths[0] ?? null,
  anchorPath: string | null = primaryPath,
) {
  const nextPaths = normalizeSelectionPaths(paths);
  const nextPrimaryPath = primaryPath && nextPaths.includes(primaryPath)
    ? primaryPath
    : nextPaths[0] ?? null;

  selectedFilePaths.value = nextPaths;
  selectedFilePath.value = nextPrimaryPath;
  selectionAnchorPath.value = nextPaths.length
    ? anchorPath && nextPaths.includes(anchorPath) ? anchorPath : nextPrimaryPath
    : null;
}

export function clearWorkspaceSelection() {
  applyWorkspaceSelection([]);
}

export function selectWorkspaceEntries(
  paths: string[],
  options: {
    primaryPath?: string | null;
    anchorPath?: string | null;
  } = {},
) {
  const nextPaths = normalizeSelectionPaths(paths);
  applyWorkspaceSelection(nextPaths, options.primaryPath ?? nextPaths[0] ?? null, options.anchorPath ?? options.primaryPath ?? nextPaths[0] ?? null);
}

export function selectWorkspaceEntry(
  path: string,
  options: {
    mode?: "replace" | "toggle" | "range";
  } = {},
) {
  const mode = options.mode ?? "replace";

  if (mode === "toggle") {
    const nextSelection = new Set(selectedFilePaths.value);
    if (nextSelection.has(path)) {
      nextSelection.delete(path);
      const nextPaths = Array.from(nextSelection);
      const nextPrimaryPath = selectedFilePath.value === path
        ? nextPaths[0] ?? null
        : selectedFilePath.value;
      applyWorkspaceSelection(nextPaths, nextPrimaryPath, selectionAnchorPath.value === path ? nextPrimaryPath : selectionAnchorPath.value);
      return;
    }
    const nextPaths = [...selectedFilePaths.value, path];
    applyWorkspaceSelection(nextPaths, path, path);
    return;
  }

  if (mode === "range") {
    const orderedPaths = visibleEntries.value.map((entry) => entry.path);
    const anchorPath = selectionAnchorPath.value ?? selectedFilePath.value ?? path;
    const anchorIndex = orderedPaths.indexOf(anchorPath);
    const targetIndex = orderedPaths.indexOf(path);
    if (anchorIndex >= 0 && targetIndex >= 0) {
      const [start, end] = anchorIndex <= targetIndex ? [anchorIndex, targetIndex] : [targetIndex, anchorIndex];
      const nextPaths = orderedPaths.slice(start, end + 1);
      applyWorkspaceSelection(nextPaths, path, anchorPath);
      return;
    }
  }

  applyWorkspaceSelection([path], path, path);
}

export function setExternalDragActive(value: boolean) {
  isExternalDragActive.value = value;
}

export function setInternalDragActive(value: boolean) {
  isInternalDragActive.value = value;
}

export function setDraggedWorkspacePaths(paths: string[]) {
  draggedWorkspacePaths.value = normalizeSelectionPaths(paths);
}

export function clearDraggedWorkspaceState() {
  isInternalDragActive.value = false;
  draggedWorkspacePaths.value = [];
}

export function setDragHoverFolderPath(path: string | null) {
  dragHoverFolderPath.value = path;
}
