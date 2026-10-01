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

`src-backend` 是当前迁移期的共享领域库，使用同一份 `models/services/viewmodels`
源码，同时被 Nana 宿主和 Tauri 适配层编译。它不依赖 Tauri；`cargo check
-p momobako-backend -p momobako-nana` 是共享边界的编译验收。服务源码仍有既存的
Clippy 基线告警，迁移期间单独清理，不通过全局 `allow` 隐藏。

Nana `ApplicationState::initialize` 现在启动共享 `RepositoryRuntime`，窗口构建使用
同一状态中的 `ShellViewModel`；启动失败会落入原生错误状态，应用销毁时释放 Runtime
辅助进程。离屏验收仍直接构造同一 `RuntimeDocument`，不启动网络服务。

`src-nana/src/services.rs` 将 repository 查询、文件浏览、交互、管理、插件、系统日志
和 Mutsuki 任务 ViewModel 全部绑定到该 Runtime；后续页面接线只需消费
`NativeServices`，不再调用 Tauri command。

原生窗口首次构建后通过 Nana `RuntimeProgramContext::run_task` 异步调用
`RepositoryQueryViewModel::list_repositories`，再请求首个资源库的
`get_repository_snapshot`。结果以 `ShellMessage` 回到应用状态，空仓库、加载错误和
文件/文件夹统计分别落入对应页面状态；未完成真实服务调用的按钮不会显示成功反馈。

当前已接通的交互还包括资源库刷新、目录浏览、文件元数据与预览源读取、播放列表查询、
播放列表删除与项目移除、插件配置读取/编辑/保存/删除、任务取消、系统日志读取和窗口
最小化/最大化/关闭。播放列表项目操作沿用 repository DTO，删除或移除后重新读取真实
服务结果，避免只修改界面文本造成假成功。

仍未宣称完成的能力包括 HostTexture/原生媒体预览器、播放列表创建/重命名/排序与添加
项目、任务进度事件流、系统设置写入、拖放、托盘、IME、Windows 原生无障碍和发布包验收；
这些需要各自的服务契约或设备证据后再接线。

插件的 Vue 自定义设置页、工具页、预览和播放器贡献不进入原生生产树。官方插件应迁移
到 Nana 原生贡献接口；第三方旧插件需要升级，宿主应提供明确的兼容提示。
