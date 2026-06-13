<script setup lang="ts">
import { Play, Plus, Trash2 } from "lucide-vue-next";
import type { PlaylistSummary } from "../types/repository";

defineProps<{
  activePanel: string;
  activePlaylistId?: string | null;
  activeRepoId: string | null;
  availablePlaylistPlayerTypeIds: ReadonlySet<string>;
  availablePlaylistPlayersCount: number;
  isActiveRepositoryMissing: boolean;
  playlistItems: PlaylistSummary[];
}>();

const emit = defineEmits<{
  create: [];
  open: [playlistId: string];
  play: [playlist: PlaylistSummary];
  remove: [playlistId: string];
}>();
</script>

<template>
  <section class="workspace-group workspace-group--tree">
    <div class="workspace-group__header">
      <span>播放集</span>
      <div class="workspace-group__actions">
        <button
          type="button"
          class="workspace-tree-action"
          :disabled="!activeRepoId || isActiveRepositoryMissing || !availablePlaylistPlayersCount"
          title="新建播放集"
          aria-label="新建播放集"
          @click="emit('create')"
        >
          <Plus :size="13" aria-hidden="true" />
        </button>
      </div>
    </div>
    <div v-if="!activeRepoId" class="workspace-empty workspace-empty--compact">
      <p class="workspace-empty__text">先选择或添加一个资源库。</p>
    </div>
    <div v-else-if="isActiveRepositoryMissing" class="workspace-empty workspace-empty--compact">
      <p class="workspace-empty__text">资源库修复后可继续使用播放集。</p>
    </div>
    <div v-else-if="playlistItems.length" class="workspace-playlists">
      <article
        v-for="playlist in playlistItems"
        :key="playlist.playlistId"
        class="workspace-playlists__item"
        :class="{ 'is-active': activePanel === 'playlist' && activePlaylistId === playlist.playlistId }"
      >
        <button type="button" class="workspace-playlists__main" :title="playlist.name" @click="emit('open', playlist.playlistId)">
          <strong>{{ playlist.name }}</strong>
          <span>{{ playlist.playerLabel }} · {{ playlist.itemCount }} 项</span>
        </button>
        <div class="workspace-playlists__actions">
          <button
            type="button"
            class="workspace-tree-action"
            :disabled="!availablePlaylistPlayerTypeIds.has(playlist.playerTypeId)"
            title="播放"
            aria-label="播放播放集"
            @click="emit('play', playlist)"
          >
            <Play :size="13" aria-hidden="true" />
          </button>
          <button
            type="button"
            class="workspace-tree-action workspace-tree-action--danger"
            title="删除"
            aria-label="删除播放集"
            @click="emit('remove', playlist.playlistId)"
          >
            <Trash2 :size="13" aria-hidden="true" />
          </button>
        </div>
      </article>
    </div>
    <div v-else-if="!availablePlaylistPlayersCount" class="workspace-empty workspace-empty--compact">
      <p class="workspace-empty__text">当前没有可用的播放插件类型。</p>
    </div>
    <div v-else class="workspace-empty workspace-empty--compact">
      <p class="workspace-empty__text">还没有播放集。</p>
    </div>
  </section>
</template>
