//! 侧栏块：仓库头、文件管理分组和底部入口，放进工作区的资源区。
//!
//! 现在仍由 `sidebar_view` 的视图函数整块建出。要不要重挂按 [`SidebarProjection`] 判断：它列出
//! 侧栏视图读到的每一项 ViewModel 状态，相等就不重挂，路由切换、日志广播这类和侧栏无关的更新
//! 不再换掉侧栏的节点。改侧栏视图读到的状态时，同步改这里的投影。

use nana_ui::runtime::view::{widget, AnyView, IntoView};
use nana_ui::runtime::{FrameworkError, MountedView, RuntimeDocument, StableNodeId};

use super::remount_state;
use super::sidebar::{ShortcutCounts, SidebarFolder, SidebarPlaylist, SidebarShortcut};
use super::view_part::{composing_under, first_root, mount_detached, BodyMode, PartCx, PartId, ShellPart, Swap};
use super::{LibraryCategory, ShellPage, ShellViewModel, WorkspacePanel};

/// 侧栏块。主区独占时没有内容。
pub(crate) struct SidebarPart {
    present: bool,
    view: Option<MountedView>,
    /// 现在这份内容按哪份投影建的。
    projection: Option<SidebarProjection>,
}

impl ShellPart for SidebarPart {
    const ID: PartId = PartId::Sidebar;
    type Signals = ();

    fn signals(_: &ShellViewModel) -> Self::Signals {}

    fn new(_: Self::Signals) -> Self {
        Self { present: false, view: None, projection: None }
    }

    fn root(&self) -> Option<StableNodeId> {
        first_root(self.view.as_ref())
    }

    fn mount(&mut self, cx: &mut PartCx<'_>, model: &ShellViewModel, mode: BodyMode) -> Result<Swap, FrameworkError> {
        self.present = mode == BodyMode::Workbench;
        self.remount(cx, model)
    }

    fn sync(&mut self, _: &ShellViewModel) {}

    fn needs_remount(&self, model: &ShellViewModel) -> bool {
        self.present && self.projection.as_ref() != Some(&SidebarProjection::project(model))
    }

    fn remount(&mut self, cx: &mut PartCx<'_>, model: &ShellViewModel) -> Result<Swap, FrameworkError> {
        let document_id = cx.document.document();
        let kept = self.view.as_ref().map(|view| remount_state::capture(cx.document.context(), document_id, view.roots()));
        let fresh = if self.present {
            mount_detached(cx.document, cx.hot, Self::ID, || Some(sidebar_view(model)))?
        } else {
            None
        };
        cx.stats.remounts += 1;
        let old = std::mem::replace(&mut self.view, fresh);
        self.projection = self.present.then(|| SidebarProjection::project(model));
        Ok(Swap::replace(old, kept, self.view.as_ref()))
    }

    fn composing(&self, document: &RuntimeDocument) -> bool {
        self.view.as_ref().is_some_and(|view| composing_under(document, view.roots()))
    }
}

/// 侧栏：仓库头、分组和底部入口。
fn sidebar_view(model: &ShellViewModel) -> AnyView {
    widget(super::sidebar_view::sidebar_frame())
        .top(super::sidebar_view::sidebar_switcher(model))
        .body(super::sidebar_view::sidebar_sections(model))
        .footer(super::sidebar_view::sidebar_footer(model))
        .into_any()
}

/// 侧栏视图读到的全部状态。底部入口平时的透明度走热信号，不在这里。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SidebarProjection {
    repository_name: Option<String>,
    has_repository: bool,
    locked: bool,
    /// 虚拟条目来源不显示文件夹分组。
    virtual_entries: bool,
    tree_error: String,
    smart_error: String,
    counts: ShortcutCounts,
    files_panel: bool,
    trash_panel: bool,
    actions_panel: bool,
    playlist_panel: bool,
    category: LibraryCategory,
    quick_access: Vec<SidebarShortcut>,
    actions: usize,
    playlists_expanded: bool,
    playlists: Vec<PlaylistRow>,
    no_players: bool,
    active_playlist_id: Option<String>,
    browsing_trash: bool,
    tree_loading: bool,
    current_directory: String,
    files_mutating: bool,
    folders: Vec<SidebarFolder>,
    expanded_folders: Vec<String>,
    folder_counts: std::collections::BTreeMap<String, usize>,
    smart_busy: bool,
    smart_folders: Vec<SmartRow>,
    active_smart_folder_id: Option<String>,
    expanded_smart_folders: Vec<String>,
    settings_page: bool,
    extensions_panel: bool,
    logs_panel: bool,
    tasks_open: bool,
    active_tasks: usize,
}

/// 播放集一行：列表数据加上「播放器 · N 项」里实际显示的项数和能不能播放。
#[derive(Clone, Debug, PartialEq)]
struct PlaylistRow {
    playlist: SidebarPlaylist,
    shown_count: i64,
    playable: bool,
}

/// 智能文件夹树的一个节点：只留视图读到的 id、名称和子级。
#[derive(Clone, Debug, PartialEq)]
struct SmartRow {
    id: String,
    name: String,
    children: Vec<SmartRow>,
}

impl SidebarProjection {
    /// 从 ViewModel 取侧栏要显示的东西，算法和 `sidebar_view`、`sidebar_tree_view` 读取时一致。
    pub(crate) fn project(model: &ShellViewModel) -> Self {
        let workspace = &model.workspace;
        let sidebar = &model.sidebar;
        let repository = workspace.active_repository();
        let panel = workspace.panel;
        let settings_page = matches!(model.page, ShellPage::Settings | ShellPage::SettingsError);
        let listed = model.player.listed.as_ref().map(|detail| (detail.playlist.playlist_id.as_str(), detail.items.len()));
        let playlists = sidebar
            .playlists
            .iter()
            .map(|playlist| PlaylistRow {
                playlist: playlist.clone(),
                shown_count: listed
                    .filter(|(id, _)| *id == playlist.id)
                    .map(|(_, count)| count as i64)
                    .unwrap_or(playlist.item_count),
                playable: !super::player_view::playlist_plugin_missing(model, &playlist.player_type_id),
            })
            .collect();
        Self {
            repository_name: repository.map(|item| item.name.clone()),
            has_repository: workspace.active_repo_id.is_some(),
            locked: model.navigation_locked(),
            virtual_entries: repository.is_some_and(|item| item.capabilities.iter().any(|capability| capability == "virtual-entries")),
            tree_error: sidebar.tree_error.clone(),
            smart_error: sidebar.smart_error.clone(),
            counts: sidebar.counts,
            files_panel: panel == WorkspacePanel::Files,
            trash_panel: panel == WorkspacePanel::Trash,
            actions_panel: panel == WorkspacePanel::Actions,
            playlist_panel: panel == WorkspacePanel::Playlist,
            category: workspace.library_category,
            quick_access: sidebar.quick_access.clone(),
            actions: model.admin.actions.len(),
            playlists_expanded: sidebar.playlists_expanded,
            playlists,
            no_players: model.playlist_players.is_empty(),
            active_playlist_id: sidebar.active_playlist_id.clone(),
            browsing_trash: sidebar.browsing_trash,
            tree_loading: sidebar.tree_loading,
            current_directory: sidebar.current_directory.clone(),
            files_mutating: model.files.mutating,
            folders: sidebar.folders.clone(),
            expanded_folders: sidebar.expanded_folders.clone(),
            folder_counts: sidebar.folder_counts.clone(),
            smart_busy: sidebar.smart_draft.busy,
            smart_folders: sidebar.smart_folders.iter().map(SmartRow::from).collect(),
            active_smart_folder_id: sidebar.active_smart_folder_id.clone(),
            expanded_smart_folders: sidebar.expanded_smart_folders.clone(),
            settings_page,
            extensions_panel: !settings_page && panel == WorkspacePanel::Extensions,
            logs_panel: !settings_page && panel == WorkspacePanel::Logs,
            tasks_open: model.admin.popover_open,
            active_tasks: model.active_tasks,
        }
    }
}

impl From<&super::sidebar::SidebarSmartFolder> for SmartRow {
    fn from(folder: &super::sidebar::SidebarSmartFolder) -> Self {
        Self {
            id: folder.id.clone(),
            name: folder.name.clone(),
            children: folder.children.iter().map(SmartRow::from).collect(),
        }
    }
}

#[cfg(test)]
#[path = "view_part_sidebar_tests.rs"]
mod tests;
