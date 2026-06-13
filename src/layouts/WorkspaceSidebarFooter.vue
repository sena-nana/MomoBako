<script setup lang="ts">
import { RouterLink } from "vue-router";
import { Puzzle, Settings } from "lucide-vue-next";
import TaskPopover from "../components/TaskPopover.vue";
import type { WorkspacePanelKey } from "../composables/useRepositoryWorkspace";

defineProps<{
  activePanel: WorkspacePanelKey;
  isSettingsRoute: boolean;
}>();

const emit = defineEmits<{
  selectExtensions: [];
}>();
</script>

<template>
  <footer class="workspace-sidebar__footer" aria-label="辅助入口">
    <RouterLink
      to="/settings"
      class="workspace-footer__btn"
      active-class="is-active"
      title="设置"
      aria-label="设置"
    >
      <Settings :size="14" aria-hidden="true" />
    </RouterLink>
    <button
      type="button"
      class="workspace-footer__btn"
      :class="{ 'is-active': activePanel === 'extensions' && !isSettingsRoute }"
      title="拓展"
      aria-label="拓展"
      @click="emit('selectExtensions')"
    >
      <Puzzle :size="14" aria-hidden="true" />
    </button>
    <TaskPopover />
  </footer>
</template>
