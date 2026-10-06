//! 侧栏里还没接到实况界面的分支。
//!
//! 文件夹对话框、智能文件夹编辑和删除、播放集播放和移除、弹层夹取、
//! Eagle / 云盘表单、拖放悬停和全局 Escape。文案与 Vue 侧栏 composable 一致。

use super::{SidebarEffect, SidebarSmartFolder, SidebarState};

pub const FOLDER_CREATE_TITLE: &str = "新建文件夹";
pub const FOLDER_RENAME_TITLE: &str = "重命名文件夹";
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
    pub value: String,
    pub error: String,
}

impl FolderDialog {
    pub fn title(&self) -> &'static str {
        if self.rename { FOLDER_RENAME_TITLE } else { FOLDER_CREATE_TITLE }
    }
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
    ConfirmFolderDelete,
    OpenSmartEdit(String),
    OpenSmartDelete { id: String, label: String },
    CloseSmartDelete,
    ConfirmSmartDelete,
    PlayPlaylist(String),
    RemovePlaylist(String),
    PlacePopover { x: f32, y: f32, width: f32, height: f32, viewport_w: f32, viewport_h: f32 },
    OpenBackend { plugin_id: String },
    SetBackendName(String),
    SetBackendUrl(String),
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
    match message {
        GapMessage::FolderHover { path, now_ms, dragging } => {
            model.sidebar.hover_folder(path, now_ms, dragging, locked);
        }
        GapMessage::FolderLeave(path) => model.sidebar.leave_folder(&path),
        GapMessage::OpenFolderCreate(parent) => model.sidebar.open_folder_create(parent, locked),
        GapMessage::OpenFolderRename { path, label } => model.sidebar.open_folder_rename(path, label, locked),
        GapMessage::SetFolderValue(value) => model.sidebar.folder_dialog.value = value,
        GapMessage::CloseFolderDialog => model.sidebar.close_folder_dialog(),
        GapMessage::SubmitFolderDialog => model.sidebar.submit_folder_dialog(repo_id.as_deref()),
        GapMessage::OpenFolderDelete { path, label } => model.sidebar.open_folder_delete(path, label, locked),
        GapMessage::CloseFolderDelete => model.sidebar.close_folder_delete(),
        GapMessage::ConfirmFolderDelete => model.sidebar.confirm_folder_delete(repo_id.as_deref()),
        GapMessage::OpenSmartEdit(id) => model.sidebar.open_smart_edit(&id, locked),
        GapMessage::OpenSmartDelete { id, label } => model.sidebar.open_smart_delete(id, label, locked),
        GapMessage::CloseSmartDelete => model.sidebar.close_smart_delete(),
        GapMessage::ConfirmSmartDelete => model.sidebar.confirm_smart_delete(repo_id.as_deref()),
        GapMessage::PlayPlaylist(id) => model.sidebar.play_playlist(id, locked),
        GapMessage::RemovePlaylist(id) => model.sidebar.remove_playlist(id, repo_id.as_deref(), locked),
        GapMessage::PlacePopover { x, y, width, height, viewport_w, viewport_h } => {
            let (x, y) = clamp_anchored(x, y, width, height, viewport_w, viewport_h);
            model.sidebar.popover_x = x;
            model.sidebar.popover_y = y;
        }
        GapMessage::OpenBackend { plugin_id } => model.sidebar.open_backend(plugin_id),
        GapMessage::SetBackendName(value) => model.sidebar.backend_name = value,
        GapMessage::SetBackendUrl(value) => model.sidebar.backend_url = value,
        GapMessage::SubmitBackend => model.sidebar.submit_backend(),
        GapMessage::Escape => {
            dismiss_top(model);
        }
    }
}

/// 关掉最上面的一层。没有弹层时返回 false。
pub fn dismiss_top(model: &mut super::super::ShellViewModel) -> bool {
    if model.input.pending_close {
        model.input.answer_close(false);
        return true;
    }
    if !model.sidebar.folder_delete_path.is_empty() {
        model.sidebar.close_folder_delete();
        return true;
    }
    if model.sidebar.folder_dialog.open {
        model.sidebar.close_folder_dialog();
        return true;
    }
    if !model.sidebar.smart_delete_id.is_empty() {
        model.sidebar.close_smart_delete();
        return true;
    }
    if model.sidebar.smart_draft.open {
        model.sidebar.close_smart_dialog();
        return true;
    }
    if model.playlist_dialog_open {
        model.playlist_dialog_open = false;
        return true;
    }
    if model.sidebar.popover != super::PopoverMode::Closed {
        model.sidebar.close_popover();
        return true;
    }
    if model.admin.popover_open {
        model.admin.popover_open = false;
        return true;
    }
    if model.inspect.tag_menu_open() {
        model.inspect.close_tag_menu();
        return true;
    }
    false
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

    fn hover_folder(&mut self, path: String, now_ms: u64, dragging: bool, locked: bool) {
        if locked || !dragging || path.is_empty() {
            return;
        }
        if self.hover_folder.as_deref() != Some(path.as_str()) {
            self.hover_folder = Some(path.clone());
            self.hover_since_ms = Some(now_ms);
            return;
        }
        let since = self.hover_since_ms.unwrap_or(now_ms);
        if now_ms.saturating_sub(since) >= super::super::motion::FOLDER_HOVER_MS {
            if !self.expanded_folders.iter().any(|item| item == &path) {
                self.expanded_folders.push(path.clone());
            }
            self.current_directory = path;
            self.hover_since_ms = None;
        }
    }

    fn leave_folder(&mut self, path: &str) {
        if self.hover_folder.as_deref() == Some(path) {
            self.hover_folder = None;
            self.hover_since_ms = None;
        }
    }

    fn open_folder_create(&mut self, parent: String, locked: bool) {
        if locked {
            eprintln!("Nana 当前不能新建文件夹");
            return;
        }
        self.folder_dialog = FolderDialog { open: true, parent, ..FolderDialog::default() };
    }

    fn open_folder_rename(&mut self, path: String, label: String, locked: bool) {
        if locked || path.is_empty() {
            eprintln!("Nana 当前不能重命名文件夹");
            return;
        }
        self.folder_dialog = FolderDialog { open: true, rename: true, target: path, value: label, ..FolderDialog::default() };
    }

    fn close_folder_dialog(&mut self) {
        if self.folder_dialog_busy() {
            return;
        }
        self.folder_dialog = FolderDialog::default();
    }

    fn folder_dialog_busy(&self) -> bool {
        false
    }

    fn submit_folder_dialog(&mut self, repo_id: Option<&str>) {
        let Some(repo_id) = repo_id.filter(|id| !id.is_empty()) else {
            self.folder_dialog.error = "先选择一个资源库。".into();
            return;
        };
        let name = self.folder_dialog.value.trim().to_string();
        if name.is_empty() {
            return;
        }
        if self.folder_dialog.rename {
            self.pending_folder_rename = Some((repo_id.to_string(), self.folder_dialog.target.clone(), name));
        } else {
            self.pending_folder_create = Some((repo_id.to_string(), self.folder_dialog.parent.clone(), name));
        }
        self.folder_dialog.open = false;
    }

    fn open_folder_delete(&mut self, path: String, label: String, locked: bool) {
        if locked || path.is_empty() {
            return;
        }
        self.folder_delete_path = path;
        self.folder_delete_label = label;
    }

    fn close_folder_delete(&mut self) {
        self.folder_delete_path.clear();
        self.folder_delete_label.clear();
    }

    fn confirm_folder_delete(&mut self, repo_id: Option<&str>) {
        let Some(repo_id) = repo_id.filter(|id| !id.is_empty()) else {
            return;
        };
        if self.folder_delete_path.is_empty() {
            return;
        }
        self.pending_folder_delete = Some((repo_id.to_string(), self.folder_delete_path.clone()));
        self.close_folder_delete();
    }

    fn open_smart_edit(&mut self, id: &str, locked: bool) {
        if locked {
            return;
        }
        let Some(folder) = find_smart(&self.smart_folders, id).cloned() else {
            eprintln!("Nana 找不到要编辑的智能文件夹：{id}");
            return;
        };
        self.smart_draft.open = true;
        self.smart_draft.mode_edit = true;
        self.smart_draft.target_id = folder.id;
        self.smart_draft.parent_id = folder.parent_id.unwrap_or_default();
        self.smart_draft.name = folder.name;
        self.smart_draft.query = folder.filter.query.unwrap_or_default();
        self.smart_draft.path_prefix = folder.filter.path_prefix.unwrap_or_default();
        self.smart_draft.formats = folder.filter.formats.unwrap_or_default().join("，");
        self.smart_draft.tags = folder.filter.tags.unwrap_or_default().join("，");
        self.smart_draft.match_mode = folder.filter.match_mode.unwrap_or_else(|| "and".into());
        self.smart_draft.error.clear();
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

    fn open_backend(&mut self, plugin_id: String) {
        if self.submitting {
            return;
        }
        self.backend_plugin_id = plugin_id;
        self.backend_name.clear();
        self.backend_url.clear();
        self.popover = super::PopoverMode::BackendForm;
        self.popover_error.clear();
    }

    fn submit_backend(&mut self) {
        if self.submitting {
            return;
        }
        let path = self.backend_url.trim().to_string();
        if path.is_empty() {
            return;
        }
        let name = if self.backend_name.trim().is_empty() {
            if self.backend_plugin_id == "momobako.source.eagle-library" { "Eagle Library".into() } else { "云盘".into() }
        } else {
            self.backend_name.trim().to_string()
        };
        self.submitting = true;
        self.effects.push(SidebarEffect::CreateBackendRepository {
            name,
            path,
            plugin_id: self.backend_plugin_id.clone(),
        });
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
        if let Some((repo_id, path)) = self.pending_folder_delete.take() {
            return Some(FolderMutation::Delete { repo_id, path });
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::{ShellMessage, ShellViewModel};
    use super::{GapMessage, FOLDER_CREATE_TITLE, FOLDER_RENAME_TITLE, SMART_EDIT_TITLE};
    use crate::shell::workspace::WorkspaceRepository;
    use crate::shell::SidebarMessage;

    fn shell() -> ShellViewModel {
        let mut model = ShellViewModel::default();
        model.repository_id = Some("repo".into());
        model.workspace.active_repo_id = Some("repo".into());
        model.workspace.repositories.push(WorkspaceRepository {
            repo_id: "repo".into(),
            name: "库".into(),
            path: "C:/repo".into(),
            status: "ready".into(),
            backend_plugin_id: "filesystem".into(),
            capabilities: vec!["write".into()],
            cache_required: false,
            cache_status: String::new(),
        });
        model.sidebar.bind_repository(Some("repo"), false);
        model
    }

    fn gap(model: &mut ShellViewModel, message: GapMessage) {
        model.reduce(ShellMessage::Sidebar(SidebarMessage::Gap(message)));
    }

    #[test]
    fn folder_dialogs_use_the_vue_titles_and_escape_closes_the_top_one() {
        let mut model = shell();
        gap(&mut model, GapMessage::OpenFolderCreate("photos".into()));
        assert_eq!(model.sidebar.folder_dialog.title(), FOLDER_CREATE_TITLE);
        assert!(model.sidebar.folder_dialog.open);
        gap(&mut model, GapMessage::SetFolderValue("  ".into()));
        gap(&mut model, GapMessage::SubmitFolderDialog);
        assert!(model.sidebar.folder_dialog.open);
        gap(&mut model, GapMessage::Escape);
        assert!(!model.sidebar.folder_dialog.open);
        gap(&mut model, GapMessage::OpenFolderRename { path: "photos".into(), label: "照片".into() });
        assert_eq!(model.sidebar.folder_dialog.title(), FOLDER_RENAME_TITLE);
        assert_eq!(model.sidebar.folder_dialog.value, "照片");
    }

    #[test]
    fn hover_opens_after_450ms_and_playlists_hide_when_the_repository_is_missing() {
        let mut model = shell();
        gap(&mut model, GapMessage::FolderHover { path: "photos".into(), now_ms: 0, dragging: true });
        gap(&mut model, GapMessage::FolderHover { path: "photos".into(), now_ms: 449, dragging: true });
        assert_eq!(model.sidebar.current_directory, "");
        gap(&mut model, GapMessage::FolderHover { path: "photos".into(), now_ms: 450, dragging: true });
        assert_eq!(model.sidebar.current_directory, "photos");
        assert!(model.sidebar.expanded_folders.iter().any(|path| path == "photos"));
        assert!(model.sidebar.playlists_visible(false));
        model.sidebar.bind_repository(Some("repo"), true);
        assert!(!model.sidebar.playlists_visible(true));
    }

    #[test]
    fn smart_edit_popover_clamp_and_playlist_play_match_vue() {
        let mut model = shell();
        model.sidebar.apply_smart_folders("repo", Ok(vec![super::super::SidebarSmartFolder {
            id: "sf".into(),
            parent_id: None,
            name: "高评分".into(),
            filter: Default::default(),
            children: Vec::new(),
        }]));
        gap(&mut model, GapMessage::OpenSmartEdit("sf".into()));
        assert!(model.sidebar.smart_draft.mode_edit);
        assert_eq!(model.sidebar.smart_draft.name, "高评分");
        assert_eq!(SMART_EDIT_TITLE, "编辑智能文件夹");
        gap(&mut model, GapMessage::OpenSmartDelete { id: "sf".into(), label: "高评分".into() });
        assert!(model.sidebar.smart_delete_open());
        assert_eq!(super::SMART_DELETE_TITLE, "删除智能文件夹");
        gap(&mut model, GapMessage::PlacePopover { x: -20.0, y: 900.0, width: 200.0, height: 100.0, viewport_w: 400.0, viewport_h: 300.0 });
        assert_eq!(model.sidebar.popover_x, 4.0);
        assert_eq!(model.sidebar.popover_y, 196.0);
        gap(&mut model, GapMessage::PlayPlaylist("pl-1".into()));
        assert_eq!(model.sidebar.pending_play.as_deref(), Some("pl-1"));
        gap(&mut model, GapMessage::OpenBackend { plugin_id: "momobako.cloud-drive".into() });
        assert_eq!(model.sidebar.popover, super::super::PopoverMode::BackendForm);
        gap(&mut model, GapMessage::SetBackendUrl("https://disk.example".into()));
        gap(&mut model, GapMessage::SubmitBackend);
        assert!(model.sidebar.submitting);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FolderMutation {
    Create { repo_id: String, parent: String, name: String },
    Rename { repo_id: String, path: String, name: String },
    Delete { repo_id: String, path: String },
}
