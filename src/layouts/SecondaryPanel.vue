<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { open } from "@tauri-apps/plugin-dialog";
import { RouterLink, useRoute, useRouter } from "vue-router";
import {
  Archive,
  FolderOpen,
  FolderTree,
  LoaderCircle,
  Plus,
  Puzzle,
  RefreshCw,
  Settings,
  Tag,
  Trash2,
  X,
} from "lucide-vue-next";
import FolderTreeNode from "../components/FolderTreeNode.vue";
import { useRepositoryWorkspace, type WorkspacePanelKey } from "../composables/useRepositoryWorkspace";
import type { FileDeleteMode } from "../types/repository";

type PanelKey = Exclude<WorkspacePanelKey, "files" | "search">;

type AddRepositoryPopoverMode = "closed" | "menu" | "form";
type AddRepositoryAnchor = {
  left: number;
  top: number;
  right: number;
  bottom: number;
};
type AddRepositoryRequestDetail = {
  anchor?: AddRepositoryAnchor;
};

const addRepositoryPopoverMode = ref<AddRepositoryPopoverMode>("closed");
const addRepositoryPopoverPosition = ref({ left: 0, top: 0 });
const addRepositoryPopoverRef = ref<HTMLElement | null>(null);
const backendPluginId = ref("builtin.local-filesystem");
const backendName = ref("");
const backendUrl = ref("");
const backendUsername = ref("");
const backendPassword = ref("");
const backendRoot = ref("");
const isSubmittingBackend = ref(false);
const addRepositoryError = ref("");
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
  enabled: item.enabled,
})));
const selectedBackend = computed(() => (
  repositoryBackendOptions.value.find((item) => item.pluginId === backendPluginId.value)
  ?? repositoryBackendOptions.value[0]
  ?? null
));
const backendSubmitDisabled = computed(() => {
  if (!selectedBackend.value?.enabled) {
    return true;
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

function resetBackendForm(pluginId = repositoryBackendOptions.value[0]?.pluginId ?? "builtin.local-filesystem") {
  backendPluginId.value = pluginId;
  backendName.value = "";
  backendUrl.value = "";
  backendUsername.value = "";
  backendPassword.value = "";
  backendRoot.value = "";
  addRepositoryError.value = "";
}

function getAnchorFromElement(element: EventTarget | null): AddRepositoryAnchor | null {
  if (!(element instanceof HTMLElement)) return null;
  const rect = element.getBoundingClientRect();
  return {
    left: rect.left,
    top: rect.top,
    right: rect.right,
    bottom: rect.bottom,
  };
}

function getPopoverPosition(anchor?: AddRepositoryAnchor | null) {
  const fallback = {
    left: 16,
    top: 44,
    right: 16,
    bottom: 44,
  };
  const current = anchor ?? fallback;
  const width = 320;
  const maxLeft = Math.max(8, window.innerWidth - width - 8);
  const left = Math.max(8, Math.min(current.left, maxLeft));
  const top = Math.max(8, Math.min(current.bottom + 6, window.innerHeight - 80));
  return { left, top };
}

function openAddRepositoryMenu(anchor?: AddRepositoryAnchor | null) {
  if (!isSubmittingBackend.value) {
    resetBackendForm();
  }
  addRepositoryPopoverPosition.value = getPopoverPosition(anchor);
  addRepositoryPopoverMode.value = "menu";
}

function openAddRepositoryMenuFromEvent(event: MouseEvent) {
  openAddRepositoryMenu(getAnchorFromElement(event.currentTarget));
}

function closeAddRepositoryPopover() {
  if (isSubmittingBackend.value) return;
  addRepositoryPopoverMode.value = "closed";
}

async function selectBackend(pluginId: string) {
  if (isSubmittingBackend.value) return;
  const backend = repositoryBackendOptions.value.find((item) => item.pluginId === pluginId);
  if (!backend?.enabled) return;
  resetBackendForm(pluginId);
  if (pluginId === "builtin.local-filesystem") {
    await chooseLocalFolderAndCreate();
    return;
  }
  backendPluginId.value = pluginId;
  addRepositoryPopoverMode.value = "form";
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

async function chooseLocalFolderAndCreate() {
  addRepositoryError.value = "";
  const previousPosition = addRepositoryPopoverPosition.value;
  addRepositoryPopoverMode.value = "closed";
  const selected = await open({
    directory: true,
    multiple: false,
    title: "选择资源库文件夹",
  });
  if (typeof selected === "string" && selected.trim()) {
    await createLocalRepositoryFromPath(selected, previousPosition);
  }
}

function inferRepositoryNameFromPath(path: string) {
  const segments = path.split(/[\\/]/).filter(Boolean);
  return segments[segments.length - 1] ?? "";
}

async function createLocalRepositoryFromPath(path: string, fallbackPosition = addRepositoryPopoverPosition.value) {
  const nextPath = path.trim();
  if (!nextPath) return false;
  backendPluginId.value = "builtin.local-filesystem";
  const name = backendName.value.trim() || inferRepositoryNameFromPath(nextPath) || "新资源库";
  addRepositoryError.value = "";
  isSubmittingBackend.value = true;
  try {
    await createNewRepository(name, nextPath, "builtin.local-filesystem");
    addRepositoryPopoverMode.value = "closed";
    return true;
  } catch (cause) {
    addRepositoryError.value = cause instanceof Error ? cause.message : String(cause);
    addRepositoryPopoverPosition.value = fallbackPosition;
    addRepositoryPopoverMode.value = "menu";
    console.error("failed to create repository backend", cause);
    return false;
  } finally {
    isSubmittingBackend.value = false;
  }
}

async function submitAddRepositoryForm() {
  if (backendSubmitDisabled.value) return;
  isSubmittingBackend.value = true;
  addRepositoryError.value = "";
  try {
    const name = backendName.value.trim() || selectedBackend.value?.name || "新资源库";
    const path = backendUrl.value.trim();
    await createNewRepository(name, path, backendPluginId.value, {
      baseUrl: path,
      username: backendUsername.value.trim() || undefined,
      password: backendPassword.value.trim() || undefined,
      rootPath: backendRoot.value.trim() || "",
    });
    addRepositoryPopoverMode.value = "closed";
  } catch (cause) {
    addRepositoryError.value = cause instanceof Error ? cause.message : String(cause);
    console.error("failed to create repository backend", cause);
  } finally {
    isSubmittingBackend.value = false;
  }
}

function handleAddRepositoryRequest(event: Event) {
  const detail = (event as CustomEvent<AddRepositoryRequestDetail>).detail;
  openAddRepositoryMenu(detail?.anchor);
}

function handleDocumentKeydown(event: KeyboardEvent) {
  if (event.key === "Escape" && addRepositoryPopoverMode.value !== "closed") {
    closeAddRepositoryPopover();
  }
}

function handleDocumentPointerDown(event: PointerEvent) {
  if (addRepositoryPopoverMode.value === "closed" || isSubmittingBackend.value) return;
  const target = event.target as Node | null;
  if (target && addRepositoryPopoverRef.value?.contains(target)) return;
  closeAddRepositoryPopover();
}

onMounted(() => {
  void ensureRepositoryWorkspace();
  window.addEventListener("momo:add-repository", handleAddRepositoryRequest);
  document.addEventListener("keydown", handleDocumentKeydown);
  document.addEventListener("pointerdown", handleDocumentPointerDown, true);
});

onBeforeUnmount(() => {
  window.removeEventListener("momo:add-repository", handleAddRepositoryRequest);
  document.removeEventListener("keydown", handleDocumentKeydown);
  document.removeEventListener("pointerdown", handleDocumentPointerDown, true);
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
            @click="openAddRepositoryMenuFromEvent"
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

      <footer class="workspace-sidebar__footer" aria-label="辅助入口">
        <RouterLink
          to="/settings"
          class="workspace-footer__btn"
          active-class="is-active"
          title="设置"
          aria-label="设置"
        >
          <Settings :size="14" aria-hidden="true" />
        </RouterLink>
        <button
          type="button"
          class="workspace-footer__btn"
          :class="{ 'is-active': activePanel === 'extensions' && route.path !== '/settings' }"
          title="拓展"
          aria-label="拓展"
          @click="selectPanel('extensions')"
        >
          <Puzzle :size="14" aria-hidden="true" />
        </button>
      </footer>
    </div>
  </aside>

  <Teleport to="body">
    <Transition name="panel">
      <section
        v-if="addRepositoryPopoverMode !== 'closed'"
        ref="addRepositoryPopoverRef"
        class="repository-add-popover"
        :class="{ 'ctx-menu': addRepositoryPopoverMode === 'menu' }"
        :style="{ left: `${addRepositoryPopoverPosition.left}px`, top: `${addRepositoryPopoverPosition.top}px` }"
        aria-label="添加资源库"
      >
        <template v-if="addRepositoryPopoverMode === 'menu'">
          <button
            v-for="option in backendOptions"
            :key="option.value"
            type="button"
            class="ctx-menu__item"
            :disabled="isSubmittingBackend || !option.enabled"
            @click="selectBackend(String(option.value))"
          >
            {{ option.label }}
          </button>

          <p v-if="addRepositoryError" class="repository-add-popover__error">
            {{ addRepositoryError }}
          </p>
        </template>

        <template v-else>
          <header class="repository-add-popover__header">
            <span>{{ selectedBackend?.name ?? "添加资源库" }}</span>
            <button
              type="button"
              class="repository-add-popover__close"
              title="关闭"
              aria-label="关闭添加资源库"
              :disabled="isSubmittingBackend"
              @click="closeAddRepositoryPopover"
            >
              <X :size="13" aria-hidden="true" />
            </button>
          </header>

          <div class="repository-add-popover__body">
            <p class="repository-add-popover__summary">
              {{ selectedBackend?.description ?? "填写资源库配置。" }}
            </p>

            <label class="repository-add-popover__field">
              <span>资源库名称</span>
              <input v-model="backendName" type="text" placeholder="可选，默认使用后端名称" :disabled="isSubmittingBackend" />
            </label>

            <label class="repository-add-popover__field">
              <span>服务地址</span>
              <input v-model="backendUrl" type="url" placeholder="https://example.com/dav/" :disabled="isSubmittingBackend" />
            </label>
            <label class="repository-add-popover__field">
              <span>根目录</span>
              <input v-model="backendRoot" type="text" placeholder="/assets/anime" :disabled="isSubmittingBackend" />
            </label>
            <label class="repository-add-popover__field">
              <span>用户名</span>
              <input v-model="backendUsername" type="text" placeholder="可选" :disabled="isSubmittingBackend" />
            </label>
            <label class="repository-add-popover__field">
              <span>密码 / Token</span>
              <input v-model="backendPassword" type="password" placeholder="可选" :disabled="isSubmittingBackend" />
            </label>
            <p class="repository-add-popover__note">
              当前仅完成后端配置入口与服务端抽象。远端适配器尚未实现，请先用于配置演进与契约联调。
            </p>

            <p v-if="addRepositoryError" class="repository-add-popover__error">
              {{ addRepositoryError }}
            </p>
          </div>

          <div class="repository-add-popover__actions">
            <button type="button" class="ghost" :disabled="isSubmittingBackend" @click="addRepositoryPopoverMode = 'menu'">
              返回
            </button>
            <button type="button" class="ghost" :disabled="isSubmittingBackend" @click="closeAddRepositoryPopover">
              取消
            </button>
            <button
              type="button"
              class="primary"
              :disabled="isSubmittingBackend || backendSubmitDisabled"
              @click="submitAddRepositoryForm"
            >
              <LoaderCircle v-if="isSubmittingBackend" class="spin" :size="13" aria-hidden="true" />
              {{ isSubmittingBackend ? "创建中" : "创建" }}
            </button>
          </div>
        </template>
      </section>
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
            <label class="dialog-field">
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
