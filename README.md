# MomoBako

MomoBako 是一个桌面资源库工作台。默认桌面端是原生壳层 `momobako-nana`。Vue 3 与 Tauri 2 界面保留一个版本周期，用来对照回归。

当前工程包含：

- 自绘标题栏、可拖拽侧栏、紧凑工作台 UI。
- 主窗口位置、尺寸与最大化状态恢复，避免启动时先闪默认窗口再跳转。
- 暗色 / 浅色主题切换与本地持久化。
- 组件声明式右键菜单、程序化打开菜单、危险项二次确认，并全局屏蔽浏览器原生右键菜单。
- 通用确认弹层和 `AGENTS.md` 开发规范。
- Yarn 4 单应用包管理与 `verify` 验证脚本。
- Tauri Rust 服务、SQLite 资源库、文件同步、托盘和媒体预览能力。

## 命令

项目工具链固定为 Node.js 26.5.0、Corepack 0.35.0 和 Yarn 4.17.1。Node.js 26 不再内置 Corepack，首次使用前需显式安装。

```bash
npm install --global corepack@0.35.0
corepack enable
corepack yarn install --immutable
cargo run -p momobako-nana
yarn dev
yarn tauri:dev
yarn verify
```

`cargo run -p momobako-nana` 从仓库根目录启动默认桌面端，服务数据写在当前目录的 `.service-data`。`yarn dev` 仅启动 Vite 前端。`yarn tauri:dev` 仍会先完整构建、打包并暂存外置插件，再启动 Tauri 对照窗口。

原生插件共享仓库根 `Cargo.lock` 与 `target/`。日常增量调试可运行 `yarn plugins:build:dev <目录名或 pluginId>`；发布产物使用 `yarn plugins:build` 与 `yarn plugins:package`，生成带目标三元组的可复现 v2 `.momoplug`。

`yarn verify` 会串行运行前端测试、前端构建和 Tauri Rust 编译检查。
