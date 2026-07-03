<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from "vue";
import {
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
} from "@lucide/vue";
import FileMetadataEditor from "./FileMetadataEditor.vue";
import ThumbnailPalette from "../../../components/ThumbnailPalette.vue";
import type { ContextMenuItem } from "../../../ui/core";
import { vContextMenu } from "../../../ui/core";
import type { RegisteredLibraryExtension } from "../../../plugins/sdk";
import type { FileBrowserEntry, RepositoryTagGroup } from "../../../types/repository";
import { useFileBrowserPanelViewModel } from "./useFileBrowserPanelViewModel";
import { entryDisplayTitle } from "./filePresentation";

type FileDisplayMode = "adaptive" | "masonry" | "grid" | "list";
type BreadcrumbSegment = {
  label: string;
  path: string;
};
type SelectionMode = "replace" | "toggle" | "range";
type BoxSelectionMode = "replace" | "append";

const props = defineProps<{
  breadcrumbs: BreadcrumbSegment[];
  canDragEntries: boolean;
  canDeleteSelected: boolean;
  canImport?: boolean;
  canOpenSelected: boolean;
  canRenameSelected: boolean;
  canRestoreSelected: boolean;
  canClearRecentHistory?: boolean;
  currentFileEntry: FileBrowserEntry | null;
  currentDirectoryDisplayName: string;
  currentDirectoryPath: string;
  allEntries: FileBrowserEntry[];
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
  isLoadingFileBrowserMore?: boolean;
  isModelEntry: (entry: FileBrowserEntry) => boolean;
  isMutatingFiles: boolean;
  isReadOnlyVirtual?: boolean;
  isRecentView?: boolean;
  isClearingRecentHistory?: boolean;
  isTrashPanel: boolean;
  isVirtualView?: boolean;
  isVideoEntry: (entry: FileBrowserEntry) => boolean;
  hasMoreEntries?: boolean;
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
  libraryExtensions: RegisteredLibraryExtension[];
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
  clearRecentHistory: [];
  deleteSelected: [];
  dragLeave: [event: DragEvent];
  dragOver: [event: DragEvent];
  drop: [event: DragEvent];
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
  visibleEntriesChange: [entries: FileBrowserEntry[]];
}>();

const filesListRef = ref<HTMLElement | null>(null);
const importMenuRef = ref<HTMLElement | null>(null);
const importMenuOpen = ref(false);
const showEagleImportActions = ref(false);
const {
  dropTargetPathSet,
  multiSelectionSummary,
  selectedPathSet,
  selectionBoxStyle,
  cancelEntryDragIntent,
  clearBoxSelection,
  clearEntryDragIntent,
  handleEntryClick,
  handleEntryDoubleClick,
  handleEntryPointerDown,
  handleEntryPointerMove,
  handleListPointerDown,
  librarySummary,
  updateBoxSelection,
} = useFileBrowserPanelViewModel({
  filesListRef,
  props,
  emit,
});

function closeImportMenu() {
  importMenuOpen.value = false;
  showEagleImportActions.value = false;
}

function toggleImportMenu() {
  if (!props.canImport) return;
  importMenuOpen.value = !importMenuOpen.value;
  if (!importMenuOpen.value) {
    showEagleImportActions.value = false;
  }
}

function toggleEagleImportActions() {
  showEagleImportActions.value = !showEagleImportActions.value;
}

function handleGlobalPointerDown(event: PointerEvent) {
  const target = event.target;
  if (!(target instanceof Node)) return;
  if (importMenuRef.value?.contains(target)) return;
  closeImportMenu();
}

function triggerImportFolder() {
  closeImportMenu();
  emit("importFolder");
}

function triggerImportZip() {
  closeImportMenu();
  emit("importZip");
}

function triggerImportEagleCopy() {
  closeImportMenu();
  emit("importEagleCopy");
}

function triggerImportEagleMove() {
  closeImportMenu();
  emit("importEagleMove");
}

onMounted(() => {
  window.addEventListener("pointerdown", handleGlobalPointerDown);
});

onBeforeUnmount(() => {
  window.removeEventListener("pointerdown", handleGlobalPointerDown);
});
</script>

<template>
  <div class="files-workbench__main">
    <div
      class="files-browser"
      :class="{ 'is-dragging': isDraggingFiles }"
      @dragover="emit('dragOver', $event)"
      @dragleave="emit('dragLeave', $event)"
      @drop="emit('drop', $event)"
    >
      <header class="files-browser__header">
        <div>
          <p class="asset-browser__eyebrow">{{ isVirtualView ? "分类视图" : isTrashPanel ? "回收站" : "当前目录" }}</p>
          <div class="files-breadcrumbs">
            <button
              type="button"
              class="files-breadcrumbs__item"
              :disabled="isVirtualView"
              @click="emit('openDirectory', '')"
            >
              {{ isVirtualView ? virtualTitle || "分类视图" : isTrashPanel ? "回收站" : "根目录" }}
            </button>
            <button
              v-if="!isVirtualView"
              v-for="segment in breadcrumbs"
              :key="segment.path"
              type="button"
              class="files-breadcrumbs__item"
              @click="emit('openDirectory', segment.path)"
            >
              {{ segment.label }}
            </button>
          </div>
          <p v-if="isVirtualView && virtualSubline" class="files-browser__subline">{{ virtualSubline }}</p>
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
          <button
            v-if="isRecentView"
            type="button"
            class="ghost files-toolbar__btn"
            :disabled="!canClearRecentHistory || isClearingRecentHistory"
            @click="emit('clearRecentHistory')"
          >
            <Trash2 :size="14" aria-hidden="true" />
            {{ isClearingRecentHistory ? "清空中..." : "清空记录" }}
          </button>

          <template v-if="!isTrashPanel && !isVirtualView">
            <label class="files-toolbar__field">
              <Plus :size="14" aria-hidden="true" />
              <input v-model="createFileName" type="text" placeholder="新建空文件，例如 note.txt" />
            </label>
            <button type="button" class="ghost files-toolbar__btn" :disabled="isMutatingFiles" @click="emit('createFile')">
              <File :size="14" aria-hidden="true" />
              建文件
            </button>
            <div ref="importMenuRef" class="files-toolbar__import">
              <button
                type="button"
                class="ghost files-toolbar__btn"
                :disabled="isMutatingFiles || !canImport"
                @click="toggleImportMenu"
              >
                <FolderOpen :size="14" aria-hidden="true" />
                导入
              </button>
              <div v-if="importMenuOpen" class="files-toolbar__menu">
                <button type="button" class="files-toolbar__menu-item" :disabled="isMutatingFiles" @click="triggerImportFolder">
                  从文件夹导入
                </button>
                <button type="button" class="files-toolbar__menu-item" :disabled="isMutatingFiles" @click="triggerImportZip">
                  从 ZIP 导入
                </button>
                <button type="button" class="files-toolbar__menu-item" :disabled="isMutatingFiles" @click="toggleEagleImportActions">
                  从 Eagle 导入
                </button>
                <div v-if="showEagleImportActions" class="files-toolbar__menu-subactions">
                  <button type="button" class="files-toolbar__menu-item" :disabled="isMutatingFiles" @click="triggerImportEagleCopy">
                    复制导入
                  </button>
                  <button type="button" class="files-toolbar__menu-item" :disabled="isMutatingFiles" @click="triggerImportEagleMove">
                    剪切导入
                  </button>
                </div>
              </div>
            </div>
          </template>

          <template v-else-if="isTrashPanel && !isReadOnlyVirtual">
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
          <div v-if="directoryEntries.length" class="files-list__group files-list__directories" :class="displayModeClass">
            <button
              v-for="entry in directoryEntries"
              :key="entry.path"
              v-context-menu="() => fileEntryContextMenu(entry)"
              type="button"
              class="files-list__item files-list__item--file files-list__item--directory"
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
                <Folder v-else :size="24" aria-hidden="true" />
              </div>
              <div class="files-list__body">
                <strong>{{ entryDisplayTitle(entry) }}</strong>
                <span v-if="fileDisplayMode === 'list'">{{ entry.path }}</span>
              </div>
              <div v-if="fileDisplayMode === 'list'" class="files-list__meta">
                <span>文件夹</span>
                <span>{{ entry.sizeLabel || "目录项" }}</span>
                <span>{{ entryModifiedAtLabel(entry) }}</span>
              </div>
            </button>
          </div>

          <div
            v-if="hasSplitFileGroups && directoryEntries.length && fileEntries.length"
            class="files-list__divider"
            aria-hidden="true"
          ></div>

          <div v-if="fileEntries.length" class="files-list__group files-list__files" :class="displayModeClass">
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
                <strong>{{ entryDisplayTitle(entry) }}</strong>
                <span v-if="hardlinkStateLabel(entry) && fileDisplayMode !== 'list'">{{ hardlinkStateLabel(entry) }}</span>
                <span v-if="librarySummary(entry)?.inline && fileDisplayMode !== 'list'" class="files-list__library-line">{{ librarySummary(entry)?.inline }}</span>
                <span v-if="fileDisplayMode === 'list'">{{ entry.path }}</span>
              </div>
              <div v-if="fileDisplayMode === 'list'" class="files-list__meta">
                <span v-if="librarySummary(entry)?.inline" class="files-list__library-list-label">{{ librarySummary(entry)?.inline }}</span>
                <span>{{ entry.extension || '文件' }}</span>
                <span>{{ entry.sizeLabel || "未知" }}</span>
                <span v-if="hardlinkStateLabel(entry)">{{ hardlinkStateLabel(entry) }}</span>
                <span>{{ entryModifiedAtLabel(entry) }}</span>
              </div>
            </button>
          </div>

          <div
            v-if="hasMoreEntries || isLoadingFileBrowserMore"
            class="files-list__load-more"
            data-load-more-sentinel="true"
          >
            <LoaderCircle v-if="isLoadingFileBrowserMore" class="spin" :size="14" aria-hidden="true" />
            <span>{{ isLoadingFileBrowserMore ? "继续读取目录..." : "滚动继续加载" }}</span>
          </div>

          <div v-if="selectionBoxStyle" class="files-list__selection-box" :style="selectionBoxStyle"></div>
        </div>
      </template>
    </div>

    <slot name="player"></slot>
  </div>

  <aside class="files-detail">
    <div v-if="selectedEntries.length > 1" class="files-detail__card">
      <div class="files-detail__section">
        <h2>{{ selectedEntries.length }} 个项目</h2>
        <p class="files-detail__subline">{{ multiSelectionSummary }}</p>
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
        <h2>{{ entryDisplayTitle(currentFileEntry) }}</h2>
        <p v-if="currentFileEntry.path !== currentFileEntry.name" class="files-detail__subline">
          {{ currentFileEntry.path }}
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
        <template v-if="librarySummary(currentFileEntry)?.rows?.length">
          <div v-for="row in librarySummary(currentFileEntry)?.rows" :key="`${row.label}:${row.value}`" class="asset-meta__row">
            <span>{{ row.label }}</span>
            <span class="asset-meta__value">{{ row.value }}</span>
          </div>
        </template>
      </div>

      <FileMetadataEditor
        v-if="!isTrashPanel"
        :entry="currentFileEntry"
        :is-saving="isSavingMetadata"
        :available-tags="availableTags"
        :tag-groups="tagGroups"
        :library-extensions="libraryExtensions"
        :playlist-entries="fileEntries"
        :save-metadata="saveMetadata"
      />
    </div>

    <div v-else-if="!isReadOnlyVirtual && !isTrashPanel && !isVirtualView" class="files-detail__card">
      <div class="files-detail__section">
        <h2>{{ currentDirectoryDisplayName }}</h2>
        <p class="files-detail__subline">{{ currentDirectoryPath || "根目录" }}</p>
      </div>
      <div class="files-detail__stats">
        <div class="asset-meta__row">
          <span>直属文件</span>
          <span class="asset-meta__value">{{ fileEntries.length }}</span>
        </div>
        <div class="asset-meta__row">
          <span>直属子文件夹</span>
          <span class="asset-meta__value">{{ directoryEntries.length }}</span>
        </div>
        <div class="asset-meta__row">
          <span>当前视图总条目</span>
          <span class="asset-meta__value">{{ fileEntries.length + directoryEntries.length }}</span>
        </div>
      </div>
    </div>

    <div v-else class="files-detail__empty">
      <p class="asset-browser__eyebrow">{{ isReadOnlyVirtual ? "智能文件夹" : isTrashPanel ? "回收站" : "文件管理" }}</p>
      <h2>选择一个文件或文件夹</h2>
      <p>{{ isReadOnlyVirtual ? "在中间列表中选择目标查看详情。" : isTrashPanel ? "在中间列表中选择目标，然后可执行还原或彻底删除。" : "在中间列表中选择目标查看详情。" }}</p>
    </div>
  </aside>
</template>

<style scoped>
.files-toolbar__import {
  position: relative;
}

.files-toolbar__menu {
  position: absolute;
  top: calc(100% + 8px);
  right: 0;
  min-width: 184px;
  display: grid;
  gap: 6px;
  padding: 8px;
  border-radius: 12px;
  border: 1px solid var(--panel-border, rgba(255, 255, 255, 0.08));
  background: var(--panel-bg, #15171a);
  box-shadow: 0 16px 32px rgba(0, 0, 0, 0.24);
  z-index: 20;
}

.files-toolbar__menu-item {
  width: 100%;
  padding: 10px 12px;
  border: 0;
  border-radius: 10px;
  background: transparent;
  color: inherit;
  text-align: left;
  cursor: pointer;
}

.files-toolbar__menu-item:hover:enabled {
  background: rgba(255, 255, 255, 0.06);
}

.files-toolbar__menu-item:disabled {
  cursor: not-allowed;
  opacity: 0.55;
}

.files-toolbar__menu-subactions {
  display: grid;
  gap: 6px;
  padding-top: 2px;
}
</style>
