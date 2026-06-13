<script setup lang="ts">
import { nextTick, ref, watch } from "vue";
import { Check } from "lucide-vue-next";
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

function hasChildren(item: ContextMenuItem) {
  return Boolean(item.children?.length);
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
      <div
        v-for="(item, index) in state.items"
        :key="item.id ?? index"
        class="ctx-menu__slot"
        :class="{ 'ctx-menu__slot--nested': hasChildren(item) }"
      >
        <button
          type="button"
          class="ctx-menu__item"
          :class="{
            'ctx-menu__item--danger': isDanger(item),
            'ctx-menu__item--pending': isContextMenuItemPending(item),
          }"
          :disabled="item.disabled"
          role="menuitem"
          :aria-haspopup="hasChildren(item) ? 'menu' : undefined"
          @click="selectContextMenuItem(item)"
        >
          <Check v-if="item.checked" :size="13" aria-hidden="true" />
          <component v-if="item.icon" :is="item.icon" :size="13" aria-hidden="true" />
          <span class="ctx-menu__label">{{ displayLabel(item) }}</span>
          <span v-if="hasChildren(item)" class="ctx-menu__chevron" aria-hidden="true">›</span>
        </button>
        <div v-if="item.children?.length" class="ctx-menu ctx-menu__submenu" role="menu">
          <button
            v-for="(child, childIndex) in item.children"
            :key="child.id ?? childIndex"
            type="button"
            class="ctx-menu__item"
            :class="{
              'ctx-menu__item--danger': isDanger(child),
              'ctx-menu__item--pending': isContextMenuItemPending(child),
            }"
            :disabled="child.disabled"
            role="menuitem"
            @click="selectContextMenuItem(child)"
          >
            <Check v-if="child.checked" :size="13" aria-hidden="true" />
            <component v-if="child.icon" :is="child.icon" :size="13" aria-hidden="true" />
            <span class="ctx-menu__label">{{ displayLabel(child) }}</span>
          </button>
        </div>
      </div>
    </div>
  </Teleport>
</template>
