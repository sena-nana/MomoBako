/// <reference types="vitest" />
import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

const host = process.env.TAURI_DEV_HOST;
const port = Number(process.env.PORT) || 1420;

export default defineConfig(() => ({
  plugins: [vue()],
  clearScreen: false,
  optimizeDeps: {
    exclude: ["@lilia/ui"],
  },
  ssr: {
    noExternal: ["@lilia/ui"],
  },
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
