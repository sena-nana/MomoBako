<script setup lang="ts">
import { Check, LoaderCircle, Plus, Trash2, X } from "lucide-vue-next";
import type { RepositoryBackendOption, RepositorySummary } from "../types/repository";
import type { RepositoryPopoverMode } from "./useRepositorySwitcherUi";

type RepositoryPopoverPosition = {
  left: number;
  top: number;
  width: number;
};

defineProps<{
  activeRepoId: string | null;
  addRepositoryError: string;
  backendOptions: Array<{ value: string; label: string; enabled: boolean }>;
  backendSubmitDisabled: boolean;
  isConfirmingRepositoryDelete: boolean;
  isRemovingRepository: boolean;
  isSubmittingBackend: boolean;
  mode: RepositoryPopoverMode;
  neteaseLoginMessage: string;
  neteaseQrSession: { unikey?: string; qrurl?: string; qrimg?: string | null } | null;
  neteaseCachePath: string;
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
  chooseNeteaseCacheFolder: [];
  createNeteaseQrSession: [];
  deleteActive: [];
  pollNeteaseQrSession: [];
  selectBackend: [pluginId: string];
  selectRepository: [repoId: string];
  setPopoverRef: [element: HTMLElement | null];
  showAddMenu: [];
  submit: [];
}>();
</script>

<template>
  <Teleport to="body">
    <Transition name="panel">
      <section
        v-if="mode !== 'closed'"
        :ref="(element) => emit('setPopoverRef', element as HTMLElement | null)"
        class="repository-add-popover"
        :class="{
          'ctx-menu': mode === 'addMenu',
          'repository-add-popover--menu': mode === 'addMenu',
          'repository-add-popover--switcher': mode === 'switcher',
        }"
        :style="{
          left: `${position.left}px`,
          top: `${position.top}px`,
          width: mode === 'switcher' ? `${position.width}px` : undefined,
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
              :disabled="isRemovingRepository || isSubmittingBackend"
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
            <button type="button" class="ctx-menu__item" :disabled="isSubmittingBackend || isRemovingRepository" @click="emit('showAddMenu')">
              <Plus :size="14" aria-hidden="true" />
              <span class="ctx-menu__label">添加资源库</span>
            </button>
            <button
              type="button"
              class="ctx-menu__item ctx-menu__item--danger"
              :class="{ 'ctx-menu__item--pending': isConfirmingRepositoryDelete }"
              :disabled="!activeRepoId || isSubmittingBackend || isRemovingRepository"
              @click="emit('deleteActive')"
            >
              <LoaderCircle v-if="isRemovingRepository" class="spin" :size="14" aria-hidden="true" />
              <Trash2 v-else :size="14" aria-hidden="true" />
              <span class="ctx-menu__label">
                {{ isConfirmingRepositoryDelete ? "确认删除当前资源库" : "删除当前资源库" }}
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

        <template v-else>
          <header class="repository-add-popover__header">
            <span>登录网易云音乐</span>
            <button type="button" class="repository-add-popover__close" title="关闭" aria-label="关闭添加资源库" :disabled="isSubmittingBackend" @click="emit('close')">
              <X :size="13" aria-hidden="true" />
            </button>
          </header>

          <div class="repository-add-popover__body">
            <p class="repository-add-popover__summary">
              创建资源库时登录网易云账号，并指定本地缓存目录。每个账号对应一个资源库，成功后会自动扫描创建的歌单和收藏的歌单。
            </p>

            <label class="repository-add-popover__field">
              <span>本地缓存目录</span>
              <button type="button" class="ghost" :disabled="isSubmittingBackend" @click="emit('chooseNeteaseCacheFolder')">
                {{ neteaseCachePath ? "重新选择目录" : "选择目录" }}
              </button>
            </label>
            <p v-if="neteaseCachePath" class="repository-add-popover__note">
              {{ neteaseCachePath }}
            </p>

            <div v-if="neteaseQrSession?.qrimg" class="file-metadata-card__candidate-import repository-add-popover__qr">
              <img
                :src="neteaseQrSession.qrimg"
                alt="网易云二维码登录"
                style="width: 180px; height: 180px; object-fit: contain; border-radius: 12px;"
              />
            </div>

            <p v-if="neteaseLoginMessage" class="repository-add-popover__note">
              {{ neteaseLoginMessage }}
            </p>

            <p v-if="addRepositoryError" class="repository-add-popover__error">
              {{ addRepositoryError }}
            </p>
          </div>

          <div class="repository-add-popover__actions">
            <button type="button" class="ghost" :disabled="isSubmittingBackend" @click="modeModel = 'addMenu'">
              返回
            </button>
            <button type="button" class="ghost" :disabled="isSubmittingBackend" @click="emit('createNeteaseQrSession')">
              重新生成二维码
            </button>
            <button type="button" class="primary" :disabled="isSubmittingBackend || !neteaseQrSession?.unikey || !neteaseCachePath" @click="emit('pollNeteaseQrSession')">
              <LoaderCircle v-if="isSubmittingBackend" class="spin" :size="13" aria-hidden="true" />
              {{ isSubmittingBackend ? "检查中" : "检查扫码结果" }}
            </button>
          </div>
        </template>
      </section>
    </Transition>
  </Teleport>
</template>
