# Backend Rust Template

这个模板用于原生后端插件（`runtime: "native-dylib"`），是插件工程唯一的复制起点：插件只有后端形态，不带界面代码。

## 结构

- `manifest.json`
- `Cargo.toml`
- `src/lib.rs`

## 构建

复制成 `External/Plugins/<plugin-slug>/` 以后，照现有原生插件（例如 `parser-asmr-folder/`）补上 `plugin.project.json` 和 Mutsuki `plugin.toml`，把目录加进根 `Cargo.toml` 的 `workspace.members`，再用 `External/Plugins` 的 `yarn build`、`yarn package` 打成 v2 `.momoplug`。步骤见 `External/Plugins/dev.md`。
