<script setup lang="ts">
import { computed, watch, type ComponentPublicInstance } from "vue";
import { Check, LoaderCircle, Plus, Trash2, X } from "@lucide/vue";
import type { RepositoryBackendOption, RepositorySummary } from "../types/repository";
import {
  SB_LAYER_Z_INDEX,
  SB_MENU_POP_TRANSITION_MS,
  createAnchoredMenuPosition,
} from "../composables/menuMotion";
import { useAnchoredMenuSurface } from "../composables/useAnchoredMenuSurface";
import type { RepositoryPopoverMode } from "./useRepositorySwitcherUi";

type RepositoryPopoverPosition = {
  left: number;
  top: number;
  width: number;
  anchorX: number;
  anchorY: number;
};

const props = defineProps<{
  activeRepoId: string | null;
  addRepositoryError: string;
  backendOptions: Array<{ value: string; label: string; enabled: boolean }>;
  backendSubmitDisabled: boolean;
  isSubmittingBackend: boolean;
  mode: RepositoryPopoverMode;
  position: RepositoryPopoverPosition;
  repositories: RepositorySummary[];
  selectedBackend: RepositoryBackendOption | null;
}>();

const backendName = defineModel<string>("backendName", { required: true });
const backendPassword = defineModel<string>("backendPassword", { required: true });
const backendRoot = defineModel<string>("backendRoot", { required: true });
const backendUrl = defineModel<string>("backendUrl", { required: true });
const backendUsername = defineModel<string>("backendUsername", { required: true });
const modeModel = defineModel<RepositoryPopoverMode>("mode", { required: true });

const emit = defineEmits<{
  close: [];
  deleteActive: [];
  selectBackend: [pluginId: string];
  selectRepository: [repoId: string];
  setPopoverRef: [element: HTMLElement | null];
  showAddMenu: [];
  submit: [];
}>();

const isMenuMode = computed(() => props.mode === "switcher" || props.mode === "addMenu");
const {
  surfaceEl: popoverEl,
  position: popoverPosition,
  origin: popoverOrigin,
  setPosition,
  syncPosition,
} = useAnchoredMenuSurface();

function setPopoverElement(element: Element | ComponentPublicInstance | null) {
  const domElement = element instanceof HTMLElement ? element : null;
  popoverEl.value = domElement;
  emit("setPopoverRef", domElement);
}

async function updatePopoverGeometry() {
  const nextPosition = createAnchoredMenuPosition(
    props.position.left,
    props.position.top,
    props.position.anchorX,
    props.position.anchorY,
  );
  setPosition(nextPosition);
  await syncPosition(nextPosition);
}

watch(
  () => [props.mode, props.position.left, props.position.top, props.position.anchorX, props.position.anchorY] as const,
  async ([mode]) => {
    if (mode === "closed") return;
    await updatePopoverGeometry();
  },
  { immediate: true },
);
</script>

<template>
  <Teleport to="body">
    <Transition :name="isMenuMode ? 'sb-menu-pop' : 'panel'" :duration="isMenuMode ? SB_MENU_POP_TRANSITION_MS : undefined">
      <section
        v-if="mode !== 'closed'"
        :ref="setPopoverElement"
        class="repository-add-popover"
        :class="{
          'ctx-menu': mode === 'addMenu',
          'repository-add-popover--menu': mode === 'addMenu',
          'repository-add-popover--switcher': mode === 'switcher',
        }"
        :style="{
          left: `${popoverPosition.x}px`,
          top: `${popoverPosition.y}px`,
          width: mode === 'switcher' ? `${position.width}px` : undefined,
          zIndex: String(SB_LAYER_Z_INDEX.popover),
          '--sb-menu-origin-x': `${popoverOrigin.x}px`,
          '--sb-menu-origin-y': `${popoverOrigin.y}px`,
        }"
        :aria-label="mode === 'switcher' ? '切换资源库' : '添加资源库'"
      >
        <template v-if="mode === 'switcher'">
          <div class="repository-switcher__list" role="menu" aria-label="资源库列表">
            <button
              v-for="library in repositories"
              :key="library.repoId"
              type="button"
              class="repository-switcher__item"
              :class="{ 'is-active': activeRepoId === library.repoId, 'is-missing': library.status === 'missing' }"
              :title="`${library.name}\n${library.path}`"
              :aria-label="`切换资源库 ${library.name}`"
              :disabled="isSubmittingBackend"
              @click="emit('selectRepository', library.repoId)"
            >
              <span class="repository-switcher__check">
                <Check v-if="activeRepoId === library.repoId" :size="13" aria-hidden="true" />
              </span>
              <span class="repository-switcher__main">
                <strong>{{ library.name }}</strong>
                <span v-if="library.status === 'missing'" class="repository-switcher__status">丢失</span>
              </span>
            </button>
          </div>

          <div class="repository-switcher__actions">
            <button type="button" class="ctx-menu__item" :disabled="isSubmittingBackend" @click="emit('showAddMenu')">
              <Plus :size="14" aria-hidden="true" />
              <span class="ctx-menu__label">添加资源库</span>
            </button>
            <button
              type="button"
              class="ctx-menu__item ctx-menu__item--danger"
              :disabled="!activeRepoId || isSubmittingBackend"
              @click="emit('deleteActive')"
            >
              <Trash2 :size="14" aria-hidden="true" />
              <span class="ctx-menu__label">
                删除当前资源库
              </span>
            </button>
          </div>

          <p v-if="addRepositoryError" class="repository-add-popover__error">
            {{ addRepositoryError }}
          </p>
        </template>

        <template v-else-if="mode === 'addMenu'">
          <button
            v-for="option in backendOptions"
            :key="option.value"
            type="button"
            class="ctx-menu__item"
            :disabled="isSubmittingBackend || !option.enabled"
            @click="emit('selectBackend', String(option.value))"
          >
            {{ option.label }}
          </button>

          <p v-if="addRepositoryError" class="repository-add-popover__error">
            {{ addRepositoryError }}
          </p>
        </template>

        <template v-else-if="mode === 'form'">
          <header class="repository-add-popover__header">
            <span>{{ selectedBackend?.name ?? "添加资源库" }}</span>
            <button type="button" class="repository-add-popover__close" title="关闭" aria-label="关闭添加资源库" :disabled="isSubmittingBackend" @click="emit('close')">
              <X :size="13" aria-hidden="true" />
            </button>
          </header>

          <div class="repository-add-popover__body">
            <p class="repository-add-popover__summary">
              {{ selectedBackend?.description ?? "填写资源库配置。" }}
            </p>

            <label class="repository-add-popover__field">
              <span>资源库名称</span>
              <input v-model="backendName" type="text" placeholder="可选，默认使用后端名称" :disabled="isSubmittingBackend" />
            </label>

            <label class="repository-add-popover__field">
              <span>服务地址</span>
              <input v-model="backendUrl" type="url" placeholder="https://example.com/dav/" :disabled="isSubmittingBackend" />
            </label>
            <label class="repository-add-popover__field">
              <span>根目录</span>
              <input v-model="backendRoot" type="text" placeholder="/assets/anime" :disabled="isSubmittingBackend" />
            </label>
            <label class="repository-add-popover__field">
              <span>用户名</span>
              <input v-model="backendUsername" type="text" placeholder="可选" :disabled="isSubmittingBackend" />
            </label>
            <label class="repository-add-popover__field">
              <span>密码 / Token</span>
              <input v-model="backendPassword" type="password" placeholder="可选" :disabled="isSubmittingBackend" />
            </label>
            <p class="repository-add-popover__note">
              当前仅完成后端配置入口与服务端抽象。远端适配器尚未实现，请先用于配置演进与契约联调。
            </p>

            <p v-if="addRepositoryError" class="repository-add-popover__error">
              {{ addRepositoryError }}
            </p>
          </div>

          <div class="repository-add-popover__actions">
            <button type="button" class="ghost" :disabled="isSubmittingBackend" @click="modeModel = 'addMenu'">
              返回
            </button>
            <button type="button" class="ghost" :disabled="isSubmittingBackend" @click="emit('close')">
              取消
            </button>
            <button type="button" class="primary" :disabled="isSubmittingBackend || backendSubmitDisabled" @click="emit('submit')">
              <LoaderCircle v-if="isSubmittingBackend" class="spin" :size="13" aria-hidden="true" />
              {{ isSubmittingBackend ? "创建中" : "创建" }}
            </button>
          </div>
        </template>

      </section>
    </Transition>
  </Teleport>
</template>
