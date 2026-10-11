# MomoBako

MomoBako 是一个桌面资源库工作台，面向本地素材管理、文件同步、预览、缩略图和可扩展插件能力。桌面端是 NanaUI 原生壳层 `momobako-nana`。原来的 Vue 3 与 Tauri 2 界面已经删掉，Vue/Tauri 源码最后一次出现在提交 `7bc847c`。

## 文档入口

- [架构](./architecture.md)：资源库布局、SQLite 存储、同步、缓存和插件运行时。
- [API 设计](./api-design.md)：宿主在进程内调用的资源库、文件、缩略图、插件和缓存接口，以及外部 HTTP API 和插件合同。
- [插件分层 TODO](./plugin-taxonomy-todo.md)：插件类别落地后的核心宿主、解析器、服务和库类型后续任务。
- [样式标准](./design/style-standard.md)：Nana 原生界面的样式分层、标准组件和扩展规则。
- [Nana 迁移边界](./nana-migration.md)：原生端的入口和运行命令、渲染和验收路径、服务和宿主事件边界、按领域列出的已接通能力、还没做和要靠设备验证的部分，以及怎么验证。
- [Nana 壳层绑定式视图](./nana-shell-view.md)：常驻壳层的分块、路由、浮层和对话框框架、全局状态区，以及把一块区域改成常驻的约定。
- [Nana 组件对应](./nana-component-map.md)：当前构建编译的 Nana 组件、产品用到的组件、旧框架对应、主题、图标和没有现成组件的部分。
- [Nana 逻辑矩阵](./nana-logic-matrix.md)：从 Vue 分支迁到原生壳层的记账表，每行写触发、服务、结果、可见性和验证状态。
- [Nana 离屏验收证据](./nana-offscreen-evidence.md)：离屏验收的命令、场景和证据文件，`scene_shots` 按场景出图，以及实况视觉和点击测试。
- [Nana 与 Vue 对照](./nana-vue-parity.md)：对照方法、全局取舍、没照抄的 Vue 缺陷、已知差异和 NanaUI 缺口。
- [Nana 设备矩阵](./nana-device-matrix.md)：离屏验收替代不了的设备验收项：真窗口、拖放和文件对话框、剪贴板和外部打开、托盘、IME、UIA/AccessKit。
- [Nana 发布包](./nana-packaging.md)：用 `nana-packager` 打的两个 zip、包里的插件和外部程序、数据目录、本地打包和发布流程。

## 本地开发

项目使用 Node.js 26.5.0、Corepack 0.35.0 和 Yarn 4.17.1。首次准备 Node 26 环境时需显式安装 Corepack。

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

`cargo run -p momobako-nana` 从仓库根目录启动桌面端，服务数据写在当前目录的 `.service-data`，随附插件读 `External/Plugins/.packages`，所以第一次运行前先构建并打包插件。

`yarn verify` 会串行运行 Node 脚本类型检查、测试和 `momobako-backend`、`momobako-nana` 的编译检查。`yarn test:backend` 运行 `momobako-backend` 的单元测试（`cargo test -p momobako-backend --locked`），不在 `yarn verify` 里，CI 单独执行。
