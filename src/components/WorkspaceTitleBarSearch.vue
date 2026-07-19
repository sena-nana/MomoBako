<!-- MomoBako 工作区标题栏的搜索与筛选内容。 -->
<script setup lang="ts">
import { SlidersHorizontal } from "@lucide/vue";
import { useRoute, useRouter } from "vue-router";
import {
  useWorkspaceNavigation,
  useWorkspaceSearch,
} from "../composables/useRepositoryWorkspace";

const route = useRoute();
const router = useRouter();
const {
  activeFilterCount,
  hasActiveFilters,
  isFilterBarOpen,
  searchQuery,
  runSearch,
  toggleFilterBar,
} = useWorkspaceSearch();
const { setActivePanel } = useWorkspaceNavigation();

/** 将标题栏输入同步到工作区搜索面板。 */
function onSearchInput(event: Event) {
  const query = event.target instanceof HTMLInputElement ? event.target.value : "";
  setActivePanel("search");
  if (route.path !== "/") {
    void router.push("/");
  }
  void runSearch({ query });
}

/** 切换筛选栏并确保用户回到工作区。 */
function onToggleFilterBar() {
  toggleFilterBar();
  setActivePanel("search");
  if (route.path !== "/") {
    void router.push("/");
  }
}
</script>

<template>
  <div class="titlebar__search-group">
    <label class="titlebar__search" aria-label="全局搜索">
      <input
        :value="searchQuery"
        type="search"
        aria-label="全局搜索"
        placeholder="搜索文件名、标签、元数据"
        @input="onSearchInput"
      />
    </label>
    <button
      type="button"
      class="titlebar__filter-btn"
      :class="{ 'is-active': isFilterBarOpen || hasActiveFilters }"
      :aria-label="isFilterBarOpen ? '隐藏筛选栏' : '显示筛选栏'"
      :aria-pressed="isFilterBarOpen"
      title="筛选"
      @click="onToggleFilterBar"
    >
      <SlidersHorizontal :size="14" aria-hidden="true" />
      <span v-if="activeFilterCount" class="titlebar__filter-count">{{ activeFilterCount }}</span>
    </button>
  </div>
</template>
