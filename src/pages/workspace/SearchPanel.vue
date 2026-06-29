<script setup lang="ts">
import { FileImage, LoaderCircle } from "@lucide/vue";
import type { SearchHit } from "../../types/repository";

defineProps<{
  isSearching: boolean;
  repositoriesCount: number;
  results: SearchHit[];
  scopeLabel: string;
  summary: string;
  resultContext: (result: SearchHit) => string[];
}>();

const emit = defineEmits<{
  openResult: [result: SearchHit];
}>();
</script>

<template>
  <section class="search-workbench">
    <div class="search-workbench__panel">
      <header class="search-workbench__header">
        <div>
          <p class="asset-browser__eyebrow">{{ scopeLabel }}</p>
          <h1>搜索结果</h1>
          <p class="search-workbench__subline">{{ summary }}</p>
        </div>
        <div class="search-workbench__stats">
          <span class="asset-stat">{{ repositoriesCount }} 个仓库</span>
          <span class="asset-stat">{{ results.length }} 条结果</span>
        </div>
      </header>

      <div v-if="isSearching" class="asset-browser__state">
        <LoaderCircle class="spin" :size="16" aria-hidden="true" />
        正在执行全局搜索
      </div>

      <div v-if="!repositoriesCount" class="search-workbench__empty">
        <h2>还没有可搜索的资源库</h2>
        <p>先在资源库页面添加一个仓库，再执行跨仓库搜索。</p>
      </div>

      <div v-else-if="!isSearching && !results.length" class="search-workbench__empty">
        <h2>等待搜索条件</h2>
        <p>输入关键词、标签或评分条件后，这里会展示结果。</p>
      </div>

      <div v-else class="search-workbench__results">
        <button
          v-for="result in results"
          :key="`${result.repoId}:${result.assetId}`"
          type="button"
          class="search-workbench__item"
          @click="emit('openResult', result)"
        >
          <div class="search-workbench__item-icon">
            <FileImage :size="18" aria-hidden="true" />
          </div>
          <div class="search-workbench__item-body">
            <strong>{{ result.filename }}</strong>
            <span>{{ result.repoName }} / {{ result.path }}</span>
          </div>
          <div class="search-workbench__item-tags">
            <span v-for="item in resultContext(result)" :key="item" class="workspace-hints__chip">
              {{ item }}
            </span>
          </div>
        </button>
      </div>
    </div>
  </section>
</template>
