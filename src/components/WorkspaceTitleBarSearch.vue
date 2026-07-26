<!-- MomoBako 工作区标题栏的搜索与筛选内容。 -->
<script setup lang="ts">
import { SlidersHorizontal } from "@lucide/vue";
import { onBeforeUnmount } from "vue";
import { useRoute, useRouter } from "vue-router";
import {
  useWorkspaceNavigation,
  useWorkspaceSearch,
} from "../composables/useRepositoryWorkspace";

const SEARCH_DEBOUNCE_MS = 250;

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
let searchTimer: ReturnType<typeof window.setTimeout> | null = null;

/** 清理尚未触发的标题栏搜索。 */
function clearSearchTimer() {
  if (searchTimer === null) return;
  window.clearTimeout(searchTimer);
  searchTimer = null;
}

/** 打开工作区搜索面板。 */
function showSearchWorkspace() {
  setActivePanel("search");
  if (route.path !== "/") {
    void router.push("/");
  }
}

/** 将标题栏输入同步到工作区搜索面板。 */
function onSearchInput(event: Event) {
  const query = event.target instanceof HTMLInputElement ? event.target.value : "";
  showSearchWorkspace();
  clearSearchTimer();
  searchTimer = window.setTimeout(() => {
    searchTimer = null;
    void runSearch({ query });
  }, SEARCH_DEBOUNCE_MS);
}

/** 切换筛选栏并确保用户回到工作区。 */
function onToggleFilterBar() {
  toggleFilterBar();
  showSearchWorkspace();
}

onBeforeUnmount(clearSearchTimer);
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
