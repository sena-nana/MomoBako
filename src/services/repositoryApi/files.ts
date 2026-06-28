import type {
  BinaryFileWriteRequest,
  BinaryFileWriteResponse,
  EntryPlaybackProgressEvent,
  EntryPlaybackRequest,
  EntryPlaybackSourceResponse,
  FileBrowserRequest,
  FileBrowserSnapshot,
  FileCopyRequest,
  FileCreateRequest,
  FileDeleteRequest,
  FileImportRequest,
  FileMoveRequest,
  FilePreviewSourceResponse,
  FileReadRequest,
  FileRenameRequest,
  HardlinkCandidateResponse,
  HardlinkConfirmRequest,
  HardlinkConfirmResponse,
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
  return invokeCommand<FilePreviewSourceResponse>("prepare_preview_file_source", { request });
}

export function prepareEntryPlaybackSource(request: EntryPlaybackRequest) {
  return invokeCommand<EntryPlaybackSourceResponse>("prepare_entry_playback_source", { request });
}

export function prepareEntryPlaybackSourceWithProgress(
  request: EntryPlaybackRequest,
  onEvent: (event: EntryPlaybackProgressEvent) => void,
) {
  return invokeWithProgress<EntryPlaybackSourceResponse, EntryPlaybackProgressEvent>(
    "prepare_entry_playback_source_with_progress",
    { request },
    onEvent,
  );
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
