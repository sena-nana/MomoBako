<script setup lang="ts">
defineProps<{
  error: string | null;
  isDragging: boolean;
}>();

const emit = defineEmits<{
  dragLeave: [event: DragEvent];
  dragOver: [event: DragEvent];
  drop: [event: DragEvent];
}>();
</script>

<template>
  <section
    class="empty-state-page"
    :class="{ 'is-dragging': isDragging }"
    @dragover="emit('dragOver', $event)"
    @dragleave="emit('dragLeave', $event)"
    @drop="emit('drop', $event)"
  >
    <div class="empty-state-page__panel">
      <h1>还没有可用资源库</h1>
      <p>拖入一个本地文件夹创建资源库。</p>
      <p v-if="error" class="empty-state-page__error">
        {{ error }}
      </p>
    </div>
  </section>
</template>
