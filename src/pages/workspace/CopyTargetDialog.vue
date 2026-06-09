<script setup lang="ts">
import { Copy } from "lucide-vue-next";

defineProps<{
  open: boolean;
  targetPath: string;
  isMutating: boolean;
}>();

const emit = defineEmits<{
  cancel: [];
  submit: [];
  "update:targetPath": [value: string];
}>();

function updateTargetPath(event: Event) {
  emit("update:targetPath", (event.target as HTMLInputElement | null)?.value ?? "");
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
        aria-label="复制到文件夹"
        @click.self="emit('cancel')"
      >
        <div class="modal-card dialog-card copy-target-dialog">
          <div class="dialog-card__header">
            <Copy :size="14" aria-hidden="true" />
            <span>复制到文件夹</span>
          </div>
          <div class="dialog-card__body copy-target-dialog__body">
            <label class="dialog-field">
              <span>目标目录</span>
              <input :value="targetPath" type="text" placeholder="留空表示根目录" @input="updateTargetPath" />
            </label>
          </div>
          <div class="dialog-card__actions">
            <button type="button" class="ghost" :disabled="isMutating" @click="emit('cancel')">
              取消
            </button>
            <button type="button" class="primary" :disabled="isMutating" @click="emit('submit')">
              {{ isMutating ? "处理中..." : "复制" }}
            </button>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>
