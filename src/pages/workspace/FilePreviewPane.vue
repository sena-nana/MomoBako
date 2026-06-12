<script setup lang="ts">
import { ArrowLeft, Eye, FileAudio, FileImage, FileVideo, FolderOpen } from "lucide-vue-next";
import type { Component } from "vue";
import { computed, onBeforeUnmount, ref, watch } from "vue";
import { usePlaylistPlayer } from "../../composables/usePlaylistPlayer";
import FileMetadataEditor from "./FileMetadataEditor.vue";
import ThumbnailPalette from "../../components/ThumbnailPalette.vue";
import type { FileBrowserEntry, RepositoryTagGroup } from "../../types/repository";

const props = defineProps<{
  entry: FileBrowserEntry;
  plugin: { component: Component } | null;
  repoId: string;
  thumbnailSrc: (entry: FileBrowserEntry) => string | null;
  isVideoEntry: (entry: FileBrowserEntry) => boolean;
  isAudioEntry: (entry: FileBrowserEntry) => boolean;
  hardlinkStateLabel: (entry: FileBrowserEntry) => string;
  statusLabel: (status: string) => string;
  isSavingMetadata: boolean;
  availableTags: string[];
  tagGroups?: RepositoryTagGroup[];
  thumbnailPalette: (entry: FileBrowserEntry) => string[];
  saveMetadata: (entry: FileBrowserEntry, metadata: Record<string, unknown>) => Promise<unknown>;
}>();

const emit = defineEmits<{
  back: [];
  open: [path: string];
  reveal: [path: string];
  thumbnailError: [entry: FileBrowserEntry];
  thumbnailLoaded: [entry: FileBrowserEntry, event: Event];
}>();

const playlistPlayer = usePlaylistPlayer();
const playerPreviewMount = ref<HTMLElement | null>(null);
const isCurrentPlaylistItem = computed(() => (
  playlistPlayer.activeRepoId.value === props.repoId
  && playlistPlayer.currentItem.value?.path === props.entry.path
));

function handlePreviewMediaPlay(event: Event) {
  if (isCurrentPlaylistItem.value) return;
  if (!(event.target instanceof HTMLMediaElement)) return;
  if (playlistPlayer.activeFileClass.value !== "audio" || !playlistPlayer.isPlaying.value) return;
  void playlistPlayer.setPlaybackState({ isPlaying: false });
}

watch(
  [isCurrentPlaylistItem, playerPreviewMount],
  ([active, element]) => {
    playlistPlayer.attachVisibleMountTarget(active ? element : null);
  },
  { immediate: true },
);

onBeforeUnmount(() => {
  playlistPlayer.attachVisibleMountTarget(null);
});
</script>

<template>
  <header class="files-preview-page__header">
    <button type="button" class="ghost files-preview-page__back" @click="emit('back')">
      <ArrowLeft :size="15" aria-hidden="true" />
      返回
    </button>
    <div>
      <p class="asset-browser__eyebrow">文件预览</p>
      <h1>{{ entry.name }}</h1>
      <p class="files-preview-page__subline">{{ entry.path }}</p>
    </div>
    <div class="files-preview-page__actions">
      <button type="button" class="ghost" @click="emit('open', entry.path)">
        <Eye :size="14" aria-hidden="true" />
        打开
      </button>
      <button type="button" class="ghost" @click="emit('reveal', entry.path)">
        <FolderOpen :size="14" aria-hidden="true" />
        定位
      </button>
    </div>
  </header>

  <div class="files-preview-page__body">
    <div class="files-preview-page__preview-shell">
      <div
        class="files-preview-page__preview"
        :class="{ 'files-preview-page__preview--plugin': plugin }"
        @play.capture="handlePreviewMediaPlay"
      >
        <div
          v-if="isCurrentPlaylistItem"
          ref="playerPreviewMount"
          class="files-preview-page__player-mount"
          aria-label="当前播放画面"
        ></div>
        <component
          :is="plugin.component"
          v-else-if="plugin"
          :entry="entry"
          :repo-id="repoId"
        />
        <img
          v-else-if="thumbnailSrc(entry)"
          :src="thumbnailSrc(entry) ?? undefined"
          alt=""
          crossorigin="anonymous"
          @load="emit('thumbnailLoaded', entry, $event)"
          @error="emit('thumbnailError', entry)"
        />
        <FileVideo v-else-if="isVideoEntry(entry)" :size="54" aria-hidden="true" />
        <FileAudio v-else-if="isAudioEntry(entry)" :size="54" aria-hidden="true" />
        <FileImage v-else :size="54" aria-hidden="true" />
      </div>
      <div class="files-preview-page__palette-slot">
        <ThumbnailPalette :colors="thumbnailPalette(entry)" />
      </div>
    </div>
    <div class="files-detail__stats files-preview-page__stats">
      <div class="asset-meta__row">
        <span>类型</span>
        <span class="asset-meta__value">{{ entry.extension || '文件' }}</span>
      </div>
      <div class="asset-meta__row">
        <span>大小</span>
        <span class="asset-meta__value">{{ entry.sizeLabel || "未知" }}</span>
      </div>
      <div class="asset-meta__row">
        <span>状态</span>
        <span class="asset-meta__value">{{ entry.status ? statusLabel(entry.status) : "未索引" }}</span>
      </div>
      <div v-if="hardlinkStateLabel(entry)" class="asset-meta__row">
        <span>硬链接</span>
        <span class="asset-meta__value">{{ hardlinkStateLabel(entry) }}</span>
      </div>
      <div class="asset-meta__row">
        <span>修改时间</span>
        <span class="asset-meta__value">{{ entry.modifiedAt ? new Date(entry.modifiedAt).toLocaleString("zh-CN") : "未记录" }}</span>
      </div>
      <FileMetadataEditor
        :entry="entry"
        :is-saving="isSavingMetadata"
        :available-tags="availableTags"
        :tag-groups="tagGroups"
        :save-metadata="saveMetadata"
      />
    </div>
  </div>
</template>
