# Backend Rust Template

这个模板用于原生后端插件。

## 结构

- `manifest.json`
- `Cargo.toml`
- `src/lib.rs`

## 构建

在模板复制出的插件目录内执行：

```bash
cargo build --release
```

然后将生成的动态库与 `manifest.json` 一起打入 `.momoplug`。
