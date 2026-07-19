<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { RouterView } from "vue-router";
import { PanelLeftClose, PanelLeftOpen, RefreshCw } from "@lucide/vue";
import WorkspaceTitleBarSearch from "../components/WorkspaceTitleBarSearch.vue";
import SecondaryPanel from "./SecondaryPanel.vue";
import {
  useWorkspacePlaylists,
  useWorkspaceProgress,
  useWorkspaceRepository,
} from "../composables/useRepositoryWorkspace";
import { usePlaylistPlayer } from "../composables/usePlaylistPlayer";
import { useSystemMediaSession } from "../composables/useSystemMediaSession";
import { getCachedPlaylistDetail } from "../composables/workspace/playlists";
import {
  LiliaPrimaryContent,
  LiliaResourcePanel,
  LiliaWorkspace,
} from "../ui";
import { appUIPreset } from "../ui/preset";
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

/** 读取并约束持久化的侧栏宽度。 */
function readSidebarWidth() {
  const raw = readStorage(WIDTH_STORAGE_KEY);
  if (raw === null) return DEFAULT_WIDTH;
  const stored = Number(raw);
  if (!Number.isFinite(stored)) return DEFAULT_WIDTH;
  return Math.min(MAX_WIDTH, Math.max(MIN_WIDTH, stored));
}

function readPlaybackPlaylistId(repoId: string) {
  const raw = readStorage(`momobako.playbackSession:${repoId}`);
  if (!raw) return null;
  try {
    const session = JSON.parse(raw) as { playlistId?: unknown };
    return typeof session.playlistId === "string" && session.playlistId.trim()
      ? session.playlistId
      : null;
  } catch {
    return null;
  }
}

const sidebarCollapsed = ref(readStorage(COLLAPSED_STORAGE_KEY) === "1");
const sidebarWidth = ref(readSidebarWidth());
const playerMountRef = ref<HTMLElement | null>(null);

const {
  activeRepoId,
  ensureRepositoryWorkspace,
} = useWorkspaceRepository();
const { playlists } = useWorkspacePlaylists();
const { workspaceStartup } = useWorkspaceProgress();
const player = usePlaylistPlayer();
const systemMediaSession = useSystemMediaSession(player);
const hasRenderedWorkspace = ref(false);
const startupStepHints = [
  {
    label: "准备资源库",
    detail: "读取仓库列表或切换目标资源库。",
  },
  {
    label: "同步文件变化",
    detail: "扫描新增、移动、删除和缓存状态。",
  },
  {
    label: "读取资源索引",
    detail: "整理摘要、素材索引和默认预览对象。",
  },
  {
    label: "加载首屏内容",
    detail: "准备目录、播放列表和首屏辅助数据。",
  },
];

const isWorkspaceReady = computed(() => workspaceStartup.value.status === "ready");
const isWorkspaceStartupError = computed(() => workspaceStartup.value.status === "error");
const startupVisibleLogs = computed(() => workspaceStartup.value.logs.slice(-8).reverse());
const startupStepItems = computed(() => startupStepHints.map((step, index) => {
  const stepNumber = index + 1;
  const isCurrent = workspaceStartup.value.currentStep === stepNumber;
  const isDone = workspaceStartup.value.currentStep > stepNumber || workspaceStartup.value.status === "ready";
  const isError = isCurrent && workspaceStartup.value.status === "error";
  return {
    ...step,
    stepNumber,
    state: isError ? "error" : isDone ? "done" : isCurrent ? "current" : "pending",
  };
}));

watch(sidebarCollapsed, (value) => {
  writeStorage(COLLAPSED_STORAGE_KEY, value ? "1" : "0");
});

/** 保存 Workspace Region 完成调整后的宽度。 */
function persistSidebarWidth(value: number) {
  writeStorage(WIDTH_STORAGE_KEY, String(value));
}

function retryWorkspaceStartup() {
  void ensureRepositoryWorkspace();
}

watch(playerMountRef, (element) => {
  player.attachMountTarget(element);
}, { immediate: true });

watch(isWorkspaceReady, (ready) => {
  if (ready) {
    hasRenderedWorkspace.value = true;
  }
}, { immediate: true });

watch(activeRepoId, async (repoId, previousRepoId) => {
  if (previousRepoId && previousRepoId !== repoId) {
    await player.stop();
  }
});

watch(
  [activeRepoId, playlists],
  async ([repoId, playlistItems]) => {
    const playlistId = repoId ? readPlaybackPlaylistId(repoId) : null;
    if (!repoId || !playlistId || !playlistItems.some((playlist) => playlist.playlistId === playlistId) || player.activeRepoId.value === repoId) return;
    const cachedDetail = getCachedPlaylistDetail(repoId, playlistId);
    if (cachedDetail) {
      await player.restoreSession(repoId, cachedDetail);
    }
    const detail = await getPlaylistDetail(repoId, playlistId);
    const restored = await player.restoreSession(repoId, detail);
    if (!restored) {
      player.clearSession(repoId);
    }
  },
  { immediate: true },
);

onMounted(() => {
  void ensureRepositoryWorkspace();
});

onBeforeUnmount(() => {
  systemMediaSession.dispose();
});
</script>

<template>
  <component :is="appUIPreset.shell" title="MomoBako">
    <template #header-leading>
      <button
        type="button"
        class="titlebar__btn titlebar__left-sidebar-btn"
        :aria-label="sidebarCollapsed ? '展开侧边栏' : '折叠侧边栏'"
        :title="sidebarCollapsed ? '展开侧边栏' : '折叠侧边栏'"
        :aria-pressed="sidebarCollapsed"
        @click="sidebarCollapsed = !sidebarCollapsed"
      >
        <PanelLeftOpen v-if="sidebarCollapsed" :size="15" aria-hidden="true" />
        <PanelLeftClose v-else :size="15" aria-hidden="true" />
      </button>
    </template>
    <template #header-center>
      <WorkspaceTitleBarSearch />
    </template>

    <LiliaWorkspace
      class="shell"
      aria-label="MomoBako 工作区"
    >
      <LiliaResourcePanel
        v-if="hasRenderedWorkspace"
        id="repository-sidebar"
        role="resources"
        class="shell__sidebar-region"
        v-model:size="sidebarWidth"
        :default-size="DEFAULT_WIDTH"
        :min-size="MIN_WIDTH"
        :max-size="MAX_WIDTH"
        v-model:collapsed="sidebarCollapsed"
        :hidden="!isWorkspaceReady"
        collapsible
        resizable
        overflow="hidden"
        narrow-behavior="overlay"
        :collapse-below="720"
        resize-label="拖动调整侧边栏宽度（双击恢复默认）"
        @resize-end="persistSidebarWidth"
      >
        <SecondaryPanel />
      </LiliaResourcePanel>

      <LiliaPrimaryContent
        id="workspace-primary"
        role="primary"
        class="shell__main"
        overflow="auto"
      >
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
            <p
              v-if="workspaceStartup.stepDetail"
              id="workspace-startup-detail"
              class="workspace-startup__detail"
            >
              {{ workspaceStartup.stepDetail }}
            </p>
            <ol class="workspace-startup__steps" aria-label="加载步骤">
              <li
                v-for="item in startupStepItems"
                :key="item.stepNumber"
                class="workspace-startup__step"
                :class="`is-${item.state}`"
              >
                <span class="workspace-startup__step-index">{{ item.stepNumber }}</span>
                <span class="workspace-startup__step-copy">
                  <strong>{{ item.label }}</strong>
                  <small>{{ item.detail }}</small>
                </span>
              </li>
            </ol>
            <p v-if="workspaceStartup.error" class="workspace-startup__error">
              {{ workspaceStartup.error }}
            </p>
            <section
              v-if="startupVisibleLogs.length"
              class="workspace-startup__logs"
              aria-label="首屏加载日志"
            >
              <header class="workspace-startup__logs-head">
                <strong>加载日志</strong>
                <span>{{ startupVisibleLogs.length }} 条最近记录</span>
              </header>
              <ol>
                <li
                  v-for="record in startupVisibleLogs"
                  :key="record.id"
                  :class="`is-${record.level}`"
                >
                  <time>{{ new Date(record.timestamp).toLocaleTimeString("zh-CN", { hour12: false }) }}</time>
                  <span>
                    <strong>{{ record.message }}</strong>
                    <small v-if="record.detail">{{ record.detail }}</small>
                  </span>
                </li>
              </ol>
            </section>
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
        <div
          v-if="hasRenderedWorkspace"
          class="shell__workspace-layer"
          :style="{ display: isWorkspaceReady ? 'contents' : 'none' }"
        >
          <RouterView />
        </div>
      </LiliaPrimaryContent>
    </LiliaWorkspace>

    <div ref="playerMountRef" class="workspace-player-host" aria-hidden="true"></div>
  </component>
</template>
