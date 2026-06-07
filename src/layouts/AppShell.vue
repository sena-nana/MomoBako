<script setup lang="ts">
import { ref } from "vue";
import { RouterView } from "vue-router";
import TitleBar from "../components/TitleBar.vue";
import SecondaryPanel from "./SecondaryPanel.vue";
import { useResizablePane } from "../composables/useResizablePane";

const MIN_WIDTH = 220;
const MAX_WIDTH = 480;
const DEFAULT_WIDTH = 276;
const WIDTH_STORAGE_KEY = "momobako.sidebarWidth";
const COLLAPSED_STORAGE_KEY = "momobako.sidebarCollapsed";

function readStorage(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function writeStorage(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    /* ignore */
  }
}

const sidebarCollapsed = ref(readStorage(COLLAPSED_STORAGE_KEY) === "1");
const sidebarWidth = useResizablePane({
  storageKey: WIDTH_STORAGE_KEY,
  minWidth: MIN_WIDTH,
  maxWidth: MAX_WIDTH,
  defaultWidth: DEFAULT_WIDTH,
  edge: "right",
  disabled: sidebarCollapsed,
});

function toggleSidebarCollapsed() {
  sidebarCollapsed.value = !sidebarCollapsed.value;
  writeStorage(COLLAPSED_STORAGE_KEY, sidebarCollapsed.value ? "1" : "0");
}
</script>

<template>
  <div
    class="shell"
    :class="{
      'is-resizing': sidebarWidth.isResizing.value,
      'is-sidebar-collapsed': sidebarCollapsed,
    }"
    :style="{ '--sidebar-width': sidebarCollapsed ? '0px' : sidebarWidth.width.value + 'px' }"
  >
    <TitleBar
      :left-sidebar-collapsed="sidebarCollapsed"
      @toggle-left-sidebar="toggleSidebarCollapsed"
    />
    <SecondaryPanel />
    <div
      class="shell__resizer"
      role="separator"
      aria-orientation="vertical"
      :aria-disabled="sidebarCollapsed ? 'true' : undefined"
      :aria-valuenow="sidebarWidth.width.value"
      :aria-valuemin="MIN_WIDTH"
      :aria-valuemax="MAX_WIDTH"
      title="拖动调整侧边栏宽度（双击恢复默认）"
      @pointerdown="sidebarWidth.startResize"
      @dblclick="sidebarWidth.resetWidth"
    />
    <main class="shell__main">
      <RouterView />
    </main>
  </div>
</template>
