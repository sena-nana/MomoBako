//! 拖放、外部打开和关闭确认的归约。
//!
//! 服务请求只进入已有的移动、导入和附加效果。系统动作记到宿主请求里。

use crate::host_api::{HostInputRequest, HostRequest};
use crate::shell::admin::AdminMessage;

use super::support::{
    absolute_drag_paths, can_drag_entries, dropped_source_paths, filter_external_import_paths, internal_drag_distance,
    normalize_move_paths, resolve_drop_target, should_delegate_to_external_drag, DIALOG_EXPORT_ID, DIALOG_PLUGIN_ID,
    DIALOG_ATTACH_ID, DIALOG_RELOCATE_ID, EXTERNAL_DRAG_SWITCH_DISTANCE,
};
use super::{HostDragPhase, InputMessage, InputState, InternalSession};
use crate::shell::{ShellMessage, ShellViewModel, WindowAction, WorkspacePanel};

/// 为缺失资源库排队文件夹选择。没有活动仓库或正在处理时只记日志，不改页内路径框。
pub(crate) fn begin_relocate_dialog(model: &mut ShellViewModel) {
    if model.workspace.active_repo_id.is_none() || model.workspace.missing_busy() {
        eprintln!("Nana 当前不能打开重定向文件夹对话框");
        return;
    }
    model.workspace.missing_error.clear();
    model.input.queue_folder_dialog();
}

pub(crate) fn reduce_message(model: &mut ShellViewModel, message: ShellMessage) -> Option<ShellMessage> {
    match message {
        ShellMessage::WindowAction(WindowAction::Close) => {
            let dirty = model.close_is_dirty();
            let behavior = model.settings.close_behavior.clone();
            model.input.apply_close(super::support::decide_close(&behavior, dirty), dirty);
            None
        }
        ShellMessage::Input(message) => {
            reduce_input(model, message);
            None
        }
        ShellMessage::Sidebar(message) => {
            if let crate::shell::SidebarMessage::RepositoryAttachFinished(result) = &message {
                note_attach(model, result);
            }
            Some(ShellMessage::Sidebar(message))
        }
        other => Some(other),
    }
}

fn note_attach(model: &mut ShellViewModel, result: &Result<(), String>) {
    if !model.input.pending_attach {
        return;
    }
    model.input.pending_attach = false;
    match result {
        Ok(()) => model.input.empty_repository_error.clear(),
        Err(error) => {
            eprintln!("Nana 拖放附加资源库失败：{error}");
            model.input.empty_repository_error = error.clone();
        }
    }
}

fn reduce_input(model: &mut ShellViewModel, message: InputMessage) {
    match message {
        InputMessage::BeginEntryDrag {
            path,
            selected,
            x,
            y,
            writable,
            trash,
            smart_folder,
            backend_kind,
            repo_root,
            hover_folder,
            over_browser,
        } => begin_drag(model, &path, &selected, x, y, writable, trash, smart_folder, &backend_kind, &repo_root, hover_folder, over_browser),
        InputMessage::EntryDragMove { x, y, bounds_width, bounds_height, hover_folder, over_browser } => {
            drag_move(model, x, y, bounds_width, bounds_height, hover_folder, over_browser);
        }
        InputMessage::EntryDragEnd { hover_folder, over_browser, has_pointer } => {
            drag_end(model, hover_folder, over_browser, has_pointer);
        }
        InputMessage::WindowPointerLeave { x, y } => pointer_leave(model, x, y),
        InputMessage::WindowBlur => window_blur(model),
        InputMessage::DragOver { internal_transfer, writable, files_panel } => drag_over(model, internal_transfer, writable, files_panel),
        InputMessage::DragLeave { nested } => drag_leave(model, nested),
        InputMessage::BrowserDrop { internal_transfer, paths, writable, trash, has_snapshot, repo_root } => {
            browser_drop(model, internal_transfer, &paths, writable, trash, has_snapshot, &repo_root);
        }
        InputMessage::FolderHover { path, trash } => {
            if !trash {
                model.input.hover_folder = Some(path);
            }
        }
        InputMessage::FolderLeave(path) => {
            if model.input.hover_folder.as_deref() == Some(path.as_str()) {
                model.input.hover_folder = None;
            }
        }
        InputMessage::FolderDrop { path, internal_transfer, paths } => folder_drop(model, &path, internal_transfer, &paths),
        InputMessage::EmptyDragOver { has_active_id, has_repository } => {
            if has_active_id || has_repository {
                return;
            }
            model.input.drop_effect = "copy".into();
            model.input.dragging_repository_folder = true;
        }
        InputMessage::EmptyDragLeave { nested } => {
            if nested {
                return;
            }
            model.input.dragging_repository_folder = false;
        }
        InputMessage::EmptyDrop { has_active_id, has_repository, paths } => {
            model.input.dragging_repository_folder = false;
            if has_active_id || has_repository {
                return;
            }
            if let Some(path) = dropped_source_paths(&paths).into_iter().next() {
                attach_folder(model, &path);
            }
        }
        InputMessage::HostDrag {
            phase,
            paths,
            has_repository,
            missing_repository,
            writable,
            files_panel,
            has_snapshot,
            repo_root,
        } => host_drag(model, phase, &paths, has_repository, missing_repository, writable, files_panel, has_snapshot, &repo_root),
        InputMessage::BoxSelect { paths, append } => box_select(model, &paths, append),
        InputMessage::OpenEntry { has_repo, absolute_path } => open_entry(model, has_repo, &absolute_path),
        InputMessage::RevealEntry { absolute_path } => reveal_entry(model, &absolute_path),
        InputMessage::OpenExternalUrl { url } => open_url(model, &url),
        InputMessage::StartExternalDrag { paths, trash, backend_kind, repo_root } => {
            start_external_drag(&mut model.input, &paths, trash, &backend_kind, &repo_root);
        }
        InputMessage::ConfirmCloseAnswer(accept) => model.input.answer_close(accept),
        InputMessage::FileDialogCompleted { request_id, paths, failed } => complete_dialog(model, request_id, &paths, failed),
        InputMessage::ClearDrag => model.input.clear_flags(),
    }
}

fn begin_drag(
    model: &mut ShellViewModel,
    path: &str,
    selected: &[String],
    x: f32,
    y: f32,
    writable: bool,
    trash: bool,
    smart_folder: bool,
    backend_kind: &str,
    repo_root: &str,
    hover_folder: Option<String>,
    over_browser: bool,
) {
    if !can_drag_entries(writable, trash, smart_folder, backend_kind) {
        return;
    }
    let drag_paths = if selected.iter().any(|item| item == path) {
        selected.to_vec()
    } else {
        model.files.set_drag_selection(vec![path.to_string()], Some(path.to_string()), Some(path.to_string()));
        vec![path.to_string()]
    };
    model.input.dragged_paths = drag_paths.clone();
    model.input.internal_active = true;
    model.input.session = Some(InternalSession {
        start_x: x,
        start_y: y,
        last_x: x,
        last_y: y,
        paths: drag_paths,
        delegated: false,
        repo_root: repo_root.to_string(),
        backend_kind: backend_kind.to_string(),
    });
    store_hover(model, hover_folder, over_browser);
}

fn store_hover(model: &mut ShellViewModel, hover_folder: Option<String>, over_browser: bool) {
    let current = model.files.current_path.clone();
    model.input.hover_folder = resolve_drop_target(hover_folder.as_deref(), over_browser, &current);
}

fn drag_move(
    model: &mut ShellViewModel,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    hover_folder: Option<String>,
    over_browser: bool,
) {
    if !model.input.internal_active {
        return;
    }
    if let Some(session) = model.input.session.as_mut() {
        session.last_x = x;
        session.last_y = y;
    }
    store_hover(model, hover_folder, over_browser);
    let Some(session) = model.input.session.clone() else {
        return;
    };
    if session.delegated || session.paths.is_empty() {
        return;
    }
    if should_delegate_to_external_drag(session.start_x, session.start_y, session.last_x, session.last_y, x, y, width, height, EXTERNAL_DRAG_SWITCH_DISTANCE) {
        delegate(model, &session);
    }
}

fn delegate(model: &mut ShellViewModel, session: &InternalSession) {
    if let Some(active) = model.input.session.as_mut() {
        active.delegated = true;
    }
    let paths = session.paths.clone();
    let repo_root = session.repo_root.clone();
    let backend_kind = session.backend_kind.clone();
    model.input.finish_internal();
    start_external_drag(&mut model.input, &paths, false, &backend_kind, &repo_root);
}

fn drag_end(model: &mut ShellViewModel, hover_folder: Option<String>, over_browser: bool, has_pointer: bool) {
    let Some(session) = model.input.session.clone() else {
        model.input.finish_internal();
        return;
    };
    if session.delegated {
        model.input.finish_internal();
        return;
    }
    let target = if has_pointer {
        let current = model.files.current_path.clone();
        resolve_drop_target(hover_folder.as_deref(), over_browser, &current)
    } else {
        model.input.hover_folder.clone()
    };
    let Some(target) = target else {
        model.input.finish_internal();
        return;
    };
    move_paths(model, &target);
}

fn move_paths(model: &mut ShellViewModel, target: &str) {
    let sources = normalize_move_paths(&model.input.dragged_paths, target);
    let trash = model.workspace.panel == WorkspacePanel::Trash;
    model.input.finish_internal();
    if sources.is_empty() || trash {
        return;
    }
    let repo_id = model.workspace.active_repo_id.clone().unwrap_or_default();
    model.files.enqueue_move(repo_id, sources, target.to_string());
}

fn pointer_leave(model: &mut ShellViewModel, x: f32, y: f32) {
    let Some(session) = model.input.session.clone() else {
        return;
    };
    if session.delegated || !model.input.internal_active {
        return;
    }
    if let Some(active) = model.input.session.as_mut() {
        active.last_x = x;
        active.last_y = y;
    }
    let Some(session) = model.input.session.clone() else {
        return;
    };
    if internal_drag_distance(session.start_x, session.start_y, session.last_x, session.last_y) >= EXTERNAL_DRAG_SWITCH_DISTANCE {
        delegate(model, &session);
    }
}

fn window_blur(model: &mut ShellViewModel) {
    let Some(session) = model.input.session.clone() else {
        return;
    };
    if session.delegated || !model.input.internal_active {
        return;
    }
    if internal_drag_distance(session.start_x, session.start_y, session.last_x, session.last_y) >= EXTERNAL_DRAG_SWITCH_DISTANCE {
        delegate(model, &session);
    }
}

fn drag_over(model: &mut ShellViewModel, internal_transfer: bool, writable: bool, files_panel: bool) {
    if !writable || !files_panel {
        return;
    }
    if internal_transfer || model.input.internal_active {
        model.input.drop_effect = "move".into();
        return;
    }
    model.input.drop_effect = "copy".into();
    model.input.external_active = true;
    model.input.dragging_files = true;
}

fn drag_leave(model: &mut ShellViewModel, nested: bool) {
    if nested || model.input.internal_active {
        return;
    }
    model.input.external_active = false;
    model.input.hover_folder = None;
    model.input.dragging_files = false;
}

fn browser_drop(
    model: &mut ShellViewModel,
    internal_transfer: bool,
    paths: &[String],
    writable: bool,
    trash: bool,
    has_snapshot: bool,
    repo_root: &str,
) {
    if internal_transfer || model.input.internal_active {
        let target = model.input.hover_folder.clone().unwrap_or_else(|| model.files.current_path.clone());
        move_paths(model, &target);
        return;
    }
    model.input.external_active = false;
    model.input.dragging_files = false;
    if !writable || trash {
        return;
    }
    let sources = dropped_source_paths(paths);
    if sources.is_empty() {
        return;
    }
    import_external(model, &sources, has_snapshot, repo_root, None);
}

fn import_external(model: &mut ShellViewModel, paths: &[String], has_snapshot: bool, repo_root: &str, folder: Option<&str>) {
    model.input.external_active = false;
    let target = folder.map(str::to_string).or_else(|| model.input.hover_folder.clone()).unwrap_or_else(|| model.files.current_path.clone());
    model.input.hover_folder = None;
    if !has_snapshot {
        return;
    }
    let sources = if folder.is_some() {
        dropped_source_paths(paths)
    } else {
        filter_external_import_paths(paths, repo_root, &target)
    };
    if sources.is_empty() {
        return;
    }
    let parent = if target.is_empty() { None } else { Some(target) };
    let repo_id = model.workspace.active_repo_id.clone().unwrap_or_default();
    model.files.enqueue_import(repo_id, parent, sources);
}

fn folder_drop(model: &mut ShellViewModel, path: &str, internal_transfer: bool, paths: &[String]) {
    if internal_transfer || model.input.internal_active {
        move_paths(model, path);
        return;
    }
    let sources = dropped_source_paths(paths);
    if sources.is_empty() {
        return;
    }
    model.input.external_active = false;
    model.input.dragging_files = false;
    model.input.hover_folder = None;
    let parent = if path.is_empty() { None } else { Some(path.to_string()) };
    let repo_id = model.workspace.active_repo_id.clone().unwrap_or_default();
    model.files.enqueue_import(repo_id, parent, sources);
}

fn attach_folder(model: &mut ShellViewModel, path: &str) {
    let path = path.trim();
    if path.is_empty() {
        return;
    }
    model.input.empty_repository_error.clear();
    model.input.pending_attach = true;
    model.sidebar.queue_attach(path.to_string());
}

fn host_drag(
    model: &mut ShellViewModel,
    phase: HostDragPhase,
    paths: &[String],
    has_repository: bool,
    missing_repository: bool,
    writable: bool,
    files_panel: bool,
    has_snapshot: bool,
    repo_root: &str,
) {
    if !has_repository && !missing_repository {
        match phase {
            HostDragPhase::Enter | HostDragPhase::Over => model.input.dragging_repository_folder = true,
            HostDragPhase::Leave => model.input.dragging_repository_folder = false,
            HostDragPhase::Drop => {
                model.input.dragging_repository_folder = false;
                if let Some(path) = dropped_source_paths(paths).into_iter().next() {
                    attach_folder(model, &path);
                }
            }
        }
        return;
    }
    if !writable || !files_panel {
        return;
    }
    match phase {
        HostDragPhase::Enter | HostDragPhase::Over => {
            model.input.external_active = true;
            model.input.dragging_files = true;
        }
        HostDragPhase::Leave => {
            model.input.external_active = false;
            model.input.hover_folder = None;
            model.input.dragging_files = false;
        }
        HostDragPhase::Drop => {
            model.input.external_active = false;
            model.input.dragging_files = false;
            if paths.is_empty() {
                return;
            }
            import_external(model, paths, has_snapshot, repo_root, None);
        }
    }
}

fn box_select(model: &mut ShellViewModel, paths: &[String], append: bool) {
    if append {
        if paths.is_empty() {
            return;
        }
        let mut next = model.files.selected_paths().to_vec();
        for path in paths {
            if !next.iter().any(|item| item == path) {
                next.push(path.clone());
            }
        }
        let primary = model.files.primary.clone().or_else(|| paths.first().cloned());
        model.files.set_drag_selection(next, primary.clone(), primary);
        return;
    }
    if paths.is_empty() {
        model.files.set_drag_selection(Vec::new(), None, None);
        return;
    }
    let primary = paths.first().cloned();
    model.files.set_drag_selection(paths.to_vec(), primary.clone(), primary);
}

fn open_entry(model: &mut ShellViewModel, has_repo: bool, absolute_path: &str) {
    if !has_repo || absolute_path.trim().is_empty() {
        return;
    }
    model.input.error.clear();
    model.input.remember_external(absolute_path, false);
}

fn reveal_entry(model: &mut ShellViewModel, absolute_path: &str) {
    if absolute_path.trim().is_empty() {
        return;
    }
    model.input.error.clear();
    model.input.remember_external(absolute_path, true);
}

fn open_url(model: &mut ShellViewModel, url: &str) {
    if url.trim().is_empty() {
        return;
    }
    model.input.error.clear();
    model.input.remember_external(url, false);
}

fn start_external_drag(input: &mut InputState, paths: &[String], trash: bool, backend_kind: &str, repo_root: &str) {
    if trash || backend_kind != "filesystem" {
        input.external_drag_result = Some(false);
        return;
    }
    let absolute = absolute_drag_paths(paths, repo_root);
    if absolute.is_empty() {
        input.external_drag_result = Some(false);
        return;
    }
    input.error.clear();
    input.host_requests.push(HostRequest::Input(HostInputRequest::DragOut { paths: absolute }));
    eprintln!("Nana 宿主文件拖出尚未接通");
    input.error = "拖出失败：宿主文件拖出尚未接通".into();
    input.external_drag_result = Some(false);
}

fn complete_dialog(model: &mut ShellViewModel, request_id: u64, paths: &[String], failed: Option<String>) {
    if let Some(error) = failed {
        eprintln!("Nana 文件对话框失败：{error}");
        if request_id == DIALOG_EXPORT_ID {
            model.admin.external_message.clear();
            model.admin.external_error = format!("导出失败：{error}");
        } else if request_id == DIALOG_PLUGIN_ID {
            model.admin.action_message.clear();
            model.admin.action_error = format!("插件包选择失败：{error}");
        } else if request_id == DIALOG_RELOCATE_ID {
            model.workspace.missing_error = format!("文件夹选择失败：{error}");
        } else if request_id == DIALOG_ATTACH_ID {
            model.sidebar.popover_error = format!("文件夹选择失败：{error}");
        }
        return;
    }
    let path = paths.iter().find(|path| !path.trim().is_empty()).cloned();
    if request_id == DIALOG_EXPORT_ID {
        model.admin.external_message.clear();
        model.reduce(ShellMessage::Admin(AdminMessage::CompleteExport(path)));
        return;
    }
    if request_id == DIALOG_PLUGIN_ID {
        model.admin.action_message.clear();
        model.reduce(ShellMessage::Admin(AdminMessage::InstallArchive(path)));
        return;
    }
    if request_id == DIALOG_RELOCATE_ID {
        // 空白视为取消。非空交给 submit_missing_path，由它推 RelocateRepository。
        let Some(path) = path else {
            eprintln!("Nana 取消重定向文件夹选择");
            return;
        };
        model.workspace.set_path_draft(path);
        model.workspace.submit_missing_path();
        return;
    }
    if request_id == DIALOG_ATTACH_ID {
        let Some(path) = path else {
            eprintln!("Nana 取消添加资源库文件夹选择");
            return;
        };
        model.sidebar.set_attach_path(path);
        if !model.sidebar.submit_attach() {
            eprintln!("Nana 添加资源库文件夹没有提交");
        }
        return;
    }
    eprintln!("Nana 忽略未知的文件对话框：{request_id}");
}
