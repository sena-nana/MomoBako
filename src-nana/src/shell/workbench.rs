//! 工作台页面的圆角面板、右上角计数和虚线空状态。
//!
//! 版式对齐 Vue 的 `search-workbench__panel`、`asset-stat` 和虚线空框。
//! 颜色只用语义角色；虚线描边用布局的 `BorderStyle`，不写死 RGB。

use nana_ui::runtime::view::{widget, AnyView, IntoView};
use nana_ui::runtime::{AlignSpec, Button, ConfirmDialog, JustifySpec, LengthSpec, RadiusTier, SemanticColorRole, Stack, Text, TextHorizontalAlignment};
use nana_ui::ButtonKind;

/// 页边距。面板自己再做圆角和内边距。
pub(crate) fn page(body: Vec<AnyView>) -> AnyView {
    widget(Stack::fill_column(16.0).padding_xy(20.0, 18.0).min_height(LengthSpec::Px(0.0)))
        .children(body)
        .into_any()
}

/// 抬升的圆角内容面板，占满主区剩余高度。
/// 矮不过内容：窄窗里说明折行后不能被压到下一张卡片上。
pub(crate) fn panel(body: Vec<AnyView>) -> AnyView {
    widget(
        Stack::fill_column(16.0)
            .height(LengthSpec::Shrink)
            .surface(SemanticColorRole::Surface)
            .radius(RadiusTier::Xl)
            .padding_xy(22.0, 22.0)
            .min_height(LengthSpec::MinContent)
            .grow(1.0)
            .shrink(0.0),
    )
    .children(body)
    .key("workbench-panel")
    .into_any()
}

/// 设置页里的小卡片：标题在上，内容跟在下面。
pub(crate) fn section_card(title: &str, body: Vec<AnyView>) -> AnyView {
    let mut rows = vec![widget(muted(title, 13.0, 600)).into_any()];
    rows.extend(body);
    widget(
        Stack::column(8.0)
            .surface(SemanticColorRole::Surface)
            .radius(RadiusTier::Md)
            .padding_xy(16.0, 14.0),
    )
    .children(rows)
    .into_any()
}

/// 标题在左，徽章和操作在右。空副标题不占一行。
/// 副标题单独占满列宽再折行，避免按单行宽画到下一张卡片上。
pub(crate) fn header(
    eyebrow: &str,
    eyebrow_key: &'static str,
    title: &str,
    title_key: &'static str,
    subline: &str,
    subline_key: &'static str,
    trailing: Vec<AnyView>,
) -> AnyView {
    let left = vec![
        widget(Text::new(eyebrow).color(SemanticColorRole::Faint).font_size(11.0).font_weight(600))
            .key(eyebrow_key)
            .into_any(),
        widget(Text::new(title).font_size(22.0).font_weight(600)).key(title_key).into_any(),
    ];
    // 左列如果用 Fill，会按父级 100% 算宽，把右侧徽章挤出卡片。
    let title_row = widget(Stack::bar(12.0).align(AlignSpec::Start))
        .children((
            widget(Stack::column(4.0).width(LengthSpec::Shrink).min_width(LengthSpec::Px(0.0)).grow(1.0).shrink(1.0)).children(left),
            widget(Stack::row(8.0).align(AlignSpec::Center).grow(0.0).shrink(0.0)).children(trailing),
        ))
        .into_any();
    if subline.is_empty() {
        return title_row;
    }
    widget(Stack::column(4.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0)))
        .children((title_row, widget(wrapping_subline(subline)).key(subline_key)))
        .into_any()
}

/// 说明按列宽折行。`Text` 没有 wrap，用 `line-break: anywhere`。
pub(crate) fn wrapping_note(label: impl Into<String>) -> Text {
    let mut node = muted(label.into(), 13.0, 400);
    let layout = std::sync::Arc::make_mut(&mut node.style.layout);
    layout.width = Some(LengthSpec::Fill);
    layout.min_width = Some(LengthSpec::Px(0.0));
    layout.line_break = Some(nana_ui_core::LineBreakSpec::Anywhere);
    layout.overflow_wrap = Some(nana_ui_core::OverflowWrapSpec::Anywhere);
    node
}

fn wrapping_subline(label: &str) -> Text {
    wrapping_note(label)
}

/// 右上角计数胶囊。
pub(crate) fn badge(label: impl Into<String>, key: impl Into<String>) -> AnyView {
    widget(
        Stack::row(0.0)
            .surface(SemanticColorRole::Background)
            .radius_px(999.0)
            .padding_xy(10.0, 4.0)
            .grow(0.0)
            .shrink(0.0),
    )
    .children((widget(muted(label, 12.0, 500)).key(key.into()),))
    .into_any()
}

/// 虚线空框。播放集文案居中，日志和搜索文案靠左。
pub(crate) fn dashed_empty(
    title: &str,
    message: &str,
    title_key: &'static str,
    message_key: &'static str,
    center: bool,
    fill: bool,
) -> AnyView {
    let align = if center { TextHorizontalAlignment::Center } else { TextHorizontalAlignment::Start };
    let frame = if fill { Stack::fill_column(8.0).grow(1.0) } else { Stack::column(8.0).grow(0.0) };
    widget(
        frame
            .align(if center { AlignSpec::Center } else { AlignSpec::Stretch })
            .justify(JustifySpec::Center)
            .surface(SemanticColorRole::Subtle)
            .outline(SemanticColorRole::BorderStrong, 1.0)
            .radius(RadiusTier::Lg)
            .padding_xy(24.0, 24.0)
            .min_height(LengthSpec::Px(if fill { 220.0 } else { 280.0 }))
            .with_layout(|layout| {
                layout.border_style = Some(nana_ui_core::BorderStyle::Dashed);
            }),
    )
    .children((
        widget(aligned(title, 18.0, 600, SemanticColorRole::Text, align)).key(title_key),
        widget(aligned(message, 13.0, 400, SemanticColorRole::Muted, align)).key(message_key),
    ))
    .into_any()
}

/// 页面标题。18px，对应设计标准里的页面标题。
pub(crate) fn page_title(label: impl Into<String>) -> Text {
    aligned(label, 18.0, 600, SemanticColorRole::Text, TextHorizontalAlignment::Start)
}

/// 分区标题。14px 工作文本，用来扫读一块内容，不拿来写过程。
pub(crate) fn section_title(label: impl Into<String>) -> Text {
    aligned(label, 14.0, 600, SemanticColorRole::Text, TextHorizontalAlignment::Start)
}

/// 眉题。11px 弱色，标出当前表面。
pub(crate) fn eyebrow(label: impl Into<String>) -> Text {
    aligned(label, 11.0, 600, SemanticColorRole::Faint, TextHorizontalAlignment::Start)
}

/// 次级说明。路径、时间和过程信息用它，不放进标题。
pub(crate) fn meta(label: impl Into<String>) -> Text {
    aligned(label, 12.0, 400, SemanticColorRole::Muted, TextHorizontalAlignment::Start)
}

/// Vue `asset-meta__row` 的行间分割。只画一条边，不另加卡片底。
/// 第一行不要调用。上边距只留 2px，避免把尺寸和原始大小挤出首屏。
pub(crate) fn with_top_divider(column: Stack) -> Stack {
    edge_divider(column, true)
}

/// 页头底边。对应 `files-preview-page__header` 的下边线。
pub(crate) fn with_bottom_divider(column: Stack) -> Stack {
    edge_divider(column, false)
}

fn edge_divider(column: Stack, top: bool) -> Stack {
    let mut style = column.node_style();
    style.border = Some(SemanticColorRole::BorderSoft);
    let layout = std::sync::Arc::make_mut(&mut style.layout);
    if top {
        layout.border_top_width = Some(1.0);
        layout.padding_top = Some(LengthSpec::Px(2.0));
    } else {
        layout.border_bottom_width = Some(1.0);
    }
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

/// 危险操作。删除、清空和不可恢复的确认用它，和普通按钮分开。
pub(crate) fn danger_button(label: impl Into<String>) -> Button {
    Button::new(label).kind(ButtonKind::Danger)
}

/// 危险确认框。标题和后果说明由组件承载，确认钮再标成危险。
pub(crate) fn danger_dialog(title: impl Into<std::sync::Arc<str>>, message: impl Into<std::sync::Arc<str>>) -> ConfirmDialog {
    let mut dialog = ConfirmDialog::new(title, message);
    dialog.danger = true;
    dialog
}

fn muted(label: impl Into<String>, size: f32, weight: u16) -> Text {
    aligned(label, size, weight, SemanticColorRole::Muted, TextHorizontalAlignment::Start)
}

fn aligned(label: impl Into<String>, size: f32, weight: u16, color: SemanticColorRole, align: TextHorizontalAlignment) -> Text {
    let mut node = Text::new(label.into()).color(color).font_size(size).font_weight(weight);
    node.style.text_horizontal_alignment = align;
    node
}
