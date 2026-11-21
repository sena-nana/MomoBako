//! 空库路由（常驻）：还没有资源库时的拖入引导，整节是系统文件拖放目标。
//!
//! 照启动页的样板改成常驻：[`EmptyView`] 是投影，[`EmptySignals`] 建在主区块的常驻作用域里，
//! 视图只建一次。附加失败的错误条按字段绑定。拖着文件夹经过时的悬停样子由 NanaUI 画在整节拖放目标上。
//! 拖放处理器在事件到达时现读信号里的拖放条件，不抄建视图那一刻的值。
//!
//! 版式：NanaUI 的空状态（图标、标题、拖入说明）和附加失败的提示条。

use std::sync::Arc;

use nana_ui::icons_tabler::{ALERT_CIRCLE, FOLDER};
use nana_ui::runtime::view::{
    AnyView, IntoView, Signal, callout, empty_state, node_ref, signal, widget,
};
use nana_ui::runtime::{
    AlignSpec, FileDropEvent, JustifySpec, LengthSpec, RadiusTier, Stack, StatusTone,
};

use super::ShellViewModel;
use super::input::{FileDropFlags, accept_file_drops, file_drop_flags, file_drop_message};

/// 空库页要显示的东西，以及拖放消息要带的仓库条件。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct EmptyView {
    /// 附加失败的错误，空时不显示错误条。
    pub error: String,
    pub drop: FileDropFlags,
}

impl EmptyView {
    /// 从 ViewModel 取空库页的投影。
    pub(crate) fn project(model: &ShellViewModel) -> Self {
        Self {
            error: model.input.empty_repository_error.clone(),
            drop: file_drop_flags(model),
        }
    }
}

/// 空库页的信号。句柄是 `Copy` 的 id，值在主区块的常驻作用域里。
#[derive(Clone, Copy)]
pub(crate) struct EmptySignals {
    view: Signal<EmptyView>,
}

impl EmptySignals {
    /// 在常驻作用域里建信号，初值是这一刻的投影。
    pub(crate) fn new(model: &ShellViewModel) -> Self {
        Self {
            view: signal(EmptyView::project(model)),
        }
    }

    /// 写入投影，值没变不写；信号已随文档回收时不写。
    pub(crate) fn write(&self, view: EmptyView) {
        self.view.try_set_if_changed(view);
    }
}

/// 空库页：首页外框里纵向滚动的一节，内容在可见高度里居中。
pub(super) fn view(signals: EmptySignals) -> AnyView {
    super::route_home::home_page(
        None,
        super::route_home::home_scroll("workspace-empty-scroll", panel(signals.view)),
    )
}

/// 拖入引导：NanaUI 的空状态（`empty_state`，文件夹图标、标题和说明）和附加失败的提示条（`callout`）。
/// 整节都是拖放目标，悬停和放下走同一条宿主拖放消息；拖着文件夹经过时 NanaUI 在整节上画悬停样子。
fn panel(view: Signal<EmptyView>) -> AnyView {
    let error_text = move || view.with(|view| view.error.clone());
    let error = widget(
        Stack::column(0.0)
            .width(LengthSpec::Fill)
            .with_layout(|layout| layout.max_width = Some(LengthSpec::Px(420.0))),
    )
    .visible(move || view.with(|view| !view.error.is_empty()))
    .children((callout(ALERT_CIRCLE, StatusTone::Danger, error_text),))
    .key("empty-error");
    let panel = Stack::column(10.0)
        .width(LengthSpec::Fill)
        .min_height(LengthSpec::Px(220.0))
        .padding_xy(16.0, 12.0)
        .radius(RadiusTier::Md)
        .align(AlignSpec::Center)
        .justify(JustifySpec::Center)
        .with_layout(|layout| layout.max_width = Some(LengthSpec::Px(520.0)));
    let rows = (
        empty_state("还没有可用资源库")
            .message(Some(Arc::<str>::from("拖入一个本地文件夹创建资源库。")))
            .icon(Some(FOLDER))
            .key("empty-state"),
        error,
    );
    let drop = node_ref();
    accept_file_drops(drop);
    widget(super::startup_view::fill_section())
        .node_ref(drop)
        .on_cx(move |_, event: &FileDropEvent, cx| {
            let flags = view.with_untracked(|view| view.drop.clone());
            cx.dispatch_program_all(file_drop_message(&flags, event));
        })
        .children((widget(panel).children(rows).key("empty-panel"),))
        .key("empty-repository-page")
        .into_any()
}

#[cfg(test)]
mod tests;
