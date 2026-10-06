//! 关闭确认和系统文件拖放。验收场景不挂关闭条，避免改掉旧的命中目标。

use std::path::PathBuf;

use nana_ui::runtime::view::{button, text, widget, AnyView, IntoView};
use nana_ui::runtime::{Activate, AlignSpec, ConfirmDialog, FileDropEvent, JustifySpec, LengthSpec, Stack};
use super::{HostDragPhase, InputMessage};
use crate::shell::{MainRegion, ShellMessage, ShellViewModel, WorkspacePanel};

/// 挂上拖放时从壳层抄下来的仓库条件。事件回调里不再借壳层。
#[derive(Clone)]
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

/// 空库拖放。悬停和放下都走同一条宿主拖放消息。
pub(crate) fn empty_repository_panel(model: &ShellViewModel) -> AnyView {
    let error = model.workspace.startup.error.clone().or_else(|| {
        (!model.workspace.missing_error.is_empty()).then(|| model.workspace.missing_error.clone())
    });
    let card = widget(Stack::column(10.0).width(LengthSpec::Px(520.0)).align(AlignSpec::Center)).children((
        text("还没有可用资源库").key("empty-title"),
        text("拖入一个本地文件夹创建资源库。").key("empty-detail"),
        error.map(|error| text(error).key("empty-error")),
    ));
    widget(
        Stack::fill_column(0.0)
            .padding_xy(24.0, 24.0)
            .justify(JustifySpec::Center)
            .align(AlignSpec::Center),
    )
    .on_cx({
        let flags = file_drop_flags(model);
        move |_, event: &FileDropEvent, cx| {
            cx.dispatch_program(file_drop_message(&flags, event));
        }
    })
    .children((drop_marker("empty"), card))
    .into_any()
}

/// 隐藏标记。挂载后按它的父节点登记文件拖放目标。
pub(crate) fn drop_marker(kind: &str) -> AnyView {
    let mut marker = nana_ui::runtime::Text::new(format!("momobako-drop:{kind}"));
    let layout = std::sync::Arc::make_mut(&mut marker.style.layout);
    layout.hidden = true;
    layout.width = Some(LengthSpec::Px(0.0));
    layout.height = Some(LengthSpec::Px(0.0));
    widget(marker).into_any()
}

/// 有待确认的关闭时用确认对话框。只有说明、尚未进入确认时保留一行文字。
pub(crate) fn close_prompt(model: &ShellViewModel) -> Option<AnyView> {
    if model.acceptance_scene || (!model.input.pending_close && model.input.notice.is_empty()) {
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
            .cancel(button("取消").key("close-confirm-cancel").on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program(ShellMessage::Input(InputMessage::ConfirmCloseAnswer(false)));
            }))
            .confirm(button("确认关闭").key("close-confirm-accept").on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program(ShellMessage::Input(InputMessage::ConfirmCloseAnswer(true)));
            }))
            .into_any(),
    )
}
