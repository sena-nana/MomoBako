//! 工作台页面的纵向排布、过程信息文字、页头分割线和按钮分级。
//!
//! 按钮分级照 DESIGN.md 5.1：普通操作透明，主操作低饱和强调色。
//! 颜色只用语义角色，不写死 RGB。

use nana_ui::runtime::view::{widget, AnyView, IntoView};
use nana_ui::runtime::{Button, LengthSpec, SemanticColorRole, Stack, Text, TextHorizontalAlignment};
use nana_ui::ButtonKind;

/// 页面纵向排布。页边距由主区外框统一给（上下 20、左右 24），这里不再加。
pub(crate) fn page(body: Vec<AnyView>) -> AnyView {
    widget(Stack::fill_column(16.0).min_height(LengthSpec::Px(0.0)))
        .children(body)
        .into_any()
}

/// 次级说明。路径、时间和过程信息用它，不放进标题。
pub(crate) fn meta(label: impl Into<String>) -> Text {
    aligned(label, 12.0, 400, SemanticColorRole::Muted, TextHorizontalAlignment::Start)
}

/// 页头底边。对应 `files-preview-page__header` 的下边线。
pub(crate) fn with_bottom_divider(column: Stack) -> Stack {
    let mut style = column.node_style();
    style.border = Some(SemanticColorRole::BorderSoft);
    std::sync::Arc::make_mut(&mut style.layout).border_bottom_width = Some(1.0);
    column.style(style)
}

/// 普通操作。常态透明，不和主按钮抢视线。
pub(crate) fn ghost_button(label: impl Into<String>) -> Button {
    Button::new(label).kind(ButtonKind::Ghost)
}

/// 主操作。低饱和强调色，一块表面里只放真正要执行的动作。
pub(crate) fn primary_button(label: impl Into<String>) -> Button {
    Button::new(label).kind(ButtonKind::Primary)
}

fn aligned(label: impl Into<String>, size: f32, weight: u16, color: SemanticColorRole, align: TextHorizontalAlignment) -> Text {
    let mut node = Text::new(label.into()).color(color).font_size(size).font_weight(weight);
    node.style.text_horizontal_alignment = align;
    node
}
