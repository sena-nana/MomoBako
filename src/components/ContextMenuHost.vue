<script setup lang="ts">
import { nextTick, ref, watch } from "vue";
import { Check } from "lucide-vue-next";
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
  clampAnchoredMenuPosition,
  createAnchoredMenuPosition,
  resolveMenuTransformOrigin,
} from "../composables/menuMotion";

const { state } = useContextMenu();

const menuEl = ref<HTMLElement | null>(null);
const rendered = ref(false);
const pos = ref(createAnchoredMenuPosition(0, 0));
const origin = ref({ x: 0, y: 0 });

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
  const initialPos = createAnchoredMenuPosition(
    state.x,
    state.y,
    state.anchorX,
    state.anchorY,
  );
  pos.value = initialPos;
  origin.value = resolveMenuTransformOrigin(initialPos);
  await nextTick();
  const element = menuEl.value;
  if (!element) return;
  const clampedPos = clampAnchoredMenuPosition(initialPos, element.offsetWidth, element.offsetHeight);
  pos.value = clampedPos;
  origin.value = resolveMenuTransformOrigin(clampedPos, element.offsetWidth, element.offsetHeight);
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
        ref="menuEl"
        class="ctx-menu"
        role="menu"
        :style="{
          left: `${pos.x}px`,
          top: `${pos.y}px`,
          zIndex: String(SB_LAYER_Z_INDEX.contextMenu),
          '--sb-menu-origin-x': `${origin.x}px`,
          '--sb-menu-origin-y': `${origin.y}px`,
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
