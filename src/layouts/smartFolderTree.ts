import type { SmartFolder, SmartFolderTreeNode } from "../types/repository";

export function flattenSmartFolders(nodes: SmartFolderTreeNode[]): SmartFolder[] {
  return nodes.flatMap((node) => [
    node,
    ...flattenSmartFolders(node.children),
  ]);
}

export function smartFolderMapFromFlatList(folders: SmartFolder[]) {
  return new Map(folders.map((item) => [item.smartFolderId, item]));
}

export function smartFolderAncestry(id: string, folderById: ReadonlyMap<string, SmartFolder>) {
  const path: SmartFolder[] = [];
  let current = folderById.get(id) ?? null;
  while (current) {
    path.unshift(current);
    current = current.parentId
      ? folderById.get(current.parentId) ?? null
      : null;
  }
  return path;
}
