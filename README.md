# MomoBako

MomoBako 是一个桌面资源库工作台。桌面端是 NanaUI 原生壳层 `momobako-nana`（`src-nana/`），领域服务在 `src-backend/`（crate `momobako-backend`），插件在 `External/Plugins/`。原来的 Vue 3 与 Tauri 2 界面已经删掉，Vue/Tauri 源码最后一次出现在提交 `7bc847c`。

当前工程包含：

- 自绘标题栏、可拖拽侧栏、紧凑工作台 UI。
- 主窗口位置、尺寸与最大化状态恢复，避免启动时先闪默认窗口再跳转。
- 暗色 / 浅色主题切换与本地持久化。
- 右键菜单（键盘也能打开）和危险项二次确认。
- 通用确认弹层和 `AGENTS.md` 开发规范。
- Yarn 4 管理文档站、测试和插件工具链，以及 `verify` 验证脚本。
- Rust 领域服务、SQLite 资源库、文件同步、托盘和媒体预览能力。

## 命令

项目工具链固定为 Node.js 26.5.0、Corepack 0.35.0 和 Yarn 4.17.1。Node.js 26 不再内置 Corepack，首次使用前需显式安装。

```bash
npm install --global corepack@0.35.0
corepack enable
corepack yarn install --immutable
(cd External/Plugins && corepack yarn install --immutable)
corepack yarn plugins:build
corepack yarn plugins:package
cargo run -p momobako-nana
corepack yarn verify
corepack yarn test:backend
```

`cargo run -p momobako-nana` 从仓库根目录启动桌面端，服务数据写在当前目录的 `.service-data`。随附插件读插件打包输出 `External/Plugins/.packages`，启动时同步进服务目录的 `plugins/builtin`；第一次运行前先 `yarn plugins:build`、`yarn plugins:package`，本地文件夹资源库也要靠其中的 `local-filesystem` 插件。

原生插件共享仓库根 `Cargo.lock` 与 `target/`。日常增量调试可运行 `yarn plugins:build:dev <目录名或 pluginId>`；发布产物使用 `yarn plugins:build` 与 `yarn plugins:package`，生成带目标三元组的可复现 v2 `.momoplug`。`yarn verify` 不需要先打包插件。插件开发见 `External/Plugins/dev.md`。

发布包是 `momobako-nana`，用 NanaUI 的 `nana-packager` 打：先 `yarn plugins:build`、`yarn plugins:package`，再 `yarn nana:build`、`yarn nana:package`，产出便携版和安装版布局两个 zip，见 `docs/nana-packaging.md`。

`yarn verify` 会串行运行 Node 脚本类型检查、测试（工具链、打包、文档和插件清单）和 `momobako-backend`、`momobako-nana` 的编译检查。`yarn test:backend` 运行 `momobako-backend` 的单元测试（`cargo test -p momobako-backend --locked`），不在 `yarn verify` 里，CI 单独执行。原生界面的测试用 `cargo test -p momobako-nana`，见 `docs/nana-migration.md`。
