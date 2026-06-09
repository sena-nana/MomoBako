import { defineAsyncComponent } from "vue";
import manifest from "./manifest.json";
import { definePreviewPlugin } from "../../../src/plugins/sdk";
import { textPreviewExtensions } from "../../../src/plugins/textPreview/textExtensions";
import { generateTextThumbnailForEntry } from "../../../src/plugins/textPreview/textThumbnail";
import type { PluginManifest } from "../../../src/types/repository";

export default definePreviewPlugin({
  manifest: manifest as PluginManifest,
  supportedExtensions: textPreviewExtensions,
  component: defineAsyncComponent(() => import("../../../src/plugins/textPreview/TextPreview.vue")),
  generateThumbnail: ({ repoId, entry }) => generateTextThumbnailForEntry(repoId, entry),
});
