<script setup lang="ts">
import { Archive, Download, GitBranch, LoaderCircle, X } from "lucide-vue-next";
import type {
  RepositoryArchiveFormat,
  RepositoryCompressionLevel,
  RepositorySummary,
} from "../../types/repository";

defineProps<{
  repository: RepositorySummary | null;
  target: "archive" | "git";
  archiveFormat: RepositoryArchiveFormat;
  compression: RepositoryCompressionLevel;
  encrypt: boolean;
  password: string;
  gitRemote: string;
  gitBranch: string;
  gitMessage: string;
  error: string;
  isExporting: boolean;
  actionLabel: string;
}>();

const emit = defineEmits<{
  close: [];
  submit: [];
  "update:target": [value: "archive" | "git"];
  "update:archiveFormat": [value: RepositoryArchiveFormat];
  "update:compression": [value: RepositoryCompressionLevel];
  "update:encrypt": [value: boolean];
  "update:password": [value: string];
  "update:gitRemote": [value: string];
  "update:gitBranch": [value: string];
  "update:gitMessage": [value: string];
}>();

function updateSelect<T extends string>(event: Event) {
  return (event.target as HTMLSelectElement | null)?.value as T | undefined;
}

function updateInput(event: Event) {
  return (event.target as HTMLInputElement | null)?.value ?? "";
}

function updateChecked(event: Event) {
  return Boolean((event.target as HTMLInputElement | null)?.checked);
}
</script>

<template>
  <Teleport to="body">
    <Transition name="modal">
      <div
        v-if="repository"
        class="modal-overlay"
        role="dialog"
        aria-modal="true"
        aria-label="导出资源库"
        @click.self="emit('close')"
      >
        <div class="modal-card dialog-card repository-export-dialog">
          <div class="dialog-card__header">
            <Download :size="14" aria-hidden="true" />
            <span>导出资源库</span>
            <button
              type="button"
              class="repository-export-dialog__close"
              title="关闭"
              aria-label="关闭导出配置"
              :disabled="isExporting"
              @click="emit('close')"
            >
              <X :size="13" aria-hidden="true" />
            </button>
          </div>

          <div class="dialog-card__body repository-export-dialog__body">
            <div class="repository-export-dialog__repo">
              <strong>{{ repository.name }}</strong>
              <span>{{ repository.path }}</span>
            </div>

            <div class="segmented repository-export-dialog__tabs">
              <button
                type="button"
                :class="{ 'is-active': target === 'archive' }"
                :disabled="isExporting"
                @click="emit('update:target', 'archive')"
              >
                <Archive :size="13" aria-hidden="true" />
                压缩包
              </button>
              <button
                type="button"
                :class="{ 'is-active': target === 'git' }"
                :disabled="isExporting"
                @click="emit('update:target', 'git')"
              >
                <GitBranch :size="13" aria-hidden="true" />
                Git
              </button>
            </div>

            <template v-if="target === 'archive'">
              <div class="repository-export-dialog__grid">
                <label class="dialog-field">
                  <span>格式</span>
                  <select
                    :value="archiveFormat"
                    :disabled="isExporting"
                    @change="emit('update:archiveFormat', updateSelect<RepositoryArchiveFormat>($event) ?? archiveFormat)"
                  >
                    <option value="zip">zip</option>
                    <option value="7z">7z</option>
                    <option value="tar">tar</option>
                  </select>
                </label>

                <label class="dialog-field">
                  <span>压缩</span>
                  <select
                    :value="compression"
                    :disabled="isExporting"
                    @change="emit('update:compression', updateSelect<RepositoryCompressionLevel>($event) ?? compression)"
                  >
                    <option value="none">不压缩</option>
                    <option value="fast">快速</option>
                    <option value="balanced">均衡</option>
                    <option value="maximum">最大</option>
                  </select>
                </label>
              </div>

              <label class="repository-export-dialog__toggle">
                <input
                  :checked="encrypt"
                  type="checkbox"
                  :disabled="isExporting"
                  @change="emit('update:encrypt', updateChecked($event))"
                />
                <span>加密压缩包</span>
              </label>

              <label v-if="encrypt" class="dialog-field">
                <span>密码</span>
                <input
                  :value="password"
                  type="password"
                  placeholder="用于压缩包加密"
                  :disabled="isExporting"
                  @input="emit('update:password', updateInput($event))"
                  @keydown.enter.prevent="emit('submit')"
                />
              </label>
            </template>

            <template v-else>
              <div class="repository-export-dialog__grid">
                <label class="dialog-field">
                  <span>远端</span>
                  <input
                    :value="gitRemote"
                    type="text"
                    placeholder="origin"
                    :disabled="isExporting"
                    @input="emit('update:gitRemote', updateInput($event))"
                  />
                </label>

                <label class="dialog-field">
                  <span>分支</span>
                  <input
                    :value="gitBranch"
                    type="text"
                    placeholder="默认当前分支"
                    :disabled="isExporting"
                    @input="emit('update:gitBranch', updateInput($event))"
                  />
                </label>
              </div>

              <label class="dialog-field">
                <span>提交信息</span>
                <input
                  :value="gitMessage"
                  type="text"
                  placeholder="导出资源库"
                  :disabled="isExporting"
                  @input="emit('update:gitMessage', updateInput($event))"
                  @keydown.enter.prevent="emit('submit')"
                />
              </label>
            </template>

            <p v-if="error" class="repository-add-popover__error">
              {{ error }}
            </p>
          </div>

          <div class="dialog-card__actions">
            <button type="button" class="ghost" :disabled="isExporting" @click="emit('close')">
              取消
            </button>
            <button type="button" class="primary" :disabled="isExporting" @click="emit('submit')">
              <LoaderCircle v-if="isExporting" class="spin" :size="13" aria-hidden="true" />
              {{ isExporting ? "处理中" : actionLabel }}
            </button>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>
