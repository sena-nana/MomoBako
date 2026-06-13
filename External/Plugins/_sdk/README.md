# External Plugin SDK

这里存放插件工程随包携带的最小 SDK。

当前宿主约定：

- 前端入口导出 `register(ctx)`
- `ctx.manifest` 为当前插件 manifest
- `ctx.registerPreview(definition)` 用于注册预览插件
- `ctx.registerPlaylistPlayer(definition)` 用于注册播放列表播放器；播放器运行时可实现可选 `configure(settings)`，当前设置形状为 `{ imageDurationMs?: number, objectFit?: "contain" | "cover" }`
- `ctx.defineLazyComponent(loader)` 可声明按需组件
- `ctx.loadModule(path)` 可按需读取同包内其他 JS 模块
- `ctx.callPlugin(request)` 可调用后端插件能力

目录：

- `backend-rust/`
  - 后端原生插件 Rust SDK
  - 提供 C ABI 请求/响应辅助

模板与示例工程会直接复用这个目录，而不依赖主工程内旧 `plugins/backend-sdk`。
