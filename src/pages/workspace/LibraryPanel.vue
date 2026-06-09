<script setup lang="ts">
import { Download, Files, Folder, HardDrive, LoaderCircle, Plus, RefreshCw, Trash2 } from "lucide-vue-next";
import Markdown from "vue3-markdown-it";
import type { RepositorySnapshot, RepositorySummary } from "../../types/repository";

defineProps<{
  activeRepoId: string | null;
  snapshot: RepositorySnapshot | null;
  repositories: RepositorySummary[];
  error: string | null;
  isBusy: boolean;
  statusLabel: (status: string) => string;
}>();

const emit = defineEmits<{
  addRepository: [event: MouseEvent];
  exportRepository: [repository: RepositorySummary];
  refresh: [];
  removeRepository: [repoId: string];
  selectRepository: [repoId: string];
}>();
</script>

<template>
  <section class="library-overview">
    <div class="library-overview__panel">
      <header class="library-overview__header">
        <div>
          <p class="asset-browser__eyebrow">当前资源库</p>
          <h1>{{ snapshot?.repository.name ?? "正在加载" }}</h1>
          <p class="library-overview__subline">
            {{ snapshot?.repository.path }}
          </p>
        </div>
        <div class="library-overview__actions">
          <button type="button" class="ghost" @click="emit('refresh')">
            <RefreshCw :size="14" aria-hidden="true" />
            刷新资源库
          </button>
          <button type="button" class="primary" @click="emit('addRepository', $event)">
            <Plus :size="14" aria-hidden="true" />
            添加资源库
          </button>
        </div>
      </header>

      <div v-if="error" class="asset-browser__state asset-browser__state--error">
        {{ error }}
      </div>

      <div v-else-if="isBusy" class="asset-browser__state">
        <LoaderCircle class="spin" :size="16" aria-hidden="true" />
        正在加载资源库
      </div>

      <template v-else>
        <div class="library-overview__stats">
          <article class="library-overview__stat-card">
            <span class="library-overview__stat-label">仓库名称</span>
            <strong>{{ snapshot?.repository.name }}</strong>
          </article>
          <article class="library-overview__stat-card">
            <span class="library-overview__stat-label">总大小</span>
            <strong>{{ snapshot?.overview.totalSizeLabel ?? "0 B" }}</strong>
            <span class="library-overview__stat-meta">
              <HardDrive :size="13" aria-hidden="true" />
              {{ snapshot?.overview.totalSizeBytes ?? 0 }} Bytes
            </span>
          </article>
          <article class="library-overview__stat-card">
            <span class="library-overview__stat-label">文件个数</span>
            <strong>{{ snapshot?.overview.fileCount ?? 0 }}</strong>
            <span class="library-overview__stat-meta">
              <Files :size="13" aria-hidden="true" />
              已索引文件
            </span>
          </article>
          <article class="library-overview__stat-card">
            <span class="library-overview__stat-label">文件夹个数</span>
            <strong>{{ snapshot?.overview.folderCount ?? 0 }}</strong>
            <span class="library-overview__stat-meta">
              <Folder :size="13" aria-hidden="true" />
              不含内部元数据目录
            </span>
          </article>
        </div>

        <section class="library-overview__readme">
          <div class="library-overview__section-head">
            <div>
              <p class="asset-browser__eyebrow">README</p>
              <h2>根目录说明</h2>
            </div>
          </div>

          <div v-if="snapshot?.overview.readmeContent" class="library-overview__readme-card">
            <Markdown :source="snapshot.overview.readmeContent" />
          </div>
          <div v-else class="library-overview__empty">
            <h2>未发现 `readme.md`</h2>
            <p>如果资源库根目录存在 `readme.md` 或 `README.md`，这里会直接展示其内容。</p>
          </div>
        </section>

        <section class="library-manager">
          <div class="library-overview__section-head">
            <div>
              <p class="asset-browser__eyebrow">Repositories</p>
              <h2>资源库管理</h2>
            </div>
          </div>

          <div class="library-manager__list">
            <article
              v-for="library in repositories"
              :key="library.repoId"
              class="library-manager__item"
              :class="{ 'is-active': activeRepoId === library.repoId }"
            >
              <button
                type="button"
                class="library-manager__summary"
                @click="emit('selectRepository', library.repoId)"
              >
                <div class="library-manager__title">
                  <strong>{{ library.name }}</strong>
                  <span>{{ library.assetCount }} 个资源</span>
                </div>
                <span class="library-manager__meta">{{ statusLabel(library.status) }}</span>
                <span class="library-manager__path">{{ library.path }}</span>
              </button>

              <div class="library-manager__actions">
                <button
                  type="button"
                  class="ghost"
                  @click="emit('exportRepository', library)"
                >
                  <Download :size="14" aria-hidden="true" />
                  导出
                </button>
                <button
                  type="button"
                  class="ghost danger"
                  @click="emit('removeRepository', library.repoId)"
                >
                  <Trash2 :size="14" aria-hidden="true" />
                  删除
                </button>
              </div>
            </article>
          </div>
        </section>
      </template>
    </div>
  </section>
</template>
