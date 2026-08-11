/** 通过宿主播放源路由准备音频，播放器不识别来源协议。 */
export async function prepareAudioSource(ctx, repoId, path, onProgress) {
  const request = { repoId, path };
  const response = ctx.prepareEntryPlaybackSourceWithProgress
    ? await ctx.prepareEntryPlaybackSourceWithProgress(request, (event) => {
        if (event.path !== path) return;
        onProgress?.(event);
      })
    : await ctx.prepareEntryPlaybackSource(request);
  const sourceUrl = response.sourceUrl || (response.localPath ? ctx.fileSrc(response.localPath) : null);
  if (!sourceUrl) throw new Error("音频播放源不可用");
  return { ...response, sourceUrl };
}

export function errorText(cause, fallback) {
  return cause instanceof Error ? cause.message : fallback;
}
