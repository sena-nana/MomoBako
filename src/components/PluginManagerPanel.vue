<script setup lang="ts">
import { computed, ref } from "vue";
import { open } from "@tauri-apps/plugin-dialog";
import { Power, RefreshCw, Trash2, Upload } from "lucide-vue-next";
import ConfirmDialog from "./ConfirmDialog.vue";
import {
  deletePluginInWorkspace,
  installPluginArchiveInWorkspace,
  loadSettingsData,
  setPluginEnabledInWorkspace,
  useRepositoryWorkspace,
} from "../composables/useRepositoryWorkspace";
import type { PluginManifest } from "../types/repository";

withDefaults(defineProps<{
  title?: string;
  eyebrow?: string;
  subline?: string;
  searchPlaceholder?: string;
  emptyTitle?: string;
  emptyDescription?: string;
}>(), {
  title: "插件系统",
  eyebrow: "拓展能力",
  subline: "集中管理当前可用插件与后端能力。",
  searchPlaceholder: "筛选导入器、脚本或元数据拓展",
  emptyTitle: "暂无匹配插件",
  emptyDescription: "调整筛选条件，或先从压缩包安装新的插件。",
});

const {
  plugins,
  isLoadingSettingsData,
  isManagingPlugins,
  error,
} = useRepositoryWorkspace();

const keyword = ref("");
const actionError = ref("");
const actionMessage = ref("");
const pendingDeletePlugin = ref<PluginManifest | null>(null);

const filteredPlugins = computed(() => {
  const normalizedKeyword = keyword.value.trim().toLowerCase();
  if (!normalizedKeyword) return plugins.value;
  return plugins.value.filter((plugin) => (
    plugin.name.toLowerCase().includes(normalizedKeyword) ||
    plugin.description.toLowerCase().includes(normalizedKeyword) ||
    plugin.kind.toLowerCase().includes(normalizedKeyword) ||
    plugin.capabilities.some((capability) => capability.toLowerCase().includes(normalizedKeyword))
  ));
});

function pluginSourceLabel(source: PluginManifest["source"]) {
  if (source === "user") return "用户插件";
  if (source === "system") return "系统插件";
  return "内置插件";
}

function pluginRuntimeLabel(runtime: PluginManifest["runtime"]) {
  if (runtime === "native-dylib") return "原生动态库";
  if (runtime === "vue-module") return "Vue 模块";
  if (runtime === "manifest-only") return "仅清单";
  return "未知运行时";
}

function pluginStatusLabel(plugin: PluginManifest) {
  if (!plugin.enabled || plugin.status === "disabled") return "未启用";
  if (plugin.status === "unavailable") return "不可用";
  if (plugin.status === "error") return "错误";
  return "已启用";
}

function pluginStatusClass(plugin: PluginManifest) {
  if (!plugin.enabled || plugin.status === "disabled") return "asset-card__pill--ghost";
  if (plugin.status === "unavailable" || plugin.status === "error") return "asset-card__pill--danger";
  return "";
}

function canDeletePlugin(plugin: PluginManifest) {
  return plugin.source === "user";
}

function resetActionState() {
  actionError.value = "";
  actionMessage.value = "";
}

async function refreshPlugins() {
  resetActionState();
  await loadSettingsData();
  if (error.value) {
    actionError.value = error.value;
  }
}

async function choosePluginArchive() {
  resetActionState();
  const selected = await open({
    title: "选择插件压缩包",
    multiple: false,
    filters: [
      {
        name: "MomoBako 插件",
        extensions: ["zip"],
      },
    ],
  });
  if (typeof selected !== "string" || !selected.trim()) return;

  const response = await installPluginArchiveInWorkspace(selected);
  if (response) {
    actionMessage.value = "插件已安装。";
  } else {
    actionError.value = error.value ?? "插件安装失败。";
  }
}

async function togglePlugin(plugin: PluginManifest) {
  resetActionState();
  const response = await setPluginEnabledInWorkspace(plugin.pluginId, !plugin.enabled);
  if (response) {
    actionMessage.value = plugin.enabled ? "插件已禁用。" : "插件已启用。";
  } else {
    actionError.value = error.value ?? "插件状态更新失败。";
  }
}

function requestDeletePlugin(plugin: PluginManifest) {
  if (!canDeletePlugin(plugin)) return;
  pendingDeletePlugin.value = plugin;
}

function cancelDeletePlugin() {
  pendingDeletePlugin.value = null;
}

async function confirmDeletePlugin() {
  const plugin = pendingDeletePlugin.value;
  if (!plugin) return;

  resetActionState();
  const response = await deletePluginInWorkspace(plugin.pluginId);
  if (response) {
    actionMessage.value = "插件已删除。";
    pendingDeletePlugin.value = null;
  } else {
    actionError.value = error.value ?? "插件删除失败。";
  }
}
</script>

<template>
  <div class="search-workbench__panel plugin-manager">
    <header class="search-workbench__header">
      <div>
        <p class="asset-browser__eyebrow">{{ eyebrow }}</p>
        <h1>{{ title }}</h1>
        <p class="search-workbench__subline">{{ subline }}</p>
      </div>
      <div class="search-workbench__stats">
        <span class="asset-stat">{{ filteredPlugins.length }} 个插件</span>
        <button type="button" class="ghost" :disabled="isManagingPlugins" @click="refreshPlugins">
          <RefreshCw :size="14" aria-hidden="true" />
          刷新
        </button>
        <button type="button" class="primary" :disabled="isManagingPlugins" @click="choosePluginArchive">
          <Upload :size="14" aria-hidden="true" />
          从压缩包安装
        </button>
      </div>
    </header>

    <div v-if="actionError" class="asset-browser__state asset-browser__state--error">
      {{ actionError }}
    </div>
    <div v-else-if="actionMessage" class="asset-browser__state">
      {{ actionMessage }}
    </div>
    <div v-else-if="error && !isLoadingSettingsData" class="asset-browser__state asset-browser__state--error">
      {{ error }}
    </div>

    <label class="search-workbench__field">
      <input
        v-model="keyword"
        type="search"
        :placeholder="searchPlaceholder"
      />
    </label>

    <div v-if="isLoadingSettingsData" class="workspace-state">
      正在加载插件信息
    </div>
    <div v-else-if="!filteredPlugins.length" class="search-workbench__empty">
      <h2>{{ emptyTitle }}</h2>
      <p>{{ emptyDescription }}</p>
    </div>
    <div v-else class="extensions-workbench__list">
      <article v-for="plugin in filteredPlugins" :key="plugin.pluginId" class="extensions-workbench__card">
        <div class="extensions-workbench__card-head">
          <strong>{{ plugin.name }}</strong>
          <span class="asset-card__pill" :class="pluginStatusClass(plugin)">
            {{ pluginStatusLabel(plugin) }}
          </span>
        </div>
        <p class="extensions-workbench__card-desc">{{ plugin.description }}</p>
        <div class="plugin-manager__meta">
          <span class="muted">{{ plugin.pluginId }}</span>
          <span class="muted">v{{ plugin.version }}</span>
        </div>
        <div class="settings-list__chips">
          <span class="workspace-hints__chip">{{ plugin.kind }}</span>
          <span class="workspace-hints__chip">{{ pluginSourceLabel(plugin.source) }}</span>
          <span class="workspace-hints__chip">{{ pluginRuntimeLabel(plugin.runtime) }}</span>
          <span v-for="capability in plugin.capabilities" :key="capability" class="workspace-hints__chip">
            {{ capability }}
          </span>
        </div>
        <div class="extensions-workbench__card-actions">
          <button type="button" class="ghost" :disabled="isManagingPlugins" @click="togglePlugin(plugin)">
            <Power :size="14" aria-hidden="true" />
            {{ plugin.enabled ? "禁用" : "启用" }}
          </button>
          <button
            v-if="canDeletePlugin(plugin)"
            type="button"
            class="ghost danger"
            :disabled="isManagingPlugins"
            @click="requestDeletePlugin(plugin)"
          >
            <Trash2 :size="14" aria-hidden="true" />
            删除
          </button>
        </div>
      </article>
    </div>

    <ConfirmDialog
      :open="Boolean(pendingDeletePlugin)"
      title="删除插件"
      :message="pendingDeletePlugin ? `删除插件“${pendingDeletePlugin.name}”后将移除其安装目录。` : ''"
      confirm-text="删除"
      cancel-text="取消"
      :danger="true"
      :busy="isManagingPlugins"
      busy-text="删除中..."
      @cancel="cancelDeletePlugin"
      @confirm="confirmDeletePlugin"
    />
  </div>
</template>
