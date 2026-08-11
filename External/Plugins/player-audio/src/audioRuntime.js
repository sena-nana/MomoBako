/** 音频播放列表运行时，统一承载队列内的音频与歌词展示。 */
import { audioDisplayMetadata, resolveArtworkUrl } from "./audioMetadata.js";
import {
  decodeTextBytes,
  findActiveLyricIndex,
  parseLrcLyrics,
  readLocalTextFile,
  siblingLrcPath,
} from "./lyrics.js";
import { errorText, prepareAudioSource } from "./playbackSource.js";

export function createAudioPlaylistRuntime(ctx, controller) {
  let currentItem = null;
  let currentTimeMs = 0;
  let durationMs = 0;
  let isPlaying = false;
  let mediaElement = null;
  let lyricLines = [];
  let lyricViewport = null;
  let lyricItems = [];

  function emitState(extra = {}) {
    controller.onEvent({ type: "state", canPlay: Boolean(currentItem), isPlaying, ...extra });
  }
  function emitTime() {
    controller.onEvent({ type: "time", currentTimeMs, durationMs });
  }
  function clearTarget() {
    controller.mountTarget.replaceChildren();
    mediaElement = null;
    lyricViewport = null;
    lyricItems = [];
    lyricLines = [];
  }
  function syncMediaTime() {
    if (!mediaElement) return;
    currentTimeMs = Math.round((mediaElement.currentTime ?? 0) * 1000);
    durationMs = Number.isFinite(mediaElement.duration) ? Math.round(mediaElement.duration * 1000) : durationMs;
    updateLyrics();
    emitTime();
  }
  function createAudioNode(sourceUrl, mediaType) {
    const audio = document.createElement("audio");
    audio.className = "media-preview__audio-control media-playlist-runtime__media";
    audio.controls = false;
    audio.preload = "metadata";
    audio.src = sourceUrl;
    audio.dataset.mediaType = mediaType ?? "";
    audio.addEventListener("loadedmetadata", () => {
      durationMs = Number.isFinite(audio.duration) ? Math.round(audio.duration * 1000) : 0;
      emitTime();
      emitState();
    });
    audio.addEventListener("timeupdate", syncMediaTime);
    audio.addEventListener("play", () => { isPlaying = true; emitState(); });
    audio.addEventListener("pause", () => { isPlaying = false; emitState(); });
    audio.addEventListener("ended", () => {
      isPlaying = false;
      syncMediaTime();
      emitState();
      controller.onEvent({ type: "ended" });
    });
    audio.addEventListener("error", () => controller.onEvent({ type: "error", message: "音频无法播放" }));
    return audio;
  }

  function createArtwork(item) {
    const wrapper = document.createElement("div");
    wrapper.className = "media-preview__audio-art media-playlist-runtime__audio-art";
    const artworkUrl = resolveArtworkUrl(item.thumbnailPath, ctx.fileSrc);
    if (artworkUrl) {
      const cover = document.createElement("img");
      cover.className = "media-preview__audio-cover";
      cover.src = artworkUrl;
      cover.alt = "";
      wrapper.append(cover);
    } else {
      const chip = document.createElement("span");
      chip.className = "media-preview__audio-chip";
      chip.textContent = "音频";
      wrapper.append(chip);
    }
    return wrapper;
  }

  function createAudioStage(item) {
    const metadata = audioDisplayMetadata(item);
    const stage = document.createElement("section");
    stage.className = "media-preview__audio-stage";
    stage.setAttribute("aria-label", "音频封面");
    const record = document.createElement("div");
    record.className = "media-preview__audio-record";
    record.append(createArtwork(item));
    const caption = document.createElement("div");
    caption.className = "media-preview__audio-caption";
    const title = document.createElement("h2");
    title.textContent = metadata.title;
    const secondary = document.createElement("p");
    secondary.textContent = metadata.artist || metadata.album || "通用音频";
    const meta = document.createElement("div");
    meta.className = "media-preview__audio-meta";
    const kind = document.createElement("span");
    kind.textContent = "音频";
    meta.append(kind);
    caption.append(title, secondary, meta);
    stage.append(record, caption);
    return stage;
  }

  function createLyricsViewport() {
    lyricViewport = document.createElement("div");
    lyricViewport.className = "media-preview__audio-lyrics media-preview__audio-lyrics--empty media-playlist-runtime__lyrics";
    lyricViewport.textContent = "读取歌词...";
    return lyricViewport;
  }

  function renderLyrics(lines) {
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
    track.style.setProperty("--lyrics-inset", `${Math.max(96, Math.floor((lyricViewport.clientHeight || 0) / 2))}px`);
    for (const [index, line] of lines.entries()) {
      const button = document.createElement("button");
      button.type = "button";
      button.className = ["media-preview__audio-lyric", line.timeMs == null ? "" : "is-timed"].filter(Boolean).join(" ");
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
    updateLyrics();
  }

  async function loadLyrics(item, playback) {
    try {
      let text;
      if (playback?.lyricSourceUrl || playback?.lyricPath) {
        text = await readLocalTextFile(playback.lyricSourceUrl || ctx.fileSrc(playback.lyricPath));
      } else {
        const bytes = await ctx.readFile({ repoId: controller.repoId, path: siblingLrcPath(item.path) });
        text = decodeTextBytes(Uint8Array.from(bytes));
      }
      lyricLines = parseLrcLyrics(text);
    } catch (cause) {
      lyricLines = [];
      void ctx.logger.debug("播放列表条目没有可用歌词。", {
        action: "audioRuntime.loadLyrics",
        repoId: controller.repoId,
        context: { path: item.path, message: errorText(cause, "歌词不可用") },
      });
    }
    renderLyrics(lyricLines);
  }

  function updateLyrics() {
    if (!lyricLines.length || !lyricViewport) return;
    const activeIndex = findActiveLyricIndex(lyricLines, currentTimeMs);
    for (const [index, item] of lyricItems.entries()) {
      item?.classList.toggle("is-active", index === activeIndex);
      item?.classList.toggle("is-passed", activeIndex > index);
    }
    const activeItem = activeIndex >= 0 ? lyricItems[activeIndex] : null;
    if (activeItem) {
      lyricViewport.scrollTop = Math.max(0, activeItem.offsetTop - (lyricViewport.clientHeight / 2) + (activeItem.clientHeight / 2));
    }
  }

  function createShell(item, audio) {
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
    controlBar.append(audio);
    layout.append(createAudioStage(item), info);
    wrapper.append(layout, controlBar);
    return wrapper;
  }

  function mount(node) {
    const frame = document.createElement("div");
    frame.className = "media-playlist-runtime media-playlist-runtime--audio";
    frame.append(node);
    controller.mountTarget.replaceChildren(frame);
  }

  return {
    async load(item) {
      currentItem = item;
      currentTimeMs = 0;
      durationMs = 0;
      isPlaying = false;
      clearTarget();
      try {
        const progress = createProgressNode(item);
        mount(progress.node);
        const response = await prepareAudioSource(ctx, controller.repoId, item.path, (event) => {
          progress.update(event);
          controller.onEvent({ type: "state", canPlay: false, isPlaying: false, loading: true, progress: event });
        });
        mediaElement = createAudioNode(response.sourceUrl, response.mediaType);
        mediaElement.dataset.path = item.path;
        mount(createShell(item, mediaElement));
        void loadLyrics(item, response);
      } catch (cause) {
        currentItem = null;
        const message = errorText(cause, "音频播放源不可用");
        void ctx.logger.error("播放列表音频源准备失败。", {
          action: "audioRuntime.prepareSource",
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
      if (!currentItem || !mediaElement) return;
      try {
        await mediaElement.play();
      } catch (cause) {
        isPlaying = false;
        emitState();
        controller.onEvent({ type: "error", message: errorText(cause, "音频无法播放") });
      }
    },
    pause() {
      isPlaying = false;
      mediaElement?.pause();
    },
    seek(timeMs) {
      currentTimeMs = durationMs > 0
        ? Math.min(durationMs, Math.max(0, Math.round(timeMs)))
        : Math.max(0, Math.round(timeMs));
      if (mediaElement) mediaElement.currentTime = currentTimeMs / 1000;
      updateLyrics();
      emitTime();
    },
    setVolume(value) {
      if (mediaElement) mediaElement.volume = Math.max(0, Math.min(1, value));
    },
    dispose() {
      if (mediaElement) {
        mediaElement.pause();
        mediaElement.src = "";
      }
      clearTarget();
    },
  };
}

function createProgressNode(item) {
  const node = document.createElement("div");
  node.className = "media-playlist-runtime__loading";
  const title = document.createElement("strong");
  title.textContent = item.name || item.filename || "准备播放";
  const detail = document.createElement("span");
  detail.className = "media-playlist-runtime__loading-detail";
  detail.textContent = "准备音频";
  const bar = document.createElement("div");
  bar.className = "media-preview__download-progress media-preview__download-progress--indeterminate";
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
