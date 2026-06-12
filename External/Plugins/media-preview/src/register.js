import {
  audioPreviewExtensions,
  imagePreviewExtensions,
  isImageExtension,
  isVideoExtension,
  videoPreviewExtensions,
} from "./mediaExtensions.js";

export function register(ctx) {
  const {
    computed,
    h,
    nextTick,
    onBeforeUnmount,
    onMounted,
    ref,
    watch,
  } = ctx.vue;

  const MediaPreviewPlugin = {
    name: "MediaPreviewPlugin",
    props: {
      entry: {
        type: Object,
        default: null,
      },
      repoId: {
        type: String,
        default: "",
      },
      saveMetadata: {
        type: Function,
        default: null,
      },
    },
    setup(props) {
      const state = ref("idle");
      const sourceUrl = ref("");
      const sourceMediaType = ref("");
      const errorMessage = ref("");
      const audioArtworkPath = ref(props.entry?.thumbnailPath ?? null);
      const lyricsStatus = ref("idle");
      const lyricsLines = ref([]);
      const currentPlaybackMs = ref(0);
      const activeLyricIndex = ref(-1);
      const lyricsInset = ref(104);
      const lyricsViewport = ref(null);
      const lyricsItems = ref([]);
      let objectUrl = null;
      let resizeObserver = null;
      let lastProgressSaveAt = 0;
      let lastProgressSaveSecond = -1;

      const mediaKind = computed(() => (
        isImageExtension(props.entry?.extension) ? "image" : isVideoExtension(props.entry?.extension) ? "video" : "audio"
      ));
      const extensionLabel = computed(() => (
        props.entry?.extension?.toUpperCase() || (mediaKind.value === "image" ? "IMAGE" : mediaKind.value === "video" ? "VIDEO" : "AUDIO")
      ));
      const audioArtworkUrl = computed(() => (
        audioArtworkPath.value ? ctx.fileSrc(audioArtworkPath.value) : null
      ));
      const lyricsPlaceholder = computed(() => (
        lyricsStatus.value === "loading" ? "读取歌词..." : "暂无歌词"
      ));

      function revokeObjectUrl() {
        if (!objectUrl) return;
        URL.revokeObjectURL(objectUrl);
        objectUrl = null;
      }

      async function loadMediaSource() {
        state.value = "loading";
        sourceUrl.value = "";
        sourceMediaType.value = "";
        errorMessage.value = "";
        revokeObjectUrl();

        try {
          const response = await ctx.preparePreviewFileSource({
            repoId: props.repoId,
            path: props.entry.path,
          });
          if (!response.sourceUrl) {
            throw new Error("媒体预览源不可用");
          }
          sourceUrl.value = response.sourceUrl;
          sourceMediaType.value = response.mediaType;
          state.value = "ready";
        } catch (cause) {
          state.value = "error";
          errorMessage.value = cause instanceof Error ? cause.message : String(cause);
        }
      }

      async function loadAudioLyrics() {
        if (mediaKind.value !== "audio") {
          lyricsStatus.value = "idle";
          lyricsLines.value = [];
          return;
        }
        lyricsStatus.value = "loading";
        try {
          const bytes = await ctx.readFile({
            repoId: props.repoId,
            path: siblingLrcPath(props.entry.path),
          });
          const parsed = parseLrcLyrics(decodeTextBytes(Uint8Array.from(bytes)));
          lyricsLines.value = parsed;
          lyricsStatus.value = parsed.length ? "ready" : "empty";
          await nextTick();
          syncLyricsInset();
        } catch {
          lyricsLines.value = [];
          lyricsStatus.value = "empty";
        }
      }

      function syncLyricsInset() {
        const viewport = lyricsViewport.value;
        if (!viewport) return;
        lyricsInset.value = Math.max(72, Math.floor(viewport.clientHeight / 2) - 32);
      }

      function centerActiveLyric() {
        const viewport = lyricsViewport.value;
        if (!viewport || activeLyricIndex.value < 0) return;
        const item = lyricsItems.value[activeLyricIndex.value];
        if (!item) return;
        const top = item.offsetTop - (viewport.clientHeight / 2) + (item.clientHeight / 2);
        viewport.scrollTop = Math.max(0, top);
      }

      watch(
        () => [props.repoId, props.entry?.path, props.entry?.extension],
        async () => {
          audioArtworkPath.value = props.entry?.thumbnailPath ?? null;
          await loadMediaSource();
          await loadAudioLyrics();
        },
        { immediate: true },
      );

      watch(currentPlaybackMs, () => {
        activeLyricIndex.value = findActiveLyricIndex(lyricsLines.value, currentPlaybackMs.value);
        centerActiveLyric();
      });

      onMounted(() => {
        if (typeof ResizeObserver !== "undefined" && lyricsViewport.value) {
          resizeObserver = new ResizeObserver(() => {
            syncLyricsInset();
            centerActiveLyric();
          });
          resizeObserver.observe(lyricsViewport.value);
        }
      });

      onBeforeUnmount(() => {
        resizeObserver?.disconnect();
        revokeObjectUrl();
      });

      function handleMediaError() {
        state.value = "error";
        errorMessage.value = "媒体无法播放";
      }

      function isAsmrAudioEntry() {
        const metadata = props.entry?.metadata ?? {};
        return mediaKind.value === "audio" && (
          metadata.libraryKind === "asmr" ||
          Boolean(metadata.workId) ||
          Boolean(metadata.rjCode)
        );
      }

      function currentTrackDurationMs(target) {
        const duration = Number(target?.duration);
        return Number.isFinite(duration) && duration > 0 ? Math.round(duration * 1000) : 0;
      }

      function buildProgressMetadata(target, statusOverride = null) {
        const durationMs = currentTrackDurationMs(target);
        const currentMs = Math.max(0, Math.round((Number(target?.currentTime) || 0) * 1000));
        const progress = durationMs > 0 ? Math.min(100, Math.max(0, Math.round((currentMs / durationMs) * 100))) : 0;
        const finished = statusOverride === "listened" || (durationMs > 0 && progress >= 95);
        return {
          listeningProgress: finished ? 100 : progress,
          listeningStatus: finished ? "listened" : "listening",
          lastListenedAt: new Date().toISOString(),
          trackDurationMs: durationMs,
          trackPositionMs: finished ? durationMs : currentMs,
        };
      }

      function persistAsmrDuration(target) {
        if (!props.saveMetadata || !props.entry || !isAsmrAudioEntry()) return;
        const durationMs = currentTrackDurationMs(target);
        if (durationMs <= 0 || props.entry.metadata?.trackDurationMs === durationMs) return;
        void props.saveMetadata(props.entry, { trackDurationMs: durationMs });
      }

      function persistAsmrProgress(target, options = {}) {
        if (!props.saveMetadata || !props.entry || !isAsmrAudioEntry()) return;
        const metadata = buildProgressMetadata(target, options.status);
        const currentSecond = Math.floor((Number(target?.currentTime) || 0));
        const now = Date.now();
        const shouldSave = options.force ||
          metadata.listeningStatus === "listened" ||
          (metadata.trackDurationMs > 0 &&
            metadata.trackPositionMs > 0 &&
            now - lastProgressSaveAt >= 15000 &&
            Math.abs(currentSecond - lastProgressSaveSecond) >= 5);
        if (!shouldSave || (metadata.listeningStatus !== "listened" && metadata.trackPositionMs <= 0)) return;
        lastProgressSaveAt = now;
        lastProgressSaveSecond = currentSecond;
        void props.saveMetadata(props.entry, metadata);
      }

      function setLyricItemRef(index, element) {
        if (!element) return;
        lyricsItems.value[index] = element;
      }

      return {
        activeLyricIndex,
        audioArtworkUrl,
        currentPlaybackMs,
        entry: props.entry,
        extensionLabel,
        handleMediaError,
        lyricsInset,
        lyricsLines,
        lyricsPlaceholder,
        lyricsStatus,
        lyricsViewport,
        mediaKind,
        setLyricItemRef,
        sourceMediaType,
        sourceUrl,
        state,
        errorMessage,
        onAudioEnded(event) {
          currentPlaybackMs.value = Math.round(((event.target?.currentTime ?? 0) * 1000));
          persistAsmrProgress(event.target, { force: true, status: "listened" });
        },
        onAudioLoadedMetadata(event) {
          persistAsmrDuration(event.target);
        },
        onAudioPause(event) {
          persistAsmrProgress(event.target, { force: true });
        },
        onAudioTimeUpdate(event) {
          currentPlaybackMs.value = Math.round(((event.target?.currentTime ?? 0) * 1000));
          persistAsmrProgress(event.target);
        },
      };
    },
    render() {
      if (this.state === "loading") {
        return h("div", { class: "media-preview__status" }, [
          h("span", "读取媒体"),
          h("span", this.entry?.sizeLabel ? `准备 ${this.entry.sizeLabel}` : "建立预览流"),
        ]);
      }
      if (this.state === "error") {
        return h("div", { class: "media-preview__overlay media-preview__overlay--error" }, [
          h("strong", "无法预览该媒体"),
          h("span", this.errorMessage),
        ]);
      }
      if (this.sourceUrl && this.mediaKind === "image") {
        return h("div", { class: "media-preview media-preview--image" }, [
          h("img", {
            class: "media-preview__image",
            src: this.sourceUrl,
            onError: this.handleMediaError,
          }),
        ]);
      }
      if (this.sourceUrl && this.mediaKind === "video") {
        return h("div", { class: "media-preview media-preview--video" }, [
          h("video", {
            class: "media-preview__video",
            controls: true,
            preload: "metadata",
            playsinline: true,
            onError: this.handleMediaError,
          }, [
            h("source", { src: this.sourceUrl, type: this.sourceMediaType }),
          ]),
        ]);
      }
      return h("div", { class: "media-preview media-preview--audio" }, [
        h("div", { class: "media-preview__audio-main" }, [
          h("div", { class: "media-preview__audio-art", "aria-hidden": "true" }, [
            this.audioArtworkUrl
              ? h("img", { class: "media-preview__audio-cover", src: this.audioArtworkUrl, alt: "" })
              : h("span", { class: "media-preview__audio-chip" }, this.extensionLabel),
          ]),
          h("section", { class: "media-preview__audio-panel", "aria-label": "歌词面板" }, [
            h("header", { class: "media-preview__audio-panel-head" }, [
              h("strong", this.entry?.name ?? ""),
              h("span", { class: "media-preview__audio-chip" }, this.extensionLabel),
            ]),
            h("div", {
              ref: "lyricsViewport",
              class: ["media-preview__audio-lyrics", { "media-preview__audio-lyrics--empty": !this.lyricsLines.length }],
            }, this.lyricsLines.length
              ? [
                  h("div", {
                    class: "media-preview__audio-lyrics-track",
                    style: { "--lyrics-inset": `${this.lyricsInset}px` },
                  }, this.lyricsLines.map((line, index) => h("button", {
                    key: line.id,
                    type: "button",
                    class: [
                      "media-preview__audio-lyric",
                      {
                        "is-active": index === this.activeLyricIndex,
                        "is-passed": this.activeLyricIndex > index,
                        "is-timed": line.timeMs != null,
                      },
                    ],
                    disabled: line.timeMs == null,
                    ref: (element) => this.setLyricItemRef(index, element),
                  }, line.text))),
                ]
              : [h("span", this.lyricsPlaceholder)]),
          ]),
        ]),
        h("audio", {
          class: "media-preview__audio-control",
          controls: true,
          preload: "metadata",
          onError: this.handleMediaError,
          onEnded: this.onAudioEnded,
          onLoadedmetadata: this.onAudioLoadedMetadata,
          onPause: this.onAudioPause,
          onTimeupdate: this.onAudioTimeUpdate,
        }, [
          h("source", { src: this.sourceUrl, type: this.sourceMediaType }),
        ]),
      ]);
    },
  };

  ctx.registerPreview({
    supportedExtensions: [...imagePreviewExtensions, ...videoPreviewExtensions, ...audioPreviewExtensions],
    component: MediaPreviewPlugin,
  });
}

function siblingLrcPath(path) {
  const extensionIndex = path.lastIndexOf(".");
  return extensionIndex >= 0 ? `${path.slice(0, extensionIndex)}.lrc` : `${path}.lrc`;
}

function decodeTextBytes(bytes) {
  if (bytes.length >= 3 && bytes[0] === 0xef && bytes[1] === 0xbb && bytes[2] === 0xbf) {
    return new TextDecoder("utf-8").decode(bytes.slice(3));
  }
  return new TextDecoder("utf-8").decode(bytes);
}

function parseLrcLyrics(text) {
  const normalized = text.replace(/\r\n?/g, "\n");
  const rawLines = normalized
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean);

  const parsed = [];
  for (const rawLine of rawLines) {
    const timeTags = [...rawLine.matchAll(/\[(\d{1,2}):(\d{1,2})(?:[.:](\d{1,3}))?\]/g)];
    const plainText = rawLine.replace(/\[(\d{1,2}):(\d{1,2})(?:[.:](\d{1,3}))?\]/g, "").trim();
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

function timestampToMs(minutes, seconds, fraction) {
  const minuteValue = Number.parseInt(minutes, 10);
  const secondValue = Number.parseInt(seconds, 10);
  const fractionValue = fraction ? Number.parseInt(fraction.padEnd(3, "0").slice(0, 3), 10) : 0;
  return (minuteValue * 60 * 1000) + (secondValue * 1000) + fractionValue;
}

function findActiveLyricIndex(lines, playbackMs) {
  let index = -1;
  for (let cursor = 0; cursor < lines.length; cursor += 1) {
    const line = lines[cursor];
    if (line.timeMs == null || line.timeMs > playbackMs) continue;
    index = cursor;
  }
  return index;
}
