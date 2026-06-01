<script setup lang="ts">
import { CheckSquare, LayoutDashboard, MessagesSquare } from "lucide-vue-next";

export type ViewKey = "overview" | "board" | "todo";

interface Props {
  active: ViewKey;
}

defineProps<Props>();

const tabs: Array<{ key: ViewKey; label: string; icon: unknown; disabled: boolean }> = [
  { key: "overview", label: "概览", icon: MessagesSquare, disabled: false },
  { key: "board", label: "看板", icon: LayoutDashboard, disabled: true },
  { key: "todo", label: "Todo", icon: CheckSquare, disabled: true },
];
</script>

<template>
  <div class="view-tabs" role="tablist" aria-label="视图">
    <button
      v-for="tab in tabs"
      :key="tab.key"
      type="button"
      class="view-tabs__tab"
      :class="{ 'is-active': active === tab.key }"
      :disabled="tab.disabled"
      :aria-selected="active === tab.key"
      :title="tab.disabled ? '即将上线' : tab.label"
      role="tab"
    >
      <component :is="tab.icon" :size="14" aria-hidden="true" />
      <span>{{ tab.label }}</span>
    </button>
  </div>
</template>
