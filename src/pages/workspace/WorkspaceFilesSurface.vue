<script setup lang="ts">
import { PencilLine } from "@lucide/vue";
import type { Component } from "vue";
import WorkspacePlayerBar from "../../components/WorkspacePlayerBar.vue";
import type { WorkspacePlayerBarHandlers, WorkspacePlayerBarProps } from "../../components/workspacePlayerBar.contract";
import { FileBrowserPanel, FilePreviewPane } from "./lazyComponents";
import type { ContextMenuItem } from "../../ui/core";
import type { RegisteredLibraryExtension } from "../../plugins/sdk";
import type { FileBrowserEntry, RepositoryTagGroup } from "../../types/repository";
import type { FileDisplayMode } from "./useWorkspaceViewState";

type BreadcrumbSegment = {
  label: string;
  path: string;
};

type SelectionMode = "replace" | "toggle" | "range";
type BoxSelectionMode = "replace" | "append";

defineProps<{
  activeFileEntries: FileBrowserEntry[];
  allEntries: FileBrowserEntry[];
  activeRepoId: string | null;
  availableTags: string[];
  breadcrumbs: BreadcrumbSegment[];
  canDeleteSelected: boolean;
  canDragEntries: boolean;
  canImport?: boolean;
  canOpenSelected: boolean;
  canRenameSelected: boolean;
  canRestoreSelected: boolean;
  canClearRecentHistory?: boolean;
  currentFileEntry: FileBrowserEntry | null;
  currentDirectoryDisplayName: string;
  currentDirectoryPath: string;
  currentLibraryExtensions: RegisteredLibraryExtension[];
  directoryEntries: FileBrowserEntry[];
  displayModeClass: string;
  displayModeOptions: Array<{ value: FileDisplayMode; label: string }>;
  dropTargetPath: string | null;
  entryDeletedAtLabel: (entry: FileBrowserEntry) => string | null;
  entryModifiedAtLabel: (entry: FileBrowserEntry) => string;
  error: string | null;
  fileEntryContextMenu: (entry: FileBrowserEntry) => ContextMenuItem[];
  fileItemStyle: (entry: FileBrowserEntry) => Record<string, string>;
  fileTone: (entry: FileBrowserEntry) => string;
  hardlinkStateLabel: (entry: FileBrowserEntry) => string;
  hasSplitFileGroups: boolean;
  isAudioEntry: (entry: FileBrowserEntry) => boolean;
  isDragActive: boolean;
  isDraggingFiles: boolean;
  isLoadingFileBrowser: boolean;
  isLoadingFileBrowserMore?: boolean;
  isModelEntry: (entry: FileBrowserEntry) => boolean;
  isMutatingFiles: boolean;
  isClearingRecentHistory?: boolean;
  isRecentView?: boolean;
  isReadOnlyVirtual: boolean;
  isSavingMetadata: boolean;
  isTrashPanel: boolean;
  isVirtualView: boolean;
  isVideoEntry: (entry: FileBrowserEntry) => boolean;
  hasMoreEntries?: boolean;
  libraryExtensions: RegisteredLibraryExtension[];
  openSelectedLabel: string;
  previewFileEntry: FileBrowserEntry | null;
  previewLibraryExtensions: RegisteredLibraryExtension[];
  previewPlugin: { component: Component } | null;
  renameTargetPath: string | null;
  saveCoverThumbnail: (path: string, sourceUrl: string) => Promise<unknown>;
  saveMetadata: (entry: FileBrowserEntry, metadata: Record<string, unknown>) => Promise<unknown>;
  selectedEntries: FileBrowserEntry[];
  selectedFilePath: string | null;
  selectedFilePaths: string[];
  showWorkspacePlayer: boolean;
  statusLabel: (status: string) => string;
  tagGroups: RepositoryTagGroup[];
  thumbnailPalette: (entry: FileBrowserEntry) => string[];
  thumbnailSrc: (entry: FileBrowserEntry) => string | null;
  virtualSubline: string;
  virtualTitle?: string;
  workspacePlayerBarHandlers: WorkspacePlayerBarHandlers;
  workspacePlayerBarProps: WorkspacePlayerBarProps;
}>();

const createFileName = defineModel<string>("createFileName", { required: true });
const fileDisplayMode = defineModel<FileDisplayMode>("fileDisplayMode", { required: true });
const renameValue = defineModel<string>("renameValue", { required: true });

const emit = defineEmits<{
  back: [];
  createFile: [];
  clearRecentHistory: [];
  deleteSelected: [];
  dragLeave: [event: DragEvent];
  dragOver: [event: DragEvent];
  drop: [event: DragEvent];
  dropOnFolder: [path: string, dragEvent: DragEvent];
  emptyTrash: [];
  importEagleCopy: [];
  importEagleMove: [];
  importFolder: [];
  importZip: [];
  entryDragEnd: [event: PointerEvent | null];
  entryDragMove: [event: PointerEvent];
  entryDragStart: [entry: FileBrowserEntry, event: PointerEvent];
  hoverFolder: [path: string];
  leaveFolder: [path: string];
  loadMore: [];
  markThumbnailFailed: [entry: FileBrowserEntry];
  openDirectory: [path: string];
  openEntry: [entry: FileBrowserEntry];
  openSelected: [];
  previewFile: [entry: FileBrowserEntry];
  restoreAllTrash: [];
  restoreSelected: [];
  revealEntry: [entry: FileBrowserEntry];
  revealSelected: [];
  selectEntries: [paths: string[], mode: BoxSelectionMode];
  selectEntry: [entry: FileBrowserEntry, mode: SelectionMode];
  startRename: [];
  cancelRename: [];
  submitRename: [];
  thumbnailError: [entry: FileBrowserEntry];
  thumbnailLoaded: [entry: FileBrowserEntry, event: Event];
  visibleEntriesChange: [entries: FileBrowserEntry[]];
}>();
</script>

<template>
  <section :class="previewFileEntry ? 'files-preview-page' : 'files-workbench'">
    <template v-if="previewFileEntry">
      <FilePreviewPane
        :entry="previewFileEntry"
        :plugin="previewPlugin"
        :repo-id="activeRepoId ?? ''"
        :thumbnail-src="thumbnailSrc"
        :is-video-entry="isVideoEntry"
        :is-audio-entry="isAudioEntry"
        :hardlink-state-label="hardlinkStateLabel"
        :is-saving-metadata="isSavingMetadata"
        :available-tags="availableTags"
        :tag-groups="tagGroups"
        :playlist-entries="activeFileEntries"
        :library-extensions="previewLibraryExtensions"
        :thumbnail-palette="thumbnailPalette"
        :save-metadata="saveMetadata"
        :save-cover-thumbnail="saveCoverThumbnail"
        @back="emit('back')"
        @open="emit('openEntry', $event)"
        @reveal="emit('revealEntry', $event)"
        @preview="emit('previewFile', $event)"
        @thumbnail-loaded="(entry, event) => emit('thumbnailLoaded', entry, event)"
        @thumbnail-error="emit('thumbnailError', $event)"
      />
      <WorkspacePlayerBar v-if="showWorkspacePlayer" v-bind="workspacePlayerBarProps" v-on="workspacePlayerBarHandlers" />
    </template>

    <div v-show="!previewFileEntry" class="workspace-files-surface__browser-shell">
      <FileBrowserPanel
        v-model:create-file-name="createFileName"
        v-model:file-display-mode="fileDisplayMode"
        :breadcrumbs="breadcrumbs"
        :can-drag-entries="canDragEntries"
        :can-delete-selected="canDeleteSelected"
        :can-import="canImport"
        :can-open-selected="canOpenSelected"
        :can-rename-selected="canRenameSelected"
        :can-restore-selected="canRestoreSelected"
        :can-clear-recent-history="canClearRecentHistory"
        :current-file-entry="currentFileEntry"
        :current-directory-display-name="currentDirectoryDisplayName"
        :current-directory-path="currentDirectoryPath"
        :all-entries="allEntries"
        :directory-entries="directoryEntries"
        :display-mode-class="displayModeClass"
        :display-mode-options="displayModeOptions"
        :drop-target-path="dropTargetPath"
        :entry-deleted-at-label="entryDeletedAtLabel"
        :entry-modified-at-label="entryModifiedAtLabel"
        :error="error"
        :file-entries="activeFileEntries"
        :file-entry-context-menu="fileEntryContextMenu"
        :file-item-style="fileItemStyle"
        :file-tone="fileTone"
        :hardlink-state-label="hardlinkStateLabel"
        :has-split-file-groups="hasSplitFileGroups"
        :is-audio-entry="isAudioEntry"
        :is-drag-active="isDragActive"
        :is-dragging-files="isDraggingFiles"
        :is-loading-file-browser="isLoadingFileBrowser"
        :is-loading-file-browser-more="isLoadingFileBrowserMore"
        :is-model-entry="isModelEntry"
        :is-mutating-files="isMutatingFiles"
        :is-clearing-recent-history="isClearingRecentHistory"
        :is-recent-view="isRecentView"
        :is-read-only-virtual="isReadOnlyVirtual"
        :is-trash-panel="isTrashPanel"
        :is-virtual-view="isVirtualView"
        :is-video-entry="isVideoEntry"
        :has-more-entries="hasMoreEntries"
        :open-selected-label="openSelectedLabel"
        :is-saving-metadata="isSavingMetadata"
        :available-tags="availableTags"
        :tag-groups="tagGroups"
        :library-extensions="currentLibraryExtensions"
        :thumbnail-palette="thumbnailPalette"
        :save-metadata="saveMetadata"
        :selected-entries="selectedEntries"
        :selected-file-paths="selectedFilePaths"
        :selected-file-path="selectedFilePath"
        :status-label="statusLabel"
        :thumbnail-src="thumbnailSrc"
        :virtual-subline="virtualSubline"
        :virtual-title="virtualTitle"
        @clear-recent-history="emit('clearRecentHistory')"
        @create-file="emit('createFile')"
        @delete-selected="emit('deleteSelected')"
        @drag-leave="emit('dragLeave', $event)"
        @drag-over="emit('dragOver', $event)"
        @drop="emit('drop', $event)"
        @empty-trash="emit('emptyTrash')"
        @import-eagle-copy="emit('importEagleCopy')"
        @import-eagle-move="emit('importEagleMove')"
        @import-folder="emit('importFolder')"
        @import-zip="emit('importZip')"
        @entry-drag-end="emit('entryDragEnd', $event)"
        @entry-drag-move="emit('entryDragMove', $event)"
        @entry-drag-start="(entry, event) => emit('entryDragStart', entry, event)"
        @hover-folder="emit('hoverFolder', $event)"
        @load-more="emit('loadMore')"
        @mark-thumbnail-failed="emit('markThumbnailFailed', $event)"
        @leave-folder="emit('leaveFolder', $event)"
        @drop-on-folder="(path, dragEvent) => emit('dropOnFolder', path, dragEvent)"
        @open-directory="emit('openDirectory', $event)"
        @open-selected="emit('openSelected')"
        @preview-file="emit('previewFile', $event)"
        @restore-all-trash="emit('restoreAllTrash')"
        @restore-selected="emit('restoreSelected')"
        @reveal-selected="emit('revealSelected')"
        @select-entry="(entry, mode) => emit('selectEntry', entry, mode)"
        @select-entries="(paths, mode) => emit('selectEntries', paths, mode)"
        @start-rename="emit('startRename')"
        @submit-rename="emit('submitRename')"
        @thumbnail-loaded="(entry, event) => emit('thumbnailLoaded', entry, event)"
        @visible-entries-change="emit('visibleEntriesChange', $event)"
      >
        <template #player>
          <WorkspacePlayerBar v-if="showWorkspacePlayer" v-bind="workspacePlayerBarProps" v-on="workspacePlayerBarHandlers" />
        </template>
      </FileBrowserPanel>
    </div>
    <Teleport to="body">
      <Transition name="modal">
        <div
          v-if="renameTargetPath"
          class="modal-overlay"
          role="dialog"
          aria-modal="true"
          aria-label="重命名文件"
          @click.self="emit('cancelRename')"
        >
          <div class="modal-card dialog-card workspace-rename-dialog">
            <div class="dialog-card__header">
              <PencilLine :size="14" aria-hidden="true" />
              <span>重命名文件</span>
            </div>
            <div class="dialog-card__body workspace-rename-dialog__body">
              <label class="dialog-field">
                <span>新名称</span>
                <input
                  v-model="renameValue"
                  type="text"
                  autofocus
                  :disabled="isMutatingFiles"
                  placeholder="输入新的文件名"
                  @keydown.esc.prevent="emit('cancelRename')"
                  @keydown.enter.prevent="emit('submitRename')"
                />
              </label>
            </div>
            <div class="dialog-card__actions">
              <button type="button" class="ghost" :disabled="isMutatingFiles" @click="emit('cancelRename')">
                取消
              </button>
              <button type="button" class="primary" :disabled="isMutatingFiles" @click="emit('submitRename')">
                {{ isMutatingFiles ? "处理中..." : "保存" }}
              </button>
            </div>
          </div>
        </div>
      </Transition>
    </Teleport>
  </section>
</template>

<style scoped>
.workspace-files-surface__browser-shell {
  display: contents;
}

.workspace-rename-dialog {
  width: min(460px, 92vw);
}
</style>
