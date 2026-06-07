<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import {
  Eye,
  File,
  FileImage,
  Folder,
  FolderOpen,
  LoaderCircle,
  PencilLine,
  Plus,
  HardDrive,
  Files,
} from "lucide-vue-next";
import Markdown from "vue3-markdown-it";
import { useRepositoryWorkspace } from "../composables/useRepositoryWorkspace";
import type { FileBrowserEntry } from "../types/repository";

const createFileName = ref("");
const renameValue = ref("");
const renameTargetPath = ref<string | null>(null);

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
  isMutatingFiles,
  error,
  ensureRepositoryWorkspace,
  selectRepository,
  selectAsset,
  loadFileBrowserForDirectory,
  createFileInWorkspace,
  renameWorkspaceEntry,
  deleteWorkspaceEntry,
  openWorkspaceEntry,
  revealWorkspaceEntry,
  selectWorkspaceEntry,
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
      void loadFileBrowserForDirectory("");
    }
  },
);

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
  };

  return palette[extension.toLowerCase()] ?? "linear-gradient(135deg, #6f7788 0%, #2b313e 100%)";
}

function fileTone(entry: FileBrowserEntry) {
  if (entry.kind === "directory") {
    return "linear-gradient(135deg, #c7a566 0%, #73552f 100%)";
  }
  return assetTone(entry.extension ?? "");
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

async function openSelectedEntry() {
  if (!currentFileEntry.value) return;
  await openWorkspaceEntry(currentFileEntry.value.path);
}

async function revealSelectedEntry() {
  if (!currentFileEntry.value) return;
  await revealWorkspaceEntry(currentFileEntry.value.path);
}

function openSearchHit(repoId: string, assetId: string) {
  if (activeRepoId.value !== repoId) {
    void selectRepository(repoId).then(() => selectAsset(assetId));
    return;
  }
  void selectAsset(assetId);
}

const searchSummary = computed(() => {
  if (searchQuery.value.trim()) {
    return `当前查询: ${searchQuery.value}`;
  }
  return "在左侧输入关键词、标签或评分条件后，这里会展示跨仓库结果。";
});

onMounted(() => {
  void ensureRepositoryWorkspace();
});
</script>

<template>
  <section v-if="hasRepository && isLibrariesPanel" class="library-overview">
    <div class="library-overview__panel">
      <header class="library-overview__header">
        <div>
          <p class="asset-browser__eyebrow">当前资源库</p>
          <h1>{{ activeSnapshot?.repository.name ?? "资源库" }}</h1>
          <p class="library-overview__subline">
            {{ activeSnapshot?.repository.path }}
          </p>
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
      </template>
    </div>
  </section>

  <section v-else-if="hasRepository && isFilesPanel" class="files-workbench">
    <div class="files-browser">
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
            v-for="entry in fileBrowser?.entries ?? []"
            :key="entry.path"
            type="button"
            class="files-list__item"
            :class="{ 'is-active': selectedFilePath === entry.path }"
            @click="selectFileEntry(entry)"
          >
            <div class="files-list__icon" :style="{ background: fileTone(entry) }">
              <Folder v-if="entry.kind === 'directory'" :size="18" aria-hidden="true" />
              <FileImage v-else :size="18" aria-hidden="true" />
            </div>
            <div class="files-list__body">
              <strong>{{ entry.name }}</strong>
              <span>{{ entry.kind === 'directory' ? '文件夹' : entry.sizeLabel || '文件' }}</span>
            </div>
            <div class="files-list__meta">
              <span v-if="entry.status" class="asset-card__pill asset-card__pill--ghost">{{ statusLabel(entry.status) }}</span>
              <span>{{ entry.modifiedAt ? new Date(entry.modifiedAt).toLocaleString("zh-CN") : "未记录" }}</span>
            </div>
          </button>
        </div>
      </template>
    </div>

    <aside class="files-detail">
      <div v-if="currentFileEntry" class="files-detail__card">
        <div class="files-detail__preview" :style="{ background: fileTone(currentFileEntry) }">
          <Folder v-if="currentFileEntry.kind === 'directory'" :size="34" aria-hidden="true" />
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

      <div v-if="!repositories.length" class="search-workbench__empty">
        <h2>还没有可搜索的资源库</h2>
        <p>先在左侧“资源库列表”添加一个仓库，再执行跨仓库搜索。</p>
      </div>

      <div v-else-if="!searchResults.length" class="search-workbench__empty">
        <h2>等待搜索条件</h2>
        <p>在左侧搜索面板输入关键词、标签或评分条件，主界面会同步展示结果。</p>
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
          <p class="search-workbench__subline">侧栏切换会同步切主界面，这里集中展示当前插件和后端能力。</p>
        </div>
        <div class="search-workbench__stats">
          <span class="asset-stat">{{ plugins.length }} 个插件</span>
        </div>
      </header>

      <div class="extensions-workbench__list">
        <article v-for="plugin in plugins" :key="plugin.pluginId" class="extensions-workbench__card">
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

  <section v-else class="empty-state-page">
    <div class="empty-state-card">
      <p class="asset-browser__eyebrow">
        {{ isExtensionsPanel ? "拓展能力" : isSearchPanel ? "全局搜索" : "资源仓库" }}
      </p>
      <h1>
        {{
          isExtensionsPanel
            ? "当前没有可展示的拓展能力"
            : isSearchPanel
              ? "还没有可搜索的资源库"
              : "还没有可用资源库"
        }}
      </h1>
      <p v-if="isSearchPanel">先在左侧“资源库列表”添加资源库，再执行跨仓库搜索。</p>
      <p v-else-if="isExtensionsPanel">先加载插件与文件系统后端，主界面会展示当前拓展能力。</p>
      <p v-else>在左侧“资源库列表”点击 `+` 选择文件夹。包含 `.momo` 的文件夹会自动导入，否则会原地初始化为新资源库。</p>
    </div>
  </section>
</template>
