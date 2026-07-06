<script setup lang="ts">
/**
 * 系统日志中心面板，提供筛选、检索与实时追踪能力。
 */
import { computed, nextTick, ref, watch } from "vue";
import { Eraser, LoaderCircle, Pause, Play, Search, Trash2 } from "@lucide/vue";
import type {
  SystemLogLevel,
  SystemLogRecord,
  SystemLogSourceKind,
} from "../../types/repository";

const props = defineProps<{
  records: SystemLogRecord[];
  isLoading: boolean;
  isClearing: boolean;
}>();

const emit = defineEmits<{
  clear: [];
}>();

type LogFilterOption<TValue extends string> = {
  value: TValue;
  label: string;
};

const levelOptions: LogFilterOption<SystemLogLevel>[] = [
  { value: "debug", label: "调试" },
  { value: "info", label: "信息" },
  { value: "warn", label: "警告" },
  { value: "error", label: "错误" },
];

const sourceKindOptions: LogFilterOption<SystemLogSourceKind>[] = [
  { value: "host", label: "宿主" },
  { value: "frontend-host", label: "前端宿主" },
  { value: "frontend-plugin", label: "前端插件" },
  { value: "backend-plugin", label: "后端插件" },
  { value: "helper", label: "辅助进程" },
];

const selectedLevels = ref<SystemLogLevel[]>([]);
const selectedSourceKinds = ref<SystemLogSourceKind[]>([]);
const selectedPluginId = ref("");
const selectedRepoId = ref("");
const searchText = ref("");
const pausedAutoScroll = ref(false);
const logListRef = ref<HTMLElement | null>(null);

function toggleFilter<TValue extends string>(items: TValue[], value: TValue) {
  return items.includes(value)
    ? items.filter((item) => item !== value)
    : [...items, value];
}

function includesText(record: SystemLogRecord, text: string) {
  const keyword = text.trim().toLowerCase();
  if (!keyword) return true;
  const location = [
    record.location.modulePath ?? "",
    record.location.file ?? "",
    record.location.line ?? "",
  ].join(":");
  const source = [
    record.source.kind ?? "",
    record.source.label ?? "",
    record.source.pluginId ?? "",
    record.source.repoId ?? "",
  ].join(" ");
  const haystack = [
    record.level,
    record.category,
    record.action,
    record.message,
    source,
    location,
    JSON.stringify(record.context ?? {}),
  ]
    .join("\n")
    .toLowerCase();
  return haystack.includes(keyword);
}

function sourceKindLabel(kind: string) {
  return sourceKindOptions.find((item) => item.value === kind)?.label ?? kind;
}

function levelLabel(level: string) {
  return levelOptions.find((item) => item.value === level)?.label ?? level.toUpperCase();
}

function timestampLabel(timestamp: string) {
  const date = new Date(timestamp);
  if (Number.isNaN(date.getTime())) return timestamp;
  return new Intl.DateTimeFormat("zh-CN", {
    hour12: false,
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  }).format(date);
}

function locationLabel(record: SystemLogRecord) {
  const parts = [
    record.location.modulePath,
    record.location.file,
    record.location.line != null ? String(record.location.line) : null,
  ].filter(Boolean);
  return parts.join(":");
}

function contextLabel(record: SystemLogRecord) {
  return JSON.stringify(record.context ?? {}, null, 2);
}

const pluginOptions = computed(() => (
  [...new Set(props.records.map((record) => record.source.pluginId).filter(Boolean) as string[])]
    .sort((left, right) => left.localeCompare(right))
));

const repoOptions = computed(() => (
  [...new Set(props.records.map((record) => record.source.repoId).filter(Boolean) as string[])]
    .sort((left, right) => left.localeCompare(right))
));

const filteredRecords = computed(() => (
  props.records
    .filter((record) => !selectedLevels.value.length || selectedLevels.value.includes(record.level as SystemLogLevel))
    .filter((record) => !selectedSourceKinds.value.length || selectedSourceKinds.value.includes(record.source.kind as SystemLogSourceKind))
    .filter((record) => !selectedPluginId.value || record.source.pluginId === selectedPluginId.value)
    .filter((record) => !selectedRepoId.value || record.source.repoId === selectedRepoId.value)
    .filter((record) => includesText(record, searchText.value))
    .slice()
    .sort((left, right) => (
      left.timestamp.localeCompare(right.timestamp)
      || left.id.localeCompare(right.id)
    ))
));

const activeFilterCount = computed(() => (
  selectedLevels.value.length
  + selectedSourceKinds.value.length
  + (selectedPluginId.value ? 1 : 0)
  + (selectedRepoId.value ? 1 : 0)
  + (searchText.value.trim() ? 1 : 0)
));

const hasRecords = computed(() => props.records.length > 0);

function resetFilters() {
  selectedLevels.value = [];
  selectedSourceKinds.value = [];
  selectedPluginId.value = "";
  selectedRepoId.value = "";
  searchText.value = "";
}

async function syncAutoScroll() {
  if (pausedAutoScroll.value) return;
  await nextTick();
  const element = logListRef.value;
  if (!element) return;
  if (typeof element.scrollTo === "function") {
    element.scrollTo({
      top: element.scrollHeight,
      behavior: "smooth",
    });
    return;
  }
  element.scrollTop = element.scrollHeight;
}

watch(
  () => filteredRecords.value.map((record) => record.id).join("|"),
  async (nextValue, previousValue) => {
    if (!nextValue || nextValue === previousValue) return;
    await syncAutoScroll();
  },
  { flush: "post" },
);
</script>

<template>
  <section class="logs-workbench">
    <div class="search-workbench__panel logs-workbench__panel">
      <header class="search-workbench__header logs-workbench__header">
        <div>
          <p class="asset-browser__eyebrow">Logs</p>
          <h1>系统日志</h1>
          <p class="search-workbench__subline">
            统一查看宿主、插件与辅助进程的实时日志流。
          </p>
        </div>
        <div class="search-workbench__stats">
          <span class="asset-stat">{{ records.length }} 条缓存</span>
          <span class="asset-stat">{{ filteredRecords.length }} 条命中</span>
        </div>
      </header>

      <div class="logs-workbench__toolbar">
        <label class="search-workbench__field logs-workbench__search">
          <Search :size="15" aria-hidden="true" />
          <input
            v-model="searchText"
            type="search"
            aria-label="搜索日志"
            placeholder="搜索消息、动作、位置或上下文"
          >
        </label>

        <label class="search-workbench__field logs-workbench__select">
          <span>插件</span>
          <select v-model="selectedPluginId" aria-label="筛选插件">
            <option value="">全部插件</option>
            <option v-for="pluginId in pluginOptions" :key="pluginId" :value="pluginId">
              {{ pluginId }}
            </option>
          </select>
        </label>

        <label class="search-workbench__field logs-workbench__select">
          <span>仓库</span>
          <select v-model="selectedRepoId" aria-label="筛选仓库">
            <option value="">全部仓库</option>
            <option v-for="repoId in repoOptions" :key="repoId" :value="repoId">
              {{ repoId }}
            </option>
          </select>
        </label>

        <button
          type="button"
          class="ui-button"
          :aria-pressed="pausedAutoScroll"
          :title="pausedAutoScroll ? '恢复自动滚动' : '暂停自动滚动'"
          @click="pausedAutoScroll = !pausedAutoScroll"
        >
          <Play v-if="pausedAutoScroll" :size="14" aria-hidden="true" />
          <Pause v-else :size="14" aria-hidden="true" />
          {{ pausedAutoScroll ? "恢复追踪" : "暂停追踪" }}
        </button>

        <button
          type="button"
          class="ui-button"
          :disabled="!activeFilterCount"
          @click="resetFilters"
        >
          <Eraser :size="14" aria-hidden="true" />
          重置筛选
        </button>

        <button
          type="button"
          class="ui-button"
          :disabled="isClearing || (!records.length && !isLoading)"
          @click="emit('clear')"
        >
          <LoaderCircle v-if="isClearing" class="spin" :size="14" aria-hidden="true" />
          <Trash2 v-else :size="14" aria-hidden="true" />
          清空日志
        </button>
      </div>

      <section class="logs-workbench__filters" aria-label="日志筛选">
        <div class="logs-workbench__filter-group">
          <span>级别</span>
          <div class="logs-workbench__chips">
            <button
              v-for="option in levelOptions"
              :key="option.value"
              type="button"
              class="workspace-filter-chip"
              :class="{ 'is-active': selectedLevels.includes(option.value) }"
              :aria-pressed="selectedLevels.includes(option.value)"
              @click="selectedLevels = toggleFilter(selectedLevels, option.value)"
            >
              {{ option.label }}
            </button>
          </div>
        </div>

        <div class="logs-workbench__filter-group">
          <span>来源</span>
          <div class="logs-workbench__chips">
            <button
              v-for="option in sourceKindOptions"
              :key="option.value"
              type="button"
              class="workspace-filter-chip"
              :class="{ 'is-active': selectedSourceKinds.includes(option.value) }"
              :aria-pressed="selectedSourceKinds.includes(option.value)"
              @click="selectedSourceKinds = toggleFilter(selectedSourceKinds, option.value)"
            >
              {{ option.label }}
            </button>
          </div>
        </div>
      </section>

      <div v-if="isLoading && !hasRecords" class="asset-browser__state">
        <LoaderCircle class="spin" :size="16" aria-hidden="true" />
        正在加载系统日志
      </div>

      <div v-else-if="!hasRecords" class="search-workbench__empty">
        <h2>还没有系统日志</h2>
        <p>宿主、插件和辅助进程产生的关键操作会在这里持续汇总。</p>
      </div>

      <div v-else-if="!filteredRecords.length" class="search-workbench__empty">
        <h2>当前筛选没有命中</h2>
        <p>保留最近日志缓存，调整级别、来源或关键字后可以继续查看。</p>
      </div>

      <div
        v-else
        ref="logListRef"
        class="logs-workbench__list"
      >
        <article
          v-for="record in filteredRecords"
          :key="record.id"
          class="logs-workbench__item"
        >
          <header class="logs-workbench__item-head">
            <div class="logs-workbench__badges">
              <span class="logs-workbench__level" :class="`is-${record.level}`">
                {{ levelLabel(record.level) }}
              </span>
              <span class="workspace-hints__chip">{{ sourceKindLabel(record.source.kind) }}</span>
              <span v-if="record.source.label" class="workspace-hints__chip">{{ record.source.label }}</span>
            </div>
            <time :datetime="record.timestamp">{{ timestampLabel(record.timestamp) }}</time>
          </header>

          <div class="logs-workbench__body">
            <div class="logs-workbench__message">
              <strong>{{ record.action }}</strong>
              <p>{{ record.message }}</p>
            </div>
            <div class="logs-workbench__meta">
              <span>{{ record.category }}</span>
              <span v-if="record.source.pluginId">插件 {{ record.source.pluginId }}</span>
              <span v-if="record.source.repoId">仓库 {{ record.source.repoId }}</span>
              <span v-if="locationLabel(record)">位置 {{ locationLabel(record) }}</span>
            </div>
          </div>

          <details v-if="Object.keys(record.context ?? {}).length" class="logs-workbench__context">
            <summary>上下文</summary>
            <pre>{{ contextLabel(record) }}</pre>
          </details>
        </article>
      </div>
    </div>
  </section>
</template>
