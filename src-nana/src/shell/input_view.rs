//! 关闭确认和系统文件拖放。空库页在 `route_empty.rs`。验收场景不挂关闭条，避免改掉旧的命中目标。

use std::path::PathBuf;

use nana_ui::runtime::view::{on_mount, text, widget, AnyView, IntoView, NodeRef};
use nana_ui::runtime::{Activate, ConfirmDialog, FileDropEvent};
use nana_ui_core::{DropAccepts, DropEffect};
use super::{HostDragPhase, InputMessage};
use crate::shell::{MainRegion, ShellMessage, ShellViewModel, WorkspacePanel};

/// 挂上拖放时从壳层抄下来的仓库条件。事件回调里不再借壳层；常驻视图把它放进信号，事件到达时现读。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FileDropFlags {
    has_repository: bool,
    missing_repository: bool,
    writable: bool,
    files_panel: bool,
    has_snapshot: bool,
    repo_root: String,
}

pub(crate) fn file_drop_flags(model: &ShellViewModel) -> FileDropFlags {
    let repository = model.workspace.active_repository();
    FileDropFlags {
        has_repository: model.workspace.active_repo_id.is_some(),
        missing_repository: model.navigation_locked(),
        writable: repository.as_ref().is_some_and(|item| {
            crate::shell::files::repository_is_writable(&item.status, &item.capabilities)
        }),
        files_panel: model.workspace.panel == WorkspacePanel::Files,
        has_snapshot: model.workspace.main_region() == MainRegion::HasRepository,
        repo_root: repository.map(|item| item.path.clone()).unwrap_or_default(),
    }
}

/// 把 Nana 的文件拖放收成和 Vue `onDragDropEvent` 一样的宿主拖放消息。
pub(crate) fn file_drop_message(flags: &FileDropFlags, event: &FileDropEvent) -> ShellMessage {
    let phase = match event {
        FileDropEvent::Hovered { .. } => HostDragPhase::Over,
        FileDropEvent::Dropped { .. } => HostDragPhase::Drop,
        FileDropEvent::Left => HostDragPhase::Leave,
    };
    let paths = match event {
        FileDropEvent::Hovered { paths, .. } | FileDropEvent::Dropped { paths, .. } => {
            paths.iter().map(path_text).collect()
        }
        FileDropEvent::Left => Vec::new(),
    };
    ShellMessage::Input(InputMessage::HostDrag {
        phase,
        paths,
        has_repository: flags.has_repository,
        missing_repository: flags.missing_repository,
        writable: flags.writable,
        files_panel: flags.files_panel,
        has_snapshot: flags.has_snapshot,
        repo_root: flags.repo_root.clone(),
    })
}

fn path_text(path: &PathBuf) -> String {
    path.to_string_lossy().into_owned()
}

/// `target` 建好后登记成系统文件拖放目标：接受文件、复制效果，整块子树都算。事件由视图的
/// `FileDropEvent` 处理器收成 `HostDrag`。在建 `target` 的那段视图里调用，跟着它的作用域走。
pub(crate) fn accept_file_drops(target: NodeRef) {
    on_mount(move |cx| {
        let Some(id) = target.get_untracked() else {
            eprintln!("Nana 文件拖放目标没有建出来，不登记拖放");
            return;
        };
        if let Err(error) = cx.set_drop_target_node(id, DropAccepts::files().effect(DropEffect::Copy)) {
            eprintln!("Nana 文件拖放目标没有挂上：{error}");
        }
    });
}

/// 有待确认的关闭时用确认对话框。只有说明、尚未进入确认时保留一行文字。
pub(crate) fn close_prompt(model: &ShellViewModel) -> Option<AnyView> {
    if !model.input.pending_close && model.input.notice.is_empty() {
        return None;
    }
    if !model.input.pending_close {
        return Some(text(model.input.notice.clone()).key("close-confirm-notice").into_any());
    }
    let notice = if model.input.notice.is_empty() {
        "确认关闭 MomoBako？".to_string()
    } else {
        model.input.notice.clone()
    };
    Some(
        widget(ConfirmDialog::new(notice.clone(), notice))
            .cancel(widget(super::super::workbench::ghost_button("取消")).key("close-confirm-cancel").on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program_all(ShellMessage::Input(InputMessage::ConfirmCloseAnswer(false)));
            }))
            .confirm(widget(super::super::workbench::primary_button("确认关闭")).key("close-confirm-accept").on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program_all(ShellMessage::Input(InputMessage::ConfirmCloseAnswer(true)));
            }))
            .into_any(),
    )
}
