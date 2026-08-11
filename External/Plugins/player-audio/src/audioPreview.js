/** 通用音频预览组件，负责播放源、封面、歌词与媒体状态事件。 */
import { audioDisplayMetadata, resolveArtworkUrl } from "./audioMetadata.js";
import {
  decodeTextBytes,
  findActiveLyricIndex,
  parseLrcLyrics,
  readLocalTextFile,
  siblingLrcPath,
} from "./lyrics.js";
import { errorText, prepareAudioSource } from "./playbackSource.js";

export function createAudioPreviewComponent(ctx) {
  const { computed, h, nextTick, onBeforeUnmount, onMounted, ref, watch } = ctx.vue;
  return {
    name: "AudioPreviewPlugin",
    props: {
      entry: { type: Object, default: null },
      repoId: { type: String, default: "" },
      saveMetadata: { type: Function, default: null },
    },
    setup(props) {
      const state = ref("idle");
      const sourceUrl = ref("");
      const sourceMediaType = ref("");
      const errorMessage = ref("");
      const preparedPlayback = ref(null);
      const playbackProgress = ref(initialProgress());
      const lyricsStatus = ref("idle");
      const lyricsLines = ref([]);
      const currentPlaybackMs = ref(0);
      const activeLyricIndex = ref(-1);
      const lyricsInset = ref(104);
      const lyricsViewport = ref(null);
      const lyricsItems = ref([]);
      let resizeObserver = null;

      const displayMetadata = computed(() => audioDisplayMetadata(props.entry));
      const artworkUrl = computed(() => resolveArtworkUrl(
        props.entry?.thumbnailPath || displayMetadata.value.coverArt,
        ctx.fileSrc,
      ));
      const lyricsPlaceholder = computed(() => (lyricsStatus.value === "loading" ? "读取歌词..." : "暂无歌词"));

      async function loadSource() {
        state.value = "loading";
        sourceUrl.value = "";
        sourceMediaType.value = "";
        errorMessage.value = "";
        preparedPlayback.value = null;
        playbackProgress.value = initialProgress();
        try {
          const response = await prepareAudioSource(ctx, props.repoId, props.entry.path, (event) => {
            playbackProgress.value = {
              value: event.value ?? playbackProgress.value.value,
              detail: event.detail || playbackProgress.value.detail,
              indeterminate: Boolean(event.indeterminate),
              cached: event.cached ?? playbackProgress.value.cached,
            };
          });
          preparedPlayback.value = response;
          sourceUrl.value = response.sourceUrl;
          sourceMediaType.value = response.mediaType ?? "";
          playbackProgress.value = { value: 100, detail: "播放源已就绪", indeterminate: false, cached: response.cached ?? null };
          state.value = "ready";
        } catch (cause) {
          state.value = "error";
          errorMessage.value = errorText(cause, "音频播放源不可用");
          void ctx.logger.error("音频预览源准备失败。", {
            action: "audioPreview.prepareSource",
            repoId: props.repoId,
            context: { path: props.entry?.path, message: errorMessage.value },
          });
        }
      }

      async function loadLyrics() {
        lyricsStatus.value = "loading";
        try {
          let text;
          if (preparedPlayback.value?.lyricSourceUrl || preparedPlayback.value?.lyricPath) {
            text = await readLocalTextFile(
              preparedPlayback.value.lyricSourceUrl || ctx.fileSrc(preparedPlayback.value.lyricPath),
            );
          } else {
            const bytes = await ctx.readFile({ repoId: props.repoId, path: siblingLrcPath(props.entry.path) });
            text = decodeTextBytes(Uint8Array.from(bytes));
          }
          lyricsLines.value = parseLrcLyrics(text);
          lyricsStatus.value = lyricsLines.value.length ? "ready" : "empty";
          await nextTick();
          syncLyricsInset();
        } catch (cause) {
          lyricsLines.value = [];
          lyricsStatus.value = "empty";
          void ctx.logger.debug("当前音频没有可用歌词。", {
            action: "audioPreview.loadLyrics",
            repoId: props.repoId,
            context: { path: props.entry?.path, message: errorText(cause, "歌词不可用") },
          });
        }
      }

      function syncLyricsInset() {
        if (lyricsViewport.value) lyricsInset.value = Math.max(96, Math.floor(lyricsViewport.value.clientHeight / 2));
      }
      function centerActiveLyric() {
        const viewport = lyricsViewport.value;
        const item = lyricsItems.value[activeLyricIndex.value];
        if (!viewport || !item) return;
        viewport.scrollTop = Math.max(0, item.offsetTop - (viewport.clientHeight / 2) + (item.clientHeight / 2));
      }
      function emitPlaybackState(event, playbackState) {
        const target = event.target;
        const duration = Number(target?.duration);
        const currentTime = Number(target?.currentTime);
        ctx.emitPluginEvent("media.playback", {
          repoId: props.repoId,
          entry: props.entry,
          state: playbackState,
          currentTimeMs: Number.isFinite(currentTime) && currentTime > 0 ? Math.round(currentTime * 1000) : 0,
          durationMs: Number.isFinite(duration) && duration > 0 ? Math.round(duration * 1000) : 0,
          saveMetadata: props.saveMetadata,
        });
      }

      watch(
        [() => props.repoId, () => props.entry?.path, () => props.entry?.extension],
        async () => { await loadSource(); await loadLyrics(); },
        { immediate: true },
      );
      watch(currentPlaybackMs, () => {
        activeLyricIndex.value = findActiveLyricIndex(lyricsLines.value, currentPlaybackMs.value);
        centerActiveLyric();
      });
      onMounted(() => {
        if (typeof ResizeObserver === "undefined" || !lyricsViewport.value) return;
        resizeObserver = new ResizeObserver(() => { syncLyricsInset(); centerActiveLyric(); });
        resizeObserver.observe(lyricsViewport.value);
      });
      onBeforeUnmount(() => resizeObserver?.disconnect());

      return {
        activeLyricIndex,
        artworkUrl,
        currentPlaybackMs,
        displayMetadata,
        errorMessage,
        lyricsInset,
        lyricsLines,
        lyricsPlaceholder,
        lyricsViewport,
        playbackProgress,
        sourceMediaType,
        sourceUrl,
        state,
        handleMediaError() {
          state.value = "error";
          errorMessage.value = "音频无法播放";
          void ctx.logger.warn("音频元素播放失败。", {
            action: "audioPreview.mediaError",
            repoId: props.repoId,
            context: { path: props.entry?.path },
          });
        },
        setLyricItemRef(index, element) {
          if (element) lyricsItems.value[index] = element;
        },
        onAudioEnded(event) {
          currentPlaybackMs.value = Math.round((event.target?.currentTime ?? 0) * 1000);
          emitPlaybackState(event, "ended");
        },
        onAudioLoadedMetadata(event) { emitPlaybackState(event, "metadata"); },
        onAudioPause(event) { emitPlaybackState(event, "pause"); },
        onAudioTimeUpdate(event) {
          currentPlaybackMs.value = Math.round((event.target?.currentTime ?? 0) * 1000);
          emitPlaybackState(event, "timeupdate");
        },
      };
    },
    render() {
      if (this.state === "loading") return renderLoading(h, this.playbackProgress);
      if (this.state === "error") {
        return h("div", { class: "media-preview__overlay media-preview__overlay--error" }, [
          h("strong", "无法预览该音频"),
          h("span", this.errorMessage),
        ]);
      }
      return h("div", { class: "media-preview media-preview--audio" }, [
        h("div", { class: "media-preview__audio-layout" }, [
          h("section", { class: "media-preview__audio-stage", "aria-label": "音频封面" }, [
            h("div", { class: "media-preview__audio-record" }, [
              h("div", { class: "media-preview__audio-art", "aria-hidden": "true" }, [
                this.artworkUrl
                  ? h("img", { class: "media-preview__audio-cover", src: this.artworkUrl, alt: "" })
                  : h("span", { class: "media-preview__audio-chip" }, "音频"),
              ]),
            ]),
            h("div", { class: "media-preview__audio-caption" }, [
              h("h2", this.displayMetadata.title),
              h("p", this.displayMetadata.artist || this.displayMetadata.album || "通用音频"),
              h("div", { class: "media-preview__audio-meta" }, [
                h("span", "音频"),
                this.displayMetadata.album ? h("span", this.displayMetadata.album) : null,
              ]),
            ]),
          ]),
          h("section", { class: "media-preview__audio-info", "aria-label": "歌词" }, [
            h("section", { class: "media-preview__audio-panel", "aria-label": "歌词面板" }, [
              h("div", {
                ref: "lyricsViewport",
                class: ["media-preview__audio-lyrics", { "media-preview__audio-lyrics--empty": !this.lyricsLines.length }],
              }, this.lyricsLines.length
                ? [h("div", {
                    class: "media-preview__audio-lyrics-track",
                    style: { "--lyrics-inset": `${this.lyricsInset}px` },
                  }, this.lyricsLines.map((line, index) => h("button", {
                    key: line.id,
                    type: "button",
                    class: ["media-preview__audio-lyric", {
                      "is-active": index === this.activeLyricIndex,
                      "is-passed": this.activeLyricIndex > index,
                      "is-timed": line.timeMs != null,
                    }],
                    disabled: line.timeMs == null,
                    ref: (element) => this.setLyricItemRef(index, element),
                  }, line.text)))]
                : [h("span", this.lyricsPlaceholder)]),
            ]),
          ]),
        ]),
        h("div", { class: "media-preview__audio-control-bar" }, [
          h("audio", {
            class: "media-preview__audio-control",
            controls: true,
            preload: "metadata",
            onError: this.handleMediaError,
            onEnded: this.onAudioEnded,
            onLoadedmetadata: this.onAudioLoadedMetadata,
            onPause: this.onAudioPause,
            onTimeupdate: this.onAudioTimeUpdate,
          }, [h("source", { src: this.sourceUrl, type: this.sourceMediaType })]),
        ]),
      ]);
    },
  };
}

function initialProgress() {
  return { value: 6, detail: "准备音频", indeterminate: true, cached: null };
}

function renderLoading(h, progress) {
  const value = Math.max(0, Math.min(100, Math.round(progress.value || 0)));
  return h("div", { class: "media-preview__status" }, [
    h("span", progress.cached ? "读取缓存" : "准备播放"),
    h("span", progress.detail || "建立预览流"),
    h("div", {
      class: ["media-preview__download-progress", { "media-preview__download-progress--indeterminate": progress.indeterminate }],
      role: "progressbar",
      "aria-label": "下载进度",
      "aria-valuemin": 0,
      "aria-valuemax": 100,
      "aria-valuenow": progress.indeterminate ? undefined : value,
    }, [h("span", { style: { width: `${value}%` } })]),
    progress.indeterminate ? null : h("span", `${value}%`),
  ]);
}
