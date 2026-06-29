<script setup lang="ts">
import { Plus } from "@lucide/vue";
import SmartFolderTreeNode from "../components/SmartFolderTreeNode.vue";
import type { SmartFolderTreeNode as SmartFolderNode } from "../types/repository";

defineProps<{
  activeRepoId: string | null;
  activeSmartFolderId: string | null;
  expandedSmartFolderIdSet: Set<string>;
  isActiveRepositoryMissing: boolean;
  isMutatingSmartFolder: boolean;
  smartFolders: SmartFolderNode[];
}>();

const emit = defineEmits<{
  create: [parentId?: string];
  delete: [smartFolderId: string, label: string];
  edit: [smartFolderId: string];
  open: [smartFolderId: string];
  toggle: [smartFolderId: string];
}>();
</script>

<template>
  <section class="workspace-group workspace-group--tree">
    <div class="workspace-group__header">
      <span>智能文件夹</span>
      <div class="workspace-group__actions">
        <button
          type="button"
          class="workspace-tree-action"
          :disabled="!activeRepoId || isActiveRepositoryMissing || isMutatingSmartFolder"
          title="新建智能文件夹"
          aria-label="新建智能文件夹"
          @click="emit('create')"
        >
          <Plus :size="13" aria-hidden="true" />
        </button>
      </div>
    </div>
    <div v-if="!activeRepoId" class="workspace-empty workspace-empty--compact">
      <p class="workspace-empty__text">先选择或添加一个资源库。</p>
    </div>
    <div v-else-if="isActiveRepositoryMissing" class="workspace-empty workspace-empty--compact">
      <p class="workspace-empty__text">资源库修复后可继续使用智能文件夹。</p>
    </div>
    <div v-else-if="smartFolders.length" class="workspace-folder-tree">
      <SmartFolderTreeNode
        v-for="node in smartFolders"
        :key="node.smartFolderId"
        :node="node"
        :active-id="activeSmartFolderId"
        :expanded-ids="expandedSmartFolderIdSet"
        :depth="1"
        :is-mutating="isMutatingSmartFolder"
        @toggle="emit('toggle', $event)"
        @open="emit('open', $event)"
        @create="emit('create', $event)"
        @edit="emit('edit', $event)"
        @delete="(smartFolderId, label) => emit('delete', smartFolderId, label)"
      />
    </div>
    <div v-else class="workspace-empty workspace-empty--compact">
      <p class="workspace-empty__text">还没有智能文件夹。</p>
    </div>
  </section>
</template>
