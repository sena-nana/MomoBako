# NanaUI 原生迁移边界

MomoBako 的原生 UI 入口位于 `src-nana`。该 crate 固定使用 NanaUI 提交
`ee94106746b13f356af17586ed5e35ed78f9eb40`，依赖来自 Git revision，不读取本地
NanaUI 工作树的未提交文件。

## 渲染与验收路径

生产窗口使用 Nana `NanaApplication`、`RuntimeApplication` 和 `ApplicationState`。
界面状态进入 `RuntimeDocument`，提交为 `UiScene`，再由 `SceneWgpuPainter` 画入宿主
Surface。离屏验收使用同一份 Runtime 文档和 `RuntimeAgentSession`，通过
`HeadlessInput → RuntimeDocument.flush → OffscreenSnapshots` 生成 PNG、语义树和输入
结果；离屏路径不属于产品窗口，也不替代 Windows 原生窗口、拖放、IME、托盘和发布包验收。

运行最小原生宿主：

```text
cargo run -p momobako-nana
```

运行固定视口的浅色/深色离屏验收：

```text
cargo test -p momobako-nana --test offscreen_acceptance -- --nocapture
```

## 服务事件边界

`src-tauri/src/services/host_events.rs` 定义宿主无关的 `HostEvent` 和
`HostEventSink`。日志广播与资源库结构更新不再持有 Tauri `AppHandle`；迁移期 Tauri
入口负责将事件转发为原有的 `system://log-recorded` 和
`repository://structure-updated` 事件，原 JSON 负载保持兼容。Nana 宿主消费同一事件
边界并在自己的事件循环中更新 Runtime 状态。

`src-nana/src/host_api.rs` 进一步定义窗口生命周期、通知、文件对话框、外部打开和
取消探针。它们是宿主无关请求；Windows 原生适配器可以实现这些请求而不把窗口对象
泄漏进领域服务或 ViewModel。

插件的 Vue 自定义设置页、工具页、预览和播放器贡献不进入原生生产树。官方插件应迁移
到 Nana 原生贡献接口；第三方旧插件需要升级，宿主应提供明确的兼容提示。
