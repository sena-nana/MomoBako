//! 启动页和缺失仓库页共用的排版零件：占满可见高度的一节、写成像素行高的文字、眉题和外边距。
//!
//! 两个页面本身在 `route_startup.rs` 和 `route_missing.rs`（都是常驻路由）。行高按根字号的
//! 1.55 倍和各类自己的行高换算，挂成字号档的倍数（`theme_map::text_line_height`）；间距取主题间距档。

use std::sync::Arc;

use nana_ui::runtime::view::{AnyView, IntoView, widget};
use nana_ui::runtime::{AlignSpec, JustifySpec, LengthSpec, SemanticColorRole, Stack, Text};
use nana_ui_core::{SpacingStep, TextWeight};

use crate::theme_map::tiers::{self, Edge};

use super::render::{PRIMARY_INSET_Y, TITLE_BAR_PX};
use super::sidebar_view::parts::label_text;

/// 根元素的行高倍数。
pub(super) const ROOT_LINE_HEIGHT: f32 = 1.55;

/// 占满主区可见高度、内容居中的一节，内边距取页面档（默认 24px）。
///
/// 启动页、缺失仓库页和空库页都用它，最小高度占满主区可见高度：
/// 窗口高减去标题栏和主区上下内边距。窗口矮时内容
/// 撑高这一节，交给外层滚动。
pub(crate) fn fill_section() -> Stack {
    Stack::column(0.0)
        .min_height(LengthSpec::viewport_offset(
            nana_ui_core::ViewportAxis::Height,
            100.0,
            -(TITLE_BAR_PX + tiers::space_px(PRIMARY_INSET_Y) * 2.0),
        ))
        .padding_step(SpacingStep::Page, SpacingStep::Page)
        .justify(JustifySpec::Center)
        .align(AlignSpec::Center)
}

/// 一段文字，行高按像素给、挂成主题档位（见 `theme_map::text_line_height`）。
pub(super) fn line(
    value: impl Into<String>,
    size: f32,
    weight: TextWeight,
    color: SemanticColorRole,
    line_height: f32,
) -> Text {
    crate::theme_map::text_line_height(label_text(value, size, weight, Some(color)), line_height)
}

/// 占满列宽、可在任意位置折行的一段字。
pub(super) fn wrapping(
    value: impl Into<String>,
    size: f32,
    weight: TextWeight,
    color: SemanticColorRole,
    line_height: f32,
) -> Text {
    let mut node = line(value, size, weight, color, line_height);
    let layout = Arc::make_mut(&mut node.style.layout);
    layout.width = Some(LengthSpec::Fill);
    layout.min_width = Some(LengthSpec::Px(0.0));
    layout.text_ext.overflow_wrap = Some(nana_ui_core::OverflowWrapSpec::Anywhere);
    node
}

/// 眉题：11px、600、弱色、字距 0.4px。英文直接写大写。
pub(super) fn eyebrow(value: &str, key: &'static str) -> AnyView {
    let mut node = line(
        value,
        11.0,
        TextWeight::Semibold,
        SemanticColorRole::Faint,
        11.0 * ROOT_LINE_HEIGHT,
    );
    Arc::make_mut(&mut node.style.layout)
        .text_ext
        .letter_spacing = Some(0.4);
    widget(node).key(key).into_any()
}

/// 往上收一档间距（负的上外边距）的一层，标题下的说明贴近标题用。
pub(super) fn pull_up(step: SpacingStep) -> Stack {
    Stack::column(0.0)
        .width(LengthSpec::Fill)
        .with_layout(move |layout| {
            layout.margin_top = Some(LengthSpec::Px(-tiers::space_px(step)));
            layout.theme.margin_top = Some(tiers::space_expr(step).scaled(-1.0));
        })
}

/// 带外边距的一层，用来微调几处间距：上下各取一档间距，`None` 是不留。
pub(super) fn margin(top: Option<SpacingStep>, bottom: Option<SpacingStep>) -> Stack {
    Stack::column(0.0)
        .width(LengthSpec::Fill)
        .with_layout(move |layout| {
            if let Some(step) = top {
                tiers::margin(layout, &[Edge::Top], step);
            }
            if let Some(step) = bottom {
                tiers::margin(layout, &[Edge::Bottom], step);
            }
        })
}
