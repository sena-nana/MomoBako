# Example Plugin

这个示例演示一个最小可运行的前端预览插件工程。

## 目录

- `manifest.json`
- `src/register.js`

## 验证链路

1. 进入 `External/Plugins/` 并执行 `yarn install`
2. `yarn build`
3. `yarn package`
4. `yarn stage:dev`
5. 启动 `yarn tauri:dev:with-plugins`
6. 在插件管理中确认发现 `momobako.example.text-preview`

这个示例不会混入主前端 bundle；宿主会在运行时从 `.momoplug` 的单插件根目录内读取 `dist/register.js` 并执行 `register(ctx)`。
