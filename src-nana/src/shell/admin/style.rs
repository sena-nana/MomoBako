//! 设置、插件、日志和工具页的版式原语。
//!
//! 字号和字重按主题档位写（没有档位的字号写像素），行高、内边距、间距和控件高度写成固定值；
//! 颜色只用主题角色。
//! 圆角一律用 `RadiusTier`，形状和半径基数由渲染层统一生效。

use std::sync::Arc;

use nana_ui::runtime::view::{AnyView, El, IntoProp, IntoView, fields, widget};
use nana_ui::runtime::{
    AlignSpec, Button, IconGlyph, InteractionStyle, JustifySpec, LengthSpec, NodeStyle,
    SemanticPaint, Stack, Text, TextHorizontalAlignment, TextInput,
};
use nana_ui_core::{
    ButtonKind, Icon, LineHeightSpec, OverflowWrapSpec, RadiusTier, SemanticColorRole as Role,
    TextWeight, TypeRole,
};

/// 正文行高倍数。
pub(crate) const LINE: f32 = 1.55;
/// 等宽字体在 Windows 上用的字体。
pub(crate) const MONO_FAMILY: &str = "Cascadia Mono";
/// 绑定文字：`value` 是常量、信号或闭包，其余版式照 `node`。
pub(crate) fn bound(node: Text, value: impl IntoProp<String>) -> El<Text> {
    widget(node).prop::<String, fields::text::value>(value)
}

/// 字号：主题字号档（11 `Hint`、12 `Meta`、13 `Body`、14 `Section`、15 `CardTitle`、18 `Title`、
/// 20 `Display`），换了字号刻度的主题跟着变；没有对应档位的字号（22、24 和小数）写像素。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum FontSize {
    Role(TypeRole),
    Px(f32),
}

impl From<TypeRole> for FontSize {
    fn from(role: TypeRole) -> Self {
        Self::Role(role)
    }
}

impl From<f32> for FontSize {
    fn from(px: f32) -> Self {
        Self::Px(px)
    }
}

/// 按字号档和字重档写文字的字号、字重。行高另外设，不跟着档位变。
pub(crate) fn typeset(text: Text, size: impl Into<FontSize>, weight: TextWeight) -> Text {
    let text = match size.into() {
        FontSize::Role(role) => text.role(role),
        FontSize::Px(px) => {
            let mut text = text;
            Arc::make_mut(&mut text.style.layout).font_size = Some(px);
            text
        }
    };
    text.weight(weight)
}

/// 一段文字：字号、字重、颜色，行高按默认的 1.55 倍。
pub(crate) fn label(
    value: impl Into<String>,
    size: impl Into<FontSize>,
    weight: TextWeight,
    color: Role,
) -> Text {
    label_lh(value, size, weight, color, LINE)
}

/// 指定行高倍数的文字。标题 1.25、说明 1.45 或 1.5 这类单独写。
pub(crate) fn label_lh(
    value: impl Into<String>,
    size: impl Into<FontSize>,
    weight: TextWeight,
    color: Role,
    line: f32,
) -> Text {
    let mut text = typeset(Text::new(value.into()).color(color), size, weight);
    Arc::make_mut(&mut text.style.layout).line_height = Some(LineHeightSpec::Relative(line));
    text.style.text_horizontal_alignment = TextHorizontalAlignment::Start;
    text
}

/// 行高写成像素的文字，用于行高固定为 24px 一类的标签。
pub(crate) fn label_px(
    value: impl Into<String>,
    size: impl Into<FontSize>,
    weight: TextWeight,
    color: Role,
    line_px: f32,
) -> Text {
    let mut text = label(value, size, weight, color);
    Arc::make_mut(&mut text.style.layout).line_height = Some(LineHeightSpec::Absolute(line_px));
    text
}

/// 等宽文字。
pub(crate) fn mono(
    value: impl Into<String>,
    size: impl Into<FontSize>,
    weight: TextWeight,
    color: Role,
) -> Text {
    mono_lh(value, size, weight, color, LINE)
}

/// 指定行高倍数的等宽文字，日志上下文的 `<pre>` 用 1.6。
pub(crate) fn mono_lh(
    value: impl Into<String>,
    size: impl Into<FontSize>,
    weight: TextWeight,
    color: Role,
    line: f32,
) -> Text {
    let mut text = label_lh(value, size, weight, color, line);
    Arc::make_mut(&mut text.style.layout).text_ext.font_family = Some(MONO_FAMILY.into());
    text
}

/// 占满父级宽度并在任意处折行，对应 `overflow-wrap: anywhere`。
pub(crate) fn wrapping(mut text: Text) -> Text {
    let layout = Arc::make_mut(&mut text.style.layout);
    layout.width = Some(LengthSpec::Fill);
    layout.min_width = Some(LengthSpec::Px(0.0));
    layout.text_ext.overflow_wrap = Some(OverflowWrapSpec::Anywhere);
    text
}

/// 右对齐。键值表的值列和输出值用它。
pub(crate) fn align_end(mut text: Text) -> Text {
    text.style.text_horizontal_alignment = TextHorizontalAlignment::End;
    text
}

/// 小图标。颜色跟随所在控件的前景角色。
pub(crate) fn glyph(icon: Icon, size: f32, color: Role) -> IconGlyph {
    IconGlyph::new(icon).size(size).role(color)
}

/// 普通按钮：配色按 `kind` 取主题按钮配方（`Ghost` 透明底正文色、`Primary` 强调浅底、`Danger` 红字），
/// 这里只改版式：高 32、左右内边距 10、字 14（`Section`）/500、主操作 600、图标 14、间距 6。
/// 禁用时整体 45% 不透明，不换颜色（主题的禁用配色会换成弱色和浅底）。要绑定禁用时用
/// `bind::ActionDisabled`，透明度一起换；进行中用 `fields::button::loading`，图标位转圈。
pub(crate) fn action(
    text: impl Into<String>,
    icon: Option<Icon>,
    kind: ButtonKind,
    disabled: bool,
) -> Button {
    let mut button = Button::new(text)
        .kind(kind)
        .icon_size(14.0)
        .icon_gap(6.0)
        .disabled(disabled);
    if let Some(icon) = icon {
        button = button.icon(icon);
    }
    let style = &mut button.style;
    style.interaction.disabled = SemanticPaint::default();
    style.control_height = None;
    style.control_padding_x = None;
    let layout = Arc::make_mut(&mut style.layout);
    layout.height = Some(LengthSpec::Px(32.0));
    layout.min_height = Some(LengthSpec::Px(32.0));
    layout.width = Some(LengthSpec::Shrink);
    layout.padding_left = Some(LengthSpec::Px(10.0));
    layout.padding_right = Some(LengthSpec::Px(10.0));
    layout.padding_top = Some(LengthSpec::Px(0.0));
    layout.padding_bottom = Some(LengthSpec::Px(0.0));
    layout.border_width = None;
    layout.line_height = Some(LineHeightSpec::Relative(LINE));
    layout.flex_grow = Some(0.0);
    layout.flex_shrink = Some(0.0);
    layout.theme.border_width = None;
    layout.theme.line_height = None;
    crate::theme_map::control_font(layout, TypeRole::Section);
    if kind == ButtonKind::Primary {
        crate::theme_map::control_weight(layout, TextWeight::Semibold);
    }
    if disabled {
        layout.opacity = Some(crate::theme_map::DISABLED_OPACITY);
    }
    button
}

/// 拼进 `.key()` 的外部文本（插件、字段、日志、仓库、任务标识）。`/` 是键路径分隔符，
/// 留着会让壳层挂载失败；按 `%` 转义成 `%2F`，不同的原文仍得到不同的键。
pub(crate) fn key_part(text: &str) -> String {
    text.replace('%', "%25")
        .replace('/', "%2F")
        .replace('~', "%7E")
}

/// 一列外部标识对应的键：先转义，重复出现的再加「~第几次」。第一次出现的键保持原样，
/// 列表增删时其它项的键不变，焦点和滚动跟得住；同层重复的键会让挂载失败。
pub(crate) fn unique_keys<'a>(ids: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut seen = std::collections::HashMap::<String, usize>::new();
    ids.into_iter()
        .map(|id| {
            let base = key_part(id);
            let count = seen.entry(base.clone()).or_insert(0);
            *count += 1;
            if *count == 1 {
                base
            } else {
                format!("{base}~{count}")
            }
        })
        .collect()
}

/// CSS `minmax(min, max)` 的定宽上限轨道。CSS 先把剩余空间分给这类轨道直到上限，再分给 `fr`；
/// Nana 网格按权重分配后冻结越界轨道，所以给它极大的权重：空间够时先到上限，余下的给其它轨道。
pub(crate) fn capped(min_px: f32, max_px: f32) -> nana_ui_core::GridTrack {
    nana_ui_core::GridTrack::MinMax {
        min_px,
        fr: 1000.0,
        max_px: Some(max_px),
    }
}

/// 水平排列：子项垂直居中，宽度随内容。
pub(crate) fn row(gap: f32) -> Stack {
    Stack::row(gap).align(AlignSpec::Center)
}

/// 两端对齐的一行，对应 `display:flex; justify-content: space-between`。
pub(crate) fn spread(gap: f32, align: AlignSpec) -> Stack {
    Stack::bar(gap)
        .align(align)
        .justify(JustifySpec::SpaceBetween)
}

/// 竖直排列，宽度占满，高度随内容。
pub(crate) fn column(gap: f32) -> Stack {
    Stack::column(gap)
}

/// 定高定宽不收缩的盒子，给图标格和固定列用。
pub(crate) fn fixed(stack: Stack, width: Option<f32>, height: Option<f32>) -> Stack {
    stack.with_layout(|layout| {
        if let Some(width) = width {
            layout.width = Some(LengthSpec::Px(width));
            layout.min_width = Some(LengthSpec::Px(width));
        }
        if let Some(height) = height {
            layout.height = Some(LengthSpec::Px(height));
            layout.min_height = Some(LengthSpec::Px(height));
        }
        layout.flex_grow = Some(0.0);
        layout.flex_shrink = Some(0.0);
    })
}

/// 四边内边距不同的盒子。顺序同 CSS：上、右、下、左。
pub(crate) fn pad(stack: Stack, top: f32, right: f32, bottom: f32, left: f32) -> Stack {
    stack.with_layout(|layout| {
        layout.padding = None;
        layout.padding_top = Some(LengthSpec::Px(top));
        layout.padding_right = Some(LengthSpec::Px(right));
        layout.padding_bottom = Some(LengthSpec::Px(bottom));
        layout.padding_left = Some(LengthSpec::Px(left));
    })
}

/// 只画上边线的盒子，对应 `border-top: 1px solid var(--border-soft)`。
pub(crate) fn top_rule(stack: Stack) -> Stack {
    let mut style = stack.node_style();
    style.border = Some(Role::BorderSoft);
    {
        let layout = Arc::make_mut(&mut style.layout);
        layout.border_width = None;
        layout.border_sides.top_width = Some(1.0);
        layout.border_sides.left_width = Some(0.0);
        layout.border_sides.right_width = Some(0.0);
        layout.border_sides.bottom_width = Some(0.0);
    }
    stack.style(style)
}

/// 工作区面板：`bg-elev` 底、`--radius-2xl`、1px 透明边加 22 内边距、竖排间距 16，
/// 撑满主区剩余高度。
pub(crate) fn workbench_panel(body: Vec<AnyView>, key: &'static str) -> AnyView {
    let panel = pad(Stack::fill_column(16.0), 23.0, 23.0, 23.0, 23.0)
        .surface(Role::Surface)
        .radius(RadiusTier::Xxl)
        .with_layout(|layout| {
            layout.height = Some(LengthSpec::Shrink);
            layout.min_height = Some(LengthSpec::MinContent);
            layout.flex_grow = Some(1.0);
            layout.flex_shrink = Some(0.0);
        });
    widget(panel).children(body).key(key).into_any()
}

/// 面板页头：左边眉题、24/700 标题和说明，右边统计和操作，顶对齐。
pub(crate) fn workbench_header(
    eyebrow: &str,
    title: &str,
    subline: &str,
    trailing: Vec<AnyView>,
    key: &'static str,
) -> AnyView {
    let mut left = vec![
        widget(eyebrow_text(eyebrow))
            .key(format!("{key}-eyebrow"))
            .into_any(),
        widget(pad(column(0.0), 4.0, 0.0, 0.0, 0.0))
            .children((widget(label(title, 24.0, TextWeight::Bold, Role::Text))
                .key(format!("{key}-title")),))
            .into_any(),
    ];
    if !subline.is_empty() {
        left.push(
            widget(pad(column(0.0), 8.0, 0.0, 0.0, 0.0))
                .children((widget(wrapping(label(
                    subline,
                    TypeRole::Section,
                    TextWeight::Regular,
                    Role::Muted,
                )))
                .key(format!("{key}-subline")),))
                .into_any(),
        );
    }
    let stats = widget(
        Stack::row(10.0)
            .align(AlignSpec::Start)
            .wrap(true)
            .grow(0.0)
            .shrink(0.0),
    )
    .children(trailing);
    widget(spread(16.0, AlignSpec::Start))
        .children((
            widget(
                Stack::column(0.0)
                    .width(LengthSpec::Shrink)
                    .min_width(LengthSpec::Px(0.0))
                    .grow(1.0)
                    .shrink(1.0),
            )
            .children(left),
            stats,
        ))
        .key(format!("{key}-header"))
        .into_any()
}

/// 眉题：11/600 弱色、大写、字距 0.4。
pub(crate) fn eyebrow_text(value: &str) -> Text {
    let mut text = label(
        value.to_uppercase(),
        TypeRole::Hint,
        TextWeight::Semibold,
        Role::Faint,
    );
    Arc::make_mut(&mut text.style.layout)
        .text_ext
        .letter_spacing = Some(0.4);
    text
}

/// 输入框外框：主背景、1px 边线、lg 圆角，最小高 38，左右 12，间距 8，弱色。
pub(crate) fn field_frame() -> Stack {
    pad(Stack::fill_row(8.0), 0.0, 12.0, 0.0, 12.0)
        .surface(Role::Background)
        .border(Role::Border, 1.0)
        .radius(RadiusTier::Lg)
        .with_layout(|layout| {
            layout.min_height = Some(LengthSpec::Px(38.0));
            layout.flex_grow = Some(0.0);
        })
}

/// 嵌在 [`field_frame`] 里的输入框：透明、无边、无内边距，高 32，正文档字号。
pub(crate) fn bare_input(input: TextInput) -> TextInput {
    let mut style = NodeStyle {
        foreground: Some(Role::Text),
        text_vertical_alignment: nana_ui::runtime::TextVerticalAlignment::Center,
        ..NodeStyle::default()
    };
    {
        let layout = Arc::make_mut(&mut style.layout);
        layout.width = Some(LengthSpec::Fill);
        layout.min_width = Some(LengthSpec::Px(0.0));
        layout.height = Some(LengthSpec::Px(32.0));
        layout.flex_grow = Some(1.0);
        layout.flex_shrink = Some(1.0);
        crate::theme_map::control_font(layout, TypeRole::Body);
        layout.line_height = Some(LineHeightSpec::Relative(LINE));
        layout.white_space_nowrap = true;
        layout.overflow_x = nana_ui_core::OverflowSpec::Hidden;
        layout.overflow_y = nana_ui_core::OverflowSpec::Hidden;
    }
    input.style(style)
}

/// 单行输入框：高 32、内边距 6/10、sm 圆角、1px `border-strong`、`bg-subtle`，
/// 聚焦时换主背景。
pub(crate) fn native_input(input: TextInput) -> TextInput {
    input.style(native_field_style(32.0))
}

/// 原生输入框和文本域共用的外形。`height` 给单行高度，文本域另设最小高。
pub(crate) fn native_field_style(height: f32) -> NodeStyle {
    let mut style = NodeStyle {
        foreground: Some(Role::Text),
        background: Some(Role::Subtle),
        border: Some(Role::BorderStrong),
        radius: Some(RadiusTier::Sm),
        interaction: InteractionStyle {
            focused: SemanticPaint {
                background: Some(Role::Background),
                border: Some(Role::BorderStrong),
                ..SemanticPaint::default()
            },
            ..InteractionStyle::default()
        }
        .into(),
        text_vertical_alignment: nana_ui::runtime::TextVerticalAlignment::Center,
        ..NodeStyle::default()
    };
    {
        let layout = Arc::make_mut(&mut style.layout);
        layout.width = Some(LengthSpec::Fill);
        layout.min_width = Some(LengthSpec::Px(0.0));
        layout.height = Some(LengthSpec::Px(height));
        layout.border_width = Some(1.0);
        layout.padding_left = Some(LengthSpec::Px(10.0));
        layout.padding_right = Some(LengthSpec::Px(10.0));
        layout.padding_top = Some(LengthSpec::Px(6.0));
        layout.padding_bottom = Some(LengthSpec::Px(6.0));
        crate::theme_map::control_font(layout, TypeRole::Body);
        layout.line_height = Some(LineHeightSpec::Relative(LINE));
        layout.white_space_nowrap = true;
        layout.overflow_x = nana_ui_core::OverflowSpec::Hidden;
        layout.overflow_y = nana_ui_core::OverflowSpec::Hidden;
    }
    style
}
