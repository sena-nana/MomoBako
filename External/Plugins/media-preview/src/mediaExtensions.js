export const imagePreviewExtensions = ["png", "jpg", "jpeg", "webp", "gif", "bmp", "avif", "svg"];
export const videoPreviewExtensions = ["mp4", "mov", "mkv", "webm", "avi", "m4v"];
export const audioPreviewExtensions = ["mp3", "wav", "ogg", "flac", "m4a", "aac", "opus"];

export function isImageExtension(extension) {
  return imagePreviewExtensions.includes((extension ?? "").toLowerCase());
}

export function isVideoExtension(extension) {
  return videoPreviewExtensions.includes((extension ?? "").toLowerCase());
}
