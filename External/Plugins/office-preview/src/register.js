import * as pdfjsLib from "pdfjs-dist";

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

const THUMBNAIL_SIZE = 512;
const OFFICE_CONVERT_PLUGIN_ID = "momobako.service.office-convert";

pdfjsLib.GlobalWorkerOptions.workerSrc = new URL(
  "pdfjs-dist/build/pdf.worker.mjs",
  import.meta.url,
).toString();

export function register(ctx) {
  const pdfRuntime = ctx.pdfRuntime ?? pdfjsLib;
  const {
    computed,
    h,
    nextTick,
    onBeforeUnmount,
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
      const sourceUrl = ref("");
      const previewInfo = ref(null);
      const pageCount = ref(0);
      const currentPage = ref(1);
      const loadingLabel = ref("准备预览");
      const viewer = ref(null);
      const canvas = ref(null);
      const pdfDocument = ref(null);
      let loadToken = 0;

      const kind = computed(() => getOfficePreviewKind(props.entry?.extension));
      const kindLabel = computed(() => officeKindLabel(kind.value));
      const extensionLabel = computed(() => props.entry?.extension?.toUpperCase() || kindLabel.value.toUpperCase());
      const statusDetail = computed(() => {
        if (state.value === "ready" && pageCount.value > 0) return `${pageCount.value} 页 PDF`;
        if (previewInfo.value?.converter) return previewInfo.value.converter;
        return props.entry?.sizeLabel || "准备文档";
      });

      watch(
        [() => props.repoId, () => props.entry?.path],
        () => {
          void loadPreview();
        },
        { immediate: true },
      );

      watch(currentPage, () => {
        if (state.value === "ready") {
          void renderCurrentPage();
        }
      });

      onBeforeUnmount(() => {
        destroyPdfDocument();
      });

      async function loadPreview() {
        const token = ++loadToken;
        state.value = "loading";
        errorMessage.value = "";
        sourceUrl.value = "";
        previewInfo.value = null;
        pageCount.value = 0;
        currentPage.value = 1;
        loadingLabel.value = kind.value === "pdf" ? "载入 PDF" : "转换文档";
        destroyPdfDocument();

        try {
          const preview = await resolvePreviewPdf(ctx, props.repoId, props.entry);
          if (token !== loadToken) return;
          previewInfo.value = preview;
          sourceUrl.value = preview.sourceUrl;
          loadingLabel.value = "解析 PDF";
          await loadPdfDocument(token, preview.sourceUrl);
          if (token !== loadToken) return;
          state.value = "ready";
          void persistPdfThumbnail(token);
        } catch (cause) {
          if (token !== loadToken) return;
          state.value = "error";
          errorMessage.value = cause instanceof Error ? cause.message : String(cause);
        }
      }

      async function loadPdfDocument(token, url) {
        const task = pdfRuntime.getDocument(url);
        const document = await task.promise;
        if (token !== loadToken) {
          await document.destroy();
          return;
        }
        pdfDocument.value = document;
        pageCount.value = document.numPages;
        currentPage.value = 1;
        await nextTick();
        await renderCurrentPage();
      }

      async function renderCurrentPage() {
        const document = pdfDocument.value;
        const canvasElement = canvas.value;
        const container = viewer.value;
        if (!document || !canvasElement || !container) return;
        const page = await document.getPage(currentPage.value);
        const containerWidth = Math.max(container.clientWidth - 32, 320);
        const initialViewport = page.getViewport({ scale: 1 });
        const scale = containerWidth / initialViewport.width;
        const viewport = page.getViewport({ scale });
        const context = canvasElement.getContext("2d");
        if (!context) return;
        canvasElement.width = Math.ceil(viewport.width * window.devicePixelRatio);
        canvasElement.height = Math.ceil(viewport.height * window.devicePixelRatio);
        canvasElement.style.width = `${Math.ceil(viewport.width)}px`;
        canvasElement.style.height = `${Math.ceil(viewport.height)}px`;
        context.setTransform(window.devicePixelRatio, 0, 0, window.devicePixelRatio, 0, 0);
        await page.render({
          canvasContext: context,
          viewport,
        }).promise;
      }

      async function persistPdfThumbnail(token) {
        await nextTick();
        if (token !== loadToken) return;
        const thumbnail = await generatePdfThumbnailFromPreview();
        if (token !== loadToken || !thumbnail) return;
        await ctx.saveGeneratedThumbnail({
          repoId: props.repoId,
          path: props.entry.path,
          imageBytes: thumbnail.bytes,
          mediaType: thumbnail.mediaType,
        });
      }

      async function generatePdfThumbnailFromPreview() {
        const document = pdfDocument.value;
        if (!document || typeof document === "undefined") return null;
        const page = await document.getPage(1);
        const viewport = page.getViewport({ scale: 1 });
        const scale = THUMBNAIL_SIZE / Math.max(viewport.width, viewport.height, 1);
        const thumbnailViewport = page.getViewport({ scale });
        const offscreen = window.document.createElement("canvas");
        offscreen.width = Math.ceil(thumbnailViewport.width);
        offscreen.height = Math.ceil(thumbnailViewport.height);
        const context = offscreen.getContext("2d");
        if (!context) return null;
        await page.render({
          canvasContext: context,
          viewport: thumbnailViewport,
        }).promise;
        const blob = await new Promise((resolve) => {
          offscreen.toBlob(resolve, "image/jpeg", 0.88);
        });
        if (!blob) return null;
        return {
          bytes: Array.from(new Uint8Array(await blob.arrayBuffer())),
          mediaType: blob.type || "image/jpeg",
        };
      }

      function destroyPdfDocument() {
        const document = pdfDocument.value;
        pdfDocument.value = null;
        if (document?.destroy) {
          void document.destroy();
        }
      }

      function goToPreviousPage() {
        if (currentPage.value > 1) currentPage.value -= 1;
      }

      function goToNextPage() {
        if (currentPage.value < pageCount.value) currentPage.value += 1;
      }

      return {
        canvas,
        currentPage,
        entry: props.entry,
        errorMessage,
        extensionLabel,
        goToNextPage,
        goToPreviousPage,
        kind,
        kindLabel,
        loadingLabel,
        pageCount,
        previewInfo,
        sourceUrl,
        state,
        statusDetail,
        viewer,
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
            h("span", this.loadingLabel),
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

      return h("div", { class: `office-preview office-preview--${this.kind}` }, [
        toolbar,
        h("div", { class: "office-preview__viewer", ref: "viewer" }, [
          h("div", { class: "office-preview__pagination" }, [
            h("button", {
              type: "button",
              class: "office-preview__page-button",
              disabled: this.currentPage <= 1,
              onClick: this.goToPreviousPage,
            }, "上一页"),
            h("span", { class: "office-preview__page-label" }, `${this.currentPage} / ${this.pageCount || 1}`),
            h("button", {
              type: "button",
              class: "office-preview__page-button",
              disabled: this.currentPage >= this.pageCount,
              onClick: this.goToNextPage,
            }, "下一页"),
          ]),
          h("div", { class: "office-preview__page-surface" }, [
            h("canvas", {
              ref: "canvas",
              class: "office-preview__canvas",
            }),
          ]),
        ]),
      ]);
    },
  };

  ctx.registerPreview({
    supportedExtensions: officePreviewExtensions,
    component: OfficePreviewPlugin,
    generateThumbnail: async ({ repoId, entry }) => {
      const preview = await resolvePreviewPdf(ctx, repoId, entry);
      const task = pdfRuntime.getDocument(preview.sourceUrl);
      const document = await task.promise;
      try {
        const page = await document.getPage(1);
        const viewport = page.getViewport({ scale: 1 });
        const scale = THUMBNAIL_SIZE / Math.max(viewport.width, viewport.height, 1);
        const thumbnailViewport = page.getViewport({ scale });
        const canvas = window.document.createElement("canvas");
        canvas.width = Math.ceil(thumbnailViewport.width);
        canvas.height = Math.ceil(thumbnailViewport.height);
        const context = canvas.getContext("2d");
        if (!context) return null;
        await page.render({
          canvasContext: context,
          viewport: thumbnailViewport,
        }).promise;
        const blob = await new Promise((resolve) => {
          canvas.toBlob(resolve, "image/jpeg", 0.88);
        });
        if (!blob) return null;
        return {
          bytes: Array.from(new Uint8Array(await blob.arrayBuffer())),
          mediaType: blob.type || "image/jpeg",
        };
      } finally {
        await document.destroy();
      }
    },
  });
}

async function resolvePreviewPdf(ctx, repoId, entry) {
  if (getOfficePreviewKind(entry?.extension) === "pdf") {
    const source = await ctx.preparePreviewFileSource({
      repoId,
      path: entry.path,
    });
    if (!source.sourceUrl) {
      throw new Error("PDF 预览源不可用");
    }
    return {
      sourceUrl: source.sourceUrl,
      converter: "pdf",
      cacheKey: `${repoId}:${entry.path}`,
    };
  }

  const converted = await ctx.callPlugin({
    pluginId: OFFICE_CONVERT_PLUGIN_ID,
    method: "officeConvert.ensurePreviewPdf",
    payload: {
      repoId,
      entryPath: entry.path,
      extension: entry.extension ?? "",
      sourcePath: entry.localAbsolutePath ?? null,
      sourceModifiedAt: entry.modifiedAt ?? null,
      sourceSizeBytes: entry.sizeBytes ?? null,
    },
  });
  const preview = await ctx.prepareRepositoryCacheFilePreviewSource({
    repoId,
    path: converted.payload?.pdfPath,
    mediaType: "application/pdf",
  });
  if (!preview.sourceUrl) {
    throw new Error("转换后的 PDF 预览源不可用");
  }
  return {
    sourceUrl: preview.sourceUrl,
    converter: converted.payload?.converter || "office-convert",
    cacheKey: converted.payload?.cacheKey || `${repoId}:${entry.path}`,
  };
}

function getOfficePreviewKind(extension) {
  const normalized = extension?.toLowerCase() ?? "";
  if (pdfPreviewExtensions.includes(normalized)) return "pdf";
  if (wordPreviewExtensions.includes(normalized)) return "word";
  if (spreadsheetPreviewExtensions.includes(normalized)) return "spreadsheet";
  if (presentationPreviewExtensions.includes(normalized)) return "presentation";
  return "office";
}

function officeKindLabel(kind) {
  if (kind === "pdf") return "PDF";
  if (kind === "word") return "Word";
  if (kind === "spreadsheet") return "Excel";
  if (kind === "presentation") return "PowerPoint";
  return "Office";
}
