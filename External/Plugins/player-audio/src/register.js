/** 通用音频播放器插件入口。 */
import { audioPreviewExtensions } from "./audioExtensions.js";
import { createAudioPreviewComponent } from "./audioPreview.js";
import { createAudioPlaylistRuntime } from "./audioRuntime.js";

export function register(ctx) {
  ctx.registerPreview({
    supportedExtensions: audioPreviewExtensions,
    component: createAudioPreviewComponent(ctx),
  });

  ctx.registerPlaylistPlayer({
    playerTypeId: "momobako.playlist.audio-sequence",
    capabilityId: "momobako.player.audio",
    label: "音频顺序播放",
    fileClass: "audio",
    supportedExtensions: audioPreviewExtensions,
    supportsSeek: true,
    supportsVolume: true,
    supportsPreviewNavigation: true,
    description: "通过宿主播放源路由播放通用音频队列。",
    createRuntime(controller) {
      return createAudioPlaylistRuntime(ctx, controller);
    },
  });
}
