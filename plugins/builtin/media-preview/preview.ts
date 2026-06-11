import { defineAsyncComponent } from "vue";
import manifest from "./manifest.json";
import { definePreviewPlugin } from "../../../src/plugins/sdk";
import {
  audioPreviewExtensions,
  imagePreviewExtensions,
  videoPreviewExtensions,
} from "../../../src/plugins/mediaPreview/mediaExtensions";
import type { PluginManifest } from "../../../src/types/repository";

export default definePreviewPlugin({
  manifest: manifest as PluginManifest,
  supportedExtensions: [...imagePreviewExtensions, ...videoPreviewExtensions, ...audioPreviewExtensions],
  component: defineAsyncComponent(() => import("../../../src/plugins/mediaPreview/MediaPreview.vue")),
});
