<script setup lang="ts">
import { LoaderCircle, Plus, RefreshCw } from "lucide-vue-next";
import FolderTreeNode from "../components/FolderTreeNode.vue";
import type { FileTreeNode } from "../types/repository";

defineProps<{
  activeRepoId: string | null;
  currentDirectoryPath: string;
  dragHoverFolderPath: string | null;
  expandedFolderPathSet: Set<string>;
  fileTreeNodes: FileTreeNode[];
  isActiveRepositoryMissing: boolean;
  isFolderDragActive: boolean;
  isLoadingFileBrowser: boolean;
  isMutatingFiles: boolean;
  isTrashPanel: boolean;
}>();

const emit = defineEmits<{
  create: [parentPath?: string];
  delete: [path: string, label: string];
  dropFolder: [path: string, event: DragEvent];
  hoverFolder: [path: string];
  leaveFolder: [path: string];
  open: [path: string];
  refresh: [];
  rename: [path: string, label: string];
  toggle: [path: string];
}>();
</script>

<template>
  <section class="workspace-group workspace-group--tree">
    <div class="workspace-group__header">
      <span>文件夹</span>
      <div class="workspace-group__actions">
        <button
          type="button"
          class="workspace-tree-action"
          :disabled="!activeRepoId || isActiveRepositoryMissing || isMutatingFiles || isTrashPanel"
          title="在当前目录新建文件夹"
          aria-label="在当前目录新建文件夹"
          @click="emit('create', currentDirectoryPath)"
        >
          <Plus :size="13" aria-hidden="true" />
        </button>
        <button
          type="button"
          class="workspace-tree-action"
          :disabled="!activeRepoId || isActiveRepositoryMissing || isLoadingFileBrowser"
          title="刷新文件夹树"
          aria-label="刷新文件夹树"
          @click="emit('refresh')"
        >
          <RefreshCw v-if="!isLoadingFileBrowser" :size="13" aria-hidden="true" />
          <LoaderCircle v-else class="spin" :size="13" aria-hidden="true" />
        </button>
      </div>
    </div>
    <div v-if="!activeRepoId" class="workspace-empty workspace-empty--compact">
      <p class="workspace-empty__text">先选择或添加一个资源库。</p>
    </div>
    <div v-else-if="isActiveRepositoryMissing" class="workspace-empty workspace-empty--compact">
      <p class="workspace-empty__text">资源库文件夹丢失，请先在主视图修复。</p>
    </div>
    <div v-else class="workspace-folder-tree">
      <FolderTreeNode
        v-if="!isTrashPanel"
        v-for="node in fileTreeNodes"
        :key="node.path"
        :node="node"
        :current-path="currentDirectoryPath"
        :expanded-paths="expandedFolderPathSet"
        :depth="1"
        :drop-target-path="dragHoverFolderPath"
        :is-drag-active="isFolderDragActive"
        :is-mutating="isMutatingFiles"
        @toggle="emit('toggle', $event)"
        @open="emit('open', $event)"
        @create="emit('create', $event)"
        @rename="(path, label) => emit('rename', path, label)"
        @delete="(path, label) => emit('delete', path, label)"
        @hover-folder="emit('hoverFolder', $event)"
        @leave-folder="emit('leaveFolder', $event)"
        @drop-folder="(path, event) => emit('dropFolder', path, event)"
      />
    </div>
    <div v-if="activeRepoId && !isActiveRepositoryMissing && (isTrashPanel || !fileTreeNodes.length) && !isLoadingFileBrowser" class="workspace-empty workspace-empty--compact">
      <p class="workspace-empty__text">{{ isTrashPanel ? "回收站条目在主视图中管理。" : "当前仓库还没有子文件夹。" }}</p>
    </div>
  </section>
</template>
