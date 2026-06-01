<script setup lang="ts">
import { onBeforeUnmount, ref } from "vue";
import { RouterView } from "vue-router";
import TitleBar from "../components/TitleBar.vue";
import SecondaryPanel from "./SecondaryPanel.vue";

const MIN_WIDTH = 180;
const MAX_WIDTH = 480;
const DEFAULT_WIDTH = 220;
const STORAGE_KEY = "tauri-template.sidebarWidth";

function clamp(value: number) {
  return Math.min(MAX_WIDTH, Math.max(MIN_WIDTH, value));
}

function loadInitial(): number {
  const raw =
    typeof localStorage !== "undefined" ? localStorage.getItem(STORAGE_KEY) : null;
  const parsed = raw ? Number.parseFloat(raw) : NaN;
  return Number.isFinite(parsed) ? clamp(parsed) : DEFAULT_WIDTH;
}

const sidebarWidth = ref(loadInitial());
const isResizing = ref(false);

let startX = 0;
let startWidth = 0;

function saveWidth(value: number) {
  try {
    localStorage.setItem(STORAGE_KEY, String(value));
  } catch {
    // 本地存储不可用时只影响下次启动的默认宽度。
  }
}

function onPointerMove(event: PointerEvent) {
  sidebarWidth.value = clamp(startWidth + event.clientX - startX);
}

function onPointerUp(event: PointerEvent) {
  isResizing.value = false;
  window.removeEventListener("pointermove", onPointerMove);
  window.removeEventListener("pointerup", onPointerUp);
  (event.target as Element | null)?.releasePointerCapture?.(event.pointerId);
  saveWidth(sidebarWidth.value);
}

function startResize(event: PointerEvent) {
  if (event.button !== 0) return;
  event.preventDefault();
  isResizing.value = true;
  startX = event.clientX;
  startWidth = sidebarWidth.value;
  (event.currentTarget as Element).setPointerCapture?.(event.pointerId);
  window.addEventListener("pointermove", onPointerMove);
  window.addEventListener("pointerup", onPointerUp);
}

function resetWidth() {
  sidebarWidth.value = DEFAULT_WIDTH;
  saveWidth(DEFAULT_WIDTH);
}

onBeforeUnmount(() => {
  window.removeEventListener("pointermove", onPointerMove);
  window.removeEventListener("pointerup", onPointerUp);
});
</script>

<template>
  <div
    class="shell"
    :class="{ 'is-resizing': isResizing }"
    :style="{ '--sidebar-width': sidebarWidth + 'px' }"
  >
    <TitleBar />
    <SecondaryPanel />
    <div
      class="shell__resizer"
      role="separator"
      aria-orientation="vertical"
      :aria-valuenow="sidebarWidth"
      :aria-valuemin="MIN_WIDTH"
      :aria-valuemax="MAX_WIDTH"
      title="拖动调整侧栏宽度（双击恢复默认）"
      @pointerdown="startResize"
      @dblclick="resetWidth"
    />
    <main class="shell__main">
      <RouterView />
    </main>
  </div>
</template>
