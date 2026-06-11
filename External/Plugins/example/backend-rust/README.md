# Backend Rust Example

这个示例演示最小后端插件链路：

1. `cargo build --release`
2. 将生成的动态库与 `manifest.json` 一起打入 `.momoplug`
3. 复制到 `<serviceRoot>/plugins`
4. 由宿主在运行时按 C ABI 加载
