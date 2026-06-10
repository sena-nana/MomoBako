import { defineAsyncComponent } from "vue";
import manifest from "./manifest.json";
import { definePreviewPlugin } from "../../../src/plugins/sdk";
import type { PluginManifest } from "../../../src/types/repository";

export default definePreviewPlugin({
  manifest: manifest as PluginManifest,
  supportedExtensions: ["fbx", "obj", "glb", "gltf", "vrm", "stl", "3mf", "blend"],
  component: defineAsyncComponent(() => import("../../../src/plugins/threeModelPreview/ThreeModelPreview.vue")),
});
