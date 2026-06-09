<script setup lang="ts">
import { computed, defineAsyncComponent, onMounted, ref } from "vue";
import { RouterView } from "vue-router";
import { RefreshCw } from "lucide-vue-next";
import TitleBar from "../components/TitleBar.vue";
import { useRepositoryWorkspace } from "../composables/useRepositoryWorkspace";
import { useResizablePane } from "../composables/useResizablePane";

const MIN_WIDTH = 220;
const MAX_WIDTH = 480;
const DEFAULT_WIDTH = 276;
const WIDTH_STORAGE_KEY = "momobako.sidebarWidth";
const COLLAPSED_STORAGE_KEY = "momobako.sidebarCollapsed";
const SecondaryPanel = defineAsyncComponent(() => import("./SecondaryPanel.vue"));

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
const {
  workspaceStartup,
  ensureRepositoryWorkspace,
} = useRepositoryWorkspace();
const isWorkspaceReady = computed(() => workspaceStartup.value.status === "ready");
const isWorkspaceStartupError = computed(() => workspaceStartup.value.status === "error");
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

function retryWorkspaceStartup() {
  void ensureRepositoryWorkspace();
}

onMounted(() => {
  void ensureRepositoryWorkspace();
});
</script>

<template>
  <div
    class="shell"
    :class="{
      'is-resizing': sidebarWidth.isResizing.value,
      'is-sidebar-collapsed': sidebarCollapsed,
      'is-starting-workspace': !isWorkspaceReady,
    }"
    :style="{ '--sidebar-width': sidebarCollapsed ? '0px' : sidebarWidth.width.value + 'px' }"
  >
    <TitleBar
      :left-sidebar-collapsed="sidebarCollapsed"
      @toggle-left-sidebar="toggleSidebarCollapsed"
    />
    <SecondaryPanel v-if="isWorkspaceReady" />
    <div
      v-if="isWorkspaceReady"
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
      <section v-if="!isWorkspaceReady" class="workspace-startup" aria-live="polite">
        <div class="workspace-startup__panel">
          <p class="asset-browser__eyebrow">MomoBako</p>
          <h1>{{ workspaceStartup.stepLabel }}</h1>
          <p class="workspace-startup__meta">
            第 {{ workspaceStartup.currentStep }} / {{ workspaceStartup.totalSteps }} 步
          </p>
          <div
            class="workspace-startup__progress"
            role="progressbar"
            :aria-valuenow="workspaceStartup.percent"
            aria-valuemin="0"
            aria-valuemax="100"
            :aria-label="workspaceStartup.stepLabel"
          >
            <span :style="{ width: `${workspaceStartup.percent}%` }"></span>
          </div>
          <p v-if="workspaceStartup.error" class="workspace-startup__error">
            {{ workspaceStartup.error }}
          </p>
          <button
            v-if="isWorkspaceStartupError"
            type="button"
            class="ghost workspace-startup__retry"
            @click="retryWorkspaceStartup"
          >
            <RefreshCw :size="14" aria-hidden="true" />
            重试
          </button>
        </div>
      </section>
      <RouterView v-else />
    </main>
  </div>
</template>
