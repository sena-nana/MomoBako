/**
 * 将缩略图路径转换为可直接渲染的图片地址。
 * 本地文件路径走 Tauri 的 convertFileSrc，远程地址直接返回。
 */
import { convertFileSrc } from "@tauri-apps/api/core";

export function resolveThumbnailSrc(thumbnailPath?: string | null) {
  if (!thumbnailPath) return null;
  if (/^https?:\/\//i.test(thumbnailPath)) return thumbnailPath;
  return convertFileSrc(thumbnailPath);
}
