import pdfWorkerUrl from "pdfjs-dist/build/pdf.worker.mjs?url";

const PDF_CANVAS_MAX_EDGE = 1800;
const PDF_THUMBNAIL_SIZE = 512;

let workerConfigured = false;
let pdfjsModule: typeof import("pdfjs-dist") | null = null;

export type PdfRenderResult = {
  pageCount: number;
  canvas: HTMLCanvasElement;
};

export async function renderPdfFirstPage(
  sourceUrl: string,
  options: {
    maxEdge?: number;
    signal?: AbortSignal;
  } = {},
): Promise<PdfRenderResult> {
  if (typeof document === "undefined") {
    throw new Error("当前环境无法渲染 PDF");
  }

  const pdfjs = await loadPdfJs();
  const loadingTask = pdfjs.getDocument({
    url: sourceUrl,
    disableAutoFetch: true,
    disableStream: false,
  });
  options.signal?.addEventListener("abort", () => loadingTask.destroy(), { once: true });

  const pdf = await loadingTask.promise;
  const pageCount = pdf.numPages;
  const page = await pdf.getPage(1);
  const viewport = page.getViewport({ scale: 1 });
  const maxEdge = options.maxEdge ?? PDF_CANVAS_MAX_EDGE;
  const scale = Math.min(maxEdge / Math.max(viewport.width, viewport.height), 2.2);
  const scaledViewport = page.getViewport({ scale });
  const canvas = document.createElement("canvas");
  canvas.width = Math.max(Math.floor(scaledViewport.width), 1);
  canvas.height = Math.max(Math.floor(scaledViewport.height), 1);
  await page.render({
    canvas,
    viewport: scaledViewport,
    background: "#ffffff",
  }).promise;

  page.cleanup();
  await pdf.cleanup();
  await loadingTask.destroy();
  return {
    pageCount,
    canvas,
  };
}

export async function generatePdfThumbnail(sourceUrl: string): Promise<{ bytes: number[]; mediaType: string } | null> {
  if (typeof document === "undefined") return null;
  const rendered = await renderPdfFirstPage(sourceUrl, { maxEdge: PDF_THUMBNAIL_SIZE * 1.7 });
  const canvas = document.createElement("canvas");
  canvas.width = PDF_THUMBNAIL_SIZE;
  canvas.height = PDF_THUMBNAIL_SIZE;
  const context = canvas.getContext("2d");
  if (!context) return null;

  const styles = getComputedStyle(document.documentElement);
  const background = cssColor(styles, "--bg-elev", "#202020");
  const border = cssColor(styles, "--border", "#2a2a2a");
  context.fillStyle = background;
  context.fillRect(0, 0, canvas.width, canvas.height);

  const margin = 36;
  const availableWidth = canvas.width - margin * 2;
  const availableHeight = canvas.height - margin * 2;
  const ratio = Math.min(availableWidth / rendered.canvas.width, availableHeight / rendered.canvas.height);
  const width = Math.max(1, Math.floor(rendered.canvas.width * ratio));
  const height = Math.max(1, Math.floor(rendered.canvas.height * ratio));
  const x = Math.floor((canvas.width - width) / 2);
  const y = Math.floor((canvas.height - height) / 2);
  context.fillStyle = "#ffffff";
  roundRect(context, x - 8, y - 8, width + 16, height + 16, 10);
  context.fill();
  context.strokeStyle = border;
  context.lineWidth = 2;
  context.stroke();
  context.drawImage(rendered.canvas, x, y, width, height);

  return canvasToJpegBytes(canvas, 0.88);
}

export async function canvasToJpegBytes(canvas: HTMLCanvasElement, quality = 0.88) {
  const blob = await new Promise<Blob | null>((resolve) => {
    canvas.toBlob(resolve, "image/jpeg", quality);
  });
  if (!blob) return null;
  return {
    bytes: Array.from(new Uint8Array(await blob.arrayBuffer())),
    mediaType: blob.type || "image/jpeg",
  };
}

async function loadPdfJs() {
  const pdfjs = pdfjsModule ?? await import("pdfjs-dist");
  pdfjsModule = pdfjs;
  if (workerConfigured) return pdfjs;
  pdfjs.GlobalWorkerOptions.workerSrc = pdfWorkerUrl;
  workerConfigured = true;
  return pdfjs;
}

function cssColor(styles: CSSStyleDeclaration, name: string, fallback: string) {
  return styles.getPropertyValue(name).trim() || fallback;
}

function roundRect(
  context: CanvasRenderingContext2D,
  x: number,
  y: number,
  width: number,
  height: number,
  radius: number,
) {
  context.beginPath();
  context.moveTo(x + radius, y);
  context.arcTo(x + width, y, x + width, y + height, radius);
  context.arcTo(x + width, y + height, x, y + height, radius);
  context.arcTo(x, y + height, x, y, radius);
  context.arcTo(x, y, x + width, y, radius);
  context.closePath();
}
