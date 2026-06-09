<script setup lang="ts">
import { computed, onMounted } from "vue";
import { Database, Moon, ServerCog, Sun } from "lucide-vue-next";
import PluginManagerPanel from "../components/PluginManagerPanel.vue";
import { useTheme } from "../composables/useTheme";
import { useRepositoryWorkspace } from "../composables/useRepositoryWorkspace";

const { theme, setTheme } = useTheme();
const {
  repositories,
  cacheSnapshot,
  apiDesign,
  loadSettingsData,
} = useRepositoryWorkspace();

const repositoryCount = computed(() => repositories.value.length);
const repositoryBackends = computed(() => {
  const backendMap = new Map<string, { name: string; kind: string; count: number }>();
  for (const repository of repositories.value) {
    const current = backendMap.get(repository.backend.pluginId);
    if (current) {
      current.count += 1;
      continue;
    }
    backendMap.set(repository.backend.pluginId, {
      name: repository.backend.name,
      kind: repository.backend.kind,
      count: 1,
    });
  }
  return Array.from(backendMap.values());
});

onMounted(() => {
  void loadSettingsData();
});
</script>

<template>
  <section>
    <div class="page-header">
      <div>
        <h1>设置</h1>
        <p>管理仓库服务、插件、缓存与 API 契约。</p>
      </div>
    </div>

    <div class="card">
      <h2>外观</h2>
      <div class="settings-row">
        <div class="settings-row__label">
          <div>主题</div>
          <div class="settings-row__hint">选择应用配色，立即生效并记忆到本地。</div>
        </div>
        <div class="segmented" role="radiogroup" aria-label="主题">
          <button
            type="button"
            role="radio"
            :aria-checked="theme === 'dark'"
            :class="{ 'is-active': theme === 'dark' }"
            @click="setTheme('dark')"
          >
            <Moon :size="14" aria-hidden="true" />
            暗色
          </button>
          <button
            type="button"
            role="radio"
            :aria-checked="theme === 'light'"
            :class="{ 'is-active': theme === 'light' }"
            @click="setTheme('light')"
          >
            <Sun :size="14" aria-hidden="true" />
            浅色
          </button>
        </div>
      </div>
    </div>

    <div class="card">
      <h2>仓库服务</h2>
      <ul class="kv">
        <li><span>已注册仓库</span><span>{{ repositoryCount }}</span></li>
        <li><span>并发模型</span><span>SQLite WAL + 乐观锁</span></li>
        <li><span>同步方式</span><span>全量扫描 + 事件表</span></li>
        <li>
          <span>已接入后端</span>
          <span>{{ repositoryBackends.map((item) => `${item.name} (${item.count})`).join(" / ") || "无" }}</span>
        </li>
      </ul>
    </div>

    <PluginManagerPanel
      title="插件管理"
      eyebrow="系统扩展"
      subline="在这里启用、禁用、删除用户插件，或从压缩包导入新的插件。"
      search-placeholder="筛选插件、能力或运行时"
      empty-title="没有匹配的插件"
      empty-description="试试其他关键词，或从压缩包导入新的插件。"
    />

    <div class="card">
      <h2>缓存</h2>
      <div class="settings-grid">
        <div class="settings-metric">
          <Database :size="16" aria-hidden="true" />
          <div>
            <strong>{{ cacheSnapshot?.config.metadataCapacity ?? 0 }}</strong>
            <span>Metadata LRU</span>
          </div>
        </div>
        <div class="settings-metric">
          <Database :size="16" aria-hidden="true" />
          <div>
            <strong>{{ cacheSnapshot?.config.thumbnailCapacity ?? 0 }}</strong>
            <span>Thumbnail LRU</span>
          </div>
        </div>
        <div class="settings-metric">
          <Database :size="16" aria-hidden="true" />
          <div>
            <strong>{{ cacheSnapshot?.config.queryCapacity ?? 0 }}</strong>
            <span>Query LRU</span>
          </div>
        </div>
      </div>
      <ul class="kv">
        <li v-for="entry in cacheSnapshot?.entries ?? []" :key="`${entry.cacheType}:${entry.key}`">
          <span>{{ entry.cacheType }} / {{ entry.key }}</span>
          <span>{{ entry.lastAccessedAt }}</span>
        </li>
      </ul>
    </div>

    <div class="card">
      <h2>API 设计</h2>
      <p class="muted">
        <ServerCog :size="13" aria-hidden="true" />
        {{ apiDesign?.transport ?? "本地服务契约未加载" }}
      </p>
      <ul class="kv">
        <li v-for="endpoint in apiDesign?.endpoints ?? []" :key="`${endpoint.method}:${endpoint.path}`">
          <span>{{ endpoint.group }} / {{ endpoint.method }} {{ endpoint.path }}</span>
          <span>{{ endpoint.summary }}</span>
        </li>
      </ul>
    </div>
  </section>
</template>
