//! 宿主输入的分支回归。几何、拖放、外部打开和关闭确认都不依赖真窗口。

use super::super::{
    FilesEffect, ShellMessage, ShellPage, ShellViewModel, SidebarEffect, WorkspaceEffect, WorkspacePanel, WorkspaceRepository,
};
use super::support::{
    absolute_drag_paths, can_drag_entries, decide_close, filter_external_import_paths, internal_drag_distance,
    join_repository_path, normalize_filesystem_path, normalize_move_paths, normalize_workspace_path, resolve_drop_target,
    should_delegate_to_external_drag, workspace_parent_path, CloseDecision, DIALOG_EXPORT_ID, DIALOG_PLUGIN_ID,
    DIALOG_ATTACH_ID, DIALOG_RELOCATE_ID, EXTERNAL_DRAG_SWITCH_DISTANCE,
};
use super::{begin_relocate_dialog, close_prompt, HostDragPhase, InputMessage, InternalSession};
use nana_ui::FileDialogKind;
use crate::host_api::{HostInputRequest, HostRequest};
use crate::shell::InspectMessage;

fn send(model: &mut ShellViewModel, message: InputMessage) {
    model.reduce(ShellMessage::Input(message));
}

fn ready(repo: &str) -> ShellViewModel {
    let mut model = ShellViewModel::default();
    model.workspace.active_repo_id = Some(repo.into());
    model.repository_id = Some(repo.into());
    model.files.current_path = "photos".into();
    model
}

fn begin(path: &str, selected: &[&str], x: f32) -> InputMessage {
    InputMessage::BeginEntryDrag {
        path: path.into(),
        selected: selected.iter().map(|item| (*item).to_string()).collect(),
        x,
        y: 10.0,
        writable: true,
        trash: false,
        smart_folder: false,
        backend_kind: "filesystem".into(),
        repo_root: "C:\\repo".into(),
        hover_folder: None,
        over_browser: true,
    }
}

#[test]
fn drag_geometry_filters_paths_and_self_drops() {
    assert_eq!(normalize_workspace_path(" /a\\b/ "), "a/b");
    assert_eq!(workspace_parent_path("a/b/c"), "a/b");
    assert_eq!(workspace_parent_path("a"), "");
    assert_eq!(internal_drag_distance(0.0, 0.0, 3.0, 4.0), 5.0);
    assert!(!should_delegate_to_external_drag(0.0, 0.0, 80.0, 0.0, 10.0, 10.0, 100.0, 100.0, EXTERNAL_DRAG_SWITCH_DISTANCE));
    assert!(!should_delegate_to_external_drag(0.0, 0.0, 10.0, 0.0, 0.0, 10.0, 100.0, 100.0, EXTERNAL_DRAG_SWITCH_DISTANCE));
    assert!(should_delegate_to_external_drag(0.0, 0.0, 72.0, 0.0, 100.0, 10.0, 100.0, 100.0, EXTERNAL_DRAG_SWITCH_DISTANCE));
    assert_eq!(resolve_drop_target(Some("albums"), false, "photos"), Some("albums".into()));
    assert_eq!(resolve_drop_target(None, true, "photos"), Some("photos".into()));
    assert_eq!(resolve_drop_target(None, false, "photos"), None);
    assert_eq!(
        normalize_move_paths(&[" ".into(), "a".into(), "b/c".into(), "b".into(), "b/c/d".into()], "b"),
        vec!["a".to_string(), "b/c/d".to_string()]
    );
    assert_eq!(join_repository_path("C:\\repo", "photos", Some("a.png")), "C:\\repo\\photos\\a.png");
    assert_eq!(normalize_filesystem_path("C:/Repo/a.png"), "c:\\repo\\a.png");
    assert_eq!(
        filter_external_import_paths(&["C:\\repo\\a.png".into(), "D:\\other\\b.png".into(), " ".into()], "C:\\repo", ""),
        vec!["D:\\other\\b.png".to_string()]
    );
    assert!(absolute_drag_paths(&[" ".into()], "").is_empty());
    assert_eq!(absolute_drag_paths(&["a.png".into()], "C:\\repo"), vec!["C:\\repo\\a.png".to_string()]);
    assert!(!can_drag_entries(true, true, false, "filesystem"));
    assert!(!can_drag_entries(true, false, false, "webdav"));
    assert!(can_drag_entries(true, false, false, "filesystem"));
    assert_eq!(decide_close("quit", false), CloseDecision::CloseNow);
    assert_eq!(decide_close("quit", true), CloseDecision::Ask { dirty: true });
    assert_eq!(decide_close("minimizeToTray", true), CloseDecision::HoldForTray);
    assert_eq!(decide_close("confirm", false), CloseDecision::Ask { dirty: false });
    assert_eq!(decide_close("other", false), CloseDecision::Ask { dirty: false });
}

#[test]
fn internal_drag_selects_moves_and_delegates_outside_the_window() {
    let mut model = ready("repo");
    send(&mut model, begin("solo.png", &["other.png"], 0.0));
    assert_eq!(model.files.selected_paths(), ["solo.png"]);
    assert_eq!(model.files.primary.as_deref(), Some("solo.png"));
    assert_eq!(model.input.hover_folder.as_deref(), Some("photos"));
    send(&mut model, InputMessage::EntryDragMove {
        x: 20.0,
        y: 10.0,
        bounds_width: 200.0,
        bounds_height: 200.0,
        hover_folder: Some("albums".into()),
        over_browser: false,
    });
    assert_eq!(model.input.hover_folder.as_deref(), Some("albums"));
    assert!(model.input.external_drag_result.is_none());
    send(&mut model, InputMessage::EntryDragEnd { hover_folder: Some("albums".into()), over_browser: false, has_pointer: true });
    assert!(matches!(model.files.take_effects().pop(), Some(FilesEffect::Move { parent, sources, .. }) if parent == "albums" && sources == ["solo.png"]));
    assert!(!model.input.internal_active);

    let mut model = ready("repo");
    send(&mut model, begin("keep.png", &["keep.png", "b/c.png"], 0.0));
    assert_eq!(model.input.dragged_paths, ["keep.png", "b/c.png"]);
    model.workspace.panel = WorkspacePanel::Trash;
    send(&mut model, InputMessage::EntryDragEnd { hover_folder: Some("albums".into()), over_browser: false, has_pointer: true });
    assert!(model.files.take_effects().is_empty());

    let mut model = ready("repo");
    send(&mut model, begin("photos/a.png", &["photos/a.png"], 0.0));
    send(&mut model, InputMessage::EntryDragEnd { hover_folder: Some("photos".into()), over_browser: true, has_pointer: true });
    assert!(model.files.take_effects().is_empty());

    let mut model = ready("repo");
    send(&mut model, begin("a.png", &["a.png"], 0.0));
    send(&mut model, InputMessage::EntryDragMove {
        x: 100.0,
        y: 10.0,
        bounds_width: 100.0,
        bounds_height: 100.0,
        hover_folder: None,
        over_browser: false,
    });
    assert_eq!(model.input.external_drag_result, Some(false));
    assert_eq!(model.input.error, "拖出失败：宿主文件拖出尚未接通");
    assert!(matches!(model.input.host_requests.last(), Some(HostRequest::Input(HostInputRequest::DragOut { paths })) if paths == &["C:\\repo\\a.png".to_string()]));
    assert!(model.input.session.is_none());
    send(&mut model, InputMessage::EntryDragEnd { hover_folder: Some("albums".into()), over_browser: true, has_pointer: true });
    assert!(model.files.take_effects().is_empty());
}

#[test]
fn refused_drags_pointer_leave_and_delegated_end_do_not_move() {
    let mut model = ready("repo");
    send(&mut model, InputMessage::BeginEntryDrag {
        path: "a.png".into(),
        selected: vec!["a.png".into()],
        x: 0.0,
        y: 0.0,
        writable: false,
        trash: false,
        smart_folder: false,
        backend_kind: "filesystem".into(),
        repo_root: "C:\\repo".into(),
        hover_folder: None,
        over_browser: true,
    });
    assert!(model.input.session.is_none());
    send(&mut model, InputMessage::BeginEntryDrag {
        path: "a.png".into(),
        selected: vec!["a.png".into()],
        x: 0.0,
        y: 0.0,
        writable: true,
        trash: false,
        smart_folder: true,
        backend_kind: "filesystem".into(),
        repo_root: "C:\\repo".into(),
        hover_folder: None,
        over_browser: true,
    });
    assert!(model.input.session.is_none());

    send(&mut model, begin("a.png", &["a.png"], 0.0));
    send(&mut model, InputMessage::WindowPointerLeave { x: 10.0, y: 0.0 });
    assert!(model.input.session.is_some());
    send(&mut model, InputMessage::WindowPointerLeave { x: 72.0, y: 0.0 });
    assert!(model.input.session.is_none());
    assert_eq!(model.input.external_drag_result, Some(false));

    let mut model = ready("repo");
    send(&mut model, begin("a.png", &["a.png"], 0.0));
    send(&mut model, InputMessage::EntryDragMove { x: 10.0, y: 0.0, bounds_width: 400.0, bounds_height: 400.0, hover_folder: None, over_browser: true });
    send(&mut model, InputMessage::WindowBlur);
    assert!(model.input.session.is_some());
    send(&mut model, InputMessage::EntryDragMove { x: 80.0, y: 0.0, bounds_width: 400.0, bounds_height: 400.0, hover_folder: None, over_browser: true });
    model.input.session = Some(InternalSession {
        start_x: 0.0,
        start_y: 0.0,
        last_x: 80.0,
        last_y: 0.0,
        paths: vec!["a.png".into()],
        delegated: true,
        repo_root: "C:\\repo".into(),
        backend_kind: "filesystem".into(),
    });
    model.input.internal_active = true;
    model.input.dragged_paths = vec!["a.png".into()];
    send(&mut model, InputMessage::WindowBlur);
    assert!(model.input.session.is_some());
    send(&mut model, InputMessage::EntryDragEnd { hover_folder: Some("albums".into()), over_browser: true, has_pointer: false });
    assert!(model.files.take_effects().is_empty());
    assert!(model.input.session.is_none());

    send(&mut model, InputMessage::EntryDragEnd { hover_folder: None, over_browser: false, has_pointer: true });
    assert!(model.files.take_effects().is_empty());
}

#[test]
fn html_and_host_drops_import_move_or_attach() {
    let mut model = ready("repo");
    model.input.drop_effect = "keep".into();
    send(&mut model, InputMessage::DragOver { internal_transfer: false, writable: false, files_panel: true });
    assert_eq!(model.input.drop_effect, "keep");
    send(&mut model, InputMessage::DragOver { internal_transfer: true, writable: true, files_panel: true });
    assert_eq!(model.input.drop_effect, "move");
    send(&mut model, InputMessage::DragOver { internal_transfer: false, writable: true, files_panel: true });
    assert_eq!(model.input.drop_effect, "copy");
    assert!(model.input.external_active && model.input.dragging_files);
    send(&mut model, InputMessage::DragLeave { nested: true });
    assert!(model.input.external_active);
    model.input.internal_active = true;
    send(&mut model, InputMessage::DragLeave { nested: false });
    assert!(model.input.external_active);
    model.input.internal_active = false;
    send(&mut model, InputMessage::DragLeave { nested: false });
    assert!(!model.input.external_active);
    assert!(model.input.hover_folder.is_none());

    send(&mut model, begin("a.png", &["a.png"], 0.0));
    model.input.hover_folder = None;
    send(&mut model, InputMessage::BrowserDrop {
        internal_transfer: true,
        paths: Vec::new(),
        writable: true,
        trash: false,
        has_snapshot: true,
        repo_root: "C:\\repo".into(),
    });
    assert!(matches!(model.files.take_effects().pop(), Some(FilesEffect::Move { parent, .. }) if parent == "photos"));

    send(&mut model, InputMessage::BrowserDrop {
        internal_transfer: false,
        paths: vec!["C:\\repo\\a.png".into(), "D:\\other\\b.png".into()],
        writable: false,
        trash: false,
        has_snapshot: true,
        repo_root: "C:\\repo".into(),
    });
    assert!(model.files.take_effects().is_empty());
    send(&mut model, InputMessage::BrowserDrop {
        internal_transfer: false,
        paths: vec!["D:\\other\\b.png".into()],
        writable: true,
        trash: true,
        has_snapshot: true,
        repo_root: "C:\\repo".into(),
    });
    assert!(model.files.take_effects().is_empty());
    send(&mut model, InputMessage::BrowserDrop {
        internal_transfer: false,
        paths: vec![" ".into()],
        writable: true,
        trash: false,
        has_snapshot: true,
        repo_root: "C:\\repo".into(),
    });
    assert!(model.files.take_effects().is_empty());
    send(&mut model, InputMessage::BrowserDrop {
        internal_transfer: false,
        paths: vec!["C:\\repo\\photos\\a.png".into(), "D:\\other\\b.png".into()],
        writable: true,
        trash: false,
        has_snapshot: false,
        repo_root: "C:\\repo".into(),
    });
    assert!(model.files.take_effects().is_empty());
    model.input.hover_folder = Some("albums".into());
    send(&mut model, InputMessage::BrowserDrop {
        internal_transfer: false,
        paths: vec!["C:\\repo\\albums\\a.png".into(), "D:\\other\\b.png".into()],
        writable: true,
        trash: false,
        has_snapshot: true,
        repo_root: "C:\\repo".into(),
    });
    assert!(matches!(
        model.files.take_effects().pop(),
        Some(FilesEffect::Import { parent, sources, .. }) if parent.as_deref() == Some("albums") && sources == ["D:\\other\\b.png"]
    ));

    model.workspace.panel = WorkspacePanel::Trash;
    send(&mut model, InputMessage::FolderHover { path: "albums".into(), trash: true });
    assert!(model.input.hover_folder.is_none());
    model.workspace.panel = WorkspacePanel::Files;
    send(&mut model, InputMessage::FolderHover { path: "albums".into(), trash: false });
    send(&mut model, InputMessage::FolderLeave("other".into()));
    assert_eq!(model.input.hover_folder.as_deref(), Some("albums"));
    send(&mut model, InputMessage::FolderLeave("albums".into()));
    assert!(model.input.hover_folder.is_none());
    send(&mut model, InputMessage::FolderDrop { path: "albums".into(), internal_transfer: false, paths: vec![" ".into()] });
    assert!(model.files.take_effects().is_empty());
    send(&mut model, begin("a.png", &["a.png"], 0.0));
    model.workspace.panel = WorkspacePanel::Trash;
    send(&mut model, InputMessage::FolderDrop { path: "albums".into(), internal_transfer: true, paths: Vec::new() });
    assert!(model.files.take_effects().is_empty());
    model.workspace.panel = WorkspacePanel::Files;
    send(&mut model, InputMessage::FolderDrop { path: "albums".into(), internal_transfer: false, paths: vec!["D:\\other\\a.png".into()] });
    assert!(matches!(
        model.files.take_effects().pop(),
        Some(FilesEffect::Import { parent, sources, .. }) if parent.as_deref() == Some("albums") && sources == ["D:\\other\\a.png"]
    ));

    send(&mut model, InputMessage::EmptyDragOver { has_active_id: true, has_repository: false });
    assert!(!model.input.dragging_repository_folder);
    send(&mut model, InputMessage::EmptyDragOver { has_active_id: false, has_repository: false });
    assert!(model.input.dragging_repository_folder);
    assert_eq!(model.input.drop_effect, "copy");
    send(&mut model, InputMessage::EmptyDragLeave { nested: true });
    assert!(model.input.dragging_repository_folder);
    send(&mut model, InputMessage::EmptyDragLeave { nested: false });
    assert!(!model.input.dragging_repository_folder);
    send(&mut model, InputMessage::EmptyDrop { has_active_id: true, has_repository: false, paths: vec!["C:\\new".into()] });
    assert!(model.sidebar.take_effects().is_empty());
    send(&mut model, InputMessage::EmptyDrop { has_active_id: false, has_repository: false, paths: vec![" ".into()] });
    assert!(model.sidebar.take_effects().is_empty());
    send(&mut model, InputMessage::EmptyDrop { has_active_id: false, has_repository: false, paths: vec![" C:\\new ".into(), "C:\\other".into()] });
    assert!(matches!(model.sidebar.take_effects().pop(), Some(SidebarEffect::AttachRepository { path }) if path == "C:\\new"));
    assert!(model.input.pending_attach);
    model.reduce(ShellMessage::Sidebar(crate::shell::SidebarMessage::RepositoryAttachFinished(Err("附加失败".into()))));
    assert_eq!(model.input.empty_repository_error, "附加失败");
    assert!(!model.input.pending_attach);
    model.reduce(ShellMessage::Sidebar(crate::shell::SidebarMessage::RepositoryAttachFinished(Err("忽略".into()))));
    assert_eq!(model.input.empty_repository_error, "附加失败");

    model.input.dragging_files = true;
    send(&mut model, InputMessage::HostDrag {
        phase: HostDragPhase::Enter,
        paths: Vec::new(),
        has_repository: false,
        missing_repository: false,
        writable: false,
        files_panel: false,
        has_snapshot: false,
        repo_root: String::new(),
    });
    assert!(model.input.dragging_repository_folder);
    assert!(model.input.dragging_files);
    model.input.dragging_files = false;
    send(&mut model, InputMessage::HostDrag {
        phase: HostDragPhase::Leave,
        paths: Vec::new(),
        has_repository: false,
        missing_repository: false,
        writable: false,
        files_panel: false,
        has_snapshot: false,
        repo_root: String::new(),
    });
    assert!(!model.input.dragging_repository_folder);
    send(&mut model, InputMessage::HostDrag {
        phase: HostDragPhase::Drop,
        paths: vec!["C:\\library".into()],
        has_repository: false,
        missing_repository: true,
        writable: false,
        files_panel: true,
        has_snapshot: false,
        repo_root: String::new(),
    });
    assert!(model.sidebar.take_effects().is_empty());
    send(&mut model, InputMessage::HostDrag {
        phase: HostDragPhase::Over,
        paths: Vec::new(),
        has_repository: true,
        missing_repository: false,
        writable: false,
        files_panel: true,
        has_snapshot: true,
        repo_root: "C:\\repo".into(),
    });
    assert!(!model.input.dragging_files);
    send(&mut model, InputMessage::HostDrag {
        phase: HostDragPhase::Enter,
        paths: Vec::new(),
        has_repository: true,
        missing_repository: false,
        writable: true,
        files_panel: true,
        has_snapshot: true,
        repo_root: "C:\\repo".into(),
    });
    assert!(model.input.dragging_files);
    send(&mut model, InputMessage::HostDrag {
        phase: HostDragPhase::Leave,
        paths: Vec::new(),
        has_repository: true,
        missing_repository: false,
        writable: true,
        files_panel: true,
        has_snapshot: true,
        repo_root: "C:\\repo".into(),
    });
    assert!(!model.input.external_active);
    model.input.hover_folder = Some("photos".into());
    send(&mut model, InputMessage::HostDrag {
        phase: HostDragPhase::Drop,
        paths: vec!["D:\\other\\c.png".into()],
        has_repository: true,
        missing_repository: false,
        writable: true,
        files_panel: false,
        has_snapshot: true,
        repo_root: "C:\\repo".into(),
    });
    assert!(model.files.take_effects().is_empty());
    assert_eq!(model.input.hover_folder.as_deref(), Some("photos"));
    send(&mut model, InputMessage::HostDrag {
        phase: HostDragPhase::Drop,
        paths: Vec::new(),
        has_repository: true,
        missing_repository: false,
        writable: true,
        files_panel: true,
        has_snapshot: true,
        repo_root: "C:\\repo".into(),
    });
    assert!(model.files.take_effects().is_empty());
    send(&mut model, InputMessage::HostDrag {
        phase: HostDragPhase::Drop,
        paths: vec!["D:\\other\\c.png".into()],
        has_repository: true,
        missing_repository: false,
        writable: true,
        files_panel: true,
        has_snapshot: true,
        repo_root: "C:\\repo".into(),
    });
    assert!(matches!(model.files.take_effects().pop(), Some(FilesEffect::Import { sources, .. }) if sources == ["D:\\other\\c.png"]));
}

#[test]
fn box_selection_open_reveal_and_external_drag_follow_the_vue_guards() {
    let mut model = ready("repo");
    model.files.set_drag_selection(vec!["a.png".into()], Some("a.png".into()), Some("old".into()));
    send(&mut model, InputMessage::BoxSelect { paths: Vec::new(), append: true });
    assert_eq!(model.files.selected_paths(), ["a.png"]);
    send(&mut model, InputMessage::BoxSelect { paths: vec!["a.png".into(), "b.png".into()], append: true });
    assert_eq!(model.files.selected_paths(), ["a.png", "b.png"]);
    assert_eq!(model.files.primary.as_deref(), Some("a.png"));
    assert_eq!(model.files.anchor.as_deref(), Some("a.png"));
    send(&mut model, InputMessage::BoxSelect { paths: Vec::new(), append: false });
    assert!(model.files.selected_paths().is_empty());
    assert!(model.files.primary.is_none());
    send(&mut model, InputMessage::BoxSelect { paths: vec!["c.png".into(), "d.png".into()], append: false });
    assert_eq!(model.files.primary.as_deref(), Some("c.png"));
    assert_eq!(model.files.anchor.as_deref(), Some("c.png"));

    let before = model.input.error.clone();
    send(&mut model, InputMessage::OpenEntry { has_repo: false, absolute_path: "C:\\a.png".into() });
    send(&mut model, InputMessage::OpenEntry { has_repo: true, absolute_path: " ".into() });
    send(&mut model, InputMessage::RevealEntry { absolute_path: " ".into() });
    send(&mut model, InputMessage::OpenExternalUrl { url: " ".into() });
    assert_eq!(model.input.error, before);
    assert!(model.input.host_requests.is_empty());
    send(&mut model, InputMessage::OpenEntry { has_repo: true, absolute_path: "C:\\a.png".into() });
    assert!(model.input.error.is_empty());
    send(&mut model, InputMessage::RevealEntry { absolute_path: "C:\\a.png".into() });
    assert!(matches!(model.input.host_requests.last(), Some(crate::host_api::HostRequest::OpenExternal(request)) if request.reveal && request.target == "C:\\a.png"));
    send(&mut model, InputMessage::OpenExternalUrl { url: "https://momobako.local".into() });
    assert!(model.input.error.is_empty());
    assert_eq!(model.input.host_requests.len(), 3);

    send(&mut model, InputMessage::StartExternalDrag { paths: vec!["a.png".into()], trash: true, backend_kind: "filesystem".into(), repo_root: "C:\\repo".into() });
    assert_eq!(model.input.external_drag_result, Some(false));
    assert_eq!(model.input.host_requests.len(), 3);
    send(&mut model, InputMessage::StartExternalDrag { paths: vec!["a.png".into()], trash: false, backend_kind: "webdav".into(), repo_root: "C:\\repo".into() });
    send(&mut model, InputMessage::StartExternalDrag { paths: vec![" ".into()], trash: false, backend_kind: "filesystem".into(), repo_root: "C:\\repo".into() });
    assert_eq!(model.input.host_requests.len(), 3);
    assert!(model.input.error.is_empty());
    send(&mut model, InputMessage::StartExternalDrag { paths: vec!["a.png".into()], trash: false, backend_kind: "filesystem".into(), repo_root: "C:\\repo".into() });
    assert_eq!(model.input.error, "拖出失败：宿主文件拖出尚未接通");
    assert_eq!(model.input.external_drag_result, Some(false));

    model.input.external_active = true;
    model.input.dragging_files = true;
    model.input.dragging_repository_folder = true;
    send(&mut model, InputMessage::ClearDrag);
    assert!(!model.input.external_active);
    assert!(!model.input.dragging_files);
    assert!(!model.input.dragging_repository_folder);
    assert_eq!(model.input.error, "拖出失败：宿主文件拖出尚未接通");
}

#[test]
fn close_confirmation_and_file_dialogs_do_not_pretend_the_host_finished() {
    let mut model = ShellViewModel::default();
    model.settings.close_behavior = "confirm".into();
    model.reduce(ShellMessage::WindowAction(crate::shell::WindowAction::Close));
    assert!(model.input.pending_close);
    assert_eq!(model.input.notice, "确认关闭 MomoBako？");
    assert!(matches!(model.input.host_requests.last(), Some(HostRequest::Input(HostInputRequest::ConfirmClose { dirty: false }))));
    model.reduce(ShellMessage::WindowAction(crate::shell::WindowAction::Close));
    assert_eq!(model.input.host_requests.len(), 1);
    assert!(close_prompt(&model).is_some());
    send(&mut model, InputMessage::ConfirmCloseAnswer(false));
    assert!(!model.input.pending_close);
    assert!(model.input.notice.is_empty());
    assert!(close_prompt(&model).is_none());
    send(&mut model, InputMessage::ConfirmCloseAnswer(true));
    assert_eq!(model.input.notice, "");

    model.settings.close_behavior = "quit".into();
    model.reduce(ShellMessage::WindowAction(crate::shell::WindowAction::Close));
    assert!(!model.input.pending_close);
    let commands = model.input.take_platform_commands(nana_ui_platform::WindowId(1), false);
    assert!(matches!(commands.first(), Some(nana_ui_platform::host::WindowCommand::Close(_))));

    model.dirty = true;
    model.reduce(ShellMessage::WindowAction(crate::shell::WindowAction::Close));
    assert!(matches!(model.input.host_requests.last(), Some(HostRequest::Input(HostInputRequest::ConfirmClose { dirty: true }))));
    assert_eq!(model.input.notice, "有未保存的修改，确认关闭？");
    send(&mut model, InputMessage::ConfirmCloseAnswer(true));
    assert!(!model.input.take_platform_commands(nana_ui_platform::WindowId(1), false).is_empty());

    model.reduce(ShellMessage::Inspect(InspectMessage::SetComment("未保存".into())));
    model.dirty = false;
    assert!(model.close_is_dirty());
    model.settings.close_behavior = "quit".into();
    model.reduce(ShellMessage::WindowAction(crate::shell::WindowAction::Close));
    assert!(model.input.pending_close);

    model.settings.close_behavior = "minimizeToTray".into();
    model.reduce(ShellMessage::WindowAction(crate::shell::WindowAction::Close));
    assert!(!model.input.pending_close);
    assert_eq!(model.input.notice, "最小化到托盘尚未接通");
    assert!(matches!(model.input.host_requests.last(), Some(HostRequest::Input(HostInputRequest::MinimizeToTray))));
    assert!(model.input.take_platform_commands(nana_ui_platform::WindowId(1), false).is_empty());

    let acceptance = ShellViewModel::for_page(ShellPage::Settings);
    assert!(close_prompt(&acceptance).is_none());

    model.admin.external_message = "正在选择导出位置…".into();
    send(&mut model, InputMessage::FileDialogCompleted { request_id: DIALOG_EXPORT_ID, paths: Vec::new(), failed: None });
    assert!(model.admin.external_message.is_empty());
    assert!(model.admin.take_effects().is_empty());
    send(&mut model, InputMessage::FileDialogCompleted { request_id: DIALOG_EXPORT_ID, paths: Vec::new(), failed: Some("Busy".into()) });
    assert_eq!(model.admin.external_error, "导出失败：Busy");
    send(&mut model, InputMessage::FileDialogCompleted { request_id: 99, paths: vec!["x".into()], failed: None });
    model.input.queue_save_dialog();
    model.input.queue_plugin_dialog();
    let commands = model.input.take_platform_commands(nana_ui_platform::WindowId(1), false);
    assert_eq!(commands.len(), 2);
    assert!(matches!(&commands[0], nana_ui_platform::host::WindowCommand::OpenFileDialog { request, .. } if request.id == DIALOG_EXPORT_ID && request.file_name.as_deref() == Some("external-api.json")));
    assert!(matches!(&commands[1], nana_ui_platform::host::WindowCommand::OpenFileDialog { request, .. } if request.id == DIALOG_PLUGIN_ID));
    send(&mut model, InputMessage::FileDialogCompleted { request_id: DIALOG_PLUGIN_ID, paths: vec![" ".into()], failed: None });
    assert!(model.admin.take_effects().is_empty());
    send(&mut model, InputMessage::FileDialogCompleted { request_id: DIALOG_PLUGIN_ID, paths: vec!["plugin.momoplug".into()], failed: None });
    assert!(matches!(model.admin.take_effects().pop(), Some(crate::shell::admin::AdminEffect::Install(path)) if path == "plugin.momoplug"));
    send(&mut model, InputMessage::FileDialogCompleted { request_id: DIALOG_PLUGIN_ID, paths: Vec::new(), failed: Some("Unavailable".into()) });
    assert_eq!(model.admin.action_error, "插件包选择失败：Unavailable");
}

fn missing_repository(id: &str) -> WorkspaceRepository {
    WorkspaceRepository {
        repo_id: id.into(),
        name: id.into(),
        path: format!("C:/{id}"),
        status: "missing".into(),
        backend_plugin_id: "filesystem".into(),
        capabilities: vec!["localRootPath".into()],
        cache_required: false,
        cache_status: String::new(),
    }
}

/// 启动已完成、有活动仓库，且该仓库状态为 missing。
fn finished_missing(id: &str) -> ShellViewModel {
    let mut model = ShellViewModel::default();
    model.workspace.startup.finish();
    model.workspace.active_repo_id = Some(id.into());
    model.workspace.repositories.push(missing_repository(id));
    model
}

#[test]
fn relocate_folder_dialog_queues_pick_folder_and_submits_only_a_chosen_path() {
    let mut model = finished_missing("repo");
    model.workspace.missing_error = "旧错误".into();
    begin_relocate_dialog(&mut model);
    assert!(!model.workspace.path_prompt);
    assert!(!model.workspace.relocating);
    assert!(model.workspace.missing_error.is_empty());
    let commands = model.input.take_platform_commands(nana_ui_platform::WindowId(1), false);
    assert_eq!(commands.len(), 1);
    assert!(matches!(
        &commands[0],
        nana_ui_platform::host::WindowCommand::OpenFileDialog { request, .. }
            if request.id == DIALOG_RELOCATE_ID
                && request.kind == FileDialogKind::PickFolder
                && request.title.as_deref() == Some("重定向资源库位置")
    ));

    let mut idle = ShellViewModel::default();
    idle.workspace.startup.finish();
    idle.workspace.repositories.push(missing_repository("repo"));
    begin_relocate_dialog(&mut idle);
    assert!(idle.input.take_platform_commands(nana_ui_platform::WindowId(1), false).is_empty());

    let mut busy = finished_missing("repo");
    busy.workspace.relocating = true;
    begin_relocate_dialog(&mut busy);
    assert!(busy.workspace.relocating);
    assert!(busy.input.take_platform_commands(nana_ui_platform::WindowId(1), false).is_empty());

    let mut cancelled = finished_missing("repo");
    cancelled.workspace.path_draft = "C:/old".into();
    send(&mut cancelled, InputMessage::FileDialogCompleted { request_id: DIALOG_RELOCATE_ID, paths: Vec::new(), failed: None });
    assert!(!cancelled.workspace.relocating);
    assert!(!cancelled.workspace.take_effects().iter().any(|effect| matches!(effect, WorkspaceEffect::RelocateRepository { .. })));
    send(
        &mut cancelled,
        InputMessage::FileDialogCompleted { request_id: DIALOG_RELOCATE_ID, paths: vec![" ".into()], failed: None },
    );
    assert!(!cancelled.workspace.relocating);
    assert!(cancelled.workspace.take_effects().is_empty());

    let mut failed = finished_missing("repo");
    failed.workspace.path_draft = "C:/old".into();
    send(
        &mut failed,
        InputMessage::FileDialogCompleted { request_id: DIALOG_RELOCATE_ID, paths: vec!["D:/library".into()], failed: Some("Busy".into()) },
    );
    assert_eq!(failed.workspace.missing_error, "文件夹选择失败：Busy");
    assert!(!failed.workspace.relocating);
    assert!(failed.workspace.take_effects().is_empty());

    let mut chosen = finished_missing("repo");
    send(
        &mut chosen,
        InputMessage::FileDialogCompleted { request_id: DIALOG_RELOCATE_ID, paths: vec!["D:/library".into()], failed: None },
    );
    assert!(chosen.workspace.relocating);
    assert!(matches!(
        chosen.workspace.take_effects().as_slice(),
        [WorkspaceEffect::RelocateRepository { repo_id, path }] if repo_id == "repo" && path == "D:/library"
    ));
}

#[test]
fn attach_folder_dialog_submits_a_chosen_path_and_ignores_cancel() {
    let mut model = ShellViewModel::default();
    model.reduce(ShellMessage::Sidebar(super::super::SidebarMessage::ShowRepositoryAddMenu));
    let commands = model.input.take_platform_commands(nana_ui_platform::WindowId(1), false);
    assert!(matches!(
        commands.as_slice(),
        [nana_ui_platform::host::WindowCommand::OpenFileDialog { request, .. }]
            if request.id == DIALOG_ATTACH_ID
                && request.kind == FileDialogKind::PickFolder
                && request.title.as_deref() == Some("添加资源库")
    ));

    send(&mut model, InputMessage::FileDialogCompleted { request_id: DIALOG_ATTACH_ID, paths: Vec::new(), failed: None });
    assert!(model.sidebar.take_effects().is_empty());

    send(
        &mut model,
        InputMessage::FileDialogCompleted { request_id: DIALOG_ATTACH_ID, paths: vec!["D:/library".into()], failed: Some("Busy".into()) },
    );
    assert_eq!(model.sidebar.popover_error, "文件夹选择失败：Busy");
    assert!(model.sidebar.take_effects().is_empty());

    send(
        &mut model,
        InputMessage::FileDialogCompleted { request_id: DIALOG_ATTACH_ID, paths: vec!["D:/library".into()], failed: None },
    );
    assert!(matches!(
        model.sidebar.take_effects().as_slice(),
        [SidebarEffect::AttachRepository { path }] if path == "D:/library"
    ));
}
