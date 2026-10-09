//! 侧栏导航状态。
//!
//! 对应 Vue `useSidebarShortcutsUi`、`useFolderSidebarUi`、`useSmartFolderSidebarUi`、
//! `usePlaylistSidebarUi` 和 `useRepositorySwitcherUi` 里已经读过的导航分支。
//! 新建文件夹、智能文件夹筛选表单、播放集播放和系统文件夹对话框不在这里完成。

use super::workspace::{LibraryCategory, WorkspacePanel, WorkspaceState};

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

#[path = "sidebar_smart.rs"]
mod smart;
#[path = "sidebar_gap.rs"]
mod gap;
#[path = "sidebar_bind.rs"]
mod bind;
#[path = "sidebar_popover.rs"]
mod popover;

pub use smart::{SmartFolderDraft, SmartFolderField};
pub use gap::{clamp_anchored, escape_layer, FolderDeleteMode, FolderMenu, FolderMutation, GapMessage, FOLDER_DELETE_TITLE};
pub use popover::{backend_options, BackendOption, BackendRoute};

/// 一次目录树读取：树本身和每个目录的直属文件数（Vue `FileTreeNode.fileCount`）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SidebarTree {
    pub folders: Vec<SidebarFolder>,
    pub counts: std::collections::BTreeMap<String, usize>,
}

impl From<Vec<SidebarFolder>> for SidebarTree {
    fn from(folders: Vec<SidebarFolder>) -> Self {
        Self { folders, counts: Default::default() }
    }
}

impl SidebarTree {
    /// 把服务返回的目录节点收成侧栏树，同时记下每个目录的文件数。
    pub fn from_nodes(nodes: &[crate::backend::services::repository::FileTreeNode]) -> Self {
        fn walk(nodes: &[crate::backend::services::repository::FileTreeNode], counts: &mut std::collections::BTreeMap<String, usize>) {
            for node in nodes {
                counts.insert(node.path.clone(), node.file_count);
                walk(&node.children, counts);
            }
        }
        let mut counts = std::collections::BTreeMap::new();
        walk(nodes, &mut counts);
        Self { folders: nodes.iter().map(SidebarFolder::from_file_node).collect(), counts }
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
    SetSmartFolderField { field: SmartFolderField, value: String },
    SubmitSmartFolder,
    SmartFolderSaved { repo_id: String, result: Result<Vec<SidebarSmartFolder>, String> },
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
    RepositoryAttachFinished(Result<(), String>),
    SidebarTreeLoaded { repo_id: String, result: Result<SidebarTree, String> },
    SidebarSmartFoldersLoaded { repo_id: String, result: Result<Vec<SidebarSmartFolder>, String> },
    SidebarSmartFolderQueried { repo_id: String, smart_folder_id: String, result: Result<super::files::VirtualQuery, String> },
    SidebarPlaylistsLoaded { repo_id: String, result: Result<Vec<crate::backend::services::repository::PlaylistSummary>, String> },
    /// 文件夹对话框、智能文件夹编辑、播放、弹层夹取和 Escape。
    Gap(gap::GapMessage),
    ClearRecent,
    RecentCleared { repo_id: String, result: Result<usize, String> },
}

/// 侧栏归约后交给宿主的服务请求。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SidebarEffect {
    LoadTree { repo_id: String },
    LoadSmartFolders { repo_id: String },
    QuerySmartFolder { repo_id: String, smart_folder_id: String },
    CreateSmartFolder { repo_id: String },
    LoadPlaylists { repo_id: String },
    LoadPlaylistDetail { repo_id: String, playlist_id: String },
    Browse { repo_id: String, path: String, trash: bool },
    AttachRepository { path: String },
    UpdateSmartFolder { repo_id: String },
    DeleteSmartFolder { repo_id: String, smart_folder_id: String },
    DeletePlaylist { repo_id: String, playlist_id: String },
    /// 新建播放集对话框的「创建」。结果回 `ShellMessage::PlaylistCreated`。
    CreatePlaylist { repo_id: String, name: String, player_type_id: String },
    /// `config` 是后端表单的 `backendConfig`；Eagle 这类只给目录的来源为 `None`。
    CreateBackendRepository { name: String, path: String, plugin_id: String, config: Option<serde_json::Value> },
    ClearRecent { repo_id: String },
}

#[derive(Clone, Debug)]
pub struct SidebarState {
    pub counts: ShortcutCounts,
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
    pub smart_result_count: Option<usize>,
    pub playlists: Vec<SidebarPlaylist>,
    pub playlists_expanded: bool,
    pub active_playlist_id: Option<String>,
    pub popover: PopoverMode,
    pub submitting: bool,
    pub attach_path: String,
    pub popover_error: String,
    pub selected_path: Option<String>,
    pub folder_dialog: gap::FolderDialog,
    pub folder_delete_path: String,
    pub folder_delete_label: String,
    /// 处理文件夹失败时的错误，留在对话框里。
    pub folder_delete_error: String,
    /// 处理文件夹已交给文件服务，等结果。
    pub folder_delete_submitting: bool,
    pub hover_folder: Option<String>,
    pub hover_since_ms: Option<u64>,
    /// 拖放悬停用的空闲时钟。动效时钟在没有轨道时不前进。
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
            smart_result_count: None,
            playlists: Vec::new(),
            playlists_expanded: false,
            active_playlist_id: None,
            popover: PopoverMode::Closed,
            submitting: false,
            attach_path: String::new(),
            popover_error: String::new(),
            selected_path: None,
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
        self.effects.push(SidebarEffect::LoadTree { repo_id: repo_id.clone() });
        self.effects.push(SidebarEffect::LoadSmartFolders { repo_id: repo_id.clone() });
        self.effects.push(SidebarEffect::LoadPlaylists { repo_id });
        true
    }

    /// 用摘要重算快捷方式。同一仓库的后续摘要不重新请求目录树。
    pub fn apply_snapshot(&mut self, assets: &[ShortcutAsset], trash_count: i64, quick_access: Vec<SidebarShortcut>) {
        self.counts = count_shortcuts(assets, trash_count);
        self.quick_access = quick_access;
    }

    /// 缺失仓库时快捷方式、快捷访问和目录打开都直接返回。
    pub fn select_shortcut(&mut self, workspace: &mut WorkspaceState, id: ShortcutId, locked: bool) -> bool {
        if locked {
            return false;
        }
        let Some(repo_id) = workspace.active_repo_id.clone() else {
            return false;
        };
        if id == ShortcutId::Trash {
            workspace.panel = WorkspacePanel::Trash;
            self.browsing_trash = true;
            self.effects.push(SidebarEffect::Browse { repo_id, path: String::new(), trash: true });
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
            self.effects.push(SidebarEffect::Browse { repo_id, path: String::new(), trash: false });
        }
        true
    }

    pub fn open_quick_access(&mut self, workspace: &mut WorkspaceState, shortcut_id: &str, locked: bool) -> bool {
        if locked {
            return false;
        }
        let Some(shortcut) = self.quick_access.iter().find(|item| item.id == shortcut_id).cloned() else {
            return false;
        };
        if shortcut.target_kind == "smartFolder" {
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
            if !self.open_folder(workspace, &parent, locked) {
                return false;
            }
            self.selected_path = Some(path);
            return true;
        }
        let Some(path) = shortcut.target_path else {
            return false;
        };
        self.open_folder(workspace, &normalize_workspace_path(&path), locked)
    }

    pub fn toggle_folder(&mut self, path: &str) {
        toggle_id(&mut self.expanded_folders, path);
    }

    pub fn open_folder(&mut self, workspace: &mut WorkspaceState, path: &str, locked: bool) -> bool {
        if locked || workspace.active_repo_id.is_none() {
            return false;
        }
        let repo_id = workspace.active_repo_id.clone().expect("repository id");
        workspace.panel = WorkspacePanel::Files;
        workspace.library_category = LibraryCategory::All;
        self.browsing_trash = false;
        self.current_directory = path.to_string();
        self.expand_directory(path);
        self.effects.push(SidebarEffect::Browse { repo_id, path: path.to_string(), trash: false });
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
            self.effects.push(SidebarEffect::LoadTree { repo_id: repo_id.to_string() });
        }
        self.effects.push(SidebarEffect::LoadSmartFolders { repo_id: repo_id.to_string() });
        self.effects.push(SidebarEffect::LoadPlaylists { repo_id: repo_id.to_string() });
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
                self.expanded_folders.retain(|path| path.is_empty() || valid.contains(path));
            }
            Err(error) => {
                eprintln!("Nana 读取目录树失败：{error}");
                self.tree_error = error;
            }
        }
    }

    /// 当前目录的每一级都展开，和文件夹侧栏对 `currentDirectoryPath` 的监听一致。
    pub fn note_directory(&mut self, path: &str, trash: bool) {
        self.browsing_trash = trash;
        self.current_directory = path.to_string();
        if !trash {
            self.expand_directory(path);
        }
    }

    pub fn toggle_smart_folder(&mut self, id: &str) {
        toggle_id(&mut self.expanded_smart_folders, id);
    }

    pub fn select_smart_folder(&mut self, workspace: &mut WorkspaceState, smart_folder_id: &str, locked: bool) -> bool {
        if locked || smart_folder_id.is_empty() || workspace.active_repo_id.is_none() {
            return false;
        }
        let repo_id = workspace.active_repo_id.clone().expect("repository id");
        workspace.panel = WorkspacePanel::SmartFolder;
        self.active_smart_folder_id = Some(smart_folder_id.to_string());
        self.smart_loading = true;
        self.smart_error.clear();
        self.expand_smart_ancestors(smart_folder_id);
        self.effects.push(SidebarEffect::QuerySmartFolder {
            repo_id,
            smart_folder_id: smart_folder_id.to_string(),
        });
        true
    }

    pub fn apply_smart_folders(&mut self, repo_id: &str, result: Result<Vec<SidebarSmartFolder>, String>) {
        if self.bound_repo_id.as_deref() != Some(repo_id) {
            eprintln!("Nana 忽略过期的智能文件夹：{repo_id}");
            return;
        }
        match result {
            Ok(folders) => {
                self.smart_folders = folders;
                let valid = smart_ids(&self.smart_folders);
                self.expanded_smart_folders.retain(|id| valid.contains(id));
                if let Some(active) = self.active_smart_folder_id.clone() {
                    self.expand_smart_ancestors(&active);
                }
            }
            Err(error) => {
                eprintln!("Nana 读取智能文件夹失败：{error}");
                self.smart_error = error;
            }
        }
    }

    /// 仓库或当前智能文件夹已经变化时，旧查询不能覆盖新的结果。
    pub fn note_smart_query(&mut self, repo_id: &str, smart_folder_id: &str, result: Result<usize, String>) -> bool {
        if self.bound_repo_id.as_deref() != Some(repo_id)
            || self.active_smart_folder_id.as_deref() != Some(smart_folder_id)
        {
            eprintln!("Nana 忽略过期的智能文件夹查询：{smart_folder_id}");
            return false;
        }
        self.smart_loading = false;
        match result {
            Ok(count) => {
                self.smart_error.clear();
                self.smart_result_count = Some(count);
            }
            Err(error) => {
                eprintln!("Nana 查询智能文件夹失败：{error}");
                self.smart_error = error;
            }
        }
        true
    }

    pub fn toggle_playlists(&mut self) {
        self.playlists_expanded = !self.playlists_expanded;
    }

    pub fn select_playlist(&mut self, workspace: &mut WorkspaceState, playlist_id: &str) -> bool {
        if workspace.active_repo_id.is_none() || playlist_id.is_empty() {
            return false;
        }
        let repo_id = workspace.active_repo_id.clone().expect("repository id");
        workspace.panel = WorkspacePanel::Playlist;
        self.active_playlist_id = Some(playlist_id.to_string());
        self.effects.push(SidebarEffect::LoadPlaylistDetail {
            repo_id,
            playlist_id: playlist_id.to_string(),
        });
        true
    }

    pub fn apply_playlists(&mut self, workspace: &mut WorkspaceState, repo_id: &str, result: Result<Vec<SidebarPlaylist>, String>) {
        if self.bound_repo_id.as_deref() != Some(repo_id) {
            eprintln!("Nana 忽略过期的播放集列表：{repo_id}");
            return;
        }
        match result {
            Ok(playlists) => {
                let active_exists = self
                    .active_playlist_id
                    .as_ref()
                    .is_some_and(|id| playlists.iter().any(|item| &item.id == id));
                if self.active_playlist_id.is_some() && !active_exists {
                    self.active_playlist_id = None;
                    if workspace.panel == WorkspacePanel::Playlist {
                        workspace.panel = WorkspacePanel::Files;
                    }
                }
                self.playlists = playlists;
            }
            Err(error) => eprintln!("Nana 读取播放集失败：{error}"),
        }
    }

    /// 拖放还按着，或悬停还没结算时，把悬停时钟向前拨一帧。
    pub fn tick_hover(&mut self, step_ms: u64) {
        self.hover_clock = self.hover_clock.saturating_add(step_ms);
    }

    pub fn hover_now(&self) -> u64 {
        self.hover_clock
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
        self.smart_result_count = None;
        self.playlists.clear();
        self.active_playlist_id = None;
        self.selected_path = None;
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
            if !self.expanded_smart_folders.iter().any(|item| item == &parent) {
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
    pub fn from_tree_node(node: &crate::backend::services::repository::SmartFolderTreeNode) -> Self {
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

/// 快捷方式计数。路径不含 `/` 记为未分类，没有标签记为未标签，有访问时间记为最近使用。
fn count_shortcuts(assets: &[ShortcutAsset], trash_count: i64) -> ShortcutCounts {
    let mut counts = ShortcutCounts {
        trash: usize::try_from(trash_count).unwrap_or(0),
        ..ShortcutCounts::default()
    };
    for asset in assets.iter().filter(|asset| !asset.deleted) {
        counts.all += 1;
        if !asset.path.contains('/') {
            counts.uncategorized += 1;
        }
        if asset.untagged {
            counts.untagged += 1;
        }
        if asset.accessed {
            counts.recent += 1;
        }
    }
    counts
}

/// 把仓库路径收成 `/` 分隔、去掉首尾斜杠的相对路径。
fn normalize_workspace_path(path: &str) -> String {
    path.trim().replace('\\', "/").trim_matches('/').to_string()
}

/// 仓库相对路径的父目录。根上的文件返回空路径。
pub fn parent_path(path: &str) -> String {
    let normalized = normalize_workspace_path(path);
    match normalized.rfind('/') {
        Some(index) => normalized[..index].to_string(),
        None => String::new(),
    }
}

fn toggle_id(ids: &mut Vec<String>, id: &str) {
    if let Some(index) = ids.iter().position(|item| item == id) {
        ids.remove(index);
    } else {
        ids.push(id.to_string());
    }
}

fn folder_paths(nodes: &[SidebarFolder]) -> Vec<String> {
    let mut paths = Vec::new();
    fn walk(nodes: &[SidebarFolder], paths: &mut Vec<String>) {
        for node in nodes {
            paths.push(node.path.clone());
            walk(&node.children, paths);
        }
    }
    walk(nodes, &mut paths);
    paths
}

fn smart_ids(nodes: &[SidebarSmartFolder]) -> Vec<String> {
    let mut ids = Vec::new();
    fn walk(nodes: &[SidebarSmartFolder], ids: &mut Vec<String>) {
        for node in nodes {
            ids.push(node.id.clone());
            walk(&node.children, ids);
        }
    }
    walk(nodes, &mut ids);
    ids
}

/// 展开当前智能文件夹的祖先，不展开它自己。
fn smart_ancestor_ids(nodes: &[SidebarSmartFolder], id: &str) -> Vec<String> {
    fn find(nodes: &[SidebarSmartFolder], id: &str, trail: &mut Vec<String>) -> bool {
        for node in nodes {
            if node.id == id {
                return true;
            }
            trail.push(node.id.clone());
            if find(&node.children, id, trail) {
                return true;
            }
            trail.pop();
        }
        false
    }
    let mut trail = Vec::new();
    if find(nodes, id, &mut trail) {
        trail
    } else {
        Vec::new()
    }
}

/// 处理侧栏消息。其它消息原样返回，交给壳层的其余归约。
pub(super) fn reduce_message(model: &mut super::ShellViewModel, message: super::ShellMessage) -> Option<super::ShellMessage> {
    let super::ShellMessage::Sidebar(message) = message else {
        return Some(message);
    };
    match message {
        SidebarMessage::SelectShortcut(id) => model.apply_shortcut(id),
        SidebarMessage::OpenQuickAccess(id) => model.apply_quick_access(id),
        SidebarMessage::ToggleFolder(path) => model.sidebar.toggle_folder(&path),
        SidebarMessage::OpenFolder(path) => model.apply_open_folder(path),
        SidebarMessage::RefreshFolderTree => super::tree_sync::begin(model),
        SidebarMessage::ToggleSmartFolder(id) => model.sidebar.toggle_smart_folder(&id),
        SidebarMessage::OpenSmartFolder(id) => model.apply_smart_folder(id),
        SidebarMessage::OpenSmartFolderDialog => {
            if model.navigation_locked() || model.workspace.active_repo_id.is_none() {
                eprintln!("Nana 当前不能新建智能文件夹");
            } else {
                model.sidebar.open_smart_dialog(None);
            }
        }
        SidebarMessage::OpenSmartFolderChild(parent_id) => {
            if model.navigation_locked() || model.workspace.active_repo_id.is_none() {
                eprintln!("Nana 当前不能新建子智能文件夹");
            } else {
                model.sidebar.open_smart_dialog(Some(parent_id));
            }
        }
        SidebarMessage::CloseSmartFolderDialog => model.sidebar.close_smart_dialog(),
        SidebarMessage::SetSmartFolderField { field, value } => model.sidebar.set_smart_field(field, value),
        SidebarMessage::SubmitSmartFolder => {
            model.sidebar.submit_smart_folder(model.workspace.active_repo_id.as_deref());
        }
        SidebarMessage::SmartFolderSaved { repo_id, result } => {
            model.sidebar.note_smart_saved(&repo_id, result);
            let draft = &model.sidebar.smart_draft;
            if !draft.open && !draft.error.is_empty() {
                let failure = format!("智能文件夹操作失败：{}", draft.error);
                model.status.fail(super::status::FailureSource::SmartFolder, failure);
            }
        }
        SidebarMessage::TogglePlaylists => model.sidebar.toggle_playlists(),
        SidebarMessage::OpenSidebarPlaylist(id) => model.apply_playlist(id),
        SidebarMessage::OpenRepositorySwitcher => model.sidebar.open_switcher(),
        SidebarMessage::ShowRepositoryAddMenu => {
            model.sidebar.show_add_menu();
        }
        SidebarMessage::SelectRepositoryBackend(plugin_id) => model.apply_repository_backend(&plugin_id),
        SidebarMessage::BackToAddMenu => model.sidebar.back_to_add_menu(),
        SidebarMessage::CloseRepositoryPopover => {
            model.sidebar.close_popover();
        }
        SidebarMessage::SelectRepositoryFromSwitcher(repo_id) => {
            if model.sidebar.select_from_switcher(&repo_id) {
                model.workspace.select_repository(&repo_id);
                model.repository_id = model.workspace.active_repo_id.clone();
                if let Some(repository) = model.workspace.active_repository() {
                    model.repository_name = repository.name.clone();
                }
                model.bind_sidebar_repository();
                model.leave_settings_page();
            }
        }
        SidebarMessage::DeleteRepositoryFromSwitcher => {
            if model.sidebar.delete_from_switcher(model.workspace.active_repo_id.as_deref()) {
                model.workspace.open_delete_dialog();
                model.leave_settings_page();
            }
        }
        SidebarMessage::RepositoryAttachPathChanged(path) => model.sidebar.set_attach_path(path),
        SidebarMessage::SubmitRepositoryAttach => {
            model.sidebar.submit_attach();
        }
        SidebarMessage::RepositoryAttachFinished(result) => {
            let succeeded = result.is_ok();
            model.sidebar.note_attach_finished(result);
            if succeeded {
                model.workspace.request_repository_refresh();
            }
        }
        SidebarMessage::SidebarTreeLoaded { repo_id, result } => model.sidebar.apply_tree(&repo_id, result),
        SidebarMessage::SidebarSmartFoldersLoaded { repo_id, result } => model.sidebar.apply_smart_folders(&repo_id, result),
        SidebarMessage::SidebarSmartFolderQueried { repo_id, smart_folder_id, result } => {
            let counted = match &result {
                Ok(query) => Ok(query.count),
                Err(error) => Err(error.clone()),
            };
            if model.sidebar.note_smart_query(&repo_id, &smart_folder_id, counted) {
                model.files.set_virtual_rows(result.map(|query| query.rows).unwrap_or_default());
            }
        }
        SidebarMessage::SidebarPlaylistsLoaded { repo_id, result: Ok(playlists) } => {
            model.apply_playlist_list(&repo_id, &playlists, None);
        }
        SidebarMessage::SidebarPlaylistsLoaded { repo_id, result: Err(error) } => {
            model.sidebar.apply_playlists(&mut model.workspace, &repo_id, Err(error));
        }
        SidebarMessage::Gap(message) => gap::reduce(model, message),
        SidebarMessage::ClearRecent => model.clear_recent_access(),
        SidebarMessage::RecentCleared { repo_id, result } => model.note_recent_cleared(&repo_id, result),
    }
    None
}

#[cfg(test)]
#[path = "sidebar_tests.rs"]
mod tests;
