<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from "vue";
import { ClipboardList, X } from "@lucide/vue";
import ProgressBar from "./ProgressBar.vue";
import { useWorkspaceProgress } from "../composables/useRepositoryWorkspace";
import { useTaskCenter, type TaskProgress } from "../composables/useTaskCenter";
import {
  SB_LAYER_Z_INDEX,
  SB_MENU_POP_TRANSITION_MS,
  createAnchoredMenuPosition,
} from "../composables/menuMotion";
import { useAnchoredMenuSurface } from "../composables/useAnchoredMenuSurface";

const popoverOpen = ref(false);
const buttonRef = ref<HTMLElement | null>(null);
const {
  surfaceEl: popoverRef,
  position: popoverPosition,
  origin: popoverOrigin,
  syncPosition,
} = useAnchoredMenuSurface(createAnchoredMenuPosition(8, 8));

const { operationProgress } = useWorkspaceProgress();
const { tasks: registeredTasks } = useTaskCenter();

const repositoryTask = computed<TaskProgress | null>(() => {
  if (!operationProgress.value) return null;
  return {
    id: "workspace-operation",
    label: operationProgress.value.label,
    detail: operationProgress.value.detail,
    value: operationProgress.value.value,
    indeterminate: operationProgress.value.indeterminate,
    source: "资源库",
    updatedAt: Date.now(),
  };
});

const tasks = computed(() => {
  const items = [...registeredTasks.value];
  if (repositoryTask.value) {
    items.unshift(repositoryTask.value);
  }
  return items.sort((first, second) => second.updatedAt - first.updatedAt);
});
const activeTaskCount = computed(() => tasks.value.length);

function updatePopoverPosition() {
  const rect = buttonRef.value?.getBoundingClientRect();
  const width = 340;
  const height = Math.min(360, Math.max(180, 96 + activeTaskCount.value * 70));
  const left = Math.max(8, Math.min(rect ? rect.left : 8, window.innerWidth - width - 8));
  const anchorTop = rect ? rect.top : window.innerHeight - 40;
  const top = Math.max(8, Math.min(anchorTop - height - 8, window.innerHeight - height - 8));
  const anchorX = rect ? rect.left + rect.width / 2 : left + width / 2;
  const anchorY = rect ? rect.top : anchorTop;
  return createAnchoredMenuPosition(left, top, anchorX, anchorY);
}

function openPopover() {
  popoverOpen.value = true;
  document.addEventListener("pointerdown", handleDocumentPointerDown, true);
  document.addEventListener("keydown", handleDocumentKeydown);
}

function closePopover() {
  popoverOpen.value = false;
  document.removeEventListener("pointerdown", handleDocumentPointerDown, true);
  document.removeEventListener("keydown", handleDocumentKeydown);
}

function togglePopover() {
  if (popoverOpen.value) {
    closePopover();
    return;
  }
  openPopover();
}

function handleDocumentPointerDown(event: PointerEvent) {
  const target = event.target as Node | null;
  if (!target) return;
  if (popoverRef.value?.contains(target) || buttonRef.value?.contains(target)) return;
  closePopover();
}

function handleDocumentKeydown(event: KeyboardEvent) {
  if (event.key === "Escape") {
    closePopover();
  }
}

watch(popoverOpen, async (open) => {
  if (!open) return;
  await syncPosition(updatePopoverPosition());
});

onBeforeUnmount(() => {
  closePopover();
});
</script>

<template>
  <button
    ref="buttonRef"
    type="button"
    class="workspace-footer__btn task-button"
    :class="{ 'is-active': popoverOpen, 'has-tasks': activeTaskCount > 0 }"
    title="任务"
    aria-label="任务"
    :aria-expanded="popoverOpen"
    @click="togglePopover"
  >
    <ClipboardList :size="14" aria-hidden="true" />
    <span v-if="activeTaskCount > 0" class="task-button__badge">{{ activeTaskCount }}</span>
  </button>

  <Teleport to="body">
    <Transition name="sb-menu-pop" :duration="SB_MENU_POP_TRANSITION_MS">
      <section
        v-if="popoverOpen"
        ref="popoverRef"
        class="task-popover"
        :style="{
          left: `${popoverPosition.x}px`,
          top: `${popoverPosition.y}px`,
          zIndex: String(SB_LAYER_Z_INDEX.popover),
          '--sb-menu-origin-x': `${popoverOrigin.x}px`,
          '--sb-menu-origin-y': `${popoverOrigin.y}px`,
        }"
        aria-label="任务进度"
      >
        <header class="task-popover__header">
          <span>任务</span>
          <button
            type="button"
            class="task-popover__close"
            title="关闭"
            aria-label="关闭任务"
            @click="closePopover"
          >
            <X :size="13" aria-hidden="true" />
          </button>
        </header>
        <div v-if="tasks.length" class="task-popover__list">
          <article v-for="task in tasks" :key="task.id" class="task-popover__item">
            <div class="task-popover__source">{{ task.source ?? "任务" }}</div>
            <ProgressBar
              :value="task.value"
              :label="task.label"
              :detail="task.detail"
              :indeterminate="task.indeterminate"
            />
          </article>
        </div>
        <div v-else class="task-popover__empty">当前没有运行中的任务。</div>
      </section>
    </Transition>
  </Teleport>
</template>
