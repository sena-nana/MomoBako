# Nana 逻辑迁移矩阵

静态截图不能当作迁移完成。每一行从 Vue 或 composable 的分支读出，不从现有离屏场景反推。尚未读过的来源保持“未读”，不预填分支。

记账列是：来源、触发、服务、结果、可见性、状态。状态依次为未读、已记账、已测试、已离屏。服务行要有真实调用测试；不改变控件树的分支也要有 ViewModel 测试；改变控件树的分支才增加离屏场景。「已离屏」指有验收场景直接走过这一行的分支、结果画在场景里，括号里写场景名；场景定义在 `src-nana/src/shell/acceptance*.rs`，命令和断言见 [离屏验收证据](./nana-offscreen-evidence.md)。可见性列写画面上看得到的文字。

工作台面板来自 `src/composables/workspace/state.ts`：

| 枚举 | 取值 |
| --- | --- |
| `WorkspacePanelKey` | files、trash、search、smartFolder、playlist、actions、extensions、logs |
| `WorkspaceLibraryCategoryKey` | all、uncategorized、untagged、recent |
| 文件显示 | adaptive、masonry、grid、list |
| 筛选 | `WorkspaceFilterState` 的标签、格式、颜色、形状、排除条件、元数据、数值、日期、AND/OR、排序、条数、最低评分 |

这些面板和启动、加载错误、缺失仓库、空库（`MainRegion`）以及浮层正交。`ShellPage` 还在用：设置页、选中文件和预览页靠它区分，离屏验收的 15 页也按它起名；主区走哪条路由由区域和面板决定，播放集页、插件设置页、任务页这些页面身份不再触发导航或读取。

## Phase 1 壳层

这些行由 `src-nana/src/shell/workspace_tests.rs` 覆盖状态机和副作用枚举，「选择路径」一行在 `input_tests.rs`。窗口首次 `build` 时按 `prepare_initial_list` 给的代次直接提交 `list_repositories`；之后归约留下的副作用交给领域服务：非缺失仓库接着 `PROTOCOL_REPOSITORY_SYNC`、`get_repository_snapshot` 和根目录 `get_file_browser`，刷新和重试走 `RefreshRepositories`，重定向用 `PROTOCOL_REPOSITORY_RELOCATE`，删除用 `RepositoryManagementViewModel::delete_repository`，来源设置复用系统状态和应用设置加载。协议调用没有替身测试。验收场景和产品窗口走同一个 `ShellView::mount`；标了「已离屏」的行写明场景，其余行只有 ViewModel 测试。

分隔条拖动由 `note_sidebar_resize` 在指针捕获结束时写回宽度。键盘或双击造成的宽度变化在没有捕获时立即写入。侧栏折叠按 240ms 收到 0。这两项由 `shell::motion::tests` 覆盖。

| 来源 | 触发 | 服务 | 结果 | 可见性 | 状态 |
| --- | --- | --- | --- | --- | --- |
| `lifecycle.ts` `ensureRepositoryWorkspace` | 首次进入且启动空闲 | 窗口首次 `build` 直接提交 `list_repositories`，不经副作用枚举 | 从第 1 步重新开始，进度按当前步 / 4 | 启动面板；侧栏在就绪前隐藏 | 已测试（未离屏） |
| `lifecycle.ts` `setWorkspaceStartupProgress` / `failWorkspaceStartup` | 中间步骤失败 | 无新请求 | 当前步记失败，更早的步骤保持完成，百分比保留 | 启动面板显示失败并给出重试 | 已离屏（`error`） |
| `lifecycle.ts` 重试 | 启动已失败 | 新代次的 `RefreshRepositories` | 已就绪时直接返回；否则整段回到第 1 步，旧步骤不再保持完成 | 进行中的步骤条替换失败态 | 已测试（未离屏） |
| `lifecycle.ts` `loadInitialRepository` | 列表为空 | 不同步 | 清空活动仓库和记住的 id，结束启动 | “还没有可用资源库” / “拖入一个本地文件夹创建资源库。” | 已离屏（`empty-repository`） |
| `lifecycle.ts` `loadInitialRepository` | 选中仓库 `status == missing` | 不同步，立即记住 id | 结束启动，主区为缺失 | 缺失面板；不进入文件列表 | 已离屏（`missing`） |
| `lifecycle.ts` `loadInitialRepository` | 选中仓库可同步 | `SyncRepository` | 选择顺序是当前 id、记住的 id、第一项；同步成功后才记住 | 第 2 步“扫描资源库文件” | 已离屏（`loading`，走记住的 id） |
| `lifecycle.ts` 同步结果 | 代次不匹配，或启动已不是加载中 | 丢弃 | 不改已完成步骤和活动仓库 | 保持当前步骤 | 已测试（未离屏） |
| `lifecycle.ts` 同步成功 | 代次匹配 | `LoadSnapshot` → `get_repository_snapshot` | 进入第 3 步；没有活动仓库则失败 | 第 3 步“读取仓库摘要” | 已测试（未离屏） |
| `lifecycle.ts` 摘要与首屏 | 活动仓库匹配且步骤已到 3 / 4 | 摘要成功后根目录浏览 | 成功则结束启动并进入有仓库；失败则加载错误 | 第 4 步“读取首屏目录”，或错误面板 | 已测试（未离屏） |
| `lifecycle.ts` 列表失败 | 启动尚未就绪 | 无 | 启动失败，主区为加载错误；已就绪时记下缺失错误，主区不是缺失页时同时写进全局状态区（来源 `Repository`） | 错误面板，已完成步骤保留 | 已测试（未离屏） |
| `useWorkspaceHomeViewModel.ts` | 启动结束 | 无 | 缺失、有仓库、空仓库、加载错误四者互斥；错误优先于另外三条 | 同一时刻只显示一个主区 | 已测试（未离屏） |
| `AppShell.vue` `watch(activeRepoId)` | 上一个 id 存在且变为另一个 id | `StopPlayback` | 相同 id 不停止播放，也不清缺失错误 | 切换后的仓库主区 | 已测试（未离屏） |
| `AppShell.vue` 侧栏 | 折叠，或宽度在松手时提交 | `PersistSidebar` → `sidebar.json` | 宽度夹在 220–480，默认 276；只有存储值 `"1"` 才折叠；拖动中只夹取 | 折叠隐藏第一栏；键名沿用 `momobako.sidebarCollapsed` / `momobako.sidebarWidth` | 已测试（未离屏） |
| `useMissingRepositoryActions.ts` 刷新 | 缺失仓库且不忙 | `RefreshRepositories` | 忙（重定向或删除中）时忽略 | “刷新”不可再次提交 | 已测试（未离屏） |
| `useMissingRepositoryActions.ts` 选择路径 | 有仓库且不忙 | `OpenFileDialog`，`PickFolder`，编号 3 | 排队文件夹对话框并清错误；没有仓库或忙时不排队；取消、空白或对话框失败不重定向 | “重定向”；失败时“文件夹选择失败：…” | 已测试（未离屏） |
| `useMissingRepositoryActions.ts` 提交路径 | 对话框返回去空白后非空的路径 | `RelocateRepository`，没有替身 | 成功后刷新列表；失败写入缺失错误并解除忙状态 | 忙时“重定向中...” | 已测试（未离屏） |
| `MissingRepositoryState.vue` 来源缓存 | `localCache.required` 且缓存不是 ready | 照 Vue `/settings?plugin=` 进设置页：读设置包和应用设置，展开来源插件的设置（读它的配置） | 不打开路径框；忙着或不是缓存问题时不打开 | 主按钮为“打开来源设置” | 已测试（未离屏） |
| `RepositoryDeleteDialog.vue` | 打开、取消、确认 | `DeleteRepository` | 缺失的本地仓库只能“只删除记录”；删除中不能关闭或再次确认；没有删除标记的成功结果不关对话框；成功后移除记录并选剩余第一项或空仓库 | “删除资源库”标题和三个删除方式；删除中对话框左下角写“处理中...”（缺失页的删除按钮写“删除中...”） | 已测试（未离屏） |
| `state.ts` 面板与分类 | 启动进度推进 | 无 | 面板保留；空列表把分类重置为全部 | 面板枚举不随步骤丢失 | 已测试（未离屏） |

## Phase 2 侧栏

这些行由 `src-nana/src/shell/sidebar_tests.rs`、`tests.rs` 的 `stale_playlist_detail_keeps_the_bound_repository_screen` 和刷新文件夹树的 `tree_sync_tests.rs` 覆盖状态机。宿主在归约后把副作用交给领域服务：目录树用 `FileBrowserViewModel::get_repository_tree`，智能文件夹用 `list_smart_folders` 和 `query_smart_folder`，播放集用 `list_playlists` 和 `get_playlist_detail`（播放器类型跟插件列表走，见阶段 6），目录浏览用 `get_file_browser`，附加本地文件夹用 `PROTOCOL_REPOSITORY_ATTACH`，Eagle Library 和表单类来源用 `PROTOCOL_REPOSITORY_CREATE`。协议调用没有替身测试。侧栏在启动就绪后挂一次：`SidebarFrame` 里是 `ListItem` 行，文件夹树和智能文件夹树是按显示顺序展开的扁平行，放在 Store 里用带键的 `each` 建，不用 `TreeView`；验收场景走同一个 `ShellView::mount`。标了「已离屏」的行写明场景。同一仓库且缺失标记不变时，再次绑定不重复请求目录树、智能文件夹和播放集。

快捷方式计数在摘要到达时立即计算，没有移植 Vue 的 200ms idle 调度。文件快捷访问会先把 `\` 收成 `/`，再打开父目录并选中该路径。

文件夹悬停 450ms、文件夹新建/重命名/删除、智能文件夹编辑和删除、播放集播放、Escape 和缺失仓库锁定由 `shell::sidebar::gap::tests`（`sidebar_gap_tests.rs`）覆盖；添加资源库的后端菜单和云盘/Eagle 表单由 `sidebar_popover.rs` 的测试覆盖。实况路径另由 `pointer_gesture.rs` 的 `live_folder_hover_opens_after_the_idle_clock_reaches_450ms`、`live_popover_opens_under_the_anchor_inside_the_viewport` 和 `live_folder_button_escape_and_prefetch_use_the_shell_path` 覆盖：悬停满 450ms 走 `apply_open_folder`，`prepare` 帧把 `Browse` 提交成目录请求，文件列表路径改成该目录、文件列显示「正在读取目录」，按着指针时不拆树，松手后选中该文件夹；离开行清掉计时，回来要重新等 450ms。仓库弹层左边对齐仓库头，在它下面 6px，离窗口左右至少 8px（`sidebar_popover_view.rs`）。焦点在「文件夹名称」时 Escape 关掉最上层。Escape 走全局输入处理，运行时没处理掉的按下按 `escape_layer` 关掉最上层，焦点不在按钮或输入框上也关；对话框、弹层、右键菜单和筛选栏上的行为由 `shell::escape_tests` 覆盖。系统文件夹对话框的操作系统结果仍要设备验证。刷新进行中，状态区的转圈读动效时钟（`hot.spinner`），刷新按钮换成加载图标并禁用。

| 来源 | 触发 | 服务 | 结果 | 可见性 | 状态 |
| --- | --- | --- | --- | --- | --- |
| `useSidebarShortcutsUi.ts` 计数 | 仓库摘要 | 无新请求 | 已删除素材不进入全部、未分类、未标签、最近使用；路径不含 `/` 为未分类；标签为空为未标签；有 `lastAccessedAt` 为最近使用；回收站用 `overview.trashCount` | “全部”等五行，计数在右侧 | 已测试（未离屏） |
| `useSidebarShortcutsUi.ts` `selectShortcut` | 资源库缺失 | 无 | 快捷方式、快捷访问、打开文件夹和打开智能文件夹直接返回 | 快捷方式、快捷访问和动作行变暗禁用；文件夹和智能文件夹分组不画树，换成修复提示 | 已测试（未离屏） |
| `useSidebarShortcutsUi.ts` `selectShortcut` | 回收站，且未锁定 | `get_file_browser`，`special_location = trash` | 面板改为 trash；离开设置页 | “回收站” | 已测试（未离屏） |
| `useSidebarShortcutsUi.ts` `selectShortcut` | 其余分类，且未锁定 | 若当前在回收站，则根目录浏览且不带 trash | 面板改为 files 并写入对应分类；离开设置页 | 分类名 | 已测试（未离屏） |
| `useSidebarShortcutsUi.ts` `openQuickAccess` | `targetKind == smartFolder` 且有 id | `query_smart_folder` | 进入智能文件夹；没有 id 或找不到条目则返回 | 智能文件夹面板 | 已测试（未离屏） |
| `useSidebarShortcutsUi.ts` `openQuickAccess` | `targetKind == file` 且有路径 | 浏览父目录 | 分类为全部；选中规范化后的文件路径；空路径返回 | 父目录 | 已测试（未离屏） |
| `useSidebarShortcutsUi.ts` `openQuickAccess` | 其它非空 `targetPath` | 浏览该目录 | 分类为全部 | 该目录 | 已测试（未离屏） |
| `useFolderSidebarUi.ts` 打开与展开 | 点击目录或当前目录变化 | `get_file_browser` | 面板 files、分类全部；按 `/` 逐段展开，空路径不展开；回收站浏览不展开目录 | 目录树选中当前路径 | 已测试（未离屏） |
| `useFolderSidebarUi.ts` 树替换 | 新树到达 | 无 | 仓库不匹配则忽略；否则丢掉树里已经不存在的展开路径，空路径始终有效 | 保留仍存在的展开节点 | 已测试（未离屏） |
| `sync.ts` `refreshFileBrowserTree` | 启动完成、有仓库、未锁定，且不在读目录、目录树或刷新中 | `PROTOCOL_REPOSITORY_SYNC`，再读 `get_repository_snapshot`、`list_hardlink_candidates`、`get_repository_tree`（回收站不读树），最后非静默读当前目录 | 开始时清掉上一次失败；进度扫描 1/3 → 刷新 3/3 → 完成（写入 2/3 和刷新 3/3 在同一次归约里连写，和 Vue 一样看不到 2/3）；任一段失败写进状态区（来源 `Sync`）；换了仓库或过期的结果作废；进行中再点被拦下 | 状态区「扫描文件夹结构」和「33%」等，刷新按钮换成加载图标并禁用 | 已离屏（`folder-tree-refresh`） |
| `WorkspaceSidebar*.vue` 文件夹空态 | 无仓库、缺失、回收站或空树 | 无 | 不画树行（重命名、删除和拖放都在树行上）；标题上的「在当前目录新建文件夹」还在，无仓库、缺失和回收站时禁用，空树时可用 | “先选择或添加一个资源库。” / “资源库文件夹丢失，请先在主视图修复。” / “回收站条目在主视图中管理。” / “当前仓库还没有子文件夹。” | 已测试（未离屏） |
| `useSmartFolderSidebarUi.ts` 选择 | 有仓库、id 非空、且未锁定 | `query_smart_folder` | 面板 smartFolder，展开祖先但不展开自己；空 id 返回 | “正在查询智能文件夹 …”只写进 `ShellViewModel::detail`，界面不显示 | 已测试（未离屏） |
| `useSmartFolderSidebarUi.ts` 查询结果 | 仓库或当前 id 已变化 | 丢弃 | 返回 false，不改详情；匹配的错误只写入智能文件夹错误，不把整页改成 Error | 错误留在侧栏 | 已测试（未离屏） |
| `useSmartFolderSidebarUi.ts` 树替换 | 列表到达 | `list_smart_folders` | 仓库不匹配则忽略；先剪掉无效展开，再展开当前项的祖先 | “还没有智能文件夹。”或树 | 已测试（未离屏） |
| `usePlaylistSidebarUi.ts` 选择 | 有仓库 id 且 id 非空 | `get_playlist_detail` | 面板 playlist；播放集从列表消失时清空当前项，若面板仍是 playlist 则回到 files | 分组默认收起，只留标题“播放集”和个数；展开后每行是名称和「播放器 · N 项」，右侧是播放和删除 | 已测试（未离屏） |
| `usePlaylistSidebarUi.ts` 可播放 | 播放器类型读回（插件列表换新以后读，见阶段 6） | `list_playlist_players` | 行上有「播放播放集」，播放器类型在内置候选或已读到的播放器贡献里才可点（`player_view::playlist_plugin_missing`）；读回之前「新建播放集」不可用，读回后对话框默认选第一个类型 | 展开后每行右侧的播放按钮，不可播放时禁用 | 已测试（`data_load_tests.rs`，未离屏） |
| `PlaylistDetailLoaded` | 侧栏已绑定的仓库与详情仓库不同 | 丢弃 | 未绑定仓库时仍应用详情，避免旧页面测试失效 | 保持当前页 | 已测试（未离屏） |
| `useRepositorySwitcherUi.ts` 打开与选择 | 点击仓库名 | `select_repository` 后重新绑定侧栏 | 已经是切换列表或正在提交时忽略；选择后关闭弹层并离开设置页 | “资源库 · 名称” | 已测试（未离屏） |
| `useRepositorySwitcherUi.ts` 删除 | 有活动仓库且未提交 | 打开已有删除对话框 | 没有活动仓库或正在提交时忽略 | “删除当前资源库” | 已离屏（`repo-delete-dialog`） |
| `useRepositorySwitcherUi.ts` 附加 | 路径去空白后非空 | 本地文件夹走 `PROTOCOL_REPOSITORY_ATTACH`；Eagle Library 和表单类来源走 `PROTOCOL_REPOSITORY_CREATE` | 空白不提交；提交中不能关闭、改选或再打开添加菜单；成功后关闭并刷新列表；失败回到添加菜单并记下错误 | 切换列表的“添加资源库”进添加菜单；本地文件夹和 Eagle 弹编号 4 的系统文件夹对话框“添加资源库”；提交中菜单项禁用，失败在菜单里显示错误 | 已测试（未离屏） |
| `useWorkspaceSidebarShellUi.ts` 缺失 | 活动仓库 `status == missing` | 不请求树、智能文件夹或播放集 | 快捷方式等导航锁定；仓库切换弹层本身不锁定 | 快捷方式变暗；文件夹和智能文件夹分组写修复提示；展开播放集后写“资源库修复后可继续使用播放集。” | 已测试（未离屏） |

## Phase 3 文件浏览与变更

这些行由 `src-nana/src/shell/files_tests.rs` 覆盖状态机。宿主在归约后把副作用交给领域服务：浏览、新建、重命名和回收站用 `FileBrowserViewModel` 的 `get_file_browser`、`create_directory`、`create_file`、`rename_entry`、`mutate_trash`；硬链接用 `RepositoryInteractionViewModel` 的 `list_hardlink_candidates` 和 `confirm_hardlink_candidate`。复制、移动、导入、压缩包、Eagle 和删除走 Mutsuki 协议 `PROTOCOL_ENTRY_COPY`、`PROTOCOL_ENTRY_MOVE`、`PROTOCOL_ENTRY_IMPORT`、`PROTOCOL_ARCHIVE_IMPORT`、`PROTOCOL_EAGLE_IMPORT`、`PROTOCOL_ENTRY_DELETE`，协议调用没有替身测试。复制模式固定为 `hardlinkPreferred`。文件路由（`RouteKey::Files`）在启动就绪、主区有仓库、且面板是文件、回收站或智能文件夹时出现，列表用 `each_virtual`。验收场景和产品窗口挂同一棵文件页树。标了「已离屏」的行写明场景。

文件表面自己发起的首页是 80 条、继续加载是 160 条。侧栏目录、启动首屏和预览页「返回」（`OpenDirectory`）各读 200 条。四种展示都用 `each_virtual`，只建视口上下各 240px 以内的行，行高按内容量（`files_virtual.rs`）：列表一卡一行，卡片至少 72 高；网格卡片固定 148×190，按列表宽靠左装行；自适应卡片宽 `clamp(118, 120×宽高比+16, 300)`、缩略图高 120；瀑布流照 CSS 多列定列（列宽 164 起、列距 14、列宽均分），条目按顺序装列并平衡列高，不是最短列优先。缩略图纹理登记好（`texture_ready`）以后显示 Nana `Thumbnail`，否则卡片里画类型图标。

复制进行中，状态机记「创建硬链接或复制文件」32%，成功后重读目录时记「刷新文件索引」84%；界面不显示这两条（`FilesState::operation_label` 没有调用方），变更进行中文件列显示「正在处理文件」。点击一律替换选择；切换和范围只在状态机里（`SelectionMode`），界面没有入口。单击只改选择；400 毫秒内再点同一条才进入目录或打开预览。预览是文件页里单独的一页（`files_view::previewing`）：双击、右键「预览」或别处读回同一文件时整块换成预览页；单击只在右侧详情里看元数据。筛选栏常驻在文件页顶部，开合由 `filter_bar_open` 决定。实况树在 `prepare` 里把指针拖动和框选送进同一套输入归约：条目拖动 7px 起步，框选任一轴超过 3px，从空白处按下、松手没拖动则清空选择，拖到文件夹上则移进该文件夹。本地插件按 filesystem 拖放。缩略图预取由 `prepare` 拨空闲时钟，满 420ms 才解码。侧栏新建、重命名、删除、编辑、播放、移除和 Escape 走同一套缺口消息。预览页页头和右键菜单的「打开」「定位」走 `OpenEntry` / `RevealEntry`，由宿主启动系统程序；系统调用失败时状态区写「打开失败：{错误}」「定位失败：{错误}」。最近使用视图可以「清空记录」，成功后计数归零并静默重读摘要，不把当前目录列表清空。右键菜单有预览、打开（目录是「进入」）、定位、复制到…、加入播放列表、来源插件动作、缩略图（选择文件、从剪贴板、取消自定义、刷新）、重命名、删除；回收站多「还原」，「彻底删除」要点两次。来源 entryActions 里的 clear-cache 和 refresh-playback 调用 call_plugin 并带上当前仓库。下载先打开编号 5 的文件夹对话框，选中目录后把 `destination.kind` 为 `localFolder` 的路径放进载荷再调用；取消或失败不调用。创建来源播放列表弹出名称输入，空白不提交，确认后载荷带上名称和当前仓库再调用。自定义缩略图「选文件」打开编号 6 的 `OpenFileDialog`，「剪贴板」读已有文本剪贴板里的图片路径或 `data:image` base64，「取消自定义」写入 `clear`。保存和取消都进现有 `ensure_thumbnail`；文件取消后没有剩余路径时再 `refresh`。对话框取消不写入。认不出的剪贴板记失败，不假装已经有图。色板最多五枚 `#RRGGBB` 文本。

| 来源 | 触发 | 服务 | 结果 | 可见性 | 状态 |
| --- | --- | --- | --- | --- | --- |
| `files.ts` 分页 | 首屏或继续加载 | `get_file_browser` | 追加按 `kind:path` 去重；`has_more` 为合并后条数小于 `total_entries`；虚拟视图不再分页 | “滚动继续加载”：还有下一页或正在续读时才出现，续读中写“继续读取目录...”；没有更多和虚拟视图时不出现 | 已测试（未离屏） |
| `files.ts` 过期列表 | 快照的仓库、路径或回收站与 pending 不同 | 丢弃 | 保留原列表，清掉加载标记 | 仍是原来的条目 | 已测试（未离屏） |
| `files.ts` 读取失败 | 文件表面自己处于加载中 | 无新请求 | 写下错误，不替换列表；启动阶段的失败仍交给壳层切到错误页 | 错误留在文件列 | 已测试（未离屏） |
| `fileOperations.ts` 可写 | 新建、导入、复制、移动、重命名、删除 | 无 | 需要活动仓库、状态不是 missing、能力含 `write`；变更进行中全部拒绝 | 对应按钮禁用 | 已测试（未离屏） |
| `fileOperations.ts` 虚拟视图 | 智能文件夹或分类视图 | 无 | 智能文件夹拒绝新建、导入、重命名、删除，行来自查询结果；分类视图隐藏文件夹，允许重命名和删除，拒绝新建、导入和用户发起的目录浏览 | 眉题“分类视图”（智能文件夹也是），面包屑是智能文件夹名或分类名；建文件和导入不出现 | 已测试（未离屏） |
| `fileOperations.ts` `is_virtual` | 选中行带虚拟标记 | 无 | 复制、移动、重命名、删除和还原都拒绝 | 这些按钮禁用 | 已测试（未离屏） |
| `files.ts` 选择 | 替换、切换、范围 | 无 | 没有锚点或锚点已不在列表时，范围退回替换；快照丢掉已经不存在的路径，主选不在则取剩余第一项 | 界面没有切换方式的入口：点击一律替换，框选另写多选 | 已测试（未离屏） |
| `files.ts` 打开条目 | 双击文件夹 | `get_file_browser` | 单击只选中；双击进入该目录并清空选择；切换或范围下单击只选中，不进入 | 面包屑“根目录”和路径段 | 已测试（未离屏） |
| `files.ts` 打开文件 | 单击带 `asset_id` 的文件 | `get_asset_detail` | 选中并读取素材；右侧详情显示元数据，列表留着、不进预览页；双击或右键「预览」才换成预览页 | 选中该文件，右侧详情写出路径、大小和元数据 | 已离屏（`live-files-selected`、`files-selected-metadata`） |
| `fileOperations.ts` 空白提交 | 名称去空白后为空，或导入来源拆行和分号后为空 | 无 | 不进入变更中，不发请求 | 对话框保持打开 | 已测试（未离屏） |
| `fileOperations.ts` 新建和重命名 | 名称非空 | `create_directory` / `create_file` / `rename_entry` | 成功只采用返回快照里的名字；分类视图路径不一致时保留原列表并关闭对话框；失败保留原列表和对话框 | 变更进行中文件列显示“正在处理文件”，对话框主按钮写“处理中...”并禁用 | 已测试（未离屏） |
| `CopyTargetDialog.vue` 复制和移动 | 已有选择，目标路径由页内输入 | `PROTOCOL_ENTRY_COPY` / `PROTOCOL_ENTRY_MOVE` | 复制父目录为空表示根；移动父目录用空字符串表示根；路径里的 `\` 收成 `/` | 文件列显示“正在处理文件”，对话框主按钮写“处理中...”并禁用；复制入口是右键“复制到…”，移动走拖放 | 已测试（未离屏） |
| `fileOperations.ts` 协议结果 | 复制、移动、导入、压缩包、Eagle 或删除返回 | 成功后重新 `get_file_browser` | 成功不改当前行，保留选择并允许分类视图重读；智能文件夹不按目录重读；复制成功再列硬链接候选；失败保留原列表和对话框 | 对话框在成功后关闭 | 已测试（未离屏） |
| `fileOperations.ts` 删除 | 有选择且可写 | `PROTOCOL_ENTRY_DELETE` | 回收站 mode 为 `permanentDelete`，普通删除 mode 为空；失败保留原列表和对话框 | 文件列显示“正在处理文件”；回收站里“彻底删除”要点两次 | 已测试（未离屏） |
| `fileOperations.ts` 回收站 | 还原、还原全部、清空 | `mutate_trash` | 还原只在回收站；清空和还原全部不要求当前选择 | 文件列显示“正在处理文件” | 已测试（未离屏） |
| `HardlinkCandidateDialog.vue` | 候选到达（复制以后；刷新文件夹树以后；启动和换仓库绑定侧栏时照 Vue 后台读 `list_hardlink_candidates`）、跳过、确认 | 确认用 `confirm_hardlink_candidate`；跳过无请求；后台读取失败只记日志 | 跳过只记本地 id 并显示下一条；确认失败保留该条；确认成功按 id 移除，没有剩余则关闭 | “确认后会将新文件加入硬链接关联。”；跳过 / 加入关联 | 已测试（未离屏） |
| `files.ts` 展示方式 | 切换或读取 `file-display.json` | `PersistDisplayMode` | 值是 adaptive、masonry、grid、list，也接受中文标签；未知、损坏或文件缺失回到自适应；只在切换时写入。四种都用 `each_virtual` 按视口虚拟化，行高按内容量；瀑布流照 CSS 多列分列 | “自适应 / 瀑布流 / 网格 / 列表” | 已测试（未离屏） |
| `files.ts` 重命名草稿 | 选择变成多项，或主选变化 | 无 | 清掉重命名路径和草稿，并关闭重命名框 | 重命名框消失 | 已测试（未离屏） |
| Eagle 导入 | 模式为 copy 或 move，库路径非空 | `PROTOCOL_EAGLE_IMPORT` | 其他模式不打开对话框；空白路径不提交 | 对话框主按钮写“处理中...”并禁用 | 已测试（未离屏） |

## Phase 4 预览、元数据和搜索

预览和元数据由 `src-nana/src/shell/inspect_tests.rs` 覆盖状态机，搜索和筛选由 `search_tests.rs` 覆盖，解码和内置预览的单测在各自模块里（`native_preview.rs`、`video_preview.rs`、`audio_decode.rs`）。浏览、详情和文本字节走 `RepositoryQueryViewModel` 的 `get_asset_detail`、`read_file`、`prepare_preview_file_source`；元数据保存走 `update_asset_metadata`；搜索走 `search_assets`；撤销和重做走 `RepositoryInteractionViewModel` 的 `undo_last_revision`、`redo_last_revision`。这些调用没有替身测试。预览按扩展名分派（`inspect_support.rs`）：图片在产品窗口经宿主纹理槽 `file-preview` 用 `GpuTextureView` 显示；Markdown 和纯文本同走等宽原文，顶栏多一枚「Markdown」标签；文件预览里的 WAV、mp3、flac 和 ogg 读出 PCM 后与底部播放条共用游标，未压缩和 MJPEG 的 AVI 用纯 Rust 解出画面和 PCM，其余认得出的容器在 Windows 上用系统媒体基础源读取器解画面和音轨，不打开窗口、不拉起播放器；没有画面的 m4a、aac、opus 只解音轨。Windows 非测试构建用 winmm 出声，单元测试构建不开设备。ZIP、CBZ、7z、RAR、CBR 列出文件。PDF 按内容流画出页面并可翻页，文案是「当前页 / 总页」，只留前 32 页；ASCII85、LZW、Flate 和 DCTDecode 会解，解不开的流是该页失败，全部页失败则整份失败。Open XML（含 pot、ppsx、xlsb、xltx、dotm 等）以及 OLE 文档里的 UTF-16 片段会抽出文本；抽不出文本是错误态。OBJ、glTF、GLB、STL、3MF 和 VRM 能抽出三角形时软件光栅，可旋转、缩放；抽不出时保留结构摘要。FBX 和 BLEND 只看文件头。PDF 页和模型光栅编码成 PNG 画进预览框；Nana `GpuView` 没有网格管线，网格不走 `GpuView`。不嵌入 pdf.js、Three.js 或 Chromium。播放列表当前项的装载在阶段 5。标了「已离屏」的行写明场景。

搜索面板在启动就绪后、面板是搜索时出现：有仓库是 `RouteKey::Search`，没有资源库是 `EmptySearch`。预览页在 `page == SelectedFile` 且不是单击选中时整块替换文件工作台（`files_view::previewing`，页面在 `inspect_preview_frame.rs`）：双击、右键「预览」，以及从搜索结果或播放集打开的文件会进；单击只选中，右侧 300px 详情卡显示缩略图、事实和元数据。标题栏输入先切到搜索面板，查询在 250 毫秒后才跑；在筛选栏里改条件也切到搜索面板。画面上没有保存按钮：草稿变脏后 260 毫秒自动保存，换选前先写完上一份。版本冲突保留本地草稿和原来的 `expected_version`。生产环境从内置的压缩包、PDF、文档和模型贡献开始。库类型快捷方式在插件加载前是空的；加载后，对象形式的 searchShortcuts，以及 ASMR 官方字符串 id，在当前文件或搜索结果里有该库类型的条目时出现在筛选栏。未知字符串记日志并跳过。后登记的同扩展名预览贡献优先。

标签候选是元数据区里铺开的面板，由「添加标签」和「+」开合，加了标签或按 Escape 关闭；候选取自当前目录各行的标签，按输入过滤，最多 18 个（`inspect_metadata_view.rs`）。`inspect_tags.rs` 里按视口夹取、点外面关闭的标签菜单状态只有测试在用。筛选芯片可以再点一次取消。图片走 `GpuTextureView::new("file-preview")`，没用全窗模态的 `ImageViewer`。资源库扩展认领 `lyricStatus`、`listeningStatus`、`listeningProgress`、`lastListenedAt` 等键，画进「作品信息」区块，有歌词正文键时多一个「歌词」区块（`inspect_library.rs`）；音频舞台右侧是歌词面板，没有正文时写「暂无歌词」（`inspect_audio_stage.rs`）。Vue 库类型注册表本身不进生产树，已经能用筛选数据表达的快捷方式会进筛选栏。

| 来源 | 触发 | 服务 | 结果 | 可见性 | 状态 |
| --- | --- | --- | --- | --- | --- |
| `filePreviewExtensions.ts` 图片 | 扩展名是 png、jpg、jpeg、webp、gif、bmp、avif、svg | `prepare_preview_file_source` 后 `read_file`，没有替身 | 媒体类型以 `image/` 开头才解码；失败是错误态，不显示空纹理；路径与当前目标不一致时丢弃 | 读取中左上角胶囊“准备播放 / 准备媒体”和进度条；纹理槽 `file-preview`（离屏没有宿主纹理时按像素画） | 已测试（未离屏） |
| `filePreviewExtensions.ts` Markdown | md、markdown、mdown、mkd、mkdn、mdx | `read_file`，没有替身 | 先于纯文本列表；和纯文本一样按等宽原文显示，顶栏种类写「Markdown」 | 读取中胶囊“读取文本” | 已测试（未离屏） |
| `filePreviewExtensions.ts` 纯文本 | txt、text、log、csv、tsv、json、jsonl、yaml、yml、toml、xml、html、css、scss、sass、less、js、jsx、ts、tsx、vue、rs、py、rb、go、java、c、h、cpp、hpp、cs、php、sh、bash、zsh、ps1、bat、cmd、ini、cfg、conf、env、gitignore、gitattributes | `read_file`，没有替身 | 只取前 768 KiB 并标「仅显示前 …」；按 BOM 识别 UTF-8 / UTF-16，其余宽松解码；等宽原文，不着色 | 顶栏写扩展名和“N 行”；截断时多一枚“仅显示前 768.0 KB” | 已测试（未离屏） |
| `filePreviewExtensions.ts` 音视频 | mp4、mov、mkv、webm、avi、m4v、mp3、wav、ogg、flac、m4a、aac、opus | `read_file` 后解析 WAV，并用 symphonia 解 mp3、flac、ogg。未压缩和 MJPEG 的 AVI 用纯 Rust；其余认得出的容器在 Windows 上用媒体基础源读取器。没有画面时只解音轨 | WAV、mp3、flac、ogg、解出的视频，以及没有画面的 m4a、aac、opus 进入 paused。播放、暂停、跳转和音量与底部播放条共用会话；有音轨时 Windows 非测试构建用 winmm 出声，单元测试构建不开设备。画面进预览纹理 | 读取中居中卡片写文件名和“准备音频”；音频装好后是唱片舞台，播放条接管 | 已离屏（`preview-audio`，只有 mp3） |
| 音视频解不开 | 同上，字节读不出、解不开或认不出 | 同上 | failed：视频容器和 m4a、aac、opus 写「解码失败」，wav、mp3、flac、ogg 写各自格式的原因，认不出的字节是「没有原生解码器」；失败不接管播放条 | 预览框里红色标题“无法预览该音频”（视频是“无法预览该媒体”），下面一行原因 | 已离屏（`preview-audio-failed`） |
| `office-preview` / `preview-archive` / `three-model-preview` | 扩展名不在内置列表，有 Preview 贡献、但 view 不是内置能画的 | 无，不读文件 | 写「原生预览 · {label} · {view_id}」，没有升级提示；没有贡献时走「未知扩展名」行。pot、ppsx、xlsb、xltx、dotm 等已并进内置 Open XML/OLE 抽文本。不嵌入 office-convert、pdf.js 或 Three.js | “原生预览 · {label} · {view_id}” | 已测试（未离屏） |
| 内置压缩包、PDF、文档和模型 | zip、cbz、7z、rar、cbr、pdf、docx、docm、doc、dotx、dotm、dot、xlsx、xlsm、xlsb、xls、xltx、xltm、xlt、pptx、pptm、ppt、ppsx、ppsm、pps、potx、potm、pot、obj、gltf、glb、stl、3mf、vrm、fbx、blend | `read_file` 后按 view 解析，没有替身 | 压缩包列文件，最多 200 条；PDF 按内容流分页绘制，只留前 32 页，翻页显示「当前页 / 总页」，解不开的滤镜是该页失败；Open XML 抽段落；OLE 刮 UTF-16 片段；抽不出文本是错误态；能抽出三角形的模型软件光栅并可旋转缩放，否则保留摘要；FBX/BLEND 只看文件头。坏文件是错误态 | 读取中写“读取压缩包”“载入 PDF”“转换文档”“读取模型”；PDF 翻页“1 / N”；FBX 写“二进制 FBX”或模型数，BLEND 写“Blender 版本 …” | 已测试（未离屏） |
| `RegisterPreview` | 贡献种类是 Preview，扩展名匹配 | 已知 view 走上面的读取；未知 view 不读文件 | 后登记的同扩展名优先；其他种类忽略并记日志 | 未知 view：“原生预览 · {label} · {view_id}” | 已测试（未离屏） |
| 未知扩展名 | 没有内置类型，也没有 Preview 贡献 | 无 | 失败，不显示空图片 | 预览框画缩略图或类型图标，不写字 | 已测试（未离屏） |
| 过期预览 | 文本代次或图片路径与当前目标不同 | 丢弃 | 保留当前预览体 | 仍是当前目标 | 已测试（未离屏） |
| `FileMetadataEditor.vue` 草稿 | 评分、注释、链接、标签；自定义字段只有 ASMR 候选应用会写 | 无，直到保存 | 评分 0–5，相同值回到 0；空白和重复标签忽略；保留键不能当自定义字段；注释没有时回退 note。自定义行只读，跳过编辑器自己画的通用字段，以及后端给每个素材种下的 title、type、favorite、color | 注释、链接输入框里是草稿；评分是五颗星（无障碍名“1 星”…“5 星”） | 已测试（未离屏） |
| `FileMetadataEditor.vue` 保存 | 草稿已脏，且不是虚拟素材、不是保存中 | `update_asset_metadata`，`source` 为 desktop，没有替身 | 请求带 `expected_version`，以及评分、注释、链接和 `tagGroups`；自定义字段只写这次改过的，没动过的不回写，数字、布尔和数组不会被改存成字符串；虚拟、未脏或保存中不发请求；错误保留草稿。脏草稿再等 260 毫秒自动保存，换选前先保存上一份 | 画面上没有保存按钮；保存中评分、注释、链接和标签控件禁用；失败写进状态区“保存元数据失败：…” | 已测试（未离屏） |
| `FileMetadataEditor.vue` 冲突 | 结果是 conflict | 不覆盖草稿，`expected_version` 不变 | 采用服务器版本后才替换草稿和版本 | “版本冲突，未写入” / “采用服务器版本” | 已离屏（`conflict`） |
| `assetMetadata.ts` 撤销和重做 | 草稿不脏 | `undo_last_revision` / `redo_last_revision`，没有替身 | 有未保存编辑时拒绝并记日志 | 没有界面入口（Vue 也只在 composable 里导出） | 已测试（未离屏） |
| `search.ts` 空条件 | 查询和筛选都为空 | 不调用 `search_assets` | 清空结果，结果区回到等条件 | “等待搜索条件” | 已测试（未离屏） |
| `search.ts` 查询 | 有关键词或筛选条件 | `search_assets`，没有替身 | 标题栏改查询后等 250 毫秒再搜。AND 不传 `matchMode`；错误保留上一份结果；过期代次忽略成功和失败。跑完没有命中时空状态按页头范围写明哪里没有匹配的文件；失败时只写错误，不另写空状态 | “当前查询: …” / “当前资源库筛选: …” / “没有匹配的文件” | 已离屏（`search-empty`，只走空结果） |
| `WorkspaceFilterBar.vue` 筛选 | 标签、格式、颜色、形状、评分、高级条件 | `search_assets`，没有替身 | 颜色和形状变成 color、shape 元数据筛选；只有 OR 传 `matchMode`；排序字段为空不传排序；limit ≤ 0 为空；minRating ≤ 0 为空 | 评分芯片“全部”“1 星+”…“5 星+”；筛选栏没有匹配方式开关 | 已测试（未离屏） |
| `selectors.ts` `hasActiveFilters` | 只有排除关键词、排除路径、排除数值或排除日期 | 不设置 `repoId` | 这些条件本身仍然会发起搜索 | 摘要保持“当前查询” | 已测试（未离屏） |
| `search.ts` 缺少仓库 | 活动筛选需要仓库，但没有活动仓库 | 不请求 | 清空结果并记日志 | 结果被清空 | 已测试（未离屏） |
| `useSearchUi.ts` 筛选栏 | 开关、清空、不可写 | 清空后按剩余查询重跑 | 开关不清筛选；清空回到初始筛选，不关栏也不清查询；资源库不可写时忽略筛选变更 | 标题栏开关“显示筛选栏 / 隐藏筛选栏”（提示“筛选”）；栏内“清除”“关闭筛选栏” | 已测试（未离屏） |
| `filterInputs.ts` 解析 | `key=value`、`key=min..max`、`key=from..to`、路径 | 无 | 空键或空值丢掉；数值要有一个有限边界；`\` 收成 `/` 并去掉首尾斜杠 | 高级筛选输入 | 已测试（未离屏） |
| `SearchPanel.vue` 打开结果 | 点击一条结果 | `get_asset_detail`，没有替身 | 切到文件面板，选中路径，用结果自己的仓库 id；空素材 id 报错，不假装成功 | “搜索结果没有素材 id” | 已测试（未离屏） |
| `WorkspaceFilterBar.vue` 库类型快捷方式 | `ApplyShortcut` | 无 | 插件加载后登记对象快捷方式和 ASMR 的 works、lyrics、continue、random，当前文件或搜索结果里有该库类型的条目时才显示；点按钮写入已有筛选和排序。未知字符串跳过。不可写时忽略 | “ASMR 作品” / “含歌词” / “继续收听” / “随机一首” | 已测试（未离屏） |
| 主按钮 | 已经有预览目标 | 按当前类型重新打开 | 没有目标时写「当前没有可打开的预览」，且不重复拉取预览；`ShellMessage::PrimaryAction` 没有界面发送方，文字写进没有视图读的 `detail` | 没有界面入口 | 已测试（未离屏） |
| 预览页 | `page == SelectedFile`，且不是单击选中（`files_view::previewing`） | 无 | 整块替换文件工作台，眉题键 `inspect-preview-eyebrow`；双击、右键「预览」，以及从搜索结果或播放集打开的文件会进，单击不进 | “文件预览” | 已离屏（`selected-file`、`live-preview`、`preview-text`、`preview-image`、`preview-audio`） |
| 搜索面板 | 启动就绪后面板是搜索 | 无 | 有仓库是 `RouteKey::Search`；没有资源库是 `EmptySearch`，写「还没有可搜索的资源库」 | “输入关键词、标签或评分条件后，这里会展示跨仓库结果。” | 已离屏（有仓库：`filter-bar`、`search-empty`） |

## Phase 5 播放列表

这些行由 `src-nana/src/shell/player_tests.rs`（含 `player_wav_tests.rs`）和 `player_clip_tests.rs` 覆盖状态机，播放集页和路由由 `player_playlist_page_tests.rs`、`route_playlists_tests.rs` 覆盖，系统媒体键的测试在 `system_media.rs`。成员走 `RepositoryInteractionViewModel` 的 `list_playlist_memberships` 和 `set_playlist_membership`；目录和不能切换的条目走已有的 `add_playlist_items_by_paths`；排序走已有的 `reorder_playlist_items`；移除条目走 `remove_playlist_item`；恢复详情走 `get_playlist_detail`；当前项的字节走 `read_file`；下载走 `MutsukiTaskViewModel::execute` 的 `momobako.playlist.download`。这些调用没有替身测试。播放条和预览共用一份 `PlaybackSessionState`。预览里的 WAV、mp3、flac、ogg，以及 Windows 上媒体基础解出的 m4a、aac、opus，会把 PCM 装进同一游标；Windows 非测试构建用 winmm 出声，单元测试构建不开设备。播放列表的当前项（音频、视频、图片）只读、只解这一条：先置 loading，字节经仓库服务读出，在任务里解码，用 `ItemLoaded` 装进现有会话；画面 16 MiB、PCM 32 MiB 的上限沿用预览；没有当前项不解码。没有候选也没有贡献时写「缺少对应播放插件」或回退提示；只有贡献、扩展名又不是音视频会话或图片时写「暂不支持播放 {扩展名} 文件」；都不会变成 playing。播放集页的条目放在 `ReorderList` 里，行内只有「播放」「移除」，没有上移、下移；验收场景渲染的就是这一页。标了「已离屏」的行写明场景。

播放条在启动就绪、主区有仓库，且面板是文件或播放列表时出现；预览页底部的播放条在预览打开时就出现。会话写到设置目录的 `playback-sessions.json`，只保存非临时条目。生产环境的原生播放候选只有内置解码器：WAV、MP3 / FLAC / Ogg，Windows 上还有交给媒体基础的 M4A / AAC / Opus；插件登记的播放器贡献在插件列表换新以后读回（见阶段 6）。播放器的播放集列表和侧栏是同一份：侧栏绑定仓库时读，新建、删除以后换上返回的整份列表。Windows 非测试构建注册系统媒体传输控件；单元测试构建不注册，也不打开真系统会话。

下载（`momobako.playlist.download`）目前没有界面入口，只有状态机和测试：有任务编号之后取消下载记下「正在取消下载…」，编号返回前是「下载任务还没有可取消的句柄」；`DownloadProgress` 可以在整批结果返回前更新「正在下载 N / N，失败 N」，下载进行时这一行画在播放条下面。设置页播放器选择在阶段 6。

| 来源 | 触发 | 服务 | 结果 | 可见性 | 状态 |
| --- | --- | --- | --- | --- | --- |
| `usePlaylistMembershipUi.ts` 目录 | 条目是目录 | `add_playlist_items_by_paths`，没有替身 | 兼容列表是全部播放列表；有路径才提交 | “正在按路径加入播放列表…”（只写进 `PlayerState.activity`，界面不显示） | 已测试（未离屏） |
| `usePlaylistMembershipUi.ts` 无扩展名 | 文件扩展名为空 | 不请求 | 没有兼容播放列表 | 不出现加入按钮 | 已测试（未离屏） |
| `usePlaylistMembershipUi.ts` 扩展名 | 扩展名命中播放器类型 | 无，直到点击 | 候选和贡献的扩展名合并，忽略大小写 | “加入 {名称}” / “移出 {名称}” | 已测试（未离屏） |
| `usePlaylistMembershipUi.ts` 切换 | 文件、有素材 id、不是虚拟项，且资源库可写 | `set_playlist_membership`，没有替身 | 已在列表中则移除，否则追加；本地成员要等 `MembershipSaved`；失败保留旧成员 | 右键菜单的“加入 {名称}” / “移出 {名称}”；“正在更新播放列表成员…”只写进 `PlayerState.activity`，界面不显示 | 已测试（未离屏） |
| `usePlaylistMembershipUi.ts` 虚拟或无素材 | 虚拟项，或没有素材 id | `add_playlist_items_by_paths`，没有替身 | 不走成员切换；没有路径则不请求 | “正在按路径加入播放列表…”（只写进 `PlayerState.activity`，界面不显示） | 已测试（未离屏） |
| `usePlaylistMembershipUi.ts` 拒绝 | 资源库不可写，或没有活动仓库 | 不请求 | 记日志，成员不变 | 成员列表不变 | 已测试（未离屏） |
| `workspace.ts` 成员索引 | 播放列表加载、读取失败、仓库不一致或列表为空 | `list_playlist_memberships`，没有替身 | 失败清空；过期仓库忽略；空列表清空且不请求 | 成员与当前仓库一致 | 已测试（未离屏） |
| `ReorderList` 排序 | 拖到某项之前，或放到末尾 | `reorder_playlist_items`，没有替身 | 用播放列表页的条目顺序，不先改本地；相同顺序或缺少 id 不请求；不可写时忽略 | “正在保存播放列表顺序…”（只写进 `PlayerState.activity`，界面不显示） | 已测试（未离屏） |
| 播放集条目 | 点开播放集且有条目 | `ReorderList` 和「移除」按钮 | 列表键 `playlist-reorder`，行键 `playlist-item-{id}`，移除键 `playlist-remove-{id}`（id 经 `key_part` 转义） | “移除” / “演示播放列表” | 已离屏（`playlist-open`） |
| `playlistPlayerPreference.ts` 回退 | 解析某个播放器类型 | 无 | 先用偏好，再官方，再第一个候选；音频能力不会落到未选择的第三方；偏好缺失时 `fallbackUsed` | “所选播放器当前不可用，已回退到 …” / “官方音频播放器未启用或缺失，音频播放暂不可用。” | 已测试（未离屏） |
| `playlistPlayerPreference.ts` 空能力 | 能力标识为空白 | 不写偏好 | 记日志 | “播放器能力标识不能为空”（只写进 `PlayerState.activity`，界面不显示） | 已测试（未离屏） |
| `usePlaylistPlayer.ts` 缺少插件 | 播放列表项就绪，但没有候选也没有贡献 | 不调用解码器 | 状态 failed，不是 playing | “缺少对应播放插件” | 已离屏（`playback-running`） |
| `usePlaylistPlayer.ts` Vue 贡献 | 只有贡献、没有原生候选，且扩展名既不是音视频会话也不是图片 | 不调用解码器 | 状态 failed，时长清空，不能跳转、不能调音量；扩展名属于音视频会话或图片时改走「播放列表当前项」的装载 | “暂不支持播放 {扩展名} 文件” | 已离屏（`outside-playback`） |
| 内置 WAV | 候选是 `momobako.player.wav`，文件是 PCM WAV | 经仓库服务 `read_file` 读出字节再解析（`LoadItem`、`ItemLoaded`）。Windows 非测试构建用 winmm `PlaySoundW` 异步播放；单元测试构建不开设备 | 读取中 loading；装好后有播放意图直接 playing，没有才 paused；seek 写入 `current_time_ms`；pause 回到 paused；设备失败只记日志，会话不因此变成 failed | 会话 `playing` | 已测试（未离屏） |
| 内置音频候选认领 | 预览或插播的扩展名没有插件贡献 | 无 | 各平台由内置候选认领 wav、mp3、flac、ogg；Windows 上再由 `momobako.player.system-audio` 认领 m4a、aac、opus（媒体基础解音轨），其他平台不认领。认领后预览接管播放条，PCM 进同一游标；没有候选时不接管 | “没有可用于播放此媒体的插件” | 已测试（未离屏） |
| `usePlaylistPlayer.ts` 解码失败 | 解析到的原生候选不是内置内存候选（WAV、MP3 / FLAC / Ogg、M4A / AAC / Opus） | 不读文件，直接失败 | 播放、暂停、跳转和音量仍是 failed；音量和进度不被失败改写。生产候选都是内置内存候选，只有测试注入的候选走到这里 | “没有原生解码器” | 已测试（未离屏） |
| 播放列表当前项 | 当前条目扩展名是 mp4、mov、mkv、webm、avi、m4v、m4a、aac、opus | 只读这一条：`read_file` 后经 `decode_media` 解码，沿用预览的画面和 PCM 上限 | 没有当前项不解码。有 PCM 时进入现有播放会话；Windows 非测试构建出声，单元测试构建不开设备。解不开是 failed，文案含「解码失败」。队列里其它路径不打开 | “解码失败” | 已测试（未离屏） |
| `usePlayerUi.ts` 图片停留和适配 | 当前项是图片，或切换适应 / 填充 | 写入 `playback-settings.json` | 停留钳在 2000–30000，坏数字回到 5000；只有 cover 是填充 | “适应” / “填充” | 已测试（未离屏） |
| `usePlaylistPlayer.ts` 模式 | 循环、随机、单曲，以及自然结束 | 无 | 单曲自然结束留在当前项；手动到末尾停止；随机缺序列时重建后取后继，走到末尾时重建后取新序列第一项 | “列表循环” / “随机播放” / “单曲循环” | 已测试（未离屏） |
| `usePlaylistPlayer.ts` 临时项 | 从文件插播（产品里由预览接管触发，`PlayEntry` 没有界面发送方），或自然结束 | 无 | 相同路径的旧临时项去掉；自然结束后清掉已经不是当前项的临时项；活动列表详情保留当前项和历史里的临时项 | “当前队列” / “N 项” | 已测试（未离屏） |
| `usePlaylistPlayer.ts` 跨仓库插播 | 当前仓库不同 | `stop(false)`，不删存储 | 先确认有播放器，再停运行时；临时项不写入会话文件 | 会话仍是 failed | 已测试（未离屏） |
| `usePlaylistPlayer.ts` 会话 | 非临时项开始播放，或停止 | 写入 `playback-sessions.json` | 坏 JSON 或缺文件回到默认；不完整会话忽略；停止把状态写成 ended，时间归零，保留当前 id；切仓库的停止还删掉该仓库的存储，系统媒体键的停止不删 | “0:00 / 0:00” | 已测试（未离屏） |
| `AppShell.vue` 恢复 | 播放集列表（侧栏读回、新建或删除以后）里有存储的列表，播放器仓库还不是该仓库，且播放器类型认得：内置候选当场认得，插件类型等播放器类型读回；读回了还不认得就清掉会话 | `get_playlist_detail`，没有替身；详情在读时不重复读 | 类型须是候选或贡献，条目须就绪且列表一致；否则清掉该仓库会话。照 Vue `setActivePlaylist(.., { restore: true })`：重新装载当前项（先 loading），模式、音量、播放意图沿用存下的；装载期间进度和时长显示存下的值、会话文件不被清零；装好以后能跳转就跳到存下的位置（超过时长夹到结尾），图片幻灯片不能跳转、从头放；装载失败保留存下的进度 | 音量保留，进度从存下的位置接着 | 已测试（`player_wav_tests.rs`，未离屏） |
| `AppShell.vue` 切仓库 | 播放另一个仓库的列表，或停止指定仓库 | 清掉被换下仓库的会话 | 即使当前播放仓库已经不同，也删除被指定仓库的存储；只停止仍属于该仓库的运行时 | 被停止的会话是 ended | 已测试（未离屏） |
| `WorkspacePlayerBar.vue` 共用会话 | 预览读到音视频，或播放条改音量 | 与预览共用 `PlaybackSessionState` 和同一 WAV 游标 | 图片预览不覆盖播放条；WAV、mp3、flac、ogg、解出的视频，以及没有画面的 m4a、aac、opus 会装入会话，播放条的播放、跳转和音量在有音轨时于 Windows 非测试构建出声，单元测试构建不开设备；解不开的媒体停在 failed；播放条失败时预览里的会话一起停在 failed 或 ended | “没有原生解码器” / “解码失败” | 已测试（未离屏） |
| `usePlaylistPlayer.ts` 打开预览 | 当前有条目 | 有素材 id 时 `get_asset_detail`，没有替身 | 切到文件面板和全部分类，并选中路径 | 点播放集条目的标题或扩展名方块，或播放条左侧的封面和两行字 | 已测试（未离屏） |
| `momobako.playlist.download` | 插件提交下载请求（当前没有界面入口） | `MutsukiTaskViewModel::execute`，没有替身 | 提交阶段是 submitting；返回的进度按顺序应用；其他列表的事件忽略；空结果记为 complete；失败记为 error | “正在提交播放列表下载…” / “正在下载 N / N，失败 N” | 已测试（未离屏） |
| `momobako.playlist.download` 取消 | 任务编号还没有返回 | 不调用 `cancel` | 记日志，并写进 `PlayerState.activity`（界面不显示） | “下载任务还没有可取消的句柄” | 已测试（未离屏） |
| `useSystemMediaSession.ts` | 播放、暂停、上一首、下一首、跳转、停止 | 无。动作写回现有播放条消息 | Windows 非测试构建 `system_media_session_available` 为真，并向当前窗口注册系统媒体控件。播放、暂停、上一首、下一首、跳转和停止分别写成 `SetPlaying`、`PlayPrevious`、`PlayNext`、`Seek`、`Stop`，停止不删除已存储会话。标题用元数据 title，否则用文件名；艺人优先用 artists 数组。封面优先用播放列表缩略图。单元测试构建恒为假，不注册真会话 | “夜曲” / 测试构建没有系统会话 | 已测试（未离屏）。测试不注册真系统会话 |
| 播放条 | 启动就绪、主区有仓库，且面板是文件或播放列表；预览页底部的播放条在预览打开时就出现 | 无 | 文件路由的工作台和预览页、播放集路由各有一个播放条岛；播放条根键 `player-card`，下载进行时外面多一层 `player-surface` | “未选择播放内容” / “0:00 / 0:00” / “列表循环” | 已离屏（`file-list`、`playback-running`、`preview-audio`） |

## Phase 6 设置、插件、日志和任务

这些行由 `src-nana/src/shell/admin_tests.rs` 覆盖状态机，读取时机由 `data_load_tests.rs` 从启动消息和用户操作出发覆盖。设置包一次读五项：`PluginViewModel` 的 `list_plugins`、`list_plugin_hook_executions`、`get_cache_snapshot`、`get_api_design_snapshot`，加 `SystemViewModel::get_external_api_connection_status`；照 Vue `loadSettingsData` 的时机读：启动结束时（有仓库、空库和缺失仓库都读）、打开设置页、插件面板「刷新」，拓展页还没有插件列表时也补读一次。插件列表换新（设置包读回，或安装、删除、启停之后）就用 `list_playlist_players` 重读播放器类型，和 Vue 同步前端插件注册表的时机一致。打开设置页还读一次设置目录里的应用设置（`SettingsLoaded`）。安装、删除、启停、配置读写和数据目录走同一个 `PluginViewModel`。外部连接导出走 `SystemViewModel::write_binary_file`。仓库动作列表走 `RepositoryInteractionViewModel::list_repository_actions`，执行走 `MutsukiTaskViewModel::execute` 的 `momobako.repository.action.run`，完成后用 `FileBrowserViewModel::get_file_browser` 刷新当前目录。这些调用没有替身测试。圆角写到设置目录旁边的 `corners.json`，缺文件或坏 JSON 用平台默认，用户改过才写回。音频播放器偏好仍走阶段 5 的偏好文件，不写入 `ApplicationSettings.default_playlist_player_type_id`。日志页每次打开都用 `SystemViewModel::list_system_logs` 读最近 200 条历史日志（Vue `setActivePanel('logs')` 的 `loadSystemLogsInWorkspace`）。标了「已离屏」的行写明场景。

设置页在页面是设置或设置错误时出现。动作、工具页和日志还要求启动就绪且主区有仓库；任务弹层只要侧栏在。工具页从空列表开始，读到插件清单后按清单里的工具页声明填上。剪贴板写入系统剪贴板，插件目录定位交给宿主。保存对话框和打开对话框会发出 Nana 的 `OpenFileDialog`，系统对话框的结果仍要设备验证。插件页的安装、删除和启停都经 `PluginsReplaced` 回写列表，不切页。设置页读到插件配置时留在设置页。非法数字不提交。日志里的插件和仓库选项按字典序。

剪贴板写入成功显示「{名称} 已复制。」（例如「Token 已复制。」），失败是「复制失败：系统剪贴板写入失败」。插件设置目录定位成功显示「已打开…设置目录。」。插件设置只画清单 `contributes.settings.fields` 声明的字段，带输入、重置和「打开目录」；没有字段的设置页（例如下载服务）只画设置区页头和「打开目录」，没有升级提示。来源账号区建扫码会话成功后写「请扫码并在手机端确认，然后检查登录结果。」，仓库行写登录状态；返回的 `qrUrl` 或 `qrurl` 用 `QrCode` 重新编码画出来，`data:` 图片和 credentialRef 不画。文件右键里 clear-cache 和 refresh-playback 调用 `call_plugin`，成功文案「已调用 {method}。」只写进 `files.activity`，界面不显示。下载在编号 5 的文件夹对话框返回目录后调用，载荷带 `destination`；取消不调用。创建来源播放列表在名称非空后调用，载荷带名称和当前仓库。任务弹层没有取消按钮，Vue 的 `TaskPopover.vue` 也没有。侧栏在有仓库动作时，于快捷访问之后增加「动作」入口。第三方工具页只画页头和「这个工具页由插件的前端组件绘制，Nana 原生界面不能运行它。」。

| 来源 | 触发 | 服务 | 结果 | 可见性 | 状态 |
| --- | --- | --- | --- | --- | --- |
| `Settings.vue` 音频播放器 | 选择一个实现，或空白 | 阶段 5 的偏好写入，没有替身 | 只认 `momobako.player.audio`；实现取自启用且状态不是 error 的插件清单 `playlistPlayers`（和后端登记播放器类型同一口径），加上登记了该能力的原生候选；空白清除偏好；缺失项显示但不可选；解析仍用音频序列类型。没有选中也没有官方实现时，音频由内置解码器（WAV / MP3 / FLAC / Ogg，Windows 上加 M4A / AAC / Opus）播放：下拉框按本平台实际能解的格式写它，回退提示指向它，只有连内置解码器都没有才报缺失 | “Audio Player · momobako.player.audio” / “内置解码器 · WAV / MP3 / FLAC / Ogg / M4A / AAC / Opus”（其他平台没有后三项） / “所选播放器当前不可用，已回退到 …” | 已测试（未离屏） |
| `useCornerStyle` 圆角 | 样式或半径 | 写入 `corners.json` | 只接受 smooth 和 round；半径钳在 0–20；未知样式和坏数字忽略；缺文件不立刻写回 | “平滑” / “普通” | 已测试（未离屏） |
| `Settings.vue` 后端计数 | 仓库列表成功 | 无 | 按后端插件累计，保留首次出现的顺序 | “本地 (2)” / “无” | 已测试（未离屏） |
| `Settings.vue` 外部连接 | 复制或导出 | 有路径时 `write_binary_file`，没有替身；复制交给宿主剪贴板；导出先发 `OpenFileDialog` | 空值不复制；令牌取前 10 和后 6 位；取消导出不写文件；写出成功后记文件名；剪贴板写入失败才显示失败文案 | “Token 已复制。” / “复制失败：系统剪贴板写入失败” / “external-api.json 已导出。” / “导出失败：{原因}” | 已测试（未离屏） |
| `Settings.vue` 设置包 | 启动结束、打开设置页、插件面板「刷新」，或拓展页还没有插件列表 | 五项读取（插件、钩子记录、缓存、API 设计、外部连接），没有替身；读回后重读播放器类型 | 任一失败则五份都不写入，保留上一份 | 插件管理面板的错误条写原文，例如“读取插件目录失败：拒绝访问。 (os error 5)” | 已离屏（`settings-error`，只走全败分支） |
| `repository.select` | 插件事件带来仓库 id | 非空白时走现有仓库选择 | 空白忽略，当前仓库不变 | 当前仓库不变 | 已记账（`AdminMessage::SelectRepository` 没有发送方，也没有测试） |
| `PluginManagerPanel.vue` 分组和搜索 | 插件列表或关键词 | 无 | 分类顺序是来源、库类型、解析、预览、服务、未分类；未知分类进未分类；搜索不区分大小写 | “3 个插件” | 已测试（未离屏） |
| `PluginManagerPanel.vue` 删除 | 用户插件确认，或非用户插件 | `delete_plugin`，没有替身 | 先进入待确认，取消不请求；非用户插件忽略；成功文案在列表替换后出现 | 确认框标题“删除插件”，按钮“删除”（忙时“删除中...”） / “插件已删除。” | 已测试（未离屏） |
| `PluginManagerPanel.vue` 字段 | 数字、选择、布尔、JSON | `set_plugin_config_value` 或 `delete_plugin_config_value`，没有替身 | 空数字或空选项是重置；非法 JSON 不请求；空 JSON 文本按 null 保存 | “原始 不是有效 JSON。” / “插件设置已保存。” | 已测试（未离屏） |
| `PluginManagerPanel.vue` 设置页 | 再次打开同一插件，或路由到未知插件 | 已有快照时不重复 `get_plugin_config` | 第二次折叠；未知和空白 id 忽略；设置页加载配置不跳走 | “插件设置” | 已测试（未离屏） |
| `PluginManagerPanel.vue` 启停和安装 | 实况启停，或选择安装包 | `set_plugin_enabled`、`install_plugin_from_archive`，没有替身；选择安装包先发 `OpenFileDialog` | 不切到插件设置页；空白路径不安装；取消对话框不安装 | “插件已禁用。” / “插件已安装。” / “插件包选择失败：{原因}” | 已测试（未离屏） |
| `PluginManagerPanel.vue` Vue 页面 | 自定义设置页或来源账号 | 有 `fields` 的设置页用原生字段，不读 `provider.settings`。来源账号按认证声明调用插件方法：建会话载荷 `{qrImage: true, qrimg: true}`，检查和退出载荷 `{}` | 没有 fields 的非来源设置页只画设置区页头和「打开目录」，没有升级提示。建会话成功写「请扫码并在手机端确认，然后检查登录结果。」，仓库行写登录状态和缓存路径；二维码取 `qrUrl` 或 `qrurl` 重新编码，`data:` 图片和 credentialRef 不画。缺 `createSessionMethod` 写错误行「认证声明没有 createSessionMethod。」 | “连接新账号” / “已登录” / “扫码登录二维码” | 已测试（建会话和二维码另有离屏场景 `source-auth-methods`） |
| `PluginManagerPanel.vue` 数据目录 | 打开插件目录 | `get_plugin_data_directory`，没有替身；查到路径后由宿主定位 | 读取失败仍显示错误；成功排队 `OpenExternal` 并显示已打开 | “已打开“user.one”设置目录。” / “插件设置目录打开失败。” | 已测试（未离屏） |
| `PluginManagerPanel.vue` 依赖 | 依赖状态为空或有状态 | 无 | 状态列表为空时用 requires 和 optional 的数量 | “必需 2 / 可选 0” / “缺失” / “已启用” | 已测试（未离屏） |
| `logs.ts` `loadSystemLogsInWorkspace` | 切到日志面板，每次都读 | `list_system_logs`，limit 200，没有替身 | 读回整份换上，按时间再 id 降序；读的时候手上没有日志就写正在加载、不写空状态，「清空日志」照 Vue 可点；读失败保留手上的日志，状态区写「无法读取系统日志：…」 | “正在加载系统日志” | 已测试（`data_load_tests.rs`、`route_admin_tests.rs`，未离屏） |
| `WorkspaceLogsPanel.vue` 筛选 | 级别、来源、搜索、暂停 | 不把条件写进 `SystemLogQuery` | 客户端排序是时间再 id；筛选变空时不滚动；暂停后签名变化也不滚动；记录本身不删。实况页画出过滤后的时间、级别和消息 | “2 条缓存” / “2 条命中”；按钮“暂停追踪 / 恢复追踪”；空态“当前筛选没有命中” | 已测试（未离屏） |
| `TaskPopover.vue` | 任务和仓库操作 | 宿主线程每 250 ms 读 Mutsuki 任务快照（`task_watch.rs`），排队、运行和取消中的任务变了才发 `TaskProgressLoaded`；弹层没有取消按钮（Vue 也没有） | 合成行 id 是 `workspace-operation`，来源是资源库，所有行按更新时间降序；任务结束就从列表里拿掉；「任务」的计数是行数（仓库操作也算）；点弹层外的透明层关闭，Escape 走全局 Escape | “任务” / “当前没有运行中的任务。” | 已测试（`task_watch.rs`、`data_load_tests.rs`，未离屏） |
| `RepositoryActionsPanel.vue` 列表 | 启动和换仓库绑定侧栏时（Vue `queueRepositoryBackgroundLoads`）、结构更新、切到动作面板，或过期仓库 | `list_repository_actions`，没有替身 | 过期仓库忽略；当前仓库读失败保留旧列表；没有选中时用第一项；读回有动作时侧栏多出「动作」入口 | “正在加载动作” / “当前仓库没有导入动作。” | 已测试（`data_load_tests.rs`，未离屏） |
| `RepositoryActionsPanel.vue` 执行 | 点击执行 | `momobako.repository.action.run`，没有替身 | 需要 ready、启用、已选路径且不在执行中；选中动作同时切到动作面板；成功后刷新文件列表 | “不支持” / “执行” | 已测试（未离屏） |
| `ExtensionsPanel.vue` 工具页 | 工具页列表变化 | 文件导入和 Eagle 导入派发已有的 `OpenDialog` / `OpenEagle`，并回到文件页把对话框画出来；API Playground 的契约来自 API 设计快照，快照为空时用三个外部 API 兜底 | 三个内置 id 画原生页面。没有仓库、只读、回收站或虚拟视图时按钮禁用。其它工具页只画页头和一句说明 | “当前没有可用仓库。” / “多个路径用分号分隔” / “这个工具页由插件的前端组件绘制，Nana 原生界面不能运行它。” | 已测试；导入对话框由实况点击覆盖，第三方页另有离屏场景 `foreign-tool` |
| 设置页、拓展页、日志页和任务弹层 | 设置、插件、日志或任务 | 无 | 验收页走同一棵树；键是 `admin-settings`、`admin-extensions`、`admin-log-panel`、`clear-logs`、`admin-task-popover` | “音频播放” / “系统日志” / “扫描默认资源库” | 已离屏（`settings`、`plugin-settings`、`logs`、`task-running`） |

## Phase 7 宿主输入

这些行由 `src-nana/src/shell/input_tests.rs` 覆盖状态机。内部移动走已有的 `momobako.entry.move`，外部导入走 `momobako.entry.import`，空库附加走 `momobako.repository.attach`。这些调用没有替身测试。打开路径、打开网址和目录揭示记录带 `reveal` 的 `OpenExternal`，由 `host_bridge` 启动系统程序，不记访问；拖出文件记录 `DragOut`，Windows 上由宿主调用和 Tauri 相同的系统拖放，失败才写文案。关闭设置里的 `confirm` 总是先询问；`quit` 在壳层或元数据没有未保存修改时关闭进程，有修改时询问；`minimizeToTray` 记录托盘请求并隐藏窗口，不退出进程。保存和打开对话框排队为 `WindowCommand::OpenFileDialog`。主窗口位置、尺寸和最大化写到 `nana-main-window-state.json`，启动时恢复。关闭确认是浮层层里的 `ConfirmDialog`，排在最上层；验收场景里没有待确认的关闭。标了「已离屏」的行写明场景；离屏不替代真窗口验收。

实况文件列和空库面板挂上时用 `set_drop_target_node` 登记拖放目标（`input::accept_file_drops`），`FileDropEvent` 收成 `HostDrag`，由 `pointer_gesture.rs` 的 `live_file_drop_imports_and_empty_drop_attaches` 覆盖：有仓库时悬停再放下导入外部路径，空库放下附加第一条文件夹。拖放几何和放置决定仍由消息归约覆盖。卸载拖放只清标记，不清错误。

打开和定位交给系统程序，入口是文件右键菜单和预览页页头，网址来自元数据里的来源链接。拖出在 Windows 上调用系统文件拖放；最小化到托盘会隐藏窗口，托盘提示是 MomoBako，左键或双击显示窗口，菜单是「打开 MomoBako」和「退出」。`quit` 或确认关闭会关掉窗口并结束进程。这些系统效果仍要真窗口点过才算设备验收。IME 留在 [设备矩阵](./nana-device-matrix.md)。

| 来源 | 触发 | 服务 | 结果 | 可见性 | 状态 |
| --- | --- | --- | --- | --- | --- |
| `dragBehavior.ts` 路径 | 规范化或过滤移动路径 | 无 | 反斜杠转正斜杠并去掉首尾斜杠；父路径取最后一个斜杠之前；空白、放到自身、父目录已是目标的路径丢掉 | 无新文案 | 已测试（未离屏） |
| `dragBehavior.ts` 外部拖出 | 指针在窗口外且距离达到 72 | 无 | 窗口内或距离不够保持内部拖放；指针离开和失焦只看距离，不要求仍在窗外 | 无新文案 | 已测试（未离屏） |
| `useWorkspaceDragDrop.ts` 起点 | 按下一条条目 | 无 | 不可写、回收站、智能文件夹或后端不是 filesystem 时不开始；未选中的条目先变成单独选择 | 选择变成该路径 | 已测试（未离屏） |
| `useWorkspaceDragDrop.ts` 放下 | 内部拖放结束，或放到文件夹 | `momobako.entry.move`，没有替身 | 没有目标、已经交给拖出、回收站，或过滤后没有路径时不移动 | 无新文案 | 已测试（未离屏） |
| `useWorkspaceDragDrop.ts` 导入 | 外部文件放到文件区或文件夹 | `momobako.entry.import`，没有替身 | 文件区会丢掉落到自身绝对路径上的文件；文件夹放下不做这层过滤；不可写、回收站、没有快照或空白路径不导入 | 无新文案 | 已测试（未离屏） |
| `useWorkspaceDragDrop.ts` 悬停 | dragover、dragleave 或文件夹悬停 | 无 | 不可写或不在文件面板时不改放置效果；内部是 move，外部是 copy；嵌套离开和内部拖放离开不清除外部状态；回收站忽略文件夹悬停 | 放置效果 | 已测试（未离屏） |
| `useWorkspaceDragDrop.ts` 空库 | 拖入一个文件夹 | `momobako.repository.attach`，没有替身 | 已有活动 id 或已有仓库时忽略；只用第一条非空路径；失败写入空库错误，没有进行中的附加时忽略后续结果 | 空库错误文本 | 已测试（未离屏） |
| `useWorkspaceDragDrop.ts` 宿主拖放 | enter、over、leave 或 drop | 附加或导入，没有替身 | 没有仓库且不是丢失仓库时按空库附加；有仓库时要可写并且在文件面板，离开会清悬停 | 文件区和空库页挂上时登记的拖放目标 | 已测试（未离屏） |
| `useWorkspaceDragDrop.ts` 框选 | 追加或替换 | 无 | 追加时空列表不变并按原顺序并上新路径；替换空列表清空；主选和锚点用已有主选，否则用第一项 | 选择路径 | 已测试（未离屏） |
| `core.ts` 打开和揭示 | 打开条目、网址或在目录中定位 | 宿主启动系统程序，失败才写文案 | 没有仓库或路径为空则不动；否则记录带 `reveal` 的 `OpenExternal` | 成功不写失败文案；失败时状态区写“打开失败：{错误}” / “定位失败：{错误}” | 已离屏（`status-error`，只走定位失败） |
| `core.ts` 拖出 | 把选中路径拖出窗口 | Windows 上 `drag::start_drag`，复制效果 | 回收站、非 filesystem 或没有绝对路径时失败且不记请求；其余记录 `DragOut`，系统调用失败才写文案 | 成功不写失败文案；失败为“拖出失败：…” | 已测试（未离屏） |
| 关闭行为 | 标题栏关闭或系统 `CloseRequested` | 托盘隐藏窗口；退出关闭窗口并结束进程 | `confirm` 总是询问；`quit` 在没有脏数据时关闭；壳层未保存或元数据草稿未保存时询问；托盘不退出进程；取消不关闭；已经在问时再关不重复问；确认框只由 `pending_close` 驱动，不进宿主请求队列 | “确认关闭 MomoBako？” / “有未保存的修改，确认关闭？” / 托盘失败才写“最小化到托盘失败：…” | 已测试；`live_visual` 点「关闭」验证默认关闭不弹确认，确认框由 `overlay_dialog_tests.rs` 覆盖，反复确认后宿主请求队列为空由 `host_bridge.rs` 的测试覆盖；脏数据、托盘点击和真窗口未离屏 |
| 主窗口几何 | 移动、缩放、最大化或再次启动 | 写入 `nana-main-window-state.json` | 宽小于 960 或高小于 600 不恢复；最大化时保留上一帧正常位置和尺寸；最小化原点不写入 | 无新文案 | 已测试（未离屏）；真窗口恢复待设备矩阵 |
| 文件对话框 | 导出外部连接或选择插件包 | `OpenFileDialog`；写出和安装没有替身 | 取消不写文件也不安装；失败记下错误；未知编号忽略 | “导出失败：{原因}” / “插件包选择失败：{原因}” | 已测试（未离屏） |
| 关闭确认 | 有待确认的关闭 | 无 | 浮层层里的 `ConfirmDialog`，排在最上层；对话框键 `close-confirm`，按钮键 `close-confirm-cancel`、`close-confirm-accept` | 标题“关闭 MomoBako”，按钮“取消”“确认关闭”；文案“确认关闭 MomoBako？”或“有未保存的修改，确认关闭？” | 已测试（未离屏） |

## 宿主事件

这些行由 `src-nana/src/shell/host_events.rs` 和 `workspace_refresh.rs` 覆盖，硬链接候选的静默刷新在 `files_tests.rs`。日志和结构更新从 `host_event_channel` 进入壳层。实时日志按 id 合并，时间再 id 降序，最多 500 条；暂停时不标记滚动。启动仍在加载且分类是 `repository.sync` 时，才把消息追加到启动日志；记录里的仓库和当前仓库都非空且不同则忽略。结构更新只在启动完成、仓库匹配且不是缺失仓库时刷新：目录树、智能文件夹、播放集、仓库动作、文件或回收站的静默目录重读，以及静默的仓库列表、摘要和硬链接候选。播放集和智能文件夹面板改为重读当前项。分类视图不按目录重读。静默重读失败只记日志，不把整页改成错误，也不弹出硬链接对话框。

启动未完成或仓库已经缺失时仍忽略整次结构更新，不重置内容。这批路径的状态机测试已经覆盖，离屏场景不替代设备矩阵。

| 来源 | 触发 | 服务 | 结果 | 可见性 | 状态 |
| --- | --- | --- | --- | --- | --- |
| `logs.ts` `mergeSystemLogs` | `system://log-recorded` | 日志器 `write` 后的事件，没有替身 | 相同 id 替换；降序后保留前 500 条；暂停不滚动 | 日志列表 | 已测试（未离屏） |
| `lifecycle.ts` `appendStartupSyncLog` | 启动加载中的 `repository.sync` | 无新请求 | 其它分类不进入启动日志；仓库不一致则忽略；启动结束后不再追加 | 启动日志 | 已测试（未离屏） |
| `lifecycle.ts` `handleRepositoryStructureUpdated` | `repository://structure-updated` | 目录树、智能文件夹、播放集、仓库动作、当前目录浏览，没有替身 | 别的仓库、启动未完成或缺失仓库不刷新；目录树正在读取时不重复请求树；文件和回收站静默重读并保留选择 | 当前列表 | 已测试（未离屏） |
| 静默目录失败 | 重读返回错误 | 无 | 保留原列表和当前页 | 页面不变 | 已测试（未离屏） |
| `refresh.ts` `refreshRepositorySummaries` | 就绪后的结构更新 | `list_repositories`，没有替身 | 替换列表；失败或启动中不写；不改活动 id、页面和启动步骤；当前仓库缺失或不在列表中时保留内容和主区 | 仓库名称；页面不变 | 已测试（未离屏） |
| `refresh.ts` `refreshRepositorySnapshot` | 就绪且仓库匹配 | `get_repository_snapshot`，没有替身 | 更新名称、详情和快捷计数；不重新绑定目录树；失败、仓库不一致或缺失时不改页面 | 仓库名称和侧栏快捷计数 | 已测试（未离屏） |
| `refresh.ts` `refreshHardlinkCandidates` | 就绪后的结构更新 | `list_hardlink_candidates`，没有替身 | 替换候选；不打开对话框，不清已跳过项；失败不写页面错误 | 页面不变 | 已测试（未离屏） |

原生窗口用 `cargo run -p momobako-nana` 启动，Vue/Tauri 对照窗口用 `yarn tauri:dev`。离屏通过不勾掉拖放、IME、托盘和真窗口行；那些行留在 [设备矩阵](./nana-device-matrix.md)。
