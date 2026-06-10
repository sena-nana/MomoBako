export type WorkspaceDragSessionSnapshot = {
  startX: number;
  startY: number;
  lastX: number;
  lastY: number;
};

export type WindowBounds = {
  width: number;
  height: number;
};

function normalizeWorkspacePath(path: string) {
  return path.trim().replace(/\\/g, "/").replace(/^\/+|\/+$/g, "");
}

export function getWorkspaceParentPath(path: string) {
  const normalizedPath = normalizeWorkspacePath(path);
  const index = normalizedPath.lastIndexOf("/");
  return index >= 0 ? normalizedPath.slice(0, index) : "";
}

export function internalWorkspaceDragDistance(session: WorkspaceDragSessionSnapshot) {
  return Math.hypot(session.lastX - session.startX, session.lastY - session.startY);
}

export function shouldDelegateToExternalDrag(
  session: WorkspaceDragSessionSnapshot,
  clientX: number,
  clientY: number,
  bounds: WindowBounds,
  threshold: number,
) {
  const outsideWindow = (
    clientX <= 0 ||
    clientY <= 0 ||
    clientX >= bounds.width ||
    clientY >= bounds.height
  );
  if (!outsideWindow) return false;
  return internalWorkspaceDragDistance(session) >= threshold;
}

export function resolveWorkspaceDropTarget(
  doc: Document,
  clientX: number,
  clientY: number,
  currentDirectoryPath: string,
) {
  const target = doc.elementFromPoint(clientX, clientY) as HTMLElement | null;
  const folderTarget = target?.closest<HTMLElement>("[data-folder-path]");
  if (folderTarget?.dataset.folderPath) {
    return folderTarget.dataset.folderPath;
  }
  const browserTarget = target?.closest<HTMLElement>(".files-browser");
  return browserTarget ? currentDirectoryPath : null;
}

export function normalizeWorkspaceMovePaths(sourcePaths: string[], targetPath: string) {
  const normalizedTargetPath = normalizeWorkspacePath(targetPath);
  return sourcePaths.filter((sourcePath) => {
    const normalizedSourcePath = normalizeWorkspacePath(sourcePath);
    if (!normalizedSourcePath) return false;
    if (normalizedSourcePath === normalizedTargetPath) return false;
    return getWorkspaceParentPath(normalizedSourcePath) !== normalizedTargetPath;
  });
}
