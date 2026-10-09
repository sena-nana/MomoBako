//! 浮层块：对话框、弹层和右键菜单，放进 AppShell 的 overlay 槽位，同一时刻最多一层。
//!
//! 按 [`OverlayIdentity`] 换块：身份是「现在是哪一种浮层」加上这种浮层结构上的变化（文件对话框的
//! 种类、仓库弹层的页、右键菜单的目标和落点）。身份不变时浮层常驻：打开期间内容变了，只经各浮层
//! 登记的会话（[`session::OverlaySession`]）写信号、改绑定的字段，不重挂。没有浮层时槽位为空，
//! AppShell 不再挡住下面的点击。
//!
//! 对话框都从统一框架 [`dialog`] 建：NanaUI `Dialog` / `ConfirmDialog` 挂在自己的 `OverlayHost`
//! 下，开合、焦点和无障碍交给框架，关闭手势只发关闭消息。每种浮层的入口函数在 [`overlay_branch`]
//! 里登记。

use nana_ui::runtime::view::AnyView;
use nana_ui::runtime::{FrameworkError, MountedView, RuntimeDocument, StableNodeId};

use super::remount_state;
use super::sidebar::PopoverMode;
use super::view_part::{first_root, mount_detached, BodyMode, PartCx, PartId, ShellPart, Swap};
use super::ShellViewModel;

#[path = "overlay_session.rs"]
pub(crate) mod session;
#[path = "overlay_dialog.rs"]
pub(crate) mod dialog;

use session::OverlaySession;

/// 现在显示哪一种浮层。顺序就是同时打开时谁显示在上面：前面的显示，后面的等它关掉。
///
/// 关闭确认最先：用户在关窗口，不管停在哪个页面都要看得到。其后照原来浮层槽位里的先后，
/// 对话框在弹层和右键菜单之前。文件页的导出和文件对话框原来画在主区里、压在整个浮层槽位下面，
/// 所以排在最后。
///
/// Escape 先关显示着的那一层：显示着的对话框已经激活，运行时把 Escape 交给它、发它自己的关闭
/// 请求，全局 Escape 不再发；显示着的是弹层或菜单时走全局 Escape（`escape_layer`），那里弹层和
/// 菜单都排在文件页的导出和文件对话框前面，等着的对话框不会先被关掉。`escape_layer` 里文件夹
/// 菜单排在仓库弹层前面，和这里相反，但两者都由点击打开，开着一个时浮层槽位挡住另一个的入口，
/// 不会同时开着。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum OverlayKey {
    /// 关闭确认。
    CloseConfirm,
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
    /// 导出资源库。
    ExportDialog,
    /// 文件页的对话框：重命名、新建、复制、移动、导入和硬链接确认。
    FileDialog,
}

impl OverlayKey {
    /// 按 ViewModel 取当前浮层；没有时为 `None`。条件和各入口函数返回 `Some` 的条件一致。
    pub(crate) fn of(model: &ShellViewModel) -> Option<Self> {
        let sidebar = &model.sidebar;
        let key = if model.input.pending_close {
            Self::CloseConfirm
        } else if sidebar.folder_delete_open() {
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
        } else if model.files.export.open {
            Self::ExportDialog
        } else if model.files.dialog != super::files::FileDialog::Closed {
            Self::FileDialog
        } else {
            return None;
        };
        Some(key)
    }
}

/// 浮层块的身份：哪一种浮层，加上必须换块才跟得上的结构。身份不变时浮层常驻。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OverlayIdentity {
    pub key: OverlayKey,
    /// 结构：文件对话框的种类（硬链接确认还有没有下一条）、仓库弹层的页、右键菜单的目标和落点。
    /// 别的浮层结构固定，是空串。
    structure: String,
}

impl OverlayIdentity {
    pub(crate) fn of(model: &ShellViewModel) -> Option<Self> {
        let key = OverlayKey::of(model)?;
        let structure = match key {
            // 硬链接确认在队列空了的那一刻没有内容，有了下一条时要重新建。
            OverlayKey::FileDialog => format!("{:?}:{}", model.files.dialog, model.files.current_hardlink().is_some()),
            OverlayKey::RepositoryPopover => format!("{:?}", model.sidebar.popover),
            OverlayKey::FolderMenu => {
                model.sidebar.folder_menu.as_ref().map(|menu| format!("{}@{},{}", menu.path, menu.x, menu.y)).unwrap_or_default()
            }
            OverlayKey::EntryMenu => entry_menu_structure(model),
            _ => String::new(),
        };
        Some(Self { key, structure })
    }
}

/// 文件右键菜单的结构：目标、落点，以及目标还在不在列表里（不在时菜单没有内容，槽位空着）。
fn entry_menu_structure(model: &ShellViewModel) -> String {
    let Some(menu) = model.files.entry_menu.as_ref() else {
        return String::new();
    };
    let ctx = super::files::FileContext::from_model(model);
    let present = model.files.visible_rows(&ctx).iter().any(|row| row.path == menu.path);
    format!("{}@{},{}:{present}", menu.path, menu.x, menu.y)
}

/// 一种浮层的内容。弹层带开合动效，对话框的开合动效由框架负责；入口函数在自己的模块里，
/// 建的时候把会话登记给 [`session::register`]。文件右键菜单的条目已经不在列表里时没有内容，
/// 浮层槽位保持空着。
pub(crate) fn overlay_branch(key: OverlayKey, model: &ShellViewModel) -> Option<AnyView> {
    match key {
        OverlayKey::CloseConfirm => super::input::close_prompt(model),
        OverlayKey::FolderDelete => super::sidebar_view::folder_delete_dialog(model),
        OverlayKey::SmartDelete => super::sidebar_view::smart_delete_dialog(model),
        OverlayKey::RepositoryDelete => super::sidebar_view::repository_delete_dialog(model),
        OverlayKey::PluginDelete => super::admin::plugin_delete_dialog(model),
        OverlayKey::SourcePlaylist => super::input::playlist_name_dialog(model),
        OverlayKey::PlaylistCreate => super::sidebar_view::playlist_create_dialog(model),
        OverlayKey::FolderDialog => super::sidebar_view::folder_dialog(model),
        OverlayKey::SmartFolderDialog => super::sidebar_view::smart_folder_dialog(model),
        OverlayKey::RepositoryPopover => super::sidebar_view::repository_popover(model),
        OverlayKey::FolderMenu => super::sidebar_view::folder_menu(model),
        OverlayKey::TaskPopover => {
            super::admin::task_popover(model).map(|popover| super::motion::paint_panel(popover, &model.motion))
        }
        OverlayKey::EntryMenu => super::files_view::entry_menu(model),
        OverlayKey::ExportDialog => super::workspace_dialogs::export_dialog(model),
        OverlayKey::FileDialog => super::workspace_dialogs::file_dialog(model),
    }
}

/// 浮层块。
pub(crate) struct OverlayPart {
    identity: Option<OverlayIdentity>,
    view: Option<MountedView>,
    /// 现在这块浮层登记的会话，随浮层一起换下。
    sessions: Vec<Box<dyn OverlaySession>>,
}

impl ShellPart for OverlayPart {
    const ID: PartId = PartId::Overlay;
    type Signals = ();

    fn signals(_: &ShellViewModel) -> Self::Signals {}

    fn new(_: Self::Signals) -> Self {
        Self { identity: None, view: None, sessions: Vec::new() }
    }

    fn root(&self) -> Option<StableNodeId> {
        first_root(self.view.as_ref())
    }

    /// 浮层不跟工作台排法走。
    fn mount(&mut self, cx: &mut PartCx<'_>, model: &ShellViewModel, _: BodyMode) -> Result<Swap, FrameworkError> {
        self.remount(cx, model)
    }

    /// 打开期间的变化都经会话写进信号。
    fn sync(&mut self, model: &ShellViewModel) {
        for session in &mut self.sessions {
            session.write(model);
        }
    }

    /// 只有身份变了（开、关、换了一种浮层或结构）才重挂。
    fn needs_remount(&self, model: &ShellViewModel) -> bool {
        OverlayIdentity::of(model) != self.identity
    }

    /// 换块：先让旧的对话框经框架关掉、交还焦点，再挂新的一块，挂好后告诉它的会话。
    fn remount(&mut self, cx: &mut PartCx<'_>, model: &ShellViewModel) -> Result<Swap, FrameworkError> {
        let identity = OverlayIdentity::of(model);
        for session in &mut self.sessions {
            session.retire(cx.document.context_mut());
        }
        let document_id = cx.document.document();
        let kept = self.view.as_ref().map(|view| remount_state::capture(cx.document.context(), document_id, view.roots()));
        let mut sessions = Vec::new();
        let fresh = match &identity {
            Some(identity) => mount_detached(cx.document, cx.hot, Self::ID, || {
                let (view, collected) = session::collect(|| overlay_branch(identity.key, model));
                sessions = collected;
                view
            })?,
            None => None,
        };
        if fresh.is_none() {
            sessions.clear();
        }
        for session in &mut sessions {
            session.placed();
        }
        if self.view.is_some() || fresh.is_some() {
            cx.stats.remounts += 1;
        }
        let old = std::mem::replace(&mut self.view, fresh);
        self.identity = identity;
        self.sessions = sessions;
        Ok(Swap::replace(old, kept, self.view.as_ref()))
    }

    /// 浮层换块要等组合结束的情形比别的块多一种：焦点在浮层里时换块会换掉输入框；焦点在别处
    /// （标题栏搜索框）时，新开的对话框一激活就把焦点拿走，同样打断组合。所以文档里任何一个
    /// 获得焦点的输入框还有预编辑时都延后，组合结束后的下一帧按最新状态换块。打开期间的变化
    /// 只写信号，不受影响。
    fn composing(&self, document: &RuntimeDocument) -> bool {
        let world = document.context().world();
        world.focused(document.document()).is_some_and(|focused| world.ime(focused).is_some_and(|ime| !ime.text.is_empty()))
    }
}

#[cfg(test)]
#[path = "overlay_dialog_tests.rs"]
mod dialog_tests;
#[cfg(test)]
#[path = "overlay_popover_tests.rs"]
mod popover_tests;
