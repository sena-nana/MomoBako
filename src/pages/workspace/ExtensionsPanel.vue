<script setup lang="ts">
import { computed, ref, watch } from "vue";
import PluginManagerPanel from "../../components/PluginManagerPanel.vue";
import type { ToolPageContext } from "../../plugins/sdk";
import { frontendPluginRegistryVersion } from "../../plugins/sdk";
import { listToolPages } from "../../plugins/toolPages";

defineProps<ToolPageContext>();

const toolPages = computed(() => {
  void frontendPluginRegistryVersion.value;
  return listToolPages();
});
const activeToolPageId = ref<string | null>(null);
const activeToolPage = computed(() => (
  toolPages.value.find((page) => page.toolPageId === activeToolPageId.value)
  ?? toolPages.value[0]
  ?? null
));

watch(toolPages, (pages) => {
  if (!pages.length) {
    activeToolPageId.value = null;
    return;
  }
  if (!activeToolPageId.value || !pages.some((page) => page.toolPageId === activeToolPageId.value)) {
    activeToolPageId.value = pages[0].toolPageId;
  }
}, { immediate: true });
</script>

<template>
  <section class="extensions-workbench">
    <div v-if="toolPages.length" class="extensions-workbench__tools">
      <aside class="extensions-workbench__tools-nav" aria-label="插件工具">
        <p class="asset-browser__eyebrow">插件工具</p>
        <button
          v-for="page in toolPages"
          :key="page.toolPageId"
          type="button"
          class="extensions-workbench__tool-tab"
          :class="{ 'is-active': activeToolPage?.toolPageId === page.toolPageId }"
          @click="activeToolPageId = page.toolPageId"
        >
          <strong>{{ page.label }}</strong>
          <span>{{ page.description ?? page.pluginName }}</span>
        </button>
      </aside>
      <div class="extensions-workbench__tool-page">
        <component
          :is="activeToolPage.component"
          v-if="activeToolPage"
          :manifest="activeToolPage.manifest"
          :active-repo-id="activeRepoId"
          :active-repository="activeRepository"
          :current-directory-path="currentDirectoryPath"
          :is-repository-writable="isRepositoryWritable"
          :is-trash-panel="isTrashPanel"
          :is-virtual-view="isVirtualView"
        />
      </div>
    </div>

    <PluginManagerPanel
      class="extensions-workbench__manager"
      title="文件系统与插件"
      eyebrow="拓展能力"
      subline="这里集中展示当前插件和后端能力。"
      search-placeholder="筛选导入器、脚本或元数据拓展"
      empty-title="没有匹配的插件"
      empty-description="试试其他关键词，或从 .momoplug 安装新的插件。"
    />
  </section>
</template>
