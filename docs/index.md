# MomoBako

MomoBako 是一个桌面资源库工作台，面向本地素材管理、文件同步、预览、缩略图和可扩展插件能力。默认桌面端是原生壳层 `momobako-nana`。Vue 3 与 Tauri 2 界面保留一个版本周期，用来对照回归。

## 文档入口

- [架构](./architecture.md)：资源库布局、SQLite 存储、同步、缓存和插件运行时。
- [API 设计](./api-design.md)：Tauri 命令背后的资源库、文件、缩略图、插件和缓存接口。
- [插件分层 TODO](./plugin-taxonomy-todo.md)：插件类别落地后的核心宿主、解析器、服务和库类型后续任务。
- [样式标准](./design/style-standard.md)：MomoBako 工作台 UI 的样式分层与标准组件类。
- [Nana 组件对应](./nana-component-map.md)：当前构建的 Nana 组件、主题角色、图标和宿主缺口。
- [Nana 逻辑矩阵](./nana-logic-matrix.md)：从 Vue 分支迁到原生壳层的记账表。
- [Nana 迁移边界](./nana-migration.md)：默认原生启动、服务边界和尚未完成的宿主能力。
- [Nana 与 Vue 对照](./nana-vue-parity.md)：对照方法、全局取舍、没照抄的 Vue 缺陷、已知差异和 NanaUI 缺口。

## 本地开发

项目使用 Node.js 26.5.0、Corepack 0.35.0 和 Yarn 4.17.1。首次准备 Node 26 环境时需显式安装 Corepack。

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

`yarn verify` 会串行运行前端测试、前端构建和 Tauri Rust 编译检查。
