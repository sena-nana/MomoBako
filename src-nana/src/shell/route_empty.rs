//! 空库路由（常驻）：还没有资源库时的拖入引导，整节是系统文件拖放目标。
//!
//! 照启动页的样板改成常驻：[`EmptyView`] 是投影，[`EmptySignals`] 建在主区块的常驻作用域里，
//! 视图只建一次。拖着文件夹经过时面板换底色和描边、附加失败的错误条，都按字段绑定。
//! 拖放处理器在事件到达时现读信号里的拖放条件，不抄建视图那一刻的值。
//!
//! 版式照 `EmptyRepositoryState.vue`：标题、拖入说明和附加失败的错误。

use std::sync::Arc;

use nana_ui::runtime::view::{fields, node_ref, signal, widget, AnyView, FieldWrite, IntoView, Signal, StyledComponent};
use nana_ui::runtime::{
    AlignSpec, FileDropEvent, JustifySpec, LengthSpec, RadiusTier, SemanticColorRole, Stack, Text, TextHorizontalAlignment,
};

use super::input::{accept_file_drops, file_drop_flags, file_drop_message, FileDropFlags};
use super::ShellViewModel;

/// 空库页要显示的东西，以及拖放消息要带的仓库条件。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct EmptyView {
    /// 拖着文件夹经过：面板 `--accent-soft` 底、1px 强调色描边。
    pub dragging: bool,
    /// 附加失败的错误，空时不显示错误条。
    pub error: String,
    pub drop: FileDropFlags,
}

impl EmptyView {
    /// 从 ViewModel 取空库页的投影。
    pub(crate) fn project(model: &ShellViewModel) -> Self {
        Self {
            dragging: model.input.dragging_repository_folder,
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
        Self { view: signal(EmptyView::project(model)) }
    }

    /// 写入投影，值没变不写；信号已随文档回收时不写。
    pub(crate) fn write(&self, view: EmptyView) {
        self.view.try_set_if_changed(view);
    }
}

/// 空库页：首页外框里纵向滚动的一节，内容在可见高度里居中。
pub(super) fn view(signals: EmptySignals) -> AnyView {
    super::route_home::home_page(None, super::route_home::home_scroll("workspace-empty-scroll", panel(signals.view)))
}

/// 拖入引导：标题、说明和错误条。整节都是拖放目标，悬停和放下走同一条宿主拖放消息。
fn panel(view: Signal<EmptyView>) -> AnyView {
    let initial = view.get_untracked();
    let mut copy = centered_text(initial.error.clone(), 13.0, 400, SemanticColorRole::Danger, 13.0 * 1.45);
    Arc::make_mut(&mut copy.style.layout).width = Some(LengthSpec::Fill);
    let error = widget(
        Stack::column(0.0)
            .width(LengthSpec::Fill)
            .padding_xy(10.0, 8.0)
            .painter(super::shell_tint::SoftFill::err())
            .with_layout(|layout| layout.max_width = Some(LengthSpec::Px(420.0))),
    )
    .visible(move || view.with(|view| !view.error.is_empty()))
    .children((widget(copy).prop::<String, fields::text::value>(move || view.with(|view| view.error.clone())).key("empty-error"),));
    let panel = Stack::column(10.0)
        .width(LengthSpec::Fill)
        .min_height(LengthSpec::Px(220.0))
        .padding_xy(16.0, 28.0)
        .radius(RadiusTier::Md)
        .align(AlignSpec::Center)
        .justify(JustifySpec::Center)
        .with_layout(|layout| layout.max_width = Some(LengthSpec::Px(520.0)));
    let rows = (
        widget(centered_text("还没有可用资源库", 18.0, 600, SemanticColorRole::Text, 18.0 * 1.55)).key("empty-title"),
        widget(centered_text("拖入一个本地文件夹创建资源库。", 13.0, 400, SemanticColorRole::Muted, 13.0 * 1.55)).key("empty-detail"),
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
        .children((widget(panel).prop::<bool, DropHighlight>(move || view.with(|view| view.dragging)).children(rows).key("empty-panel"),))
        .key("empty-repository-page")
        .into_any()
}

/// 居中的一段字，行高写成像素。对应 `.empty-state-page` 的 `text-align: center`。
fn centered_text(value: impl Into<String>, size: f32, weight: u16, color: SemanticColorRole, line_height: f32) -> Text {
    let mut node = Text::new(value).font_size(size).font_weight(weight).color(color).line_height(line_height);
    node.style.text_horizontal_alignment = TextHorizontalAlignment::Center;
    node
}

/// 拖着文件夹经过时的面板：`--accent-soft` 底、1px 强调色描边；平时没有底色和描边。
/// 描边和 `Stack::outline` 写的一样：边框色、清掉交互态的边框，再写边框宽度。
struct DropHighlight;

impl FieldWrite<Stack, bool> for DropHighlight {
    const FIELD: &'static str = "Stack.style.background+border+layout.border_width";

    fn write(target: &mut Stack, dragging: bool) {
        let style = <Stack as StyledComponent>::node_style_mut(target);
        if dragging {
            style.background = Some(SemanticColorRole::AccentSoft);
            style.border = Some(SemanticColorRole::Accent);
            style.interaction.base.border = None;
            style.interaction.base.border_mix = None;
            Arc::make_mut(&mut style.layout).border_width = Some(1.0);
        } else {
            style.background = None;
            style.border = None;
            Arc::make_mut(&mut style.layout).border_width = None;
        }
    }

    fn differs(target: &Stack, dragging: &bool) -> bool {
        let style = <Stack as StyledComponent>::node_style(target);
        let (background, border, width) = if *dragging {
            (Some(SemanticColorRole::AccentSoft), Some(SemanticColorRole::Accent), Some(1.0))
        } else {
            (None, None, None)
        };
        style.background != background || style.border != border || style.layout.border_width != width
    }
}

#[cfg(test)]
#[path = "route_empty_tests.rs"]
mod tests;
