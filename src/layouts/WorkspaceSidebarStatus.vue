<script setup lang="ts">
import { LoaderCircle } from "lucide-vue-next";
import type { RepositorySyncProgress } from "../types/repository";

defineProps<{
  error: string | null;
  isBusy: boolean;
  isShowingSyncProgress: boolean;
  syncProgress: RepositorySyncProgress;
}>();
</script>

<template>
  <div v-if="error" class="workspace-state workspace-state--error">
    {{ error }}
  </div>

  <div v-else-if="isBusy" class="workspace-state">
    <LoaderCircle class="spin" :size="16" aria-hidden="true" />
    正在同步仓库状态
  </div>

  <div v-else-if="isShowingSyncProgress" class="workspace-state workspace-state--progress">
    <LoaderCircle class="spin" :size="16" aria-hidden="true" />
    <span>{{ syncProgress.label }}</span>
    <span class="workspace-state__percent">{{ syncProgress.percent }}%</span>
  </div>
</template>
