# Nana 逻辑迁移矩阵

静态截图不能当作迁移完成。每一行从 Vue 或 composable 的分支读出，不从现有离屏场景反推。尚未读过的来源保持“未读”，不预填分支。

记账列是：来源、触发、服务、结果、可见性、状态。状态依次为未读、已记账、已测试、已离屏。服务行要有真实调用测试；不改变控件树的分支也要有 ViewModel 测试；改变控件树的分支才增加离屏场景。

工作台面板来自 `src/composables/workspace/state.ts`：

| 枚举 | 取值 |
| --- | --- |
| `WorkspacePanelKey` | files、trash、search、smartFolder、playlist、actions、extensions、logs |
| `WorkspaceLibraryCategoryKey` | all、uncategorized、untagged、recent |
| 文件显示 | adaptive、masonry、grid、list |
| 筛选 | `WorkspaceFilterState` 的标签、格式、颜色、形状、排除条件、元数据、数值、日期、AND/OR、排序、条数、最低评分 |

这些面板和加载、错误、缺失仓库、对话框正交。现有扁平 `ShellPage` 只保留到对应逻辑行有了新场景为止。

## Phase 1 壳层

这些行由 `src-nana/src/shell/workspace_tests.rs` 覆盖状态机和副作用枚举，状态是已测试。宿主在归约后把副作用交给领域服务：列表用 `list_repositories`，非缺失仓库接着 `PROTOCOL_REPOSITORY_SYNC`、`get_repository_snapshot` 和根目录 `get_file_browser`；重定向用 `PROTOCOL_REPOSITORY_RELOCATE`，删除用 `RepositoryManagementViewModel::delete_repository`，来源设置复用系统状态和应用设置加载。协议调用没有替身测试。下面每一行都未离屏，15 个旧验收场景仍走 `acceptance_scene`。

未完成，不能记成已测试：分隔条拖动结束没有写回宽度；系统文件夹对话框未接，重定向使用页内路径框；启动同步日志监听未移植；拖放导入属于 Phase 7；停止播放目前只记录日志，播放器还没有 `stop()`。

| 来源 | 触发 | 服务 | 结果 | 可见性 | 状态 |
| --- | --- | --- | --- | --- | --- |
| `lifecycle.ts` `ensureRepositoryWorkspace` | 首次进入且启动空闲 | `RefreshRepositories` → `list_repositories` | 从第 1 步重新开始，进度按当前步 / 4 | 启动面板；侧栏在就绪前隐藏 | 已测试（未离屏） |
| `lifecycle.ts` `setWorkspaceStartupProgress` / `failWorkspaceStartup` | 中间步骤失败 | 无新请求 | 当前步记失败，更早的步骤保持完成，百分比保留 | 启动面板显示失败并给出重试 | 已测试（未离屏） |
| `lifecycle.ts` 重试 | 启动已失败 | 新代次的 `RefreshRepositories` | 已就绪时直接返回；否则整段回到第 1 步，旧步骤不再保持完成 | 进行中的步骤条替换失败态 | 已测试（未离屏） |
| `lifecycle.ts` `loadInitialRepository` | 列表为空 | 不同步 | 清空活动仓库和记住的 id，结束启动 | “还没有可用资源库” / “拖入一个本地文件夹创建资源库。” | 已测试（未离屏） |
| `lifecycle.ts` `loadInitialRepository` | 选中仓库 `status == missing` | 不同步，立即记住 id | 结束启动，主区为缺失 | 缺失面板；不进入文件列表 | 已测试（未离屏） |
| `lifecycle.ts` `loadInitialRepository` | 选中仓库可同步 | `SyncRepository` | 选择顺序是当前 id、记住的 id、第一项；同步成功后才记住 | 第 2 步“扫描资源库文件” | 已测试（未离屏） |
| `lifecycle.ts` 同步结果 | 代次不匹配，或启动已不是加载中 | 丢弃 | 不改已完成步骤和活动仓库 | 保持当前步骤 | 已测试（未离屏） |
| `lifecycle.ts` 同步成功 | 代次匹配 | `LoadSnapshot` → `get_repository_snapshot` | 进入第 3 步；没有活动仓库则失败 | 第 3 步“读取仓库摘要” | 已测试（未离屏） |
| `lifecycle.ts` 摘要与首屏 | 活动仓库匹配且步骤已到 3 / 4 | 摘要成功后根目录浏览 | 成功则结束启动并进入有仓库；失败则加载错误 | 第 4 步“读取首屏目录”，或错误面板 | 已测试（未离屏） |
| `lifecycle.ts` 列表失败 | 启动尚未就绪 | 无 | 启动失败，主区为加载错误；已就绪时只记下缺失错误 | 错误面板，已完成步骤保留 | 已测试（未离屏） |
| `useWorkspaceHomeViewModel.ts` | 启动结束 | 无 | 缺失、有仓库、空仓库、加载错误四者互斥；错误优先于另外三条 | 同一时刻只显示一个主区 | 已测试（未离屏） |
| `AppShell.vue` `watch(activeRepoId)` | 上一个 id 存在且变为另一个 id | `StopPlayback` | 相同 id 不停止播放，也不清缺失错误 | 切换后的仓库主区 | 已测试（未离屏） |
| `AppShell.vue` 侧栏 | 折叠，或宽度在松手时提交 | `PersistSidebar` → `sidebar.json` | 宽度夹在 220–480，默认 276；只有存储值 `"1"` 才折叠；拖动中只夹取 | 折叠隐藏第一栏；键名沿用 `momobako.sidebarCollapsed` / `momobako.sidebarWidth` | 已测试（未离屏） |
| `useMissingRepositoryActions.ts` 刷新 | 缺失仓库且不忙 | `RefreshRepositories` | 忙（重定向或删除中）时忽略 | “刷新”不可再次提交 | 已测试（未离屏） |
| `useMissingRepositoryActions.ts` 选择路径 | 有仓库且不忙 | 无 | 打开页内路径框并清错误；空白提交不重定向 | “重定向”或忙时“重定向中...” | 已测试（未离屏） |
| `useMissingRepositoryActions.ts` 提交路径 | 路径去空白后非空 | `RelocateRepository` | 成功后刷新列表；失败写入缺失错误并解除忙状态 | “确认重定向” | 已测试（未离屏） |
| `MissingRepositoryState.vue` 来源缓存 | `localCache.required` 且缓存不是 ready | `OpenSourceSettings` | 不打开路径框 | 主按钮为“打开来源设置” | 已测试（未离屏） |
| `RepositoryDeleteDialog.vue` | 打开、取消、确认 | `DeleteRepository` | 缺失的本地仓库只能“只删除记录”；删除中不能关闭或再次确认；没有删除标记的成功结果不关对话框；成功后移除记录并选剩余第一项或空仓库 | “删除资源库” / “删除中...”；三个删除方式 | 已测试（未离屏） |
| `state.ts` 面板与分类 | 启动进度推进 | 无 | 面板保留；空列表把分类重置为全部 | 面板枚举不随步骤丢失 | 已测试（未离屏） |

## Phase 2 侧栏

这些行由 `src-nana/src/shell/sidebar_tests.rs` 和 `stale_playlist_detail_keeps_the_bound_repository_screen` 覆盖状态机。宿主在归约后把副作用交给领域服务：目录树用 `FileBrowserViewModel::get_repository_tree`，智能文件夹用 `list_smart_folders` 和 `query_smart_folder`，播放集用 `list_playlists`、`list_playlist_players` 和 `get_playlist_detail`，目录浏览用 `get_file_browser`，附加本地文件夹用 `PROTOCOL_REPOSITORY_ATTACH`。协议调用没有替身测试。实况侧栏在启动就绪后用 `SidebarRow` 和 `TreeView`；15 个旧验收场景仍走 `acceptance_scene`。下面每一行都未离屏。同一仓库且缺失标记不变时，再次绑定不重复请求目录树。

快捷方式计数在摘要到达时立即计算，没有移植 Vue 的 200ms idle 调度。文件快捷访问会先把 `\` 收成 `/`，再打开父目录并选中该路径。

未完成，不能记成已测试：文件夹拖放悬停、创建、重命名和删除对话框；智能文件夹筛选表单的创建、编辑和删除；播放集的创建、播放和移除完整流程（播放集页面上已有的创建不在本阶段）；系统文件夹对话框、Eagle 和云盘添加表单；弹层坐标夹取；侧栏同步进度旋转；全局 Escape。缺失仓库时播放集行整段隐藏，选择函数本身只检查仓库 id。

| 来源 | 触发 | 服务 | 结果 | 可见性 | 状态 |
| --- | --- | --- | --- | --- | --- |
| `useSidebarShortcutsUi.ts` 计数 | 仓库摘要 | 无新请求 | 已删除素材不进入全部、未分类、未标签、最近使用；路径不含 `/` 为未分类；标签为空为未标签；有 `lastAccessedAt` 为最近使用；回收站用 `overview.trashCount` | “全部 · n”等五行 | 已测试（未离屏） |
| `useSidebarShortcutsUi.ts` `selectShortcut` | 资源库缺失 | 无 | 快捷方式、快捷访问、打开文件夹和打开智能文件夹直接返回 | 这些行禁用 | 已测试（未离屏） |
| `useSidebarShortcutsUi.ts` `selectShortcut` | 回收站，且未锁定 | `get_file_browser`，`special_location = trash` | 面板改为 trash；离开设置页 | “回收站” | 已测试（未离屏） |
| `useSidebarShortcutsUi.ts` `selectShortcut` | 其余分类，且未锁定 | 若当前在回收站，则根目录浏览且不带 trash | 面板改为 files 并写入对应分类；离开设置页 | 分类名 | 已测试（未离屏） |
| `useSidebarShortcutsUi.ts` `openQuickAccess` | `targetKind == smartFolder` 且有 id | `query_smart_folder` | 进入智能文件夹；没有 id 或找不到条目则返回 | 智能文件夹面板 | 已测试（未离屏） |
| `useSidebarShortcutsUi.ts` `openQuickAccess` | `targetKind == file` 且有路径 | 浏览父目录 | 分类为全部；选中规范化后的文件路径；空路径返回 | 父目录 | 已测试（未离屏） |
| `useSidebarShortcutsUi.ts` `openQuickAccess` | 其它非空 `targetPath` | 浏览该目录 | 分类为全部 | 该目录 | 已测试（未离屏） |
| `useFolderSidebarUi.ts` 打开与展开 | 点击目录或当前目录变化 | `get_file_browser` | 面板 files、分类全部；按 `/` 逐段展开，空路径不展开；回收站浏览不展开目录 | 目录树选中当前路径 | 已测试（未离屏） |
| `useFolderSidebarUi.ts` 树替换 | 新树到达 | 无 | 仓库不匹配则忽略；否则丢掉树里已经不存在的展开路径，空路径始终有效 | 保留仍存在的展开节点 | 已测试（未离屏） |
| `useFolderSidebarUi.ts` 刷新 | 未锁定、有仓库、且不在加载中 | `get_repository_tree` | 重复刷新被忽略 | “正在刷新文件夹树”不可再次提交 | 已测试（未离屏） |
| `WorkspaceSidebar*.vue` 文件夹空态 | 无仓库、缺失、回收站或空树 | 无 | 不画创建、重命名、删除或拖放按钮 | “先选择或添加一个资源库。” / “资源库文件夹丢失，请先在主视图修复。” / “回收站条目在主视图中管理。” / “当前仓库还没有子文件夹。” | 已测试（未离屏） |
| `useSmartFolderSidebarUi.ts` 选择 | 有仓库、id 非空、且未锁定 | `query_smart_folder` | 面板 smartFolder，展开祖先但不展开自己；空 id 返回 | “正在查询智能文件夹” | 已测试（未离屏） |
| `useSmartFolderSidebarUi.ts` 查询结果 | 仓库或当前 id 已变化 | 丢弃 | 返回 false，不改详情；匹配的错误只写入智能文件夹错误，不把整页改成 Error | 错误留在侧栏 | 已测试（未离屏） |
| `useSmartFolderSidebarUi.ts` 树替换 | 列表到达 | `list_smart_folders` | 仓库不匹配则忽略；先剪掉无效展开，再展开当前项的祖先 | “还没有智能文件夹。”或树 | 已测试（未离屏） |
| `usePlaylistSidebarUi.ts` 选择 | 有仓库 id 且 id 非空 | `get_playlist_detail` | 面板 playlist；播放集从列表消失时清空当前项，若面板仍是 playlist 则回到 files | 折叠时只显示名称；展开时显示名称、播放器标签和项数 | 已测试（未离屏） |
| `usePlaylistSidebarUi.ts` 可播放 | 播放器类型已加载 | `list_playlist_players` | `player_type_id` 在已加载类型中才可播放；本阶段不画播放按钮 | 无播放按钮 | 已测试（未离屏） |
| `PlaylistDetailLoaded` | 侧栏已绑定的仓库与详情仓库不同 | 丢弃 | 未绑定仓库时仍应用详情，避免旧页面测试失效 | 保持当前页 | 已测试（未离屏） |
| `useRepositorySwitcherUi.ts` 打开与选择 | 点击仓库名 | `select_repository` 后重新绑定侧栏 | 已经是切换列表或正在提交时忽略；选择后关闭弹层并离开设置页 | “资源库 · 名称” | 已测试（未离屏） |
| `useRepositorySwitcherUi.ts` 删除 | 有活动仓库且未提交 | 打开已有删除对话框 | 没有活动仓库或正在提交时忽略 | “删除当前资源库” | 已测试（未离屏） |
| `useRepositorySwitcherUi.ts` 附加 | 路径去空白后非空 | `PROTOCOL_REPOSITORY_ATTACH` | 空白不提交；提交中不能关闭、改选或再打开添加菜单；成功后关闭并刷新列表；失败回到添加菜单并记下错误 | “附加资源库” / “正在附加…” | 已测试（未离屏） |
| `useWorkspaceSidebarShellUi.ts` 缺失 | 活动仓库 `status == missing` | 不请求树、智能文件夹或播放集 | 快捷方式等导航锁定；仓库切换弹层本身不锁定 | 播放集显示“资源库修复后可继续使用播放集。” | 已测试（未离屏） |

## Phase 3 文件浏览与变更

这些行由 `src-nana/src/shell/files_tests.rs` 覆盖状态机。宿主在归约后把副作用交给领域服务：浏览、新建、重命名和回收站用 `FileBrowserViewModel` 的 `get_file_browser`、`create_directory`、`create_file`、`rename_entry`、`mutate_trash`；硬链接用 `RepositoryInteractionViewModel` 的 `list_hardlink_candidates` 和 `confirm_hardlink_candidate`。复制、移动、导入、压缩包、Eagle 和删除走 Mutsuki 协议 `PROTOCOL_ENTRY_COPY`、`PROTOCOL_ENTRY_MOVE`、`PROTOCOL_ENTRY_IMPORT`、`PROTOCOL_ARCHIVE_IMPORT`、`PROTOCOL_EAGLE_IMPORT`、`PROTOCOL_ENTRY_DELETE`，协议调用没有替身测试。复制模式固定为 `hardlinkPreferred`。实况文件表面在启动就绪、主区有仓库、且面板是文件、回收站或智能文件夹时用 `each_virtual`；15 个旧验收场景仍走 `acceptance_scene` 的 `file_actions`。下面每一行都未离屏。

文件表面自己发起的首页是 80 条、继续加载是 160 条。侧栏和启动的 `OpenDirectory` 仍是 200 条。自适应、瀑布流和网格共用同一套网格，没有单独的瀑布流排布。缩略图有路径时用 Nana `Thumbnail::new`，没有路径时用空缩略图。

未完成，不能记成已测试：系统文件夹对话框；拖放；框选；修饰键手势（选择方式改成页内的“替换 / 切换 / 范围”）；元数据编辑器和预览属于 Phase 4；缩略图空闲预取；操作进度条（进行中只显示“正在复制…”这类短句）；在系统里显示或打开。

| 来源 | 触发 | 服务 | 结果 | 可见性 | 状态 |
| --- | --- | --- | --- | --- | --- |
| `files.ts` 分页 | 首屏或继续加载 | `get_file_browser` | 追加按 `kind:path` 去重；`has_more` 为合并后条数小于 `total_entries`；虚拟视图不再分页 | “加载更多”在没有更多、加载中或虚拟视图时禁用 | 已测试（未离屏） |
| `files.ts` 过期列表 | 快照的仓库、路径或回收站与 pending 不同 | 丢弃 | 保留原列表，清掉加载标记 | 仍是原来的条目 | 已测试（未离屏） |
| `files.ts` 读取失败 | 文件表面自己处于加载中 | 无新请求 | 写下错误，不替换列表；启动阶段的失败仍交给壳层切到错误页 | 错误留在文件列 | 已测试（未离屏） |
| `fileOperations.ts` 可写 | 新建、导入、复制、移动、重命名、删除 | 无 | 需要活动仓库、状态不是 missing、能力含 `write`；变更进行中全部拒绝 | 对应按钮禁用 | 已测试（未离屏） |
| `fileOperations.ts` 虚拟视图 | 智能文件夹或分类视图 | 无 | 智能文件夹拒绝新建、导入、重命名、删除，行来自查询结果；分类视图隐藏文件夹，允许重命名和删除，拒绝新建、导入和用户发起的目录浏览 | “只读智能文件夹，不能在这里新建、重命名或删除。” / “分类视图” | 已测试（未离屏） |
| `fileOperations.ts` `is_virtual` | 选中行带虚拟标记 | 无 | 复制、移动、重命名、删除和还原都拒绝 | 这些按钮禁用 | 已测试（未离屏） |
| `files.ts` 选择 | 替换、切换、范围 | 无 | 没有锚点或锚点已不在列表时，范围退回替换；快照丢掉已经不存在的路径，主选不在则取剩余第一项 | “选择 · 替换 / 切换 / 范围” | 已测试（未离屏） |
| `files.ts` 打开条目 | 替换模式下点击文件夹 | `get_file_browser` | 进入该目录并清空选择；切换或范围只选中文件夹，不进入 | 面包屑“根目录”和路径段 | 已测试（未离屏） |
| `files.ts` 打开文件 | 选中带 `asset_id` 的文件 | `get_asset_detail` | 只记读取素材的副作用，不在本阶段画预览 | 选中该文件 | 已测试（未离屏） |
| `fileOperations.ts` 空白提交 | 名称去空白后为空，或导入来源拆行和分号后为空 | 无 | 不进入变更中，不发请求 | 对话框保持打开 | 已测试（未离屏） |
| `fileOperations.ts` 新建和重命名 | 名称非空 | `create_directory` / `create_file` / `rename_entry` | 成功只采用返回快照里的名字；分类视图路径不一致时保留原列表并关闭对话框；失败保留原列表和对话框 | “正在新建文件夹…” / “正在新建文件…” / “正在重命名…” | 已测试（未离屏） |
| `CopyTargetDialog.vue` 复制和移动 | 已有选择，目标路径由页内输入 | `PROTOCOL_ENTRY_COPY` / `PROTOCOL_ENTRY_MOVE` | 复制父目录为空表示根；移动父目录用空字符串表示根；路径里的 `\` 收成 `/` | “正在复制…” / “正在移动…” | 已测试（未离屏） |
| `fileOperations.ts` 协议结果 | 复制、移动、导入、压缩包、Eagle 或删除返回 | 成功后重新 `get_file_browser` | 成功不改当前行，保留选择并允许分类视图重读；智能文件夹不按目录重读；复制成功再列硬链接候选；失败保留原列表和对话框 | 对话框在成功后关闭 | 已测试（未离屏） |
| `fileOperations.ts` 删除 | 有选择且可写 | `PROTOCOL_ENTRY_DELETE` | 回收站 mode 为 `permanentDelete`，普通删除 mode 为空；失败保留原列表和对话框 | “正在删除…” / “正在永久删除…” | 已测试（未离屏） |
| `fileOperations.ts` 回收站 | 还原、还原全部、清空 | `mutate_trash` | 还原只在回收站；清空和还原全部不要求当前选择 | “正在还原…” / “正在清空回收站…” | 已测试（未离屏） |
| `HardlinkCandidateDialog.vue` | 候选到达、跳过、确认 | 确认用 `confirm_hardlink_candidate`；跳过无请求 | 跳过只记本地 id 并显示下一条；确认失败保留该条；确认成功按 id 移除，没有剩余则关闭 | “确认后会将新文件加入硬链接关联。”；跳过 / 加入关联 | 已测试（未离屏） |
| `files.ts` 展示方式 | 切换或读取 `file-display.json` | `PersistDisplayMode` | 值是 adaptive、masonry、grid、list，也接受中文标签；未知、损坏或文件缺失回到自适应；只在切换时写入 | “自适应 / 瀑布流 / 网格 / 列表” | 已测试（未离屏） |
| `files.ts` 重命名草稿 | 选择变成多项，或主选变化 | 无 | 清掉重命名路径和草稿，并关闭重命名框 | 重命名框消失 | 已测试（未离屏） |
| Eagle 导入 | 模式为 copy 或 move，库路径非空 | `PROTOCOL_EAGLE_IMPORT` | 其他模式不打开对话框；空白路径不提交 | “正在导入 Eagle…” | 已测试（未离屏） |

## Phase 4 预览、元数据和搜索

| 来源 | 状态 |
| --- | --- |
| `src/pages/workspace/preview/` | 未读 |
| `src/pages/workspace/files/FileMetadataEditor.vue` | 未读 |
| `src/pages/workspace/search/` | 未读 |
| `src/pages/workspace/SearchPanel.vue` | 未读 |
| `src/pages/workspace/search/WorkspaceFilterBar.vue` | 未读 |

## Phase 5 播放列表

| 来源 | 状态 |
| --- | --- |
| `src/pages/workspace/playlists/` | 未读 |
| `src/components/WorkspacePlayerBar.vue` | 未读 |
| `src/composables/usePlaylistPlayer.ts` | 未读 |
| `src/composables/useSystemMediaSession.ts` | 未读 |

## Phase 6 设置、插件、日志和任务

| 来源 | 状态 |
| --- | --- |
| `src/pages/Settings.vue` | 未读 |
| `src/components/PluginManagerPanel.vue` | 未读 |
| `src/pages/workspace/ExtensionsPanel.vue` | 未读 |
| `src/pages/workspace/WorkspaceLogsPanel.vue` | 未读 |
| `src/components/TaskPopover.vue` | 未读 |
| `src/pages/workspace/repository/RepositoryActionsPanel.vue` | 未读 |

## Phase 7 宿主输入

| 来源 | 状态 |
| --- | --- |
| `src/services/repositoryApi/core.ts` | 未读 |
| `src/pages/workspace/useWorkspaceDragDrop.ts` | 未读 |
| `src/pages/workspace/dragBehavior.ts` | 未读 |
| `src/layouts/AppShell.vue` 的关闭确认 | 未读 |

Phase 8 在上表没有未读和未测试的产品分支之后，才把默认启动改到 `momobako-nana`。离屏通过不勾掉拖放、IME、托盘和真窗口行；那些行留在 `nana-device-matrix.md`。
