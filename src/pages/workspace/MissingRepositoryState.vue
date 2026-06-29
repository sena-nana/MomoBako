<script setup lang="ts">
import { AlertTriangle } from "@lucide/vue";
import type { RepositorySummary } from "../../types/repository";

defineProps<{
  activeRepository: RepositorySummary | null;
  error: string | null;
  isBusy: boolean;
  isDeleting: boolean;
  isRepairing: boolean;
}>();

const emit = defineEmits<{
  choosePath: [];
  deleteRepository: [];
  refresh: [];
}>();

function isNeteaseCacheIssue(repository: RepositorySummary | null) {
  return repository?.backend.pluginId === "momobako.source.netease-cloud-music"
    && repository.localCache?.status !== "ready";
}
</script>

<template>
  <section class="missing-repository-page" aria-live="polite">
    <div class="missing-repository-page__panel">
      <div class="missing-repository-page__icon" aria-hidden="true">
        <AlertTriangle :size="22" />
      </div>
      <p class="asset-browser__eyebrow">资源库丢失</p>
      <h1>{{ activeRepository?.name ?? "资源库不可用" }}</h1>
      <p class="missing-repository-page__summary">
        {{
          isNeteaseCacheIssue(activeRepository)
            ? "这个网易云资源库需要指定本地缓存目录。缓存目录会保存索引、缩略图、播放缓存和下载暂存。"
            : "MomoBako 找不到这个资源库的本地文件夹。可以重定向到原资源库位置，或移除这条注册记录和本机缓存。"
        }}
      </p>
      <p class="missing-repository-page__path">
        {{ activeRepository?.path }}
      </p>
      <p v-if="error" class="missing-repository-page__error">
        {{ error }}
      </p>
      <div class="missing-repository-page__actions">
        <button type="button" class="primary" :disabled="isBusy" @click="emit('choosePath')">
          {{ isRepairing ? (isNeteaseCacheIssue(activeRepository) ? "配置中..." : "重定向中...") : (isNeteaseCacheIssue(activeRepository) ? "指定缓存目录" : "重定向") }}
        </button>
        <button type="button" class="ghost" :disabled="isBusy" @click="emit('refresh')">
          刷新
        </button>
        <button type="button" class="ghost danger" :disabled="isBusy" @click="emit('deleteRepository')">
          {{ isDeleting ? "删除中..." : "删除资源库" }}
        </button>
      </div>
    </div>
  </section>
</template>
