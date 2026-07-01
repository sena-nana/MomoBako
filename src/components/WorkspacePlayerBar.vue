<script setup lang="ts">
import {
  ListMusic,
  Pause,
  Play,
  Repeat,
  Repeat1,
  Shuffle,
  SkipBack,
  SkipForward,
  Volume2,
} from "@lucide/vue";
import type { PlaylistPlaybackMode } from "../types/repository";
import type {
  WorkspacePlayerBarEmitMap,
  WorkspacePlayerBarProps,
} from "./workspacePlayerBar.contract";

const props = defineProps<WorkspacePlayerBarProps>();

const emit = defineEmits<WorkspacePlayerBarEmitMap>();

function formatTime(value: number) {
  const totalSeconds = Math.max(0, Math.floor(value / 1000));
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${minutes}:${String(seconds).padStart(2, "0")}`;
}

function modeLabel(mode: PlaylistPlaybackMode) {
  if (mode === "shuffle") return "随机播放";
  if (mode === "singleLoop") return "单曲循环";
  return "列表循环";
}

function progressPercent() {
  if (props.durationMs <= 0) return 0;
  return Math.min(100, Math.max(0, (props.currentTimeMs / props.durationMs) * 100));
}

function imageDurationSeconds() {
  return Math.round(props.imageDurationMs / 1000);
}

function seekFromInput(event: Event) {
  if (!props.supportsSeek) return;
  const target = event.target as HTMLInputElement | null;
  if (!target) return;
  emit("seek", Number(target.value));
}

function setImageDurationFromInput(event: Event) {
  const target = event.target as HTMLInputElement | null;
  if (!target) return;
  emit("setImageDuration", Number(target.value) * 1000);
}
</script>

<template>
  <section class="workspace-player" aria-label="播放控制器">
    <div class="workspace-player__progress">
      <span :style="{ width: `${progressPercent()}%` }"></span>
      <input
        class="workspace-player__progress-input"
        type="range"
        min="0"
        :max="Math.max(durationMs, 1)"
        :value="Math.min(currentTimeMs, Math.max(durationMs, 1))"
        :disabled="!item || durationMs <= 0 || !supportsSeek"
        @input="seekFromInput"
      />
    </div>

    <div class="workspace-player__body">
      <button type="button" class="workspace-player__media" :disabled="!item" @click="emit('openPreview')">
        <span class="workspace-player__thumb" aria-hidden="true">
          <img v-if="item?.thumbnailPath" :src="item.thumbnailPath" alt="" />
          <span v-else>{{ item?.extension?.toUpperCase() ?? "—" }}</span>
        </span>
        <span class="workspace-player__meta">
          <strong>{{ item?.filename ? `正在播放 ${item.filename}` : "未选择播放内容" }}</strong>
          <small v-if="errorMessage">{{ errorMessage }}</small>
          <small v-else>{{ playerLabel ? `${playerLabel} · ${item?.path ?? ""}` : (item?.path ?? "选择播放集后可开始播放") }}</small>
        </span>
      </button>

      <div class="workspace-player__transport">
        <button type="button" class="ui-icon-button" :title="modeLabel(mode)" :disabled="!item" @click="emit('cycleMode')">
          <Shuffle v-if="mode === 'shuffle'" :size="15" aria-hidden="true" />
          <Repeat1 v-else-if="mode === 'singleLoop'" :size="15" aria-hidden="true" />
          <Repeat v-else :size="15" aria-hidden="true" />
        </button>
        <button type="button" class="ui-icon-button" :disabled="!item || !canPlay" @click="emit('previous')">
          <SkipBack :size="16" aria-hidden="true" />
        </button>
        <button type="button" class="ui-icon-button workspace-player__play" :disabled="!item || !canPlay" @click="emit('togglePlay')">
          <Pause v-if="isPlaying" :size="17" aria-hidden="true" />
          <Play v-else :size="17" aria-hidden="true" />
        </button>
        <button type="button" class="ui-icon-button" :disabled="!item || !canPlay" @click="emit('next')">
          <SkipForward :size="16" aria-hidden="true" />
        </button>
        <button
          type="button"
          class="ui-icon-button"
          :class="{ 'is-active': queueOpen }"
          :disabled="!item"
          @click="emit('openQueue')"
        >
          <ListMusic :size="16" aria-hidden="true" />
        </button>
      </div>

      <div class="workspace-player__side">
        <span class="workspace-player__time">{{ formatTime(currentTimeMs) }} / {{ formatTime(durationMs) }}</span>
        <label class="workspace-player__volume">
          <Volume2 :size="15" aria-hidden="true" />
          <input
            type="range"
            min="0"
            max="1"
            step="0.01"
            :value="volume"
            :disabled="!item || !supportsVolume"
            @input="emit('setVolume', Number(($event.target as HTMLInputElement).value))"
          />
        </label>
      </div>
    </div>

    <div v-if="item && (fileClass === 'image' || fileClass === 'video')" class="workspace-player__settings">
      <label v-if="fileClass === 'image'" class="workspace-player__setting workspace-player__setting--duration">
        <span>停留 {{ imageDurationSeconds() }} 秒</span>
        <input
          type="range"
          aria-label="图片停留时长"
          min="2"
          max="30"
          step="1"
          :value="imageDurationSeconds()"
          @input="setImageDurationFromInput"
        />
      </label>
      <div class="workspace-player__fit" role="radiogroup" aria-label="画面适配">
        <button
          type="button"
          :class="{ 'is-active': objectFit === 'contain' }"
          role="radio"
          :aria-checked="objectFit === 'contain'"
          @click="emit('setObjectFit', 'contain')"
        >
          适应
        </button>
        <button
          type="button"
          :class="{ 'is-active': objectFit === 'cover' }"
          role="radio"
          :aria-checked="objectFit === 'cover'"
          @click="emit('setObjectFit', 'cover')"
        >
          填充
        </button>
      </div>
    </div>

    <div v-if="queueOpen" class="workspace-player__queue" role="dialog" aria-label="当前队列">
      <header class="workspace-player__queue-head">
        <strong>当前队列</strong>
        <span>{{ queueItems.length }} 项</span>
      </header>
      <div class="workspace-player__queue-list">
        <button
          v-for="queueItem in queueItems"
          :key="queueItem.playlistItemId"
          type="button"
          class="workspace-player__queue-item"
          :class="{ 'is-active': queueItem.playlistItemId === currentItemId, 'is-disabled': queueItem.status !== 'ready' }"
          :disabled="queueItem.status !== 'ready'"
          @click="emit('selectQueueItem', queueItem.playlistItemId)"
        >
          <span class="workspace-player__queue-thumb">
            <img v-if="queueItem.thumbnailPath" :src="queueItem.thumbnailPath" alt="" />
            <span v-else>{{ queueItem.extension.toUpperCase() }}</span>
          </span>
          <span class="workspace-player__queue-meta">
            <strong>{{ queueItem.filename }}</strong>
            <small v-if="queueItem.status !== 'ready'">{{ queueItem.statusReason ?? queueItem.status }}</small>
            <small v-else>{{ queueItem.path }}</small>
          </span>
        </button>
      </div>
    </div>
  </section>
</template>
