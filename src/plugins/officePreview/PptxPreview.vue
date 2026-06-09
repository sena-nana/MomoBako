<script setup lang="ts">
import { nextTick, onBeforeUnmount, ref, watch } from "vue";
import { init } from "pptx-preview";

const props = defineProps<{
  src: string;
}>();

const emit = defineEmits<{
  rendered: [];
  error: [cause: unknown];
}>();

type PptxPreviewer = {
  preview: (source: ArrayBuffer) => Promise<unknown>;
  destroy?: () => void;
};

const host = ref<HTMLElement | null>(null);
let previewer: PptxPreviewer | null = null;
let renderToken = 0;

watch(
  () => props.src,
  () => {
    void renderPreview();
  },
  { immediate: true },
);

onBeforeUnmount(() => {
  previewer?.destroy?.();
  previewer = null;
  renderToken++;
});

async function renderPreview() {
  const token = ++renderToken;
  await nextTick();
  if (token !== renderToken || !host.value || !props.src) return;

  try {
    const response = await fetch(props.src);
    if (!response.ok) {
      throw new Error(`PPTX 读取失败: ${response.status}`);
    }
    const bytes = await response.arrayBuffer();
    if (token !== renderToken || !host.value) return;

    previewer?.destroy?.();
    host.value.replaceChildren();
    previewer = init(host.value, {
      width: 960,
      height: 540,
      mode: "list",
    }) as PptxPreviewer;
    await previewer.preview(bytes);
    if (token !== renderToken) return;
    emit("rendered");
  } catch (cause) {
    if (token !== renderToken) return;
    emit("error", cause);
  }
}
</script>

<template>
  <div ref="host" class="office-preview__pptx-host" />
</template>
