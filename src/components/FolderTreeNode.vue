<script setup lang="ts">
import { computed } from "vue";
import {
  Folder,
  FolderOpen,
  FolderPlus,
  PencilLine,
  Trash2,
} from "lucide-vue-next";
import type { FileTreeNode } from "../types/repository";

defineOptions({
  name: "FolderTreeNode",
});

const props = defineProps<{
  node: FileTreeNode;
  currentPath: string;
  expandedPaths: Set<string>;
  depth: number;
  isMutating: boolean;
}>();

const emit = defineEmits<{
  (event: "toggle", path: string): void;
  (event: "open", path: string): void;
  (event: "create", path: string): void;
  (event: "rename", path: string, label: string): void;
  (event: "delete", path: string, label: string): void;
}>();

const isExpanded = computed(() => props.expandedPaths.has(props.node.path));
const isActive = computed(() => props.currentPath === props.node.path);
const isCurrentBranch = computed(() => (
  props.currentPath === props.node.path || props.currentPath.startsWith(`${props.node.path}/`)
));
const hasChildren = computed(() => props.node.children.length > 0);
const depthStyle = computed(() => ({
  "--folder-node-depth": String(props.depth),
}));
</script>

<template>
  <div class="workspace-folder-tree__branch">
    <div class="workspace-folder-tree__row" :class="{ 'is-active': isActive }" :style="depthStyle">
      <button
        type="button"
        class="workspace-folder-tree__toggle"
        :class="{ 'is-hidden': !hasChildren }"
        :aria-label="isExpanded ? '收起文件夹' : '展开文件夹'"
        :disabled="!hasChildren"
        @click.stop="emit('toggle', node.path)"
      >
        <span
          v-if="hasChildren"
          class="workspace-folder-tree__toggle-caret"
          :class="{ 'is-expanded': isExpanded }"
          aria-hidden="true"
        />
      </button>

      <div class="workspace-folder-tree__card">
        <button
          type="button"
          class="workspace-folder-tree__item"
          @click="emit('open', node.path)"
        >
          <span class="workspace-folder-tree__label">
            <FolderOpen v-if="isCurrentBranch" :size="14" aria-hidden="true" />
            <Folder v-else :size="14" aria-hidden="true" />
            {{ node.label }}
          </span>
        </button>

        <div class="workspace-folder-tree__actions">
          <button
            type="button"
            class="workspace-folder-tree__action"
            title="新建子文件夹"
            aria-label="新建子文件夹"
            :disabled="isMutating"
            @click.stop="emit('create', node.path)"
          >
            <FolderPlus :size="13" aria-hidden="true" />
          </button>
          <button
            type="button"
            class="workspace-folder-tree__action"
            title="重命名文件夹"
            aria-label="重命名文件夹"
            :disabled="isMutating"
            @click.stop="emit('rename', node.path, node.label)"
          >
            <PencilLine :size="13" aria-hidden="true" />
          </button>
          <button
            type="button"
            class="workspace-folder-tree__action workspace-folder-tree__action--danger"
            title="删除文件夹"
            aria-label="删除文件夹"
            :disabled="isMutating"
            @click.stop="emit('delete', node.path, node.label)"
          >
            <Trash2 :size="13" aria-hidden="true" />
          </button>
        </div>
      </div>
    </div>

    <div v-if="hasChildren && isExpanded" class="workspace-folder-tree__children">
      <FolderTreeNode
        v-for="child in node.children"
        :key="child.path"
        :node="child"
        :current-path="currentPath"
        :expanded-paths="expandedPaths"
        :depth="depth + 1"
        :is-mutating="isMutating"
        @toggle="emit('toggle', $event)"
        @open="emit('open', $event)"
        @create="emit('create', $event)"
        @rename="(path, label) => emit('rename', path, label)"
        @delete="(path, label) => emit('delete', path, label)"
      />
    </div>
  </div>
</template>
