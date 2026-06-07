<script setup lang="ts">
import { onMounted, onUnmounted, ref } from "vue";
import {
  Copy,
  Minus,
  PanelLeftClose,
  PanelLeftOpen,
  Square,
  X,
} from "lucide-vue-next";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useRoute, useRouter } from "vue-router";
import { useRepositoryWorkspace } from "../composables/useRepositoryWorkspace";

defineProps<{
  leftSidebarCollapsed?: boolean;
}>();

defineEmits<{
  toggleLeftSidebar: [];
}>();

const isMaximized = ref(false);
const appWindow = safeCurrentWindow();
const route = useRoute();
const router = useRouter();
const {
  searchQuery,
  runSearch,
  setActivePanel,
} = useRepositoryWorkspace();
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

function onSearchInput(event: Event) {
  const query = event.target instanceof HTMLInputElement ? event.target.value : "";
  setActivePanel("search");
  if (route.path !== "/") {
    void router.push("/");
  }
  void runSearch({ query });
}

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
    <div class="titlebar__brand" data-tauri-drag-region>
      <button
        type="button"
        class="titlebar__btn titlebar__left-sidebar-btn"
        :aria-label="leftSidebarCollapsed ? '展开侧边栏' : '折叠侧边栏'"
        :title="leftSidebarCollapsed ? '展开侧边栏' : '折叠侧边栏'"
        :aria-pressed="leftSidebarCollapsed"
        @click="$emit('toggleLeftSidebar')"
      >
        <PanelLeftOpen
          v-if="leftSidebarCollapsed"
          :size="15"
          aria-hidden="true"
        />
        <PanelLeftClose
          v-else
          :size="15"
          aria-hidden="true"
        />
      </button>
    </div>
    <label class="titlebar__search" aria-label="全局搜索">
      <input
        :value="searchQuery"
        type="search"
        aria-label="全局搜索"
        placeholder="搜索文件名、标签、元数据"
        @input="onSearchInput"
      />
    </label>
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
