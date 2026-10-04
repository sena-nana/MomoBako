//! 关闭确认条。验收场景不挂这条，避免改掉旧的命中目标。

use nana_ui::runtime::view::{button, text, widget, AnyView, IntoView};
use nana_ui::runtime::{Activate, Stack};

use super::InputMessage;
use crate::shell::{ShellMessage, ShellViewModel};

/// 有待确认的关闭，或托盘请求留下了说明时显示。
pub(crate) fn close_prompt(model: &ShellViewModel) -> Option<AnyView> {
    if model.acceptance_scene || (!model.input.pending_close && model.input.notice.is_empty()) {
        return None;
    }
    let notice = model.input.notice.clone();
    let mut rows = vec![text(notice).key("close-confirm-notice").into_any()];
    if model.input.pending_close {
        rows.push(
            widget(Stack::fill_row(8.0))
                .children((
                    button("确认关闭").key("close-confirm-accept").on_cx(|_, _: &Activate, cx| {
                        cx.dispatch_program(ShellMessage::Input(InputMessage::ConfirmCloseAnswer(true)));
                    }),
                    button("取消").key("close-confirm-cancel").on_cx(|_, _: &Activate, cx| {
                        cx.dispatch_program(ShellMessage::Input(InputMessage::ConfirmCloseAnswer(false)));
                    }),
                ))
                .into_any(),
        );
    }
    Some(widget(Stack::fill_column(8.0)).children(rows).key("close-confirm").into_any())
}
