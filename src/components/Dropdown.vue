<script setup lang="ts" generic="T extends string | number">
import { computed, nextTick, onBeforeUnmount, ref, watch } from "vue";
import { ChevronDown } from "lucide-vue-next";
import {
  SB_LAYER_Z_INDEX,
  SB_MENU_POP_TRANSITION_MS,
  clampAnchoredMenuPosition,
  createAnchoredMenuPosition,
  createPlacedAnchoredMenuPosition,
  resolveMenuTransformOrigin,
} from "../composables/menuMotion";

interface Option {
  value: T;
  label: string;
  hint?: string;
}

const props = defineProps<{
  modelValue: T;
  options: Option[];
  icon?: unknown;
  placeholder?: string;
  placement?: "top" | "bottom";
  disabled?: boolean;
}>();

const emit = defineEmits<{ "update:modelValue": [value: T] }>();

const open = ref(false);
const root = ref<HTMLElement | null>(null);
const triggerEl = ref<HTMLElement | null>(null);
const menuEl = ref<HTMLElement | null>(null);
const placement = computed(() => props.placement === "bottom" ? "bottom" : "top");
const pos = ref(createAnchoredMenuPosition(0, 0));
const origin = ref({ x: 0, y: 0 });
const anchorX = ref<number | null>(null);

const current = computed(() =>
  props.options.find((option) => option.value === props.modelValue),
);

function toggle(event: MouseEvent) {
  if (props.disabled) return;
  const trigger = triggerEl.value ?? root.value;
  if (trigger) {
    const rect = trigger.getBoundingClientRect();
    anchorX.value = Number.isFinite(event.clientX) && event.clientX > 0
      ? event.clientX
      : rect.left + rect.width / 2;
  }
  open.value = !open.value;
}

function pick(option: Option) {
  emit("update:modelValue", option.value);
  open.value = false;
}

function onDocPointer(event: PointerEvent) {
  const target = event.target as Node | null;
  if (!target) return;
  if (root.value?.contains(target)) return;
  if (menuEl.value?.contains(target)) return;
  open.value = false;
}

function onKey(event: KeyboardEvent) {
  if (event.key === "Escape" && open.value) {
    open.value = false;
    event.stopPropagation();
  }
}

function closeMenu() {
  open.value = false;
}

function updatePosition(width = 0, height = 0) {
  const trigger = triggerEl.value ?? root.value;
  if (!trigger) return;
  const nextPos = createPlacedAnchoredMenuPosition(
    trigger.getBoundingClientRect(),
    placement.value,
    height,
    anchorX.value ?? undefined,
  );
  pos.value = width > 0 && height > 0
    ? clampAnchoredMenuPosition(nextPos, width, height)
    : nextPos;
  origin.value = resolveMenuTransformOrigin(
    pos.value,
    width > 0 ? width : Number.POSITIVE_INFINITY,
    height > 0 ? height : Number.POSITIVE_INFINITY,
  );
}

watch(open, async (value) => {
  if (value) {
    updatePosition();
    await nextTick();
    const element = menuEl.value;
    if (element) updatePosition(element.offsetWidth, element.offsetHeight);
    document.addEventListener("pointerdown", onDocPointer, true);
    document.addEventListener("keydown", onKey);
    window.addEventListener("scroll", closeMenu, true);
    window.addEventListener("resize", closeMenu);
    window.addEventListener("blur", closeMenu);
  } else {
    document.removeEventListener("pointerdown", onDocPointer, true);
    document.removeEventListener("keydown", onKey);
    window.removeEventListener("scroll", closeMenu, true);
    window.removeEventListener("resize", closeMenu);
    window.removeEventListener("blur", closeMenu);
  }
});

onBeforeUnmount(() => {
  document.removeEventListener("pointerdown", onDocPointer, true);
  document.removeEventListener("keydown", onKey);
  window.removeEventListener("scroll", closeMenu, true);
  window.removeEventListener("resize", closeMenu);
  window.removeEventListener("blur", closeMenu);
});
</script>

<template>
  <div ref="root" class="dd">
    <button
      ref="triggerEl"
      type="button"
      class="chat-chip"
      :class="{ 'is-open': open, 'is-disabled': disabled }"
      :disabled="disabled"
      :aria-haspopup="true"
      :aria-expanded="open"
      @click="toggle"
    >
      <component v-if="icon" :is="icon" :size="13" aria-hidden="true" />
      <span class="chat-chip__label">
        {{ current?.label ?? placeholder ?? "-" }}
      </span>
      <ChevronDown :size="12" aria-hidden="true" class="chat-chip__caret" />
    </button>
  </div>
  <Teleport to="body">
    <Transition name="sb-menu-pop" :duration="SB_MENU_POP_TRANSITION_MS">
      <div
        v-if="open"
        ref="menuEl"
        class="dd__menu"
        role="listbox"
        :style="{
          left: `${pos.x}px`,
          top: `${pos.y}px`,
          zIndex: String(SB_LAYER_Z_INDEX.dropdown),
          '--sb-menu-origin-x': `${origin.x}px`,
          '--sb-menu-origin-y': `${origin.y}px`,
        }"
      >
        <button
          v-for="option in options"
          :key="String(option.value)"
          type="button"
          class="dd__item"
          :class="{ 'is-active': option.value === modelValue }"
          role="option"
          :aria-selected="option.value === modelValue"
          @click="pick(option)"
        >
          <span class="dd__item-label">{{ option.label }}</span>
          <span v-if="option.hint" class="dd__item-hint">{{ option.hint }}</span>
        </button>
      </div>
    </Transition>
  </Teleport>
</template>
