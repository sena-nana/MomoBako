<script setup lang="ts">
import { ChevronsUpDown } from "lucide-vue-next";
import type { RepositorySummary } from "../types/repository";

defineProps<{
  activeRepository: RepositorySummary | null;
  isSwitcherOpen: boolean;
}>();

const repositorySwitcherButtonRef = defineModel<HTMLElement | null>("repositorySwitcherButtonRef", { required: true });
const emit = defineEmits<{
  openSwitcher: [event: MouseEvent];
}>();
</script>

<template>
  <section class="workspace-sidebar__top" aria-label="资源库与视图">
    <div class="workspace-sidebar__repo-head">
      <button
        :ref="(element) => { repositorySwitcherButtonRef = element as HTMLElement | null; }"
        type="button"
        class="workspace-sidebar__repo-current"
        :title="activeRepository?.path ?? '添加资源库'"
        aria-haspopup="menu"
        :aria-expanded="isSwitcherOpen"
        aria-label="资源库"
        @click="emit('openSwitcher', $event)"
      >
        <span>{{ activeRepository?.name ?? "无资源库" }}</span>
        <ChevronsUpDown :size="13" aria-hidden="true" />
      </button>
    </div>
  </section>
</template>
