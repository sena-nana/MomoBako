import type { FileBrowserEntry } from "../../types/repository";
import { getOfficePreviewKind, officeKindLabel, type OfficePreviewKind } from "./officeExtensions";
import { loadOfficePreviewDocument, prepareOfficePreviewSource, type OfficePreviewDocument } from "./officeSource";
import { generatePdfThumbnail, canvasToJpegBytes } from "./pdfPreview";

export type GeneratedOfficeThumbnail = {
  bytes: number[];
  mediaType: string;
};

const THUMBNAIL_SIZE = 512;
const PREVIEW_LINE_LIMIT = 9;

export async function generateOfficeThumbnailForEntry(
  repoId: string,
  entry: FileBrowserEntry,
): Promise<GeneratedOfficeThumbnail | null> {
  const kind = getOfficePreviewKind(entry.extension);
  if (kind === "pdf") {
    const source = await prepareOfficePreviewSource(repoId, entry.path);
    return generatePdfThumbnail(source.sourceUrl);
  }

  try {
    const document = await loadOfficePreviewDocument(repoId, entry);
    return generateOfficeThumbnailFromDocument(entry, document);
  } catch {
    return generateOfficeThumbnailFromDocument(entry, createFallbackDocument(entry, kind));
  }
}

export async function generateOfficeThumbnailFromDocument(
  entry: FileBrowserEntry,
  previewDocument: OfficePreviewDocument,
): Promise<GeneratedOfficeThumbnail | null> {
  if (typeof window === "undefined" || typeof window.document === "undefined") return null;
  const canvas = window.document.createElement("canvas");
  canvas.width = THUMBNAIL_SIZE;
  canvas.height = THUMBNAIL_SIZE;
  const context = canvas.getContext("2d");
  if (!context) return null;

  const styles = getComputedStyle(window.document.documentElement);
  const color = (name: string, fallback: string) => styles.getPropertyValue(name).trim() || fallback;
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

  return canvasToJpegBytes(canvas, 0.88);
}

function createFallbackDocument(entry: FileBrowserEntry, kind: OfficePreviewKind): OfficePreviewDocument {
  return {
    kind,
    title: entry.name,
    subtitle: "文件信息缩略图",
    sections: [
      {
        title: "文件",
        rows: [
          ["类型", (entry.extension || "office").toUpperCase()],
          ["大小", entry.sizeLabel || "未知"],
        ],
      },
    ],
    stats: [{ label: "预览", value: "文件信息" }],
    unsupported: true,
  };
}

function documentPreviewLines(document: OfficePreviewDocument) {
  const lines: Array<{ text: string; emphasis?: boolean; rule?: boolean }> = [];
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

function kindAccent(kind: OfficePreviewKind, fallback: string) {
  if (kind === "pdf") return "#d15f5f";
  if (kind === "word") return "#5b8fd8";
  if (kind === "spreadsheet") return "#4d9c6b";
  if (kind === "presentation") return "#c8784d";
  return fallback;
}

function drawTrimmedText(
  context: CanvasRenderingContext2D,
  value: string,
  x: number,
  y: number,
  maxWidth: number,
) {
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
