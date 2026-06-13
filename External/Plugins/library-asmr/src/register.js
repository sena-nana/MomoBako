import {
  appendMetadataCandidate,
  buildCandidateSummary,
  formatCandidateFieldValue,
  parseCandidateJson,
  readMetadataCandidates,
} from "./asmrCandidates.js";
import {
  clearPlaylistForRepo,
  fileSummary,
  buildListeningProgressMetadata,
  hasPlaylistEntry,
  isAsmrAudioEntry,
  matchAsmrEntry,
  mergePlaylistEntries,
  metadataNumber,
  metadataText,
  persistPlaylist,
  readStoredPlaylist,
  resolveActivePlaylist,
  shouldPersistListeningProgress,
  workAudioEntries,
} from "./asmrLibrary.js";

function formatList(value) {
  if (Array.isArray(value)) {
    return value
      .filter((item) => typeof item === "string" || typeof item === "number" || typeof item === "boolean")
      .map(String)
      .filter(Boolean)
      .join("，");
  }
  if (typeof value === "string") return value;
  return "";
}

function formatNumber(value, suffix = "") {
  if (value == null) return "";
  return `${value.toLocaleString("zh-CN")}${suffix}`;
}

function formatDuration(value) {
  if (value == null || value <= 0) return "";
  const seconds = Math.round(value / 1000);
  const minutes = Math.floor(seconds / 60);
  const rest = seconds % 60;
  return `${minutes}:${String(rest).padStart(2, "0")}`;
}

function formatListeningStatus(status, progress) {
  const labels = {
    unlistened: "未收听",
    listening: "收听中",
    listened: "已听完",
  };
  const label = labels[status] ?? status;
  if (progress == null || progress <= 0) return label;
  return `${label} · ${Math.round(progress)}%`;
}

function remoteCoverUrl(metadata) {
  const cover = metadata?.coverUrl ?? metadata?.cover;
  if (typeof cover !== "string") return "";
  const value = cover.trim();
  return /^https?:\/\//i.test(value) ? value : "";
}

function createMetadataPanel(ctx) {
  const { h, computed, reactive, ref, watch } = ctx.vue;
  return {
    name: "AsmrMetadataPanel",
    props: ["repoId", "entry", "entries", "saveMetadata", "saveCoverThumbnail"],
    setup(props) {
      const importOpen = ref(false);
      const importDraft = ref("");
      const error = ref("");
      const coverSaveState = ref("idle");
      const lookup = reactive({
        provider: "momobako.service.provider.dlsite",
        id: "",
        isLoading: false,
      });

      const canEdit = computed(() => props.entry.kind === "file" && Boolean(props.entry.assetId));
      const workId = computed(() => metadataText(props.entry, "rjCode") || metadataText(props.entry, "workId"));
      const rows = computed(() => {
        const metadata = props.entry.metadata ?? {};
        const rowValues = [
          { key: "workId", label: "作品 ID", value: metadataText(props.entry, "workId") || metadataText(props.entry, "rjCode") },
          { key: "workTitle", label: "标题", value: metadataText(props.entry, "workTitle") },
          { key: "workRoot", label: "作品目录", value: metadataText(props.entry, "workRoot") },
          { key: "trackTitle", label: "音轨", value: metadataText(props.entry, "trackTitle") },
          { key: "circle", label: "社团", value: metadataText(props.entry, "circle") },
          { key: "voiceActors", label: "声优", value: formatList(metadata.voiceActors) },
          { key: "series", label: "系列", value: metadataText(props.entry, "series") },
          { key: "scenarioTags", label: "标签", value: formatList(metadata.scenarioTags ?? metadata.tags) },
          { key: "releaseDate", label: "发售日", value: metadataText(props.entry, "releaseDate") },
          { key: "ageRating", label: "年龄分级", value: metadataText(props.entry, "ageRating") },
          { key: "lyricStatus", label: "歌词", value: metadataText(props.entry, "lyricStatus") || "未检测" },
          { key: "entryKind", label: "条目类型", value: metadataText(props.entry, "asmrEntryKind") },
          {
            key: "listeningStatus",
            label: "收听状态",
            value: formatListeningStatus(metadataText(props.entry, "listeningStatus"), metadataNumber(props.entry, "listeningProgress")),
          },
          { key: "trackDurationMs", label: "音轨时长", value: formatDuration(metadataNumber(props.entry, "trackDurationMs")) },
          { key: "price", label: "价格", value: formatNumber(metadataNumber(props.entry, "price"), " JPY") },
          { key: "sales", label: "销量", value: formatNumber(metadataNumber(props.entry, "sales") ?? metadataNumber(props.entry, "dlCount")) },
          { key: "rateAverage", label: "评分", value: formatNumber(metadataNumber(props.entry, "rateAverage") ?? metadataNumber(props.entry, "ratingAverage")) },
          { key: "reviewCount", label: "评论数", value: formatNumber(metadataNumber(props.entry, "reviewCount")) },
        ];
        return rowValues.filter((row) => row.value);
      });
      const candidates = computed(() => (
        readMetadataCandidates(props.entry.metadata)
          .map(buildCandidateSummary)
          .filter((candidate) => Object.keys(candidate.patch).length || candidate.skipped.length)
          .map((candidate) => ({
            ...candidate,
            coverUrl: remoteCoverUrl(candidate.patch),
            fields: Object.entries(candidate.patch).map(([key, value]) => ({
              key,
              value: formatCandidateFieldValue(value),
            })),
          }))
      ));
      const coverUrl = computed(() => remoteCoverUrl(props.entry.metadata));

      watch(workId, (value) => {
        if (!lookup.id) lookup.id = value;
      }, { immediate: true });

      async function saveCover(sourceUrl) {
        if (!canEdit.value || coverSaveState.value === "saving" || !props.saveCoverThumbnail) return;
        coverSaveState.value = "saving";
        try {
          await props.saveCoverThumbnail(props.entry.path, sourceUrl);
          coverSaveState.value = "saved";
        } catch {
          coverSaveState.value = "idle";
        }
      }

      async function applyCandidate(candidate) {
        if (!canEdit.value || !Object.keys(candidate.patch).length) return;
        await props.saveMetadata(props.entry, candidate.patch);
      }

      async function importCandidate() {
        if (!canEdit.value) return;
        const result = parseCandidateJson(importDraft.value);
        if (!result.ok) {
          error.value = result.error;
          return;
        }
        error.value = "";
        await props.saveMetadata(props.entry, {
          providerCandidates: appendMetadataCandidate(props.entry.metadata, result.candidate),
        });
        importDraft.value = "";
        importOpen.value = false;
      }

      async function lookupCandidate() {
        if (!canEdit.value || lookup.isLoading) return;
        const id = (lookup.id || workId.value).trim();
        if (!id) {
          error.value = "缺少作品 ID";
          return;
        }
        lookup.isLoading = true;
        error.value = "";
        try {
          const response = await ctx.callPlugin({
            pluginId: lookup.provider,
            method: "provider.lookupMetadataCandidate",
            payload: { id },
          });
          await props.saveMetadata(props.entry, {
            providerCandidates: appendMetadataCandidate(props.entry.metadata, response.payload.candidate ?? response.payload),
          });
        } catch (cause) {
          error.value = cause instanceof Error ? cause.message : String(cause);
        } finally {
          lookup.isLoading = false;
        }
      }

      return () => h("div", [
        rows.value.length ? h("section", { class: "file-metadata-card__source file-metadata-card__library", "aria-label": "ASMR 信息" }, [
          h("div", { class: "file-metadata-card__source-head" }, [
            h("div", [
              h("p", { class: "asset-browser__eyebrow" }, "ASMR Metadata"),
              h("strong", "作品信息"),
            ]),
            coverUrl.value && props.saveCoverThumbnail ? h("button", {
              type: "button",
              class: "ghost",
              disabled: !canEdit.value || coverSaveState.value === "saving",
              onClick: () => saveCover(coverUrl.value),
            }, coverSaveState.value === "saving" ? "保存中" : "保存封面") : null,
          ]),
          h("div", { class: "file-metadata-card__source-grid" }, rows.value.map((row) => (
            h("div", { key: row.key, class: "asset-meta__row file-metadata-card__source-row" }, [
              h("span", row.label),
              h("span", { class: "asset-meta__value" }, row.value),
            ])
          ))),
        ]) : null,
        h("section", { class: "file-metadata-card__source file-metadata-card__library-candidates", "aria-label": "ASMR 元数据候选" }, [
          h("div", { class: "file-metadata-card__source-head" }, [
            h("div", [
              h("p", { class: "asset-browser__eyebrow" }, "ASMR Provider"),
              h("strong", "补全候选"),
            ]),
            h("button", {
              type: "button",
              class: "ghost",
              disabled: !canEdit.value,
              onClick: () => {
                importOpen.value = !importOpen.value;
                error.value = "";
              },
            }, "导入"),
          ]),
          importOpen.value ? h("div", { class: "file-metadata-card__candidate-import" }, [
            h("div", { class: "file-metadata-card__provider-lookup" }, [
              h("select", {
                value: lookup.provider,
                disabled: !canEdit.value || lookup.isLoading,
                "aria-label": "ASMR Provider",
                onChange: (event) => {
                  lookup.provider = event.target.value;
                },
              }, [
                h("option", { value: "momobako.service.provider.dlsite" }, "DLsite"),
                h("option", { value: "momobako.service.provider.asmr-one" }, "ASMR One"),
              ]),
              h("input", {
                value: lookup.id,
                type: "text",
                "aria-label": "作品 ID",
                placeholder: "RJ123456",
                disabled: !canEdit.value || lookup.isLoading,
                onInput: (event) => {
                  lookup.id = event.target.value;
                },
              }),
              h("button", {
                type: "button",
                class: "ghost",
                disabled: !canEdit.value || lookup.isLoading,
                onClick: lookupCandidate,
              }, lookup.isLoading ? "抓取中" : "抓取候选"),
            ]),
            h("textarea", {
              value: importDraft.value,
              "aria-label": "ASMR 候选 JSON",
              disabled: !canEdit.value,
              onInput: (event) => {
                importDraft.value = event.target.value;
              },
            }),
            h("div", [
              h("span", error.value),
              h("button", { type: "button", class: "ghost", disabled: !canEdit.value, onClick: importCandidate }, "导入候选"),
            ]),
          ]) : null,
          candidates.value.length ? h("div", { class: "file-metadata-card__candidate-list" }, candidates.value.map((candidate, index) => (
            h("article", { key: `${candidate.source}-${candidate.confidence}-${index}`, class: "file-metadata-card__candidate" }, [
              h("header", [
                h("span", [
                  h("strong", candidate.source),
                  h("small", candidate.confidence),
                ]),
                h("span", { class: "file-metadata-card__candidate-actions" }, [
                  candidate.coverUrl && props.saveCoverThumbnail ? h("button", {
                    type: "button",
                    class: "ghost",
                    disabled: !canEdit.value || coverSaveState.value === "saving",
                    onClick: () => saveCover(candidate.coverUrl),
                  }, "封面") : null,
                  h("button", {
                    type: "button",
                    class: "ghost",
                    disabled: !canEdit.value || !candidate.fields.length,
                    onClick: () => applyCandidate(candidate),
                  }, "应用"),
                ]),
              ]),
              candidate.fields.length ? h("div", { class: "file-metadata-card__candidate-fields" }, candidate.fields.map((field) => (
                h("span", { key: field.key }, `${field.key}=${field.value}`)
              ))) : null,
              candidate.skipped.length ? h("small", { class: "file-metadata-card__candidate-skipped" }, `跳过 ${candidate.skipped.join("，")}`) : null,
            ])
          ))) : !importOpen.value ? h("small", { class: "file-metadata-card__candidate-skipped" }, "暂无候选") : null,
        ]),
      ]);
    },
  };
}

function createPreviewPanel(ctx) {
  const { h, computed, ref } = ctx.vue;
  return {
    name: "AsmrPreviewPanel",
    props: ["repoId", "entry", "entries", "previewEntry"],
    setup(props) {
      const playlistItems = ref(readStoredPlaylist());
      const queueEntries = computed(() => workAudioEntries(props.entry, props.entries ?? []).filter((item) => item.path !== props.entry.path));
      const activeEntryByPath = computed(() => new Map((props.entries ?? []).map((entry) => [entry.path, entry])));
      const activePlaylist = computed(() => resolveActivePlaylist(playlistItems.value, props.repoId, activeEntryByPath.value));
      const isAudio = computed(() => isAsmrAudioEntry(props.entry));

      function updatePlaylist(next) {
        playlistItems.value = next;
        persistPlaylist(next);
      }

      function addEntries(entries) {
        updatePlaylist(mergePlaylistEntries(playlistItems.value, props.repoId, entries));
      }

      function addWork() {
        addEntries(workAudioEntries(props.entry, props.entries ?? []));
      }

      function addRandom() {
        const workEntries = workAudioEntries(props.entry, props.entries ?? []);
        const candidates = workEntries.filter((item) => !hasPlaylistEntry(playlistItems.value, props.repoId, item.path));
        const pool = candidates.length ? candidates : workEntries;
        const selected = pool[Math.floor(Math.random() * pool.length)];
        if (selected) addEntries([selected]);
      }

      function clear() {
        updatePlaylist(clearPlaylistForRepo(playlistItems.value, props.repoId));
      }

      function previewPath(path) {
        const entry = activeEntryByPath.value.get(path);
        if (entry) props.previewEntry?.(entry);
      }

      return () => h("div", [
        queueEntries.value.length ? h("section", { class: "files-preview-page__queue", "aria-label": "ASMR 作品队列" }, [
          h("div", { class: "files-preview-page__queue-head" }, [
            h("span", "作品队列"),
            h("strong", `${queueEntries.value.length + 1} 轨`),
          ]),
          h("div", { class: "files-preview-page__queue-list" }, queueEntries.value.map((entry) => (
            h("button", {
              key: entry.path,
              type: "button",
              class: "files-preview-page__queue-item",
              onClick: () => props.previewEntry?.(entry),
            }, [
              h("span", metadataText(entry, "trackTitle") || entry.name),
              h("small", metadataText(entry, "listeningStatus") || "unlistened"),
            ])
          ))),
        ]) : null,
        isAudio.value || activePlaylist.value.length ? h("section", { class: "files-preview-page__queue", "aria-label": "ASMR 播放列表" }, [
          h("div", { class: "files-preview-page__queue-head" }, [
            h("span", "播放列表"),
            h("strong", `${activePlaylist.value.length} 项`),
          ]),
          h("div", { class: "files-preview-page__queue-actions" }, [
            h("button", { type: "button", class: "ghost", disabled: !isAudio.value, onClick: addWork }, "加入作品"),
            h("button", { type: "button", class: "ghost", disabled: !isAudio.value, onClick: addRandom }, "随机"),
            h("button", { type: "button", class: "ghost", disabled: !activePlaylist.value.length, onClick: clear }, "清空"),
          ]),
          activePlaylist.value.length ? h("div", { class: "files-preview-page__queue-list" }, activePlaylist.value.map((item) => (
            h("button", {
              key: item.path,
              type: "button",
              class: ["files-preview-page__queue-item", item.path === props.entry.path ? "is-active" : ""],
              onClick: () => previewPath(item.path),
            }, [
              h("span", item.title),
              h("small", item.status || item.workTitle),
            ])
          ))) : h("div", { class: "files-preview-page__queue-empty" }, "暂无播放队列"),
        ]) : null,
      ]);
    },
  };
}

export function register(ctx) {
  const progressSaves = new Map();
  ctx.onPluginEvent("media.playback", async (event) => {
    if (!shouldPersistListeningProgress(event, progressSaves.get(`${event.repoId}\u0000${event.entry?.path}`))) return;
    const metadata = event.state === "metadata"
      ? { trackDurationMs: Math.max(0, Math.round(Number(event.durationMs) || 0)) }
      : buildListeningProgressMetadata(event);
    if (event.state !== "metadata") {
      progressSaves.set(`${event.repoId}\u0000${event.entry.path}`, {
        savedAtMs: Date.now(),
        savedSecond: Math.floor((Math.max(0, Number(event.currentTimeMs) || 0)) / 1000),
      });
    }
    await event.saveMetadata?.(event.entry, metadata);
  });

  ctx.registerLibraryExtension({
    libraryKind: "asmr",
    label: "ASMR",
    matchEntry: matchAsmrEntry,
    fileSummary,
    metadataPanel: createMetadataPanel(ctx),
    previewPanel: createPreviewPanel(ctx),
    searchShortcuts: [
      { id: "works", label: "ASMR 作品", metadataFilters: "libraryKind=asmr", sort: { field: "metadata.workId", direction: "asc" } },
      { id: "lyrics", label: "含歌词", metadataFilters: "libraryKind=asmr\nlyricStatus=local" },
      { id: "continue", label: "继续收听", metadataFilters: "libraryKind=asmr\nlisteningStatus=listening", sort: { field: "metadata.lastListenedAt", direction: "desc" } },
      { id: "random", label: "随机一首", metadataFilters: "libraryKind=asmr", sort: { field: "random", direction: "asc" } },
    ],
  });
}
