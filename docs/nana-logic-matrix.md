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

| 来源 | 状态 |
| --- | --- |
| `src/layouts/useSidebarShortcutsUi.ts` | 未读 |
| `src/layouts/useFolderSidebarUi.ts` | 未读 |
| `src/layouts/useSmartFolderSidebarUi.ts` | 未读 |
| `src/layouts/usePlaylistSidebarUi.ts` | 未读 |
| `src/layouts/useRepositorySwitcherUi.ts` | 未读 |
| `src/layouts/WorkspaceSidebar*.vue` | 未读 |

## Phase 3 文件浏览与变更

| 来源 | 状态 |
| --- | --- |
| `src/pages/workspace/WorkspaceFilesSurface.vue` | 未读 |
| `src/pages/workspace/files/` | 未读 |
| `src/composables/workspace/files.ts` | 未读 |
| `src/composables/workspace/fileOperations.ts` | 未读 |
| `src/pages/workspace/CopyTargetDialog.vue` | 未读 |
| `src/pages/workspace/HardlinkCandidateDialog.vue` | 未读 |

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
