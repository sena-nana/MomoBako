import type { FileBrowserEntry } from "../../../types/repository";

type RgbColor = {
  r: number;
  g: number;
  b: number;
};

export function extractPaletteFromImageElement(image: HTMLImageElement) {
  try {
    const canvas = document.createElement("canvas");
    const context = canvas.getContext("2d", { willReadFrequently: true });
    if (!context) return [];

    const width = 40;
    const sourceWidth = image.naturalWidth || image.width || 1;
    const sourceHeight = image.naturalHeight || image.height || 1;
    canvas.width = width;
    canvas.height = Math.min(Math.max(1, Math.round((sourceHeight / sourceWidth) * width)), 40);
    context.drawImage(image, 0, 0, canvas.width, canvas.height);
    const data = context.getImageData(0, 0, canvas.width, canvas.height).data;
    const buckets = new Map<string, { count: number; r: number; g: number; b: number }>();

    for (let index = 0; index < data.length; index += 4) {
      const alpha = data[index + 3];
      if (alpha < 24) continue;
      const r = data[index];
      const g = data[index + 1];
      const b = data[index + 2];
      const key = `${Math.floor(r / 16)}-${Math.floor(g / 16)}-${Math.floor(b / 16)}`;
      const bucket = buckets.get(key) ?? { count: 0, r: 0, g: 0, b: 0 };
      bucket.count += 1;
      bucket.r += r;
      bucket.g += g;
      bucket.b += b;
      buckets.set(key, bucket);
    }

    const ranked = [...buckets.values()]
      .filter((bucket) => bucket.count > 0)
      .map((bucket) => ({
        count: bucket.count,
        r: Math.round(bucket.r / bucket.count),
        g: Math.round(bucket.g / bucket.count),
        b: Math.round(bucket.b / bucket.count),
      }))
      .sort((left, right) => right.count - left.count);

    const colors: string[] = [];
    for (const color of ranked) {
      if (colors.some((existing) => colorDistance(existing, color) < 30)) continue;
      colors.push(rgbToHex(color.r, color.g, color.b));
      if (colors.length === 5) break;
    }
    return colors;
  } catch {
    return [];
  }
}

export function createExternalDragIcon(entry: FileBrowserEntry) {
  try {
    const canvas = document.createElement("canvas");
    const context = canvas.getContext("2d");
    if (!context) return undefined;

    const size = 72;
    const scale = Math.min(Math.max(window.devicePixelRatio || 1, 1), 2);
    canvas.width = size * scale;
    canvas.height = size * scale;
    context.scale(scale, scale);

    const gradient = context.createLinearGradient(0, 0, size, size);
    if (entry.kind === "directory") {
      gradient.addColorStop(0, "#d3b26f");
      gradient.addColorStop(1, "#6e542e");
    } else {
      gradient.addColorStop(0, "#8aa8b0");
      gradient.addColorStop(1, "#314a53");
    }

    context.fillStyle = "rgba(0, 0, 0, 0.22)";
    fillRoundedRect(context, 8, 10, 56, 56, 12);
    context.fillStyle = gradient;
    fillRoundedRect(context, 6, 6, 56, 56, 12);

    context.fillStyle = "rgba(255, 255, 255, 0.9)";
    context.font = "700 16px system-ui, sans-serif";
    context.textAlign = "center";
    context.textBaseline = "middle";
    const label = entry.kind === "directory" ? "DIR" : (entry.extension || "FILE").slice(0, 4).toUpperCase();
    context.fillText(label, 34, 34, 44);

    return canvas.toDataURL("image/png");
  } catch {
    return undefined;
  }
}

function rgbToHex(r: number, g: number, b: number) {
  return `#${[r, g, b].map((value) => value.toString(16).padStart(2, "0")).join("").toUpperCase()}`;
}

function colorDistance(leftHex: string, right: RgbColor) {
  const left = [
    Number.parseInt(leftHex.slice(1, 3), 16),
    Number.parseInt(leftHex.slice(3, 5), 16),
    Number.parseInt(leftHex.slice(5, 7), 16),
  ];
  return Math.sqrt(
    (left[0] - right.r) ** 2 +
    (left[1] - right.g) ** 2 +
    (left[2] - right.b) ** 2,
  );
}

function fillRoundedRect(context: CanvasRenderingContext2D, x: number, y: number, width: number, height: number, radius: number) {
  context.beginPath();
  context.moveTo(x + radius, y);
  context.lineTo(x + width - radius, y);
  context.quadraticCurveTo(x + width, y, x + width, y + radius);
  context.lineTo(x + width, y + height - radius);
  context.quadraticCurveTo(x + width, y + height, x + width - radius, y + height);
  context.lineTo(x + radius, y + height);
  context.quadraticCurveTo(x, y + height, x, y + height - radius);
  context.lineTo(x, y + radius);
  context.quadraticCurveTo(x, y, x + radius, y);
  context.closePath();
  context.fill();
}
