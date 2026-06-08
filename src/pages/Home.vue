<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { convertFileSrc } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { UnlistenFn } from "@tauri-apps/api/event";
import {
  ArrowLeft,
  Eye,
  File,
  FileImage,
  FileVideo,
  Folder,
  FolderOpen,
  LoaderCircle,
  PencilLine,
  Plus,
  HardDrive,
  Files,
  Download,
  RefreshCw,
  Search,
  Trash2,
} from "lucide-vue-next";
import Markdown from "vue3-markdown-it";
import { useRepositoryWorkspace } from "../composables/useRepositoryWorkspace";
import { vContextMenu } from "../directives/contextMenu";
import { getPreviewPluginForEntry } from "../plugins/previewPlugins";
import type { FileBrowserEntry } from "../types/repository";

const createFileName = ref("");
const renameValue = ref("");
const renameTargetPath = ref<string | null>(null);
const isDraggingFiles = ref(false);
const isDraggingRepositoryFolder = ref(false);
const emptyRepositoryError = ref("");
const previewFilePath = ref<string | null>(null);
const extensionKeyword = ref("");

const {
  activePanel,
  activeSnapshot,
  activeRepoId,
  fileBrowser,
  plugins,
  repositories,
  searchQuery,
  selectedFilePath,
  searchResults,
  isBusy,
  isLoadingFileBrowser,
  isSearching,
  isMutatingFiles,
  error,
  ensureRepositoryWorkspace,
  refreshRepositoryWorkspace,
  selectRepository,
  selectAsset,
  loadFileBrowserForDirectory,
  createFileInWorkspace,
  attachRepository,
  importEntriesToWorkspace,
  renameWorkspaceEntry,
  deleteWorkspaceEntry,
  openWorkspaceEntry,
  revealWorkspaceEntry,
  selectWorkspaceEntry,
  removeRepository,
  exportCurrentRepository,
} = useRepositoryWorkspace();

const hasRepository = computed(() => Boolean(activeSnapshot.value));
const isLibrariesPanel = computed(() => activePanel.value === "libraries");
const isFilesPanel = computed(() => activePanel.value === "files");
const isSearchPanel = computed(() => activePanel.value === "search");
const isExtensionsPanel = computed(() => activePanel.value === "extensions");

const currentFileEntry = computed(() => (
  fileBrowser.value?.entries.find((entry) => entry.path === selectedFilePath.value) ?? null
));

const breadcrumbSegments = computed(() => {
  const currentPath = fileBrowser.value?.currentPath ?? "";
  const segments = currentPath ? currentPath.split("/") : [];
  return segments.map((segment, index) => ({
    label: segment,
    path: segments.slice(0, index + 1).join("/"),
  }));
});

const canRenameSelected = computed(() => Boolean(currentFileEntry.value));
const canPreviewSelected = computed(() => currentFileEntry.value?.kind === "file");
const canDeleteSelected = computed(() => currentFileEntry.value?.kind === "file");
const libraryOverview = computed(() => activeSnapshot.value?.overview ?? null);
const directoryEntries = computed(() => (fileBrowser.value?.entries ?? []).filter((entry) => entry.kind === "directory"));
const fileEntries = computed(() => (fileBrowser.value?.entries ?? []).filter((entry) => entry.kind === "file"));
const previewFileEntry = computed(() => (
  (fileBrowser.value?.entries ?? []).find((entry) => entry.path === previewFilePath.value && entry.kind === "file")
  ?? null
));
const previewPlugin = computed(() => getPreviewPluginForEntry(previewFileEntry.value));
const hasSplitFileGroups = computed(() => directoryEntries.value.length > 0 && fileEntries.value.length > 0);
const filteredPlugins = computed(() => {
  const keyword = extensionKeyword.value.trim().toLowerCase();
  if (!keyword) return plugins.value;
  return plugins.value.filter((plugin) => (
    plugin.name.toLowerCase().includes(keyword) ||
    plugin.description.toLowerCase().includes(keyword) ||
    plugin.kind.toLowerCase().includes(keyword) ||
    plugin.capabilities.some((capability) => capability.toLowerCase().includes(keyword))
  ));
});

watch(currentFileEntry, (entry) => {
  if (renameTargetPath.value && renameTargetPath.value !== entry?.path) {
    renameTargetPath.value = null;
    renameValue.value = "";
  }
});

watch(
  () => isFilesPanel.value,
  (enabled) => {
    if (enabled && activeRepoId.value && !fileBrowser.value) {
      void loadFileBrowserForDirectory("", { includeTree: true });
    }
  },
);

watch(selectedFilePath, (path) => {
  if (previewFilePath.value && previewFilePath.value !== path) {
    previewFilePath.value = null;
  }
});

function statusLabel(status: string) {
  switch (status) {
    case "synced":
      return "已同步";
    case "processing":
      return "处理中";
    case "indexed":
      return "已索引";
    case "deleted":
      return "已删除";
    case "ready":
      return "已同步";
    default:
      return status;
  }
}

function assetTone(extension: string) {
  const palette: Record<string, string> = {
    psd: "linear-gradient(135deg, #4e6d7c 0%, #24333a 100%)",
    png: "linear-gradient(135deg, #7c9e70 0%, #2f4734 100%)",
    webp: "linear-gradient(135deg, #d3b98e 0%, #7f5e44 100%)",
    svg: "linear-gradient(135deg, #8e9bb8 0%, #3c4964 100%)",
    mp4: "linear-gradient(135deg, #b76e5d 0%, #4f2b26 100%)",
    tif: "linear-gradient(135deg, #92958d 0%, #464840 100%)",
    jpg: "linear-gradient(135deg, #8ba8b6 0%, #35525f 100%)",
    fbx: "linear-gradient(135deg, #6c95a8 0%, #34424a 100%)",
    obj: "linear-gradient(135deg, #87916d 0%, #404738 100%)",
    glb: "linear-gradient(135deg, #a58f73 0%, #4c3d2f 100%)",
    gltf: "linear-gradient(135deg, #a58f73 0%, #4c3d2f 100%)",
  };

  return palette[extension.toLowerCase()] ?? "linear-gradient(135deg, #6f7788 0%, #2b313e 100%)";
}

function fileTone(entry: FileBrowserEntry) {
  if (entry.kind === "directory") {
    return "linear-gradient(135deg, #c7a566 0%, #73552f 100%)";
  }
  return assetTone(entry.extension ?? "");
}

function isVideoEntry(entry: FileBrowserEntry) {
  return ["mp4", "mov", "mkv", "webm", "avi", "m4v"].includes((entry.extension ?? "").toLowerCase());
}

function isModelEntry(entry: FileBrowserEntry) {
  return Boolean(getPreviewPluginForEntry(entry));
}

function thumbnailSrc(entry: FileBrowserEntry) {
  return entry.thumbnailPath ? convertFileSrc(entry.thumbnailPath) : null;
}

function openDirectory(path: string) {
  void loadFileBrowserForDirectory(path);
}

function selectFileEntry(entry: FileBrowserEntry) {
  if (entry.kind === "directory") {
    openDirectory(entry.path);
    return;
  }
  selectWorkspaceEntry(entry.path);
}

function previewFileEntryByDoubleClick(entry: FileBrowserEntry) {
  if (entry.kind !== "file") return;
  selectWorkspaceEntry(entry.path);
  previewFilePath.value = entry.path;
}

function exitPreview() {
  previewFilePath.value = null;
}

function getDroppedSourcePaths(event: DragEvent) {
  return Array.from(event.dataTransfer?.files ?? [])
    .map((file) => (file as File & { path?: string }).path ?? "")
    .filter((path) => path.trim().length > 0);
}

async function createRepositoryFromFolder(path: string) {
  const nextPath = path.trim();
  if (!nextPath) return;
  emptyRepositoryError.value = "";
  try {
    await attachRepository(nextPath);
  } catch (cause) {
    emptyRepositoryError.value = cause instanceof Error ? cause.message : String(cause);
  }
}

function handleDragOver(event: DragEvent) {
  if (!hasRepository.value || !isFilesPanel.value) return;
  event.preventDefault();
  if (event.dataTransfer) {
    event.dataTransfer.dropEffect = "copy";
  }
  isDraggingFiles.value = true;
}

function handleDragLeave(event: DragEvent) {
  const currentTarget = event.currentTarget as HTMLElement | null;
  const relatedTarget = event.relatedTarget as Node | null;
  if (currentTarget && relatedTarget && currentTarget.contains(relatedTarget)) return;
  isDraggingFiles.value = false;
}

async function handleDrop(event: DragEvent) {
  event.preventDefault();
  isDraggingFiles.value = false;
  const sourcePaths = getDroppedSourcePaths(event);
  if (!sourcePaths.length) return;
  await importEntriesToWorkspace(sourcePaths);
}

function handleEmptyRepositoryDragOver(event: DragEvent) {
  if (hasRepository.value) return;
  event.preventDefault();
  if (event.dataTransfer) {
    event.dataTransfer.dropEffect = "copy";
  }
  isDraggingRepositoryFolder.value = true;
}

function handleEmptyRepositoryDragLeave(event: DragEvent) {
  const currentTarget = event.currentTarget as HTMLElement | null;
  const relatedTarget = event.relatedTarget as Node | null;
  if (currentTarget && relatedTarget && currentTarget.contains(relatedTarget)) return;
  isDraggingRepositoryFolder.value = false;
}

async function handleEmptyRepositoryDrop(event: DragEvent) {
  event.preventDefault();
  isDraggingRepositoryFolder.value = false;
  const [path] = getDroppedSourcePaths(event);
  if (path) {
    await createRepositoryFromFolder(path);
  }
}

async function handleCreateFile() {
  if (!createFileName.value.trim()) return;
  const snapshot = await createFileInWorkspace(createFileName.value.trim());
  if (snapshot) {
    createFileName.value = "";
  }
}

function startRenameSelected() {
  if (!currentFileEntry.value) return;
  renameTargetPath.value = currentFileEntry.value.path;
  renameValue.value = currentFileEntry.value.name;
}

async function submitRenameSelected() {
  if (!renameTargetPath.value || !renameValue.value.trim()) return;
  const snapshot = await renameWorkspaceEntry(renameTargetPath.value, renameValue.value.trim());
  if (snapshot) {
    renameTargetPath.value = null;
    renameValue.value = "";
  }
}

async function deleteSelectedEntry() {
  if (!currentFileEntry.value) return;
  await deleteWorkspaceEntry(currentFileEntry.value.path);
}

async function deleteEntry(entry: FileBrowserEntry) {
  await deleteWorkspaceEntry(entry.path);
}

async function openSelectedEntry() {
  if (!currentFileEntry.value) return;
  await openWorkspaceEntry(currentFileEntry.value.path);
}

async function revealSelectedEntry() {
  if (!currentFileEntry.value) return;
  await revealWorkspaceEntry(currentFileEntry.value.path);
}

function fileEntryContextMenu(entry: FileBrowserEntry) {
  selectWorkspaceEntry(entry.path);
  return [
    {
      id: "preview",
      label: "预览",
      icon: Eye,
      disabled: entry.kind !== "file",
      onSelect: () => {
        if (entry.kind === "file") {
          previewFilePath.value = entry.path;
        }
      },
    },
    {
      id: "open",
      label: "打开",
      icon: Eye,
      disabled: entry.kind !== "file",
      onSelect: () => openWorkspaceEntry(entry.path),
    },
    {
      id: "reveal",
      label: "定位",
      icon: FolderOpen,
      onSelect: () => revealWorkspaceEntry(entry.path),
    },
    {
      id: "rename",
      label: "重命名",
      icon: PencilLine,
      onSelect: () => {
        renameTargetPath.value = entry.path;
        renameValue.value = entry.name;
      },
    },
    {
      id: "delete",
      label: "删除",
      icon: Trash2,
      danger: true,
      disabled: entry.kind !== "file" || isMutatingFiles.value,
      confirmLabel: "确认删除？再点一次",
      onSelect: () => deleteEntry(entry),
    },
  ];
}

function openSearchHit(repoId: string, assetId: string) {
  if (activeRepoId.value !== repoId) {
    void selectRepository(repoId).then(() => selectAsset(assetId));
    return;
  }
  void selectAsset(assetId);
}

function formatRepositoryStatus(status: string) {
  switch (status) {
    case "ready":
      return "已同步";
    case "readonly":
      return "只读";
    case "indexing":
      return "处理中";
    default:
      return status;
  }
}

function getAnchorFromElement(element: EventTarget | null) {
  if (!(element instanceof HTMLElement)) return null;
  const rect = element.getBoundingClientRect();
  return {
    left: rect.left,
    top: rect.top,
    right: rect.right,
    bottom: rect.bottom,
  };
}

function requestAddRepository(event?: MouseEvent) {
  window.dispatchEvent(new CustomEvent("momo:add-repository", {
    detail: {
      anchor: getAnchorFromElement(event?.currentTarget ?? null),
    },
  }));
}

const searchSummary = computed(() => {
  if (searchQuery.value.trim()) {
    return `当前查询: ${searchQuery.value}`;
  }
  return "输入关键词、标签或评分条件后，这里会展示跨仓库结果。";
});

onMounted(() => {
  void ensureRepositoryWorkspace();
  try {
    const currentWindow = getCurrentWindow();
    currentWindow.onDragDropEvent(({ payload }) => {
      if (!hasRepository.value) {
        if (payload.type === "enter" || payload.type === "over") {
          isDraggingRepositoryFolder.value = true;
          return;
        }
        if (payload.type === "leave") {
          isDraggingRepositoryFolder.value = false;
          return;
        }
        isDraggingRepositoryFolder.value = false;
        if (payload.paths.length) {
          void createRepositoryFromFolder(payload.paths[0]);
        }
        return;
      }
      if (!hasRepository.value || !isFilesPanel.value) return;
      if (payload.type === "enter" || payload.type === "over") {
        isDraggingFiles.value = true;
        return;
      }
      if (payload.type === "leave") {
        isDraggingFiles.value = false;
        return;
      }
      isDraggingFiles.value = false;
      if (payload.paths.length) {
        void importEntriesToWorkspace(payload.paths);
      }
    }).then((unlisten) => {
      dragDropUnlisten = unlisten;
    }).catch(() => {
      dragDropUnlisten = null;
    });
  } catch {
    dragDropUnlisten = null;
  }
});

let dragDropUnlisten: UnlistenFn | null = null;

onUnmounted(() => {
  dragDropUnlisten?.();
});
</script>

<template>
  <section v-if="hasRepository && isLibrariesPanel" class="library-overview">
    <div class="library-overview__panel">
      <header class="library-overview__header">
        <div>
          <p class="asset-browser__eyebrow">当前资源库</p>
          <h1>{{ activeSnapshot?.repository.name ?? "正在加载" }}</h1>
          <p class="library-overview__subline">
            {{ activeSnapshot?.repository.path }}
          </p>
        </div>
        <div class="library-overview__actions">
          <button type="button" class="ghost" @click="refreshRepositoryWorkspace">
            <RefreshCw :size="14" aria-hidden="true" />
            刷新资源库
          </button>
          <button type="button" class="primary" @click="requestAddRepository">
            <Plus :size="14" aria-hidden="true" />
            添加资源库
          </button>
        </div>
      </header>

      <div v-if="error" class="asset-browser__state asset-browser__state--error">
        {{ error }}
      </div>

      <div v-else-if="isBusy" class="asset-browser__state">
        <LoaderCircle class="spin" :size="16" aria-hidden="true" />
        正在加载资源库摘要
      </div>

      <template v-else>
        <div class="library-overview__stats">
          <article class="library-overview__stat-card">
            <span class="library-overview__stat-label">仓库名称</span>
            <strong>{{ activeSnapshot?.repository.name }}</strong>
          </article>
          <article class="library-overview__stat-card">
            <span class="library-overview__stat-label">总大小</span>
            <strong>{{ libraryOverview?.totalSizeLabel ?? "0 B" }}</strong>
            <span class="library-overview__stat-meta">
              <HardDrive :size="13" aria-hidden="true" />
              {{ libraryOverview?.totalSizeBytes ?? 0 }} Bytes
            </span>
          </article>
          <article class="library-overview__stat-card">
            <span class="library-overview__stat-label">文件个数</span>
            <strong>{{ libraryOverview?.fileCount ?? 0 }}</strong>
            <span class="library-overview__stat-meta">
              <Files :size="13" aria-hidden="true" />
              已索引文件
            </span>
          </article>
          <article class="library-overview__stat-card">
            <span class="library-overview__stat-label">文件夹个数</span>
            <strong>{{ libraryOverview?.folderCount ?? 0 }}</strong>
            <span class="library-overview__stat-meta">
              <Folder :size="13" aria-hidden="true" />
              不含内部元数据目录
            </span>
          </article>
        </div>

        <section class="library-overview__readme">
          <div class="library-overview__section-head">
            <div>
              <p class="asset-browser__eyebrow">README</p>
              <h2>根目录说明</h2>
            </div>
          </div>

          <div v-if="libraryOverview?.readmeContent" class="library-overview__readme-card">
            <Markdown :source="libraryOverview.readmeContent" />
          </div>
          <div v-else class="library-overview__empty">
            <h2>未发现 `readme.md`</h2>
            <p>如果资源库根目录存在 `readme.md` 或 `README.md`，这里会直接展示其内容。</p>
          </div>
        </section>

        <section class="library-manager">
          <div class="library-overview__section-head">
            <div>
              <p class="asset-browser__eyebrow">Repositories</p>
              <h2>资源库管理</h2>
            </div>
          </div>

          <div class="library-manager__list">
            <article
              v-for="library in repositories"
              :key="library.repoId"
              class="library-manager__item"
              :class="{ 'is-active': activeRepoId === library.repoId }"
            >
              <button
                type="button"
                class="library-manager__summary"
                @click="selectRepository(library.repoId)"
              >
                <div class="library-manager__title">
                  <strong>{{ library.name }}</strong>
                  <span>{{ library.assetCount }} 个资源</span>
                </div>
                <span class="library-manager__meta">{{ formatRepositoryStatus(library.status) }}</span>
                <span class="library-manager__path">{{ library.path }}</span>
              </button>

              <div class="library-manager__actions">
                <button
                  type="button"
                  class="ghost"
                  @click="activeRepoId === library.repoId ? exportCurrentRepository() : selectRepository(library.repoId).then(exportCurrentRepository)"
                >
                  <Download :size="14" aria-hidden="true" />
                  导出
                </button>
                <button
                  type="button"
                  class="ghost danger"
                  @click="removeRepository(library.repoId)"
                >
                  <Trash2 :size="14" aria-hidden="true" />
                  删除
                </button>
              </div>
            </article>
          </div>
        </section>
      </template>
    </div>
  </section>

  <section v-else-if="hasRepository && isFilesPanel" :class="previewFileEntry ? 'files-preview-page' : 'files-workbench'">
    <template v-if="previewFileEntry">
      <header class="files-preview-page__header">
        <button type="button" class="ghost files-preview-page__back" @click="exitPreview">
          <ArrowLeft :size="15" aria-hidden="true" />
          返回
        </button>
        <div>
          <p class="asset-browser__eyebrow">文件预览</p>
          <h1>{{ previewFileEntry.name }}</h1>
          <p class="files-preview-page__subline">{{ previewFileEntry.path }}</p>
        </div>
        <div class="files-preview-page__actions">
          <button type="button" class="ghost" @click="openWorkspaceEntry(previewFileEntry.path)">
            <Eye :size="14" aria-hidden="true" />
            打开
          </button>
          <button type="button" class="ghost" @click="revealWorkspaceEntry(previewFileEntry.path)">
            <FolderOpen :size="14" aria-hidden="true" />
            定位
          </button>
        </div>
      </header>

      <div class="files-preview-page__body">
        <div class="files-preview-page__preview" :class="{ 'files-preview-page__preview--plugin': previewPlugin }" :style="{ background: previewFileEntry.thumbnailPath || previewPlugin ? undefined : fileTone(previewFileEntry) }">
          <component
            :is="previewPlugin.component"
            v-if="previewPlugin"
            :entry="previewFileEntry"
            :repo-id="activeRepoId ?? ''"
          />
          <img v-else-if="thumbnailSrc(previewFileEntry)" :src="thumbnailSrc(previewFileEntry) ?? undefined" alt="" />
          <FileVideo v-else-if="isVideoEntry(previewFileEntry)" :size="54" aria-hidden="true" />
          <FileImage v-else :size="54" aria-hidden="true" />
        </div>
        <div class="files-detail__stats files-preview-page__stats">
          <div class="asset-meta__row">
            <span>类型</span>
            <span class="asset-meta__value">{{ previewFileEntry.extension || '文件' }}</span>
          </div>
          <div class="asset-meta__row">
            <span>大小</span>
            <span class="asset-meta__value">{{ previewFileEntry.sizeLabel || "未知" }}</span>
          </div>
          <div class="asset-meta__row">
            <span>状态</span>
            <span class="asset-meta__value">{{ previewFileEntry.status ? statusLabel(previewFileEntry.status) : "未索引" }}</span>
          </div>
          <div class="asset-meta__row">
            <span>修改时间</span>
            <span class="asset-meta__value">{{ previewFileEntry.modifiedAt ? new Date(previewFileEntry.modifiedAt).toLocaleString("zh-CN") : "未记录" }}</span>
          </div>
        </div>
      </div>
    </template>

    <template v-else>
    <div
      class="files-browser"
      :class="{ 'is-dragging': isDraggingFiles }"
      @dragover="handleDragOver"
      @dragleave="handleDragLeave"
      @drop="handleDrop"
    >
      <header class="files-browser__header">
        <div>
          <p class="asset-browser__eyebrow">当前目录</p>
          <div class="files-breadcrumbs">
            <button type="button" class="files-breadcrumbs__item" @click="openDirectory('')">根目录</button>
            <button v-for="segment in breadcrumbSegments" :key="segment.path" type="button" class="files-breadcrumbs__item" @click="openDirectory(segment.path)">
              {{ segment.label }}
            </button>
          </div>
        </div>

        <div class="files-toolbar">
          <label class="files-toolbar__field">
            <Plus :size="14" aria-hidden="true" />
            <input v-model="createFileName" type="text" placeholder="新建空文件，例如 note.txt" />
          </label>
          <button type="button" class="ghost files-toolbar__btn" :disabled="isMutatingFiles" @click="handleCreateFile">
            <File :size="14" aria-hidden="true" />
            建文件
          </button>
        </div>
      </header>

      <div v-if="error" class="asset-browser__state asset-browser__state--error">
        {{ error }}
      </div>

      <div v-else-if="isLoadingFileBrowser" class="asset-browser__state">
        <LoaderCircle class="spin" :size="16" aria-hidden="true" />
        正在读取目录
      </div>

      <template v-else>
        <div class="files-list">
          <button
            v-for="entry in directoryEntries"
            :key="entry.path"
            v-context-menu="() => fileEntryContextMenu(entry)"
            type="button"
            class="files-list__item"
            :class="{ 'is-active': selectedFilePath === entry.path }"
            @click="selectFileEntry(entry)"
          >
            <div class="files-list__preview" :style="{ background: entry.thumbnailPath ? undefined : fileTone(entry) }">
              <Folder :size="24" aria-hidden="true" />
            </div>
            <div class="files-list__body">
              <strong>{{ entry.name }}</strong>
            </div>
          </button>

          <div v-if="hasSplitFileGroups" class="files-list__divider" aria-hidden="true"></div>

          <button
            v-for="entry in fileEntries"
            :key="entry.path"
            v-context-menu="() => fileEntryContextMenu(entry)"
            type="button"
            class="files-list__item"
            :class="{ 'is-active': selectedFilePath === entry.path }"
            @click="selectFileEntry(entry)"
            @dblclick="previewFileEntryByDoubleClick(entry)"
          >
            <div class="files-list__preview" :style="{ background: entry.thumbnailPath ? undefined : fileTone(entry) }">
              <img v-if="thumbnailSrc(entry)" :src="thumbnailSrc(entry) ?? undefined" alt="" loading="lazy" />
              <FileVideo v-else-if="isVideoEntry(entry)" :size="24" aria-hidden="true" />
              <File v-else-if="isModelEntry(entry)" :size="24" aria-hidden="true" />
              <FileImage v-else :size="24" aria-hidden="true" />
            </div>
            <div class="files-list__body">
              <strong>{{ entry.name }}</strong>
            </div>
          </button>
        </div>
      </template>
    </div>

    <aside class="files-detail">
      <div v-if="currentFileEntry" class="files-detail__card">
        <div class="files-detail__preview" :style="{ background: currentFileEntry.thumbnailPath ? undefined : fileTone(currentFileEntry) }">
          <img v-if="thumbnailSrc(currentFileEntry)" :src="thumbnailSrc(currentFileEntry) ?? undefined" alt="" />
          <Folder v-else-if="currentFileEntry.kind === 'directory'" :size="34" aria-hidden="true" />
          <FileVideo v-else-if="isVideoEntry(currentFileEntry)" :size="34" aria-hidden="true" />
          <File v-else-if="isModelEntry(currentFileEntry)" :size="34" aria-hidden="true" />
          <FileImage v-else :size="34" aria-hidden="true" />
        </div>

        <div class="files-detail__section">
          <p class="asset-browser__eyebrow">选中项</p>
          <h2>{{ currentFileEntry.name }}</h2>
          <p class="files-detail__subline">{{ currentFileEntry.path }}</p>
        </div>

        <div class="files-detail__section">
          <div class="files-detail__actions">
            <button type="button" class="ghost" :disabled="!canPreviewSelected" @click="openSelectedEntry">
              <Eye :size="14" aria-hidden="true" />
              查看
            </button>
            <button type="button" class="ghost" @click="revealSelectedEntry">
              <FolderOpen :size="14" aria-hidden="true" />
              定位
            </button>
            <button type="button" class="ghost" :disabled="!canRenameSelected" @click="startRenameSelected">
              <PencilLine :size="14" aria-hidden="true" />
              重命名
            </button>
            <button type="button" class="ghost danger" :disabled="isMutatingFiles || !canDeleteSelected" @click="deleteSelectedEntry">
              <File :size="14" aria-hidden="true" />
              删除
            </button>
          </div>
          <p v-if="currentFileEntry.kind === 'directory'" class="files-detail__hint">
            文件夹删除请在左侧文件夹树中操作，以选择删除内容或转移到上级目录。
          </p>
        </div>

        <div v-if="renameTargetPath === currentFileEntry.path" class="files-detail__section">
          <p class="asset-browser__eyebrow">重命名</p>
          <div class="files-detail__rename">
            <input v-model="renameValue" type="text" />
            <button type="button" :disabled="isMutatingFiles" @click="submitRenameSelected">
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
          <div class="asset-meta__row">
            <span>修改时间</span>
            <span class="asset-meta__value">{{ currentFileEntry.modifiedAt ? new Date(currentFileEntry.modifiedAt).toLocaleString("zh-CN") : "未记录" }}</span>
          </div>
        </div>
      </div>

      <div v-else class="files-detail__empty">
        <p class="asset-browser__eyebrow">文件管理</p>
        <h2>选择一个文件或文件夹</h2>
        <p>在中间列表中选择目标，然后可执行查看、定位、重命名和删除。</p>
      </div>
    </aside>
    </template>
  </section>

  <section v-else-if="isSearchPanel" class="search-workbench">
    <div class="search-workbench__panel">
      <header class="search-workbench__header">
        <div>
          <p class="asset-browser__eyebrow">全局搜索</p>
          <h1>搜索结果</h1>
          <p class="search-workbench__subline">{{ searchSummary }}</p>
        </div>
        <div class="search-workbench__stats">
          <span class="asset-stat">{{ repositories.length }} 个仓库</span>
          <span class="asset-stat">{{ searchResults.length }} 条结果</span>
        </div>
      </header>

      <div v-if="isSearching" class="asset-browser__state">
        <LoaderCircle class="spin" :size="16" aria-hidden="true" />
        正在执行全局搜索
      </div>

      <div v-if="!repositories.length" class="search-workbench__empty">
        <h2>还没有可搜索的资源库</h2>
        <p>先在资源库页面添加一个仓库，再执行跨仓库搜索。</p>
      </div>

      <div v-else-if="!isSearching && !searchResults.length" class="search-workbench__empty">
        <h2>等待搜索条件</h2>
        <p>输入关键词、标签或评分条件后，这里会展示结果。</p>
      </div>

      <div v-else class="search-workbench__results">
        <button
          v-for="result in searchResults"
          :key="`${result.repoId}:${result.assetId}`"
          type="button"
          class="search-workbench__item"
          @click="openSearchHit(result.repoId, result.assetId)"
        >
          <div class="search-workbench__item-icon">
            <FileImage :size="18" aria-hidden="true" />
          </div>
          <div class="search-workbench__item-body">
            <strong>{{ result.filename }}</strong>
            <span>{{ result.repoName }} / {{ result.path }}</span>
          </div>
        </button>
      </div>
    </div>
  </section>

  <section v-else-if="isExtensionsPanel" class="extensions-workbench">
    <div class="search-workbench__panel">
      <header class="search-workbench__header">
        <div>
          <p class="asset-browser__eyebrow">拓展能力</p>
          <h1>文件系统与插件</h1>
          <p class="search-workbench__subline">这里集中展示当前插件和后端能力。</p>
        </div>
        <div class="search-workbench__stats">
          <span class="asset-stat">{{ filteredPlugins.length }} 个插件</span>
        </div>
      </header>

      <label class="search-workbench__field">
        <Search :size="15" aria-hidden="true" />
        <input
          v-model="extensionKeyword"
          type="search"
          placeholder="筛选导入器、脚本或元数据拓展"
        />
      </label>

      <div class="extensions-workbench__list">
        <article v-for="plugin in filteredPlugins" :key="plugin.pluginId" class="extensions-workbench__card">
          <div class="extensions-workbench__card-head">
            <strong>{{ plugin.name }}</strong>
            <span class="asset-card__pill" :class="{ 'asset-card__pill--ghost': !plugin.enabled }">
              {{ plugin.enabled ? "已启用" : "未启用" }}
            </span>
          </div>
          <p class="extensions-workbench__card-desc">{{ plugin.description }}</p>
          <div class="settings-list__chips">
            <span class="workspace-hints__chip">{{ plugin.kind }}</span>
            <span v-for="capability in plugin.capabilities" :key="capability" class="workspace-hints__chip">
              {{ capability }}
            </span>
          </div>
        </article>
      </div>
    </div>
  </section>

  <section
    v-else
    class="empty-state-page"
    :class="{ 'is-dragging': isDraggingRepositoryFolder }"
    @dragover="handleEmptyRepositoryDragOver"
    @dragleave="handleEmptyRepositoryDragLeave"
    @drop="handleEmptyRepositoryDrop"
  >
    <div class="empty-state-page__panel">
      <h1>还没有可用资源库</h1>
      <p>拖入一个本地文件夹创建资源库。</p>
      <p v-if="emptyRepositoryError" class="empty-state-page__error">
        {{ emptyRepositoryError }}
      </p>
    </div>
  </section>
</template>
