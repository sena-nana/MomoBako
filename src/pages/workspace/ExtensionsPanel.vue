<script setup lang="ts">
import { Search } from "lucide-vue-next";
import type { PluginManifest } from "../../types/repository";
import { pluginCategory, pluginCategoryLabel } from "../../utils/pluginTaxonomy";

defineProps<{
  plugins: PluginManifest[];
  keyword: string;
}>();

const emit = defineEmits<{
  "update:keyword": [value: string];
}>();

function updateKeyword(event: Event) {
  emit("update:keyword", (event.target as HTMLInputElement | null)?.value ?? "");
}
</script>

<template>
  <section class="extensions-workbench">
    <div class="search-workbench__panel">
      <header class="search-workbench__header">
        <div>
          <p class="asset-browser__eyebrow">拓展能力</p>
          <h1>文件系统与插件</h1>
          <p class="search-workbench__subline">这里集中展示当前插件和后端能力。</p>
        </div>
        <div class="search-workbench__stats">
          <span class="asset-stat">{{ plugins.length }} 个插件</span>
        </div>
      </header>

      <label class="search-workbench__field">
        <Search :size="15" aria-hidden="true" />
        <input
          :value="keyword"
          type="search"
          placeholder="筛选导入器、脚本或元数据拓展"
          @input="updateKeyword"
        />
      </label>

      <div class="extensions-workbench__list">
        <article v-for="plugin in plugins" :key="plugin.pluginId" class="extensions-workbench__card">
          <div class="extensions-workbench__card-head">
            <strong>{{ plugin.name }}</strong>
            <span class="asset-card__pill" :class="{ 'asset-card__pill--ghost': !plugin.enabled }">
              {{ plugin.enabled ? "已启用" : "未启用" }}
            </span>
          </div>
          <p class="extensions-workbench__card-desc">{{ plugin.description }}</p>
          <div class="settings-list__chips">
            <span class="workspace-hints__chip">{{ pluginCategoryLabel(pluginCategory(plugin)) }}</span>
            <span class="workspace-hints__chip">{{ plugin.kind }}</span>
            <span v-for="capability in plugin.capabilities" :key="capability" class="workspace-hints__chip">
              {{ capability }}
            </span>
          </div>
        </article>
      </div>
    </div>
  </section>
</template>
