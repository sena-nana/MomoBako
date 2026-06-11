<script setup lang="ts">
import { computed } from "vue";
import { LoaderCircle, Play, ShieldAlert } from "lucide-vue-next";
import type { RepositoryAction } from "../../types/repository";

const props = defineProps<{
  actions: RepositoryAction[];
  activeActionId: string | null;
  selectedCount: number;
  isLoading: boolean;
  isRunning: boolean;
}>();

const emit = defineEmits<{
  select: [actionId: string];
  run: [actionId: string];
}>();

const activeAction = computed(() => (
  props.actions.find((action) => action.actionId === props.activeActionId) ?? props.actions[0] ?? null
));

function statusLabel(action: RepositoryAction) {
  if (action.status !== "ready") return "不支持";
  if (!action.enabled) return "已停用";
  return "可执行";
}

function canRun(action: RepositoryAction) {
  return action.status === "ready" && action.enabled && props.selectedCount > 0 && !props.isRunning;
}
</script>

<template>
  <section class="repository-actions-panel">
    <header class="repository-actions-panel__header">
      <div>
        <p class="asset-browser__eyebrow">动作</p>
        <h2>仓库动作</h2>
      </div>
      <span class="workspace-hints__chip">{{ actions.length }} 项</span>
    </header>

    <div v-if="isLoading" class="asset-browser__state">
      <LoaderCircle class="spin" :size="16" aria-hidden="true" />
      正在加载动作
    </div>
    <div v-else-if="!actions.length" class="asset-browser__state">
      当前仓库没有导入动作。
    </div>
    <div v-else class="repository-actions-panel__body">
      <div class="repository-actions-panel__list" role="listbox" aria-label="仓库动作">
        <button
          v-for="action in actions"
          :key="action.actionId"
          type="button"
          class="repository-actions-panel__item"
          :class="{ 'is-active': activeAction?.actionId === action.actionId }"
          @click="emit('select', action.actionId)"
        >
          <span>
            <strong>{{ action.name }}</strong>
            <small>{{ action.source }} · {{ action.steps.length }} 步 · {{ statusLabel(action) }}</small>
          </span>
          <ShieldAlert v-if="action.status !== 'ready'" :size="14" aria-hidden="true" />
        </button>
      </div>

      <article v-if="activeAction" class="repository-actions-panel__detail">
        <header class="repository-actions-panel__detail-head">
          <div>
            <h3>{{ activeAction.name }}</h3>
            <p>{{ statusLabel(activeAction) }} · 最近运行 {{ activeAction.lastRun?.status ?? "无" }}</p>
          </div>
          <button
            type="button"
            class="ui-button ui-button--primary"
            :disabled="!canRun(activeAction)"
            @click="emit('run', activeAction.actionId)"
          >
            <LoaderCircle v-if="isRunning" class="spin" :size="14" aria-hidden="true" />
            <Play v-else :size="14" aria-hidden="true" />
            执行
          </button>
        </header>
        <p v-if="activeAction.unsupportedReason" class="asset-browser__state asset-browser__state--error">
          {{ activeAction.unsupportedReason }}
        </p>
        <ol class="repository-actions-panel__steps">
          <li v-for="step in activeAction.steps" :key="step.stepId">
            <span>
              <strong>{{ step.label }}</strong>
              <small>{{ step.stepKind }} · {{ step.status }}</small>
            </span>
            <em v-if="step.unsupportedReason">{{ step.unsupportedReason }}</em>
          </li>
        </ol>
      </article>
    </div>
  </section>
</template>
