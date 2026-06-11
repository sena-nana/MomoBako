export const imagePreviewExtensions = ["png", "jpg", "jpeg", "webp", "gif", "bmp", "avif", "svg"] as const;

export const videoPreviewExtensions = ["mp4", "mov", "mkv", "webm", "avi", "m4v"] as const;

export const audioPreviewExtensions = ["mp3", "wav", "ogg", "flac", "m4a", "aac", "opus"] as const;

export function isImageExtension(extension?: string | null) {
  return imagePreviewExtensions.includes((extension ?? "").toLowerCase() as typeof imagePreviewExtensions[number]);
}

export function isVideoExtension(extension?: string | null) {
  return videoPreviewExtensions.includes((extension ?? "").toLowerCase() as typeof videoPreviewExtensions[number]);
}

export function isAudioExtension(extension?: string | null) {
  return audioPreviewExtensions.includes((extension ?? "").toLowerCase() as typeof audioPreviewExtensions[number]);
}
