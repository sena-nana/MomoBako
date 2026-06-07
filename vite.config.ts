/// <reference types="vitest" />
import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

// @ts-expect-error process 是 Node.js 全局对象
const host = process.env.TAURI_DEV_HOST;
// @ts-expect-error process 是 Node.js 全局对象
const port = Number(process.env.PORT) || 1420;

export default defineConfig(async () => ({
  plugins: [vue()],
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
}));
