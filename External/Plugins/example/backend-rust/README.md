# Backend Rust Example

这个示例是最小的 Mutsuki ABI v2 后端插件：`ping` 方法交回 `pong`。

它不在根 workspace 里，也不经 `yarn build` 构建。要打成可加载的 v2 `.momoplug`，照 `External/Plugins/dev.md` 第 9 节补上 `plugin.project.json` 和 `plugin.toml`、加进根 `Cargo.toml` 的 `workspace.members`，再 `yarn build`、`yarn package`。`native-dylib` 只能作为随附插件加载（宿主从 `.packages` 或发布包同步进 `plugins/builtin`）；用户安装的原生插件要改成 `process` 部署。
