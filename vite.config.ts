/// <reference types="vitest" />
import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

// @ts-expect-error process 是 Node.js 全局对象
const host = process.env.TAURI_DEV_HOST;
// @ts-expect-error process 是 Node.js 全局对象
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
  test: {
    environment: "jsdom",
    setupFiles: ["./tests/setupTests.ts"],
    server: {
      deps: {
        inline: ["@lilia/ui"],
      },
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
