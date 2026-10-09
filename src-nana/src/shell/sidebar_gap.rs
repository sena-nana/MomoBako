//! 侧栏的对话框、右键菜单和弹层表单。
//!
//! 文件夹新建 / 重命名 / 处理对话框、文件夹右键菜单、智能文件夹编辑和删除、播放集播放和移除、
//! 后端表单字段、拖放悬停和全局 Escape。文案与 Vue `useFolderSidebarUi`、
//! `WorkspaceSidebarFolderDialogs.vue` 和 `useSmartFolderSidebarUi` 一致。
//! 文件夹变更交给文件服务后对话框保持打开，等文件服务回报成功才关，失败把错误写在对话框里。

use super::{SidebarEffect, SidebarSmartFolder, SidebarState};

pub const FOLDER_CREATE_TITLE: &str = "新建文件夹";
pub const FOLDER_RENAME_TITLE: &str = "重命名文件夹";
pub const FOLDER_DELETE_TITLE: &str = "处理文件夹";
pub const SMART_EDIT_TITLE: &str = "编辑智能文件夹";
pub const SMART_DELETE_TITLE: &str = "删除智能文件夹";
const POPOVER_PADDING: f32 = 4.0;

/// 文件夹新建或重命名。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FolderDialog {
    pub open: bool,
    pub rename: bool,
    pub parent: String,
    pub target: String,
    /// 重命名时原来的名字，摘要里用。
    pub label: String,
    pub value: String,
    pub error: String,
    /// 已交给文件服务，等结果。
    pub submitting: bool,
}

impl FolderDialog {
    pub fn title(&self) -> &'static str {
        if self.rename { FOLDER_RENAME_TITLE } else { FOLDER_CREATE_TITLE }
    }

    /// 主按钮文案：新建是「创建」，重命名是「保存」。
    pub fn action_label(&self) -> &'static str {
        if self.rename { "保存" } else { "创建" }
    }

    pub fn placeholder(&self) -> &'static str {
        if self.rename { "输入新的文件夹名称" } else { "输入文件夹名称" }
    }

    /// 对话框里的说明句。
    pub fn summary(&self) -> String {
        if self.rename {
            format!("正在重命名 {}。", self.label)
        } else {
            let parent = if self.parent.is_empty() { "根目录" } else { self.parent.as_str() };
            format!("将在 {parent} 下创建新文件夹。")
        }
    }
}

/// 处理文件夹时内部内容的去向，对应 Vue `FileDeleteMode`。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FolderDeleteMode {
    /// 保留内部文件和子文件夹，只删除当前这一层目录。
    MoveToParent,
    /// 将该目录及其全部内容移入回收站。
    Delete,
}

impl FolderDeleteMode {
    /// 文件服务的 `mode` 参数。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MoveToParent => "moveToParent",
            Self::Delete => "delete",
        }
    }
}

/// 文件夹行的右键菜单，坐标是指针按下的位置。
#[derive(Clone, Debug, PartialEq)]
pub struct FolderMenu {
    pub path: String,
    pub label: String,
    pub x: f32,
    pub y: f32,
}

impl SidebarState {
    pub fn smart_dialog_title(&self) -> &'static str {
        if self.smart_draft.mode_edit { SMART_EDIT_TITLE } else { "新建智能文件夹" }
    }

    pub fn smart_delete_title(&self) -> &'static str {
        SMART_DELETE_TITLE
    }
}

#[derive(Clone, Debug)]
pub enum GapMessage {
    FolderHover { path: String, now_ms: u64, dragging: bool },
    FolderLeave(String),
    OpenFolderCreate(String),
    OpenFolderRename { path: String, label: String },
    SetFolderValue(String),
    CloseFolderDialog,
    SubmitFolderDialog,
    OpenFolderDelete { path: String, label: String },
    CloseFolderDelete,
    ConfirmFolderDelete(FolderDeleteMode),
    /// 文件夹行右键。
    OpenFolderMenu { path: String, label: String, x: f32, y: f32 },
    CloseFolderMenu,
    OpenSmartEdit(String),
    OpenSmartDelete { id: String, label: String },
    CloseSmartDelete,
    ConfirmSmartDelete,
    PlayPlaylist(String),
    RemovePlaylist(String),
    SetBackendName(String),
    SetBackendUrl(String),
    SetBackendRoot(String),
    SetBackendUser(String),
    SetBackendPassword(String),
    SubmitBackend,
    Escape,
}

/// 和 `menuMotion.ts` 一样把弹层夹进视口，锚点留给动效原点。
pub fn clamp_anchored(x: f32, y: f32, width: f32, height: f32, viewport_w: f32, viewport_h: f32) -> (f32, f32) {
    let max_x = (viewport_w - width - POPOVER_PADDING).max(POPOVER_PADDING);
    let max_y = (viewport_h - height - POPOVER_PADDING).max(POPOVER_PADDING);
    (x.clamp(POPOVER_PADDING, max_x), y.clamp(POPOVER_PADDING, max_y))
}

pub(super) fn reduce(model: &mut super::super::ShellViewModel, message: GapMessage) {
    let locked = model.navigation_locked();
    let repo_id = model.workspace.active_repo_id.clone();
    let mutating = model.files.mutating;
    match message {
        GapMessage::FolderHover { path, now_ms, dragging } => {
            if let Some(path) = model.sidebar.hover_folder(path, now_ms, dragging, locked) {
                model.apply_open_folder(path);
            }
        }
        GapMessage::FolderLeave(path) => model.sidebar.leave_folder(&path),
        GapMessage::OpenFolderCreate(parent) => model.sidebar.open_folder_create(parent, locked),
        GapMessage::OpenFolderRename { path, label } => model.sidebar.open_folder_rename(path, label, locked),
        GapMessage::SetFolderValue(value) => {
            if !model.sidebar.folder_dialog.submitting {
                model.sidebar.folder_dialog.value = value;
            }
        }
        GapMessage::CloseFolderDialog => model.sidebar.close_folder_dialog(mutating),
        GapMessage::SubmitFolderDialog => {
            if model.sidebar.submit_folder_dialog(repo_id.as_deref(), mutating) {
                // 新的变更开始，上一次的文件错误不再算这次的结果。
                model.files.error.clear();
            }
        }
        GapMessage::OpenFolderDelete { path, label } => model.sidebar.open_folder_delete(path, label, locked),
        GapMessage::CloseFolderDelete => model.sidebar.close_folder_delete(mutating),
        GapMessage::ConfirmFolderDelete(mode) => {
            if model.sidebar.confirm_folder_delete(repo_id.as_deref(), mode, mutating) {
                model.files.error.clear();
            }
        }
        GapMessage::OpenFolderMenu { path, label, x, y } => model.sidebar.open_folder_menu(path, label, x, y, locked),
        GapMessage::CloseFolderMenu => model.sidebar.folder_menu = None,
        GapMessage::OpenSmartEdit(id) => model.sidebar.open_smart_edit(&id, locked),
        GapMessage::OpenSmartDelete { id, label } => model.sidebar.open_smart_delete(id, label, locked),
        GapMessage::CloseSmartDelete => model.sidebar.close_smart_delete(),
        GapMessage::ConfirmSmartDelete => model.sidebar.confirm_smart_delete(repo_id.as_deref()),
        GapMessage::PlayPlaylist(id) => model.sidebar.play_playlist(id, locked),
        GapMessage::RemovePlaylist(id) => model.sidebar.remove_playlist(id, repo_id.as_deref(), locked),
        GapMessage::SetBackendName(value) => model.sidebar.backend_name = value,
        GapMessage::SetBackendUrl(value) => model.sidebar.backend_url = value,
        GapMessage::SetBackendRoot(value) => model.sidebar.backend_root = value,
        GapMessage::SetBackendUser(value) => model.sidebar.backend_user = value,
        GapMessage::SetBackendPassword(value) => model.sidebar.backend_password = value,
        GapMessage::SubmitBackend => {
            let options = super::backend_options(&model.admin.plugins);
            let option = options.iter().find(|option| option.plugin_id == model.sidebar.backend_plugin_id);
            model.sidebar.submit_backend(option);
        }
        GapMessage::Escape => {
            dismiss_top(model);
        }
    }
}

/// Escape 能关掉的一层，按关闭的先后排。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EscapeLayer {
    /// 关闭确认。
    CloseConfirm,
    FolderMenu,
    FolderDelete,
    FolderDialog,
    SmartDelete,
    SmartDialog,
    PlaylistDialog,
    SourcePlaylist,
    RepositoryPopover,
    TaskPopover,
    TagMenu,
    /// 文件页的浮层：右键菜单 → 导入菜单 → 导出对话框 → 硬链接确认 → 文件对话框，文件操作进行中不关。
    Files(super::super::files::FilesLayer),
}

/// 现在按 Escape 会关掉哪一层；没有能关的为 `None`。全局 Escape 据此决定要不要发消息。
pub fn escape_layer(model: &super::super::ShellViewModel) -> Option<EscapeLayer> {
    let sidebar = &model.sidebar;
    let layer = if model.input.pending_close {
        EscapeLayer::CloseConfirm
    } else if sidebar.folder_menu.is_some() {
        EscapeLayer::FolderMenu
    } else if !sidebar.folder_delete_path.is_empty() {
        EscapeLayer::FolderDelete
    } else if sidebar.folder_dialog.open {
        EscapeLayer::FolderDialog
    } else if !sidebar.smart_delete_id.is_empty() {
        EscapeLayer::SmartDelete
    } else if sidebar.smart_draft.open {
        EscapeLayer::SmartDialog
    } else if model.playlist_dialog_open {
        EscapeLayer::PlaylistDialog
    } else if model.input.source_playlist.is_some() {
        EscapeLayer::SourcePlaylist
    } else if sidebar.popover != super::PopoverMode::Closed {
        EscapeLayer::RepositoryPopover
    } else if model.admin.popover_open {
        EscapeLayer::TaskPopover
    } else if model.inspect.tag_menu_open() {
        EscapeLayer::TagMenu
    } else {
        EscapeLayer::Files(model.files.escape_layer()?)
    };
    Some(layer)
}

/// 关掉最上面的一层。没有弹层时返回 false。
pub fn dismiss_top(model: &mut super::super::ShellViewModel) -> bool {
    let Some(layer) = escape_layer(model) else {
        return false;
    };
    let mutating = model.files.mutating;
    match layer {
        EscapeLayer::CloseConfirm => {
            model.input.answer_close(false);
        }
        EscapeLayer::FolderMenu => model.sidebar.folder_menu = None,
        EscapeLayer::FolderDelete => model.sidebar.close_folder_delete(mutating),
        EscapeLayer::FolderDialog => model.sidebar.close_folder_dialog(mutating),
        EscapeLayer::SmartDelete => model.sidebar.close_smart_delete(),
        EscapeLayer::SmartDialog => {
            model.sidebar.close_smart_dialog();
        }
        EscapeLayer::PlaylistDialog => model.playlist_dialog_open = false,
        EscapeLayer::SourcePlaylist => {
            model.reduce(super::super::ShellMessage::Input(super::super::input::InputMessage::CloseSourcePlaylist));
        }
        EscapeLayer::RepositoryPopover => {
            model.sidebar.close_popover();
        }
        EscapeLayer::TaskPopover => model.admin.popover_open = false,
        EscapeLayer::TagMenu => model.inspect.close_tag_menu(),
        EscapeLayer::Files(_) => return model.files.dismiss_overlay(),
    }
    true
}

impl SidebarState {
    pub fn playlists_visible(&self, locked: bool) -> bool {
        !locked && !self.bound_missing
    }

    pub fn folder_delete_open(&self) -> bool {
        !self.folder_delete_path.is_empty()
    }

    pub fn smart_delete_open(&self) -> bool {
        !self.smart_delete_id.is_empty()
    }

    /// 同一路径停满 450ms 时返回该路径，由 `apply_open_folder` 打开。离开后再进来重新计时。
    fn hover_folder(&mut self, path: String, now_ms: u64, dragging: bool, locked: bool) -> Option<String> {
        if locked || !dragging || path.is_empty() {
            return None;
        }
        if self.hover_folder.as_deref() != Some(path.as_str()) {
            self.hover_folder = Some(path);
            self.hover_since_ms = Some(now_ms);
            return None;
        }
        let since = self.hover_since_ms.unwrap_or(now_ms);
        if now_ms.saturating_sub(since) >= super::super::motion::FOLDER_HOVER_MS {
            self.hover_since_ms = None;
            return Some(path);
        }
        None
    }

    fn leave_folder(&mut self, path: &str) {
        if self.hover_folder.as_deref() == Some(path) {
            self.hover_folder = None;
            self.hover_since_ms = None;
        }
    }

    fn open_folder_menu(&mut self, path: String, label: String, x: f32, y: f32, locked: bool) {
        if locked || path.is_empty() {
            return;
        }
        self.folder_menu = Some(FolderMenu { path, label, x, y });
    }

    fn open_folder_create(&mut self, parent: String, locked: bool) {
        if locked {
            eprintln!("Nana 当前不能新建文件夹");
            return;
        }
        self.folder_menu = None;
        self.folder_dialog = FolderDialog { open: true, parent, ..FolderDialog::default() };
    }

    fn open_folder_rename(&mut self, path: String, label: String, locked: bool) {
        if locked || path.is_empty() {
            eprintln!("Nana 当前不能重命名文件夹");
            return;
        }
        self.folder_menu = None;
        self.folder_dialog = FolderDialog {
            open: true,
            rename: true,
            target: path,
            value: label.clone(),
            label,
            ..FolderDialog::default()
        };
    }

    /// 文件服务还在处理时不能关，和 Vue 的 `isMutatingFiles` 一致。
    fn close_folder_dialog(&mut self, mutating: bool) {
        if mutating || self.folder_dialog.submitting {
            return;
        }
        self.folder_dialog = FolderDialog::default();
    }

    /// 名称为空或文件服务忙时不提交。提交后对话框保持打开，等结果。
    fn submit_folder_dialog(&mut self, repo_id: Option<&str>, mutating: bool) -> bool {
        if mutating || self.folder_dialog.submitting {
            return false;
        }
        let Some(repo_id) = repo_id.filter(|id| !id.is_empty()) else {
            self.folder_dialog.error = "先选择一个资源库。".into();
            return false;
        };
        let name = self.folder_dialog.value.trim().to_string();
        if name.is_empty() {
            return false;
        }
        if self.folder_dialog.rename {
            self.pending_folder_rename = Some((repo_id.to_string(), self.folder_dialog.target.clone(), name));
        } else {
            self.pending_folder_create = Some((repo_id.to_string(), self.folder_dialog.parent.clone(), name));
        }
        self.folder_dialog.error.clear();
        self.folder_dialog.submitting = true;
        true
    }

    fn open_folder_delete(&mut self, path: String, label: String, locked: bool) {
        if locked || path.is_empty() {
            return;
        }
        self.folder_menu = None;
        self.folder_delete_path = path;
        self.folder_delete_label = label;
        self.folder_delete_error.clear();
        self.folder_delete_submitting = false;
    }

    fn close_folder_delete(&mut self, mutating: bool) {
        if mutating || self.folder_delete_submitting {
            return;
        }
        self.folder_delete_path.clear();
        self.folder_delete_label.clear();
        self.folder_delete_error.clear();
    }

    fn confirm_folder_delete(&mut self, repo_id: Option<&str>, mode: FolderDeleteMode, mutating: bool) -> bool {
        if mutating || self.folder_delete_submitting {
            return false;
        }
        let Some(repo_id) = repo_id.filter(|id| !id.is_empty()) else {
            return false;
        };
        if self.folder_delete_path.is_empty() {
            return false;
        }
        self.pending_folder_delete = Some((repo_id.to_string(), self.folder_delete_path.clone(), mode));
        self.folder_delete_error.clear();
        self.folder_delete_submitting = true;
        true
    }

    /// 文件服务处理完一次文件夹变更。成功关对话框（新建时展开父级），失败把错误留在对话框里。
    pub(crate) fn settle_folder_dialogs(&mut self, mutating: bool, error: &str) {
        if mutating {
            return;
        }
        if self.folder_dialog.submitting {
            self.folder_dialog.submitting = false;
            if error.is_empty() {
                let parent = (!self.folder_dialog.rename).then(|| self.folder_dialog.parent.clone());
                self.folder_dialog = FolderDialog::default();
                if let Some(parent) = parent.filter(|parent| !parent.is_empty()) {
                    if !self.expanded_folders.iter().any(|path| path == &parent) {
                        self.expanded_folders.push(parent);
                    }
                }
            } else {
                self.folder_dialog.error = error.to_string();
            }
        }
        if self.folder_delete_submitting {
            self.folder_delete_submitting = false;
            if error.is_empty() {
                self.folder_delete_path.clear();
                self.folder_delete_label.clear();
                self.folder_delete_error.clear();
            } else {
                self.folder_delete_error = error.to_string();
            }
        }
    }

    fn open_smart_edit(&mut self, id: &str, locked: bool) {
        if locked {
            return;
        }
        let Some(folder) = find_smart(&self.smart_folders, id).cloned() else {
            eprintln!("Nana 找不到要编辑的智能文件夹：{id}");
            return;
        };
        self.smart_draft = super::SmartFolderDraft::from_folder(&folder);
    }

    fn open_smart_delete(&mut self, id: String, label: String, locked: bool) {
        if locked || id.is_empty() {
            return;
        }
        self.smart_delete_id = id;
        self.smart_delete_label = label;
    }

    fn close_smart_delete(&mut self) {
        if self.smart_draft.busy {
            return;
        }
        self.smart_delete_id.clear();
        self.smart_delete_label.clear();
    }

    fn confirm_smart_delete(&mut self, repo_id: Option<&str>) {
        let Some(repo_id) = repo_id.filter(|id| !id.is_empty()) else {
            return;
        };
        if self.smart_delete_id.is_empty() || self.smart_draft.busy {
            return;
        }
        self.smart_draft.busy = true;
        self.effects.push(SidebarEffect::DeleteSmartFolder {
            repo_id: repo_id.to_string(),
            smart_folder_id: self.smart_delete_id.clone(),
        });
    }

    fn play_playlist(&mut self, id: String, locked: bool) {
        if locked || id.is_empty() {
            return;
        }
        let Some(repo_id) = self.bound_repo_id.clone() else {
            return;
        };
        self.pending_play = Some(id.clone());
        self.effects.push(SidebarEffect::LoadPlaylistDetail { repo_id, playlist_id: id });
    }

    pub fn take_pending_play(&mut self, playlist_id: &str) -> bool {
        if self.pending_play.as_deref() == Some(playlist_id) {
            self.pending_play = None;
            true
        } else {
            false
        }
    }

    fn remove_playlist(&mut self, id: String, repo_id: Option<&str>, locked: bool) {
        let Some(repo_id) = repo_id.filter(|repo| !repo.is_empty()) else {
            return;
        };
        if locked || id.is_empty() {
            return;
        }
        self.effects.push(SidebarEffect::DeletePlaylist { repo_id: repo_id.to_string(), playlist_id: id });
    }
}

fn find_smart<'a>(folders: &'a [SidebarSmartFolder], id: &str) -> Option<&'a SidebarSmartFolder> {
    for folder in folders {
        if folder.id == id {
            return Some(folder);
        }
        if let Some(found) = find_smart(&folder.children, id) {
            return Some(found);
        }
    }
    None
}

// 文件夹变更先记在侧栏，归约结束时由壳层挪进文件效果，避免两个模块互相持有私有字段。
impl SidebarState {
    pub fn take_folder_mutation(&mut self) -> Option<FolderMutation> {
        if let Some((repo_id, parent, name)) = self.pending_folder_create.take() {
            return Some(FolderMutation::Create { repo_id, parent, name });
        }
        if let Some((repo_id, path, name)) = self.pending_folder_rename.take() {
            return Some(FolderMutation::Rename { repo_id, path, name });
        }
        if let Some((repo_id, path, mode)) = self.pending_folder_delete.take() {
            return Some(FolderMutation::Delete { repo_id, path, mode });
        }
        None
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FolderMutation {
    Create { repo_id: String, parent: String, name: String },
    Rename { repo_id: String, path: String, name: String },
    Delete { repo_id: String, path: String, mode: FolderDeleteMode },
}

#[cfg(test)]
#[path = "sidebar_gap_tests.rs"]
mod tests;
