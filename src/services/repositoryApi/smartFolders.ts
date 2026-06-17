import type {
  SmartFolderMutationRequest,
  SmartFolderMutationResponse,
  SmartFolderResultSnapshot,
  SmartFolderTreeNode,
  SmartFolderUpdateRequest,
} from "../../types/repository";
import { invokeCommand } from "./core";

export function listSmartFolders(repoId: string) {
  return invokeCommand<SmartFolderTreeNode[]>("list_smart_folders", { repoId });
}

export function createSmartFolder(request: SmartFolderMutationRequest) {
  return invokeCommand<SmartFolderMutationResponse>("create_smart_folder", { request });
}

export function updateSmartFolder(request: SmartFolderUpdateRequest) {
  return invokeCommand<SmartFolderMutationResponse>("update_smart_folder", { request });
}

export function deleteSmartFolder(repoId: string, smartFolderId: string) {
  return invokeCommand<SmartFolderMutationResponse>("delete_smart_folder", {
    repoId,
    smartFolderId,
  });
}

export function querySmartFolder(repoId: string, smartFolderId: string) {
  return invokeCommand<SmartFolderResultSnapshot>("query_smart_folder", {
    repoId,
    smartFolderId,
  });
}
