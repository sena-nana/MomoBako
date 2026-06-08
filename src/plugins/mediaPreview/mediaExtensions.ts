export const videoPreviewExtensions = ["mp4", "mov", "mkv", "webm", "avi", "m4v"] as const;

export const audioPreviewExtensions = ["mp3", "wav", "ogg", "flac", "m4a", "aac", "opus"] as const;

export function isVideoExtension(extension?: string | null) {
  return videoPreviewExtensions.includes((extension ?? "").toLowerCase() as typeof videoPreviewExtensions[number]);
}

export function isAudioExtension(extension?: string | null) {
  return audioPreviewExtensions.includes((extension ?? "").toLowerCase() as typeof audioPreviewExtensions[number]);
}
