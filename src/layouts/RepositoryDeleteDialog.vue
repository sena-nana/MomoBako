<script setup lang="ts">
/**
 * 资源库删除弹窗。
 * 统一承接侧栏切换器与缺失资源库页面的删除确认入口。
 */
import { LoaderCircle } from "@lucide/vue";
import type { RepositoryDeleteMode, RepositorySummary } from "../types/repository";

const props = defineProps<{
  open: boolean;
  repository: RepositorySummary | null;
  error: string;
  isDeleting: boolean;
  canDeleteMetadata: boolean;
  canDeleteFolder: boolean;
}>();

const emit = defineEmits<{
  close: [];
  confirm: [mode: RepositoryDeleteMode];
}>();

function metadataOptionDescription(repository: RepositorySummary | null, enabled: boolean) {
  if (enabled) {
    return repository?.localCache?.required
      ? "删除该资源库的 Momo 元数据目录与索引缓存，保留缓存文件夹中的其他用户内容。"
      : "删除该资源库的 .momo 数据目录，保留原文件夹与用户文件。";
  }
  return "当前资源库的 .momo 目录不可直接访问，请先恢复路径后再删除元数据。";
}

function folderOptionDescription(enabled: boolean) {
  if (enabled) {
    return "递归删除当前资源库文件夹，目录内的 .momo 数据与用户文件会一起删除。";
  }
  return "当前资源库目录不可直接访问，暂时不能删除整个文件夹。";
}
</script>

<template>
  <Teleport to="body">
    <Transition name="modal">
      <div
        v-if="open"
        class="modal-overlay"
        role="dialog"
        aria-modal="true"
        aria-label="删除资源库"
        @click.self="!isDeleting && emit('close')"
      >
        <div class="modal-card dialog-card repository-delete-dialog">
          <div class="dialog-card__header dialog-card__header--danger">
            <span>删除资源库</span>
          </div>
          <div class="dialog-card__body repository-delete-dialog__body">
            <p v-if="repository" class="repository-delete-dialog__summary">
              资源库“{{ repository.name }}”位于 {{ repository.path }}。下面每个操作都会移除当前注册记录。
            </p>
            <div class="repository-delete-dialog__options">
              <button
                type="button"
                class="repository-delete-dialog__option"
                :disabled="isDeleting"
                @click="emit('confirm', 'recordOnly')"
              >
                <strong>只删除记录</strong>
                <span>只从 MomoBako 移除这条资源库记录，保留文件夹、.momo 和应用托管数据。</span>
              </button>
              <button
                type="button"
                class="repository-delete-dialog__option"
                :disabled="isDeleting || !canDeleteMetadata"
                @click="emit('confirm', 'deleteMetadata')"
              >
                <strong>删除 .momo 数据</strong>
                <span>{{ metadataOptionDescription(repository, canDeleteMetadata) }}</span>
              </button>
              <button
                type="button"
                class="repository-delete-dialog__option repository-delete-dialog__option--danger"
                :disabled="isDeleting || !canDeleteFolder"
                @click="emit('confirm', 'deleteFolder')"
              >
                <strong>删除整个文件夹</strong>
                <span>{{ folderOptionDescription(canDeleteFolder) }}</span>
              </button>
            </div>
            <p v-if="error" class="repository-delete-dialog__error">
              {{ error }}
            </p>
          </div>
          <div class="dialog-card__actions">
            <span v-if="isDeleting" class="repository-delete-dialog__busy">
              <LoaderCircle class="spin" :size="13" aria-hidden="true" />
              处理中...
            </span>
            <button type="button" class="ghost" :disabled="isDeleting" @click="emit('close')">
              取消
            </button>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>
