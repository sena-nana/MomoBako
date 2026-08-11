<script setup lang="ts">
import { AlertTriangle } from "@lucide/vue";
import { useRouter } from "vue-router";
import type { RepositorySummary } from "../../types/repository";

const props = defineProps<{
  activeRepository: RepositorySummary | null;
  error: string | null;
  isBusy: boolean;
  isDeleting: boolean;
  isRepairing: boolean;
}>();
const router = useRouter();

const emit = defineEmits<{
  choosePath: [];
  deleteRepository: [];
  refresh: [];
}>();

function isSourceCacheIssue(repository: RepositorySummary | null) {
  return repository?.localCache?.required === true && repository.localCache.status !== "ready";
}

function openSourceSettings() {
  const pluginId = props.activeRepository?.backend.pluginId;
  if (!pluginId) return;
  void router.push({ path: "/settings", query: { plugin: pluginId } });
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
          isSourceCacheIssue(activeRepository)
            ? "这个来源资源库需要在插件设置中配置本地缓存或重新认证。仓库记录和已有缓存不会被删除。"
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
        <button
          type="button"
          class="primary"
          :disabled="isBusy"
          @click="isSourceCacheIssue(activeRepository) ? openSourceSettings() : emit('choosePath')"
        >
          {{ isSourceCacheIssue(activeRepository) ? "打开来源设置" : (isRepairing ? "重定向中..." : "重定向") }}
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
