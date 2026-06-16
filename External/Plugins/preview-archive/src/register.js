const archivePreviewExtensions = ["zip", "cbz", "7z", "rar", "cbr"];
const imageExtensions = ["png", "jpg", "jpeg", "webp", "gif", "bmp", "avif", "svg"];
const videoExtensions = ["mp4", "mov", "mkv", "webm", "avi", "m4v"];
const audioExtensions = ["mp3", "wav", "ogg", "flac", "m4a", "aac", "opus"];
const textExtensions = [
  "txt",
  "text",
  "log",
  "csv",
  "tsv",
  "json",
  "jsonl",
  "yaml",
  "yml",
  "toml",
  "xml",
  "html",
  "css",
  "scss",
  "sass",
  "less",
  "js",
  "jsx",
  "ts",
  "tsx",
  "vue",
  "rs",
  "py",
  "rb",
  "go",
  "java",
  "c",
  "h",
  "cpp",
  "hpp",
  "cs",
  "php",
  "sh",
  "bash",
  "zsh",
  "ps1",
  "bat",
  "cmd",
  "ini",
  "cfg",
  "conf",
  "env",
  "gitignore",
  "gitattributes",
  "md",
  "markdown",
  "mdown",
  "mkd",
  "mkdn",
  "mdx",
];

const archiveServicePluginId = "momobako.service.archive-preview";
const textByteLimit = 768 * 1024;

export function register(ctx) {
  ensureArchivePreviewStyles();

  const {
    computed,
    h,
    onBeforeUnmount,
    ref,
    watch,
  } = ctx.vue;

  const ArchivePreviewPlugin = {
    name: "ArchivePreviewPlugin",
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
      const errorMessage = ref("");
      const archivePath = ref("");
      const currentDirectory = ref("");
      const entries = ref([]);
      const selectedEntry = ref(null);
      const selectedPreview = ref(null);
      const previewState = ref("idle");
      const previewError = ref("");
      const textContent = ref("");
      let loadToken = 0;
      let previewToken = 0;
      let objectUrl = null;

      const breadcrumbs = computed(() => {
        const parts = currentDirectory.value.split("/").filter(Boolean);
        const segments = [{ label: props.entry?.name ?? "压缩包", path: "" }];
        let path = "";
        for (const part of parts) {
          path = path ? `${path}/${part}` : part;
          segments.push({ label: part, path });
        }
        return segments;
      });

      const directoryLabel = computed(() => currentDirectory.value || "根目录");

      watch(
        () => [props.repoId, props.entry?.path],
        () => {
          void loadArchive();
        },
        { immediate: true },
      );

      onBeforeUnmount(() => {
        revokeObjectUrl();
      });

      function revokeObjectUrl() {
        if (objectUrl) {
          URL.revokeObjectURL(objectUrl);
          objectUrl = null;
        }
      }

      async function callArchive(method, payload) {
        const response = await ctx.callPlugin({
          pluginId: archiveServicePluginId,
          method,
          payload,
        });
        return response.payload;
      }

      async function loadArchive() {
        const token = ++loadToken;
        state.value = "loading";
        errorMessage.value = "";
        archivePath.value = "";
        currentDirectory.value = "";
        entries.value = [];
        selectedEntry.value = null;
        selectedPreview.value = null;
        previewState.value = "idle";
        textContent.value = "";
        revokeObjectUrl();

        try {
          const source = await ctx.preparePreviewFileSource({
            repoId: props.repoId,
            path: props.entry.path,
          });
          const localPath = source.localPath;
          if (!localPath) {
            throw new Error("压缩包预览需要本地文件路径。");
          }
          await callArchive("archive.ensurePrepared", { archivePath: localPath });
          if (token !== loadToken) return;
          archivePath.value = localPath;
          await loadDirectory("");
          state.value = "ready";
        } catch (cause) {
          if (token !== loadToken) return;
          state.value = "error";
          errorMessage.value = cause instanceof Error ? cause.message : String(cause);
        }
      }

      async function loadDirectory(path) {
        const normalized = normalizeInternalPath(path);
        state.value = archivePath.value ? "ready" : "loading";
        errorMessage.value = "";
        selectedEntry.value = null;
        selectedPreview.value = null;
        previewState.value = "idle";
        textContent.value = "";
        revokeObjectUrl();
        const result = await callArchive("archive.listDirectory", {
          archivePath: archivePath.value,
          directoryPath: normalized,
        });
        entries.value = Array.isArray(result) ? result : [];
        currentDirectory.value = normalized;
      }

      async function openEntry(entry) {
        if (entry.kind === "directory") {
          await loadDirectory(entry.path);
          return;
        }
        selectEntry(entry);
        await prepareEntryPreview(entry);
      }

      function selectEntry(entry) {
        selectedEntry.value = entry;
        if (entry.kind === "directory") {
          selectedPreview.value = null;
          previewState.value = "idle";
          previewError.value = "";
          textContent.value = "";
          revokeObjectUrl();
        }
      }

      async function openFile(entry) {
        selectEntry(entry);
        await prepareEntryPreview(entry);
      }

      async function prepareEntryPreview(entry) {
        const token = ++previewToken;
        previewState.value = "loading";
        previewError.value = "";
        selectedPreview.value = null;
        textContent.value = "";
        revokeObjectUrl();

        try {
          const preview = await callArchive("archive.prepareEntryPreview", {
            archivePath: archivePath.value,
            entryPath: entry.path,
          });
          if (token !== previewToken) return;
          selectedPreview.value = preview;
          const sourceUrl = preview.sourceUrl || (preview.localPath ? ctx.fileSrc(preview.localPath) : "");
          const kind = previewKind(entry.extension, preview.mediaType);
          if (kind === "text" && sourceUrl) {
            textContent.value = await fetchTextPreview(sourceUrl, preview.sizeBytes);
          } else if (!sourceUrl && kind !== "binary") {
            throw new Error("内部文件预览源不可用");
          }
          previewState.value = "ready";
        } catch (cause) {
          if (token !== previewToken) return;
          previewState.value = "error";
          previewError.value = cause instanceof Error ? cause.message : String(cause);
        }
      }

      return {
        breadcrumbs,
        currentDirectory,
        directoryLabel,
        entries,
        errorMessage,
        fileSrc: ctx.fileSrc,
        loadDirectory,
        openFile,
        openEntry,
        previewError,
        previewState,
        selectEntry,
        selectedEntry,
        selectedPreview,
        state,
        textContent,
      };
    },
    render() {
      if (this.state === "loading") {
        return h("div", { class: "archive-preview archive-preview--status" }, [
          h("span", "读取压缩包"),
          h("span", this.entry?.sizeLabel ? `准备 ${this.entry.sizeLabel}` : "准备内部目录"),
        ]);
      }

      if (this.state === "error") {
        return h("div", { class: "archive-preview archive-preview--error" }, [
          h("strong", "无法预览该压缩包"),
          h("span", this.errorMessage),
        ]);
      }

      return h("div", { class: "archive-preview" }, [
        h("header", { class: "archive-preview__toolbar" }, [
          h("nav", { class: "archive-preview__breadcrumbs", "aria-label": "压缩包路径" }, this.breadcrumbs.map((segment, index) => h("button", {
            key: segment.path || "__root",
            type: "button",
            class: ["archive-preview__crumb", { "is-current": index === this.breadcrumbs.length - 1 }],
            disabled: index === this.breadcrumbs.length - 1,
            onClick: () => this.loadDirectory(segment.path),
          }, segment.label))),
          h("span", { class: "archive-preview__count" }, `${this.entries.length} 项`),
        ]),
        h("div", { class: "archive-preview__body" }, [
          h("section", { class: "archive-preview__list", "aria-label": "压缩包目录" }, [
            this.entries.length
              ? this.entries.map((entry) => h("button", {
                  key: entry.path,
                  type: "button",
                  class: [
                    "archive-preview__entry",
                    {
                      "is-directory": entry.kind === "directory",
                      "is-selected": this.selectedEntry?.path === entry.path,
                    },
                  ],
                  onClick: () => entry.kind === "directory" ? this.selectEntry(entry) : this.openFile(entry),
                  onDblclick: () => this.openEntry(entry),
                }, [
                  h("span", { class: "archive-preview__entry-icon" }, entry.kind === "directory" ? "DIR" : fileKindLabel(entry.extension)),
                  h("span", { class: "archive-preview__entry-main" }, [
                    h("span", { class: "archive-preview__entry-name" }, entry.name),
                    entry.nestedDepthExceeded
                      ? h("span", { class: "archive-preview__entry-note" }, "嵌套深度已达上限")
                      : h("span", { class: "archive-preview__entry-note" }, entry.kind === "directory" ? "目录" : formatByteCount(entry.sizeBytes)),
                  ]),
                ]))
              : h("div", { class: "archive-preview__empty" }, "空目录"),
          ]),
          h("section", { class: "archive-preview__stage", "aria-label": "内部文件预览" }, [
            renderInternalPreview(h, this),
          ]),
        ]),
      ]);
    },
  };

  ctx.registerPreview({
    supportedExtensions: archivePreviewExtensions,
    component: ArchivePreviewPlugin,
  });
}

function ensureArchivePreviewStyles() {
  if (typeof document === "undefined" || document.getElementById("momobako-preview-archive-styles")) return;
  const style = document.createElement("style");
  style.id = "momobako-preview-archive-styles";
  style.textContent = `
.archive-preview { display: flex; flex-direction: column; width: 100%; height: 100%; min-height: 0; color: var(--text-primary, #e5e7eb); background: var(--surface-panel, #111827); }
.archive-preview--status, .archive-preview--error { align-items: center; justify-content: center; gap: 8px; text-align: center; }
.archive-preview--error strong { color: var(--danger, #ef4444); }
.archive-preview__toolbar { display: flex; align-items: center; justify-content: space-between; gap: 12px; min-height: 42px; padding: 0 12px; border-bottom: 1px solid var(--border-subtle, rgba(148, 163, 184, 0.2)); }
.archive-preview__breadcrumbs { display: flex; min-width: 0; overflow: hidden; }
.archive-preview__crumb { border: 0; background: transparent; color: inherit; font: inherit; padding: 5px 7px; border-radius: 6px; max-width: 180px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; cursor: pointer; }
.archive-preview__crumb:not(:disabled):hover { background: var(--surface-hover, rgba(148, 163, 184, 0.14)); }
.archive-preview__crumb:disabled { opacity: 0.68; cursor: default; }
.archive-preview__count { color: var(--text-muted, #94a3b8); font-size: 12px; white-space: nowrap; }
.archive-preview__body { display: grid; grid-template-columns: minmax(220px, 0.42fr) minmax(260px, 1fr); min-height: 0; flex: 1; }
.archive-preview__list { min-height: 0; overflow: auto; border-right: 1px solid var(--border-subtle, rgba(148, 163, 184, 0.2)); padding: 8px; }
.archive-preview__entry { display: grid; grid-template-columns: 42px minmax(0, 1fr); align-items: center; width: 100%; min-height: 42px; border: 0; border-radius: 6px; background: transparent; color: inherit; text-align: left; cursor: pointer; }
.archive-preview__entry:hover, .archive-preview__entry.is-selected { background: var(--surface-hover, rgba(148, 163, 184, 0.14)); }
.archive-preview__entry-icon { justify-self: center; min-width: 30px; font-size: 10px; font-weight: 700; color: var(--text-muted, #94a3b8); }
.archive-preview__entry-main { min-width: 0; display: flex; flex-direction: column; gap: 2px; }
.archive-preview__entry-name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 13px; }
.archive-preview__entry-note { color: var(--text-muted, #94a3b8); font-size: 11px; }
.archive-preview__empty, .archive-preview__placeholder { height: 100%; min-height: 160px; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 6px; color: var(--text-muted, #94a3b8); text-align: center; }
.archive-preview__placeholder strong { color: var(--text-primary, #e5e7eb); font-size: 13px; }
.archive-preview__placeholder--error strong { color: var(--danger, #ef4444); }
.archive-preview__stage { min-width: 0; min-height: 0; overflow: auto; padding: 12px; }
.archive-preview__media { width: 100%; height: 100%; min-height: 220px; display: flex; align-items: center; justify-content: center; }
.archive-preview__media img, .archive-preview__media video { max-width: 100%; max-height: 100%; object-fit: contain; }
.archive-preview__media audio { width: min(520px, 100%); }
.archive-preview__audio-title { margin-bottom: 12px; color: var(--text-muted, #94a3b8); }
.archive-preview__text { width: 100%; height: 100%; min-height: 0; overflow: auto; border: 1px solid var(--border-subtle, rgba(148, 163, 184, 0.2)); border-radius: 6px; background: var(--surface-elevated, rgba(15, 23, 42, 0.7)); }
.archive-preview__text pre { margin: 0; padding: 12px; white-space: pre-wrap; overflow-wrap: anywhere; font-size: 12px; line-height: 1.6; }
@media (max-width: 760px) { .archive-preview__body { grid-template-columns: 1fr; grid-template-rows: minmax(180px, 0.48fr) minmax(220px, 1fr); } .archive-preview__list { border-right: 0; border-bottom: 1px solid var(--border-subtle, rgba(148, 163, 184, 0.2)); } }
`;
  document.head.appendChild(style);
}

function renderInternalPreview(h, state) {
  const entry = state.selectedEntry;
  if (!entry) {
    return h("div", { class: "archive-preview__placeholder" }, [
      h("strong", state.directoryLabel),
      h("span", "选择内部文件进行预览"),
    ]);
  }
  if (state.previewState === "loading") {
    return h("div", { class: "archive-preview__placeholder" }, [
      h("strong", "准备内部文件"),
      h("span", entry.path),
    ]);
  }
  if (state.previewState === "error") {
    return h("div", { class: "archive-preview__placeholder archive-preview__placeholder--error" }, [
      h("strong", "无法预览内部文件"),
      h("span", state.previewError),
    ]);
  }
  const preview = state.selectedPreview;
  const url = preview?.sourceUrl || (preview?.localPath ? state.fileSrc(preview.localPath) : "");
  const kind = previewKind(entry.extension, preview?.mediaType);

  if (kind === "image" && url) {
    return h("div", { class: "archive-preview__media archive-preview__media--image" }, [
      h("img", { src: url, alt: "" }),
    ]);
  }
  if (kind === "video" && url) {
    return h("div", { class: "archive-preview__media archive-preview__media--video" }, [
      h("video", { controls: true, preload: "metadata", src: url }),
    ]);
  }
  if (kind === "audio" && url) {
    return h("div", { class: "archive-preview__media archive-preview__media--audio" }, [
      h("div", { class: "archive-preview__audio-title" }, entry.name),
      h("audio", { controls: true, preload: "metadata", src: url }),
    ]);
  }
  if (kind === "text") {
    return h("div", { class: "archive-preview__text" }, [
      h("pre", state.textContent || "空文件"),
    ]);
  }
  return h("div", { class: "archive-preview__placeholder" }, [
    h("strong", entry.name),
    h("span", preview?.mediaType || "无法内联预览该文件类型"),
  ]);
}

function normalizeInternalPath(path) {
  return String(path ?? "").replace(/\\/g, "/").replace(/^\/+|\/+$/g, "");
}

function previewKind(extension, mediaType = "") {
  const ext = String(extension ?? "").toLowerCase();
  const type = String(mediaType ?? "").toLowerCase();
  if (imageExtensions.includes(ext) || type.startsWith("image/")) return "image";
  if (videoExtensions.includes(ext) || type.startsWith("video/")) return "video";
  if (audioExtensions.includes(ext) || type.startsWith("audio/")) return "audio";
  if (textExtensions.includes(ext) || type.startsWith("text/") || type === "application/json") return "text";
  return "binary";
}

function fileKindLabel(extension) {
  return String(extension || "FILE").slice(0, 4).toUpperCase();
}

function formatByteCount(value) {
  const size = Number(value ?? 0);
  if (!Number.isFinite(size) || size <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  let current = size;
  let unitIndex = 0;
  while (current >= 1024 && unitIndex < units.length - 1) {
    current /= 1024;
    unitIndex += 1;
  }
  return `${current >= 10 || unitIndex === 0 ? current.toFixed(0) : current.toFixed(1)} ${units[unitIndex]}`;
}

async function fetchTextPreview(sourceUrl, sizeBytes) {
  const headers = {};
  if (Number.isFinite(sizeBytes) && sizeBytes > textByteLimit) {
    headers.Range = `bytes=0-${textByteLimit - 1}`;
  }
  const response = await fetch(sourceUrl, { headers });
  if (!response.ok && response.status !== 206) {
    throw new Error(`读取文本失败：${response.status}`);
  }
  const bytes = new Uint8Array(await response.arrayBuffer());
  return decodeTextBytes(bytes);
}

function decodeTextBytes(bytes) {
  const bom = bytes.slice(0, 3);
  const offset = bom[0] === 0xef && bom[1] === 0xbb && bom[2] === 0xbf ? 3 : 0;
  return new TextDecoder("utf-8", { fatal: false }).decode(bytes.slice(offset));
}
