<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { open } from "@tauri-apps/plugin-dialog";
import { RouterLink, useRoute, useRouter } from "vue-router";
import {
  Archive,
  FolderOpen,
  FolderTree,
  Library,
  LoaderCircle,
  Plus,
  Puzzle,
  RefreshCw,
  Search,
  Settings,
  Tag,
  Trash2,
} from "lucide-vue-next";
import FolderTreeNode from "../components/FolderTreeNode.vue";
import Dropdown from "../components/Dropdown.vue";
import { useRepositoryWorkspace, type WorkspacePanelKey } from "../composables/useRepositoryWorkspace";
import type { FileDeleteMode } from "../types/repository";

type PanelKey = Exclude<WorkspacePanelKey, "files">;

const showBackendDialog = ref(false);
const backendPluginId = ref("builtin.local-filesystem");
const backendName = ref("");
const backendPath = ref("");
const backendUrl = ref("");
const backendUsername = ref("");
const backendPassword = ref("");
const backendRoot = ref("");
const isSubmittingBackend = ref(false);
const expandedFolderPaths = ref<string[]>([]);
const showFolderDialog = ref(false);
const folderDialogMode = ref<"create" | "rename">("create");
const folderDialogParentPath = ref("");
const folderDialogTargetPath = ref("");
const folderDialogLabel = ref("");
const folderDialogValue = ref("");
const showFolderDeleteDialog = ref(false);
const pendingDeleteFolderPath = ref("");
const pendingDeleteFolderLabel = ref("");
const route = useRoute();
const router = useRouter();

const {
  repositories,
  repositoryBackendOptions,
  activePanel,
  activeRepoId,
  activeSnapshot,
  currentDirectoryPath,
  fileTree,
  isBusy,
  isLoadingFileBrowser,
  isMutatingFiles,
  error,
  ensureRepositoryWorkspace,
  refreshFileBrowserTree,
  selectRepository,
  setActivePanel,
  loadFileBrowserForDirectory,
  createDirectoryInWorkspace,
  renameWorkspaceEntry,
  deleteWorkspaceEntry,
  createNewRepository,
} = useRepositoryWorkspace();

const primaryNav = [
  { key: "libraries" as const, label: "资源库", icon: Library },
  { key: "search" as const, label: "搜索", icon: Search },
  { key: "extensions" as const, label: "拓展", icon: Puzzle },
];

const shortcuts = computed(() => {
  const assets = activeSnapshot.value?.assets ?? [];
  return [
    { id: "all", label: "全部", count: assets.length, icon: Archive },
    { id: "processing", label: "处理中", count: assets.filter((item) => item.status === "processing").length, icon: FolderTree },
    { id: "untagged", label: "未标签", count: assets.filter((item) => item.tags.length === 0).length, icon: Tag },
    { id: "deleted", label: "已删除", count: 0, icon: Trash2 },
  ];
});

const activeRepository = computed(() => (
  repositories.value.find((item) => item.repoId === activeRepoId.value) ?? null
));
const expandedFolderPathSet = computed(() => new Set(expandedFolderPaths.value));
const fileTreeNodes = computed(() => fileTree.value);
const backendOptions = computed(() => repositoryBackendOptions.value.map((item) => ({
  value: item.pluginId,
  label: item.name,
  hint: item.enabled ? item.description : `${item.description}（未启用）`,
})));
const selectedBackend = computed(() => (
  repositoryBackendOptions.value.find((item) => item.pluginId === backendPluginId.value)
  ?? repositoryBackendOptions.value[0]
  ?? null
));
const isLocalBackend = computed(() => backendPluginId.value === "builtin.local-filesystem");
const backendSubmitDisabled = computed(() => {
  if (!selectedBackend.value?.enabled) {
    return true;
  }
  if (isLocalBackend.value) {
    return !backendPath.value.trim();
  }
  return !backendUrl.value.trim();
});
const folderDialogTitle = computed(() => (
  folderDialogMode.value === "create" ? "新建文件夹" : "重命名文件夹"
));
const folderDialogActionLabel = computed(() => (
  folderDialogMode.value === "create" ? "创建" : "保存"
));
const folderDialogPlaceholder = computed(() => (
  folderDialogMode.value === "create" ? "输入文件夹名称" : "输入新的文件夹名称"
));
const folderDialogDisabled = computed(() => !folderDialogValue.value.trim() || isMutatingFiles.value);

watch(
  fileTreeNodes,
  (nodes) => {
    const validPaths = new Set<string>([""]);
    const collectPaths = (items: typeof nodes) => {
      for (const item of items) {
        validPaths.add(item.path);
        collectPaths(item.children);
      }
    };
    collectPaths(nodes);
    expandedFolderPaths.value = expandedFolderPaths.value.filter((path) => validPaths.has(path));
  },
  { deep: true },
);

watch(
  currentDirectoryPath,
  (path) => {
    if (path == null) return;
    const segments = path ? path.split("/") : [];
    const nextExpanded = new Set(expandedFolderPaths.value);
    let cursor = "";
    for (const segment of segments) {
      cursor = cursor ? `${cursor}/${segment}` : segment;
      nextExpanded.add(cursor);
    }
    expandedFolderPaths.value = Array.from(nextExpanded);
  },
  { immediate: true },
);

function selectPanel(next: PanelKey) {
  setActivePanel(next);
  if (route.path === "/settings") {
    void router.push("/");
  }
}

function selectRepositoryFromList(repoId: string) {
  void selectRepository(repoId).then(() => {
    if (route.path === "/settings") {
      void router.push("/");
    }
  });
}

function repositoryInitial(name: string) {
  return name.trim().slice(0, 2).toUpperCase() || "库";
}

function openBackendDialog() {
  backendPluginId.value = repositoryBackendOptions.value[0]?.pluginId ?? "builtin.local-filesystem";
  backendName.value = "";
  backendPath.value = "";
  backendUrl.value = "";
  backendUsername.value = "";
  backendPassword.value = "";
  backendRoot.value = "";
  showBackendDialog.value = true;
}

function closeBackendDialog() {
  if (isSubmittingBackend.value) return;
  showBackendDialog.value = false;
}

function toggleFolderExpansion(path: string) {
  const next = new Set(expandedFolderPaths.value);
  if (next.has(path)) {
    next.delete(path);
  } else {
    next.add(path);
  }
  expandedFolderPaths.value = Array.from(next);
}

function openFolder(path: string) {
  setActivePanel("files");
  void loadFileBrowserForDirectory(path);
}

function openCreateFolderDialog(parentPath = "") {
  folderDialogMode.value = "create";
  folderDialogParentPath.value = parentPath;
  folderDialogTargetPath.value = "";
  folderDialogLabel.value = "";
  folderDialogValue.value = "";
  showFolderDialog.value = true;
}

function openRenameFolderDialog(path: string, label: string) {
  folderDialogMode.value = "rename";
  folderDialogParentPath.value = "";
  folderDialogTargetPath.value = path;
  folderDialogLabel.value = label;
  folderDialogValue.value = label;
  showFolderDialog.value = true;
}

function closeFolderDialog() {
  if (isMutatingFiles.value) return;
  showFolderDialog.value = false;
}

async function submitFolderDialog() {
  const value = folderDialogValue.value.trim();
  if (!value) return;

  if (folderDialogMode.value === "create") {
    const snapshot = await createDirectoryInWorkspace(value, folderDialogParentPath.value);
    if (snapshot) {
      if (folderDialogParentPath.value) {
        const next = new Set(expandedFolderPaths.value);
        next.add(folderDialogParentPath.value);
        expandedFolderPaths.value = Array.from(next);
      }
      showFolderDialog.value = false;
    }
    return;
  }

  const snapshot = await renameWorkspaceEntry(folderDialogTargetPath.value, value);
  if (snapshot) {
    showFolderDialog.value = false;
  }
}

function openDeleteFolderDialog(path: string, label: string) {
  pendingDeleteFolderPath.value = path;
  pendingDeleteFolderLabel.value = label;
  showFolderDeleteDialog.value = true;
}

function closeDeleteFolderDialog() {
  if (isMutatingFiles.value) return;
  showFolderDeleteDialog.value = false;
}

async function confirmDeleteFolder(mode: FileDeleteMode) {
  const snapshot = await deleteWorkspaceEntry(pendingDeleteFolderPath.value, mode);
  if (snapshot) {
    showFolderDeleteDialog.value = false;
  }
}

async function chooseLocalFolder() {
  const selected = await open({
    directory: true,
    multiple: false,
    title: "选择资源库文件夹",
  });
  if (typeof selected === "string" && selected.trim()) {
    backendPath.value = selected;
    if (!backendName.value.trim()) {
      const segments = selected.split(/[\\/]/).filter(Boolean);
      backendName.value = segments[segments.length - 1] ?? "";
    }
  }
}

async function submitBackendDialog() {
  if (backendSubmitDisabled.value) return;
  isSubmittingBackend.value = true;
  try {
    if (isLocalBackend.value) {
      const path = backendPath.value.trim();
      const segments = path.split(/[\\/]/).filter(Boolean);
      const inferredName = segments[segments.length - 1] ?? "";
      const name = backendName.value.trim() || inferredName || "新资源库";
      await createNewRepository(name, path, backendPluginId.value);
    } else {
      const name = backendName.value.trim() || selectedBackend.value.name;
      const path = backendUrl.value.trim();
      await createNewRepository(name, path, backendPluginId.value, {
        baseUrl: backendUrl.value.trim(),
        username: backendUsername.value.trim() || undefined,
        password: backendPassword.value.trim() || undefined,
        rootPath: backendRoot.value.trim() || "",
      });
    }
    showBackendDialog.value = false;
  } catch (cause) {
    console.error("failed to create repository backend", cause);
  } finally {
    isSubmittingBackend.value = false;
  }
}

function handleAddRepositoryRequest() {
  openBackendDialog();
}

onMounted(() => {
  void ensureRepositoryWorkspace();
  window.addEventListener("momo:add-repository", handleAddRepositoryRequest);
});

onBeforeUnmount(() => {
  window.removeEventListener("momo:add-repository", handleAddRepositoryRequest);
});
</script>

<template>
  <aside class="secondary-panel secondary-panel--workspace">
    <div class="workspace-sidebar">
      <section class="workspace-sidebar__top" aria-label="资源库与视图">
        <div class="workspace-sidebar__repo-head">
          <button
            type="button"
            class="workspace-sidebar__repo-current"
            :title="activeRepository?.path ?? '添加资源库'"
            aria-label="资源库"
            @click="selectPanel('libraries')"
          >
            <span>{{ activeRepository?.name ?? "无资源库" }}</span>
          </button>
          <button
            type="button"
            class="workspace-panel__refresh"
            aria-label="添加资源库"
            title="添加资源库"
            @click="openBackendDialog"
          >
            <Plus :size="14" aria-hidden="true" />
          </button>
        </div>

        <div class="workspace-sidebar__repo-list" aria-label="切换资源库">
          <button
            v-for="library in repositories"
            :key="library.repoId"
            type="button"
            class="workspace-sidebar__repo-btn"
            :class="{ 'is-active': activeRepoId === library.repoId }"
            :title="`${library.name}\n${library.path}`"
            :aria-label="`切换资源库 ${library.name}`"
            @click="selectRepositoryFromList(library.repoId)"
          >
            <span>{{ repositoryInitial(library.name) }}</span>
          </button>
        </div>

        <nav class="workspace-sidebar__nav" aria-label="工作区导航">
          <button
            v-for="item in primaryNav"
            :key="item.key"
            type="button"
            class="workspace-nav__btn"
            :class="{ 'is-active': activePanel === item.key }"
            :title="item.label"
            :aria-label="item.label"
            @click="selectPanel(item.key)"
          >
            <component :is="item.icon" :size="17" aria-hidden="true" />
          </button>
        </nav>
      </section>

      <section class="workspace-sidebar__files" aria-label="文件管理">
        <div v-if="error" class="workspace-state workspace-state--error">
          {{ error }}
        </div>

        <div v-else-if="isBusy" class="workspace-state">
          <LoaderCircle class="spin" :size="16" aria-hidden="true" />
          正在同步仓库状态
        </div>

        <section class="workspace-group">
          <div class="workspace-group__header">
            <span>快捷方式</span>
          </div>
          <div class="workspace-shortcuts">
            <button
              v-for="item in shortcuts"
              :key="item.id"
              type="button"
              class="workspace-shortcuts__item"
            >
              <span class="workspace-shortcuts__label">
                <component :is="item.icon" :size="15" aria-hidden="true" />
                {{ item.label }}
              </span>
              <span class="workspace-shortcuts__count">{{ item.count }}</span>
            </button>
          </div>
        </section>

        <section class="workspace-group workspace-group--tree">
          <div class="workspace-group__header">
            <span>文件夹</span>
            <div class="workspace-group__actions">
              <button
                type="button"
                class="workspace-tree-action"
                :disabled="!activeRepoId || isMutatingFiles"
                title="在当前目录新建文件夹"
                aria-label="在当前目录新建文件夹"
                @click="openCreateFolderDialog(currentDirectoryPath)"
              >
                <Plus :size="13" aria-hidden="true" />
              </button>
              <button
                type="button"
                class="workspace-tree-action"
                :disabled="!activeRepoId || isLoadingFileBrowser"
                title="刷新文件夹树"
                aria-label="刷新文件夹树"
                @click="refreshFileBrowserTree"
              >
                <RefreshCw v-if="!isLoadingFileBrowser" :size="13" aria-hidden="true" />
                <LoaderCircle v-else class="spin" :size="13" aria-hidden="true" />
              </button>
            </div>
          </div>
          <div v-if="!activeRepoId" class="workspace-empty workspace-empty--compact">
            <p class="workspace-empty__text">先选择或添加一个资源库。</p>
          </div>
          <div v-else class="workspace-folder-tree">
            <div class="workspace-folder-tree__branch">
              <div class="workspace-folder-tree__row" :class="{ 'is-active': currentDirectoryPath === '' }">
                <button
                  type="button"
                  class="workspace-folder-tree__toggle workspace-folder-tree__toggle is-hidden"
                  aria-hidden="true"
                  disabled
                />
                <button
                  type="button"
                  class="workspace-folder-tree__item"
                  :class="{ 'is-active': currentDirectoryPath === '' }"
                  @click="openFolder('')"
                >
                  <span class="workspace-folder-tree__label">
                    <FolderOpen :size="15" aria-hidden="true" />
                    根目录
                  </span>
                </button>
              </div>
            </div>

            <FolderTreeNode
              v-for="node in fileTreeNodes"
              :key="node.path"
              :node="node"
              :current-path="currentDirectoryPath"
              :expanded-paths="expandedFolderPathSet"
              :depth="1"
              :is-mutating="isMutatingFiles"
              @toggle="toggleFolderExpansion"
              @open="openFolder"
              @create="openCreateFolderDialog"
              @rename="openRenameFolderDialog"
              @delete="openDeleteFolderDialog"
            />
          </div>
          <div v-if="activeRepoId && !fileTreeNodes.length && !isLoadingFileBrowser" class="workspace-empty workspace-empty--compact">
            <p class="workspace-empty__text">当前仓库还没有子文件夹。</p>
          </div>
        </section>
      </section>

      <RouterLink
        to="/settings"
        class="workspace-sidebar__settings"
        active-class="is-active"
        title="设置"
        aria-label="设置"
      >
        <Settings :size="18" aria-hidden="true" />
      </RouterLink>
    </div>
  </aside>

  <Teleport to="body">
    <Transition name="modal">
      <div
        v-if="showBackendDialog"
        class="modal-overlay"
        role="dialog"
        aria-modal="true"
        aria-label="添加资源库"
        @click.self="closeBackendDialog"
      >
        <div class="modal-card dialog-card backend-dialog">
          <div class="dialog-card__header">
            <span>添加资源库</span>
          </div>
          <div class="dialog-card__body backend-dialog__body">
            <p class="backend-dialog__summary">
              {{ selectedBackend?.description ?? "选择文件系统后端并填写配置。" }}
            </p>

            <label class="backend-dialog__field">
              <span>文件系统后端</span>
              <Dropdown
                v-model="backendPluginId"
                :options="backendOptions"
                placement="bottom"
              />
            </label>

            <label class="backend-dialog__field">
              <span>资源库名称</span>
              <input v-model="backendName" type="text" placeholder="可选，默认使用路径或后端名称" />
            </label>

            <template v-if="isLocalBackend">
              <label class="backend-dialog__field">
                <span>本地路径</span>
                <div class="backend-dialog__path-row">
                  <input v-model="backendPath" type="text" placeholder="选择本地文件夹" />
                  <button type="button" class="ghost" @click="chooseLocalFolder">选择</button>
                </div>
              </label>
            </template>

            <template v-else>
              <label class="backend-dialog__field">
                <span>服务地址</span>
                <input v-model="backendUrl" type="url" placeholder="https://example.com/dav/" />
              </label>
              <label class="backend-dialog__field">
                <span>根目录</span>
                <input v-model="backendRoot" type="text" placeholder="/assets/anime" />
              </label>
              <div class="backend-dialog__grid">
                <label class="backend-dialog__field">
                  <span>用户名</span>
                  <input v-model="backendUsername" type="text" placeholder="可选" />
                </label>
                <label class="backend-dialog__field">
                  <span>密码 / Token</span>
                  <input v-model="backendPassword" type="password" placeholder="可选" />
                </label>
              </div>
              <p class="backend-dialog__note">
                当前仅完成后端配置入口与服务端抽象。远端适配器尚未实现，请先用于配置演进与契约联调。
              </p>
            </template>
          </div>
          <div class="dialog-card__actions">
            <button type="button" class="ghost" :disabled="isSubmittingBackend" @click="closeBackendDialog">
              取消
            </button>
            <button
              type="button"
              class="primary"
              :disabled="isSubmittingBackend || backendSubmitDisabled"
              @click="submitBackendDialog"
            >
              {{ isSubmittingBackend ? "正在创建..." : "创建" }}
            </button>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>

  <Teleport to="body">
    <Transition name="modal">
      <div
        v-if="showFolderDialog"
        class="modal-overlay"
        role="dialog"
        aria-modal="true"
        :aria-label="folderDialogTitle"
        @click.self="closeFolderDialog"
      >
        <div class="modal-card dialog-card folder-dialog">
          <div class="dialog-card__header">
            <span>{{ folderDialogTitle }}</span>
          </div>
          <div class="dialog-card__body folder-dialog__body">
            <p class="folder-dialog__summary">
              {{
                folderDialogMode === "create"
                  ? `将在 ${folderDialogParentPath || "根目录"} 下创建新文件夹。`
                  : `正在重命名 ${folderDialogLabel}。`
              }}
            </p>
            <label class="backend-dialog__field">
              <span>文件夹名称</span>
              <input
                v-model="folderDialogValue"
                type="text"
                :placeholder="folderDialogPlaceholder"
                @keydown.enter.prevent="submitFolderDialog"
              />
            </label>
          </div>
          <div class="dialog-card__actions">
            <button type="button" class="ghost" :disabled="isMutatingFiles" @click="closeFolderDialog">
              取消
            </button>
            <button
              type="button"
              class="primary"
              :disabled="folderDialogDisabled"
              @click="submitFolderDialog"
            >
              {{ folderDialogActionLabel }}
            </button>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>

  <Teleport to="body">
    <Transition name="modal">
      <div
        v-if="showFolderDeleteDialog"
        class="modal-overlay"
        role="dialog"
        aria-modal="true"
        aria-label="删除文件夹"
        @click.self="closeDeleteFolderDialog"
      >
        <div class="modal-card dialog-card folder-delete-dialog">
          <div class="dialog-card__header dialog-card__header--danger">
            <span>删除文件夹</span>
          </div>
          <div class="dialog-card__body folder-delete-dialog__body">
            <p>将删除文件夹“{{ pendingDeleteFolderLabel }}”。请选择内部内容的处理方式。</p>
            <div class="folder-delete-dialog__options">
              <button
                type="button"
                class="folder-delete-dialog__option"
                :disabled="isMutatingFiles"
                @click="confirmDeleteFolder('moveToParent')"
              >
                <strong>转移到上级目录</strong>
                <span>保留内部文件和子文件夹，只删除当前这一层目录。</span>
              </button>
              <button
                type="button"
                class="folder-delete-dialog__option folder-delete-dialog__option--danger"
                :disabled="isMutatingFiles"
                @click="confirmDeleteFolder('delete')"
              >
                <strong>连同内部内容一起删除</strong>
                <span>递归删除该目录下的全部文件和子文件夹。</span>
              </button>
            </div>
          </div>
          <div class="dialog-card__actions">
            <button type="button" class="ghost" :disabled="isMutatingFiles" @click="closeDeleteFolderDialog">
              取消
            </button>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>
