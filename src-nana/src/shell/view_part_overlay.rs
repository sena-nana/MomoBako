//! 浮层块：对话框、弹层和右键菜单，放进 AppShell 的 overlay 槽位，同一时刻最多一层。
//!
//! 按 [`OverlayKey`] 切换：键是「现在是哪一种浮层」，换键就换一整块；键不变时这一层的内容
//! 仍由旧视图函数整块重挂。没有浮层时槽位为空，AppShell 不再挡住下面的点击。
//! 每种浮层的入口函数在 [`overlay_branch`] 里登记，把某种浮层改成常驻时只改它自己的分支。

use nana_ui::runtime::view::AnyView;
use nana_ui::runtime::{FrameworkError, MountedView, RuntimeDocument, StableNodeId};

use super::remount_state;
use super::sidebar::PopoverMode;
use super::view_part::{composing_under, first_root, mount_detached, BodyMode, PartCx, PartId, ShellPart, Swap};
use super::ShellViewModel;

/// 现在显示哪一种浮层。顺序就是同时满足时的优先级，和 Escape 关闭的先后无关。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum OverlayKey {
    /// 处理文件夹。
    FolderDelete,
    /// 删除智能文件夹。
    SmartDelete,
    /// 删除资源库。
    RepositoryDelete,
    /// 删除插件。
    PluginDelete,
    /// 创建来源播放列表时的名称框。
    SourcePlaylist,
    /// 新建播放集。
    PlaylistCreate,
    /// 新建或重命名文件夹。
    FolderDialog,
    /// 新建或编辑智能文件夹。
    SmartFolderDialog,
    /// 仓库切换、添加和云盘表单弹层。
    RepositoryPopover,
    /// 文件夹行的右键菜单。
    FolderMenu,
    /// 任务弹层。
    TaskPopover,
    /// 文件条目的右键菜单。
    EntryMenu,
}

impl OverlayKey {
    /// 按 ViewModel 取当前浮层；没有时为 `None`。条件和各入口函数返回 `Some` 的条件一致。
    pub(crate) fn of(model: &ShellViewModel) -> Option<Self> {
        let sidebar = &model.sidebar;
        let key = if sidebar.folder_delete_open() {
            Self::FolderDelete
        } else if sidebar.smart_delete_open() {
            Self::SmartDelete
        } else if !model.workspace.dialogs.is_empty() {
            Self::RepositoryDelete
        } else if model.admin.pending_delete.is_some() {
            Self::PluginDelete
        } else if model.input.source_playlist.is_some() {
            Self::SourcePlaylist
        } else if model.playlist_dialog_open {
            Self::PlaylistCreate
        } else if sidebar.folder_dialog.open {
            Self::FolderDialog
        } else if sidebar.smart_draft.open {
            Self::SmartFolderDialog
        } else if sidebar.popover != PopoverMode::Closed {
            Self::RepositoryPopover
        } else if sidebar.folder_menu.is_some() {
            Self::FolderMenu
        } else if model.admin.popover_open {
            Self::TaskPopover
        } else if model.files.entry_menu.is_some() {
            Self::EntryMenu
        } else {
            return None;
        };
        Some(key)
    }
}

/// 一种浮层的内容。对话框和任务弹层带开合动效；入口函数在自己的模块里。
/// 文件右键菜单的条目已经不在列表里时没有内容，浮层槽位保持空着。
pub(crate) fn overlay_branch(key: OverlayKey, model: &ShellViewModel) -> Option<AnyView> {
    let modal = |dialog: Option<AnyView>| dialog.map(|dialog| super::motion::paint_modal(dialog, &model.motion));
    match key {
        OverlayKey::FolderDelete => modal(super::sidebar_view::folder_delete_dialog(model)),
        OverlayKey::SmartDelete => modal(super::sidebar_view::smart_delete_dialog(model)),
        OverlayKey::RepositoryDelete => modal(super::sidebar_view::repository_delete_dialog(model)),
        OverlayKey::PluginDelete => modal(super::admin::plugin_delete_dialog(model)),
        OverlayKey::SourcePlaylist => modal(super::input::playlist_name_dialog(model)),
        OverlayKey::PlaylistCreate => modal(super::sidebar_view::playlist_create_dialog(model)),
        OverlayKey::FolderDialog => modal(super::sidebar_view::folder_dialog(model)),
        OverlayKey::SmartFolderDialog => modal(super::sidebar_view::smart_folder_dialog(model)),
        OverlayKey::RepositoryPopover => super::sidebar_view::repository_popover(model),
        OverlayKey::FolderMenu => super::sidebar_view::folder_menu(model),
        OverlayKey::TaskPopover => {
            super::admin::task_popover(model).map(|popover| super::motion::paint_panel(popover, &model.motion))
        }
        OverlayKey::EntryMenu => super::files_view::entry_menu(model),
    }
}

/// 浮层块。
pub(crate) struct OverlayPart {
    key: Option<OverlayKey>,
    view: Option<MountedView>,
    /// 现在这份内容建好时 ViewModel 的版本。键不变时，旧视图函数建的内容在版本变了以后重挂。
    revision: u64,
}

impl ShellPart for OverlayPart {
    const ID: PartId = PartId::Overlay;
    type Signals = ();

    fn signals(_: &ShellViewModel) -> Self::Signals {}

    fn new(_: Self::Signals) -> Self {
        Self { key: None, view: None, revision: 0 }
    }

    fn root(&self) -> Option<StableNodeId> {
        first_root(self.view.as_ref())
    }

    /// 浮层不跟工作台排法走。
    fn mount(&mut self, cx: &mut PartCx<'_>, model: &ShellViewModel, _: BodyMode) -> Result<Swap, FrameworkError> {
        self.remount(cx, model)
    }

    fn sync(&mut self, _: &ShellViewModel) {}

    fn needs_remount(&self, model: &ShellViewModel) -> bool {
        OverlayKey::of(model) != self.key || (self.key.is_some() && model.revision != self.revision)
    }

    fn remount(&mut self, cx: &mut PartCx<'_>, model: &ShellViewModel) -> Result<Swap, FrameworkError> {
        let key = OverlayKey::of(model);
        let document_id = cx.document.document();
        let kept = self.view.as_ref().map(|view| remount_state::capture(cx.document.context(), document_id, view.roots()));
        let fresh = match key {
            Some(key) => mount_detached(cx.document, cx.hot, Self::ID, || overlay_branch(key, model))?,
            None => None,
        };
        if self.view.is_some() || fresh.is_some() {
            cx.stats.remounts += 1;
        }
        let old = std::mem::replace(&mut self.view, fresh);
        self.key = key;
        self.revision = model.revision;
        Ok(Swap::replace(old, kept, self.view.as_ref()))
    }

    fn composing(&self, document: &RuntimeDocument) -> bool {
        self.view.as_ref().is_some_and(|view| composing_under(document, view.roots()))
    }
}
