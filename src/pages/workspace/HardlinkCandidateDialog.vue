<script setup lang="ts">
import { Copy } from "lucide-vue-next";
import type { HardlinkCandidate } from "../../types/repository";

defineProps<{
  candidate: HardlinkCandidate | null;
  isMutating: boolean;
  message: (candidate: HardlinkCandidate) => string;
}>();

const emit = defineEmits<{
  confirm: [];
  skip: [];
}>();
</script>

<template>
  <Teleport to="body">
    <Transition name="modal">
      <div
        v-if="candidate"
        class="modal-overlay"
        role="dialog"
        aria-modal="true"
        aria-label="加入硬链接关联"
        @click.self="emit('skip')"
      >
        <div class="modal-card dialog-card hardlink-candidate-dialog">
          <div class="dialog-card__header">
            <Copy :size="14" aria-hidden="true" />
            <span>加入硬链接关联</span>
          </div>
          <div class="dialog-card__body hardlink-candidate-dialog__body">
            <p>{{ message(candidate) }}</p>
            <div class="hardlink-candidate-dialog__paths">
              <span>{{ candidate.existingPath }}</span>
              <span>{{ candidate.newPath }}</span>
            </div>
          </div>
          <div class="dialog-card__actions">
            <button type="button" class="ghost" :disabled="isMutating" @click="emit('skip')">
              跳过
            </button>
            <button type="button" class="primary" :disabled="isMutating" @click="emit('confirm')">
              {{ isMutating ? "处理中..." : "加入关联" }}
            </button>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>
