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
    ControlHeight, ControlPadding, ControlSize, LineHeightSpec, LineRole, RadiusTier,
    SemanticColorMix, SpacingStep, TextWeight, ThemeExpr, ThemeToken,
};

use super::super::files_view::style as files_style;
use super::paint::SwatchChipPainter;
use super::presenter::SwatchColor;
use crate::theme_map::tiers::{self, Edge, Hairline};

/// 行高倍数 1.55。
pub(crate) const LINE_RATIO: f32 = 1.55;
/// 筛选芯片、输入胶囊、计数胶囊和分组标签的高度 26：控件档里没有这个高度，照写像素。
pub(crate) const CONTROL_HEIGHT: f32 = 26.0;
/// 筛选栏「清除」「关闭」「应用」按钮的高度档：小号控件。
const BAR_BUTTON_SIZE: ControlSize = ControlSize::Small;
/// 筛选栏按钮的高度（默认主题）。
pub(crate) const BAR_BUTTON_HEIGHT: f32 = BAR_BUTTON_SIZE.height_in(nana_ui::theme::UI_METRICS);
/// 色块和芯片文字之间的间距档。
const CHIP_GAP: SpacingStep = SpacingStep::Sm;
/// 胶囊里的输入框高：胶囊高减去上下两根发丝边。
const PILL_INNER_HEIGHT: f32 = CONTROL_HEIGHT - tiers::hairline_px() * 2.0;
/// 关闭按钮和中号控件一样宽（默认 32）。
const CLOSE_WIDTH: ControlSize = ControlSize::Medium;
/// 输入框最窄 86（`minmax(86px, 120px)` 的下限）。
const INPUT_MIN_WIDTH: f32 = 86.0;
/// 眉题字距 0.4：主题没有字距档，照写像素。
const EYEBROW_TRACKING: f32 = 0.4;

/// 控件上的行高：按钮、输入框自带主题的行盒令牌，写数值行高不生效，这里直接取它们实际画出来的行盒档。
fn control_line(layout: &mut nana_ui_core::LayoutStyle) {
    tiers::line(layout, LineRole::Line);
}
/// 输入框宽度，`minmax(86px, 120px)` 的上限。
pub(crate) const INPUT_WIDTH: f32 = 120.0;

/// 按给定字号、字重和行高排一段文字。
pub(crate) fn label(
    content: impl Into<String>,
    size: f32,
    weight: TextWeight,
    role: SemanticColorRole,
) -> Text {
    let mut node =
        crate::theme_map::typed_text(Text::new(content.into()), size, weight).color(role);
    node.style.text_horizontal_alignment = TextHorizontalAlignment::Start;
    let role = node.style.type_role;
    let layout = Arc::make_mut(&mut node.style.layout);
    match role {
        // 行高按字号档的倍数写，主题换字阶时一起变。
        Some(role) => tiers::line_ratio(layout, role, LINE_RATIO),
        None => layout.line_height = Some(LineHeightSpec::Relative(LINE_RATIO)),
    }
    node
}

/// 能按列宽折行的文字，路径和摘要用它。
pub(crate) fn wrapping_label(
    content: impl Into<String>,
    size: f32,
    weight: TextWeight,
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

/// 文字上方留出一档外边距，对应标题和摘要的 `margin-top`。
pub(crate) fn margin_top(node: Text, step: SpacingStep) -> Text {
    files_style::margin_top(node, step)
}

/// 眉题：11px、600 字重、弱色，字距 0.4px。
pub(crate) fn eyebrow(content: impl Into<String>) -> Text {
    let mut node = label(
        content,
        11.0,
        TextWeight::Semibold,
        SemanticColorRole::Faint,
    );
    Arc::make_mut(&mut node.style.layout)
        .text_ext
        .letter_spacing = Some(EYEBROW_TRACKING);
    node
}

/// 绑定文字：`value` 是常量、信号或闭包，其余版式照 `node`。
pub(crate) fn bound(node: Text, value: impl IntoProp<String>) -> El<Text> {
    widget(node).prop::<String, fields::text::value>(value)
}

/// 26px 高的计数胶囊，左右 `Lg`，主表面底、12px 次要文字。文字可以绑定，
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
            .with_layout(|layout| {
                tiers::padding(layout, &Edge::X, SpacingStep::Lg);
                layout.padding_top = Some(LengthSpec::Px(0.0));
                layout.padding_bottom = Some(LengthSpec::Px(0.0));
            })
            .surface(SemanticColorRole::Background)
            .radius(RadiusTier::Full)
            .grow(0.0)
            .shrink(0.0),
    )
    .children((bound(
        label(
            String::new(),
            12.0,
            TextWeight::Medium,
            SemanticColorRole::Muted,
        ),
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
        // 左内边距：小号控件内边距 + 色块 + 一档间距，三段都跟着主题。
        Arc::make_mut(&mut chip.style.layout).theme.padding_left = Some(ThemeExpr {
            constant: super::paint::SWATCH_SIZE,
            terms: vec![
                (ThemeToken::ControlPadding(ControlPadding::Compact), 1.0),
                (ThemeToken::Space(CHIP_GAP), 1.0),
            ],
        });
    }
    chip
}

/// 筛选栏按钮：小号控件高、左右取标准控件内边距档的透明按钮，按钮正文档字号、中等字重、正文色。
/// 悬停是 `--bg-hover`。禁用时文字按主题的停用不透明度档混进筛选栏底色：不透明度要按 sRGB 合成，
/// 直接给半透明文字会被渲染器按线性空间合成得更亮，所以先混成不透明色。
pub(crate) fn bar_button(content: &str, disabled: bool) -> Button {
    let button = Button::new(content).disabled(disabled);
    let mut style = button.style.clone();
    {
        let layout = Arc::make_mut(&mut style.layout);
        fixed_height(layout, BAR_BUTTON_HEIGHT);
        control_inset(layout);
        control_line(layout);
    }
    clear_control_metrics(&mut style);
    style.control_height = Some(ControlHeight::Exact(BAR_BUTTON_SIZE));
    style.control_padding_x = Some(ControlPadding::Standard);
    style.radius = Some(RadiusTier::Sm);
    ghost_paint(&mut style);
    style.interaction.disabled = SemanticPaint {
        foreground_mix: Some(SemanticColorMix::new(
            SemanticColorRole::Text,
            SemanticColorRole::Surface,
            crate::theme_map::tiers::DISABLED_OPACITY,
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
        tiers::control_width(layout, CLOSE_WIDTH);
        layout.flex_grow = Some(0.0);
        layout.flex_shrink = Some(0.0);
    }
    style.control_padding_x = None;
    style.control_height = Some(ControlHeight::Exact(BAR_BUTTON_SIZE));
    style.radius = Some(RadiusTier::Sm);
    ghost_paint(&mut style);
    button.style(style)
}

/// 按钮左右内边距：标准控件内边距档（默认 10）。
fn control_inset(layout: &mut nana_ui_core::LayoutStyle) {
    crate::theme_map::tiers::control_padding_x(layout, ControlPadding::Standard);
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

/// 输入胶囊：26px 高，一圈发丝边、主表面底，里面放输入框和可选按钮。
pub(crate) fn input_pill(fill: bool) -> Stack {
    let pill = Hairline::hairline(
        Stack::row(0.0)
            .align(AlignSpec::Stretch)
            .height(LengthSpec::Px(CONTROL_HEIGHT))
            .min_height(LengthSpec::Px(CONTROL_HEIGHT))
            .surface(SemanticColorRole::Background),
        SemanticColorRole::Border,
    )
    .radius(RadiusTier::Full)
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

/// 胶囊里的输入框：透明、无边框，左右 `Md`，输入框正文档字号。宽度为 `None` 时填满胶囊。
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
        layout.height = Some(LengthSpec::Px(PILL_INNER_HEIGHT));
        layout.min_height = Some(LengthSpec::Px(0.0));
        tiers::padding(layout, &Edge::X, SpacingStep::Md);
        layout.padding_top = Some(LengthSpec::Px(0.0));
        layout.padding_bottom = Some(LengthSpec::Px(0.0));
        layout.border_width = Some(0.0);
        layout.theme.border_width = None;
        crate::theme_map::control_weight(layout, TextWeight::Regular);
        control_line(layout);
        match width {
            Some(width) => {
                layout.width = Some(LengthSpec::Px(width));
                layout.min_width = Some(LengthSpec::Px(INPUT_MIN_WIDTH.min(width)));
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

/// 胶囊右侧的「添加」：`--bg-subtle` 底，按钮正文档字号的次要色字，右端跟着胶囊的全圆角。
/// 可用时悬停是柔和强调底、强调色字；输入为空时底和字都按主题的停用不透明度档混进胶囊底色。
pub(crate) fn pill_button(content: &str, disabled: bool) -> Button {
    let button = Button::new(content).disabled(disabled);
    let mut style = button.style.clone();
    {
        let layout = Arc::make_mut(&mut style.layout);
        fixed_height(layout, PILL_INNER_HEIGHT);
        control_inset(layout);
        layout.border_width = Some(0.0);
        layout.theme.border_width = None;
        control_line(layout);
        layout.flex_grow = Some(0.0);
        layout.flex_shrink = Some(0.0);
    }
    clear_control_metrics(&mut style);
    style.control_padding_x = Some(ControlPadding::Standard);
    // 右端两个角跟着胶囊的全圆角，左端是直角。
    style.corner_radii = Some([None, Some(RadiusTier::Full), Some(RadiusTier::Full), None]);
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
            crate::theme_map::tiers::DISABLED_OPACITY,
        )),
        background_mix: Some(SemanticColorMix::new(
            SemanticColorRole::Subtle,
            SemanticColorRole::Background,
            crate::theme_map::tiers::DISABLED_OPACITY,
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
