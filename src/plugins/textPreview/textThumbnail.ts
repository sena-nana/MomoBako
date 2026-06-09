import type { FileBrowserEntry } from "../../types/repository";
import { loadTextPreviewContent, TEXT_THUMBNAIL_BYTE_LIMIT } from "./textSource";

export type GeneratedTextThumbnail = {
  bytes: number[];
  mediaType: string;
};

const THUMBNAIL_SIZE = 512;
const THUMBNAIL_LINE_LIMIT = 14;

export async function generateTextThumbnailForEntry(
  repoId: string,
  entry: FileBrowserEntry,
): Promise<GeneratedTextThumbnail | null> {
  const content = await loadTextPreviewContent(repoId, entry.path, TEXT_THUMBNAIL_BYTE_LIMIT);
  return generateTextThumbnailFromContent(entry, content.text);
}

export async function generateTextThumbnailFromContent(
  entry: FileBrowserEntry,
  content: string,
): Promise<GeneratedTextThumbnail | null> {
  if (typeof document === "undefined") return null;

  const canvas = document.createElement("canvas");
  canvas.width = THUMBNAIL_SIZE;
  canvas.height = THUMBNAIL_SIZE;
  const context = canvas.getContext("2d");
  if (!context) return null;

  const styles = getComputedStyle(document.documentElement);
  const color = (name: string, fallback: string) => styles.getPropertyValue(name).trim() || fallback;
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

  const blob = await new Promise<Blob | null>((resolve) => {
    canvas.toBlob(resolve, "image/jpeg", 0.88);
  });
  if (!blob) return null;
  return {
    bytes: Array.from(new Uint8Array(await blob.arrayBuffer())),
    mediaType: blob.type || "image/jpeg",
  };
}

function previewLines(content: string) {
  const normalized = content.replace(/\r\n?/g, "\n");
  const lines = normalized.split("\n").slice(0, THUMBNAIL_LINE_LIMIT);
  return lines.length ? lines : ["空文件"];
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
