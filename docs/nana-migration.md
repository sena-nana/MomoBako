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

默认桌面端从仓库根目录启动。Vue 与 Tauri 对照窗口仍用 `yarn tauri:dev`，这次不删除前端。

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
泄漏进领域服务或 ViewModel。Nana 启动后把 `HostEvent` 接回自己的事件循环：日志按
id 合并，启动中的 `repository.sync` 追加到启动日志，就绪后的结构更新静默刷新当前
面板。这条路径不重拉仓库列表、摘要和硬链接候选。

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
`RepositoryQueryViewModel::list_repositories`。列表里选中的非缺失仓库先执行
`PROTOCOL_REPOSITORY_SYNC`，同步成功后再请求 `get_repository_snapshot`，摘要仍属于当前仓库时才读取根目录。
缺失仓库和空仓库在列表完成后结束启动，不进入同步。结果以 `ShellMessage` 回到应用状态；
未完成真实服务调用的按钮不会显示成功反馈。启动就绪后的实况侧栏用 `SidebarRow` 和 `TreeView`
显示快捷方式、目录、智能文件夹和播放集；15 个旧验收场景仍用 `acceptance_scene` 保留原来的导航按钮。
启动就绪后的实况文件表面用 `each_virtual` 显示目录、回收站和智能文件夹；同一批验收场景继续使用原来的 `file_actions` 列表。

当前已接通的交互还包括资源库刷新、目录浏览、文件元数据与预览源读取、播放列表查询、
播放列表创建、重命名、删除与项目移除、播放器类型选择、插件配置读取/编辑/保存/删除、任务取消、系统日志读取和窗口
最小化/最大化/关闭。播放列表项目操作沿用 repository DTO，删除或移除后重新读取真实
服务结果，避免只修改界面文本造成假成功。

图片文件已经通过 `GpuTextureView → HostTextureRegistry → GpuContext` 接入原生纹理
预览；图片字节读取和解码在服务任务中完成，上传沿用窗口唯一的 GPU 上下文。非图片媒体
仍显示明确的“不支持原生纹理预览”状态，不伪造成功。

启动就绪且不是 `acceptance_scene` 时，实况预览和搜索替换原来的预览占位。Markdown 用
`NativeMarkdown`，纯文本用 `SelectableRichText`。音视频没有原生解码器时停在失败态。
PDF、Office、压缩包和三维模型在登记 Nana 原生预览贡献之前显示升级提示。这批状态有单测，还没有新的离屏场景。

仍未宣称完成的能力包括语法高亮、真实媒体解码器、系统媒体会话、PDF.js/Office/Three.js 嵌入和
Windows UIA/AccessKit 服务桥接；播放列表排序/添加、成员资格、下载进度、播放器回退、会话持久化、
仓库切换停止、任务进度快照、设置加载/校验/原子存储、宿主输入请求抽象、宿主播放会话控制器和对应离屏场景已经接入。
播放列表这批新分支有单测，还没有新的离屏场景。设置页的音频播放器、圆角、插件管理、日志筛选、任务弹层和仓库动作也有单测，还没有新的离屏场景；剪贴板和目录揭示没有宿主桥，保存对话框和打开对话框会发出 `OpenFileDialog`。拖放决定和关闭确认有单测，还没有新的离屏场景。拖放、托盘、IME、真窗口和发布包仍需设备矩阵验收，
离屏结果不会替代这些设备证据。

插件的 Vue 自定义设置页、工具页、预览和播放器贡献不进入原生生产树。官方插件应迁移
到 Nana 原生贡献接口；第三方旧插件需要升级，宿主应提供明确的兼容提示。

继续迁移时先读 [组件对应](./nana-component-map.md) 和 [逻辑矩阵](./nana-logic-matrix.md)。组件目录以
`src-nana/src/capability.rs` 为准，颜色和字号以 `src-nana/src/theme_map.rs` 为准。离屏清屏色必须等于当前主题的
`background`，不能靠按钮上的写死颜色表示主题。
