<script setup lang="ts">
import { Check } from "@lucide/vue";
import { ref, watch, type ComponentPublicInstance } from "vue";
import {
  finalizeClosedContextMenu,
  isContextMenuItemPending,
  selectContextMenuItem,
  useContextMenu,
  type ContextMenuItem,
} from "../composables/useContextMenu";
import {
  SB_LAYER_Z_INDEX,
  SB_MENU_POP_TRANSITION_MS,
  createAnchoredMenuPosition,
} from "../composables/menuMotion";
import { useAnchoredMenuSurface } from "../composables/useAnchoredMenuSurface";

const { state } = useContextMenu();
const rendered = ref(false);
const menuSurface = useAnchoredMenuSurface(createAnchoredMenuPosition(0, 0));
const menuPosition = menuSurface.position;
const menuOrigin = menuSurface.origin;

function setMenuElement(element: Element | ComponentPublicInstance | null) {
  menuSurface.surfaceEl.value = element instanceof HTMLElement ? element : null;
}

function displayLabel(item: ContextMenuItem) {
  return isContextMenuItemPending(item) ? item.confirmLabel : item.label;
}

function isDanger(item: ContextMenuItem) {
  return item.danger || isContextMenuItemPending(item);
}

function hasChildren(item: ContextMenuItem) {
  return Boolean(item.children?.length);
}

async function updateGeometry() {
  await menuSurface.syncPosition(
    createAnchoredMenuPosition(state.x, state.y, state.anchorX, state.anchorY),
  );
}

function onAfterLeave() {
  finalizeClosedContextMenu();
}

watch(
  () => [state.openSeq, state.open] as const,
  ([, open]) => {
    if (!open) {
      rendered.value = false;
      return;
    }
    rendered.value = true;
    void updateGeometry();
  },
  { immediate: true },
);
</script>

<template>
  <Teleport to="body">
    <Transition
      name="sb-menu-pop"
      :duration="SB_MENU_POP_TRANSITION_MS"
      @after-leave="onAfterLeave"
    >
      <div
        v-if="rendered"
        :ref="setMenuElement"
        class="ctx-menu"
        role="menu"
        :style="{
          left: `${menuPosition.x}px`,
          top: `${menuPosition.y}px`,
          zIndex: String(SB_LAYER_Z_INDEX.contextMenu),
          '--sb-menu-origin-x': `${menuOrigin.x}px`,
          '--sb-menu-origin-y': `${menuOrigin.y}px`,
        }"
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
    </Transition>
  </Teleport>
</template>
