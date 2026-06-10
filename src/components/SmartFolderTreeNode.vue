<script setup lang="ts">
import { computed } from "vue";
import {
  Folder,
  FolderOpen,
  FolderPlus,
  PencilLine,
  Trash2,
} from "lucide-vue-next";
import type { SmartFolderTreeNode } from "../types/repository";

defineOptions({
  name: "SmartFolderTreeNode",
});

const props = defineProps<{
  node: SmartFolderTreeNode;
  activeId: string | null;
  expandedIds: Set<string>;
  depth: number;
  isMutating: boolean;
}>();

const emit = defineEmits<{
  (event: "toggle", smartFolderId: string): void;
  (event: "open", smartFolderId: string): void;
  (event: "create", parentId: string): void;
  (event: "edit", smartFolderId: string): void;
  (event: "delete", smartFolderId: string, label: string): void;
}>();

const isExpanded = computed(() => props.expandedIds.has(props.node.smartFolderId));
const isActive = computed(() => props.activeId === props.node.smartFolderId);
const isCurrentBranch = computed(() => (
  isActive.value || containsActiveChild(props.node.children, props.activeId)
));
const hasChildren = computed(() => props.node.children.length > 0);
const depthStyle = computed(() => ({
  "--folder-node-depth": String(props.depth),
}));

function containsActiveChild(nodes: SmartFolderTreeNode[], activeId: string | null): boolean {
  if (!activeId) return false;
  return nodes.some((node) => (
    node.smartFolderId === activeId || containsActiveChild(node.children, activeId)
  ));
}
</script>

<template>
  <div class="workspace-folder-tree__branch">
    <div class="workspace-folder-tree__row" :class="{ 'is-active': isActive }" :style="depthStyle">
      <button
        type="button"
        class="workspace-folder-tree__toggle"
        :class="{ 'is-hidden': !hasChildren }"
        :aria-label="isExpanded ? '收起智能文件夹' : '展开智能文件夹'"
        :disabled="!hasChildren"
        @click.stop="emit('toggle', node.smartFolderId)"
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
          @click="emit('open', node.smartFolderId)"
        >
          <span class="workspace-folder-tree__label">
            <FolderOpen v-if="isCurrentBranch" :size="14" aria-hidden="true" />
            <Folder v-else :size="14" aria-hidden="true" />
            {{ node.name }}
          </span>
        </button>

        <div class="workspace-folder-tree__actions">
          <button
            type="button"
            class="workspace-folder-tree__action"
            title="新建子智能文件夹"
            aria-label="新建子智能文件夹"
            :disabled="isMutating"
            @click.stop="emit('create', node.smartFolderId)"
          >
            <FolderPlus :size="13" aria-hidden="true" />
          </button>
          <button
            type="button"
            class="workspace-folder-tree__action"
            title="编辑智能文件夹"
            aria-label="编辑智能文件夹"
            :disabled="isMutating"
            @click.stop="emit('edit', node.smartFolderId)"
          >
            <PencilLine :size="13" aria-hidden="true" />
          </button>
          <button
            type="button"
            class="workspace-folder-tree__action workspace-folder-tree__action--danger"
            title="删除智能文件夹"
            aria-label="删除智能文件夹"
            :disabled="isMutating"
            @click.stop="emit('delete', node.smartFolderId, node.name)"
          >
            <Trash2 :size="13" aria-hidden="true" />
          </button>
        </div>
      </div>
    </div>

    <div v-if="hasChildren && isExpanded" class="workspace-folder-tree__children">
      <SmartFolderTreeNode
        v-for="child in node.children"
        :key="child.smartFolderId"
        :node="child"
        :active-id="activeId"
        :expanded-ids="expandedIds"
        :depth="depth + 1"
        :is-mutating="isMutating"
        @toggle="emit('toggle', $event)"
        @open="emit('open', $event)"
        @create="emit('create', $event)"
        @edit="emit('edit', $event)"
        @delete="(id, label) => emit('delete', id, label)"
      />
    </div>
  </div>
</template>
