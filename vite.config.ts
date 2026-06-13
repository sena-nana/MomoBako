/// <reference types="vitest" />
import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import { fileURLToPath, pathToFileURL } from "node:url";

const threeWebgpuCompatId = "\0momobako-three-webgpu-compat";
const threeWebgpuBuildUrl = pathToFileURL(
  fileURLToPath(new URL("./node_modules/three/build/three.webgpu.js", import.meta.url)),
).href;

// @ts-expect-error process 是 Node.js 全局对象
const host = process.env.TAURI_DEV_HOST;
// @ts-expect-error process 是 Node.js 全局对象
const port = Number(process.env.PORT) || 1420;

export default defineConfig(async () => ({
  plugins: [
    {
      name: "momobako-three-webgpu-compat",
      enforce: "pre",
      resolveId(source) {
        if (source === "three/webgpu") return threeWebgpuCompatId;
        return null;
      },
      load(id) {
        if (id !== threeWebgpuCompatId) return null;
        const webgpuModule = JSON.stringify(threeWebgpuBuildUrl);
        return [
          `export * from ${webgpuModule};`,
          `import { TSL } from ${webgpuModule};`,
          "export const tslFn = TSL.Fn;",
        ].join("\n");
      },
    },
    vue(),
  ],
  clearScreen: false,
  server: {
    port,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
        }
      : undefined,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
  test: {
    environment: "jsdom",
    setupFiles: ["./tests/setupTests.ts"],
  },
  build: {
    rollupOptions: {
      output: {
        manualChunks(id) {
          const normalizedId = id.replace(/\\/g, "/");
          if (
            normalizedId.includes("/node_modules/three/") ||
            normalizedId.includes("/node_modules/@pixiv/three-vrm/")
          ) {
            return "vendor-three-preview";
          }
          if (normalizedId.includes("/node_modules/vue3-markdown-it/")) {
            return "vendor-markdown-preview";
          }
          return undefined;
        },
      },
    },
  },
}));
