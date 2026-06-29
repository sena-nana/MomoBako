<script setup lang="ts">
import { X } from "@lucide/vue";
import type { WorkspaceFilterState } from "../../../composables/workspace/state";

type LibrarySearchShortcut = {
  extension: { pluginId: string };
  shortcut: {
    id: string;
    label: string;
    metadataFilters: string;
    sort?: {
      field?: string;
      direction?: "asc" | "desc" | string;
    };
  };
};

defineProps<{
  activeFilterCount: number;
  activeLibrarySearchShortcuts: LibrarySearchShortcut[];
  colorFilterOptions: string[];
  filters: WorkspaceFilterState;
  formatFilterOptions: string[];
  hasActiveFilters: boolean;
  repositoryName?: string;
  ratingFilterOptions: number[];
  searchQuery: string;
  shapeFilterOptions: string[];
  tagFilterOptions: string[];
  filterColorStyle: (color: string) => Record<string, string>;
}>();

const colorFilterInput = defineModel<string>("colorFilterInput", { required: true });
const dateFiltersInput = defineModel<string>("dateFiltersInput", { required: true });
const excludeDateFiltersInput = defineModel<string>("excludeDateFiltersInput", { required: true });
const excludeFormatsInput = defineModel<string>("excludeFormatsInput", { required: true });
const excludeMetadataFiltersInput = defineModel<string>("excludeMetadataFiltersInput", { required: true });
const excludeNumberFiltersInput = defineModel<string>("excludeNumberFiltersInput", { required: true });
const excludePathPrefixesInput = defineModel<string>("excludePathPrefixesInput", { required: true });
const excludeQueryInput = defineModel<string>("excludeQueryInput", { required: true });
const excludeTagsInput = defineModel<string>("excludeTagsInput", { required: true });
const limitInput = defineModel<string | number>("limitInput", { required: true });
const metadataFiltersInput = defineModel<string>("metadataFiltersInput", { required: true });
const numberFiltersInput = defineModel<string>("numberFiltersInput", { required: true });
const shapeFilterInput = defineModel<string>("shapeFilterInput", { required: true });
const sortDirectionInput = defineModel<"asc" | "desc">("sortDirectionInput", { required: true });
const sortFieldInput = defineModel<string>("sortFieldInput", { required: true });

const emit = defineEmits<{
  applyAdvancedSearchFilters: [];
  applyMetadataFilterShortcut: [metadataFilters: string, sortField: string, sortDirection: "asc" | "desc"];
  clearSearchFilters: [];
  close: [];
  selectMinimumRating: [rating: number | null];
  submitMetadataFilterInput: [key: "colors" | "shapes"];
  toggleSearchFilter: [key: "tags" | "formats" | "colors" | "shapes", value: string];
}>();
</script>

<template>
  <div class="workspace-filter-bar" aria-label="资源筛选">
    <div class="workspace-filter-bar__head">
      <div>
        <p class="asset-browser__eyebrow">当前资源库筛选</p>
        <strong>{{ repositoryName }}</strong>
      </div>
      <div class="workspace-filter-bar__actions">
        <span v-if="activeFilterCount" class="asset-stat">{{ activeFilterCount }} 个条件</span>
        <button type="button" class="ghost workspace-filter-bar__btn" :disabled="!hasActiveFilters && !searchQuery.trim()" @click="emit('clearSearchFilters')">
          清除
        </button>
        <button type="button" class="ghost workspace-filter-bar__btn" aria-label="关闭筛选栏" @click="emit('close')">
          <X :size="14" aria-hidden="true" />
        </button>
      </div>
    </div>

    <div class="workspace-filter-bar__groups">
      <section v-if="formatFilterOptions.length" class="workspace-filter-bar__group" aria-label="格式筛选">
        <span>格式</span>
        <div class="workspace-filter-bar__options">
          <button
            v-for="format in formatFilterOptions"
            :key="format"
            type="button"
            class="workspace-filter-chip"
            :class="{ 'is-active': filters.formats.includes(format) }"
            @click="emit('toggleSearchFilter', 'formats', format)"
          >
            {{ format }}
          </button>
        </div>
      </section>

      <section v-if="tagFilterOptions.length" class="workspace-filter-bar__group" aria-label="文件标签筛选">
        <span>标签</span>
        <div class="workspace-filter-bar__options">
          <button
            v-for="tag in tagFilterOptions"
            :key="tag"
            type="button"
            class="workspace-filter-chip"
            :class="{ 'is-active': filters.tags.includes(tag) }"
            @click="emit('toggleSearchFilter', 'tags', tag)"
          >
            {{ tag }}
          </button>
        </div>
      </section>

      <section class="workspace-filter-bar__group" aria-label="文件颜色筛选">
        <span>颜色</span>
        <div class="workspace-filter-bar__options">
          <button
            v-for="color in colorFilterOptions"
            :key="color"
            type="button"
            class="workspace-filter-chip"
            :class="{ 'is-active': filters.colors.includes(color) }"
            :style="filterColorStyle(color)"
            @click="emit('toggleSearchFilter', 'colors', color)"
          >
            <i class="workspace-filter-chip__swatch" aria-hidden="true"></i>
            {{ color }}
          </button>
          <label class="workspace-filter-input">
            <input
              v-model="colorFilterInput"
              type="text"
              aria-label="输入文件颜色"
              placeholder="输入颜色"
              @keydown.enter.prevent="emit('submitMetadataFilterInput', 'colors')"
            />
            <button type="button" :disabled="!colorFilterInput.trim()" @click="emit('submitMetadataFilterInput', 'colors')">
              添加
            </button>
          </label>
        </div>
      </section>

      <section class="workspace-filter-bar__group" aria-label="形状筛选">
        <span>形状</span>
        <div class="workspace-filter-bar__options">
          <button
            v-for="shape in shapeFilterOptions"
            :key="shape"
            type="button"
            class="workspace-filter-chip"
            :class="{ 'is-active': filters.shapes.includes(shape) }"
            @click="emit('toggleSearchFilter', 'shapes', shape)"
          >
            {{ shape }}
          </button>
          <label class="workspace-filter-input">
            <input
              v-model="shapeFilterInput"
              type="text"
              aria-label="输入形状"
              placeholder="输入形状"
              @keydown.enter.prevent="emit('submitMetadataFilterInput', 'shapes')"
            />
            <button type="button" :disabled="!shapeFilterInput.trim()" @click="emit('submitMetadataFilterInput', 'shapes')">
              添加
            </button>
          </label>
        </div>
      </section>

      <section class="workspace-filter-bar__group" aria-label="评分筛选">
        <span>评分</span>
        <div class="workspace-filter-bar__options">
          <button
            type="button"
            class="workspace-filter-chip"
            :class="{ 'is-active': filters.minRating == null }"
            @click="emit('selectMinimumRating', null)"
          >
            全部
          </button>
          <button
            v-for="rating in ratingFilterOptions"
            :key="rating"
            type="button"
            class="workspace-filter-chip"
            :class="{ 'is-active': filters.minRating === rating }"
            @click="emit('selectMinimumRating', rating)"
          >
            {{ rating }} 星+
          </button>
        </div>
      </section>

      <section v-if="activeLibrarySearchShortcuts.length" class="workspace-filter-bar__group" aria-label="库类型筛选">
        <span>库类型</span>
        <div class="workspace-filter-bar__options">
          <button
            v-for="{ extension, shortcut } in activeLibrarySearchShortcuts"
            :key="`${extension.pluginId}:${shortcut.id}`"
            type="button"
            class="workspace-filter-chip"
            @click="emit('applyMetadataFilterShortcut', shortcut.metadataFilters, shortcut.sort?.field ?? '', shortcut.sort?.direction === 'desc' ? 'desc' : 'asc')"
          >
            {{ shortcut.label }}
          </button>
        </div>
      </section>

      <section class="workspace-filter-bar__group workspace-filter-bar__group--wide" aria-label="高级筛选">
        <span>高级</span>
        <div class="workspace-filter-bar__advanced">
          <label class="workspace-filter-input">
            <input v-model="excludeQueryInput" type="text" aria-label="排除关键词" placeholder="排除关键词" @keydown.enter.prevent="emit('applyAdvancedSearchFilters')" />
          </label>
          <label class="workspace-filter-input">
            <input v-model="excludePathPrefixesInput" type="text" aria-label="排除路径" placeholder="排除路径" @keydown.enter.prevent="emit('applyAdvancedSearchFilters')" />
          </label>
          <label class="workspace-filter-input">
            <input v-model="excludeTagsInput" type="text" aria-label="排除标签" placeholder="排除标签" @keydown.enter.prevent="emit('applyAdvancedSearchFilters')" />
          </label>
          <label class="workspace-filter-input">
            <input v-model="excludeFormatsInput" type="text" aria-label="排除格式" placeholder="排除格式" @keydown.enter.prevent="emit('applyAdvancedSearchFilters')" />
          </label>
          <label class="workspace-filter-input workspace-filter-input--wide">
            <input v-model="metadataFiltersInput" type="text" aria-label="元数据" placeholder="libraryKind=audio" @keydown.enter.prevent="emit('applyAdvancedSearchFilters')" />
          </label>
          <label class="workspace-filter-input workspace-filter-input--wide">
            <input v-model="excludeMetadataFiltersInput" type="text" aria-label="排除元数据" placeholder="status=archived" @keydown.enter.prevent="emit('applyAdvancedSearchFilters')" />
          </label>
          <label class="workspace-filter-input workspace-filter-input--wide">
            <input v-model="excludeNumberFiltersInput" type="text" aria-label="排除数值范围" placeholder="排除 width=0..640" @keydown.enter.prevent="emit('applyAdvancedSearchFilters')" />
          </label>
          <label class="workspace-filter-input workspace-filter-input--wide">
            <input v-model="excludeDateFiltersInput" type="text" aria-label="排除日期范围" placeholder="排除 fileCreatedAt=2024-01-01T00:00:00Z.." @keydown.enter.prevent="emit('applyAdvancedSearchFilters')" />
          </label>
          <label class="workspace-filter-input workspace-filter-input--wide">
            <input v-model="numberFiltersInput" type="text" aria-label="数值范围" placeholder="width=1024..4096" @keydown.enter.prevent="emit('applyAdvancedSearchFilters')" />
          </label>
          <label class="workspace-filter-input workspace-filter-input--wide">
            <input v-model="dateFiltersInput" type="text" aria-label="日期范围" placeholder="fileCreatedAt=2024-01-01T00:00:00Z.." @keydown.enter.prevent="emit('applyAdvancedSearchFilters')" />
          </label>
          <label class="workspace-filter-input">
            <input v-model="sortFieldInput" type="text" aria-label="排序字段" placeholder="排序字段" @keydown.enter.prevent="emit('applyAdvancedSearchFilters')" />
          </label>
          <label class="workspace-filter-input workspace-filter-input--select">
            <select v-model="sortDirectionInput" aria-label="排序方向">
              <option value="asc">升序</option>
              <option value="desc">降序</option>
            </select>
          </label>
          <label class="workspace-filter-input">
            <input v-model="limitInput" type="number" min="1" step="1" aria-label="结果数量" placeholder="数量" @keydown.enter.prevent="emit('applyAdvancedSearchFilters')" />
          </label>
          <button type="button" class="ghost workspace-filter-bar__btn" @click="emit('applyAdvancedSearchFilters')">
            应用
          </button>
        </div>
      </section>
    </div>
  </div>
</template>
