import { defineAsyncComponent } from "vue";
import manifest from "./manifest.json";
import { definePreviewPlugin } from "../../../src/plugins/sdk";
import { officePreviewExtensions } from "../../../src/plugins/officePreview/officeExtensions";
import { generateOfficeThumbnailForEntry } from "../../../src/plugins/officePreview/officeThumbnail";
import type { PluginManifest } from "../../../src/types/repository";

export default definePreviewPlugin({
  manifest: manifest as PluginManifest,
  supportedExtensions: officePreviewExtensions,
  component: defineAsyncComponent(() => import("../../../src/plugins/officePreview/OfficePreview.vue")),
  generateThumbnail: ({ repoId, entry }) => generateOfficeThumbnailForEntry(repoId, entry),
});
