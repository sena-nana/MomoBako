//! 搜索面板和筛选栏共用的小部件：文字、计数胶囊、筛选芯片、输入胶囊和工具按钮。
//!
//! 筛选芯片是 NanaUI 的 `Chip`，尺寸跟着主题的小号控件。
//! 输入胶囊和计数胶囊 26px，筛选栏按钮 28px。结果芯片是只显示的 NanaUI 标签，见 `inspect_library.rs`。
//! 颜色只用语义角色；色块的真实颜色由 `search_paint` 自绘。

use std::sync::Arc;

use nana_ui::icons_tabler::X;
use nana_ui::runtime::view::{El, IntoProp, fields, widget};
use nana_ui::runtime::{
    AlignSpec, Button, Chip, Divider, IconButton, LengthSpec, NodePainter, NodeStyle,
    SemanticColorRole, SemanticPaint, Stack, Text, TextHorizontalAlignment, TextInput,
};
use nana_ui_core::{
    ControlPadding, LineHeightSpec, RadiusTier, SemanticColorMix, ThemeExpr, ThemeToken,
};

use super::paint::SwatchChipPainter;
use super::presenter::SwatchColor;
use crate::theme_map::DISABLED_OPACITY;

/// 行高倍数 1.55。
pub(crate) const LINE_RATIO: f32 = 1.55;
/// 筛选芯片、输入胶囊、计数胶囊和分组标签的高度。
pub(crate) const CONTROL_HEIGHT: f32 = 26.0;
/// 筛选栏「清除」「关闭」「应用」按钮的高度。
pub(crate) const BAR_BUTTON_HEIGHT: f32 = 28.0;
/// 色块和芯片文字之间的间距。
const CHIP_GAP: f32 = 6.0;
/// 输入框宽度，`minmax(86px, 120px)` 的上限。
pub(crate) const INPUT_WIDTH: f32 = 120.0;

/// 按给定字号、字重和行高排一段文字。
pub(crate) fn label(
    content: impl Into<String>,
    size: f32,
    weight: u16,
    role: SemanticColorRole,
) -> Text {
    let mut node =
        super::super::files_view::style::typed(Text::new(content.into()), size, weight).color(role);
    node.style.text_horizontal_alignment = TextHorizontalAlignment::Start;
    let layout = Arc::make_mut(&mut node.style.layout);
    layout.line_height = Some(LineHeightSpec::Relative(LINE_RATIO));
    node
}

/// 能按列宽折行的文字，路径和摘要用它。
pub(crate) fn wrapping_label(
    content: impl Into<String>,
    size: f32,
    weight: u16,
    role: SemanticColorRole,
) -> Text {
    let mut node = label(content, size, weight, role);
    let layout = Arc::make_mut(&mut node.style.layout);
    layout.width = Some(LengthSpec::Fill);
    layout.min_width = Some(LengthSpec::Px(0.0));
    layout.white_space_nowrap = false;
    layout.text_ext.line_break = Some(nana_ui_core::LineBreakSpec::Anywhere);
    layout.text_ext.overflow_wrap = Some(nana_ui_core::OverflowWrapSpec::Anywhere);
    node
}

/// 文字上方留出外边距，对应标题和摘要的 `margin-top`。
pub(crate) fn margin_top(mut node: Text, px: f32) -> Text {
    Arc::make_mut(&mut node.style.layout).margin_top = Some(LengthSpec::Px(px));
    node
}

/// 眉题：11px、600 字重、弱色，字距 0.4px。
pub(crate) fn eyebrow(content: impl Into<String>) -> Text {
    let mut node = label(content, 11.0, 600, SemanticColorRole::Faint);
    Arc::make_mut(&mut node.style.layout)
        .text_ext
        .letter_spacing = Some(0.4);
    node
}

/// 绑定文字：`value` 是常量、信号或闭包，其余版式照 `node`。
pub(crate) fn bound(node: Text, value: impl IntoProp<String>) -> El<Text> {
    widget(node).prop::<String, fields::text::value>(value)
}

/// 26px 高的计数胶囊，主表面底、12px 次要文字。文字可以绑定，
/// 返回元素，调用方还能再绑显隐。
pub(crate) fn stat(
    content: impl IntoProp<String>,
    key: impl Into<String>,
) -> El<Stack, (El<Text>,)> {
    let key = key.into();
    widget(
        Stack::row(0.0)
            .align(AlignSpec::Center)
            .height(LengthSpec::Px(CONTROL_HEIGHT))
            .padding_xy(10.0, 0.0)
            .surface(SemanticColorRole::Background)
            .radius_px(999.0)
            .grow(0.0)
            .shrink(0.0),
    )
    .children((bound(
        label(String::new(), 12.0, 500, SemanticColorRole::Muted),
        content,
    )
    .key(format!("{key}-text")),))
    .key(key)
}

/// 筛选芯片：NanaUI 的 `Chip`，选中态由调用方绑 `selected`。颜色芯片在左内边距里多留色块和间距的
/// 位置，色块由 [`SwatchChipPainter`] 画在芯片自己的外观上面。
pub(crate) fn chip(content: &str, swatch: Option<SwatchColor>) -> Chip {
    let mut chip = Chip::new(content);
    if let Some(color) = swatch {
        chip.style.painter = Some(NodePainter::new(SwatchChipPainter { color }));
        Arc::make_mut(&mut chip.style.layout).theme.padding_left = Some(
            ThemeExpr::token(ThemeToken::ControlPadding(ControlPadding::Compact))
                .offset(super::paint::SWATCH_SIZE + CHIP_GAP),
        );
    }
    chip
}

/// 筛选栏按钮：28px 高、左右 9px 的透明按钮，14px 中等字重正文色。
/// 悬停是 `--bg-hover`。禁用时文字按 45% 混进筛选栏底色：不透明度要按 sRGB 合成，
/// 直接给半透明文字会被渲染器按线性空间合成得更亮，所以先混成不透明色。
pub(crate) fn bar_button(content: &str, disabled: bool) -> Button {
    let button = Button::new(content).disabled(disabled);
    let mut style = button.style.clone();
    {
        let layout = Arc::make_mut(&mut style.layout);
        fixed_height(layout, BAR_BUTTON_HEIGHT);
        layout.padding_left = Some(LengthSpec::Px(9.0));
        layout.padding_right = Some(LengthSpec::Px(9.0));
        layout.font_size = Some(14.0);
        layout.font_weight = Some(500);
        layout.line_height = Some(LineHeightSpec::Relative(LINE_RATIO));
    }
    clear_control_metrics(&mut style);
    style.radius = Some(RadiusTier::Sm);
    ghost_paint(&mut style);
    style.interaction.disabled = SemanticPaint {
        foreground_mix: Some(SemanticColorMix::new(
            SemanticColorRole::Text,
            SemanticColorRole::Surface,
            DISABLED_OPACITY,
        )),
        ..SemanticPaint::default()
    };
    button.style(style)
}

/// 关闭筛选栏：和工具按钮同高，14px 叉号（两条对角线），正文色。
pub(crate) fn close_button(name: &str) -> IconButton {
    let button = IconButton::new(X, name)
        .size(nana_ui_core::ControlSize::Large)
        .colors_from_style();
    let mut style = button.style.clone();
    {
        let layout = Arc::make_mut(&mut style.layout);
        fixed_height(layout, BAR_BUTTON_HEIGHT);
        layout.width = Some(LengthSpec::Px(32.0));
        layout.min_width = Some(LengthSpec::Px(32.0));
        layout.flex_grow = Some(0.0);
        layout.flex_shrink = Some(0.0);
    }
    style.control_padding_x = None;
    style.radius = Some(RadiusTier::Sm);
    ghost_paint(&mut style);
    button.style(style)
}

/// 透明底、正文色，悬停 `--bg-hover`、按下 `--bg-active`。
fn ghost_paint(style: &mut NodeStyle) {
    style.foreground = Some(SemanticColorRole::Text);
    style.background = None;
    style.border = None;
    style.interaction.hovered = SemanticPaint {
        foreground: Some(SemanticColorRole::Text),
        background: Some(SemanticColorRole::Hover),
        ..SemanticPaint::default()
    };
    style.interaction.pressed = SemanticPaint {
        foreground: Some(SemanticColorRole::Text),
        background: Some(SemanticColorRole::Active),
        ..SemanticPaint::default()
    };
    style.interaction.disabled = SemanticPaint::default();
}

/// 输入胶囊：26px 高，1px 边框、主表面底，里面放输入框和可选按钮。
pub(crate) fn input_pill(fill: bool) -> Stack {
    let pill = Stack::row(0.0)
        .align(AlignSpec::Stretch)
        .height(LengthSpec::Px(CONTROL_HEIGHT))
        .min_height(LengthSpec::Px(CONTROL_HEIGHT))
        .surface(SemanticColorRole::Background)
        .border(SemanticColorRole::Border, 1.0)
        .radius_px(999.0)
        .grow(0.0)
        .shrink(0.0)
        .with_layout(|layout| {
            layout.overflow_x = nana_ui_core::OverflowSpec::Hidden;
            layout.overflow_y = nana_ui_core::OverflowSpec::Hidden;
        });
    if fill {
        pill.width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0))
    } else {
        pill.width(LengthSpec::Shrink)
    }
}

/// 胶囊里的输入框：透明、无边框，左右 8px，12px 正文。宽度为 `None` 时填满胶囊。
pub(crate) fn pill_input(
    value: &str,
    placeholder: &str,
    name: &str,
    width: Option<f32>,
) -> TextInput {
    let input = TextInput::new(value.to_string())
        .placeholder(placeholder.to_string())
        .label(name.to_string());
    let mut style = input.style.clone();
    {
        let layout = Arc::make_mut(&mut style.layout);
        layout.height = Some(LengthSpec::Px(CONTROL_HEIGHT - 2.0));
        layout.min_height = Some(LengthSpec::Px(0.0));
        layout.padding_left = Some(LengthSpec::Px(8.0));
        layout.padding_right = Some(LengthSpec::Px(8.0));
        layout.padding_top = Some(LengthSpec::Px(0.0));
        layout.padding_bottom = Some(LengthSpec::Px(0.0));
        layout.border_width = Some(0.0);
        layout.font_size = Some(12.0);
        layout.font_weight = Some(400);
        layout.line_height = Some(LineHeightSpec::Relative(LINE_RATIO));
        match width {
            Some(width) => {
                layout.width = Some(LengthSpec::Px(width));
                layout.min_width = Some(LengthSpec::Px(86.0_f32.min(width)));
                layout.flex_grow = Some(0.0);
                layout.flex_shrink = Some(1.0);
            }
            None => {
                layout.width = Some(LengthSpec::Fill);
                layout.min_width = Some(LengthSpec::Px(0.0));
                layout.flex_grow = Some(1.0);
                layout.flex_shrink = Some(1.0);
            }
        }
    }
    clear_control_metrics(&mut style);
    style.background = None;
    style.border = None;
    style.radius = None;
    style.interaction.hovered = SemanticPaint::default();
    style.interaction.focused = SemanticPaint::default();
    style.interaction.disabled = SemanticPaint::default();
    input.style(style)
}

/// 胶囊里输入框和「添加」之间的竖分隔线：NanaUI 的 `Divider`，`--border-soft` 色。
pub(crate) fn pill_divider() -> Divider {
    Divider::vertical()
}

/// 胶囊右侧的「添加」：`--bg-subtle` 底，12px 次要字，右端跟着胶囊的全圆角。
/// 可用时悬停是柔和强调底、强调色字；输入为空时底和字都按 45% 混进胶囊底色。
pub(crate) fn pill_button(content: &str, disabled: bool) -> Button {
    let button = Button::new(content).disabled(disabled);
    let mut style = button.style.clone();
    {
        let layout = Arc::make_mut(&mut style.layout);
        fixed_height(layout, CONTROL_HEIGHT - 2.0);
        layout.padding_left = Some(LengthSpec::Px(9.0));
        layout.padding_right = Some(LengthSpec::Px(9.0));
        layout.border_width = Some(0.0);
        layout.font_size = Some(12.0);
        layout.font_weight = Some(500);
        layout.line_height = Some(LineHeightSpec::Relative(LINE_RATIO));
        layout.flex_grow = Some(0.0);
        layout.flex_shrink = Some(0.0);
        layout.paint.border_radii = Some([
            LengthSpec::Px(0.0),
            LengthSpec::Px(999.0),
            LengthSpec::Px(999.0),
            LengthSpec::Px(0.0),
        ]);
    }
    clear_control_metrics(&mut style);
    style.foreground = Some(SemanticColorRole::Muted);
    style.background = Some(SemanticColorRole::Subtle);
    style.border = None;
    style.radius = None;
    let accent = SemanticPaint {
        foreground: Some(SemanticColorRole::Accent),
        background: Some(SemanticColorRole::AccentSoft),
        ..SemanticPaint::default()
    };
    style.interaction.hovered = accent;
    style.interaction.pressed = accent;
    style.interaction.disabled = SemanticPaint {
        foreground_mix: Some(SemanticColorMix::new(
            SemanticColorRole::Muted,
            SemanticColorRole::Background,
            DISABLED_OPACITY,
        )),
        background_mix: Some(SemanticColorMix::new(
            SemanticColorRole::Subtle,
            SemanticColorRole::Background,
            DISABLED_OPACITY,
        )),
        ..SemanticPaint::default()
    };
    button.style(style)
}

/// 固定高度：去掉控件尺寸档位推出的最小高和上下内边距。
/// 宽度按文字量出来，不再按省略号截断，免得量宽和绘制的亚像素差把「添加」画成「…」。
fn fixed_height(layout: &mut nana_ui_core::LayoutStyle, height: f32) {
    layout.height = Some(LengthSpec::Px(height));
    layout.min_height = Some(LengthSpec::Px(height));
    layout.max_height = Some(LengthSpec::Px(height));
    layout.padding_top = Some(LengthSpec::Px(0.0));
    layout.padding_bottom = Some(LengthSpec::Px(0.0));
    layout.text_overflow_ellipsis = false;
}

/// 换成手写尺寸后，不再让控件尺寸档位改写高度和左右内边距。
fn clear_control_metrics(style: &mut NodeStyle) {
    style.control_height = None;
    style.control_padding_x = None;
    style.control_padding_y = None;
}
