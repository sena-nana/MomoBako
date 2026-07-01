const pdfPreviewExtensions = ["pdf"];
const wordPreviewExtensions = ["docx", "docm", "doc", "dotx", "dotm", "dot"];
const spreadsheetPreviewExtensions = ["xlsx", "xlsm", "xlsb", "xls", "xltx", "xltm", "xlt"];
const presentationPreviewExtensions = ["pptx", "pptm", "ppt", "ppsx", "ppsm", "pps", "potx", "potm", "pot"];
const officePreviewExtensions = [
  ...pdfPreviewExtensions,
  ...wordPreviewExtensions,
  ...spreadsheetPreviewExtensions,
  ...presentationPreviewExtensions,
];

const OFFICE_XML_PREVIEW_BYTE_LIMIT = 8 * 1024 * 1024;
const THUMBNAIL_SIZE = 512;
const PREVIEW_LINE_LIMIT = 9;

export function register(ctx) {
  const {
    computed,
    h,
    nextTick,
    ref,
    watch,
  } = ctx.vue;

  const OfficePreviewPlugin = {
    name: "OfficePreviewPlugin",
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
      const documentPreview = ref(null);
      const sourceUrl = ref("");
      let loadToken = 0;

      const kind = computed(() => getOfficePreviewKind(props.entry?.extension));
      const kindLabel = computed(() => officeKindLabel(kind.value));
      const extensionLabel = computed(() => props.entry?.extension?.toUpperCase() || kindLabel.value.toUpperCase());
      const usesEmbeddedSource = computed(() => kind.value === "pdf");
      const statusDetail = computed(() => {
        if (usesEmbeddedSource.value && state.value === "ready") return "内联文档预览";
        if (documentPreview.value?.unsupported) return "文件信息";
        return documentPreview.value?.subtitle || props.entry?.sizeLabel || "准备文档";
      });

      watch(
        [() => props.repoId, () => props.entry?.path],
        () => {
          void loadPreview();
        },
        { immediate: true },
      );

      async function loadPreview() {
        const token = ++loadToken;
        state.value = "loading";
        errorMessage.value = "";
        documentPreview.value = null;
        sourceUrl.value = "";

        try {
          if (usesEmbeddedSource.value) {
            const source = await prepareOfficePreviewSource(ctx, props.repoId, props.entry.path);
            if (token !== loadToken) return;
            sourceUrl.value = source.sourceUrl;
            state.value = "ready";
            void persistOfficeThumbnailForEntry(token);
            return;
          }

          const document = await loadOfficePreviewDocument(ctx, props.repoId, props.entry);
          if (token !== loadToken) return;
          documentPreview.value = document;
          state.value = "ready";
          void persistOfficeThumbnail(token, document);
        } catch (cause) {
          if (token !== loadToken) return;
          state.value = "error";
          errorMessage.value = cause instanceof Error ? cause.message : String(cause);
        }
      }

      async function persistOfficeThumbnailForEntry(token) {
        await nextTick();
        if (token !== loadToken) return;
        const thumbnail = await generateOfficeThumbnailForEntry(ctx, props.repoId, props.entry);
        if (token !== loadToken || !thumbnail) return;
        await ctx.saveGeneratedThumbnail({
          repoId: props.repoId,
          path: props.entry.path,
          imageBytes: thumbnail.bytes,
          mediaType: thumbnail.mediaType,
        });
      }

      async function persistOfficeThumbnail(token, document) {
        await nextTick();
        if (token !== loadToken) return;
        const thumbnail = await generateOfficeThumbnailFromDocument(props.entry, document);
        if (token !== loadToken || !thumbnail) return;
        await ctx.saveGeneratedThumbnail({
          repoId: props.repoId,
          path: props.entry.path,
          imageBytes: thumbnail.bytes,
          mediaType: thumbnail.mediaType,
        });
      }

      return {
        documentPreview,
        entry: props.entry,
        errorMessage,
        extensionLabel,
        kind,
        kindLabel,
        sourceUrl,
        state,
        statusDetail,
        usesEmbeddedSource,
      };
    },
    render() {
      const toolbar = h("div", { class: "office-preview__toolbar" }, [
        h("span", { class: "office-preview__kind" }, this.kindLabel),
        h("span", this.extensionLabel),
        h("span", this.statusDetail),
      ]);

      if (this.state === "loading") {
        return h("div", { class: `office-preview office-preview--${this.kind}` }, [
          toolbar,
          h("div", { class: "office-preview__status" }, [
            h("span", "读取文档"),
            h("span", this.entry?.sizeLabel ? `准备 ${this.entry.sizeLabel}` : "建立预览"),
          ]),
        ]);
      }

      if (this.state === "error") {
        return h("div", { class: `office-preview office-preview--${this.kind}` }, [
          toolbar,
          h("div", { class: "office-preview__overlay office-preview__overlay--error" }, [
            h("strong", "无法预览该文档"),
            h("span", this.errorMessage),
          ]),
        ]);
      }

      if (this.usesEmbeddedSource && this.sourceUrl) {
        return h("div", { class: `office-preview office-preview--${this.kind}` }, [
          toolbar,
          h("div", { class: "office-preview__viewer office-preview__viewer--pdf" }, [
            h("iframe", {
              class: "office-preview__iframe",
              src: this.sourceUrl,
              title: this.entry?.name ?? "document-preview",
            }),
          ]),
        ]);
      }

      return h("div", { class: `office-preview office-preview--${this.kind}` }, [
        toolbar,
        this.documentPreview
          ? h("div", { class: "office-preview__document" }, [
              h("header", { class: "office-preview__document-head" }, [
                h("div", [
                  h("h2", this.documentPreview.title),
                  h("p", this.documentPreview.subtitle),
                ]),
              ]),
              h("div", { class: "office-preview__stats" }, this.documentPreview.stats.map((item) => (
                h("span", { key: `${item.label}:${item.value}` }, `${item.label}: ${item.value}`)
              ))),
              ...this.documentPreview.sections.map((section) => (
                h("section", { key: section.title, class: "office-preview__section" }, [
                  h("h3", section.title),
                  h("div", { class: "office-preview__rows" }, section.rows.map((row, rowIndex) => (
                    h("div", { key: `${section.title}:${rowIndex}`, class: "office-preview__row" }, row.map((cell, cellIndex) => (
                      h("span", { key: `${cellIndex}:${cell}` }, cell || " ")
                    )))
                  ))),
                ])
              )),
            ])
          : null,
      ]);
    },
  };

  ctx.registerPreview({
    supportedExtensions: officePreviewExtensions,
    component: OfficePreviewPlugin,
    generateThumbnail: async ({ repoId, entry }) => generateOfficeThumbnailForEntry(ctx, repoId, entry),
  });
}

async function prepareOfficePreviewSource(ctx, repoId, path) {
  const source = await ctx.preparePreviewFileSource({ repoId, path });
  if (!source.sourceUrl) {
    throw new Error("文档预览源不可用");
  }
  return source;
}

async function loadOfficePreviewDocument(ctx, repoId, entry) {
  const kind = getOfficePreviewKind(entry.extension);
  if (!isOpenXmlOfficeExtension(entry.extension)) {
    return createUnsupportedOfficeDocument(entry, kind);
  }

  try {
    const bytes = await loadOfficeBytes(ctx, repoId, entry.path);
    const zip = await loadZip(bytes);
    if (kind === "word") return parseWordDocument(zip, entry);
    if (kind === "spreadsheet") return parseSpreadsheetDocument(zip, entry);
    if (kind === "presentation") return parsePresentationDocument(zip, entry);
  } catch {
    return createUnsupportedOfficeDocument(entry, kind);
  }
  return createUnsupportedOfficeDocument(entry, kind);
}

async function loadOfficeBytes(ctx, repoId, path) {
  let source = null;
  try {
    source = await prepareOfficePreviewSource(ctx, repoId, path);
  } catch {
    const bytes = await ctx.readFile({ repoId, path });
    return Uint8Array.from(bytes).buffer;
  }

  if (source.sizeBytes > OFFICE_XML_PREVIEW_BYTE_LIMIT) {
    throw new Error("文档过大，已跳过结构预览");
  }

  try {
    const response = await fetch(source.sourceUrl);
    if (!response.ok) {
      throw new Error(`文档读取失败: ${response.status}`);
    }
    return await response.arrayBuffer();
  } catch {
    const bytes = await ctx.readFile({ repoId, path });
    if (bytes.length > OFFICE_XML_PREVIEW_BYTE_LIMIT) {
      throw new Error("文档过大，已跳过结构预览");
    }
    return Uint8Array.from(bytes).buffer;
  }
}

async function loadZip(bytes) {
  const { default: JSZip } = await import("jszip");
  return JSZip.loadAsync(bytes);
}

async function parseWordDocument(zip, entry) {
  const xml = await readZipText(zip, "word/document.xml");
  const rows = extractWordParagraphs(xml).slice(0, 36).map((line) => [line]);
  return {
    kind: "word",
    title: entry.name,
    subtitle: rows.length ? "文档正文预览" : "未读取到正文文本",
    sections: [{
      title: "正文",
      rows: rows.length ? rows : [["空白文档或暂不支持的内容结构"]],
    }],
    stats: [
      { label: "段落", value: String(rows.length) },
      { label: "格式", value: "DOCX" },
    ],
    unsupported: false,
  };
}

async function parseSpreadsheetDocument(zip, entry) {
  const sharedStrings = await readSharedStrings(zip);
  const workbookRels = await readWorkbookRelationships(zip);
  const sheetNames = await readWorkbookSheetNames(zip);
  const sheetPaths = sheetNames
    .map((sheet) => ({
      name: sheet.name,
      path: workbookRels.get(sheet.relationshipId) ?? `xl/worksheets/sheet${sheet.index + 1}.xml`,
    }))
    .slice(0, 3);
  const sections = [];

  for (const sheet of sheetPaths) {
    const xml = await readZipText(zip, sheet.path);
    const rows = parseWorksheetRows(xml, sharedStrings).slice(0, 12);
    sections.push({
      title: sheet.name,
      rows: rows.length ? rows : [["空工作表"]],
    });
  }

  return {
    kind: "spreadsheet",
    title: entry.name,
    subtitle: sections.length ? "工作表数据预览" : "未读取到工作表",
    sections: sections.length ? sections : [{ title: "工作表", rows: [["空工作簿或暂不支持的内容结构"]] }],
    stats: [
      { label: "工作表", value: String(sections.length) },
      { label: "格式", value: (entry.extension || "xlsx").toUpperCase() },
    ],
    unsupported: false,
  };
}

async function parsePresentationDocument(zip, entry) {
  const slidePaths = Object.keys(zip.files)
    .filter((path) => /^ppt\/slides\/slide\d+\.xml$/.test(path))
    .sort((left, right) => slideIndex(left) - slideIndex(right))
    .slice(0, 8);
  const sections = [];

  for (const path of slidePaths) {
    const xml = await readZipText(zip, path);
    const lines = extractXmlTextRuns(xml).slice(0, 12);
    sections.push({
      title: `幻灯片 ${slideIndex(path)}`,
      rows: lines.length ? lines.map((line) => [line]) : [["空白幻灯片"]],
    });
  }

  return {
    kind: "presentation",
    title: entry.name,
    subtitle: sections.length ? "幻灯片文本预览" : "未读取到幻灯片",
    sections: sections.length ? sections : [{ title: "幻灯片", rows: [["空演示文稿或暂不支持的内容结构"]] }],
    stats: [
      { label: "幻灯片", value: String(sections.length) },
      { label: "格式", value: (entry.extension || "pptx").toUpperCase() },
    ],
    unsupported: false,
  };
}

function createUnsupportedOfficeDocument(entry, kind) {
  const extension = (entry.extension || "office").toUpperCase();
  return {
    kind,
    title: entry.name,
    subtitle: "内容预览暂不可用",
    sections: [{
      title: "文件",
      rows: [
        ["名称", entry.name],
        ["类型", extension],
        ["大小", entry.sizeLabel || "未知"],
        ["修改时间", entry.modifiedAt ? new Date(entry.modifiedAt).toLocaleString("zh-CN") : "未记录"],
      ],
    }],
    stats: [
      { label: "格式", value: extension },
      { label: "预览", value: "文件信息" },
    ],
    unsupported: true,
  };
}

async function generateOfficeThumbnailForEntry(ctx, repoId, entry) {
  const kind = getOfficePreviewKind(entry.extension);
  if (kind === "pdf") {
    return generatePdfFallbackThumbnail(entry);
  }

  try {
    const document = await loadOfficePreviewDocument(ctx, repoId, entry);
    return generateOfficeThumbnailFromDocument(entry, document);
  } catch {
    return generateOfficeThumbnailFromDocument(entry, createFallbackDocument(entry, kind));
  }
}

async function generateOfficeThumbnailFromDocument(entry, previewDocument) {
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
  const accent = kindAccent(previewDocument.kind, color("--accent", "#7bb9f0"));
  const accentText = color("--accent-text", "#0d1622");

  context.fillStyle = background;
  context.fillRect(0, 0, THUMBNAIL_SIZE, THUMBNAIL_SIZE);
  context.fillStyle = surface;
  roundRect(context, 30, 28, 452, 456, 18);
  context.fill();
  context.strokeStyle = border;
  context.lineWidth = 2;
  context.stroke();

  context.fillStyle = surfaceMuted;
  roundRect(context, 48, 48, 416, 78, 12);
  context.fill();
  context.fillStyle = accent;
  roundRect(context, 48, 48, 8, 78, 4);
  context.fill();

  const extension = (entry.extension || officeKindLabel(previewDocument.kind)).toUpperCase();
  context.font = "700 16px sans-serif";
  const badgeWidth = Math.min(Math.max(context.measureText(extension).width + 26, 58), 132);
  context.fillStyle = accent;
  roundRect(context, 446 - badgeWidth, 64, badgeWidth, 32, 16);
  context.fill();
  context.fillStyle = accentText;
  context.textAlign = "center";
  context.fillText(extension, 446 - badgeWidth / 2, 85);
  context.textAlign = "left";

  context.fillStyle = text;
  context.font = "600 25px sans-serif";
  drawTrimmedText(context, entry.name, 70, 80, 250);
  context.fillStyle = muted;
  context.font = "14px sans-serif";
  drawTrimmedText(context, previewDocument.subtitle, 70, 104, 336);

  const lines = documentPreviewLines(previewDocument);
  let y = 164;
  context.font = "16px sans-serif";
  for (const line of lines.slice(0, PREVIEW_LINE_LIMIT)) {
    context.fillStyle = line.emphasis ? text : muted;
    if (line.rule) {
      context.strokeStyle = border;
      context.beginPath();
      context.moveTo(50, y - 11);
      context.lineTo(462, y - 11);
      context.stroke();
    }
    drawTrimmedText(context, line.text, 56, y, 400);
    y += line.emphasis ? 28 : 24;
  }

  if (previewDocument.unsupported) {
    context.fillStyle = color("--warn-soft", "rgba(212, 168, 91, 0.16)");
    roundRect(context, 54, 418, 404, 34, 8);
    context.fill();
    context.fillStyle = color("--warn", "#d4a85b");
    context.font = "600 14px sans-serif";
    context.fillText("文件级预览", 72, 440);
  }

  const blob = await new Promise((resolve) => {
    canvas.toBlob(resolve, "image/jpeg", 0.88);
  });
  if (!blob) return null;
  return {
    bytes: Array.from(new Uint8Array(await blob.arrayBuffer())),
    mediaType: blob.type || "image/jpeg",
  };
}

async function generatePdfFallbackThumbnail(entry) {
  return generateOfficeThumbnailFromDocument(entry, {
    kind: "pdf",
    title: entry.name,
    subtitle: "PDF 文档预览",
    sections: [{
      title: "文件",
      rows: [
        ["类型", "PDF"],
        ["大小", entry.sizeLabel || "未知"],
      ],
    }],
    stats: [{ label: "预览", value: "PDF" }],
    unsupported: false,
  });
}

function createFallbackDocument(entry, kind) {
  return {
    kind,
    title: entry.name,
    subtitle: "文件信息缩略图",
    sections: [{
      title: "文件",
      rows: [
        ["类型", (entry.extension || "office").toUpperCase()],
        ["大小", entry.sizeLabel || "未知"],
      ],
    }],
    stats: [{ label: "预览", value: "文件信息" }],
    unsupported: true,
  };
}

async function readZipText(zip, path) {
  const normalizedPath = path.replace(/^\/+/, "");
  const file = zip.file(normalizedPath);
  if (!file) return "";
  return file.async("text");
}

function extractWordParagraphs(xml) {
  const paragraphs = xml.match(/<w:p[\s\S]*?<\/w:p>/g) ?? [];
  return paragraphs
    .map(extractXmlTextRuns)
    .map((runs) => compactText(runs.join("")))
    .filter(Boolean);
}

function extractXmlTextRuns(xml) {
  const text = [];
  const pattern = /<(?:\w+:)?t\b[^>]*>([\s\S]*?)<\/(?:\w+:)?t>/g;
  let match;
  while ((match = pattern.exec(xml))) {
    const value = compactText(decodeXmlEntities(match[1]));
    if (value) text.push(value);
  }
  return text;
}

async function readSharedStrings(zip) {
  const xml = await readZipText(zip, "xl/sharedStrings.xml");
  if (!xml) return [];
  const items = xml.match(/<si[\s\S]*?<\/si>/g) ?? [];
  return items.map((item) => compactText(extractXmlTextRuns(item).join("")));
}

async function readWorkbookRelationships(zip) {
  const xml = await readZipText(zip, "xl/_rels/workbook.xml.rels");
  const relationships = new Map();
  const pattern = /<Relationship\b([^>]*)\/?>/g;
  let match;
  while ((match = pattern.exec(xml))) {
    const attrs = parseXmlAttributes(match[1]);
    const id = attrs.get("Id");
    const target = attrs.get("Target");
    if (!id || !target) continue;
    relationships.set(id, normalizeWorkbookTarget(target));
  }
  return relationships;
}

async function readWorkbookSheetNames(zip) {
  const xml = await readZipText(zip, "xl/workbook.xml");
  const pattern = /<sheet\b([^>]*)\/?>/g;
  const sheets = [];
  let match;
  while ((match = pattern.exec(xml))) {
    const attrs = parseXmlAttributes(match[1]);
    sheets.push({
      name: attrs.get("name") || `Sheet ${sheets.length + 1}`,
      relationshipId: attrs.get("r:id") ?? attrs.get("id") ?? "",
      index: sheets.length,
    });
  }
  return sheets;
}

function parseWorksheetRows(xml, sharedStrings) {
  const rowXmlItems = xml.match(/<row\b[\s\S]*?<\/row>/g) ?? [];
  return rowXmlItems
    .map((rowXml) => {
      const cells = parseWorksheetCells(rowXml, sharedStrings);
      if (!cells.length) return [];
      const lastIndex = Math.min(Math.max(...cells.map((cell) => cell.index)), 7);
      return Array.from({ length: lastIndex + 1 }, (_, index) => cells.find((cell) => cell.index === index)?.text ?? "");
    })
    .filter((row) => row.some(Boolean));
}

function parseWorksheetCells(rowXml, sharedStrings) {
  const cells = [];
  const cellPattern = /<c\b([^>]*)>([\s\S]*?)<\/c>/g;
  let match;
  while ((match = cellPattern.exec(rowXml))) {
    const attrs = parseXmlAttributes(match[1]);
    const reference = attrs.get("r") ?? "";
    const type = attrs.get("t") ?? "";
    const rawValue = firstXmlTagContent(match[2], "v") || extractXmlTextRuns(match[2]).join("");
    const text = compactText(resolveCellValue(rawValue, type, sharedStrings));
    if (!text) continue;
    cells.push({
      index: columnIndexFromCellReference(reference),
      text,
    });
  }
  return cells;
}

function resolveCellValue(rawValue, type, sharedStrings) {
  const value = compactText(decodeXmlEntities(rawValue));
  if (type === "s") {
    const index = Number.parseInt(value, 10);
    return Number.isFinite(index) ? sharedStrings[index] ?? "" : "";
  }
  if (type === "b") return value === "1" ? "TRUE" : "FALSE";
  return value;
}

function firstXmlTagContent(xml, localName) {
  const pattern = new RegExp(`<(?:\\w+:)?${localName}\\b[^>]*>([\\s\\S]*?)<\\/(?:\\w+:)?${localName}>`);
  return pattern.exec(xml)?.[1] ?? "";
}

function parseXmlAttributes(value) {
  const attrs = new Map();
  const pattern = /([\w:-]+)="([^"]*)"/g;
  let match;
  while ((match = pattern.exec(value))) {
    attrs.set(match[1], decodeXmlEntities(match[2]));
  }
  return attrs;
}

function normalizeWorkbookTarget(target) {
  const cleanTarget = target.replace(/^\/+/, "");
  if (cleanTarget.startsWith("xl/")) return cleanTarget;
  return `xl/${cleanTarget}`;
}

function columnIndexFromCellReference(reference) {
  const letters = (reference.match(/[A-Z]+/i)?.[0] ?? "A").toUpperCase();
  let index = 0;
  for (const letter of letters) {
    index = index * 26 + letter.charCodeAt(0) - 64;
  }
  return Math.max(index - 1, 0);
}

function slideIndex(path) {
  return Number.parseInt(path.match(/slide(\d+)\.xml$/)?.[1] ?? "0", 10);
}

function compactText(value) {
  return value.replace(/\s+/g, " ").trim();
}

function decodeXmlEntities(value) {
  return value
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, "\"")
    .replace(/&apos;/g, "'")
    .replace(/&amp;/g, "&");
}

function documentPreviewLines(document) {
  const lines = [];
  const firstStats = document.stats.slice(0, 3).map((item) => `${item.label}: ${item.value}`).join("  ");
  if (firstStats) lines.push({ text: firstStats, emphasis: true });

  for (const section of document.sections) {
    lines.push({ text: section.title, emphasis: true, rule: lines.length > 0 });
    for (const row of section.rows.slice(0, 4)) {
      lines.push({ text: row.filter(Boolean).join("  ") || "空白" });
    }
  }
  return lines.length ? lines : [{ text: document.title, emphasis: true }];
}

function kindAccent(kind, fallback) {
  if (kind === "pdf") return "#d15f5f";
  if (kind === "word") return "#5b8fd8";
  if (kind === "spreadsheet") return "#4d9c6b";
  if (kind === "presentation") return "#c8784d";
  return fallback;
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

function getOfficePreviewKind(extension) {
  const normalized = extension?.toLowerCase() ?? "";
  if (pdfPreviewExtensions.includes(normalized)) return "pdf";
  if (wordPreviewExtensions.includes(normalized)) return "word";
  if (spreadsheetPreviewExtensions.includes(normalized)) return "spreadsheet";
  if (presentationPreviewExtensions.includes(normalized)) return "presentation";
  return "office";
}

function isOpenXmlOfficeExtension(extension) {
  const normalized = extension?.toLowerCase() ?? "";
  return [
    "docx",
    "docm",
    "dotx",
    "dotm",
    "xlsx",
    "xlsm",
    "xltx",
    "xltm",
    "pptx",
    "pptm",
    "ppsx",
    "ppsm",
    "potx",
    "potm",
  ].includes(normalized);
}

function officeKindLabel(kind) {
  if (kind === "pdf") return "PDF";
  if (kind === "word") return "Word";
  if (kind === "spreadsheet") return "Excel";
  if (kind === "presentation") return "PowerPoint";
  return "Office";
}
