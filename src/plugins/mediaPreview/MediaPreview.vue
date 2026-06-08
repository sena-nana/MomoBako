<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { AudioLines, FileAudio, FileVideo } from "lucide-vue-next";
import { preparePreviewFileSource } from "../../services/repositoryApi";
import type { FileBrowserEntry } from "../../types/repository";
import { isVideoExtension } from "./mediaExtensions";

const props = defineProps<{
  entry: FileBrowserEntry;
  repoId: string;
}>();

const state = ref<"idle" | "loading" | "ready" | "error">("idle");
const sourceUrl = ref("");
const sourceMediaType = ref("");
const errorMessage = ref("");
let loadToken = 0;

const mediaKind = computed<"video" | "audio">(() => (
  isVideoExtension(props.entry.extension) ? "video" : "audio"
));
const extensionLabel = computed(() => props.entry.extension?.toUpperCase() || (mediaKind.value === "video" ? "VIDEO" : "AUDIO"));

watch(
  [() => props.repoId, () => props.entry.path],
  () => {
    void loadMediaSource();
  },
  { immediate: true },
);

async function loadMediaSource() {
  const token = ++loadToken;
  sourceUrl.value = "";
  sourceMediaType.value = "";
  errorMessage.value = "";
  state.value = "loading";

  try {
    const response = await preparePreviewFileSource({
      repoId: props.repoId,
      path: props.entry.path,
    });
    if (token !== loadToken) return;
    if (!response.sourceUrl) {
      throw new Error("媒体预览源不可用");
    }
    sourceUrl.value = response.sourceUrl;
    sourceMediaType.value = response.mediaType;
    state.value = "ready";
  } catch (cause) {
    if (token !== loadToken) return;
    state.value = "error";
    errorMessage.value = cause instanceof Error ? cause.message : String(cause);
  }
}

function handleMediaError() {
  state.value = "error";
  errorMessage.value = `${mediaKind.value === "video" ? "视频" : "音频"}无法播放`;
}
</script>

<template>
  <div class="media-preview" :class="`media-preview--${mediaKind}`">
    <div v-if="state === 'loading'" class="media-preview__status">
      <span>读取媒体</span>
      <span>{{ entry.sizeLabel ? `准备 ${entry.sizeLabel}` : "建立预览流" }}</span>
    </div>

    <div v-else-if="state === 'error'" class="media-preview__overlay media-preview__overlay--error">
      <strong>无法预览该媒体</strong>
      <span>{{ errorMessage }}</span>
    </div>

    <template v-else-if="sourceUrl && mediaKind === 'video'">
      <video class="media-preview__video" controls preload="metadata" playsinline @error="handleMediaError">
        <source :src="sourceUrl" :type="sourceMediaType" />
      </video>
      <div class="media-preview__hud">
        <span>{{ extensionLabel }}</span>
      </div>
    </template>

    <div v-else-if="sourceUrl" class="media-preview__audio">
      <div class="media-preview__audio-art" aria-hidden="true">
        <AudioLines :size="42" />
      </div>
      <div class="media-preview__audio-body">
        <div class="media-preview__audio-title">
          <FileAudio :size="18" aria-hidden="true" />
          <strong>{{ entry.name }}</strong>
        </div>
        <audio class="media-preview__audio-control" controls preload="metadata" @error="handleMediaError">
          <source :src="sourceUrl" :type="sourceMediaType" />
        </audio>
      </div>
      <div class="media-preview__hud">
        <span>{{ extensionLabel }}</span>
      </div>
    </div>

    <FileVideo v-else-if="isVideoExtension(entry.extension)" :size="54" aria-hidden="true" />
    <FileAudio v-else :size="54" aria-hidden="true" />
  </div>
</template>
