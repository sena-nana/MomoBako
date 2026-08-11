/** 图片与视频预览插件入口。音频能力由 momobako.player.audio 独立提供。 */
import {
  imagePreviewExtensions,
  isImageExtension,
  videoPreviewExtensions,
} from "./mediaExtensions.js";

export function register(ctx) {
  const { computed, h, onBeforeUnmount, ref, watch } = ctx.vue;

  const MediaPreviewPlugin = {
    name: "MediaPreviewPlugin",
    props: {
      entry: { type: Object, default: null },
      repoId: { type: String, default: "" },
    },
    setup(props) {
      const state = ref("idle");
      const sourceUrl = ref("");
      const sourceMediaType = ref("");
      const errorMessage = ref("");
      const playbackProgress = ref(createInitialProgress());
      const mediaKind = computed(() => (isImageExtension(props.entry?.extension) ? "image" : "video"));

      async function loadMediaSource() {
        state.value = "loading";
        sourceUrl.value = "";
        sourceMediaType.value = "";
        errorMessage.value = "";
        playbackProgress.value = createInitialProgress();
        try {
          const response = await prepareSource(ctx, props.repoId, props.entry.path, (event) => {
            playbackProgress.value = progressState(playbackProgress.value, event);
          });
          sourceUrl.value = response.sourceUrl;
          sourceMediaType.value = response.mediaType ?? "";
          playbackProgress.value = { value: 100, detail: "播放源已就绪", indeterminate: false, cached: response.cached ?? null };
          state.value = "ready";
        } catch (cause) {
          state.value = "error";
          errorMessage.value = errorText(cause, "媒体预览源不可用");
          void ctx.logger.error("媒体预览源准备失败。", {
            action: "preview.prepareSource",
            repoId: props.repoId,
            context: { path: props.entry?.path, message: errorMessage.value },
          });
        }
      }

      watch(
        [() => props.repoId, () => props.entry?.path, () => props.entry?.extension],
        loadMediaSource,
        { immediate: true },
      );
      onBeforeUnmount(() => {
        sourceUrl.value = "";
      });

      return {
        errorMessage,
        handleMediaError() {
          state.value = "error";
          errorMessage.value = "媒体无法播放";
          void ctx.logger.warn("媒体元素播放失败。", {
            action: "preview.mediaError",
            repoId: props.repoId,
            context: { path: props.entry?.path },
          });
        },
        mediaKind,
        playbackProgress,
        sourceMediaType,
        sourceUrl,
        state,
      };
    },
    render() {
      if (this.state === "loading") return renderLoading(h, this.playbackProgress);
      if (this.state === "error") {
        return h("div", { class: "media-preview__overlay media-preview__overlay--error" }, [
          h("strong", "无法预览该媒体"),
          h("span", this.errorMessage),
        ]);
      }
      if (this.mediaKind === "image") {
        return h("div", { class: "media-preview media-preview--image" }, [
          h("img", { class: "media-preview__image", src: this.sourceUrl, onError: this.handleMediaError }),
        ]);
      }
      const source = { src: this.sourceUrl };
      if (this.sourceMediaType && this.sourceMediaType !== "video/x-matroska") source.type = this.sourceMediaType;
      return h("div", { class: "media-preview media-preview--video" }, [
        h("video", {
          class: "media-preview__video",
          controls: true,
          preload: "metadata",
          playsinline: true,
          onError: this.handleMediaError,
        }, [h("source", source)]),
      ]);
    },
  };

  ctx.registerPreview({
    supportedExtensions: [...imagePreviewExtensions, ...videoPreviewExtensions],
    component: MediaPreviewPlugin,
  });

  ctx.registerPlaylistPlayer({
    playerTypeId: "momobako.playlist.image-slideshow",
    label: "图片幻灯片",
    fileClass: "image",
    supportedExtensions: imagePreviewExtensions,
    supportsSeek: false,
    supportsVolume: false,
    supportsPreviewNavigation: true,
    description: "按顺序展示图片并交由宿主处理队列模式。",
    createRuntime(controller) {
      return createVisualPlaylistRuntime(ctx, controller, "image");
    },
  });

  ctx.registerPlaylistPlayer({
    playerTypeId: "momobako.playlist.video-sequence",
    label: "视频顺序播放",
    fileClass: "video",
    supportedExtensions: videoPreviewExtensions,
    supportsSeek: true,
    supportsVolume: true,
    supportsPreviewNavigation: true,
    description: "复用媒体能力播放视频队列。",
    createRuntime(controller) {
      return createVisualPlaylistRuntime(ctx, controller, "video");
    },
  });
}

/** 创建图片或视频播放列表运行时。 */
function createVisualPlaylistRuntime(ctx, controller, kind) {
  const settings = { imageDurationMs: 5000, objectFit: "contain" };
  let currentItem = null;
  let currentTimeMs = 0;
  let durationMs = kind === "image" ? settings.imageDurationMs : 0;
  let isPlaying = false;
  let timer = null;
  let mediaElement = null;
  let imageElement = null;

  function emitState(extra = {}) {
    controller.onEvent({ type: "state", canPlay: Boolean(currentItem), isPlaying, ...extra });
  }
  function emitTime() {
    controller.onEvent({ type: "time", currentTimeMs, durationMs });
  }
  function stopTimer() {
    if (!timer) return;
    clearInterval(timer);
    timer = null;
  }
  function startTimer() {
    stopTimer();
    timer = setInterval(() => {
      currentTimeMs = Math.min(durationMs, currentTimeMs + 1000);
      emitTime();
      if (currentTimeMs < durationMs) return;
      isPlaying = false;
      stopTimer();
      emitState();
      controller.onEvent({ type: "ended" });
    }, 1000);
  }
  function applyObjectFit() {
    if (imageElement) imageElement.style.objectFit = settings.objectFit;
    if (mediaElement) mediaElement.style.objectFit = settings.objectFit;
  }
  function clearTarget() {
    controller.mountTarget.replaceChildren();
    mediaElement = null;
    imageElement = null;
  }
  function mount(node) {
    const frame = document.createElement("div");
    frame.className = `media-playlist-runtime media-playlist-runtime--${kind}`;
    frame.append(node);
    controller.mountTarget.replaceChildren(frame);
    applyObjectFit();
  }
  function createVideo(sourceUrl, mediaType) {
    const element = document.createElement("video");
    element.className = "media-preview__video media-playlist-runtime__media";
    element.controls = false;
    element.preload = "metadata";
    element.playsInline = true;
    element.src = sourceUrl;
    element.dataset.mediaType = mediaType ?? "";
    element.addEventListener("loadedmetadata", () => {
      durationMs = Number.isFinite(element.duration) ? Math.round(element.duration * 1000) : 0;
      emitTime();
      emitState();
    });
    element.addEventListener("timeupdate", () => {
      currentTimeMs = Math.round((element.currentTime ?? 0) * 1000);
      durationMs = Number.isFinite(element.duration) ? Math.round(element.duration * 1000) : durationMs;
      emitTime();
    });
    element.addEventListener("play", () => { isPlaying = true; emitState(); });
    element.addEventListener("pause", () => { isPlaying = false; emitState(); });
    element.addEventListener("ended", () => {
      isPlaying = false;
      emitState();
      controller.onEvent({ type: "ended" });
    });
    element.addEventListener("error", () => controller.onEvent({ type: "error", message: "视频无法播放" }));
    return element;
  }

  return {
    async load(item) {
      currentItem = item;
      currentTimeMs = 0;
      durationMs = kind === "image" ? normalizeImageDuration(settings.imageDurationMs) : 0;
      isPlaying = false;
      stopTimer();
      clearTarget();
      try {
        const progress = createProgressNode(item);
        mount(progress.node);
        const response = await prepareSource(ctx, controller.repoId, item.path, progress.update);
        if (kind === "image") {
          imageElement = document.createElement("img");
          imageElement.className = "media-preview__image media-playlist-runtime__media";
          imageElement.src = response.sourceUrl;
          imageElement.alt = item.filename;
          imageElement.dataset.path = item.path;
          imageElement.addEventListener("error", () => controller.onEvent({ type: "error", message: "图片无法播放" }));
          mount(imageElement);
        } else {
          mediaElement = createVideo(response.sourceUrl, response.mediaType);
          mediaElement.dataset.path = item.path;
          mount(mediaElement);
        }
      } catch (cause) {
        currentItem = null;
        const message = errorText(cause, "媒体播放源不可用");
        void ctx.logger.error("播放列表媒体源准备失败。", {
          action: "playlist.prepareSource",
          repoId: controller.repoId,
          context: { path: item.path, message },
        });
        controller.onEvent({ type: "error", message });
        return;
      }
      emitState();
      emitTime();
    },
    async play() {
      if (!currentItem) return;
      if (kind === "image") {
        isPlaying = true;
        emitState();
        startTimer();
        return;
      }
      try {
        await mediaElement?.play();
      } catch (cause) {
        const message = errorText(cause, "视频无法播放");
        isPlaying = false;
        emitState();
        controller.onEvent({ type: "error", message });
      }
    },
    pause() {
      isPlaying = false;
      if (kind === "image") {
        stopTimer();
        emitState();
      } else {
        mediaElement?.pause();
      }
    },
    seek(timeMs) {
      currentTimeMs = durationMs > 0 ? Math.min(durationMs, Math.max(0, Math.round(timeMs))) : Math.max(0, Math.round(timeMs));
      if (kind === "video" && mediaElement) mediaElement.currentTime = currentTimeMs / 1000;
      emitTime();
    },
    configure(next = {}) {
      settings.imageDurationMs = normalizeImageDuration(next.imageDurationMs ?? settings.imageDurationMs);
      settings.objectFit = next.objectFit === "cover" ? "cover" : "contain";
      applyObjectFit();
      if (kind === "image") {
        durationMs = settings.imageDurationMs;
        currentTimeMs = Math.min(currentTimeMs, durationMs);
        emitTime();
        if (isPlaying) startTimer();
      }
    },
    setVolume(value) {
      if (mediaElement) mediaElement.volume = Math.max(0, Math.min(1, value));
    },
    dispose() {
      stopTimer();
      if (mediaElement) {
        mediaElement.pause();
        mediaElement.src = "";
      }
      clearTarget();
    },
  };
}

function createInitialProgress() {
  return { value: 6, detail: "准备媒体", indeterminate: true, cached: null };
}

function progressState(previous, event) {
  return {
    value: event.value ?? previous.value,
    detail: event.detail || previous.detail,
    indeterminate: Boolean(event.indeterminate),
    cached: event.cached ?? previous.cached,
  };
}

/** 通过宿主统一播放源路由准备媒体，插件不感知具体来源协议。 */
async function prepareSource(ctx, repoId, path, onProgress) {
  const request = { repoId, path };
  const response = ctx.prepareEntryPlaybackSourceWithProgress
    ? await ctx.prepareEntryPlaybackSourceWithProgress(request, (event) => {
        if (event.path !== path) return;
        onProgress?.(event);
      })
    : await ctx.prepareEntryPlaybackSource(request);
  const sourceUrl = response.sourceUrl || (response.localPath ? ctx.fileSrc(response.localPath) : null);
  if (!sourceUrl) throw new Error("媒体播放源不可用");
  return { ...response, sourceUrl };
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

function createProgressNode(item) {
  const node = document.createElement("div");
  node.className = "media-playlist-runtime__loading";
  const title = document.createElement("strong");
  title.textContent = item.name || item.filename || "准备播放";
  const detail = document.createElement("span");
  detail.className = "media-playlist-runtime__loading-detail";
  detail.textContent = "准备媒体";
  const bar = document.createElement("div");
  bar.className = "media-preview__download-progress media-preview__download-progress--indeterminate";
  bar.setAttribute("role", "progressbar");
  bar.setAttribute("aria-label", "下载进度");
  const fill = document.createElement("span");
  fill.style.width = "0%";
  bar.append(fill);
  node.append(title, detail, bar);
  return {
    node,
    update(event) {
      const value = Math.max(0, Math.min(100, Math.round(Number(event.value) || 0)));
      detail.textContent = event.detail || detail.textContent;
      bar.classList.toggle("media-preview__download-progress--indeterminate", Boolean(event.indeterminate));
      fill.style.width = `${value}%`;
    },
  };
}

function normalizeImageDuration(value) {
  if (!Number.isFinite(Number(value))) return 5000;
  return Math.min(30000, Math.max(2000, Math.round(Number(value))));
}

function errorText(cause, fallback) {
  return cause instanceof Error ? cause.message : fallback;
}
