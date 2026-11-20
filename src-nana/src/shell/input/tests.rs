//! 宿主输入的分支回归。几何、拖放、外部打开和关闭确认都不依赖真窗口。

use super::super::{
    FilesEffect, ShellMessage, ShellPage, ShellViewModel, SidebarEffect, WorkspaceEffect,
    WorkspacePanel, WorkspaceRepository,
};
use super::support::{
    CloseDecision, DIALOG_ATTACH_ID, DIALOG_DOWNLOAD_ID, DIALOG_EXPORT_ID, DIALOG_PLUGIN_ID,
    DIALOG_RELOCATE_ID, DIALOG_REPOSITORY_EXPORT_ID, DIALOG_THUMBNAIL_ID,
    EXTERNAL_DRAG_SWITCH_DISTANCE, absolute_drag_paths, can_drag_entries, decide_close,
    filter_external_import_paths, internal_drag_distance, join_repository_path,
    normalize_filesystem_path, normalize_move_paths, normalize_workspace_path, resolve_drop_target,
    should_delegate_to_external_drag, workspace_parent_path,
};
use super::{HostDragPhase, InputMessage, InternalSession, begin_relocate_dialog, close_prompt};
use crate::host_api::{HostInputRequest, HostRequest};
use crate::shell::InspectMessage;
use crate::shell::status::FailureSource;
use nana_ui::FileDialogKind;

mod dialogs;
mod drags;
mod selection;

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
        cache_path: None,
        login_expired: false,
        logged_out: false,
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

/// 下载单曲在任务弹层里的进度行：动作名，「准备下载 夜曲」→「正在下载 夜曲」。
fn download_progress() -> crate::shell::entry_actions::DownloadProgress {
    crate::shell::entry_actions::DownloadProgress {
        label: "下载单曲".into(),
        subject: "夜曲".into(),
    }
}

fn send_admin(model: &mut ShellViewModel, message: crate::shell::admin::AdminMessage) {
    model.reduce(crate::shell::ShellMessage::Admin(message));
}
