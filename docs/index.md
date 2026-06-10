# MomoBako

MomoBako 是一个基于 Tauri 2、Vue 3 与 TypeScript 的桌面资源库工作台，面向本地素材管理、文件同步、预览、缩略图和可扩展插件能力。

## 文档入口

- [架构](./architecture.md)：资源库布局、SQLite 存储、同步、缓存和插件运行时。
- [API 设计](./api-design.md)：Tauri 命令背后的资源库、文件、缩略图、插件和缓存接口。
- [样式标准](./design/style-standard.md)：MomoBako 工作台 UI 的样式分层与标准组件类。

## 本地开发

```bash
yarn install
yarn dev
yarn tauri:dev
yarn verify
```

`yarn verify` 会串行运行前端测试、前端构建、Tauri Rust 编译检查和内置插件构建。
