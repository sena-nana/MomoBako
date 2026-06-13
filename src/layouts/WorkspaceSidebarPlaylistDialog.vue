<script setup lang="ts">
import type { PlaylistPlayerContribution } from "../types/repository";

defineProps<{
  availablePlaylistPlayers: PlaylistPlayerContribution[];
  playlistDialogDisabled: boolean;
  showPlaylistDialog: boolean;
}>();

const playlistName = defineModel<string>("playlistName", { required: true });
const playlistPlayerTypeId = defineModel<string>("playlistPlayerTypeId", { required: true });

const emit = defineEmits<{
  closePlaylistDialog: [];
  submitPlaylistDialog: [];
}>();
</script>

<template>
  <Teleport to="body">
    <Transition name="modal">
      <div v-if="showPlaylistDialog" class="modal-overlay" role="dialog" aria-modal="true" aria-label="新建播放集" @click.self="emit('closePlaylistDialog')">
        <div class="modal-card dialog-card">
          <div class="dialog-card__header">
            <span>新建播放集</span>
          </div>
          <div class="dialog-card__body">
            <div class="playlist-dialog">
              <label class="dialog-field">
                <span>名称</span>
                <input v-model="playlistName" type="text" placeholder="例如 通勤歌单 / 参考分镜" @keydown.enter.prevent="emit('submitPlaylistDialog')" />
              </label>
              <label class="dialog-field">
                <span>播放类型</span>
                <select v-model="playlistPlayerTypeId">
                  <option v-for="playerType in availablePlaylistPlayers" :key="playerType.playerTypeId" :value="playerType.playerTypeId">
                    {{ playerType.label }} · {{ playerType.fileClass }}
                  </option>
                </select>
              </label>
            </div>
          </div>
          <div class="dialog-card__actions">
            <button type="button" class="ghost" @click="emit('closePlaylistDialog')">
              取消
            </button>
            <button type="button" class="primary" :disabled="playlistDialogDisabled" @click="emit('submitPlaylistDialog')">
              创建
            </button>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>
