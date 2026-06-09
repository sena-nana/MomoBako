# MomoBako

MomoBako 是一个基于 Tauri 2、Vue 3 与 TypeScript 的桌面资源库工作台。

当前工程包含：

- 自绘标题栏、可拖拽侧栏、紧凑工作台 UI。
- 主窗口位置、尺寸与最大化状态恢复，避免启动时先闪默认窗口再跳转。
- 暗色 / 浅色主题切换与本地持久化。
- 组件声明式右键菜单、程序化打开菜单、危险项二次确认，并全局屏蔽浏览器原生右键菜单。
- 通用确认弹层和 `AGENTS.md` 开发规范。
- Yarn 4 单应用包管理与 `verify` 验证脚本。
- Tauri Rust 服务、SQLite 资源库、文件同步、托盘和媒体预览能力。

## 命令

```bash
yarn install
yarn dev
yarn tauri:dev
yarn verify
```

`yarn verify` 会串行运行前端测试、前端构建和 Tauri Rust 编译检查。
