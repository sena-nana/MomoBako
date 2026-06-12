<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { RouterView } from "vue-router";
import { RefreshCw } from "lucide-vue-next";
import TitleBar from "../components/TitleBar.vue";
import SecondaryPanel from "./SecondaryPanel.vue";
import { useRepositoryWorkspace } from "../composables/useRepositoryWorkspace";
import { usePlaylistPlayer } from "../composables/usePlaylistPlayer";
import { useResizablePane } from "../composables/useResizablePane";
import { getPlaylistDetail } from "../services/repositoryApi";

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

function readPlaybackSession(repoId: string) {
  try {
    return localStorage.getItem(`momobako.playbackSession:${repoId}`);
  } catch {
    return null;
  }
}

const sidebarCollapsed = ref(readStorage(COLLAPSED_STORAGE_KEY) === "1");
const playerMountRef = ref<HTMLElement | null>(null);

const {
  activeRepoId,
  playlists,
  workspaceStartup,
  ensureRepositoryWorkspace,
} = useRepositoryWorkspace();
const player = usePlaylistPlayer();

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

watch(playerMountRef, (element) => {
  player.attachMountTarget(element);
}, { immediate: true });

watch(activeRepoId, async (repoId, previousRepoId) => {
  if (previousRepoId && previousRepoId !== repoId) {
    await player.stop();
  }
});

watch(
  [activeRepoId, playlists],
  async ([repoId, playlistItems]) => {
    if (!repoId || !playlistItems.length || player.activeRepoId.value === repoId || !readPlaybackSession(repoId)) return;
    const restored = await Promise.all(playlistItems.map(async (playlist) => {
      const detail = await getPlaylistDetail(repoId, playlist.playlistId);
      if (!detail) return false;
      return player.restoreSession(repoId, detail);
    }));
    if (!restored.some(Boolean)) {
      player.clearSession(repoId);
    }
  },
  { immediate: true },
);

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
    :style="{ '--sidebar-width': sidebarCollapsed ? '0px' : `${sidebarWidth.width.value}px` }"
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

    <div ref="playerMountRef" class="workspace-player-host" aria-hidden="true"></div>
  </div>
</template>
