# NanaUI 原生迁移边界

MomoBako 的原生 UI 入口位于 `src-nana`。该 crate 固定使用 NanaUI 提交
`e2780d929bb452182a34d266e459958378bf363d`，依赖来自 Git revision，不读取本地
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

## 壳层视图同步

`ShellViewModel` 和 `reduce` 是唯一状态源。数据流是
`ShellMessage → reduce → 服务副作用 → ShellView::sync`，`ShellView` 在
`src-nana/src/shell/view_host.rs`，只做调度。分块、路由键和把区域改成常驻的约定见
[壳层绑定式视图](./nana-shell-view.md)。

- 骨架只在窗口文档建好时挂一次：AppShell、标题栏、工作区，以及一个停放在树外、用来认出这棵骨架的隐藏标记。
  标题栏的侧栏开关、筛选开关和角标按字段绑定；全局搜索用 `.model(Signal<String>)` 受控，ViewModel 的查询相对上次投影变了才回写信号，排在后台消息后面的按键不会被旧草稿冲掉。
- 内容分三块，各在自己的模块里，实现同一个接口 `ShellPart`（挂载、同步、是否需要重挂、组合延后）：侧栏 `view_part_sidebar.rs`，主区 `view_part_primary.rs`，浮层 `view_part_overlay.rs`。有侧栏时侧栏和主区放进工作区的资源区和主区，工作区是 AppShell 的 body；主区独占时主区自己是 body；浮层是 AppShell 的 overlay。
- 侧栏仍整块重挂，但只在 `SidebarProjection`（侧栏视图读到的全部状态）变了时才重挂。
- 主区外框常驻，里面用 `dynamic` 按 `RouteKey`（启动、设置、文件、搜索、播放集、日志、拓展、动作、丢失、空库……）切换路由分支，每个路由的入口在自己的 `route_*.rs`。启动、文件、缺失仓库、空库、搜索（含空库搜索）、设置、日志、拓展、动作和播放集页已常驻：分支只在进入路由时建一次，之后同步只写信号。文件页的列表是按条目键保留的虚拟行、卡片字段绑在卡片仓上，详情、元数据和预览页按字段绑定；播放集页的页眉和条目按字段绑定，增删和重排时只换条目列表。首页筛选栏常驻，文件、搜索、播放集和管理面板共用一份信号；文件页和播放集页里的播放条暂时是旧视图岛（`Island`）。首页外框和滚动主体只有一套（`route_home.rs`）。
- 浮层按 `OverlayIdentity`（哪个对话框、弹层或右键菜单，加上它结构上的变化）换块，身份不变时常驻、只经会话写信号，没有浮层时槽位为空。所有对话框（侧栏、文件页、导出、插件删除、关闭确认）都走统一对话框框架：NanaUI `Dialog` / `ConfirmDialog` 挂在 `OverlayHost` 下、由框架激活，关闭手势只发关闭消息，开合由归约决定。
- 重挂前后在各块的根下面记下并找回焦点、选区和滚动。焦点在某块里而且输入法还有预编辑时，这块延后重挂，`prepare` 每帧检查，组合结束后按最新状态补挂。
- `ShellViewModel::revision` 是状态版本：每次归约加一，归约之外改了界面要读的状态、标脏（`mark_surface_dirty`）时也加一，整块重挂的内容按它判断要不要重挂。
- 动效时钟、播放进度和侧栏淡入的投影在 `src-nana/src/shell/hot.rs`，结果写进信号，内容里的节点绑定这些信号。`prepare` 里只有动效帧和播放推进时只写信号、不重挂；这一帧经过归约、标了脏或者工作台排法要换时才整体同步，指针手势进行中不重挂。
- 组合控件给槽位根节点打的布局补丁只在它自己投影时写，所以槽位根节点上不放绑定：弹层动效绑在铺满浮层根的内层上，对话框的开合动效由 NanaUI 自己播（遮罩补色层用隐式过渡跟着淡入）；侧栏宽度由 `ShellView` 直接写进工作区，写完让 AppShell 重新投影一次。
- `update` 和 `prepare` 里只写信号，不调用 `flush_reactive`；刷新由输入路由和帧开头完成。
- Escape：激活的对话框由运行时直接拿到、发自己的关闭请求；其余的层走 `ApplicationState::input_event`：运行时没处理掉的 Escape 按下，按 ViewModel 里还开着的层（`escape_layer`）发一条关闭消息，焦点在哪都一样。系统文件拖放目标在视图里用 `on_mount` 登记，对话框下拉框的无障碍名用 `.labelled_by` 声明，挂载后不再扫描文档补登。

`mount_shell` 留给验收文档和测试：文档里已经有它挂过的壳层就整体同步，否则新挂一棵。

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
面板、仓库列表、摘要和硬链接候选。失败不改页面，也不弹出硬链接对话框。缺失仓库的重定向排队 `PickFolder`，编号 3，取消或空白路径不提交。这条路径没有新的离屏场景。

`src-backend` 持有 `models`、`services` 和 `viewmodels`。Nana 宿主、Tauri 窗口壳和 Eagle 源插件都依赖这个 crate，领域代码不依赖 Tauri。`cargo check -p momobako-backend -p momobako-nana` 是共享边界的编译验收。服务源码仍有既存的
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
显示快捷方式、目录、智能文件夹和播放集。15 个验收场景已改用同一套侧栏，不再保留原来的导航按钮。
启动就绪后的文件表面用 `each_virtual` 显示目录、回收站和智能文件夹。同一批验收场景在文件页也走这套表面。

当前已接通的交互还包括资源库刷新、目录浏览、文件元数据与预览源读取、播放列表查询、
播放列表创建、重命名、删除与项目移除、播放器类型选择、插件配置读取/编辑/保存/删除、任务取消、系统日志读取和窗口
最小化/最大化/关闭。播放列表项目操作沿用 repository DTO，删除或移除后重新读取真实
服务结果，避免只修改界面文本造成假成功。

图片文件已经通过 `GpuTextureView → HostTextureRegistry → GpuContext` 接入原生纹理
预览；图片字节读取和解码在服务任务中完成，上传沿用窗口唯一的 GPU 上下文。PDF 页面和能抽出三角形的网格先软件光栅，再送到同一预览纹理槽。解出的视频画面也进这个预览纹理槽。解不开的具体文件仍失败，不把截断画面当成成功。

启动就绪后，搜索面板和已选文件使用预览与元数据表面；验收页里选中文件时也挂同一块表面。文件页在选中文件或打开筛选栏时把这块表面挂到列表上。标题栏输入先进入搜索，250 毫秒后再查。元数据草稿变脏后 260 毫秒自动保存。Markdown 用
`NativeMarkdown`。纯文本和 Vue `text-preview` 一致：等宽原文、不着色，只取前 768 KiB，超出时标「仅显示前 …」；按 BOM 识别 UTF-8 / UTF-16，其余按 UTF-8 宽松解码。MomoBako 不保留 NanaUI 副本，也没有 `[patch]`。WAV、mp3、flac、ogg 预览与底部播放条共用游标，正式 Windows 构建用 winmm 出声，测试构建不开设备。未压缩和 MJPEG 的 AVI 用纯 Rust 解出画面和 PCM；其余认得出的容器在 Windows 上用媒体基础源读取器。没有画面的 m4a、aac、opus 只解音轨成 PCM，进入同一 `PlaybackSessionState`，并由 Windows 上的内置候选认领、接上播放条。解不开返回「解码失败」。认不出的字节仍是「没有原生解码器」。
ZIP/CBZ/7z/RAR/CBR、按内容流分页绘制的 PDF、Open XML 和 OLE 文档文本，以及 OBJ/glTF/GLB/STL/3MF/VRM 的网格光栅或结构摘要、FBX/BLEND 文件头，已由内置原生预览读取。Office 扩展名含 pot、ppsx、xlsb、xltx、dotm 等；抽不出文本是错误态。解不开的 PDF 流是该页失败。抽不出三角形的模型保留摘要。文件导入、Eagle 导入和 API Playground 已是原生工具页。来源账号按钮会调用插件登录方法。成功文案仍是「已调用 {method}。」；qrurl 一类地址会画成二维码，登录状态另显示，credentialRef 不显示。有字段的插件设置，以及能从 `provider.settings` 映射成字段的官方设置，可以编辑、重置并打开数据目录。映射不出字段的 Vue 设置页仍只显示升级文案。ASMR 库类型快捷方式会出现在筛选栏。素材元数据里已有的 `lyricStatus`、`listeningProgress` 等非保留字段按「键 = 值」显示；没有歌词正文，所以不另嵌歌词面板。文件右键里能直接调用的来源动作会调用插件。下载先用编号 5 的文件夹对话框，选中目录后把 `localFolder` 放进载荷再调用，取消不调用。创建来源播放列表先问名称，空白不提交，确认后带上名称和当前仓库再调用。自定义缩略图用编号 6 的选文件对话框、文本剪贴板或 `clear`，写入现有 `ensure_thumbnail`。日志画出过滤后的记录，任务行可以取消。播放集条目可以移除。播放列表内置 PCM WAV；当前正在播的视频、m4a、aac、opus 只解这一条并进入同一会话。正式 Windows 构建用 winmm 出声，测试构建不开设备。正式 Windows 构建还会注册系统媒体控件，把播放、暂停、上一首、下一首、跳转和停止写回播放条；测试构建不注册真会话。自适应和瀑布流已按视口虚拟化，行高可测、列宽均分，还不是 CSS column 瀑布流。`ImageViewer` 仍未替换行内 `GpuTextureView`：它是全窗模态，接上会更重。不嵌入 Chromium。这批状态有单测，还没有新的离屏场景。

仍未宣称完成的能力包括 PDF.js/Office/Three.js 嵌入和
Windows UIA/AccessKit 服务桥接；播放列表排序/添加、成员资格、下载进度、播放器回退、会话持久化、
仓库切换停止、任务进度快照、设置加载/校验/原子存储、宿主输入请求抽象、宿主播放会话控制器和对应离屏场景已经接入。
播放列表这批新分支有单测，还没有新的离屏场景。设置页的音频播放器、圆角、插件管理、日志筛选、任务弹层和仓库动作也有单测，还没有新的离屏场景；剪贴板写入系统剪贴板，打开和目录揭示由宿主启动系统程序。保存对话框和打开对话框会发出 `OpenFileDialog`。拖放决定和关闭确认有单测，还没有新的离屏场景。Windows 上拖出文件走系统拖放，最小化到托盘会隐藏窗口并留下托盘，主窗口位置、尺寸和最大化会在下次启动恢复。这些效果的单测不打开真窗口。IME、真窗口和发布包仍需设备矩阵验收，
离屏结果不会替代这些设备证据。

插件的 Vue 自定义设置页、工具页、预览和播放器贡献不进入原生生产树。官方插件应迁移
到 Nana 原生贡献接口；第三方旧插件需要升级，宿主应提供明确的兼容提示。

继续迁移时先读 [组件对应](./nana-component-map.md) 和 [逻辑矩阵](./nana-logic-matrix.md)。组件目录以
`src-nana/src/capability.rs` 为准，颜色和字号以 `src-nana/src/theme_map.rs` 为准。离屏清屏色必须等于当前主题的
`background`，不能靠按钮上的写死颜色表示主题。
