<script setup lang="ts">
import { onMounted, onUnmounted, ref } from "vue";
import { Copy, Minus, Square, X } from "lucide-vue-next";
import { getCurrentWindow } from "@tauri-apps/api/window";

interface Props {
  title?: string;
}

withDefaults(defineProps<Props>(), { title: "Tauri Template" });

const isMaximized = ref(false);
const appWindow = safeCurrentWindow();
let unlisten: (() => void) | null = null;

function safeCurrentWindow(): ReturnType<typeof getCurrentWindow> | null {
  try {
    return getCurrentWindow();
  } catch {
    return null;
  }
}

async function syncMaximized() {
  if (!appWindow) return;
  try {
    isMaximized.value = await appWindow.isMaximized();
  } catch {
    isMaximized.value = false;
  }
}

onMounted(async () => {
  await syncMaximized();
  if (!appWindow) return;
  unlisten = await appWindow.onResized(() => {
    void syncMaximized();
  });
});

onUnmounted(() => {
  unlisten?.();
});

async function onMinimize() {
  if (!appWindow) return;
  await appWindow.minimize();
}

async function onToggleMaximize() {
  if (!appWindow) return;
  await appWindow.toggleMaximize();
  await syncMaximized();
}

async function onClose() {
  if (!appWindow) return;
  await appWindow.close();
}
</script>

<template>
  <header class="titlebar" data-tauri-drag-region>
    <div class="titlebar__spacer" data-tauri-drag-region></div>
    <div class="titlebar__brand" data-tauri-drag-region>{{ title }}</div>
    <div class="titlebar__controls">
      <button
        type="button"
        class="titlebar__btn"
        aria-label="最小化"
        @click="onMinimize"
      >
        <Minus :size="14" aria-hidden="true" />
      </button>
      <button
        type="button"
        class="titlebar__btn"
        :aria-label="isMaximized ? '还原' : '最大化'"
        @click="onToggleMaximize"
      >
        <Copy v-if="isMaximized" :size="13" aria-hidden="true" />
        <Square v-else :size="13" aria-hidden="true" />
      </button>
      <button
        type="button"
        class="titlebar__btn titlebar__btn--danger"
        aria-label="关闭"
        @click="onClose"
      >
        <X :size="15" aria-hidden="true" />
      </button>
    </div>
  </header>
</template>
