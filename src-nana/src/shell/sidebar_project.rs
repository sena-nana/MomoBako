//! 常驻侧栏的投影和信号。
//!
//! [`SidebarView`] 是从 ViewModel 算出的侧栏要显示的全部东西，文案和列表都按显示的样子算好。
//! [`SidebarSignals`] 按「谁一起变、谁读它」拆开：仓库头、导航、快捷访问、三个分组的头部和底部入口
//! 各是一个信号；播放集、文件夹树和智能文件夹树的行放进按键对照的 Store，改一行只重跑读这一行的
//! 绑定。同步时只写变了的信号；列表只删掉没了的行、插入新行、整行改写内容变了的行，顺序变了再排。

use std::collections::{HashMap, HashSet};
use std::hash::Hash;

use nana_ui::runtime::view::{signal, store, Signal, Store, StoreList, StorePath};

use super::super::sidebar::{SidebarFolder, SidebarSmartFolder};
use super::super::{LibraryCategory, ShellPage, ShellViewModel, WorkspacePanel};

/// 侧栏要显示的东西。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SidebarView {
    pub head: HeadView,
    pub nav: NavView,
    pub quick: Vec<QuickRow>,
    pub playlists: PlaylistGroup,
    pub playlist_rows: Vec<PlaylistRow>,
    pub folders: FolderGroup,
    pub folder_rows: Vec<FolderRow>,
    pub smart: SmartGroup,
    pub smart_rows: Vec<SmartRow>,
    pub footer: FooterView,
}

/// 仓库头和顶部错误条，以及文件夹分组在不在。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct HeadView {
    /// 仓库头的名字，没有仓库时是「无资源库」。
    pub name: String,
    /// 目录树或智能文件夹读取失败时的错误，先看目录树；空时不显示错误条。
    pub error: String,
    /// 虚拟条目来源没有真实目录，不显示文件夹分组（Vue `showFolderSidebar`）。
    pub folders_visible: bool,
}

/// 快捷方式和动作入口：计数、当前态，以及缺失仓库时整组禁用。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NavView {
    pub locked: bool,
    /// 五个快捷方式的计数和当前态，顺序是全部、未分类、未标签、最近使用、回收站。
    pub shortcuts: [(usize, bool); 5],
    /// 仓库动作数，0 时不显示动作分组。
    pub actions: usize,
    pub actions_active: bool,
}

/// 快捷访问的一行。书签没有会变的字段，键就是整行，内容变了整行重建。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct QuickRow {
    pub id: String,
    pub label: String,
    pub target: QuickTarget,
}

/// 快捷访问指向什么，决定行首图标。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum QuickTarget {
    SmartFolder,
    File,
    Folder,
}

/// 播放集分组的头部和正文。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PlaylistGroup {
    pub count: usize,
    pub expanded: bool,
    /// 没有仓库、仓库缺失或没有播放插件类型时不能新建。
    pub create_locked: bool,
    /// 展开后的空状态说明；有播放集可列时为 `None`，这时显示列表。
    pub hint: Option<&'static str>,
}

/// 播放集一行。键是播放集编号，其余字段原地改。字段不叫 `id`：`Item` 自己有同名方法。
#[derive(Clone, Debug, PartialEq, Store)]
pub(crate) struct PlaylistRow {
    pub playlist_id: String,
    pub name: String,
    /// 「播放器 · N 项」。当前点开的播放集按详情里实际的项数算。
    pub meta: String,
    pub active: bool,
    /// 有对应的播放插件。
    pub playable: bool,
}

/// 文件夹分组的头部和正文。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FolderGroup {
    pub create_disabled: bool,
    pub refresh_disabled: bool,
    /// 目录树读取中：刷新按钮换成转圈图标。
    pub loading: bool,
    /// 新建文件夹的父目录，也就是当前目录。处理器在点击时现读。
    pub parent: String,
    pub hint: Option<&'static str>,
    /// 显示目录树（不在回收站、有目录）。
    pub tree: bool,
}

/// 文件夹树展开后的一行。路径是身份；深度和有没有子目录决定行的结构，一起放进键，
/// 变了整行重建，其余字段原地改。
#[derive(Clone, Debug, PartialEq, Store)]
pub(crate) struct FolderRow {
    pub path: String,
    pub label: String,
    pub depth: u16,
    pub has_children: bool,
    pub expanded: bool,
    /// 当前目录。
    pub active: bool,
    /// 当前目录在这一支上：用打开的文件夹图标。
    pub branch: bool,
    /// 直属文件数。
    pub count: usize,
}

/// 智能文件夹分组的头部和正文。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SmartGroup {
    pub create_disabled: bool,
    /// 智能文件夹正在提交：行上的新建子级和编辑不可用。
    pub busy: bool,
    pub hint: Option<&'static str>,
}

/// 智能文件夹树展开后的一行。键同文件夹树：编号加深度和有没有子级。
#[derive(Clone, Debug, PartialEq, Store)]
pub(crate) struct SmartRow {
    pub smart_id: String,
    pub name: String,
    pub depth: u16,
    pub has_children: bool,
    pub expanded: bool,
    pub active: bool,
    pub branch: bool,
}

/// 底部入口：设置、拓展、日志哪个是当前入口，任务弹层开没开、有几个任务。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FooterView {
    pub settings: bool,
    pub extensions: bool,
    pub logs: bool,
    pub tasks_open: bool,
    pub tasks: usize,
}

/// 无仓库时各分组共用的说明。
const NO_REPOSITORY: &str = "先选择或添加一个资源库。";

impl SidebarView {
    /// 从 ViewModel 取侧栏的投影，算法和 Vue 侧栏各分组读取时一致。
    pub(crate) fn project(model: &ShellViewModel) -> Self {
        let workspace = &model.workspace;
        let sidebar = &model.sidebar;
        let repository = workspace.active_repository();
        let panel = workspace.panel;
        let has_repo = workspace.active_repo_id.is_some();
        let locked = model.navigation_locked();
        let settings = matches!(model.page, ShellPage::Settings | ShellPage::SettingsError);
        let files = panel == WorkspacePanel::Files;
        let category = workspace.library_category;
        let counts = sidebar.counts;
        let no_players = model.playlist_players.is_empty();
        let trash = panel == WorkspacePanel::Trash || sidebar.browsing_trash;
        let loading = sidebar.tree_loading;
        Self {
            head: HeadView {
                name: repository.map(|item| item.name.clone()).unwrap_or_else(|| "无资源库".into()),
                error: [&sidebar.tree_error, &sidebar.smart_error]
                    .into_iter()
                    .find(|error| !error.is_empty())
                    .cloned()
                    .unwrap_or_default(),
                folders_visible: repository
                    .is_none_or(|item| !item.capabilities.iter().any(|capability| capability == "virtual-entries")),
            },
            nav: NavView {
                locked,
                shortcuts: [
                    (counts.all, files && category == LibraryCategory::All),
                    (counts.uncategorized, files && category == LibraryCategory::Uncategorized),
                    (counts.untagged, files && category == LibraryCategory::Untagged),
                    (counts.recent, files && category == LibraryCategory::Recent),
                    (counts.trash, panel == WorkspacePanel::Trash),
                ],
                actions: model.admin.actions.len(),
                actions_active: panel == WorkspacePanel::Actions,
            },
            quick: sidebar
                .quick_access
                .iter()
                .map(|shortcut| QuickRow {
                    id: shortcut.id.clone(),
                    label: shortcut.label.clone(),
                    target: match shortcut.target_kind.as_str() {
                        "smartFolder" => QuickTarget::SmartFolder,
                        "file" => QuickTarget::File,
                        _ => QuickTarget::Folder,
                    },
                })
                .collect(),
            playlists: PlaylistGroup {
                count: sidebar.playlists.len(),
                expanded: sidebar.playlists_expanded,
                create_locked: !has_repo || locked || no_players,
                hint: if !has_repo {
                    Some(NO_REPOSITORY)
                } else if locked {
                    Some("资源库修复后可继续使用播放集。")
                } else if !sidebar.playlists.is_empty() {
                    None
                } else if no_players {
                    Some("当前没有可用的播放插件类型。")
                } else {
                    Some("还没有播放集。")
                },
            },
            playlist_rows: playlist_rows(model),
            folders: FolderGroup {
                create_disabled: !has_repo || locked || model.files.mutating || trash,
                refresh_disabled: !has_repo || locked || loading,
                loading,
                parent: sidebar.current_directory.clone(),
                hint: if !has_repo {
                    Some(NO_REPOSITORY)
                } else if locked {
                    Some("资源库文件夹丢失，请先在主视图修复。")
                } else if (trash || sidebar.folders.is_empty()) && !loading {
                    Some(if trash { "回收站条目在主视图中管理。" } else { "当前仓库还没有子文件夹。" })
                } else {
                    None
                },
                tree: has_repo && !locked && !trash && !sidebar.folders.is_empty(),
            },
            folder_rows: folder_rows(model),
            smart: SmartGroup {
                create_disabled: !has_repo || locked || sidebar.smart_draft.busy,
                busy: sidebar.smart_draft.busy,
                hint: if !has_repo {
                    Some(NO_REPOSITORY)
                } else if locked {
                    Some("资源库修复后可继续使用智能文件夹。")
                } else if sidebar.smart_folders.is_empty() {
                    Some("还没有智能文件夹。")
                } else {
                    None
                },
            },
            smart_rows: smart_rows(model),
            footer: FooterView {
                settings,
                extensions: !settings && panel == WorkspacePanel::Extensions,
                logs: !settings && panel == WorkspacePanel::Logs,
                tasks_open: model.admin.popover_open,
                tasks: model.active_tasks,
            },
        }
    }
}

/// 播放集各行。当前点开、详情已读回的播放集按详情里的条目数显示项数。
fn playlist_rows(model: &ShellViewModel) -> Vec<PlaylistRow> {
    let listed = model.player.listed.as_ref().map(|detail| (detail.playlist.playlist_id.as_str(), detail.items.len()));
    let playlist_panel = model.workspace.panel == WorkspacePanel::Playlist;
    model
        .sidebar
        .playlists
        .iter()
        .map(|playlist| {
            let count = listed
                .filter(|(id, _)| *id == playlist.id)
                .map(|(_, count)| count as i64)
                .unwrap_or(playlist.item_count);
            PlaylistRow {
                playlist_id: playlist.id.clone(),
                name: playlist.name.clone(),
                meta: format!("{} · {} 项", playlist.player_label, count),
                active: playlist_panel && model.sidebar.active_playlist_id.as_deref() == Some(playlist.id.as_str()),
                playable: !super::super::player_view::playlist_plugin_missing(model, &playlist.player_type_id),
            }
        })
        .collect()
}

/// 文件夹树按显示顺序展开：每个目录一行，展开了的目录后面跟着它的子级。
fn folder_rows(model: &ShellViewModel) -> Vec<FolderRow> {
    fn walk(model: &ShellViewModel, folder: &SidebarFolder, depth: u16, rows: &mut Vec<FolderRow>) {
        let sidebar = &model.sidebar;
        let current = sidebar.current_directory.as_str();
        let expanded = sidebar.expanded_folders.iter().any(|path| path == &folder.path);
        let active = current == folder.path;
        let has_children = !folder.children.is_empty();
        rows.push(FolderRow {
            path: folder.path.clone(),
            label: folder.label.clone(),
            depth,
            has_children,
            expanded,
            active,
            branch: active || current.starts_with(&format!("{}/", folder.path)),
            count: sidebar.folder_counts.get(&folder.path).copied().unwrap_or(0),
        });
        if has_children && expanded {
            for child in &folder.children {
                walk(model, child, depth + 1, rows);
            }
        }
    }
    let mut rows = Vec::new();
    for folder in &model.sidebar.folders {
        walk(model, folder, 1, &mut rows);
    }
    rows
}

/// 智能文件夹树按显示顺序展开。包含当前智能文件夹的一支用打开的图标。
fn smart_rows(model: &ShellViewModel) -> Vec<SmartRow> {
    fn contains(folders: &[SidebarSmartFolder], id: &str) -> bool {
        folders.iter().any(|folder| folder.id == id || contains(&folder.children, id))
    }
    fn walk(model: &ShellViewModel, folder: &SidebarSmartFolder, depth: u16, rows: &mut Vec<SmartRow>) {
        let sidebar = &model.sidebar;
        let active_id = sidebar.active_smart_folder_id.as_deref();
        let active = active_id == Some(folder.id.as_str());
        let expanded = sidebar.expanded_smart_folders.iter().any(|id| id == &folder.id);
        let has_children = !folder.children.is_empty();
        rows.push(SmartRow {
            smart_id: folder.id.clone(),
            name: folder.name.clone(),
            depth,
            has_children,
            expanded,
            active,
            branch: active || active_id.is_some_and(|id| contains(&folder.children, id)),
        });
        if has_children && expanded {
            for child in &folder.children {
                walk(model, child, depth + 1, rows);
            }
        }
    }
    let mut rows = Vec::new();
    for folder in &model.sidebar.smart_folders {
        walk(model, folder, 1, &mut rows);
    }
    rows
}

/// 播放集行的键：编号。
pub(crate) fn playlist_key(row: &PlaylistRow) -> String {
    row.playlist_id.clone()
}

/// 文件夹行的键：路径、深度和有没有子目录。
pub(crate) fn folder_key(row: &FolderRow) -> (String, u16, bool) {
    (row.path.clone(), row.depth, row.has_children)
}

/// 智能文件夹行的键：编号、深度和有没有子级。
pub(crate) fn smart_key(row: &SmartRow) -> (String, u16, bool) {
    (row.smart_id.clone(), row.depth, row.has_children)
}

/// 常驻侧栏的信号。句柄是 `Copy` 的 id，值在骨架的挂载作用域里，侧栏整块重挂（收起再展开）时不重建。
#[derive(Clone, Copy)]
pub(crate) struct SidebarSignals {
    pub head: Signal<HeadView>,
    pub nav: Signal<NavView>,
    pub quick: Signal<Vec<QuickRow>>,
    pub playlists: Signal<PlaylistGroup>,
    pub playlist_rows: Store<Vec<PlaylistRow>>,
    pub folders: Signal<FolderGroup>,
    pub folder_rows: Store<Vec<FolderRow>>,
    pub smart: Signal<SmartGroup>,
    pub smart_rows: Store<Vec<SmartRow>>,
    pub footer: Signal<FooterView>,
}

impl SidebarSignals {
    /// 在当前作用域里建信号，初值是 `view`。只在骨架的挂载闭包里调用。
    pub(crate) fn new(view: SidebarView) -> Self {
        Self {
            head: signal(view.head),
            nav: signal(view.nav),
            quick: signal(view.quick),
            playlists: signal(view.playlists),
            playlist_rows: store(view.playlist_rows),
            folders: signal(view.folders),
            folder_rows: store(view.folder_rows),
            smart: signal(view.smart),
            smart_rows: store(view.smart_rows),
            footer: signal(view.footer),
        }
    }

    /// 骨架还在：信号所在的作用域没有被回收。Store 和信号同一个作用域。
    pub(crate) fn alive(&self) -> bool {
        self.head.defined_at().is_some()
    }

    /// 写入投影：信号只写变了的，列表按键对照着改。骨架已回收时只记日志。
    pub(crate) fn write(&self, view: SidebarView) {
        if !self.alive() {
            eprintln!("Nana 侧栏信号已随骨架回收，跳过写入");
            return;
        }
        self.head.try_set_if_changed(view.head);
        self.nav.try_set_if_changed(view.nav);
        self.quick.try_set_if_changed(view.quick);
        self.playlists.try_set_if_changed(view.playlists);
        sync_rows(self.playlist_rows, playlist_key, view.playlist_rows);
        self.folders.try_set_if_changed(view.folders);
        sync_rows(self.folder_rows, folder_key, view.folder_rows);
        self.smart.try_set_if_changed(view.smart);
        sync_rows(self.smart_rows, smart_key, view.smart_rows);
        self.footer.try_set_if_changed(view.footer);
    }
}

/// 把 `rows` 写进按 `key` 对照的 Store 列表。
///
/// 算法：键重复的只留第一行；先删掉新列表里没有的行，再按新顺序把没有的行插到它的位置上，
/// 已有的行内容变了才整行改写（只重跑读这一行的绑定）；最后顺序和新列表不同时按新位置排一次。
/// 删、插、排都只触发列表本身，已有的行一个绑定都不重跑。
pub(crate) fn sync_rows<T, K>(list: Store<Vec<T>>, key: fn(&T) -> K, rows: Vec<T>)
where
    T: Clone + PartialEq + 'static,
    K: Hash + Eq + Clone + 'static,
{
    let mut seen = HashSet::with_capacity(rows.len());
    let rows = rows.into_iter().filter(|row| seen.insert(key(row))).collect::<Vec<_>>();
    let current = list.get_untracked();
    if current == rows {
        return;
    }
    let old = current.iter().map(|row| (key(row), row)).collect::<HashMap<_, _>>();
    if current.iter().any(|row| !seen.contains(&key(row))) {
        list.retain(|row| seen.contains(&key(row)));
    }
    let keyed = list.keyed(key);
    for (index, row) in rows.iter().enumerate() {
        let id = key(row);
        match old.get(&id) {
            None => list.insert(index, row.clone()),
            Some(previous) if *previous != row => keyed.at(&id).set(row.clone()),
            Some(_) => {}
        }
    }
    let order = rows.iter().map(key).collect::<Vec<_>>();
    if list.with(|items| items.iter().map(key).ne(order.iter().cloned())) {
        let position = order.into_iter().enumerate().map(|(index, id)| (id, index)).collect::<HashMap<_, _>>();
        list.sort_by_key(|row| position.get(&key(row)).copied().unwrap_or(usize::MAX));
    }
}

#[cfg(test)]
#[path = "sidebar_project_tests.rs"]
pub(super) mod tests;
