//! 侧栏导航状态。
//!
//! 管快捷方式、文件夹、智能文件夹、播放集和仓库切换的导航分支。
//! 新建文件夹、智能文件夹筛选表单、播放集播放和系统文件夹对话框不在这里完成。

use super::workspace::{LibraryCategory, WorkspacePanel, WorkspaceState};

mod collections;

mod reduce;

pub use reduce::*;

/// 快捷方式。回收站进入 trash 浏览，其余切换库分类。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShortcutId {
    All,
    Uncategorized,
    Untagged,
    Recent,
    Trash,
}

impl ShortcutId {
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "全部",
            Self::Uncategorized => "未分类",
            Self::Untagged => "未标签",
            Self::Recent => "最近使用",
            Self::Trash => "回收站",
        }
    }
}

/// 计算快捷方式计数时保留的素材字段。已删除素材不进入前四项。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShortcutAsset {
    pub path: String,
    pub untagged: bool,
    pub accessed: bool,
    pub deleted: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ShortcutCounts {
    pub all: usize,
    pub uncategorized: usize,
    pub untagged: usize,
    pub recent: usize,
    pub trash: usize,
}

/// 仓库摘要里的快捷访问。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SidebarShortcut {
    pub id: String,
    pub label: String,
    pub target_kind: String,
    pub target_path: Option<String>,
    pub target_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SidebarFolder {
    pub path: String,
    pub label: String,
    pub children: Vec<SidebarFolder>,
}

#[derive(Clone, Debug)]
pub struct SidebarSmartFolder {
    pub id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub filter: crate::backend::services::repository::SmartFolderFilter,
    pub children: Vec<SidebarSmartFolder>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SidebarPlaylist {
    pub id: String,
    pub name: String,
    pub player_label: String,
    pub player_type_id: String,
    pub item_count: i64,
}

/// 仓库切换弹层。提交附加文件夹时不能关闭或改选。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PopoverMode {
    #[default]
    Closed,
    Switcher,
    AddMenu,
    /// Eagle 或云盘这类需要表单的后端。
    BackendForm,
}

mod bind;
mod gap;
mod popover;
mod smart;
mod smart_flow;

pub use gap::{
    FOLDER_DELETE_TITLE, FolderDeleteMode, FolderMenu, FolderMutation, GapMessage, escape_layer,
};
pub use popover::{BackendOption, BackendRoute, backend_options};
pub use smart::{SmartFolderDraft, SmartFolderField};
pub use smart_flow::{
    SmartFolderChange, SmartFolderOutcome, SmartFolderResult, SmartResultSummary,
};

/// 一次目录树读取：树本身和每个目录的直属文件数。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SidebarTree {
    pub folders: Vec<SidebarFolder>,
    pub counts: std::collections::BTreeMap<String, usize>,
}

impl From<Vec<SidebarFolder>> for SidebarTree {
    fn from(folders: Vec<SidebarFolder>) -> Self {
        Self {
            folders,
            counts: Default::default(),
        }
    }
}

impl SidebarTree {
    /// 把服务返回的目录节点收成侧栏树，同时记下每个目录的文件数。
    pub fn from_nodes(nodes: &[crate::backend::services::repository::FileTreeNode]) -> Self {
        fn walk(
            nodes: &[crate::backend::services::repository::FileTreeNode],
            counts: &mut std::collections::BTreeMap<String, usize>,
        ) {
            for node in nodes {
                counts.insert(node.path.clone(), node.file_count);
                walk(&node.children, counts);
            }
        }
        let mut counts = std::collections::BTreeMap::new();
        walk(nodes, &mut counts);
        Self {
            folders: nodes.iter().map(SidebarFolder::from_file_node).collect(),
            counts,
        }
    }
}

/// 侧栏交互。壳层只保留一个 `ShellMessage::Sidebar`，避免主归约重复列出这些分支。
pub enum SidebarMessage {
    SelectShortcut(ShortcutId),
    OpenQuickAccess(String),
    ToggleFolder(String),
    OpenFolder(String),
    RefreshFolderTree,
    ToggleSmartFolder(String),
    OpenSmartFolder(String),
    OpenSmartFolderDialog,
    /// 智能文件夹行上的「新建子智能文件夹」，带父级 id。
    OpenSmartFolderChild(String),
    CloseSmartFolderDialog,
    SetSmartFolderField {
        field: SmartFolderField,
        value: String,
    },
    SubmitSmartFolder,
    /// 新建、编辑或删除智能文件夹的结果：成功时是整棵树和这次改的是哪个。
    SmartFolderSaved {
        repo_id: String,
        result: Result<SmartFolderOutcome, String>,
    },
    TogglePlaylists,
    OpenSidebarPlaylist(String),
    OpenRepositorySwitcher,
    ShowRepositoryAddMenu,
    /// 添加菜单里选了一个来源后端（插件 id）。
    SelectRepositoryBackend(String),
    /// 后端表单的「返回」。
    BackToAddMenu,
    CloseRepositoryPopover,
    SelectRepositoryFromSwitcher(String),
    DeleteRepositoryFromSwitcher,
    RepositoryAttachPathChanged(String),
    SubmitRepositoryAttach,
    /// 附加或新建资源库的结果：成功时是新仓库的摘要，放进列表并切过去。
    RepositoryAttachFinished(
        Result<crate::backend::services::repository::RepositorySummary, String>,
    ),
    SidebarTreeLoaded {
        repo_id: String,
        result: Result<SidebarTree, String>,
    },
    SidebarSmartFoldersLoaded {
        repo_id: String,
        result: Result<Vec<SidebarSmartFolder>, String>,
    },
    SidebarSmartFolderQueried {
        repo_id: String,
        smart_folder_id: String,
        result: Result<SmartFolderResult, String>,
    },
    SidebarPlaylistsLoaded {
        repo_id: String,
        result: Result<Vec<crate::backend::services::repository::PlaylistSummary>, String>,
    },
    /// 文件夹对话框、智能文件夹编辑、播放、弹层夹取和 Escape。
    Gap(gap::GapMessage),
    ClearRecent,
    RecentCleared {
        repo_id: String,
        result: Result<usize, String>,
    },
}

/// 侧栏归约后交给宿主的服务请求。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SidebarEffect {
    LoadTree {
        repo_id: String,
    },
    LoadSmartFolders {
        repo_id: String,
    },
    QuerySmartFolder {
        repo_id: String,
        smart_folder_id: String,
    },
    CreateSmartFolder {
        repo_id: String,
    },
    LoadPlaylists {
        repo_id: String,
    },
    LoadPlaylistDetail {
        repo_id: String,
        playlist_id: String,
    },
    /// 读一个目录。`progress` 是非静默读取（回收站、快捷访问、从回收站回到全部），任务弹层出
    /// 「读取目录」（「读取回收站」）这一行；点文件夹是静默读取，不出。
    Browse {
        repo_id: String,
        path: String,
        trash: bool,
        progress: bool,
    },
    AttachRepository {
        path: String,
    },
    UpdateSmartFolder {
        repo_id: String,
    },
    DeleteSmartFolder {
        repo_id: String,
        smart_folder_id: String,
    },
    DeletePlaylist {
        repo_id: String,
        playlist_id: String,
    },
    /// 新建播放集对话框的「创建」。结果回 `ShellMessage::PlaylistCreated`。
    CreatePlaylist {
        repo_id: String,
        name: String,
        player_type_id: String,
    },
    /// `config` 是后端表单的 `backendConfig`；Eagle 这类只给目录的来源为 `None`。
    CreateBackendRepository {
        name: String,
        path: String,
        plugin_id: String,
        config: Option<serde_json::Value>,
    },
    ClearRecent {
        repo_id: String,
    },
}

#[derive(Clone, Debug)]
pub struct SidebarState {
    pub counts: ShortcutCounts,
    /// 「清空记录」已经发出、结果还没回来。这段时间按钮写成「清空中...」并禁用。
    pub clearing_recent: bool,
    pub quick_access: Vec<SidebarShortcut>,
    pub folders: Vec<SidebarFolder>,
    /// 每个目录的直属文件数，树行右侧的计数。
    pub folder_counts: std::collections::BTreeMap<String, usize>,
    pub expanded_folders: Vec<String>,
    /// 文件夹行的右键菜单。
    pub folder_menu: Option<FolderMenu>,
    pub current_directory: String,
    pub browsing_trash: bool,
    pub tree_loading: bool,
    pub tree_error: String,
    pub smart_folders: Vec<SidebarSmartFolder>,
    pub expanded_smart_folders: Vec<String>,
    pub active_smart_folder_id: Option<String>,
    pub smart_loading: bool,
    pub smart_error: String,
    pub smart_draft: SmartFolderDraft,
    /// 最近一次智能文件夹查询的结果摘要，文件页头的标题和摘要行读它。
    pub smart_result: Option<SmartResultSummary>,
    pub playlists: Vec<SidebarPlaylist>,
    pub playlists_expanded: bool,
    pub active_playlist_id: Option<String>,
    pub popover: PopoverMode,
    pub submitting: bool,
    pub attach_path: String,
    pub popover_error: String,
    pub selected_path: Option<String>,
    /// 快捷访问点的文件。父目录快照回来以后才写进文件列表的选择；换目录或换仓库时丢掉。
    pending_quick_file: Option<String>,
    pub folder_dialog: gap::FolderDialog,
    pub folder_delete_path: String,
    pub folder_delete_label: String,
    /// 处理文件夹失败时的错误，留在对话框里。
    pub folder_delete_error: String,
    /// 处理文件夹已交给文件服务，等结果。
    pub folder_delete_submitting: bool,
    pub hover_folder: Option<String>,
    pub hover_since_ms: Option<u64>,
    /// 拖放悬停用的时钟，宿主交过来的计时器时间。
    hover_clock: u64,
    pub smart_delete_id: String,
    pub smart_delete_label: String,
    pub pending_play: Option<String>,
    pub backend_name: String,
    pub backend_url: String,
    pub backend_user: String,
    pub backend_password: String,
    pub backend_root: String,
    pub backend_plugin_id: String,
    /// 下一次系统文件夹对话框的结果按 Eagle Library 建仓，而不是附加本地文件夹。
    pub(crate) attach_eagle: bool,
    /// 在途的提交是新建资源库，不是附加本地文件夹：成功以后进度行多一步
    /// 「切换到新资源库」。
    pub(crate) creating: bool,
    /// 对任务弹层「仓库操作」行的请求（挂载、创建资源库），归约结束时交给壳层的槽位。
    pub(crate) operation: super::repo_operation::OperationRequests,
    pending_folder_create: Option<(String, String, String)>,
    pending_folder_rename: Option<(String, String, String)>,
    pending_folder_delete: Option<(String, String, FolderDeleteMode)>,
    bound_repo_id: Option<String>,
    bound_missing: bool,
    effects: Vec<SidebarEffect>,
}

impl Default for SidebarState {
    fn default() -> Self {
        Self {
            counts: ShortcutCounts::default(),
            clearing_recent: false,
            quick_access: Vec::new(),
            folders: Vec::new(),
            folder_counts: std::collections::BTreeMap::new(),
            expanded_folders: Vec::new(),
            folder_menu: None,
            current_directory: String::new(),
            browsing_trash: false,
            tree_loading: false,
            tree_error: String::new(),
            smart_folders: Vec::new(),
            expanded_smart_folders: Vec::new(),
            active_smart_folder_id: None,
            smart_loading: false,
            smart_error: String::new(),
            smart_draft: SmartFolderDraft::default(),
            smart_result: None,
            playlists: Vec::new(),
            playlists_expanded: false,
            active_playlist_id: None,
            popover: PopoverMode::Closed,
            submitting: false,
            attach_path: String::new(),
            popover_error: String::new(),
            selected_path: None,
            pending_quick_file: None,
            folder_dialog: gap::FolderDialog::default(),
            folder_delete_path: String::new(),
            folder_delete_label: String::new(),
            folder_delete_error: String::new(),
            folder_delete_submitting: false,
            hover_folder: None,
            hover_since_ms: None,
            hover_clock: 0,
            smart_delete_id: String::new(),
            smart_delete_label: String::new(),
            pending_play: None,
            backend_name: String::new(),
            backend_url: String::new(),
            backend_user: String::new(),
            backend_password: String::new(),
            backend_root: String::new(),
            backend_plugin_id: String::new(),
            attach_eagle: false,
            creating: false,
            operation: super::repo_operation::OperationRequests::default(),
            pending_folder_create: None,
            pending_folder_rename: None,
            pending_folder_delete: None,
            bound_repo_id: None,
            bound_missing: false,
            effects: Vec::new(),
        }
    }
}

impl SidebarState {
    pub fn bound_repo_id(&self) -> Option<&str> {
        self.bound_repo_id.as_deref()
    }

    pub fn take_effects(&mut self) -> Vec<SidebarEffect> {
        std::mem::take(&mut self.effects)
    }

    /// 只取走目录浏览。其余请求留在队列里，等下一次 `update`。
    pub fn take_browses(&mut self) -> Vec<SidebarEffect> {
        let mut kept = Vec::new();
        let mut browses = Vec::new();
        for effect in std::mem::take(&mut self.effects) {
            if matches!(effect, SidebarEffect::Browse { .. }) {
                browses.push(effect);
            } else {
                kept.push(effect);
            }
        }
        self.effects = kept;
        browses
    }

    /// 活动仓库变化时清空旧树，并在仓库可用时请求目录、智能文件夹和播放集。
    /// 播放器类型跟插件列表走，不按仓库读。返回是否换到了一个可用的仓库。
    pub fn bind_repository(&mut self, repo_id: Option<&str>, missing: bool) -> bool {
        if self.bound_repo_id.as_deref() == repo_id && self.bound_missing == missing {
            return false;
        }
        self.bound_repo_id = repo_id.map(str::to_owned);
        self.bound_missing = missing;
        self.clear_repository_content();
        if missing || repo_id.is_none() {
            return false;
        }
        let repo_id = repo_id.expect("ready repository id").to_string();
        self.tree_loading = true;
        self.effects.push(SidebarEffect::LoadTree {
            repo_id: repo_id.clone(),
        });
        self.effects.push(SidebarEffect::LoadSmartFolders {
            repo_id: repo_id.clone(),
        });
        self.effects.push(SidebarEffect::LoadPlaylists { repo_id });
        true
    }

    /// 用摘要重算快捷方式。同一仓库的后续摘要不重新请求目录树。
    pub fn apply_snapshot(
        &mut self,
        assets: &[ShortcutAsset],
        trash_count: i64,
        quick_access: Vec<SidebarShortcut>,
    ) {
        self.counts = count_shortcuts(assets, trash_count);
        self.quick_access = quick_access;
    }

    /// 访问记录改了素材：只重算前四项计数，回收站和快捷访问不动。
    pub(super) fn recount(&mut self, assets: &[ShortcutAsset]) {
        let trash = i64::try_from(self.counts.trash).unwrap_or(i64::MAX);
        self.counts = count_shortcuts(assets, trash);
    }

    /// 缺失仓库时快捷方式、快捷访问和目录打开都直接返回。
    pub fn select_shortcut(
        &mut self,
        workspace: &mut WorkspaceState,
        id: ShortcutId,
        locked: bool,
    ) -> bool {
        if locked {
            return false;
        }
        let Some(repo_id) = workspace.active_repo_id.clone() else {
            return false;
        };
        if id == ShortcutId::Trash {
            workspace.panel = WorkspacePanel::Trash;
            self.browsing_trash = true;
            self.effects.push(SidebarEffect::Browse {
                repo_id,
                path: String::new(),
                trash: true,
                progress: true,
            });
            return true;
        }
        workspace.library_category = match id {
            ShortcutId::Uncategorized => LibraryCategory::Uncategorized,
            ShortcutId::Untagged => LibraryCategory::Untagged,
            ShortcutId::Recent => LibraryCategory::Recent,
            ShortcutId::All | ShortcutId::Trash => LibraryCategory::All,
        };
        workspace.panel = WorkspacePanel::Files;
        if self.browsing_trash {
            self.browsing_trash = false;
            self.effects.push(SidebarEffect::Browse {
                repo_id,
                path: String::new(),
                trash: false,
                progress: true,
            });
        }
        true
    }

    pub fn open_quick_access(
        &mut self,
        workspace: &mut WorkspaceState,
        shortcut_id: &str,
        locked: bool,
    ) -> bool {
        if locked {
            return false;
        }
        let Some(shortcut) = self
            .quick_access
            .iter()
            .find(|item| item.id == shortcut_id)
            .cloned()
        else {
            return false;
        };
        if shortcut.target_kind == "smartFolder" {
            self.pending_quick_file = None;
            return shortcut
                .target_id
                .as_deref()
                .is_some_and(|id| self.select_smart_folder(workspace, id, locked));
        }
        if shortcut.target_kind == "file" {
            let Some(path) = shortcut.target_path else {
                return false;
            };
            let path = normalize_workspace_path(&path);
            let parent = parent_path(&path);
            if !self.browse_folder(workspace, &parent, locked, true) {
                return false;
            }
            // 等父目录读完才能选中它。这里先记下，快照回来再选。
            self.selected_path = Some(path.clone());
            self.pending_quick_file = Some(path);
            return true;
        }
        let Some(path) = shortcut.target_path else {
            return false;
        };
        // 快捷访问是非静默读取，任务弹层出「读取目录」。
        self.browse_folder(workspace, &normalize_workspace_path(&path), locked, true)
    }

    pub fn toggle_folder(&mut self, path: &str) {
        toggle_id(&mut self.expanded_folders, path);
    }

    /// 点文件夹（静默读取）：回到全部文件，展开到这一层并读它。
    pub fn open_folder(
        &mut self,
        workspace: &mut WorkspaceState,
        path: &str,
        locked: bool,
    ) -> bool {
        self.browse_folder(workspace, path, locked, false)
    }

    /// 打开一个文件夹；`progress` 时任务弹层出「读取目录」这一行。
    fn browse_folder(
        &mut self,
        workspace: &mut WorkspaceState,
        path: &str,
        locked: bool,
        progress: bool,
    ) -> bool {
        if locked || workspace.active_repo_id.is_none() {
            return false;
        }
        let repo_id = workspace.active_repo_id.clone().expect("repository id");
        workspace.panel = WorkspacePanel::Files;
        workspace.library_category = LibraryCategory::All;
        self.browsing_trash = false;
        self.pending_quick_file = None;
        self.current_directory = path.to_string();
        self.expand_directory(path);
        self.effects.push(SidebarEffect::Browse {
            repo_id,
            path: path.to_string(),
            trash: false,
            progress,
        });
        true
    }

    /// 结构更新后重读侧栏。不改当前面板；目录树正在加载时不重复请求。
    pub fn refresh_after_structure(&mut self, repo_id: &str, panel: WorkspacePanel) {
        if self.bound_missing || self.bound_repo_id.as_deref() != Some(repo_id) {
            eprintln!("Nana 结构更新与侧栏仓库不一致：{repo_id}");
            return;
        }
        if self.tree_loading {
            eprintln!("Nana 目录树仍在读取，跳过重复的结构刷新：{repo_id}");
        } else {
            self.tree_loading = true;
            self.tree_error.clear();
            self.effects.push(SidebarEffect::LoadTree {
                repo_id: repo_id.to_string(),
            });
        }
        self.effects.push(SidebarEffect::LoadSmartFolders {
            repo_id: repo_id.to_string(),
        });
        self.effects.push(SidebarEffect::LoadPlaylists {
            repo_id: repo_id.to_string(),
        });
        if panel == WorkspacePanel::Playlist {
            if let Some(playlist_id) = self.active_playlist_id.clone() {
                self.effects.push(SidebarEffect::LoadPlaylistDetail {
                    repo_id: repo_id.to_string(),
                    playlist_id,
                });
            }
        }
        if panel == WorkspacePanel::SmartFolder {
            if let Some(smart_folder_id) = self.active_smart_folder_id.clone() {
                self.smart_loading = true;
                self.smart_error.clear();
                self.effects.push(SidebarEffect::QuerySmartFolder {
                    repo_id: repo_id.to_string(),
                    smart_folder_id,
                });
            }
        }
    }

    /// 移走或删掉可见文件夹以后另读一次目录树。
    ///
    /// 只发 `getRepositoryTree` 这一次。仓库对不上时记日志，已有文件夹不动。
    /// 读失败交给 `apply_tree` 记日志，不另编一棵树。
    pub(in crate::shell) fn reload_folder_tree(&mut self, repo_id: &str) {
        if self.bound_missing || self.bound_repo_id.as_deref() != Some(repo_id) {
            eprintln!("Nana 侧栏树另读与当前仓库不一致：{repo_id}");
            return;
        }
        self.tree_loading = true;
        self.tree_error.clear();
        self.effects.push(SidebarEffect::LoadTree {
            repo_id: repo_id.to_string(),
        });
    }

    pub fn apply_tree(&mut self, repo_id: &str, result: Result<SidebarTree, String>) {
        if self.bound_repo_id.as_deref() != Some(repo_id) {
            eprintln!("Nana 忽略过期的目录树：{repo_id}");
            return;
        }
        self.tree_loading = false;
        match result {
            Ok(tree) => {
                self.folders = tree.folders;
                self.folder_counts = tree.counts;
                self.tree_error.clear();
                let valid = folder_paths(&self.folders);
                self.expanded_folders
                    .retain(|path| path.is_empty() || valid.contains(path));
            }
            Err(error) => {
                eprintln!("Nana 读取目录树失败：{error}");
                self.tree_error = error;
            }
        }
    }

    /// 当前目录的每一级都展开，换目录时跟着展开。
    pub fn note_directory(&mut self, path: &str, trash: bool) {
        self.browsing_trash = trash;
        self.current_directory = path.to_string();
        if !trash {
            self.expand_directory(path);
        }
    }

    /// 悬停时钟走 `step_ms`（宿主交过来的计时器时间）。
    pub fn tick_hover(&mut self, step_ms: u64) {
        self.hover_clock = self.hover_clock.saturating_add(step_ms);
    }

    /// 拖放停在文件夹上、离展开还有多少毫秒。已经停满还没结算时为 `None`：那一次由下一次重判结算
    /// （拖动的指针帧或外部拖放的悬停事件），不按它反复叫醒。
    pub(crate) fn hover_due_in(&self) -> Option<u64> {
        let due = self.hover_since_ms?.saturating_add(gap::FOLDER_HOVER_MS);
        due.checked_sub(self.hover_clock).filter(|left| *left > 0)
    }

    pub fn hover_now(&self) -> u64 {
        self.hover_clock
    }

    /// 快捷访问还在等父目录的文件路径。父目录还没读回来时留着。
    pub(super) fn pending_quick_file(&self) -> Option<&str> {
        self.pending_quick_file.as_deref()
    }

    /// 父目录已经对上，取走要选中的文件。对不上时原样留着。
    pub(super) fn take_quick_file_for_directory(&mut self, directory: &str) -> Option<String> {
        let path = self.pending_quick_file.as_deref()?;
        if parent_path(path) != directory {
            return None;
        }
        self.pending_quick_file.take()
    }

    /// 父目录没读回来，丢掉还在等的文件选择。
    pub(super) fn drop_quick_file_for_directory(&mut self, directory: &str) {
        if self
            .pending_quick_file
            .as_deref()
            .is_some_and(|path| parent_path(path) == directory)
        {
            self.pending_quick_file = None;
        }
    }

    /// 目录树上有没有这个路径。侧栏放下只认树上的文件夹，不把文件列表里的同名目录当成侧栏目标。
    pub(crate) fn contains_folder(&self, path: &str) -> bool {
        fn walk(folders: &[SidebarFolder], path: &str) -> bool {
            folders
                .iter()
                .any(|folder| folder.path == path || walk(&folder.children, path))
        }
        !path.is_empty() && walk(&self.folders, path)
    }

    fn clear_repository_content(&mut self) {
        self.counts = ShortcutCounts::default();
        self.quick_access.clear();
        self.folders.clear();
        self.folder_counts.clear();
        self.folder_menu = None;
        self.expanded_folders.clear();
        self.current_directory.clear();
        self.browsing_trash = false;
        self.tree_loading = false;
        self.tree_error.clear();
        self.smart_folders.clear();
        self.expanded_smart_folders.clear();
        self.active_smart_folder_id = None;
        self.smart_loading = false;
        self.smart_error.clear();
        self.smart_draft = SmartFolderDraft::default();
        self.smart_result = None;
        self.playlists.clear();
        self.active_playlist_id = None;
        self.selected_path = None;
        self.pending_quick_file = None;
    }

    fn expand_directory(&mut self, path: &str) {
        let mut cursor = String::new();
        for segment in path.split('/').filter(|segment| !segment.is_empty()) {
            if cursor.is_empty() {
                cursor = segment.to_string();
            } else {
                cursor = format!("{cursor}/{segment}");
            }
            if !self.expanded_folders.iter().any(|item| item == &cursor) {
                self.expanded_folders.push(cursor.clone());
            }
        }
    }

    fn expand_smart_ancestors(&mut self, id: &str) {
        let parents = smart_ancestor_ids(&self.smart_folders, id);
        for parent in parents {
            if !self
                .expanded_smart_folders
                .iter()
                .any(|item| item == &parent)
            {
                self.expanded_smart_folders.push(parent);
            }
        }
    }
}

impl SidebarFolder {
    /// 把服务返回的目录节点收成侧栏树。
    pub fn from_file_node(node: &crate::backend::services::repository::FileTreeNode) -> Self {
        Self {
            path: node.path.clone(),
            label: node.label.clone(),
            children: node.children.iter().map(Self::from_file_node).collect(),
        }
    }
}

impl SidebarSmartFolder {
    /// 把智能文件夹树收成侧栏节点，保留父子关系。
    pub fn from_tree_node(
        node: &crate::backend::services::repository::SmartFolderTreeNode,
    ) -> Self {
        Self {
            id: node.folder.smart_folder_id.clone(),
            parent_id: node.folder.parent_id.clone(),
            name: node.folder.name.clone(),
            filter: node.folder.filter.clone(),
            children: node.children.iter().map(Self::from_tree_node).collect(),
        }
    }
}

impl SidebarPlaylist {
    pub fn from_summary(playlist: &crate::backend::services::repository::PlaylistSummary) -> Self {
        Self {
            id: playlist.playlist_id.clone(),
            name: playlist.name.clone(),
            player_label: playlist.player_label.clone(),
            player_type_id: playlist.player_type_id.clone(),
            item_count: playlist.item_count,
        }
    }
}

#[cfg(test)]
mod tests;
