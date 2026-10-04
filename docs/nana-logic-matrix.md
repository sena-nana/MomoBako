# Nana 逻辑迁移矩阵

静态截图不能当作迁移完成。每一行从 Vue 或 composable 的分支读出，不从现有离屏场景反推。本文件的初始行都是“未读”：只登记来源文件和面板枚举，不预填还没读过的分支。

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

| 来源 | 状态 |
| --- | --- |
| `src/layouts/AppShell.vue` | 未读 |
| `src/pages/workspace/repository/useWorkspaceHomeViewModel.ts` | 未读 |
| `src/composables/workspace/lifecycle.ts` | 未读 |
| `src/pages/workspace/MissingRepositoryState.vue` | 未读 |
| `src/pages/workspace/EmptyRepositoryState.vue` | 未读 |

已知但尚未记账的入口：启动四步、缺失仓库、空仓库、加载错误、切换仓库时停止播放、侧栏折叠和宽度、缺失仓库的刷新、改路径和删除。

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
