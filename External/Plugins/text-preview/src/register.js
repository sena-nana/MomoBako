const markdownPreviewExtensions = [
  "md",
  "markdown",
  "mdown",
  "mkd",
  "mkdn",
  "mdx",
];

const textPreviewExtensions = [
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
  ...markdownPreviewExtensions,
];

const TEXT_PREVIEW_BYTE_LIMIT = 768 * 1024;
const TEXT_THUMBNAIL_BYTE_LIMIT = 24 * 1024;
const THUMBNAIL_SIZE = 512;
const THUMBNAIL_LINE_LIMIT = 14;

export function register(ctx) {
  const {
    computed,
    h,
    nextTick,
    ref,
    watch,
  } = ctx.vue;

  const TextPreviewPlugin = {
    name: "TextPreviewPlugin",
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
      const content = ref("");
      const previewInfo = ref(null);
      const errorMessage = ref("");
      let loadToken = 0;

      const isMarkdown = computed(() => isMarkdownExtension(props.entry?.extension));
      const extensionLabel = computed(() => props.entry?.extension?.toUpperCase() || "TEXT");
      const lineCount = computed(() => (
        content.value ? content.value.replace(/\r\n?/g, "\n").split("\n").length : 0
      ));
      const truncatedSizeLabel = computed(() => {
        const info = previewInfo.value;
        if (!info?.truncated) return "";
        return `仅显示前 ${formatByteCount(info.bytesRead)}`;
      });

      watch(
        [() => props.repoId, () => props.entry?.path],
        () => {
          void loadText();
        },
        { immediate: true },
      );

      async function loadText() {
        const token = ++loadToken;
        state.value = "loading";
        content.value = "";
        previewInfo.value = null;
        errorMessage.value = "";

        try {
          const nextContent = await loadTextPreviewContent(ctx, props.repoId, props.entry.path);
          if (token !== loadToken) return;
          content.value = nextContent.text;
          previewInfo.value = nextContent;
          state.value = "ready";
          void persistTextThumbnail(token);
        } catch (cause) {
          if (token !== loadToken) return;
          state.value = "error";
          errorMessage.value = cause instanceof Error ? cause.message : String(cause);
        }
      }

      async function persistTextThumbnail(token) {
        await nextTick();
        if (token !== loadToken) return;
        const thumbnail = await generateTextThumbnailFromContent(props.entry, content.value);
        if (token !== loadToken || !thumbnail) return;
        await ctx.saveGeneratedThumbnail({
          repoId: props.repoId,
          path: props.entry.path,
          imageBytes: thumbnail.bytes,
          mediaType: thumbnail.mediaType,
        });
      }

      return {
        content,
        entry: props.entry,
        errorMessage,
        extensionLabel,
        isMarkdown,
        lineCount,
        state,
        truncatedSizeLabel,
      };
    },
    render() {
      if (this.state === "loading") {
        return h("div", { class: "text-preview__status" }, [
          h("span", "读取文本"),
          h("span", this.entry?.sizeLabel ? `准备 ${this.entry.sizeLabel}` : "准备文本内容"),
        ]);
      }

      if (this.state === "error") {
        return h("div", { class: "text-preview__overlay text-preview__overlay--error" }, [
          h("strong", "无法预览该文本"),
          h("span", this.errorMessage),
        ]);
      }

      return h("div", { class: ["text-preview", { "text-preview--markdown": this.isMarkdown }] }, [
        h("div", { class: "text-preview__toolbar" }, [
          h("span", { class: "text-preview__kind" }, this.isMarkdown ? "Markdown" : this.extensionLabel),
          h("span", `${this.lineCount} 行`),
          this.truncatedSizeLabel ? h("span", this.truncatedSizeLabel) : null,
        ].filter(Boolean)),
        h("div", { class: "text-preview__content" }, [
          this.isMarkdown
            ? h("pre", { class: "text-preview__plain text-preview__plain--markdown" }, this.content || "空文件")
            : h("pre", { class: "text-preview__plain" }, this.content || "空文件"),
        ]),
      ]);
    },
  };

  ctx.registerPreview({
    supportedExtensions: textPreviewExtensions,
    component: TextPreviewPlugin,
    generateThumbnail: async ({ repoId, entry }) => {
      const content = await loadTextPreviewContent(ctx, repoId, entry.path, TEXT_THUMBNAIL_BYTE_LIMIT);
      return generateTextThumbnailFromContent(entry, content.text);
    },
  });
}

async function loadTextPreviewContent(ctx, repoId, path, byteLimit = TEXT_PREVIEW_BYTE_LIMIT) {
  try {
    const source = await ctx.preparePreviewFileSource({ repoId, path });
    if (source.sizeBytes <= 0) {
      return {
        text: "",
        truncated: false,
        sizeBytes: source.sizeBytes,
        bytesRead: 0,
        mediaType: source.mediaType,
        modifiedAt: source.modifiedAt,
      };
    }
    if (!source.sourceUrl) {
      throw new Error("文本预览源不可用");
    }

    const bytes = await fetchPreviewBytes(source.sourceUrl, source.sizeBytes, byteLimit);
    return {
      text: decodeTextBytes(bytes),
      truncated: source.sizeBytes > bytes.byteLength,
      sizeBytes: source.sizeBytes,
      bytesRead: bytes.byteLength,
      mediaType: source.mediaType,
      modifiedAt: source.modifiedAt,
    };
  } catch {
    const bytes = await ctx.readFile({ repoId, path });
    const previewBytes = Uint8Array.from(bytes.slice(0, byteLimit));
    return {
      text: decodeTextBytes(previewBytes),
      truncated: bytes.length > previewBytes.byteLength,
      sizeBytes: bytes.length,
      bytesRead: previewBytes.byteLength,
      mediaType: "text/plain",
      modifiedAt: null,
    };
  }
}

async function fetchPreviewBytes(sourceUrl, sizeBytes, byteLimit) {
  const end = Math.max(Math.min(sizeBytes, byteLimit) - 1, 0);
  const response = await fetch(sourceUrl, {
    headers: {
      Range: `bytes=0-${end}`,
    },
  });
  if (!response.ok) {
    throw new Error(`文本预览读取失败: ${response.status}`);
  }
  return new Uint8Array(await response.arrayBuffer()).slice(0, byteLimit);
}

function decodeTextBytes(bytes) {
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

function isMarkdownExtension(extension) {
  return markdownPreviewExtensions.includes(extension?.toLowerCase() ?? "");
}

async function generateTextThumbnailFromContent(entry, content) {
  if (typeof document === "undefined") return null;

  const canvas = document.createElement("canvas");
  canvas.width = THUMBNAIL_SIZE;
  canvas.height = THUMBNAIL_SIZE;
  const context = canvas.getContext("2d");
  if (!context) return null;

  const styles = getComputedStyle(document.documentElement);
  const color = (name, fallback) => styles.getPropertyValue(name).trim() || fallback;
  const background = color("--bg-elev", "#202020");
  const surface = color("--bg", "#181818");
  const surfaceMuted = color("--bg-subtle", "#1c1c1c");
  const border = color("--border", "#2a2a2a");
  const text = color("--text", "#dddddd");
  const muted = color("--text-muted", "#8a8a8a");
  const accent = color("--accent", "#7bb9f0");

  context.fillStyle = background;
  context.fillRect(0, 0, THUMBNAIL_SIZE, THUMBNAIL_SIZE);
  context.fillStyle = surface;
  roundRect(context, 28, 28, 456, 456, 18);
  context.fill();
  context.strokeStyle = border;
  context.lineWidth = 2;
  context.stroke();

  context.fillStyle = surfaceMuted;
  roundRect(context, 44, 44, 424, 64, 12);
  context.fill();
  context.fillStyle = accent;
  roundRect(context, 44, 44, 7, 64, 4);
  context.fill();

  context.fillStyle = text;
  context.font = "600 25px sans-serif";
  drawTrimmedText(context, entry.name, 66, 82, 288);

  const extension = (entry.extension || "text").toUpperCase();
  context.font = "700 16px sans-serif";
  const badgeWidth = Math.min(Math.max(context.measureText(extension).width + 26, 58), 120);
  context.fillStyle = accent;
  roundRect(context, 468 - badgeWidth, 58, badgeWidth, 30, 15);
  context.fill();
  context.fillStyle = "#0d1622";
  context.textAlign = "center";
  context.fillText(extension, 468 - badgeWidth / 2, 78);
  context.textAlign = "left";

  context.font = "16px ui-monospace, SFMono-Regular, Consolas, monospace";
  const lines = previewLines(content);
  let y = 144;
  lines.forEach((line, index) => {
    const lineNumber = String(index + 1).padStart(2, "0");
    context.fillStyle = muted;
    context.fillText(lineNumber, 50, y);
    context.fillStyle = text;
    drawTrimmedText(context, line || " ", 84, y, 372);
    y += 24;
  });

  const blob = await new Promise((resolve) => {
    canvas.toBlob(resolve, "image/jpeg", 0.88);
  });
  if (!blob) return null;
  return {
    bytes: Array.from(new Uint8Array(await blob.arrayBuffer())),
    mediaType: blob.type || "image/jpeg",
  };
}

function previewLines(content) {
  const normalized = content.replace(/\r\n?/g, "\n");
  const lines = normalized.split("\n").slice(0, THUMBNAIL_LINE_LIMIT);
  return lines.length ? lines : ["空文件"];
}

function drawTrimmedText(context, value, x, y, maxWidth) {
  if (context.measureText(value).width <= maxWidth) {
    context.fillText(value, x, y);
    return;
  }

  let next = value;
  while (next.length > 1 && context.measureText(`${next}...`).width > maxWidth) {
    next = next.slice(0, -1);
  }
  context.fillText(`${next}...`, x, y);
}

function roundRect(context, x, y, width, height, radius) {
  context.beginPath();
  context.moveTo(x + radius, y);
  context.arcTo(x + width, y, x + width, y + height, radius);
  context.arcTo(x + width, y + height, x, y + height, radius);
  context.arcTo(x, y + height, x, y, radius);
  context.arcTo(x, y, x + width, y, radius);
  context.closePath();
}

function formatByteCount(value) {
  if (value < 1024) return `${value} B`;
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`;
  return `${(value / 1024 / 1024).toFixed(1)} MB`;
}
