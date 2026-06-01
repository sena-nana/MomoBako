<script setup lang="ts">
import { AlertTriangle } from "lucide-vue-next";

defineProps<{
  open: boolean;
  title: string;
  message: string;
  confirmText?: string;
  cancelText?: string;
  danger?: boolean;
  busy?: boolean;
  busyText?: string;
}>();

const emit = defineEmits<{
  confirm: [];
  cancel: [];
}>();

function onKeydown(event: KeyboardEvent) {
  if (event.key === "Escape") {
    event.preventDefault();
    emit("cancel");
  }
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
        :aria-label="title"
        tabindex="-1"
        @click.self="emit('cancel')"
        @keydown="onKeydown"
      >
        <div class="modal-card dialog-card">
          <div class="dialog-card__header" :class="{ 'dialog-card__header--danger': danger }">
            <AlertTriangle v-if="danger" :size="14" aria-hidden="true" />
            <span>{{ title }}</span>
          </div>
          <div class="dialog-card__body">
            <p>{{ message }}</p>
          </div>
          <div class="dialog-card__actions">
            <button type="button" class="ghost" :disabled="busy" @click="emit('cancel')">
              {{ cancelText ?? "取消" }}
            </button>
            <button
              type="button"
              :class="danger ? 'ghost danger' : 'primary'"
              :disabled="busy"
              @click="emit('confirm')"
            >
              {{ busy ? (busyText ?? "处理中...") : (confirmText ?? "确认") }}
            </button>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>
