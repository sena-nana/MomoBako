<script setup lang="ts">
import { nextTick, ref, watch } from "vue";
import {
  isContextMenuItemPending,
  selectContextMenuItem,
  useContextMenu,
  type ContextMenuItem,
} from "../composables/useContextMenu";

const { state } = useContextMenu();

const menuEl = ref<HTMLElement | null>(null);
const pos = ref({ x: 0, y: 0 });

function displayLabel(item: ContextMenuItem) {
  return isContextMenuItemPending(item) ? item.confirmLabel : item.label;
}

function isDanger(item: ContextMenuItem) {
  return item.danger || isContextMenuItemPending(item);
}

watch(
  () => state.open,
  async (open) => {
    if (!open) return;
    pos.value = { x: state.x, y: state.y };
    await nextTick();
    const element = menuEl.value;
    if (!element) return;
    const x = Math.max(4, Math.min(state.x, window.innerWidth - element.offsetWidth - 4));
    const y = Math.max(4, Math.min(state.y, window.innerHeight - element.offsetHeight - 4));
    pos.value = { x, y };
  },
);
</script>

<template>
  <Teleport to="body">
    <div
      v-if="state.open"
      ref="menuEl"
      class="ctx-menu"
      role="menu"
      :style="{ left: `${pos.x}px`, top: `${pos.y}px` }"
    >
      <button
        v-for="(item, index) in state.items"
        :key="item.id ?? index"
        type="button"
        class="ctx-menu__item"
        :class="{
          'ctx-menu__item--danger': isDanger(item),
          'ctx-menu__item--pending': isContextMenuItemPending(item),
        }"
        :disabled="item.disabled"
        role="menuitem"
        @click="selectContextMenuItem(item)"
      >
        <component v-if="item.icon" :is="item.icon" :size="13" aria-hidden="true" />
        <span class="ctx-menu__label">{{ displayLabel(item) }}</span>
      </button>
    </div>
  </Teleport>
</template>
