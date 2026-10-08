//! 设置、插件、日志和工具页的 Vue 版式原语。
//!
//! 字号、字重、行高、内边距、间距和控件高度照 Vue 的计算样式写；颜色只用主题角色。
//! Vue 里带主题相关透明度或 `color-mix` 的柔和底色交给 [`SoftFill`]，在绘制时按当前
//! 主题解析。圆角一律用 `RadiusTier`，形状和半径基数由渲染层统一生效。

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use nana_ui::runtime::view::{widget, AnyView, IntoView};
use nana_ui::runtime::{
    AlignSpec, BoxPaint, Button, IconGlyph, InteractionStyle, JustifySpec, LengthSpec, NodeStyle, PaintContext, Painter,
    SemanticPaint, Stack, Text, TextHorizontalAlignment,
};
use nana_ui_core::{
    Icon, LineHeightSpec, OverflowWrapSpec, RadiusTier, SemanticColorMix, SemanticColorRole as Role, ThemeMode,
};

/// Vue `:root { line-height: 1.55 }`。
pub(crate) const LINE: f32 = 1.55;
/// Vue `--font-mono` 在 Windows 上解析到的字体。
pub(crate) const MONO_FAMILY: &str = "Cascadia Mono";
/// `border-radius: var(--radius-pill)`。
pub(crate) const PILL: f32 = 999.0;

/// 一段文字：字号、字重、颜色，行高按 Vue 默认的 1.55 倍。
pub(crate) fn label(value: impl Into<String>, size: f32, weight: u16, color: Role) -> Text {
    label_lh(value, size, weight, color, LINE)
}

/// 指定行高倍数的文字。标题 1.25、说明 1.45 或 1.5 这类按 Vue 单独写。
pub(crate) fn label_lh(value: impl Into<String>, size: f32, weight: u16, color: Role, line: f32) -> Text {
    let mut text = Text::new(value.into()).color(color).font_size(size).font_weight(weight);
    Arc::make_mut(&mut text.style.layout).line_height = Some(LineHeightSpec::Relative(line));
    text.style.text_horizontal_alignment = TextHorizontalAlignment::Start;
    text
}

/// 行高写成像素的文字，对应 Vue 里写死 `line-height: 24px` 一类的标签。
pub(crate) fn label_px(value: impl Into<String>, size: f32, weight: u16, color: Role, line_px: f32) -> Text {
    let mut text = label(value, size, weight, color);
    Arc::make_mut(&mut text.style.layout).line_height = Some(LineHeightSpec::Absolute(line_px));
    text
}

/// 等宽文字，对应 Vue `font-family: var(--font-mono)`。
pub(crate) fn mono(value: impl Into<String>, size: f32, weight: u16, color: Role) -> Text {
    let mut text = label(value, size, weight, color);
    Arc::make_mut(&mut text.style.layout).font_family = Some(MONO_FAMILY.into());
    text
}

/// 占满父级宽度并在任意处折行，对应 `overflow-wrap: anywhere`。
pub(crate) fn wrapping(mut text: Text) -> Text {
    let layout = Arc::make_mut(&mut text.style.layout);
    layout.width = Some(LengthSpec::Fill);
    layout.min_width = Some(LengthSpec::Px(0.0));
    layout.overflow_wrap = Some(OverflowWrapSpec::Anywhere);
    text
}

/// 右对齐。Vue `.kv` 的值列和 `<output>` 用它。
pub(crate) fn align_end(mut text: Text) -> Text {
    text.style.text_horizontal_alignment = TextHorizontalAlignment::End;
    text
}

/// 固定 RGBA 的前景色。只给 Vue 写死、两套主题共用的颜色（日志级别）用。
pub(crate) fn fixed_color(mut text: Text, rgb: [u8; 3]) -> Text {
    Arc::make_mut(&mut text.style.layout).color = Some(rgba(rgb));
    text
}

fn rgba(rgb: [u8; 3]) -> [f32; 4] {
    [f32::from(rgb[0]) / 255.0, f32::from(rgb[1]) / 255.0, f32::from(rgb[2]) / 255.0, 1.0]
}

/// 小图标。颜色跟随所在控件的前景角色。
pub(crate) fn glyph(icon: Icon, size: f32, color: Role) -> IconGlyph {
    IconGlyph::new(icon).size(size).role(color)
}

/// Vue 按钮的语气。裸 `class="primary"` 和 `ghost danger` 在 Vue 里没有样式，
/// 这里按 DESIGN.md 5.1 画：主操作低饱和蓝，危险操作红字。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tone {
    /// 普通按钮：透明底、正文色。
    Plain,
    /// 主操作：`accent-soft` 底、`accent-on-soft` 字、600 字重。
    Primary,
    /// 危险操作：透明底、危险色字。
    Danger,
}

/// Vue 原生 `<button>`：高 32、左右内边距 10、字 14/500、图标 14、间距 6、圆角 sm。
/// 禁用时整体 45% 不透明，不换颜色。
pub(crate) fn action(text: impl Into<String>, icon: Option<Icon>, tone: Tone, disabled: bool) -> Button {
    sized_action(text, icon, tone, disabled, ActionSize::Native)
}

/// 按钮尺寸档。`Native` 是裸 `<button>`，`Small` 是 `.repository-add-popover__action` 一类的 30 高按钮。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ActionSize {
    Native,
    Compact,
}

/// 指定尺寸档的按钮。
pub(crate) fn sized_action(
    text: impl Into<String>,
    icon: Option<Icon>,
    tone: Tone,
    disabled: bool,
    size: ActionSize,
) -> Button {
    let (height, font) = match size {
        ActionSize::Native => (32.0, 14.0),
        ActionSize::Compact => (30.0, 14.0),
    };
    let (foreground, background, hovered, pressed, weight) = match tone {
        Tone::Plain => (Role::Text, None, Some(Role::Hover), Some(Role::Active), 500),
        Tone::Primary => (Role::AccentOnSoft, Some(Role::AccentSoft), Some(Role::AccentSoftHover), Some(Role::AccentSoftPressed), 600),
        Tone::Danger => (Role::Danger, None, Some(Role::DangerSoftHover), Some(Role::DangerSoftPressed), 500),
    };
    let mut style = NodeStyle {
        foreground: Some(foreground),
        background,
        radius: Some(RadiusTier::Sm),
        interaction: InteractionStyle {
            hovered: SemanticPaint { background: hovered, foreground: Some(foreground), ..SemanticPaint::default() },
            pressed: SemanticPaint { background: pressed, foreground: Some(foreground), ..SemanticPaint::default() },
            focused: SemanticPaint { background: hovered, foreground: Some(foreground), ..SemanticPaint::default() },
            disabled: SemanticPaint { background, foreground: Some(foreground), ..SemanticPaint::default() },
            ..InteractionStyle::default()
        },
        text_horizontal_alignment: TextHorizontalAlignment::Center,
        text_vertical_alignment: nana_ui::runtime::TextVerticalAlignment::Center,
        ..NodeStyle::default()
    };
    {
        let layout = Arc::make_mut(&mut style.layout);
        layout.height = Some(LengthSpec::Px(height));
        layout.min_height = Some(LengthSpec::Px(height));
        layout.width = Some(LengthSpec::Shrink);
        layout.padding_left = Some(LengthSpec::Px(10.0));
        layout.padding_right = Some(LengthSpec::Px(10.0));
        layout.padding_top = Some(LengthSpec::Px(0.0));
        layout.padding_bottom = Some(LengthSpec::Px(0.0));
        layout.font_size = Some(font);
        layout.font_weight = Some(weight);
        layout.line_height = Some(LineHeightSpec::Relative(LINE));
        layout.white_space_nowrap = true;
        layout.flex_grow = Some(0.0);
        layout.flex_shrink = Some(0.0);
        if disabled {
            layout.opacity = Some(0.45);
        }
    }
    let mut button = Button::new(text).icon_gap(6.0).disabled(disabled).style(style);
    if let Some(icon) = icon {
        button = button.icon(icon).icon_size(14.0);
    }
    button
}

/// 水平排列：子项垂直居中，宽度随内容。
pub(crate) fn row(gap: f32) -> Stack {
    Stack::row(gap).align(AlignSpec::Center)
}

/// 两端对齐的一行，对应 `display:flex; justify-content: space-between`。
pub(crate) fn spread(gap: f32, align: AlignSpec) -> Stack {
    Stack::bar(gap).align(align).justify(JustifySpec::SpaceBetween)
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

/// 只画下边线的盒子，对应 `border-bottom: 1px solid var(--border-soft)`。
pub(crate) fn bottom_rule(stack: Stack) -> Stack {
    let mut style = stack.node_style();
    style.border = Some(Role::BorderSoft);
    {
        let layout = Arc::make_mut(&mut style.layout);
        layout.border_width = None;
        layout.border_top_width = Some(0.0);
        layout.border_left_width = Some(0.0);
        layout.border_right_width = Some(0.0);
        layout.border_bottom_width = Some(1.0);
    }
    stack.style(style)
}

/// 只画上边线的盒子，对应 `border-top: 1px solid var(--border-soft)`。
pub(crate) fn top_rule(stack: Stack) -> Stack {
    let mut style = stack.node_style();
    style.border = Some(Role::BorderSoft);
    {
        let layout = Arc::make_mut(&mut style.layout);
        layout.border_width = None;
        layout.border_top_width = Some(1.0);
        layout.border_left_width = Some(0.0);
        layout.border_right_width = Some(0.0);
        layout.border_bottom_width = Some(0.0);
    }
    stack.style(style)
}

/// 圆角档位。`Pill` 是 `--radius-pill`。
pub(crate) fn rounded(stack: Stack, tier: Option<RadiusTier>) -> Stack {
    match tier {
        Some(tier) => stack.radius(tier),
        None => stack.radius_px(PILL),
    }
}

/// `.asset-stat`：高 26、左右 10、药丸、主背景、弱色 12/500。
pub(crate) fn stat(text: impl Into<String>, key: impl Into<String>) -> AnyView {
    let body = fixed(row(0.0), None, Some(26.0));
    widget(rounded(pad(body, 0.0, 10.0, 0.0, 10.0), None).surface(Role::Background))
        .children((widget(label(text, 12.0, 500, Role::Muted)).key(key.into()),))
        .into_any()
}

/// `.workspace-hints__chip`：高 28、左右 10、药丸、主背景、1px 边线、弱色 12/500。
pub(crate) fn hint_chip(text: impl Into<String>, key: impl Into<String>) -> AnyView {
    let body = fixed(row(0.0), None, Some(28.0));
    widget(rounded(pad(body, 0.0, 10.0, 0.0, 10.0), None).surface(Role::Background).outline(Role::Border, 1.0))
        .children((widget(label(text, 12.0, 500, Role::Muted)).key(key.into()),))
        .into_any()
}

/// `.asset-card__pill` 的四种语气。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PillTone {
    /// 已启用：`accent-soft` 底、强调色。
    Accent,
    /// 未启用：`bg-subtle` 底、弱色。
    Ghost,
    /// 错误或不可用：`err-soft` 底、危险色。
    Danger,
    /// 降级：强调色 14% 底、强调色。
    Warning,
}

/// `.asset-card__pill`：高 24、左右 8、药丸、11/600。
pub(crate) fn pill(text: impl Into<String>, tone: PillTone, key: impl Into<String>) -> AnyView {
    let body = rounded(pad(fixed(row(0.0), None, Some(24.0)), 0.0, 8.0, 0.0, 8.0), None);
    let (body, color) = match tone {
        PillTone::Accent => (body.surface(Role::AccentSoft), Role::Accent),
        PillTone::Ghost => (body.surface(Role::Subtle), Role::Muted),
        PillTone::Danger => (body.painter(SoftFill::new(Soft::Danger, None)), Role::Danger),
        PillTone::Warning => (body.painter(SoftFill::new(Soft::Alpha { role: Role::Accent, alpha: 0.14 }, None)), Role::Accent),
    };
    widget(body).children((widget(label(text, 11.0, 600, color)).key(key.into()),)).into_any()
}

/// `.search-workbench__panel`：`bg-elev` 底、1px 透明边加 22 内边距、竖排间距 16，
/// 撑满主区剩余高度。Vue 用 `--radius-2xl`，按渲染层口径先用 `Xl`。
pub(crate) fn workbench_panel(body: Vec<AnyView>, key: &'static str) -> AnyView {
    let panel = pad(Stack::fill_column(16.0), 23.0, 23.0, 23.0, 23.0)
        .surface(Role::Surface)
        .radius(RadiusTier::Xl)
        .with_layout(|layout| {
            layout.height = Some(LengthSpec::Shrink);
            layout.min_height = Some(LengthSpec::MinContent);
            layout.flex_grow = Some(1.0);
            layout.flex_shrink = Some(0.0);
        });
    widget(panel).children(body).key(key).into_any()
}

/// `.search-workbench__header`：左边眉题、24/700 标题和说明，右边统计和操作，顶对齐。
pub(crate) fn workbench_header(
    eyebrow: &str,
    title: &str,
    subline: &str,
    trailing: Vec<AnyView>,
    key: &'static str,
) -> AnyView {
    let mut left = vec![
        widget(eyebrow_text(eyebrow)).key(format!("{key}-eyebrow")).into_any(),
        widget(pad(column(0.0), 4.0, 0.0, 0.0, 0.0))
            .children((widget(label(title, 24.0, 700, Role::Text)).key(format!("{key}-title")),))
            .into_any(),
    ];
    if !subline.is_empty() {
        left.push(
            widget(pad(column(0.0), 8.0, 0.0, 0.0, 0.0))
                .children((widget(wrapping(label(subline, 14.0, 400, Role::Muted))).key(format!("{key}-subline")),))
                .into_any(),
        );
    }
    let stats = widget(Stack::row(10.0).align(AlignSpec::Start).wrap(true).grow(0.0).shrink(0.0)).children(trailing);
    widget(spread(16.0, AlignSpec::Start))
        .children((
            widget(Stack::column(0.0).width(LengthSpec::Shrink).min_width(LengthSpec::Px(0.0)).grow(1.0).shrink(1.0)).children(left),
            stats,
        ))
        .key(format!("{key}-header"))
        .into_any()
}

/// `.asset-browser__eyebrow`：11/600 弱色、大写、字距 0.4。
pub(crate) fn eyebrow_text(value: &str) -> Text {
    let mut text = label(value.to_uppercase(), 11.0, 600, Role::Faint);
    Arc::make_mut(&mut text.style.layout).letter_spacing = Some(0.4);
    text
}

/// `.asset-browser__state`：外边距 16/20/0、内边距 10/12、xl 圆角、1px 边线、弱色字。
/// 错误时换成危险色柔和底和危险色字。
pub(crate) fn state_notice(text: String, error: bool, key: &'static str) -> AnyView {
    let body = pad(row(8.0), 10.0, 12.0, 10.0, 12.0);
    let (body, color) = if error {
        (body.painter(SoftFill::new(Soft::Danger, Some(RadiusTier::Xl)).bordered(Role::Danger, 1.0)), Role::Danger)
    } else {
        (body.surface(Role::Background).outline(Role::Border, 1.0).radius(RadiusTier::Xl), Role::Muted)
    };
    let body = body.with_layout(|layout| {
        layout.width = Some(LengthSpec::Shrink);
        layout.max_width = Some(LengthSpec::Fill);
    });
    widget(pad(column(0.0), 16.0, 20.0, 0.0, 20.0).align(AlignSpec::Start))
        .children((widget(body).children((widget(wrapping(label(text, 14.0, 400, color))).key(key),)),))
        .into_any()
}

/// `.search-workbench__empty`：虚线 `border-strong`、xl 圆角、内边距 24、`bg-subtle`，
/// 18 号粗体标题和弱色说明竖排，间距 8，竖直居中并占满剩余高度。
pub(crate) fn dashed_empty(title: &str, message: &str, key: &'static str) -> AnyView {
    let frame = pad(Stack::fill_column(8.0), 24.0, 24.0, 24.0, 24.0)
        .justify(JustifySpec::Center)
        .surface(Role::Subtle)
        .outline(Role::BorderStrong, 1.0)
        .radius(RadiusTier::Xl)
        .with_layout(|layout| {
            layout.border_style = Some(nana_ui_core::BorderStyle::Dashed);
            layout.height = Some(LengthSpec::Shrink);
            layout.flex_grow = Some(1.0);
        });
    widget(frame)
        .children((
            widget(label(title, 18.0, 700, Role::Text)).key(format!("{key}-title")),
            widget(wrapping(label(message, 14.0, 400, Role::Muted))).key(format!("{key}-detail")),
        ))
        .key(key)
        .into_any()
}

/// `.search-workbench__field`：主背景、1px 边线、lg 圆角，最小高 38，左右 12，间距 8，弱色。
pub(crate) fn field_frame() -> Stack {
    pad(Stack::fill_row(8.0), 0.0, 13.0, 0.0, 13.0)
        .surface(Role::Background)
        .outline(Role::Border, 1.0)
        .radius(RadiusTier::Lg)
        .with_layout(|layout| {
            layout.min_height = Some(LengthSpec::Px(38.0));
            layout.flex_grow = Some(0.0);
        })
}

/// 嵌在 [`field_frame`] 里的输入框：透明、无边、无内边距，高 32，14 号字。
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
        layout.font_size = Some(14.0);
        layout.line_height = Some(LineHeightSpec::Relative(LINE));
        layout.white_space_nowrap = true;
        layout.overflow_x = nana_ui_core::OverflowSpec::Hidden;
        layout.overflow_y = nana_ui_core::OverflowSpec::Hidden;
    }
    input.style(style)
}

/// Vue 原生 `<input>`：高 32、内边距 6/10、sm 圆角、1px `border-strong`、`bg-subtle`，
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
            focused: SemanticPaint { background: Some(Role::Background), border: Some(Role::BorderStrong), ..SemanticPaint::default() },
            ..InteractionStyle::default()
        },
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
        layout.font_size = Some(14.0);
        layout.line_height = Some(LineHeightSpec::Relative(LINE));
        layout.white_space_nowrap = true;
        layout.overflow_x = nana_ui_core::OverflowSpec::Hidden;
        layout.overflow_y = nana_ui_core::OverflowSpec::Hidden;
    }
    style
}

/// Vue 里只能在绘制时按主题算的柔和底色。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Soft {
    /// `--ok-soft`：浅色主题 10%、深色主题 14% 的成功色。
    Success,
    /// `--err-soft`：浅色主题 10%、深色主题 14% 的危险色。
    Danger,
    /// 某个角色按固定透明度，对应 `color-mix(in srgb, X N%, transparent)`。
    Alpha { role: Role, alpha: f32 },
    /// `color-mix(in srgb, <role> N%, <base>)`。
    RoleMix { role: Role, ratio: f32, base: Role },
    /// `color-mix(in srgb, #rrggbb N%, <base>)`，Vue 写死的日志级别色。
    RgbMix { rgb: [u8; 3], ratio: f32, base: Role },
}

/// 柔和底色绘制器。只画自身底色和可选描边，子节点照常绘制。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SoftFill {
    soft: Soft,
    /// `None` 是药丸；其余按主题圆角档位。
    radius: Option<RadiusTier>,
    border: Option<(Role, f32)>,
}

impl SoftFill {
    pub(crate) fn new(soft: Soft, radius: Option<RadiusTier>) -> Self {
        Self { soft, radius, border: None }
    }

    /// 同时描一圈 1px 角色色边线。
    pub(crate) fn bordered(mut self, role: Role, width: f32) -> Self {
        self.border = Some((role, width));
        self
    }

    fn resolve(&self, cx: &PaintContext<'_>) -> [f32; 4] {
        let dark = cx.theme_mode() == ThemeMode::Dark;
        match self.soft {
            Soft::Success => cx.color(SemanticColorMix::alpha(Role::Success, if dark { 0.14 } else { 0.10 })),
            Soft::Danger => cx.color(SemanticColorMix::alpha(Role::Danger, if dark { 0.14 } else { 0.10 })),
            Soft::Alpha { role, alpha } => cx.color(SemanticColorMix::alpha(role, alpha)),
            Soft::RoleMix { role, ratio, base } => mix(cx.color(role), cx.color(base), ratio),
            Soft::RgbMix { rgb, ratio, base } => mix(rgba(rgb), cx.color(base), ratio),
        }
    }
}

/// sRGB 里按比例混两种颜色，等价于 CSS `color-mix(in srgb, a ratio, b)`。
fn mix(first: [f32; 4], second: [f32; 4], ratio: f32) -> [f32; 4] {
    let ratio = ratio.clamp(0.0, 1.0);
    let alpha = first[3] * ratio + second[3] * (1.0 - ratio);
    if alpha <= 0.0 {
        return [0.0; 4];
    }
    let channel = |a: f32, b: f32| (a * first[3] * ratio + b * second[3] * (1.0 - ratio)) / alpha;
    [channel(first[0], second[0]), channel(first[1], second[1]), channel(first[2], second[2]), alpha]
}

impl Painter for SoftFill {
    fn paint(&self, cx: &mut PaintContext<'_>) {
        let fill = self.resolve(cx);
        let radius = match self.radius {
            Some(tier) => cx.radius(tier),
            None => cx.size()[1] / 2.0,
        };
        let mut paint = BoxPaint::fill(fill);
        if let Some((role, width)) = self.border {
            paint = paint.border(role, width);
        }
        let bounds = cx.bounds();
        cx.rounded_rect(bounds, radius, paint);
    }

    fn paint_key(&self) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        format!("{self:?}").hash(&mut hasher);
        hasher.finish()
    }
}
