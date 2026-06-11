<script setup lang="ts">
import { computed, ref } from "vue";
import {
  Eye,
  File,
  FileAudio,
  FileImage,
  FileVideo,
  Folder,
  FolderOpen,
  LoaderCircle,
  PencilLine,
  Plus,
  RotateCcw,
  Trash2,
} from "lucide-vue-next";
import FileMetadataEditor from "./FileMetadataEditor.vue";
import ThumbnailPalette from "../../components/ThumbnailPalette.vue";
import type { ContextMenuItem } from "../../composables/useContextMenu";
import { vContextMenu } from "../../directives/contextMenu";
import type { FileBrowserEntry, RepositoryTagGroup } from "../../types/repository";

type FileDisplayMode = "adaptive" | "masonry" | "grid" | "list";
type BreadcrumbSegment = {
  label: string;
  path: string;
};
type EntryDragIntent = {
  entryPath: string;
  pointerId: number;
  startX: number;
  startY: number;
};
type SelectionMode = "replace" | "toggle" | "range";
type BoxSelectionMode = "replace" | "append";
type BoxSelectionState = {
  pointerId: number;
  startX: number;
  startY: number;
  currentX: number;
  currentY: number;
  additive: boolean;
  didDrag: boolean;
};

const props = defineProps<{
  breadcrumbs: BreadcrumbSegment[];
  canDragEntries: boolean;
  canDeleteSelected: boolean;
  canOpenSelected: boolean;
  canRenameSelected: boolean;
  canRestoreSelected: boolean;
  currentFileEntry: FileBrowserEntry | null;
  directoryEntries: FileBrowserEntry[];
  displayModeClass: string;
  displayModeOptions: Array<{ value: FileDisplayMode; label: string }>;
  dropTargetPath: string | null;
  entryDeletedAtLabel: (entry: FileBrowserEntry) => string | null;
  entryModifiedAtLabel: (entry: FileBrowserEntry) => string;
  error: string | null;
  fileEntries: FileBrowserEntry[];
  fileEntryContextMenu: (entry: FileBrowserEntry) => ContextMenuItem[];
  fileItemStyle: (entry: FileBrowserEntry) => Record<string, string>;
  fileTone: (entry: FileBrowserEntry) => string;
  hardlinkStateLabel: (entry: FileBrowserEntry) => string;
  hasSplitFileGroups: boolean;
  isAudioEntry: (entry: FileBrowserEntry) => boolean;
  isDraggingFiles: boolean;
  isDragActive: boolean;
  isLoadingFileBrowser: boolean;
  isModelEntry: (entry: FileBrowserEntry) => boolean;
  isMutatingFiles: boolean;
  isReadOnlyVirtual?: boolean;
  isTrashPanel: boolean;
  isVideoEntry: (entry: FileBrowserEntry) => boolean;
  openSelectedLabel: string;
  renameTargetPath: string | null;
  selectedEntries: FileBrowserEntry[];
  selectedFilePaths: string[];
  selectedFilePath: string | null;
  statusLabel: (status: string) => string;
  thumbnailSrc: (entry: FileBrowserEntry) => string | null;
  isSavingMetadata: boolean;
  availableTags: string[];
  tagGroups?: RepositoryTagGroup[];
  thumbnailPalette: (entry: FileBrowserEntry) => string[];
  saveMetadata: (entry: FileBrowserEntry, metadata: Record<string, unknown>) => Promise<unknown>;
  virtualSubline?: string;
  virtualTitle?: string;
}>();

const createFileName = defineModel<string>("createFileName", { required: true });
const fileDisplayMode = defineModel<FileDisplayMode>("fileDisplayMode", { required: true });
const renameValue = defineModel<string>("renameValue", { required: true });

const emit = defineEmits<{
  createFile: [];
  deleteSelected: [];
  dragLeave: [event: DragEvent];
  dragOver: [event: DragEvent];
  drop: [event: DragEvent];
  emptyTrash: [];
  entryDragEnd: [event: PointerEvent | null];
  entryDragMove: [event: PointerEvent];
  entryDragStart: [entry: FileBrowserEntry, event: PointerEvent];
  hoverFolder: [path: string];
  leaveFolder: [path: string];
  markThumbnailFailed: [entry: FileBrowserEntry];
  openDirectory: [path: string];
  openSelected: [];
  dropOnFolder: [path: string, dragEvent: DragEvent];
  previewFile: [entry: FileBrowserEntry];
  restoreAllTrash: [];
  restoreSelected: [];
  revealSelected: [];
  selectEntries: [paths: string[], mode: BoxSelectionMode];
  selectEntry: [entry: FileBrowserEntry, mode: SelectionMode];
  startRename: [];
  submitRename: [];
  thumbnailLoaded: [entry: FileBrowserEntry, event: Event];
}>();

const dragStartThreshold = 7;
let entryDragIntent: EntryDragIntent | null = null;
let activeDragPointerId: number | null = null;
let suppressClickPath: string | null = null;
const filesListRef = ref<HTMLElement | null>(null);
const boxSelection = ref<BoxSelectionState | null>(null);
const selectedPathSet = computed(() => new Set(props.selectedFilePaths));
const dropTargetPathSet = computed(() => (
  props.dropTargetPath ? new Set([props.dropTargetPath]) : new Set<string>()
));
const multiSelectionSummary = computed(() => {
  const directoryCount = props.selectedEntries.filter((entry) => entry.kind === "directory").length;
  const fileCount = props.selectedEntries.length - directoryCount;
  return [
    directoryCount ? `${directoryCount} 个文件夹` : "",
    fileCount ? `${fileCount} 个文件` : "",
  ].filter(Boolean).join(" · ");
});
const selectionBoxStyle = computed(() => {
  const selection = boxSelection.value;
  const container = filesListRef.value;
  if (!selection || !selection.didDrag || !container) return null;

  const rect = container.getBoundingClientRect();
  const left = Math.max(0, Math.min(selection.startX, selection.currentX) - rect.left + container.scrollLeft);
  const top = Math.max(0, Math.min(selection.startY, selection.currentY) - rect.top + container.scrollTop);
  const width = Math.abs(selection.currentX - selection.startX);
  const height = Math.abs(selection.currentY - selection.startY);
  return {
    left: `${left}px`,
    top: `${top}px`,
    width: `${width}px`,
    height: `${height}px`,
  };
});

function selectionModeFromEvent(event: MouseEvent): SelectionMode {
  if (event.shiftKey) return "range";
  if (event.ctrlKey || event.metaKey) return "toggle";
  return "replace";
}

function handleEntryClick(entry: FileBrowserEntry, event: MouseEvent) {
  if (suppressClickPath === entry.path) {
    suppressClickPath = null;
    return;
  }
  emit("selectEntry", entry, selectionModeFromEvent(event));
}

function handleEntryDoubleClick(entry: FileBrowserEntry) {
  if (entry.kind === "directory") {
    emit("openDirectory", entry.path);
    return;
  }
  emit("previewFile", entry);
}

function handleListPointerDown(event: PointerEvent) {
  if (event.button !== 0) return;
  if ((event.target as HTMLElement | null)?.closest(".files-list__item")) return;

  boxSelection.value = {
    pointerId: event.pointerId,
    startX: event.clientX,
    startY: event.clientY,
    currentX: event.clientX,
    currentY: event.clientY,
    additive: event.ctrlKey || event.metaKey,
    didDrag: false,
  };
  filesListRef.value?.setPointerCapture?.(event.pointerId);
}

function collectBoxSelectionPaths(selection: BoxSelectionState) {
  const container = filesListRef.value;
  if (!container) return [];

  const left = Math.min(selection.startX, selection.currentX);
  const right = Math.max(selection.startX, selection.currentX);
  const top = Math.min(selection.startY, selection.currentY);
  const bottom = Math.max(selection.startY, selection.currentY);

  return Array.from(container.querySelectorAll<HTMLElement>(".files-list__item[data-entry-path]"))
    .filter((item) => {
      const rect = item.getBoundingClientRect();
      return rect.right >= left && rect.left <= right && rect.bottom >= top && rect.top <= bottom;
    })
    .map((item) => item.dataset.entryPath ?? "")
    .filter(Boolean);
}

function updateBoxSelection(event: PointerEvent) {
  const selection = boxSelection.value;
  if (!selection || selection.pointerId !== event.pointerId) return;

  selection.currentX = event.clientX;
  selection.currentY = event.clientY;
  selection.didDrag ||= Math.abs(selection.currentX - selection.startX) > 3 || Math.abs(selection.currentY - selection.startY) > 3;
  boxSelection.value = { ...selection };

  if (!selection.didDrag) return;
  const paths = collectBoxSelectionPaths(selection);
  emit("selectEntries", paths, selection.additive ? "append" : "replace");
}

function clearBoxSelection(event: PointerEvent) {
  const selection = boxSelection.value;
  if (!selection || selection.pointerId !== event.pointerId) return;

  if (!selection.didDrag && !selection.additive) {
    emit("selectEntries", [], "replace");
  }
  filesListRef.value?.releasePointerCapture?.(event.pointerId);
  boxSelection.value = null;
}

function releaseEntryPointer(event: PointerEvent) {
  const target = event.currentTarget as HTMLElement | null;
  if (target?.hasPointerCapture?.(event.pointerId)) {
    target.releasePointerCapture(event.pointerId);
  }
}

function handleEntryPointerDown(entry: FileBrowserEntry, event: PointerEvent) {
  if (!props.canDragEntries || event.button !== 0) return;
  entryDragIntent = {
    entryPath: entry.path,
    pointerId: event.pointerId,
    startX: event.clientX,
    startY: event.clientY,
  };
  activeDragPointerId = null;
  (event.currentTarget as HTMLElement | null)?.setPointerCapture?.(event.pointerId);
}

function handleEntryPointerMove(entry: FileBrowserEntry, event: PointerEvent) {
  if (activeDragPointerId === event.pointerId) {
    emit("entryDragMove", event);
    return;
  }

  const intent = entryDragIntent;
  if (!intent || intent.pointerId !== event.pointerId || intent.entryPath !== entry.path) return;
  if ((event.buttons & 1) !== 1) {
    entryDragIntent = null;
    return;
  }

  const distance = Math.hypot(event.clientX - intent.startX, event.clientY - intent.startY);
  if (distance < dragStartThreshold) return;

  suppressClickPath = entry.path;
  activeDragPointerId = event.pointerId;
  entryDragIntent = null;
  emit("entryDragStart", entry, event);
  emit("entryDragMove", event);
}

function clearEntryDragIntent(event: PointerEvent) {
  const shouldEmitDragEnd = activeDragPointerId === event.pointerId;
  if (entryDragIntent?.pointerId === event.pointerId) {
    entryDragIntent = null;
  }
  if (shouldEmitDragEnd) {
    activeDragPointerId = null;
    emit("entryDragEnd", event);
  }
  releaseEntryPointer(event);
}

function cancelEntryDragIntent(event: PointerEvent) {
  const shouldEmitDragEnd = activeDragPointerId === event.pointerId;
  if (entryDragIntent?.pointerId === event.pointerId) {
    entryDragIntent = null;
  }
  if (shouldEmitDragEnd) {
    activeDragPointerId = null;
    emit("entryDragEnd", null);
  }
  releaseEntryPointer(event);
}
</script>

<template>
  <div
    class="files-browser"
    :class="{ 'is-dragging': isDraggingFiles }"
    @dragover="emit('dragOver', $event)"
    @dragleave="emit('dragLeave', $event)"
    @drop="emit('drop', $event)"
  >
    <header class="files-browser__header">
      <div>
        <p class="asset-browser__eyebrow">{{ isReadOnlyVirtual ? "智能文件夹" : isTrashPanel ? "回收站" : "当前目录" }}</p>
        <div class="files-breadcrumbs">
          <button
            type="button"
            class="files-breadcrumbs__item"
            :disabled="isReadOnlyVirtual"
            @click="emit('openDirectory', '')"
          >
            {{ isReadOnlyVirtual ? virtualTitle || "智能文件夹" : isTrashPanel ? "回收站" : "根目录" }}
          </button>
          <button
            v-if="!isReadOnlyVirtual"
            v-for="segment in breadcrumbs"
            :key="segment.path"
            type="button"
            class="files-breadcrumbs__item"
            @click="emit('openDirectory', segment.path)"
          >
            {{ segment.label }}
          </button>
        </div>
        <p v-if="isReadOnlyVirtual && virtualSubline" class="files-browser__subline">{{ virtualSubline }}</p>
      </div>

      <div class="files-toolbar">
        <label class="files-toolbar__select">
          <span>展示方式</span>
          <select v-model="fileDisplayMode" aria-label="素材展示方式">
            <option v-for="option in displayModeOptions" :key="option.value" :value="option.value">
              {{ option.label }}
            </option>
          </select>
        </label>

        <template v-if="!isTrashPanel && !isReadOnlyVirtual">
          <label class="files-toolbar__field">
            <Plus :size="14" aria-hidden="true" />
            <input v-model="createFileName" type="text" placeholder="新建空文件，例如 note.txt" />
          </label>
          <button type="button" class="ghost files-toolbar__btn" :disabled="isMutatingFiles" @click="emit('createFile')">
            <File :size="14" aria-hidden="true" />
            建文件
          </button>
        </template>

        <template v-else>
          <button type="button" class="ghost files-toolbar__btn" :disabled="isMutatingFiles" @click="emit('restoreAllTrash')">
            <RotateCcw :size="14" aria-hidden="true" />
            还原所有项目
          </button>
          <button type="button" class="ghost danger files-toolbar__btn" :disabled="isMutatingFiles" @click="emit('emptyTrash')">
            <Trash2 :size="14" aria-hidden="true" />
            清空回收站
          </button>
        </template>
      </div>
    </header>

    <div v-if="error" class="asset-browser__state asset-browser__state--error">
      {{ error }}
    </div>

    <div v-else-if="isLoadingFileBrowser" class="asset-browser__state">
      <LoaderCircle class="spin" :size="16" aria-hidden="true" />
      正在读取目录
    </div>

    <div v-else-if="isMutatingFiles" class="asset-browser__state">
      <LoaderCircle class="spin" :size="16" aria-hidden="true" />
      正在处理文件
    </div>

    <template v-else>
      <div
        ref="filesListRef"
        class="files-list"
        @pointerdown="handleListPointerDown"
        @pointermove="updateBoxSelection"
        @pointerup="clearBoxSelection"
        @pointercancel="clearBoxSelection"
      >
        <button
          v-for="entry in directoryEntries"
          :key="entry.path"
          v-context-menu="() => fileEntryContextMenu(entry)"
          type="button"
          class="files-list__item"
          :class="{
            'is-active': selectedPathSet.has(entry.path),
            'can-drag-out': canDragEntries,
            'is-drop-target': isDragActive && dropTargetPathSet.has(entry.path),
          }"
          :data-entry-path="entry.path"
          :data-folder-path="entry.path"
          @click="handleEntryClick(entry, $event)"
          @dblclick="handleEntryDoubleClick(entry)"
          @pointerdown="handleEntryPointerDown(entry, $event)"
          @pointermove="handleEntryPointerMove(entry, $event)"
          @pointerup="clearEntryDragIntent"
          @pointercancel="cancelEntryDragIntent"
          @dragenter.prevent="emit('hoverFolder', entry.path)"
          @dragover.prevent="emit('hoverFolder', entry.path)"
          @dragleave.prevent="emit('leaveFolder', entry.path)"
          @drop.stop.prevent="emit('dropOnFolder', entry.path, $event)"
        >
          <div class="files-list__preview" :style="{ background: fileTone(entry) }">
            <Folder :size="24" aria-hidden="true" />
          </div>
          <div class="files-list__body">
            <strong>{{ entry.name }}</strong>
          </div>
        </button>

        <div v-if="hasSplitFileGroups" class="files-list__divider" aria-hidden="true"></div>

        <div class="files-list__files" :class="displayModeClass">
          <button
            v-for="entry in fileEntries"
            :key="entry.path"
            v-context-menu="() => fileEntryContextMenu(entry)"
            type="button"
            class="files-list__item files-list__item--file"
            :class="{ 'is-active': selectedPathSet.has(entry.path), 'can-drag-out': canDragEntries }"
            :data-entry-path="entry.path"
            :style="fileItemStyle(entry)"
            @click="handleEntryClick(entry, $event)"
            @dblclick="handleEntryDoubleClick(entry)"
            @pointerdown="handleEntryPointerDown(entry, $event)"
            @pointermove="handleEntryPointerMove(entry, $event)"
            @pointerup="clearEntryDragIntent"
            @pointercancel="cancelEntryDragIntent"
          >
            <div class="files-list__preview">
              <img
                v-if="thumbnailSrc(entry)"
                :src="thumbnailSrc(entry) ?? undefined"
                alt=""
                crossorigin="anonymous"
                draggable="false"
                loading="lazy"
                @load="emit('thumbnailLoaded', entry, $event)"
                @dragstart.prevent
                @error="emit('markThumbnailFailed', entry)"
              />
              <FileVideo v-else-if="isVideoEntry(entry)" :size="24" aria-hidden="true" />
              <FileAudio v-else-if="isAudioEntry(entry)" :size="24" aria-hidden="true" />
              <File v-else-if="isModelEntry(entry)" :size="24" aria-hidden="true" />
              <FileImage v-else :size="24" aria-hidden="true" />
            </div>
            <div class="files-list__body">
              <strong>{{ entry.name }}</strong>
              <span v-if="hardlinkStateLabel(entry) && fileDisplayMode !== 'list'">{{ hardlinkStateLabel(entry) }}</span>
              <span v-if="fileDisplayMode === 'list'">{{ entry.path }}</span>
            </div>
            <div v-if="fileDisplayMode === 'list'" class="files-list__meta">
              <span>{{ entry.extension || '文件' }}</span>
              <span>{{ entry.sizeLabel || "未知" }}</span>
              <span>{{ entry.status ? statusLabel(entry.status) : "未索引" }}</span>
              <span v-if="hardlinkStateLabel(entry)">{{ hardlinkStateLabel(entry) }}</span>
              <span>{{ entryModifiedAtLabel(entry) }}</span>
            </div>
          </button>
        </div>

        <div v-if="selectionBoxStyle" class="files-list__selection-box" :style="selectionBoxStyle"></div>
      </div>
    </template>
  </div>

  <aside class="files-detail">
    <div v-if="selectedEntries.length > 1" class="files-detail__card">
      <div class="files-detail__section">
        <p class="asset-browser__eyebrow">选中项</p>
        <h2>已选择 {{ selectedEntries.length }} 个项目</h2>
        <p class="files-detail__subline">{{ multiSelectionSummary }}</p>
      </div>

      <div class="files-detail__section">
        <div class="files-detail__actions">
          <button v-if="isTrashPanel" type="button" class="ghost" :disabled="isMutatingFiles || !canRestoreSelected" @click="emit('restoreSelected')">
            <RotateCcw :size="14" aria-hidden="true" />
            批量还原
          </button>
          <button type="button" class="ghost" disabled>
            <Eye :size="14" aria-hidden="true" />
            预览仅支持单选
          </button>
          <button type="button" class="ghost" disabled>
            <PencilLine :size="14" aria-hidden="true" />
            重命名仅支持单选
          </button>
          <button type="button" class="ghost danger" :disabled="isMutatingFiles || !canDeleteSelected" @click="emit('deleteSelected')">
            <File :size="14" aria-hidden="true" />
            {{ isTrashPanel ? "批量彻底删除" : "批量删除" }}
          </button>
        </div>
      </div>
    </div>

    <div v-else-if="currentFileEntry" class="files-detail__card">
      <div class="files-detail__preview" :style="{ background: currentFileEntry.kind === 'directory' ? fileTone(currentFileEntry) : undefined }">
        <img
          v-if="thumbnailSrc(currentFileEntry)"
          :src="thumbnailSrc(currentFileEntry) ?? undefined"
          alt=""
          crossorigin="anonymous"
          draggable="false"
          @load="emit('thumbnailLoaded', currentFileEntry, $event)"
          @dragstart.prevent
          @error="emit('markThumbnailFailed', currentFileEntry)"
        />
        <Folder v-else-if="currentFileEntry.kind === 'directory'" :size="34" aria-hidden="true" />
        <FileVideo v-else-if="isVideoEntry(currentFileEntry)" :size="34" aria-hidden="true" />
        <FileAudio v-else-if="isAudioEntry(currentFileEntry)" :size="34" aria-hidden="true" />
        <File v-else-if="isModelEntry(currentFileEntry)" :size="34" aria-hidden="true" />
        <FileImage v-else :size="34" aria-hidden="true" />
      </div>
      <ThumbnailPalette :colors="thumbnailPalette(currentFileEntry)" />

      <div class="files-detail__section">
        <p class="asset-browser__eyebrow">选中项</p>
        <h2>{{ currentFileEntry.name }}</h2>
        <p class="files-detail__subline">{{ currentFileEntry.path }}</p>
      </div>

      <div class="files-detail__section">
        <div class="files-detail__actions">
          <button v-if="isTrashPanel" type="button" class="ghost" :disabled="isMutatingFiles || !canRestoreSelected" @click="emit('restoreSelected')">
            <RotateCcw :size="14" aria-hidden="true" />
            还原
          </button>
          <button type="button" class="ghost" :disabled="!canOpenSelected" @click="emit('openSelected')">
            <Eye :size="14" aria-hidden="true" />
            {{ openSelectedLabel }}
          </button>
          <button type="button" class="ghost" :disabled="isTrashPanel" @click="emit('revealSelected')">
            <FolderOpen :size="14" aria-hidden="true" />
            定位
          </button>
          <button v-if="!isReadOnlyVirtual" type="button" class="ghost" :disabled="!canRenameSelected" @click="emit('startRename')">
            <PencilLine :size="14" aria-hidden="true" />
            重命名
          </button>
          <button v-if="!isReadOnlyVirtual" type="button" class="ghost danger" :disabled="isMutatingFiles || !canDeleteSelected" @click="emit('deleteSelected')">
            <File :size="14" aria-hidden="true" />
            {{ isTrashPanel ? "彻底删除" : "删除" }}
          </button>
        </div>
        <p v-if="isTrashPanel" class="files-detail__hint">
          回收站中的删除会直接从文件系统移除。
        </p>
        <p v-else-if="isReadOnlyVirtual" class="files-detail__hint">
          智能文件夹不会改变实际目录。
        </p>
      </div>

      <div v-if="!isReadOnlyVirtual && renameTargetPath === currentFileEntry.path" class="files-detail__section">
        <p class="asset-browser__eyebrow">重命名</p>
        <div class="files-detail__rename">
          <input v-model="renameValue" type="text" />
          <button type="button" :disabled="isMutatingFiles" @click="emit('submitRename')">
            <PencilLine :size="14" aria-hidden="true" />
            保存
          </button>
        </div>
      </div>

      <div class="files-detail__stats">
        <div class="asset-meta__row">
          <span>类型</span>
          <span class="asset-meta__value">{{ currentFileEntry.kind === 'directory' ? '文件夹' : currentFileEntry.extension || '文件' }}</span>
        </div>
        <div class="asset-meta__row">
          <span>大小</span>
          <span class="asset-meta__value">{{ currentFileEntry.sizeLabel || "目录项" }}</span>
        </div>
        <div class="asset-meta__row">
          <span>状态</span>
          <span class="asset-meta__value">{{ currentFileEntry.status ? statusLabel(currentFileEntry.status) : "未索引" }}</span>
        </div>
        <div v-if="hardlinkStateLabel(currentFileEntry)" class="asset-meta__row">
          <span>硬链接</span>
          <span class="asset-meta__value">{{ hardlinkStateLabel(currentFileEntry) }}</span>
        </div>
        <div v-if="currentFileEntry.folderMetadata?.protected" class="asset-meta__row">
          <span>受保护</span>
          <span class="asset-meta__value">{{ currentFileEntry.folderMetadata.passwordTip || "Eagle 迁移提示" }}</span>
        </div>
        <div class="asset-meta__row">
          <span>修改时间</span>
          <span class="asset-meta__value">{{ currentFileEntry.modifiedAt ? new Date(currentFileEntry.modifiedAt).toLocaleString("zh-CN") : "未记录" }}</span>
        </div>
        <div v-if="isTrashPanel" class="asset-meta__row">
          <span>删除时间</span>
          <span class="asset-meta__value">{{ entryDeletedAtLabel(currentFileEntry) || "未记录" }}</span>
        </div>
      </div>

      <FileMetadataEditor
        v-if="!isTrashPanel"
        :entry="currentFileEntry"
        :is-saving="isSavingMetadata"
        :available-tags="availableTags"
        :tag-groups="tagGroups"
        :save-metadata="saveMetadata"
      />
    </div>

    <div v-else class="files-detail__empty">
      <p class="asset-browser__eyebrow">{{ isReadOnlyVirtual ? "智能文件夹" : isTrashPanel ? "回收站" : "文件管理" }}</p>
      <h2>选择一个文件或文件夹</h2>
      <p>{{ isReadOnlyVirtual ? "在中间列表中选择目标，然后可执行查看、定位。" : isTrashPanel ? "在中间列表中选择目标，然后可执行还原或彻底删除。" : "在中间列表中选择目标，然后可执行查看、定位、重命名和删除。" }}</p>
    </div>
  </aside>
</template>
