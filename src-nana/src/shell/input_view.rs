//! 关闭确认。验收场景不挂这条，避免改掉旧的命中目标。

use nana_ui::runtime::view::{button, text, widget, AnyView, IntoView};
use nana_ui::runtime::{Activate, ConfirmDialog};

use super::InputMessage;
use crate::shell::{ShellMessage, ShellViewModel};

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
