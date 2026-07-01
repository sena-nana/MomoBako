<script setup lang="ts">
import { ClipboardList } from "@lucide/vue";
import type { Component } from "vue";
import type { RepositoryShortcut } from "../types/repository";
import type { ShortcutItem, ShortcutKey } from "./useSidebarShortcutsUi";
import type { WorkspaceLibraryCategoryKey, WorkspacePanelKey } from "../composables/useRepositoryWorkspace";

defineProps<{
  activePanel: WorkspacePanelKey;
  activeLibraryCategory: WorkspaceLibraryCategoryKey;
  isActiveRepositoryMissing: boolean;
  quickAccess: RepositoryShortcut[];
  repositoryActionsCount: number;
  shortcutIcon: (shortcut: RepositoryShortcut) => Component;
  shortcuts: ShortcutItem[];
}>();

const emit = defineEmits<{
  openQuickAccess: [shortcut: RepositoryShortcut];
  selectActions: [];
  selectShortcut: [id: ShortcutKey];
}>();
</script>

<template>
  <section class="workspace-group">
    <div class="workspace-shortcuts">
      <button
        v-for="item in shortcuts"
        :key="item.id"
        type="button"
        class="workspace-shortcuts__item"
        :class="{ 'is-active': item.id === 'trash' ? activePanel === 'trash' : activePanel === 'files' && activeLibraryCategory === item.id }"
        :disabled="isActiveRepositoryMissing"
        @click="emit('selectShortcut', item.id)"
      >
        <span class="workspace-shortcuts__label">
          <component :is="item.icon" :size="15" aria-hidden="true" />
          {{ item.label }}
        </span>
        <span class="workspace-shortcuts__count">{{ item.count }}</span>
      </button>
    </div>
  </section>

  <section v-if="quickAccess.length" class="workspace-group">
    <div class="workspace-group__header">
      <span>快捷访问</span>
    </div>
    <div class="workspace-shortcuts">
      <button
        v-for="item in quickAccess"
        :key="item.shortcutId"
        type="button"
        class="workspace-shortcuts__item"
        :disabled="isActiveRepositoryMissing"
        :title="item.targetPath ?? item.targetId ?? item.label"
        @click="emit('openQuickAccess', item)"
      >
        <span class="workspace-shortcuts__label">
          <component :is="shortcutIcon(item)" :size="15" aria-hidden="true" />
          {{ item.label }}
        </span>
      </button>
    </div>
  </section>

  <section v-if="repositoryActionsCount" class="workspace-group">
    <div class="workspace-group__header">
      <span>动作</span>
    </div>
    <div class="workspace-shortcuts">
      <button
        type="button"
        class="workspace-shortcuts__item"
        :class="{ 'is-active': activePanel === 'actions' }"
        :disabled="isActiveRepositoryMissing"
        @click="emit('selectActions')"
      >
        <span class="workspace-shortcuts__label">
          <ClipboardList :size="15" aria-hidden="true" />
          动作
        </span>
        <span class="workspace-shortcuts__count">{{ repositoryActionsCount }}</span>
      </button>
    </div>
  </section>
</template>
