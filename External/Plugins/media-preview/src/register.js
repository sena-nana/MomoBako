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
        lyricsInset.value = Math.max(96, Math.floor(viewport.clientHeight / 2));
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
        onAudioTimeUpdate(event) {
          currentPlaybackMs.value = Math.round(((event.target?.currentTime ?? 0) * 1000));
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
        const videoSourceAttrs = { src: this.sourceUrl };
        if (this.sourceMediaType && this.sourceMediaType !== "video/x-matroska") {
          videoSourceAttrs.type = this.sourceMediaType;
        }
        return h("div", { class: "media-preview media-preview--video" }, [
          h("video", {
            class: "media-preview__video",
            controls: true,
            preload: "metadata",
            playsinline: true,
            onError: this.handleMediaError,
          }, [
            h("source", videoSourceAttrs),
          ]),
        ]);
      }
      return h("div", { class: "media-preview media-preview--audio" }, [
        h("div", { class: "media-preview__audio-layout" }, [
          h("section", { class: "media-preview__audio-stage", "aria-label": "音频封面" }, [
            h("div", { class: "media-preview__audio-record" }, [
              h("div", { class: "media-preview__audio-art", "aria-hidden": "true" }, [
                this.audioArtworkUrl
                  ? h("img", { class: "media-preview__audio-cover", src: this.audioArtworkUrl, alt: "" })
                  : h("span", { class: "media-preview__audio-chip" }, this.extensionLabel),
              ]),
            ]),
            h("div", { class: "media-preview__audio-caption" }, [
              h("h2", this.entry?.name ?? ""),
              h("p", this.entry?.path ?? ""),
              h("div", { class: "media-preview__audio-meta" }, [
                h("span", this.extensionLabel),
                this.entry?.sizeLabel ? h("span", this.entry.sizeLabel) : null,
              ]),
            ]),
          ]),
          h("section", { class: "media-preview__audio-info", "aria-label": "歌词" }, [
            h("section", { class: "media-preview__audio-panel", "aria-label": "歌词面板" }, [
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
        ]),
        h("div", { class: "media-preview__audio-control-bar" }, [
          h("audio", {
            class: "media-preview__audio-control",
            controls: true,
            preload: "metadata",
            onError: this.handleMediaError,
            onTimeupdate: this.onAudioTimeUpdate,
          }, [
            h("source", { src: this.sourceUrl, type: this.sourceMediaType }),
          ]),
        ]),
      ]);
    },
  };

  ctx.registerPreview({
    supportedExtensions: [...imagePreviewExtensions, ...videoPreviewExtensions, ...audioPreviewExtensions],
    component: MediaPreviewPlugin,
  });

  function createPlaylistRuntime(controller, kind) {
    const settings = {
      imageDurationMs: 5000,
      objectFit: "contain",
    };
    let currentItem = null;
    let currentTimeMs = 0;
    let durationMs = kind === "image" ? settings.imageDurationMs : 0;
    let isPlaying = false;
    let timer = null;
    let mediaElement = null;
    let imageElement = null;
    let frameElement = null;
    let lyricLines = [];
    let lyricViewport = null;
    let lyricItems = [];

    function normalizeImageDurationMs(value) {
      if (!Number.isFinite(value)) return 5000;
      return Math.min(30000, Math.max(2000, Math.round(value)));
    }

    function normalizeObjectFit(value) {
      return value === "cover" ? "cover" : "contain";
    }

    function imageDurationMs() {
      return normalizeImageDurationMs(settings.imageDurationMs);
    }

    function objectFit() {
      return normalizeObjectFit(settings.objectFit);
    }

    function emitState(extra = {}) {
      controller.onEvent({
        type: "state",
        canPlay: Boolean(currentItem),
        isPlaying,
        ...extra,
      });
    }

    function emitTime() {
      controller.onEvent({
        type: "time",
        currentTimeMs,
        durationMs,
      });
    }

    function applyObjectFit() {
      if (imageElement) imageElement.style.objectFit = objectFit();
      if (mediaElement && kind === "video") mediaElement.style.objectFit = objectFit();
    }

    function buildFrame() {
      const element = document.createElement("div");
      element.className = `media-playlist-runtime media-playlist-runtime--${kind}`;
      return element;
    }

    function clearMountTarget() {
      controller.mountTarget.replaceChildren();
      mediaElement = null;
      imageElement = null;
      frameElement = null;
    }

    function stopTimer() {
      if (timer) {
        clearInterval(timer);
        timer = null;
      }
    }

    function startTimer() {
      stopTimer();
      if (durationMs <= 0) return;
      timer = setInterval(() => {
        currentTimeMs += 1000;
        if (currentTimeMs >= durationMs) {
          currentTimeMs = durationMs;
          isPlaying = false;
          stopTimer();
          emitTime();
          emitState();
          controller.onEvent({ type: "ended" });
          return;
        }
        emitTime();
      }, 1000);
    }

    function syncMediaTime() {
      if (!mediaElement) return;
      currentTimeMs = Math.round((mediaElement.currentTime ?? 0) * 1000);
      durationMs = Number.isFinite(mediaElement.duration) ? Math.round(mediaElement.duration * 1000) : durationMs;
      updateRuntimeLyrics();
      emitTime();
    }

    function syncMediaState() {
      if (!mediaElement) return;
      isPlaying = !mediaElement.paused;
      emitState();
    }

    function createMediaNode(tagName, sourceUrl, mediaType) {
      const element = document.createElement(tagName);
      element.className = tagName === "video"
        ? "media-preview__video media-playlist-runtime__media"
        : "media-preview__audio-control media-playlist-runtime__media";
      element.controls = true;
      element.preload = "metadata";
      element.src = sourceUrl;
      if (tagName === "video") {
        element.playsInline = true;
      }
      element.dataset.mediaType = mediaType ?? "";
      element.addEventListener("loadedmetadata", () => {
        durationMs = Number.isFinite(element.duration) ? Math.round(element.duration * 1000) : 0;
        emitTime();
        emitState();
      });
      element.addEventListener("timeupdate", syncMediaTime);
      element.addEventListener("play", syncMediaState);
      element.addEventListener("pause", syncMediaState);
      element.addEventListener("ended", () => {
        isPlaying = false;
        syncMediaTime();
        emitState();
        controller.onEvent({ type: "ended" });
      });
      element.addEventListener("error", () => {
        controller.onEvent({ type: "error", message: "媒体无法播放" });
      });
      applyObjectFit();
      return element;
    }

    function createAudioArtwork(item) {
      const wrapper = document.createElement("div");
      wrapper.className = "media-preview__audio-art media-playlist-runtime__audio-art";
      if (item.thumbnailPath) {
        const cover = document.createElement("img");
        cover.className = "media-preview__audio-cover";
        cover.src = ctx.fileSrc(item.thumbnailPath);
        cover.alt = "";
        wrapper.append(cover);
      } else {
        const chip = document.createElement("span");
        chip.className = "media-preview__audio-chip";
        chip.textContent = item.extension?.toUpperCase() || "AUDIO";
        wrapper.append(chip);
      }
      return wrapper;
    }

    function createAudioStage(item) {
      const stage = document.createElement("section");
      stage.className = "media-preview__audio-stage";
      stage.setAttribute("aria-label", "音频封面");
      const record = document.createElement("div");
      record.className = "media-preview__audio-record";
      record.append(createAudioArtwork(item));
      const caption = document.createElement("div");
      caption.className = "media-preview__audio-caption";
      const title = document.createElement("h2");
      title.textContent = item.filename ?? "";
      const path = document.createElement("p");
      path.textContent = item.path ?? "";
      const meta = document.createElement("div");
      meta.className = "media-preview__audio-meta";
      const extension = document.createElement("span");
      extension.textContent = item.extension?.toUpperCase() || "AUDIO";
      meta.append(extension);
      caption.append(title, path, meta);
      stage.append(record, caption);
      return stage;
    }

    function resetRuntimeLyrics() {
      lyricLines = [];
      lyricViewport = null;
      lyricItems = [];
    }

    function createLyricsViewport() {
      lyricViewport = document.createElement("div");
      lyricViewport.className = "media-preview__audio-lyrics media-preview__audio-lyrics--empty media-playlist-runtime__lyrics";
      lyricViewport.textContent = "读取歌词...";
      return lyricViewport;
    }

    function runtimeLyricsInset() {
      return Math.max(96, Math.floor((lyricViewport?.clientHeight ?? 0) / 2));
    }

    function renderRuntimeLyrics(lines) {
      if (!lyricViewport) return;
      lyricViewport.replaceChildren();
      lyricItems = [];
      if (!lines.length) {
        lyricViewport.className = "media-preview__audio-lyrics media-preview__audio-lyrics--empty media-playlist-runtime__lyrics";
        const placeholder = document.createElement("span");
        placeholder.textContent = "暂无歌词";
        lyricViewport.append(placeholder);
        return;
      }
      lyricViewport.className = "media-preview__audio-lyrics media-playlist-runtime__lyrics";
      const track = document.createElement("div");
      track.className = "media-preview__audio-lyrics-track";
      track.style.setProperty("--lyrics-inset", `${runtimeLyricsInset()}px`);
      for (const [index, line] of lines.entries()) {
        const button = document.createElement("button");
        button.type = "button";
        button.className = [
          "media-preview__audio-lyric",
          line.timeMs == null ? "" : "is-timed",
        ].filter(Boolean).join(" ");
        button.disabled = line.timeMs == null;
        button.textContent = line.text;
        button.addEventListener("click", () => {
          if (line.timeMs == null || !mediaElement) return;
          mediaElement.currentTime = line.timeMs / 1000;
          syncMediaTime();
        });
        lyricItems[index] = button;
        track.append(button);
      }
      lyricViewport.append(track);
      updateRuntimeLyrics();
    }

    async function loadRuntimeLyrics(item) {
      if (kind !== "audio") return;
      try {
        const bytes = await ctx.readFile({
          repoId: controller.repoId,
          path: siblingLrcPath(item.path),
        });
        lyricLines = parseLrcLyrics(decodeTextBytes(Uint8Array.from(bytes)));
      } catch {
        lyricLines = [];
      }
      renderRuntimeLyrics(lyricLines);
    }

    function updateRuntimeLyrics() {
      if (kind !== "audio" || !lyricLines.length || !lyricViewport) return;
      const activeIndex = findActiveLyricIndex(lyricLines, currentTimeMs);
      for (const [index, item] of lyricItems.entries()) {
        if (!item) continue;
        item.classList.toggle("is-active", index === activeIndex);
        item.classList.toggle("is-passed", activeIndex > index);
      }
      const activeItem = activeIndex >= 0 ? lyricItems[activeIndex] : null;
      if (!activeItem) return;
      const top = activeItem.offsetTop - (lyricViewport.clientHeight / 2) + (activeItem.clientHeight / 2);
      lyricViewport.scrollTop = Math.max(0, top);
    }

    function createAudioShell(item, mediaNode) {
      const wrapper = document.createElement("div");
      wrapper.className = "media-preview media-preview--audio media-playlist-runtime__audio-shell";
      const layout = document.createElement("div");
      layout.className = "media-preview__audio-layout media-playlist-runtime__audio-layout";
      const info = document.createElement("section");
      info.className = "media-preview__audio-info media-playlist-runtime__audio-info";
      info.setAttribute("aria-label", "歌词");
      const panel = document.createElement("section");
      panel.className = "media-preview__audio-panel media-playlist-runtime__audio-panel";
      panel.setAttribute("aria-label", "歌词面板");
      panel.append(createLyricsViewport());
      info.append(panel);
      const controlBar = document.createElement("div");
      controlBar.className = "media-preview__audio-control-bar";
      controlBar.append(mediaNode);
      layout.append(createAudioStage(item), info);
      wrapper.append(layout, controlBar);
      return wrapper;
    }

    function mountNode(node) {
      frameElement = buildFrame();
      frameElement.append(node);
      controller.mountTarget.replaceChildren(frameElement);
      applyObjectFit();
    }

    async function prepareSource(item) {
      const response = await ctx.preparePreviewFileSource({
        repoId: controller.repoId,
        path: item.path,
      });
      if (!response.sourceUrl) {
        throw new Error("媒体播放源不可用");
      }
      return response;
    }

    return {
      async load(item) {
        currentItem = item;
        currentTimeMs = 0;
        durationMs = kind === "image" ? imageDurationMs() : 0;
        isPlaying = false;
        stopTimer();
        clearMountTarget();
        resetRuntimeLyrics();

        try {
          const response = await prepareSource(item);
          if (kind === "image") {
            imageElement = document.createElement("img");
            imageElement.className = "media-preview__image media-playlist-runtime__media";
            imageElement.src = response.sourceUrl;
            imageElement.alt = item.filename;
            imageElement.dataset.path = item.path;
            imageElement.addEventListener("error", () => {
              controller.onEvent({ type: "error", message: "图片无法播放" });
            });
            mountNode(imageElement);
          } else {
            mediaElement = createMediaNode(kind === "video" ? "video" : "audio", response.sourceUrl, response.mediaType);
            mediaElement.dataset.path = item.path;
            mountNode(kind === "audio" ? createAudioShell(item, mediaElement) : mediaElement);
            if (kind === "audio") {
              void loadRuntimeLyrics(item);
            }
          }
        } catch (cause) {
          currentItem = null;
          controller.onEvent({
            type: "error",
            message: cause instanceof Error ? cause.message : "媒体播放源不可用",
          });
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
        if (!mediaElement) {
          emitState();
          return;
        }
        try {
          await mediaElement.play();
        } catch (error) {
          isPlaying = false;
          emitState();
          controller.onEvent({
            type: "error",
            message: error instanceof Error ? error.message : "媒体无法播放",
          });
        }
      },
      pause() {
        isPlaying = false;
        if (kind === "image") {
          stopTimer();
          emitState();
          return;
        }
        mediaElement?.pause();
      },
      seek(timeMs) {
        currentTimeMs = Math.max(0, Math.min(durationMs, Math.round(timeMs)));
        if (kind === "image") {
          emitTime();
          return;
        }
        if (!mediaElement) return;
        mediaElement.currentTime = currentTimeMs / 1000;
        updateRuntimeLyrics();
      },
      configure(nextSettings = {}) {
        if (nextSettings.imageDurationMs !== undefined) {
          settings.imageDurationMs = normalizeImageDurationMs(Number(nextSettings.imageDurationMs));
        }
        if (nextSettings.objectFit !== undefined) {
          settings.objectFit = normalizeObjectFit(nextSettings.objectFit);
        }
        applyObjectFit();
        if (kind === "image") {
          const previousDurationMs = durationMs;
          durationMs = imageDurationMs();
          currentTimeMs = Math.min(currentTimeMs, durationMs);
          if (previousDurationMs !== durationMs) {
            emitTime();
          }
          if (isPlaying) startTimer();
        }
      },
      setVolume(value) {
        if (!mediaElement) return;
        mediaElement.volume = Math.max(0, Math.min(1, value));
      },
      dispose() {
        stopTimer();
        if (mediaElement) {
          mediaElement.pause();
          mediaElement.src = "";
        }
        clearMountTarget();
      },
    };
  }

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
      return createPlaylistRuntime(controller, "image");
    },
  });

  ctx.registerPlaylistPlayer({
    playerTypeId: "momobako.playlist.audio-sequence",
    label: "音频顺序播放",
    fileClass: "audio",
    supportedExtensions: audioPreviewExtensions,
    supportsSeek: true,
    supportsVolume: true,
    supportsPreviewNavigation: true,
    description: "复用媒体能力播放音频队列。",
    createRuntime(controller) {
      return createPlaylistRuntime(controller, "audio");
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
      return createPlaylistRuntime(controller, "video");
    },
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
