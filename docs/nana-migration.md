# NanaUI 原生迁移边界

迁移已经完成：桌面端是 NanaUI 原生界面（`src-nana/`），原来的 Vue/Tauri 界面（`src/`、`src-tauri/`）和插件前端都已删掉，Vue/Tauri 源码最后一次出现在提交 `7bc847c`。本文写原生端现在的入口、渲染和验收路径、服务边界、已经接通的能力、还没做或要靠设备验证的部分，以及怎么验证。逐条分支记在 [逻辑矩阵](./nana-logic-matrix.md)，壳层视图的结构在 [壳层绑定式视图](./nana-shell-view.md)，组件目录在 [组件对应](./nana-component-map.md)，和 Vue 的取舍在 [Nana 与 Vue 对照](./nana-vue-parity.md)。

## 入口和运行

- 原生入口是 crate `momobako-nana`：`src-nana/src/main.rs` 调 `momobako_nana::run()`（`src-nana/src/lib.rs`）。
- NanaUI 固定在提交 `5bd2104f14b1d03f24fd92c5e737ba8c287dfcf4`。`src-nana/Cargo.toml` 的 `nana-ui`、`nana-ui-core`、`nana-ui-platform` 和开发依赖 `nana-ui-devtools` 都按这个 `rev` 从 Git 取。仓库里没有 NanaUI 副本，根 `Cargo.toml` 也没有 `[patch]`。

在仓库根目录运行：

```text
yarn plugins:build && yarn plugins:package   # 第一次运行前：构建并打包随附插件
cargo run -p momobako-nana                   # 原生窗口
```

原生窗口读写的本地文件。位置由 `src-nana/src/app_paths.rs` 按 NanaUI 的应用路径定：下表是开发时（从 `target/` 运行）的位置，和以前一样；安装版和便携版落在应用数据、配置和缓存目录，便携版全在 `<根>/data`，见 [Nana 发布包](./nana-packaging.md#数据写在哪)。

| 内容 | 开发时的位置 | 代码 |
| --- | --- | --- |
| 领域服务数据 | 当前目录下的 `.service-data` | `src-nana/src/app_paths.rs`、`src-backend/src/services/runtime/mod.rs` |
| 应用设置 | `%LOCALAPPDATA%\MomoBako\settings.json`，没有这个变量时依次退到 `%APPDATA%`、当前目录 | `src-nana/src/settings.rs` |
| 侧栏、展示方式、圆角 | 设置文件旁的 `sidebar.json`、`file-display.json`、`corners.json` | `workspace.rs`、`files_display.rs`、`admin_support.rs` |
| 上次打开的资源库 | 设置文件旁的 `last-repository.json`（键同 Vue 本地存储 `momobako.lastActiveRepositoryId`） | `workspace_remember.rs` |
| 播放设置、播放会话、播放器偏好 | 设置文件旁的 `playback-settings.json`、`playback-sessions.json`、`playlist-player-preferences.json` | `player_support.rs` |
| 主窗口位置、尺寸和最大化 | 应用数据目录下的 `com.momobako.desktop/nana-main-window-state.json` | `src-nana/src/window_state.rs` |
| 随附插件 | 读插件打包输出 `External/Plugins/.packages`，同步进服务目录的 `plugins/builtin` | `src-nana/src/plugin_host.rs` |

## 渲染和验收路径

- 产品窗口：`run()` 用 `NanaApplication::builder(..).run::<RuntimeApplication<MomoBakoApplication>>(..)`。`MomoBakoApplication` 实现 `ApplicationState`：`initialize` 启动领域服务，读应用设置和本地偏好；`build` 在窗口文档里挂常驻壳层（`ShellView::mount`，挂之前照 Vue 的 `lang="zh-CN"` 声明文档语言），提交读资源库列表的任务，建托盘；`update` 先做服务派发，再归约、执行副作用、同步视图；`prepare` 每帧推进计时器、动效和播放时钟，派发预取和目录请求，上传预览和缩略图纹理。步长是两帧之间真实流逝的时间（`frame_clock.rs`）：上一帧没要连续帧时从 0 起算，界面计时器和动效一帧最多算 250 ms，播放时钟不封顶；动效、计时器或播放还在走时按 30 帧要帧。界面进 `RuntimeDocument`，NanaUI 把它提交成 `UiScene`，由 `SceneWgpuPainter` 画到宿主 Surface。
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

- 领域代码在 `src-backend`（crate `momobako-backend`，含 `models`、`services`、`viewmodels`）。Nana 宿主和 Eagle 来源插件（`External/Plugins/source-eagle-library`）都依赖它，领域代码不依赖界面框架。
- `NativeServices::start`（`src-nana/src/services.rs`）在 `app_paths.rs` 定下的服务目录启动一份 `RepositoryRuntime`（`start_at`），日志器起来以后由 `plugin_host.rs` 同步随附插件、设置外部程序目录、加载插件并装上插件宿主，再把资源库查询、文件浏览、交互、管理、插件、系统和 Mutsuki 任务 ViewModel 都绑在它上面。壳层只经这些 ViewModel 调服务。启动失败时窗口进错误页（`initialize`），`NativeServices` 销毁时关掉 Runtime 的辅助进程。
- 归约留下的副作用由 `src-nana/src/*_dispatch.rs` 交给领域服务，结果作为 `ShellMessage` 回到归约。NanaUI 的任务工作线程在 `catch_unwind` 里跑任务：任务 panic 时线程不退出，但这个任务的结果消息丢掉、只记一条 `host.task_panicked` 故障，等着结果的状态收不到交回。所以所有派发点都经 `src-nana/src/task_guard.rs` 的 `spawn` / `spawn_service` 排任务，每个派发点只交代做什么和失败时交回什么：任务 panic 时在任务里接住、记进标准错误和系统日志（`taskPanicked`），任务排不进去（队列满、宿主停了）或服务没起来时当场经事件循环送回，三种都按失败交回，等着结果的状态（首屏启动、读取中、处理中、对话框的处理中）照常收尾（设置包和播放器类型失败照样进工作区，首屏启动前四步失败停在启动页给「重试」）。失败消息经事件循环到达，排在正在归约的这条消息之后。测试 `task_guard::tests::every_task_goes_through_the_guard` 扫描源码，直接调 `run_task` 的只能是 `task_guard.rs` 自己。缩略图解码在任务里逐张接住 panic，一张解不开不连累同一批的别的。
- 宿主事件边界在 `src-backend/src/services/host_events.rs`：`HostEvent`（日志记录、资源库结构更新）和 `HostEventSink`。Nana 用 `host_event_channel` 接收，`NativeServices::pump_host_events` 在独立线程里把事件转成 `ShellMessage::Host`。插件经 SDK 写的日志随这次插件调用的结果交回，后端收下结果时写进系统日志（`src-backend/src/services/plugin_events.rs`），和别的日志一样经日志记录事件进日志页。Mutsuki 任务运行时只存快照、不发变化通知，`NativeServices::watch_tasks` 另起一个线程每 250 ms 读一次（`src-nana/src/task_watch.rs`），排队、运行和取消中的任务有变化才发 `ShellMessage::TaskProgressLoaded`，服务销毁时线程退出。日志合并、启动日志和结构更新后的静默重读见逻辑矩阵的「宿主事件」。
- 宿主请求在 `src-nana/src/host_api.rs`。外部打开和目录揭示（`OpenExternal`）、拖出和最小化到托盘（`HostInputRequest`）由 `host_bridge::perform` 执行，每次执行都清空队列；目录揭示交给 NanaUI 的 `reveal_in_file_manager`（文件在所在文件夹里选中，文件夹直接打开，路径不存在时报「路径不存在」），不再自己拼 `explorer /select,`；关闭确认是浮层里的确认框，只看 `InputState::pending_close`，不经宿主请求；窗口最小化、最大化和关闭经 `host_api::WindowCommand` 换成 Nana 平台命令。系统文件对话框直接排成 Nana 平台的 `WindowCommand::OpenFileDialog`，编号 1–8 在 `src-nana/src/shell/input_support.rs`。

## 已经接通的能力

下文的「Windows 非测试构建」指 `cfg(all(windows, not(test)))`：只有单元测试构建（`cfg(test)`）不开声卡、不注册系统媒体会话。代码路径都在 `src-nana/src/shell/` 下，另有说明的除外。

- **启动**：四步依次读资源库列表、同步仓库（`PROTOCOL_REPOSITORY_SYNC`）、读仓库摘要、读首屏目录；缺失仓库和空列表不进同步。照 Vue 先打开上次打开的资源库（`last-repository.json`），它不在列表里时打开第一项；读完摘要（缺失仓库是选中时）记住这次的仓库，列表空了清掉。失败停在当前步，保留已完成的步骤，给「重试」；读完首屏目录（空库、缺失仓库是选定以后）照 Vue `await loadSettingsData()` 停在第 4 步「加载应用配置、插件设置和首屏辅助数据。」，等设置包和插件列表到了以后读的播放器类型都回来（失败也算）才进工作区，进来时来源动作、库类型快捷方式和新建播放集都已可用；等的这段外面来的结构更新照常静默刷新。启动页显示最近 8 条加载日志，条目和明细照 Vue `emitStartupLog`。侧栏在同步以后（读到摘要时）才绑定仓库、读目录树。见 `workspace_launch.rs`（启动流程）、`workspace_startup.rs`（步骤条和加载日志）、`route_startup.rs`；工作台状态和主区在 `workspace.rs`，缺失仓库的动作和删除资源库对话框在 `workspace_missing.rs`。
- **换仓库和刷新**：照 Vue `selectRepository` / `loadRepositories`（`workspace_switch.rs`、`workspace_flow.rs`）。
  - 换到别的仓库（切换列表、附加或新建资源库成功、删除当前仓库后选下一个、搜索命中在别的仓库、刷新后当前仓库已不在列表里）：回到启动页、收起侧栏，从「切换资源库」走「扫描资源库文件 → 读取仓库摘要 → 读取首屏目录」；不写首屏启动日志、不重读设置包；目标缺失时直接进缺失页。失败停在启动页给「重试」，重试是完整的首屏启动。上一个仓库的条目、选择、预览、搜索和侧栏内容在活动仓库变了的那次归约里清掉。
  - 同一个仓库（来源动作完成后的刷新、缺失页「刷新」、切换列表选当前仓库、重定向成功）：先同步，再读摘要、重读当前目录（读失败退回根目录），侧栏在后台重读；整轮状态区写「正在同步仓库状态」，失败写进状态区，不回启动页。从刷新进来时先重读资源库列表。
- **侧栏**：仓库切换弹层（切换、添加资源库、删除当前资源库），仓库头是菜单按钮，焦点在上面时 ArrowDown / ArrowUp 打开切换列表、焦点落在首项或末项；快捷方式（全部、未分类、未标签、最近使用、回收站）带计数，以及快捷访问；文件夹树和智能文件夹树是按显示顺序展开的扁平行，文件夹悬停 450 ms 打开；文件夹行的右键菜单（打开、新建子文件夹、重命名、删除；菜单键和 Shift+F10 也能打开，焦点落在首项，和文件右键菜单同一份实现 `context_menu.rs`）；文件夹新建、重命名、删除，智能文件夹新建、编辑、删除，播放集新建、打开、播放、删除（新建对话框的类型和「播放」能不能点看读回的播放器类型）；「刷新文件夹树」同步整个仓库再重读（`tree_sync.rs`、`src-nana/src/sync_dispatch.rs`）；顶部的全局状态区显示没有就近显示的失败、读取中和同步进度（`status.rs`）；折叠和宽度（220–480，默认 276）写进 `sidebar.json`；启动和换仓库绑定侧栏时照 Vue 在后台读仓库动作和硬链接候选，有仓库动作时多一个「动作」入口，有没确认的硬链接候选时弹出确认。见 `sidebar*.rs`、`view_part_sidebar.rs`。
- **文件**：文件页用 `each_virtual` 按视口建行，缩略图只解码挂着的、纹理只登记一次，滚出视口的超过 240 张按最近最少使用回收（`thumbnail_host.rs`、`thumbnail_cache.rs`），列表、网格、自适应、瀑布流四种展示写进 `file-display.json`。单击选中，Shift 单击从锚点选一段，Ctrl / Meta 单击切换（Vue `selectionModeFromEvent`；修饰键由宿主在输入路由完以后记下，`window_host::note_modifiers`），右侧详情跟着主选中项看元数据；双击或右键「预览」进预览页。工具栏有「建文件」和导入（文件夹、ZIP、Eagle 复制或剪切），回收站里有「还原所有项目」「清空回收站」；工具栏「导入」是菜单按钮，ArrowDown / ArrowUp 打开导入菜单、焦点落在首项或末项；右键菜单（卡片上的菜单键和 Shift+F10 也能打开，焦点落在首项）有预览、打开、定位、还原、复制到、加入播放列表、缩略图（刷新、自定义）、重命名、删除和来源插件的条目动作（`entry_actions.rs`：清理缓存、重新获取播放资源和下载带本地缓存目录 `managedCacheRoot`；下载分到本地和到其他资源库，到本地时目录里已有的同名文件插件保留、不覆盖并报出条数，到其他资源库先选目标资源库、下载完导入并同步目标（目标里已有的同名文件保留、跳过并报出条数；插件写在宿主给这次下载新建的暂存目录里，导入完清掉，导入失败时留着并在日志里写明位置），`source_target.rs`、`src-nana/src/download_dispatch.rs`；插件报告部分条目失败时状态区写失败的条数、每一条的原因写进系统日志，`source_download.rs`；来源认证或本地缓存不可用时禁用并写原因）；条目可以拖进文件夹移动，可以框选（按下时按着 Ctrl / Meta 并进原来的选择）；有硬链接候选时弹出确认。仓库导出对话框能提交压缩包和 Git 导出，和 Vue 一样首页没有入口。见 `route_files.rs`、`files_*.rs`。
- **预览**：预览页按扩展名分派（`inspect_support.rs`）。图片在产品窗口里经宿主纹理槽 `file-preview` 用 `GpuTextureView` 显示；文本和 Markdown 都按等宽原文显示前 768 KiB；WAV 用自有解析，MP3、FLAC、Ogg 用 symphonia，Windows 上 M4A、AAC、Opus 和其余视频容器用媒体基础（`video_mf.rs`），未压缩和 MJPEG 的 AVI 用纯 Rust（`video_avi.rs`），视频画面进同一个纹理槽；压缩包（zip、cbz、7z、rar、cbr）最多列 200 条；PDF 按内容流画页、可翻页；Office 的 Open XML 和 OLE 文件抽文本；OBJ、glTF、GLB、STL、3MF、VRM 能取出三角形时软件光栅、可旋转缩放，FBX、BLEND 只报文件头（`native_preview*.rs`）。PDF 页、模型光栅和图片幻灯片的帧编码成 PNG 画进预览框（`inspect_preview_page.rs`、`inspect_preview_body.rs`），不上传宿主纹理；`file-preview` 只在预览框这一帧画它时上传（预览页开着、画的是图片或视频画面，`inspect_preview_body::wants_preview_texture`），单击只选中时预览页藏着，不传。读不出、解不开时预览框里写失败标题和原因。
- **元数据**：右侧详情和预览页共用一块元数据编辑：注释、链接、评分和标签可以改，自定义字段只读显示；草稿变脏 260 ms 后自动保存（`update_asset_metadata`，带 `expected_version`），换选前先存上一份，画面上没有保存按钮；版本冲突时保留本地草稿，给「采用服务器版本」。见 `files_detail.rs`、`inspect_metadata_*.rs`、`inspect.rs`。
- **搜索**：标题栏输入先切到搜索面板（`route_search.rs`），250 ms 后调 `search_assets`；首页筛选栏（格式、标签、颜色、形状、评分和高级条件）常驻在文件、搜索、播放集、日志、拓展和动作页；插件登记的库类型快捷方式（ASMR 等）进筛选栏（`inspect_shortcuts.rs`）；跑完没有命中时写明哪里没有匹配的文件；点结果回到文件页并选中。见 `inspect_search*.rs`、`search_*.rs`。
- **播放**：播放条和预览共用一份 `PlaybackSessionState`，预览里解得开的音视频接管播放条（`player_preview.rs`）；Windows 非测试构建经 winmm 出声（`wav_player.rs`），并注册系统媒体传输控件（`system_media.rs`）。侧栏绑定仓库时读播放集列表，新建、删除以后换上返回的整份列表，侧栏和播放器共用这一份（`apply_playlist_list`）：播放器据此读成员索引（文件右键「加入播放列表」），存下的会话所在的播放集在列表里、播放器类型也认得时读它的详情来恢复；新建以后照 Vue 接着点开它。点开侧栏的播放集进播放集页，条目用 `each(..).container(ReorderList)` 建，可以拖动排序、移除；当前项只读、只解这一条（`player_clip.rs`、`src-nana/src/player_dispatch.rs`）；图片幻灯片有停留时长和适应 / 填充。会话写进 `playback-sessions.json`。见 `player*.rs`、`route_playlists.rs`。
- **设置**：侧栏「设置」和缺失仓库的「打开来源设置」都进设置页（`ShellMessage::OpenSettings`、`admin::open_settings_page`），后者照 Vue `?plugin=` 展开来源插件的设置。设置页有音频播放、外观（主题、圆角，改了立即生效）、仓库服务、外部素材接入（复制连接信息、导出 `external-api.json`）、插件管理、缓存和 API 设计。主题写进 `settings.json`，圆角写进 `corners.json`。见 `route_settings.rs`、`admin_settings_view.rs`。
- **插件**：首屏启动第 4 步照 Vue `loadSettingsData` 读一次设置包（插件、钩子记录、缓存、API 设计和外部连接），读回来以前停在启动页；打开设置页、插件面板「刷新」时再读；插件列表换新以后重读播放器类型（`list_playlist_players`），新建播放集的类型、播放器贡献和插件类型的播放集能不能播都从这里来。插件管理面板（设置页和拓展页都有）分组和搜索，能启停、从压缩包安装（编号 2 的文件对话框）、确认后删除、编辑和重置清单声明的设置字段、打开插件目录；来源插件的扫码登录画二维码（`source_auth_page.rs`）。拓展页的 API Playground、文件导入、Eagle 导入是原生页面。见 `admin_plugin*.rs`、`admin_gap.rs`、`tool_native.rs`、`api_playground*.rs`。
- **日志**：日志页按级别、来源和关键字筛选，可以暂停追踪、清空；页面主体和日志列表都能滚，列表在筛选下面分走面板剩下的高度、最矮 72（有意小于 Vue 的 360，矮窗口里追踪时最新一条不用滚页面就看得到），窗口放得下时只有列表滚（`route_admin.rs`、`admin_logs_view.rs`）。每次切到日志面板照 Vue 读最近 200 条历史日志（读的时候手上没有日志就写「正在加载系统日志」），之后经宿主事件合并新记录。
- **任务**：侧栏底部的「任务」打开任务弹层（`admin_view.rs`），列出当前的仓库操作和宿主观察到的运行中任务（排队、运行和取消中，结束就拿掉）；「任务」右上角的计数是弹层的行数，和 Vue 的 `activeTaskCount` 一样。弹层里没有取消按钮，Vue 的 `TaskPopover.vue` 也没有；点弹层外面、头部的关闭按钮和 Escape 都关掉它。
  - 仓库操作只有一个槽位（`repo_operation.rs`，Vue `operationProgress`）：最后开始的操作占着它，旧操作的更新和完成不再生效；等待时每 220 ms 往上爬到 88，完成停在 100% 再过 180 ms 收起，失败立即收起。
  - 每一行进度条的宽度照 Vue `.progress-bar__fill` 按 0.18 s 过渡到新值，减少动态效果时直接跳到新值（`motion.rs` 的 `set_task_fills`）。
  - 出这一行的操作：加载资源库（换仓库、刷新）、挂载资源库、创建资源库、重定向资源库、刷新文件树、来源下载，复制、移动、导入、ZIP 和 Eagle 导入、多选删除和还原，以及非静默的读目录（「读取目录」「读取回收站」「读取文件树」：回收站和快捷访问、刷新时重读目录、仓库动作完成后的重读、搜索命中所在目录）。点文件夹、双击进入和面包屑在 Vue 里是静默读取，不出。见 `operation_row_tests.rs`、`workspace_switch_tests.rs`。
- **键盘和焦点**：菜单按钮（仓库头、「导入」）上的 ArrowDown / ArrowUp，文件卡片和文件夹行上的菜单键和 Shift+F10 打开菜单，焦点落在首项或末项（`menu_focus.rs`、`menu_focus_view.rs`）；菜单项首尾相接，Enter 执行，Escape 关掉。对话框、弹层和菜单关掉以后按同一条规则交还焦点（`focus_return.rs`）：回到打开它的控件；它随菜单、弹层收起了，回到菜单、弹层的主人（文件条目、文件夹行、仓库头、「导入」）；被删掉了，回到它那一片里选中的项或第一个能聚焦的控件；自己弹出来的硬链接确认回到文件列表里的条目。对话框在刷新时才关上，每帧画完以后（`presented`）补还。各对话框和弹层还到哪见 [壳层绑定式视图](./nana-shell-view.md) 的「焦点归还」，和 Vue 的差别见 [对照](./nana-vue-parity.md)。
- **宿主操作**：标题栏的最小化、最大化、关闭；关闭行为按设置确认、退出或最小化到托盘（`src-nana/src/window_host.rs`）；托盘在 Windows 上用 `tray-icon`（`src-nana/src/tray.rs`）；主窗口几何下次启动恢复；打开和定位启动系统程序；拖出文件在 Windows 上用 `drag` crate（`src-nana/src/drag_out.rs`）；剪贴板读写系统剪贴板（`src-nana/src/host_bridge.rs`）；文件区和空库页接收系统文件拖入（`input::accept_file_drops`）；Escape 由运行时交给激活的对话框，其余按 `escape_layer` 关最上面一层。

## 还没做或要靠设备验证的部分

### 接线缺口

归约和服务派发已经写好、但产品界面触发不到的路径，以及 Vue 有、Nana 还没接上的反馈：目前没有。

### 设备验证和其它

- **IME**：输入法组合期间延后重挂和换块（`ShellPart::composing`），由单测覆盖；真窗口里的中文输入法没有验收。
- **真窗口**：窗口几何恢复、关闭行为和托盘在真窗口里的效果没有验收；测试只断言排出的窗口命令和宿主请求，不开真窗口。
- **拖放**：拖入的放置决定由输入归约测试覆盖，拖出调用系统拖放；和资源管理器之间的真实拖放没有验收。
- **托盘**：只在 Windows 上建；点击和菜单没有在真窗口里验收。
- **UIA/AccessKit**：`hosted` 带 AccessKit 平台适配器；测试只读 NanaUI 自己的无障碍树，读屏器看到的结果没有验收。
- **发布包**：发布流程（`.github/workflows/release.yml`）改用 NanaUI 的 `nana-packager` 打 `momobako-nana`，不再调用 tauri-action：dist 配置编主程序，打安装版布局和便携版两个 zip，各自 `validate`，便携版再跑 `validate --run --tamper-suite`，发到草稿 Release。本地用 `yarn nana:build`、`yarn nana:package`，见 [Nana 发布包](./nana-packaging.md)。没有安装程序（NanaUI 还没有安装包后端）和代码签名；没在干净的 Windows 上验收：没装 VC++ 运行库时能不能启动、解压到只读目录（例如 `Program Files`）以后数据是不是都落在用户目录。CI（`ci.yml`）跑 `yarn verify`（含 `momobako-nana` 的编译检查）和 `yarn test:backend`，不打包、不跑 Nana 的测试。
- **全局快捷键**：没有实现。删掉的 Vue 和 Tauri 端（提交 `7bc847c` 的 `src/`、`src-tauri/src`）也没有注册全局快捷键，宿主请求里不留占位变体。
- **插件不带界面**：插件前端（设置页、工具页、预览、播放器的 Vue 组件）已经删掉，插件只剩后端形态，见 [API 设计](./api-design.md) 的 Plugin API。插件设置只画清单 `contributes.settings.fields` 声明的字段；工具页来自清单的 `contributes.toolPages`，三个内置页面画原生页面，宿主没有的工具页只画页头和「MomoBako 没有这个工具页：插件只能打开内置的工具页，不能自带页面。」（`admin_gap.rs`）；预览由内置视图按扩展名绘制（`native_preview::builtin_view`），认不出的扩展名照 Vue 没有预览插件时一样只画缩略图或类型图标；只有播放器贡献、扩展名又不是音视频或图片的条目写「暂不支持播放 {扩展名} 文件」（`player_clip.rs`）。

逐项的设备验收要求见 [设备矩阵](./nana-device-matrix.md)。

## 怎么验证

| 命令 | 覆盖 |
| --- | --- |
| `cargo test -p momobako-nana` | 全部单测（状态机、投影、`view_harness` 的增量同步和新挂对照）和 `src-nana/tests/` 下的四个集成测试 |
| `yarn test:nana:offscreen` | 离屏验收证据，等于 `cargo test -p momobako-nana --test offscreen_acceptance -- --nocapture` 再存日志，见 [离屏验收证据](./nana-offscreen-evidence.md) |
| `cargo test -p momobako-nana --test live_visual`、`--test live_surfaces` | 实况视图的布局、点击和输入 |
| `NANA_SCENES=… NANA_SIZES=… NANA_THEMES=… cargo test -p momobako-nana --test scene_shots -- --nocapture` | 按场景名出截图和节点清单，场景定义在 `src-nana/src/shell/acceptance*.rs` |
| `cargo check -p momobako-backend -p momobako-nana --all-targets --locked` | 共享领域边界和原生界面的编译，`yarn verify` 的最后一步 |
| `yarn test:backend` | `momobako-backend` 单测，等于 `cargo test -p momobako-backend --locked`，不在 `yarn verify` 里 |
| `cargo test -p momobako_source_netease_cloud_music`、`-p momobako_mutsuki_plugin_sdk` | 网易云来源插件（下载落盘不覆盖、暂存目录、错误原因）和插件 SDK（调用结果带回插件日志）；插件都是根 workspace 成员，用根目录的 `Cargo.lock` |
| `yarn verify` | Node 脚本类型检查、vitest（工具链、打包、文档和插件清单）和上面的 `cargo check`，不跑 `momobako-nana` 的测试 |
| `yarn nana:build`、`yarn nana:package` | dist 版主程序，打安装版布局和便携版、`nana-packager validate`、便携版自检，见 [Nana 发布包](./nana-packaging.md#本地打包) |

Vue 界面已经删掉。还要和 Vue 对照时，在提交 `7bc847c` 的检出里用本机的 `tmp/vue-mock`（`tmp/` 被 git 忽略，不在仓库里）：模拟 Tauri IPC，在无头 Edge 里渲染那份检出里 `src/` 的真实页面，和 Nana 用同名场景、同一份夹具。做法见 [Nana 与 Vue 对照](./nana-vue-parity.md#对照方法)。

颜色和字号以 `src-nana/src/theme_map.rs` 为准，组件目录以 `src-nana/src/capability.rs` 为准。
