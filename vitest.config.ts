// 工具链、打包、文档和插件清单的测试都只读仓库文件和跑 Node 脚本，在 Node 环境里跑。
import { configDefaults, defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    environment: "node",
    // 代理 worktree、构建产物和本地对照环境里也有测试文件，不属于本仓库的测试集。
    exclude: [...configDefaults.exclude, "**/.claude/**", "**/target/**", "tmp/**"],
    testTimeout: 10_000,
  },
});
