<script setup lang="ts">
import { convertFileSrc } from "@tauri-apps/api/core";
import { computed, nextTick, onBeforeUnmount, onMounted, ref, useTemplateRef, watch } from "vue";
import { AudioLines, FileAudio, FileImage, FileVideo } from "lucide-vue-next";
import { ensureThumbnail, preparePreviewFileSource, readFile } from "../../services/repositoryApi";
import type { FileBrowserEntry } from "../../types/repository";
import { isImageExtension, isVideoExtension } from "./mediaExtensions";

type ParsedLyricLine = {
  id: string;
  text: string;
  timeMs: number | null;
};

const props = defineProps<{
  entry: FileBrowserEntry;
  repoId: string;
}>();

const state = ref<"idle" | "loading" | "ready" | "error">("idle");
const sourceUrl = ref("");
const sourceMediaType = ref("");
const errorMessage = ref("");
const audioArtworkPath = ref<string | null>(null);
const lyricsStatus = ref<"idle" | "loading" | "ready" | "empty">("idle");
const lyricsLines = ref<ParsedLyricLine[]>([]);
const currentPlaybackMs = ref(0);
const activeLyricIndex = ref(-1);
const lyricsInset = ref(104);
const audioElement = useTemplateRef<HTMLAudioElement>("audioElement");
const lyricsViewport = useTemplateRef<HTMLDivElement>("lyricsViewport");
const lyricsItems = ref<HTMLElement[]>([]);
let resizeObserver: ResizeObserver | null = null;
let loadToken = 0;
let audioToken = 0;

const mediaKind = computed<"image" | "video" | "audio">(() => (
  isImageExtension(props.entry.extension) ? "image" : isVideoExtension(props.entry.extension) ? "video" : "audio"
));
const extensionLabel = computed(() => (
  props.entry.extension?.toUpperCase() || (mediaKind.value === "image" ? "IMAGE" : mediaKind.value === "video" ? "VIDEO" : "AUDIO")
));
const audioArtworkUrl = computed(() => (
  audioArtworkPath.value ? convertFileSrc(audioArtworkPath.value) : null
));
const lyricsPlaceholder = computed(() => (
  lyricsStatus.value === "loading" ? "读取歌词..." : "暂无歌词"
));
const hasTimedLyrics = computed(() => lyricsLines.value.some((line) => line.timeMs != null));

watch(
  [() => props.repoId, () => props.entry.path, () => props.entry.extension],
  () => {
    void loadMediaSource();
    void loadAudioCompanions();
  },
  { immediate: true },
);

watch(
  () => props.entry.thumbnailPath,
  (value) => {
    if (mediaKind.value !== "audio") return;
    audioArtworkPath.value = value ?? audioArtworkPath.value;
  },
);

watch(
  [lyricsLines, currentPlaybackMs],
  async () => {
    if (!hasTimedLyrics.value) {
      activeLyricIndex.value = -1;
      return;
    }
    syncLyricsInset();
    activeLyricIndex.value = findActiveLyricIndex(lyricsLines.value, currentPlaybackMs.value);
    await nextTick();
    centerActiveLyric();
  },
  { deep: true },
);

watch(lyricsViewport, (element) => {
  resizeObserver?.disconnect();
  resizeObserver = null;
  if (!element || typeof ResizeObserver === "undefined") return;

  resizeObserver = new ResizeObserver(() => {
    syncLyricsInset();
    centerActiveLyric();
  });
  resizeObserver.observe(element);
});

onMounted(() => {
  syncLyricsInset();
  window.addEventListener("resize", handleWindowResize);
});

onBeforeUnmount(() => {
  window.removeEventListener("resize", handleWindowResize);
  resizeObserver?.disconnect();
  resizeObserver = null;
});

async function loadMediaSource() {
  const token = ++loadToken;
  sourceUrl.value = "";
  sourceMediaType.value = "";
  errorMessage.value = "";
  state.value = "loading";
  currentPlaybackMs.value = 0;

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

async function loadAudioCompanions() {
  const token = ++audioToken;
  audioArtworkPath.value = props.entry.thumbnailPath ?? null;
  lyricsLines.value = [];
  lyricsStatus.value = mediaKind.value === "audio" ? "loading" : "idle";
  lyricsItems.value = [];
  activeLyricIndex.value = -1;

  if (mediaKind.value !== "audio") {
    return;
  }

  await Promise.all([
    loadAudioArtwork(token),
    loadAudioLyrics(token),
  ]);

  await nextTick();
  syncLyricsInset();
}

async function loadAudioArtwork(token: number) {
  if (props.entry.thumbnailPath) {
    audioArtworkPath.value = props.entry.thumbnailPath;
    return;
  }

  try {
    const response = await ensureThumbnail({
      repoId: props.repoId,
      path: props.entry.path,
      action: "ensure",
    });
    if (token !== audioToken) return;
    audioArtworkPath.value = response.thumbnailPath ?? null;
  } catch {
    if (token !== audioToken) return;
    audioArtworkPath.value = null;
  }
}

async function loadAudioLyrics(token: number) {
  try {
    const bytes = await readFile({
      repoId: props.repoId,
      path: siblingLrcPath(props.entry.path),
    });
    if (token !== audioToken) return;
    const parsed = parseLrcLyrics(decodeTextBytes(Uint8Array.from(bytes)));
    lyricsLines.value = parsed;
    lyricsStatus.value = parsed.length > 0 ? "ready" : "empty";
  } catch {
    if (token !== audioToken) return;
    lyricsLines.value = [];
    lyricsStatus.value = "empty";
  }
}

function handleMediaError() {
  state.value = "error";
  errorMessage.value = `${mediaKind.value === "image" ? "图片" : mediaKind.value === "video" ? "视频" : "音频"}无法播放`;
}

function handleAudioTimeUpdate(event: Event) {
  const target = event.target as HTMLAudioElement | null;
  currentPlaybackMs.value = Math.round((target?.currentTime ?? 0) * 1000);
}

function handleAudioSeeking(event: Event) {
  handleAudioTimeUpdate(event);
}

function handleAudioPlay() {
  centerActiveLyric();
}

function handleAudioEnded() {
  currentPlaybackMs.value = 0;
}

function siblingLrcPath(path: string) {
  const extensionIndex = path.lastIndexOf(".");
  return extensionIndex >= 0 ? `${path.slice(0, extensionIndex)}.lrc` : `${path}.lrc`;
}

function decodeTextBytes(bytes: Uint8Array) {
  if (bytes.length >= 3 && bytes[0] === 0xef && bytes[1] === 0xbb && bytes[2] === 0xbf) {
    return new TextDecoder("utf-8").decode(bytes.slice(3));
  }
  if (bytes.length >= 2 && bytes[0] === 0xff && bytes[1] === 0xfe) {
    return new TextDecoder("utf-16le").decode(bytes.slice(2));
  }
  if (bytes.length >= 2 && bytes[0] === 0xfe && bytes[1] === 0xff) {
    return new TextDecoder("utf-16be").decode(bytes.slice(2));
  }
  return new TextDecoder("utf-8").decode(bytes);
}

function parseLrcLyrics(text: string) {
  const normalized = text.replace(/\r\n?/g, "\n");
  const rawLines = normalized
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean);

  const parsed: ParsedLyricLine[] = [];

  for (const rawLine of rawLines) {
    const timeTags = [...rawLine.matchAll(/\[(\d{1,2}):(\d{1,2})(?:[.:](\d{1,3}))?\]/g)];
    const plainText = rawLine
      .replace(/\[(\d{1,2}):(\d{1,2})(?:[.:](\d{1,3}))?\]/g, "")
      .trim();

    if (timeTags.length > 0) {
      const textValue = plainText || "…";
      for (const [index, tag] of timeTags.entries()) {
        parsed.push({
          id: `${tag[0]}-${parsed.length}-${index}`,
          text: textValue,
          timeMs: timestampToMs(tag[1], tag[2], tag[3]),
        });
      }
      continue;
    }

    if (/^\[[^:\]]+:[^\]]*\]$/.test(rawLine)) {
      continue;
    }

    if (plainText) {
      parsed.push({
        id: `plain-${parsed.length}`,
        text: plainText,
        timeMs: null,
      });
    }
  }

  return parsed.sort((left, right) => {
    if (left.timeMs == null && right.timeMs == null) return 0;
    if (left.timeMs == null) return 1;
    if (right.timeMs == null) return -1;
    return left.timeMs - right.timeMs;
  });
}

function timestampToMs(minutes: string, seconds: string, fraction?: string) {
  const minuteValue = Number.parseInt(minutes, 10);
  const secondValue = Number.parseInt(seconds, 10);
  const fractionValue = fraction
    ? Number.parseInt(fraction.padEnd(3, "0").slice(0, 3), 10)
    : 0;
  return (minuteValue * 60 * 1000) + (secondValue * 1000) + fractionValue;
}

function findActiveLyricIndex(lines: ParsedLyricLine[], playbackMs: number) {
  let index = -1;
  for (let cursor = 0; cursor < lines.length; cursor += 1) {
    const line = lines[cursor];
    if (line.timeMs == null || line.timeMs > playbackMs) {
      continue;
    }
    index = cursor;
  }
  return index;
}

function centerActiveLyric() {
  const viewport = lyricsViewport.value;
  if (!viewport || activeLyricIndex.value < 0) return;
  const item = lyricsItems.value[activeLyricIndex.value];
  if (!item) return;

  const top = item.offsetTop - (viewport.clientHeight / 2) + (item.clientHeight / 2);
  const nextTop = Math.max(0, top);
  if (typeof viewport.scrollTo === "function") {
    viewport.scrollTo({
      top: nextTop,
      behavior: "smooth",
    });
    return;
  }
  viewport.scrollTop = nextTop;
}

function syncLyricsInset() {
  const viewport = lyricsViewport.value;
  if (!viewport) return;
  lyricsInset.value = Math.max(72, Math.floor(viewport.clientHeight / 2) - 32);
}

function setLyricItemRef(index: number, element: Element | null) {
  if (!(element instanceof HTMLElement)) return;
  lyricsItems.value[index] = element;
}

function handleWindowResize() {
  syncLyricsInset();
  centerActiveLyric();
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

    <template v-else-if="sourceUrl && mediaKind === 'image'">
      <img class="media-preview__image" :src="sourceUrl" alt="" @error="handleMediaError" />
      <div class="media-preview__hud">
        <span>{{ extensionLabel }}</span>
      </div>
    </template>

    <template v-else-if="sourceUrl && mediaKind === 'video'">
      <video class="media-preview__video" controls preload="metadata" playsinline @error="handleMediaError">
        <source :src="sourceUrl" :type="sourceMediaType" />
      </video>
      <div class="media-preview__hud">
        <span>{{ extensionLabel }}</span>
      </div>
    </template>

    <div v-else-if="sourceUrl" class="media-preview__audio">
      <div class="media-preview__audio-main">
        <div class="media-preview__audio-art" aria-hidden="true">
          <img v-if="audioArtworkUrl" class="media-preview__audio-cover" :src="audioArtworkUrl" alt="" />
          <AudioLines v-else :size="42" />
        </div>
        <section class="media-preview__audio-panel" aria-label="歌词面板">
          <header class="media-preview__audio-panel-head">
            <div class="media-preview__audio-title">
              <FileAudio :size="18" aria-hidden="true" />
              <strong>{{ entry.name }}</strong>
            </div>
            <span class="media-preview__audio-chip">{{ extensionLabel }}</span>
          </header>
          <div
            ref="lyricsViewport"
            class="media-preview__audio-lyrics"
            :class="{ 'media-preview__audio-lyrics--empty': !lyricsLines.length }"
          >
            <div
              v-if="lyricsLines.length"
              class="media-preview__audio-lyrics-track"
              :style="{ '--lyrics-inset': `${lyricsInset}px` }"
            >
              <button
                v-for="(line, index) in lyricsLines"
                :key="line.id"
                type="button"
                class="media-preview__audio-lyric"
                :class="{
                  'is-active': index === activeLyricIndex,
                  'is-passed': activeLyricIndex > index,
                  'is-timed': line.timeMs != null,
                }"
                :disabled="line.timeMs == null"
                @click="line.timeMs != null && audioElement && (audioElement.currentTime = line.timeMs / 1000)"
                :ref="(element) => setLyricItemRef(index, element)"
              >
                {{ line.text }}
              </button>
            </div>
            <span v-else>{{ lyricsPlaceholder }}</span>
          </div>
        </section>
      </div>
      <audio
        ref="audioElement"
        class="media-preview__audio-control"
        controls
        preload="metadata"
        @error="handleMediaError"
        @timeupdate="handleAudioTimeUpdate"
        @seeking="handleAudioSeeking"
        @play="handleAudioPlay"
        @ended="handleAudioEnded"
      >
        <source :src="sourceUrl" :type="sourceMediaType" />
      </audio>
    </div>

    <FileImage v-else-if="isImageExtension(entry.extension)" :size="54" aria-hidden="true" />
    <FileVideo v-else-if="isVideoExtension(entry.extension)" :size="54" aria-hidden="true" />
    <FileAudio v-else :size="54" aria-hidden="true" />
  </div>
</template>
