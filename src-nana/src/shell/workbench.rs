//! 工作台页面的纵向排布、过程信息文字和页头分割线。对话框的按钮分级在统一对话框框架里。
//!
//! 颜色只用语义角色，不写死 RGB。

use nana_ui::runtime::view::{AnyView, IntoView, widget};
use nana_ui::runtime::{LengthSpec, SemanticColorRole, Stack, Text, TextHorizontalAlignment};
use nana_ui_core::TextWeight;

/// 页面纵向排布，块与块之间是窗口一栏里分组的间距（`SpacingRole::GroupInPane`，默认 16）。
/// 页边距由主区外框统一给，这里不再加。
pub(crate) fn page(body: Vec<AnyView>) -> AnyView {
    widget(
        Stack::fill_column(0.0)
            .gap_step(nana_ui_core::SpacingRole::GroupInPane)
            .min_height(LengthSpec::Px(0.0)),
    )
    .children(body)
    .into_any()
}

/// 次级说明。路径、时间和过程信息用它，不放进标题。
pub(crate) fn meta(label: impl Into<String>) -> Text {
    aligned(
        label,
        12.0,
        TextWeight::Regular,
        SemanticColorRole::Muted,
        TextHorizontalAlignment::Start,
    )
}

/// 页头底边。对应 `files-preview-page__header` 的下边线。
pub(crate) fn with_bottom_divider(column: Stack) -> Stack {
    let mut style = column.node_style();
    style.border = Some(SemanticColorRole::BorderSoft);
    crate::theme_map::tiers::border_side(
        std::sync::Arc::make_mut(&mut style.layout),
        crate::theme_map::tiers::Edge::Bottom,
        1,
    );
    column.style(style)
}

fn aligned(
    label: impl Into<String>,
    size: f32,
    weight: TextWeight,
    color: SemanticColorRole,
    align: TextHorizontalAlignment,
) -> Text {
    let mut node = crate::theme_map::typed_text(Text::new(label.into()), size, weight).color(color);
    node.style.text_horizontal_alignment = align;
    node
}
