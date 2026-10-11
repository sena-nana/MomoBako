# External Plugin SDK

这里存放插件工程随包携带的最小 SDK。插件只有后端形态（`native-dylib`、`process`、`manifest-only`），不带界面代码；宿主按清单里的声明原生绘制设置、工具页、播放器和库类型面板，见 `External/Plugins/dev.md` 和 `docs/api-design.md` 的 Plugin API。

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

配置值由宿主写入插件数据目录下的 `config.json`；插件管理面板按字段画表单，后端调用时作为 `runtime.plugin_config` 交给插件。

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

模板与示例工程（`template/backend-rust/`、`example/backend-rust/`）会直接复用这个目录。
