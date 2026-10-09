// 将测试进程配置与 Vite 开发配置分离，并关闭 Node 试验性 Web Storage。
import vue from "@vitejs/plugin-vue";
import { configDefaults, defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [vue()],
  optimizeDeps: {
    exclude: ["@lilia/ui"],
  },
  ssr: {
    noExternal: ["@lilia/ui"],
  },
  test: {
    environment: "jsdom",
    // 代理 worktree、构建产物和本地对照环境里也有测试文件，不属于本仓库的测试集。
    exclude: [...configDefaults.exclude, "**/.claude/**", "**/target/**", "tmp/**"],
    execArgv: ["--no-experimental-webstorage"],
    setupFiles: ["./tests/setupTests.ts"],
    testTimeout: 10_000,
    server: {
      deps: {
        inline: ["@lilia/ui"],
      },
    },
  },
});
