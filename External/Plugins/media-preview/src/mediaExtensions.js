/** 图片与视频预览插件支持的扩展名。 */
export const imagePreviewExtensions = ["png", "jpg", "jpeg", "webp", "gif", "bmp", "avif", "svg"];
export const videoPreviewExtensions = ["mp4", "mov", "mkv", "webm", "avi", "m4v"];

export function isImageExtension(extension) {
  return imagePreviewExtensions.includes((extension ?? "").toLowerCase());
}
