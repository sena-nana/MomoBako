# Tauri Template

一个从 Lilia 当前桌面端外壳提取出的最小 Tauri 2 + Vue 3 + TypeScript 应用模板。

模板保留：

- Lilia 风格的自绘标题栏、可拖拽侧栏、紧凑工作台 UI。
- 主窗口位置、尺寸与最大化状态恢复，避免启动时先闪默认窗口再跳转。
- 暗色 / 浅色主题切换与本地持久化。
- 组件声明式右键菜单、程序化打开菜单、危险项二次确认，并全局屏蔽浏览器原生右键菜单。
- 通用确认弹层和模板版 `AGENTS.md` 开发规范。
- Yarn 4 单应用包管理与 `verify` 验证脚本。
- 最小 Tauri Rust 壳和 `ping` invoke 冒烟命令。

模板不包含：

- Lilia 的 Claude / Codex / CC-Switch / agent runner 业务。
- workspace、`packages/contracts`、项目 stub、聊天流、provider 配置。
- SQLite、WebDAV、托盘、小组件等 Momo 业务能力。

## 命令

```bash
yarn install
yarn dev
yarn tauri:dev
yarn verify
```

`yarn verify` 会串行运行前端测试、前端构建和 Tauri Rust 编译检查。
