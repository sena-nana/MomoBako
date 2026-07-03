import type {
  BinaryFileWriteRequest,
  BinaryFileWriteResponse,
  EntryAccessRecordRequest,
  EntryAccessRecordResponse,
  EntryPlaybackProgressEvent,
  EntryPlaybackRequest,
  EntryPlaybackSourceResponse,
  FileBrowserRequest,
  FileBrowserSnapshot,
  FileArchiveImportRequest,
  FileCopyRequest,
  FileCreateRequest,
  FileDeleteRequest,
  FileImportRequest,
  FileMoveRequest,
  EagleLibraryImportRequest,
  EagleLibraryImportResponse,
  FilePreviewSourceResponse,
  FileReadRequest,
  FileRenameRequest,
  HardlinkCandidateResponse,
  HardlinkConfirmRequest,
  HardlinkConfirmResponse,
  RecentAccessHistoryClearRequest,
  RecentAccessHistoryClearResponse,
  RepositoryTreeSnapshot,
  TrashMutationRequest,
} from "../../types/repository";
import {
  invokeCommand,
  invokeWithProgress,
  openExternalUrl,
  openRepositoryPath,
  revealRepositoryPath,
  startExternalFileDrag,
} from "./core";

export { openExternalUrl, openRepositoryPath, revealRepositoryPath, startExternalFileDrag };

type RepositoryEntryAccessListener = (event: {
  repoId: string;
  path: string;
  recordedAt: string;
}) => void;

const repositoryEntryAccessListeners = new Set<RepositoryEntryAccessListener>();

function emitRepositoryEntryAccess(event: {
  repoId: string;
  path: string;
  recordedAt: string;
}) {
  for (const listener of repositoryEntryAccessListeners) {
    listener(event);
  }
}

export function onRepositoryEntryAccess(listener: RepositoryEntryAccessListener) {
  repositoryEntryAccessListeners.add(listener);
  return () => {
    repositoryEntryAccessListeners.delete(listener);
  };
}

export function getFileBrowser(request: FileBrowserRequest) {
  return invokeCommand<FileBrowserSnapshot>("get_file_browser", { request });
}

export function getRepositoryTree(repoId: string) {
  return invokeCommand<RepositoryTreeSnapshot>("get_repository_tree", { repoId });
}

export function readFile(request: FileReadRequest) {
  return invokeCommand<number[]>("read_file", { request });
}

export function preparePreviewFileSource(request: FileReadRequest) {
  return invokeCommand<FilePreviewSourceResponse>("prepare_preview_file_source", { request }).then((response) => {
    emitRepositoryEntryAccess({
      repoId: request.repoId,
      path: request.path,
      recordedAt: new Date().toISOString(),
    });
    return response;
  });
}

export function prepareEntryPlaybackSource(request: EntryPlaybackRequest) {
  return invokeCommand<EntryPlaybackSourceResponse>("prepare_entry_playback_source", { request }).then((response) => {
    emitRepositoryEntryAccess({
      repoId: request.repoId,
      path: request.path,
      recordedAt: new Date().toISOString(),
    });
    return response;
  });
}

export function prepareEntryPlaybackSourceWithProgress(
  request: EntryPlaybackRequest,
  onEvent: (event: EntryPlaybackProgressEvent) => void,
) {
  return invokeWithProgress<EntryPlaybackSourceResponse, EntryPlaybackProgressEvent>(
    "prepare_entry_playback_source_with_progress",
    { request },
    onEvent,
  ).then((response) => {
    emitRepositoryEntryAccess({
      repoId: request.repoId,
      path: request.path,
      recordedAt: new Date().toISOString(),
    });
    return response;
  });
}

export function recordEntryAccess(request: EntryAccessRecordRequest) {
  return invokeCommand<EntryAccessRecordResponse>("record_entry_access", { request }).then((response) => {
    emitRepositoryEntryAccess(response);
    return response;
  });
}

export function clearRecentAccessHistory(request: RecentAccessHistoryClearRequest) {
  return invokeCommand<RecentAccessHistoryClearResponse>("clear_recent_access_history", { request });
}

export function writeBinaryFile(request: BinaryFileWriteRequest) {
  return invokeCommand<BinaryFileWriteResponse>("write_binary_file", { request });
}

export function createDirectory(request: FileCreateRequest) {
  return invokeCommand<FileBrowserSnapshot>("create_directory", { request });
}

export function createFile(request: FileCreateRequest) {
  return invokeCommand<FileBrowserSnapshot>("create_file", { request });
}

export function importEntries(request: FileImportRequest) {
  return invokeCommand<FileBrowserSnapshot>("import_entries", { request });
}

export function importArchiveEntries(request: FileArchiveImportRequest) {
  return invokeCommand<FileBrowserSnapshot>("import_archive_entries", { request });
}

export function importEagleLibrary(request: EagleLibraryImportRequest) {
  return invokeCommand<EagleLibraryImportResponse>("import_eagle_library", { request });
}

export function copyEntries(request: FileCopyRequest) {
  return invokeCommand<FileBrowserSnapshot>("copy_entries", { request });
}

export function moveEntries(request: FileMoveRequest) {
  return invokeCommand<FileBrowserSnapshot>("move_entries", { request });
}

export function renameEntry(request: FileRenameRequest) {
  return invokeCommand<FileBrowserSnapshot>("rename_entry", { request });
}

export function deleteEntry(request: FileDeleteRequest) {
  return invokeCommand<FileBrowserSnapshot>("delete_entry", { request });
}

export function mutateTrash(request: TrashMutationRequest) {
  return invokeCommand<FileBrowserSnapshot>("mutate_trash", { request });
}

export function listHardlinkCandidates(repoId: string) {
  return invokeCommand<HardlinkCandidateResponse>("list_hardlink_candidates", { repoId });
}

export function confirmHardlinkCandidate(request: HardlinkConfirmRequest) {
  return invokeCommand<HardlinkConfirmResponse>("confirm_hardlink_candidate", { request });
}
