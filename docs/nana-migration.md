# NanaUI 原生迁移边界

Vue/Tauri 界面（`src/`、`src-tauri/`）正在迁到 NanaUI 原生界面（`src-nana/`）。本文写原生端现在的入口、渲染和验收路径、服务边界、已经接通的能力、还没做或要靠设备验证的部分，以及怎么验证。逐条分支记在 [逻辑矩阵](./nana-logic-matrix.md)，壳层视图的结构在 [壳层绑定式视图](./nana-shell-view.md)，组件目录在 [组件对应](./nana-component-map.md)，和 Vue 的取舍在 [Nana 与 Vue 对照](./nana-vue-parity.md)。

## 入口和运行

- 原生入口是 crate `momobako-nana`：`src-nana/src/main.rs` 调 `momobako_nana::run()`（`src-nana/src/lib.rs`）。
- NanaUI 固定在提交 `c6dc1086bb1b21406ee5e20a40ac6207c664913b`。`src-nana/Cargo.toml` 的 `nana-ui`、`nana-ui-core`、`nana-ui-platform` 和开发依赖 `nana-ui-devtools` 都按这个 `rev` 从 Git 取。仓库里没有 NanaUI 副本，根 `Cargo.toml` 也没有 `[patch]`。

在仓库根目录运行：

```text
cargo run -p momobako-nana   # 原生窗口
yarn tauri:dev               # Vue/Tauri 对照窗口，先构建、打包并暂存外置插件
```

原生窗口读写的本地文件：

| 内容 | 位置 | 代码 |
| --- | --- | --- |
| 领域服务数据 | 当前目录下的 `.service-data` | `src-backend/src/services/runtime/mod.rs` |
| 应用设置 | `%LOCALAPPDATA%\MomoBako\settings.json`，没有这个变量时依次退到 `%APPDATA%`、当前目录 | `src-nana/src/settings.rs` |
| 侧栏、展示方式、圆角 | 设置文件旁的 `sidebar.json`、`file-display.json`、`corners.json` | `workspace.rs`、`files_actions.rs`、`admin_support.rs` |
| 播放设置、播放会话、播放器偏好 | 设置文件旁的 `playback-settings.json`、`playback-sessions.json`、`playlist-player-preferences.json` | `player_support.rs` |
| 主窗口位置、尺寸和最大化 | 应用数据目录下的 `com.momobako.desktop/nana-main-window-state.json` | `src-nana/src/window_state.rs` |

## 渲染和验收路径

- 产品窗口：`run()` 用 `NanaApplication::builder(..).run::<RuntimeApplication<MomoBakoApplication>>(..)`。`MomoBakoApplication` 实现 `ApplicationState`：`initialize` 启动领域服务，读应用设置和本地偏好；`build` 在窗口文档里挂常驻壳层（`ShellView::mount`），提交读资源库列表的任务，建托盘；`update` 先做服务派发，再归约、执行副作用、同步视图；`prepare` 每帧推进计时器和动效，派发预取和目录请求，上传预览和缩略图纹理。界面进 `RuntimeDocument`，NanaUI 把它提交成 `UiScene`，由 `SceneWgpuPainter` 画到宿主 Surface。
- 离屏和测试不开窗口：`acceptance_document_for_model`（`lib.rs`）新建一份 `RuntimeDocument`，用 `mount_shell` 挂壳层（和产品窗口一样走 `ShellView::mount`），再交给 `nana-ui-devtools` 的 `RuntimeAgentSession` 布局、出图、读无障碍树和点击。这条路径不启动领域服务和网络服务，验收场景的服务应答由场景按夹具送回（`acceptance_base.rs`）。
- 离屏结果不替代 Windows 真窗口、拖放、IME、托盘、UIA/AccessKit 和发布包的验收，见 [设备矩阵](./nana-device-matrix.md)。

## 壳层视图同步

`ShellViewModel` 是唯一状态源。数据流是 `ShellMessage → reduce → 服务副作用 → ShellView::sync`（`src-nana/src/shell/view_host.rs`）。

- 骨架（AppShell、标题栏、工作区、浮层层）只挂一次。内容分侧栏、主区、浮层三块，都常驻，同步只写信号和 Store。主区按 `RouteKey` 切换路由分支，分支在进入路由时建一次。
- 还会整块换的只有两处：浮层按 `OverlayIdentity` 换块；文件页和播放集页里的播放条是旧视图岛，按 `player_view::bar_stamp` 重建。
- 对话框都是 NanaUI `Dialog` / `ConfirmDialog`（`overlay_dialog.rs`），常驻在浮层层下，靠 `open` 开合，换下的放完退场才卸。距顶、最高、圆角、分区、遮罩和开合动效由主题配方给出（`src-nana/src/appearance.rs`）。
- `reduce` 和 `mark_surface_dirty` 都把 `ShellViewModel::revision` 加一。`prepare` 看到版本或工作台排法变了才整体同步；指针手势进行中、只有动效或播放推进的帧只写热信号（`hot.rs`）。

分块、路由、浮层会话、全局状态区和把一块区域改成常驻的步骤见 [壳层绑定式视图](./nana-shell-view.md)。

## 服务和宿主事件

- 领域代码在 `src-backend`（crate `momobako-backend`，含 `models`、`services`、`viewmodels`）。Nana 宿主、Tauri 窗口壳和 Eagle 来源插件（`External/Plugins/source-eagle-library`）都依赖它，领域代码不依赖 Tauri。
- `NativeServices::start`（`src-nana/src/services.rs`）启动一份 `RepositoryRuntime`，把资源库查询、文件浏览、交互、管理、插件、系统和 Mutsuki 任务 ViewModel 都绑在它上面。壳层只经这些 ViewModel 调服务，不调 Tauri command。启动失败时窗口进错误页（`initialize`），`NativeServices` 销毁时关掉 Runtime 的辅助进程。
- 归约留下的副作用由 `src-nana/src/*_dispatch.rs` 交给 `RuntimeProgramContext::run_task`，结果作为 `ShellMessage` 回到归约。
- 宿主事件边界在 `src-backend/src/services/host_events.rs`：`HostEvent`（日志记录、资源库结构更新）和 `HostEventSink`。Tauri 入口（`src-tauri/src/app_shell.rs`）按原事件名 `system://log-recorded`、`repository://structure-updated` 和原 JSON 负载转给前端。Nana 用 `host_event_channel` 接收，`NativeServices::pump_host_events` 在独立线程里把事件转成 `ShellMessage::Host`。Mutsuki 任务运行时只存快照、不发变化通知，`NativeServices::watch_tasks` 另起一个线程每 250 ms 读一次（`src-nana/src/task_watch.rs`），排队、运行和取消中的任务有变化才发 `ShellMessage::TaskProgressLoaded`，服务销毁时线程退出。日志合并、启动日志和结构更新后的静默重读见逻辑矩阵的「宿主事件」。
- 宿主请求在 `src-nana/src/host_api.rs`。外部打开和目录揭示（`OpenExternal`）、拖出和最小化到托盘（`HostInputRequest`）由 `host_bridge::perform` 执行，每次执行都清空队列，执行不了的请求记日志后丢掉；关闭确认是浮层里的确认框，只看 `InputState::pending_close`，不经宿主请求；窗口最小化、最大化和关闭经 `host_api::WindowCommand` 换成 Nana 平台命令。系统文件对话框直接排成 Nana 平台的 `WindowCommand::OpenFileDialog`，编号 1–8 在 `src-nana/src/shell/input_support.rs`。

## 已经接通的能力

下文的「Windows 非测试构建」指 `cfg(all(windows, not(test)))`：只有单元测试构建（`cfg(test)`）不开声卡、不注册系统媒体会话。代码路径都在 `src-nana/src/shell/` 下，另有说明的除外。

- **启动**：四步依次读资源库列表、同步仓库（`PROTOCOL_REPOSITORY_SYNC`）、读仓库摘要、读首屏目录；缺失仓库和空列表不进同步。失败停在当前步，保留已完成的步骤，给「重试」；启动页显示最近 8 条加载日志。见 `workspace.rs`、`workspace_startup.rs`、`route_startup.rs`。
- **侧栏**：仓库切换弹层（切换、添加资源库、删除当前资源库）；快捷方式（全部、未分类、未标签、最近使用、回收站）带计数，以及快捷访问；文件夹树和智能文件夹树是按显示顺序展开的扁平行，文件夹悬停 450 ms 打开；文件夹新建、重命名、删除，智能文件夹新建、编辑、删除，播放集新建、打开、播放、删除（新建对话框的类型和「播放」能不能点看读回的播放器类型）；「刷新文件夹树」同步整个仓库再重读（`tree_sync.rs`、`src-nana/src/sync_dispatch.rs`）；顶部的全局状态区显示没有就近显示的失败、读取中和同步进度（`status.rs`）；折叠和宽度（220–480，默认 276）写进 `sidebar.json`；启动和换仓库绑定侧栏时照 Vue 在后台读仓库动作和硬链接候选，有仓库动作时多一个「动作」入口，有没确认的硬链接候选时弹出确认。见 `sidebar*.rs`、`view_part_sidebar.rs`。
- **文件**：文件页用 `each_virtual` 按视口建行，列表、网格、自适应、瀑布流四种展示写进 `file-display.json`。单击选中，Shift 单击从锚点选一段，Ctrl / Meta 单击切换（Vue `selectionModeFromEvent`；修饰键由宿主在输入路由完以后记下，`window_host::note_modifiers`），右侧详情跟着主选中项看元数据；双击或右键「预览」进预览页。工具栏有「建文件」和导入（文件夹、ZIP、Eagle 复制或剪切），回收站里有「还原所有项目」「清空回收站」；右键菜单有预览、打开、定位、还原、复制到、加入播放列表、缩略图（刷新、自定义）、重命名、删除和来源插件的条目动作（`entry_actions.rs`）；条目可以拖进文件夹移动，可以框选（按下时按着 Ctrl / Meta 并进原来的选择）；有硬链接候选时弹出确认。仓库导出对话框能提交压缩包和 Git 导出，和 Vue 一样首页没有入口。见 `route_files.rs`、`files_*.rs`。
- **预览**：预览页按扩展名分派（`inspect_support.rs`）。图片在产品窗口里经宿主纹理槽 `file-preview` 用 `GpuTextureView` 显示；文本和 Markdown 都按等宽原文显示前 768 KiB；WAV 用自有解析，MP3、FLAC、Ogg 用 symphonia，Windows 上 M4A、AAC、Opus 和其余视频容器用媒体基础（`video_mf.rs`），未压缩和 MJPEG 的 AVI 用纯 Rust（`video_avi.rs`），视频画面进同一个纹理槽；压缩包（zip、cbz、7z、rar、cbr）最多列 200 条；PDF 按内容流画页、可翻页；Office 的 Open XML 和 OLE 文件抽文本；OBJ、glTF、GLB、STL、3MF、VRM 能取出三角形时软件光栅、可旋转缩放，FBX、BLEND 只报文件头（`native_preview*.rs`）。PDF 页、模型光栅和图片幻灯片的帧编码成 PNG 画进预览框（`inspect_preview_page.rs`、`inspect_preview_body.rs`）。读不出、解不开时预览框里写失败标题和原因。
- **元数据**：右侧详情和预览页共用一块元数据编辑：注释、链接、评分和标签可以改，自定义字段只读显示；草稿变脏 260 ms 后自动保存（`update_asset_metadata`，带 `expected_version`），换选前先存上一份，画面上没有保存按钮；版本冲突时保留本地草稿，给「采用服务器版本」。见 `files_detail.rs`、`inspect_metadata_*.rs`、`inspect.rs`。
- **搜索**：标题栏输入先切到搜索面板（`route_search.rs`），250 ms 后调 `search_assets`；首页筛选栏（格式、标签、颜色、形状、评分和高级条件）常驻在文件、搜索、播放集、日志、拓展和动作页；插件登记的库类型快捷方式（ASMR 等）进筛选栏（`inspect_shortcuts.rs`）；跑完没有命中时写明哪里没有匹配的文件；点结果回到文件页并选中。见 `inspect_search*.rs`、`search_*.rs`。
- **播放**：播放条和预览共用一份 `PlaybackSessionState`，预览里解得开的音视频接管播放条（`player_preview.rs`）；Windows 非测试构建经 winmm 出声（`wav_player.rs`），并注册系统媒体传输控件（`system_media.rs`）。侧栏绑定仓库时读播放集列表，新建、删除以后换上返回的整份列表，侧栏和播放器共用这一份（`apply_playlist_list`）：播放器据此读成员索引（文件右键「加入播放列表」），存下的会话所在的播放集在列表里、播放器类型也认得时读它的详情来恢复；新建以后照 Vue 接着点开它。点开侧栏的播放集进播放集页，条目用 `each(..).container(ReorderList)` 建，可以拖动排序、移除；当前项只读、只解这一条（`player_clip.rs`、`src-nana/src/player_dispatch.rs`）；图片幻灯片有停留时长和适应 / 填充。会话写进 `playback-sessions.json`。见 `player*.rs`、`route_playlists.rs`。
- **设置**：侧栏「设置」和缺失仓库的「打开来源设置」都进设置页（`ShellMessage::OpenSettings`、`admin::open_settings_page`），后者照 Vue `?plugin=` 展开来源插件的设置。设置页有音频播放、外观（主题、圆角，改了立即生效）、仓库服务、外部素材接入（复制连接信息、导出 `external-api.json`）、插件管理、缓存和 API 设计。主题写进 `settings.json`，圆角写进 `corners.json`。见 `route_settings.rs`、`admin_settings_view.rs`。
- **插件**：启动结束时照 Vue `loadSettingsData` 读一次设置包（插件、钩子记录、缓存、API 设计和外部连接），打开设置页、插件面板「刷新」时再读；插件列表换新以后重读播放器类型（`list_playlist_players`），新建播放集的类型、播放器贡献和插件类型的播放集能不能播都从这里来。插件管理面板（设置页和拓展页都有）分组和搜索，能启停、从压缩包安装（编号 2 的文件对话框）、确认后删除、编辑和重置清单声明的设置字段、打开插件目录；来源插件的扫码登录画二维码（`source_auth_page.rs`）。拓展页的 API Playground、文件导入、Eagle 导入是原生页面。见 `admin_plugin*.rs`、`admin_gap.rs`、`tool_native.rs`、`api_playground*.rs`。
- **日志**：日志页按级别、来源和关键字筛选，可以暂停追踪、清空；页头、工具条和筛选固定，列表自己滚动（`route_home::home_fixed`、`admin_logs_view.rs`）。每次切到日志面板照 Vue 读最近 200 条历史日志（读的时候手上没有日志就写「正在加载系统日志」），之后经宿主事件合并新记录。
- **任务**：侧栏底部的「任务」打开任务弹层（`admin_view.rs`），列出当前的仓库操作（刷新文件夹树，或复制、移动、导入、多选删除和还原这些文件变更，`operation_row_tests.rs`）和宿主观察到的运行中任务（排队、运行和取消中，结束就拿掉）；「任务」右上角的计数是弹层的行数，和 Vue 的 `activeTaskCount` 一样。弹层里没有取消按钮，Vue 的 `TaskPopover.vue` 也没有；点弹层外面、头部的关闭按钮和 Escape 都关掉它。
- **宿主操作**：标题栏的最小化、最大化、关闭；关闭行为按设置确认、退出或最小化到托盘（`src-nana/src/window_host.rs`）；托盘在 Windows 上用 `tray-icon`（`src-nana/src/tray.rs`）；主窗口几何下次启动恢复；打开和定位启动系统程序；拖出文件在 Windows 上用 `drag` crate（`src-nana/src/drag_out.rs`）；剪贴板读写系统剪贴板（`src-nana/src/host_bridge.rs`）；文件区和空库页接收系统文件拖入（`input::accept_file_drops`）；Escape 由运行时交给激活的对话框，其余按 `escape_layer` 关最上面一层。

## 还没做或要靠设备验证的部分

### 接线缺口

归约和服务派发已经写好、但产品界面触发不到的路径，以及 Vue 有、Nana 还没接上的反馈：

- 任务弹层的仓库操作行只接了刷新文件夹树和文件变更。Vue 读目录（`读取目录 / 读取回收站 / 读取文件树`）、加载资源库、同步资源库、导入和挂载资源库、重定向资源库、来源下载时也在这一行出进度，Nana 这些路径不出。
- `ShellMessage::Refresh`（重读资源库列表）有归约和派发，没有发送方，只有测试在用。
- `InspectMessage::RegisterPreview`（登记插件的预览贡献）没有发送方：预览贡献只有内置的那几项（`native_preview::builtin_bindings`），插件声明的预览贡献不会登记，「原生预览 · 名称 · 视图 id」在产品里出不来。
- `ShellMessage::SetSidebarWidth`、`CommitSidebarWidth` 和 `SetLibraryCategory` 没有发送方：侧栏宽度由宿主在准备帧里直接写进工作台（`window_host::sync_sidebar_resize`），分类视图由侧栏快捷方式直接切换。

### 设备验证和其它

- **IME**：输入法组合期间延后重挂和换块（`ShellPart::composing`），由单测覆盖；真窗口里的中文输入法没有验收。
- **真窗口**：窗口几何恢复、关闭行为和托盘在真窗口里的效果没有验收；测试只断言排出的窗口命令和宿主请求，不开真窗口。
- **拖放**：拖入的放置决定由输入归约测试覆盖，拖出调用系统拖放；和资源管理器之间的真实拖放没有验收。
- **托盘**：只在 Windows 上建；点击和菜单没有在真窗口里验收。
- **UIA/AccessKit**：`hosted` 带 AccessKit 平台适配器；测试只读 NanaUI 自己的无障碍树，读屏器看到的结果没有验收。
- **发布包**：仓库里没有 `momobako-nana` 的打包脚本；CI 和发布流程（`.github/workflows/`）只跑 `yarn verify` 和 Tauri 打包。
- **全局快捷键**：没有实现。`host_api::HostInputRequest::RegisterShortcut` 没有调用方。
- **Vue 插件组件不进原生树**：插件前端注册的 Vue 组件（设置页、工具页、预览、播放器）不在原生树里运行。插件设置只画清单 `contributes.settings.fields` 声明的字段；第三方工具页只画页头和「这个工具页由插件的前端组件绘制，Nana 原生界面不能运行它。」（`admin_gap.rs`）；预览贡献只认内置能画的视图，其余写「原生预览 · 名称 · 视图 id」（`native_preview.rs`）；只有播放器贡献、扩展名又不是音视频或图片的条目写「暂不支持播放 {扩展名} 文件」（`player_clip.rs`）。原生贡献的描述是 `src-nana/src/plugin_api.rs` 的 `NativePluginContribution`。

逐项的设备验收要求见 [设备矩阵](./nana-device-matrix.md)。

## 怎么验证

| 命令 | 覆盖 |
| --- | --- |
| `cargo test -p momobako-nana` | 全部单测（状态机、投影、`view_harness` 的增量同步和新挂对照）和 `src-nana/tests/` 下的四个集成测试 |
| `yarn test:nana:offscreen` | 离屏验收证据，等于 `cargo test -p momobako-nana --test offscreen_acceptance -- --nocapture` 再存日志，见 [离屏验收证据](./nana-offscreen-evidence.md) |
| `cargo test -p momobako-nana --test live_visual`、`--test live_surfaces` | 实况视图的布局、点击和输入 |
| `NANA_SCENES=… NANA_SIZES=… NANA_THEMES=… cargo test -p momobako-nana --test scene_shots -- --nocapture` | 按场景名出截图和节点清单，场景定义在 `src-nana/src/shell/acceptance*.rs` |
| `cargo check -p momobako-backend -p momobako-nana` | 共享领域边界的编译 |
| `yarn verify` | 前端测试、前端构建和 `src-tauri` 编译检查，不含 `momobako-nana` |

和 Vue 对照时用本机的 `tmp/vue-mock`（`tmp/` 被 git 忽略，不在仓库里）：模拟 Tauri IPC，在无头 Edge 里渲染 `src/` 的真实页面，和 Nana 用同名场景、同一份夹具。做法见 [Nana 与 Vue 对照](./nana-vue-parity.md#对照方法)。

颜色和字号以 `src-nana/src/theme_map.rs` 为准，组件目录以 `src-nana/src/capability.rs` 为准。
