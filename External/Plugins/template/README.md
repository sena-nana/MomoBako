# Template Plugin

这个目录是新插件工程的复制起点。

## 结构

- `manifest.json`: manifest v2
- `src/register.js`: 前端入口，导出 `register(ctx)`
- `src/chunks/`: 可选拆分模块

## 开发

1. 复制 `template/` 为新的 `External/Plugins/<plugin-slug>/`
2. 修改 `manifest.json`
3. 在 `src/register.js` 中实现插件注册
4. 在 `External/Plugins/` 内执行 `yarn build`
5. 在 `External/Plugins/` 内执行 `yarn package`
6. 将输出的 `.momoplug` 复制到 `<serviceRoot>/plugins`

模板默认面向前端 `vue-module` 插件，宿主会在运行时直接读取包内 `dist/register.js`。
