<script setup lang="ts">
import { GripVertical, Play, Trash2 } from "@lucide/vue";
import WorkspacePlayerBar from "../../../components/WorkspacePlayerBar.vue";
import type { PlaylistDetail, PlaylistItem } from "../../../types/repository";
import type { WorkspacePlayerBarHandlers, WorkspacePlayerBarProps } from "./usePlayerUi";

defineProps<{
  activePlaylistDetail: PlaylistDetail | null;
  hasPlayer: boolean;
  playlistStatusLabel: string;
  showWorkspacePlayer: boolean;
  workspacePlayerBarHandlers: WorkspacePlayerBarHandlers;
  workspacePlayerBarProps: WorkspacePlayerBarProps;
  playlistItemThumbnailSrc: (item: PlaylistItem) => string | null;
}>();

const emit = defineEmits<{
  dragStart: [item: PlaylistItem];
  dropItem: [item: PlaylistItem];
  openPreview: [item: PlaylistItem];
  play: [item?: PlaylistItem | null];
  remove: [item: PlaylistItem];
}>();
</script>

<template>
  <section class="playlist-page">
    <div v-if="activePlaylistDetail" class="playlist-page__panel">
      <header class="playlist-page__header">
        <div>
          <p class="asset-browser__eyebrow">播放集</p>
          <h1>{{ activePlaylistDetail.playlist.name }}</h1>
          <p class="playlist-page__subline">
            {{ playlistStatusLabel }}
            <template v-if="!hasPlayer"> · 缺少对应播放插件</template>
          </p>
        </div>
        <div class="playlist-page__actions">
          <button type="button" class="ghost files-toolbar__btn" :disabled="!hasPlayer" @click="emit('play')">
            <Play :size="14" aria-hidden="true" />
            播放
          </button>
        </div>
      </header>

      <div v-if="!activePlaylistDetail.items.length" class="playlist-page__empty">
        <h2>播放集还是空的</h2>
        <p>在文件浏览区右键文件，使用“加入播放列表”把内容加入这里。</p>
      </div>

      <div v-else class="playlist-page__list" role="list" aria-label="播放集条目">
        <article
          v-for="item in activePlaylistDetail.items"
          :key="item.playlistItemId"
          class="playlist-page__item"
          :class="{ 'is-unavailable': item.status !== 'ready' }"
          role="listitem"
          draggable="true"
          @dragstart="emit('dragStart', item)"
          @dragover.prevent
          @drop.prevent="emit('dropItem', item)"
          @dblclick="emit('play', item)"
        >
          <button type="button" class="playlist-page__drag" aria-label="拖动排序">
            <GripVertical :size="16" aria-hidden="true" />
          </button>
          <button type="button" class="playlist-page__preview" @click="emit('openPreview', item)">
            <img v-if="playlistItemThumbnailSrc(item)" :src="playlistItemThumbnailSrc(item) ?? undefined" alt="" />
            <span v-else>{{ item.extension.toUpperCase() }}</span>
          </button>
          <div class="playlist-page__meta">
            <button type="button" class="playlist-page__title" @click="emit('openPreview', item)">
              {{ item.filename }}
            </button>
            <p v-if="item.status !== 'ready'" class="playlist-page__status">
              {{ item.statusReason ?? item.status }}
            </p>
            <p v-else class="playlist-page__path">{{ item.path }}</p>
          </div>
          <div class="playlist-page__row-actions">
            <button
              type="button"
              class="ghost files-toolbar__btn"
              :disabled="item.status !== 'ready' || !hasPlayer"
              @click="emit('play', item)"
            >
              <Play :size="14" aria-hidden="true" />
              播放
            </button>
            <button type="button" class="ghost danger files-toolbar__btn" @click="emit('remove', item)">
              <Trash2 :size="14" aria-hidden="true" />
              移除
            </button>
          </div>
        </article>
      </div>

      <WorkspacePlayerBar v-if="showWorkspacePlayer" v-bind="workspacePlayerBarProps" v-on="workspacePlayerBarHandlers" />
    </div>

    <div v-else class="playlist-page__empty">
      <h2>选择一个播放集</h2>
      <p>在左侧播放集区选择要查看或播放的列表。</p>
    </div>
  </section>
</template>
