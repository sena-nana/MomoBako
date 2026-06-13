<script setup lang="ts">
import type { FileDeleteMode } from "../types/repository";

defineProps<{
  folderDialogActionLabel: string;
  folderDialogDisabled: boolean;
  folderDialogLabel: string;
  folderDialogMode: "create" | "rename";
  folderDialogParentPath: string;
  folderDialogPlaceholder: string;
  folderDialogTitle: string;
  isMutatingFiles: boolean;
  pendingDeleteFolderLabel: string;
  showFolderDeleteDialog: boolean;
  showFolderDialog: boolean;
}>();

const folderDialogValue = defineModel<string>("folderDialogValue", { required: true });

const emit = defineEmits<{
  closeFolderDelete: [];
  closeFolderDialog: [];
  confirmDeleteFolder: [mode: FileDeleteMode];
  submitFolderDialog: [];
}>();
</script>

<template>
  <Teleport to="body">
    <Transition name="modal">
      <div v-if="showFolderDialog" class="modal-overlay" role="dialog" aria-modal="true" :aria-label="folderDialogTitle" @click.self="emit('closeFolderDialog')">
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
              <input v-model="folderDialogValue" type="text" :placeholder="folderDialogPlaceholder" @keydown.enter.prevent="emit('submitFolderDialog')" />
            </label>
          </div>
          <div class="dialog-card__actions">
            <button type="button" class="ghost" :disabled="isMutatingFiles" @click="emit('closeFolderDialog')">
              取消
            </button>
            <button type="button" class="primary" :disabled="folderDialogDisabled" @click="emit('submitFolderDialog')">
              {{ folderDialogActionLabel }}
            </button>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>

  <Teleport to="body">
    <Transition name="modal">
      <div v-if="showFolderDeleteDialog" class="modal-overlay" role="dialog" aria-modal="true" aria-label="处理文件夹" @click.self="emit('closeFolderDelete')">
        <div class="modal-card dialog-card folder-delete-dialog">
          <div class="dialog-card__header dialog-card__header--danger">
            <span>处理文件夹</span>
          </div>
          <div class="dialog-card__body folder-delete-dialog__body">
            <p>将处理文件夹“{{ pendingDeleteFolderLabel }}”。请选择内部内容的处理方式。</p>
            <div class="folder-delete-dialog__options">
              <button type="button" class="folder-delete-dialog__option" :disabled="isMutatingFiles" @click="emit('confirmDeleteFolder', 'moveToParent')">
                <strong>转移到上级目录</strong>
                <span>保留内部文件和子文件夹，只删除当前这一层目录。</span>
              </button>
              <button type="button" class="folder-delete-dialog__option folder-delete-dialog__option--danger" :disabled="isMutatingFiles" @click="emit('confirmDeleteFolder', 'delete')">
                <strong>移入回收站</strong>
                <span>将该目录及其全部内容移入回收站，可在回收站中还原。</span>
              </button>
            </div>
          </div>
          <div class="dialog-card__actions">
            <button type="button" class="ghost" :disabled="isMutatingFiles" @click="emit('closeFolderDelete')">
              取消
            </button>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>
