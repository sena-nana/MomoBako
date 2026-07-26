# External Plugin SDK

这里存放插件工程随包携带的最小 SDK。

当前宿主约定：

- 前端入口导出 `register(ctx)`
- `ctx.manifest` 为当前插件 manifest
- `ctx.registerPreview(definition)` 用于注册预览插件
- `ctx.registerPlaylistPlayer(definition)` 用于注册播放列表播放器；播放器运行时可实现可选 `configure(settings)`，当前设置形状为 `{ imageDurationMs?: number, objectFit?: "contain" | "cover" }`
- `ctx.registerToolPage(definition)` 用于注册插件工具页
- `ctx.registerSettingsPage(definition)` 用于注册插件自定义设置页
- `ctx.defineLazyComponent(loader)` 可声明按需组件
- `ctx.loadModule(path)` 可按需读取同包内其他 JS 模块
- `ctx.getApiDesignSnapshot()` 可读取宿主 API 契约快照；快照包含 `external-http`、`tauri-command` 和插件声明的 `plugin-call`
- `ctx.getExternalApiConnectionStatus()` 可读取本机外部 API 连接状态
- `ctx.getPluginDataDirectory()` 可读取当前插件自有数据目录
- `ctx.getPluginConfig()`、`ctx.setPluginConfigValue(key, value)`、`ctx.deletePluginConfigValue(key)` 可访问当前插件的 host-managed key-value 配置
- `ctx.invokeCommand(command, args?)` 可调用宿主 Tauri 命令
- `ctx.callPlugin(request)` 可调用后端插件能力

插件可通过 manifest 声明宿主设置 schema：

```json
{
  "contributes": {
    "settings": {
      "schemaVersion": 1,
      "settingsPage": {
        "label": "插件设置"
      },
      "fields": [
        {
          "key": "apiKey",
          "label": "API Key",
          "type": "string"
        },
        {
          "key": "enabled",
          "label": "启用同步",
          "type": "boolean",
          "default": true
        }
      ]
    }
  }
}
```

配置值由宿主写入插件数据目录下的 `config.json`；前端插件和插件管理面板共享同一套 key-value API。

后端插件如需让 API Playground 自动识别自己的测试入口，可在 manifest 中声明：

```json
{
  "contributes": {
    "apiTests": [
      {
        "method": "provider.lookupMetadataCandidate",
        "summary": "查询元数据候选。",
        "payload": { "id": "RJ123456" }
      }
    ]
  }
}
```

目录：

- `mutsuki-rust/`
  - 后端原生插件 Mutsuki ABI v2 Rust 适配 SDK
  - 提供 Runner、protocol/binding 和 DomainEvent 日志适配
  - `PluginCallEnvelope.runtime.plugin_data_dir` 指向同一个插件数据目录；`runtime.plugin_config` 是宿主从 `config.json` 读取的当前 key-value 配置快照

模板与示例工程会直接复用这个目录。
