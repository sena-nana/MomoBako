//! 文件页本地样式：字号、行高、行分隔、阴影和缩略图底色的自绘。
//!
//! 间距、行高、圆角、边框宽度都取主题档位（[`crate::theme_map::tiers`]），颜色只用主题角色和角色混色。
//! 缩略图占位底色是 `--border-strong` 和 `--bg-active` 的混色；文件夹是警告色到警告色压暗的渐变，
//! 压暗取主题的媒体遮罩色，深浅主题都是棕色，二者走自绘。
//! `workbench.rs` 的公共小部件外观不改，文件卡片、详情和元数据编辑只用这里的辅助函数。

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use nana_ui::runtime::{
    Gradient, LayoutBox, LengthSpec, PaintContext, Painter, RadiusTier, SemanticColorRole, Stack,
    Text, TextHorizontalAlignment,
};
use nana_ui_core::{
    BorderWidth, ControlHeight, ControlPadding, ControlSize, Icon, LayoutStyle, LineHeightSpec,
    LineRole, SemanticColorMix, SpacingStep, TextWeight, ThemeExpr, ThemeToken, TypeRole,
    TypographyTokens,
};

use crate::theme_map::tiers::{self, Edge};

/// 正文行高 1.55。
pub(crate) const LINE_BODY: f32 = 1.55;

/// 竖排，子项间距取主题间距档。
pub(crate) fn column(step: SpacingStep) -> Stack {
    Stack::column(0.0).gap_theme(step)
}

/// 横排，子项间距取主题间距档。
pub(crate) fn row(step: SpacingStep) -> Stack {
    Stack::row(0.0).gap_theme(step)
}

/// 内边距取主题间距档：左右一档、上下一档。
pub(crate) fn pad(stack: Stack, x: SpacingStep, y: SpacingStep) -> Stack {
    stack.with_layout(|layout| tiers::padding_xy(layout, x, y))
}

/// `count` 根发丝边的主题表达式：边框宽度、给边框预留的内边距用。
pub(crate) fn hairline_expr(count: f32) -> ThemeExpr {
    ThemeExpr {
        constant: 0.0,
        terms: vec![(ThemeToken::Border(BorderWidth::Hairline), count)],
    }
}

/// 四边各留一根发丝边宽的内边距：给只在某些状态描出来的边先占好位置，描边时内容不挪。
pub(crate) fn hairline_padding(layout: &mut LayoutStyle) {
    let px = Some(LengthSpec::Px(tiers::hairline_px()));
    layout.padding_top = px;
    layout.padding_right = px;
    layout.padding_bottom = px;
    layout.padding_left = px;
    layout.theme.padding_top = Some(hairline_expr(1.0));
    layout.theme.padding_right = Some(hairline_expr(1.0));
    layout.theme.padding_bottom = Some(hairline_expr(1.0));
    layout.theme.padding_left = Some(hairline_expr(1.0));
}

/// 绝对定位的四边偏移都取这一档：盖在父级上、四周内缩一档的浮层用。
pub(crate) fn inset(layout: &mut LayoutStyle, step: SpacingStep) {
    offset(layout, &Edge::ALL, step);
}

/// 绝对定位时给出的每条边的偏移都取这一档：浮在父级角上、离边一档的小件用。
pub(crate) fn offset(layout: &mut LayoutStyle, edges: &[Edge], step: SpacingStep) {
    let px = Some(LengthSpec::Px(tiers::space_px(step)));
    for edge in edges {
        let expr = Some(tiers::space_expr(step));
        match edge {
            Edge::Top => {
                layout.offset_top = px;
                layout.theme.offset_top = expr;
            }
            Edge::Right => {
                layout.offset_right = px;
                layout.theme.offset_right = expr;
            }
            Edge::Bottom => {
                layout.offset_bottom = px;
                layout.theme.offset_bottom = expr;
            }
            Edge::Left => {
                layout.offset_left = px;
                layout.theme.offset_left = expr;
            }
        }
    }
}

/// 停用、锁住时整块淡下去的不透明度：主题的停用状态层档。节点不透明度在建树时就要定，
/// 深浅两套主题的这一档相同，取深色档。
pub(crate) const fn disabled_opacity() -> f32 {
    tiers::opacity(nana_ui_core::StateLayer::Disabled, true)
}

/// 四边一圈发丝边，颜色取主题角色，宽度取主题的发丝边档。
pub(crate) fn hairline(stack: Stack, role: SemanticColorRole) -> Stack {
    stack
        .border(role, tiers::hairline_px())
        .with_layout(|layout| tiers::border(layout, 1))
}

/// 行高写成主题档位。正好是行盒档（16 / 18）的取 [`LineRole`]；字号在主题字阶上的按字号档的倍数写，
/// 比例不变，主题换字阶时一起变；字号不在字阶上的照写像素。
pub(crate) fn line_height(layout: &mut LayoutStyle, role: Option<TypeRole>, size: f32, px: f32) {
    let tokens = TypographyTokens::DEFAULT;
    if let Some(line) = [LineRole::Line, LineRole::Tall]
        .into_iter()
        .find(|line| tokens.line_height(*line) == px)
    {
        tiers::line(layout, line);
        return;
    }
    match role {
        Some(role) if size > 0.0 => tiers::line_ratio(layout, role, px / size),
        _ => {
            layout.line_height = Some(LineHeightSpec::Absolute(px));
            layout.theme.line_height = None;
        }
    }
}

/// 一段文字。行高给像素，按 [`line_height`] 折成主题档位。
pub(crate) fn text(
    label: impl Into<String>,
    size: f32,
    weight: TextWeight,
    role: SemanticColorRole,
    line_height_px: f32,
) -> Text {
    let mut node = crate::theme_map::typed_text(Text::new(label.into()), size, weight).color(role);
    let type_role = node.style.type_role;
    line_height(
        std::sync::Arc::make_mut(&mut node.style.layout),
        type_role,
        size,
        line_height_px,
    );
    node
}

/// 正文：14px、400、主文本色，行高 1.55 倍。
pub(crate) fn body(label: impl Into<String>) -> Text {
    text(
        label,
        14.0,
        TextWeight::Regular,
        SemanticColorRole::Text,
        14.0 * LINE_BODY,
    )
}

/// 次级说明：12px、400、弱化色，行高 1.55 倍。
pub(crate) fn small_muted(label: impl Into<String>) -> Text {
    text(
        label,
        12.0,
        TextWeight::Regular,
        SemanticColorRole::Muted,
        12.0 * LINE_BODY,
    )
}

/// 微型标签的字距 0.4。主题没有字距档，照写像素。
const LABEL_TRACKING: f32 = 0.4;

/// 元数据行的标签：11px、700、最弱色，字距 0.4。
pub(crate) fn row_label(label: impl Into<String>) -> Text {
    let mut node = text(
        label,
        11.0,
        TextWeight::Bold,
        SemanticColorRole::Faint,
        11.0 * LINE_BODY,
    );
    std::sync::Arc::make_mut(&mut node.style.layout)
        .text_ext
        .letter_spacing = Some(LABEL_TRACKING);
    node
}

/// 眉题：11px、600、最弱色，字距 0.4。
pub(crate) fn eyebrow(label: impl Into<String>) -> Text {
    let mut node = text(
        label,
        11.0,
        TextWeight::Semibold,
        SemanticColorRole::Faint,
        11.0 * LINE_BODY,
    );
    std::sync::Arc::make_mut(&mut node.style.layout)
        .text_ext
        .letter_spacing = Some(LABEL_TRACKING);
    node.nowrap(true)
}

/// 详情标题：18px、700，行高 1.55 倍。
pub(crate) fn heading(label: impl Into<String>) -> Text {
    wrapping(text(
        label,
        18.0,
        TextWeight::Bold,
        SemanticColorRole::Text,
        18.0 * LINE_BODY,
    ))
}

/// 详情副行：14px 弱化色，长路径按列宽断行。
pub(crate) fn subline(label: impl Into<String>) -> Text {
    wrapping(text(
        label,
        14.0,
        TextWeight::Regular,
        SemanticColorRole::Muted,
        14.0 * LINE_BODY,
    ))
}

/// 值文本：14px 主文本色，长值按列宽断行。
pub(crate) fn value(label: impl Into<String>) -> Text {
    wrapping(body(label))
}

/// 按父列宽折行，任意位置可断，和 `overflow-wrap: anywhere` 一致。
pub(crate) fn wrapping(mut node: Text) -> Text {
    let layout = std::sync::Arc::make_mut(&mut node.style.layout);
    layout.width = Some(LengthSpec::Fill);
    layout.min_width = Some(LengthSpec::Px(0.0));
    layout.text_ext.overflow_wrap = Some(nana_ui_core::OverflowWrapSpec::Anywhere);
    node
}

/// 上外边距取主题间距档，标题和段落用。
pub(crate) fn margin_top(mut node: Text, step: SpacingStep) -> Text {
    tiers::margin(
        std::sync::Arc::make_mut(&mut node.style.layout),
        &[Edge::Top],
        step,
    );
    node
}

/// 文字居中。卡片标题和对话框按钮用。
pub(crate) fn centered(mut node: Text) -> Text {
    node.style.text_horizontal_alignment = TextHorizontalAlignment::Center;
    node
}

/// 元数据行：标签在上、值在下，间距 `Sm`；除第一行外上边一条发丝分隔，上内边距 `Xl`。
pub(crate) fn meta_row(first: bool) -> Stack {
    let row = column(SpacingStep::Sm)
        .width(LengthSpec::Fill)
        .min_width(LengthSpec::Px(0.0));
    if first {
        row
    } else {
        top_rule(row, SpacingStep::Xl)
    }
}

/// 上边一条 `--border-soft` 发丝线，并留出一档上内边距。
pub(crate) fn top_rule(stack: Stack, padding_top: SpacingStep) -> Stack {
    let mut style = stack.node_style();
    style.border = Some(SemanticColorRole::BorderSoft);
    let layout = std::sync::Arc::make_mut(&mut style.layout);
    layout.border_sides.top_width = Some(tiers::hairline_px());
    layout.theme.border_top_width = Some(hairline_expr(1.0));
    tiers::padding(layout, &[Edge::Top], padding_top);
    stack.style(style)
}

/// 下边一条 `--border-soft` 发丝线。
pub(crate) fn bottom_rule(stack: Stack) -> Stack {
    let mut style = stack.node_style();
    style.border = Some(SemanticColorRole::BorderSoft);
    let layout = std::sync::Arc::make_mut(&mut style.layout);
    layout.border_sides.bottom_width = Some(tiers::hairline_px());
    layout.theme.border_bottom_width = Some(hairline_expr(1.0));
    stack.style(style)
}

/// 固定宽高，不随父级伸缩。
pub(crate) fn fixed(stack: Stack, width: f32, height: f32) -> Stack {
    stack
        .width(LengthSpec::Px(width))
        .height(LengthSpec::Px(height))
        .min_width(LengthSpec::Px(width))
        .min_height(LengthSpec::Px(height))
        .grow(0.0)
        .shrink(0.0)
}

/// 按钮左右内边距：控件内边距档、间距档，或者不留。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Inset {
    None,
    Control(ControlPadding),
    Step(SpacingStep),
}

/// 按钮高度：控件高度档，或者控件档以外的像素（工具栏 34、文字行盒一类）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Height {
    Control(ControlSize),
    Px(f32),
}

/// 文件页专用按钮的外观（工具栏按钮、面包屑项一类）。
#[derive(Clone, Copy, Debug)]
pub(crate) struct ButtonLook {
    pub height: Height,
    pub padding_x: Inset,
    pub foreground: SemanticColorRole,
    pub background: Option<SemanticColorRole>,
    pub hover: Option<SemanticColorRole>,
    pub radius: RadiusTier,
    pub icon_size: f32,
    pub gap: SpacingStep,
}

/// 工具栏按钮高 34：控件档在 32 和 36 之间，没有对应的档，照写像素。
pub(crate) const TOOLBAR_HEIGHT: f32 = 34.0;
/// 工具栏图标 14px。主题没有图标尺寸档，照写像素。
pub(crate) const TOOLBAR_ICON: f32 = 14.0;

impl ButtonLook {
    /// 工具栏按钮：高 34，左右取控件内边距档，字号、字重取按钮的正文档和中等档，常态透明，悬停 `--bg-hover`。
    pub(crate) const TOOLBAR: Self = Self {
        height: Height::Px(TOOLBAR_HEIGHT),
        padding_x: Inset::Control(ControlPadding::Standard),
        foreground: SemanticColorRole::Text,
        background: None,
        hover: Some(SemanticColorRole::Hover),
        radius: RadiusTier::Sm,
        icon_size: TOOLBAR_ICON,
        gap: SpacingStep::Sm,
    };
}

/// 按 [`ButtonLook`] 画的按钮。禁用时只把字压成最弱色，不加底和边。
pub(crate) fn styled_button(
    label: impl Into<String>,
    icon: Option<Icon>,
    look: ButtonLook,
) -> nana_ui::runtime::Button {
    let mut button = nana_ui::runtime::Button::new(label);
    if let Some(icon) = icon {
        button = button.icon(icon).icon_size(look.icon_size);
    }
    button = button.icon_gap(tiers::space_px(look.gap));
    let mut style = button.style.clone();
    style.foreground = Some(look.foreground);
    style.background = look.background;
    style.border = None;
    style.radius = Some(look.radius);
    style.control_height = match look.height {
        Height::Control(size) => Some(ControlHeight::Exact(size)),
        Height::Px(_) => None,
    };
    style.control_padding_x = match look.padding_x {
        Inset::Control(padding) => Some(padding),
        Inset::None | Inset::Step(_) => None,
    };
    style.control_padding_y = None;
    style.interaction.hovered = nana_ui::runtime::SemanticPaint {
        background: look.hover.or(look.background),
        ..Default::default()
    };
    style.interaction.pressed = nana_ui::runtime::SemanticPaint {
        background: look.hover.or(look.background),
        ..Default::default()
    };
    style.interaction.disabled = nana_ui::runtime::SemanticPaint {
        foreground: Some(SemanticColorRole::Faint),
        background: look.background,
        ..Default::default()
    };
    let layout = std::sync::Arc::make_mut(&mut style.layout);
    let height = match look.height {
        Height::Control(size) => size.height_in(nana_ui::theme::UI_METRICS),
        Height::Px(px) => px,
    };
    layout.height = Some(LengthSpec::Px(height));
    layout.min_height = Some(LengthSpec::Px(height));
    match look.padding_x {
        Inset::None => {
            layout.padding_left = Some(LengthSpec::Px(0.0));
            layout.padding_right = Some(LengthSpec::Px(0.0));
            layout.theme.padding_left = None;
            layout.theme.padding_right = None;
        }
        Inset::Control(padding) => {
            let px = Some(LengthSpec::Px(padding.resolve(nana_ui::theme::UI_METRICS)));
            layout.padding_left = px;
            layout.padding_right = px;
        }
        Inset::Step(step) => tiers::padding(layout, &Edge::X, step),
    }
    layout.padding_top = Some(LengthSpec::Px(0.0));
    layout.padding_bottom = Some(LengthSpec::Px(0.0));
    layout.flex_grow = Some(0.0);
    layout.flex_shrink = Some(0.0);
    button.style(style)
}

/// 缩略图盒的底色：主题占位灰或文件夹棕色渐变。
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum Tone {
    Placeholder,
    Folder,
}

/// 缩略图盒的自绘：底色、已有像素（cover 铺满）或居中的类型图标。
///
/// 列表和详情的缩略图盒都用它：底是 `Tone`，图标是媒体前景色（深浅主题都是白）92% 不透明。
/// 底色都从主题角色解析，主题切换时运行时会重录。
#[derive(Clone, Debug)]
pub(crate) struct PreviewPainter {
    pub tone: Tone,
    pub icon: Option<Icon>,
    pub icon_size: f32,
    pub radius: RadiusTier,
}

/// 缩略图占位底色：`--border-strong` 35% 混 `--bg-active`。深色约 #373737，浅色约 #d8dbe1。
pub(crate) const PLACEHOLDER: SemanticColorMix = SemanticColorMix::new(
    SemanticColorRole::BorderStrong,
    SemanticColorRole::Active,
    0.35,
);

/// 缩略图上的类型图标：媒体前景色 92% 不透明。
const PREVIEW_ICON: SemanticColorMix = SemanticColorMix::alpha(SemanticColorRole::OnMedia, 0.92);

/// 文件夹渐变末端压进媒体遮罩色的比例。
const FOLDER_SHADE: f32 = 0.45;

/// 两个颜色在 sRGB 里按比例混，结果不透明：`t` 是 `b` 的份额。
pub(crate) fn mix_rgb(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
        1.0,
    ]
}

/// 135° 线性渐变线：起点在左上、终点在右下，`from` 到 `to`。
///
/// 算法：CSS 渐变线穿过盒子中心，方向角 135°，长度是 |w·sinθ| + |h·cosθ|，
/// 两端各取半长，得到起点和终点。
fn folder_gradient(bounds: LayoutBox, from: [f32; 4], to: [f32; 4]) -> Gradient {
    let angle = 135f32.to_radians();
    let (sin, cos) = angle.sin_cos();
    let half = (bounds.width * sin.abs() + bounds.height * cos.abs()) / 2.0;
    let center = [
        bounds.x + bounds.width / 2.0,
        bounds.y + bounds.height / 2.0,
    ];
    let direction = [sin, -cos];
    let start = [
        center[0] - direction[0] * half,
        center[1] - direction[1] * half,
    ];
    let end = [
        center[0] + direction[0] * half,
        center[1] + direction[1] * half,
    ];
    Gradient::linear(start, end).stop(0.0, from).stop(1.0, to)
}

impl Painter for PreviewPainter {
    fn paint(&self, cx: &mut PaintContext<'_>) {
        let bounds = cx.bounds();
        let radius = self.radius;
        match self.tone {
            Tone::Placeholder => {
                cx.rounded_rect(
                    bounds,
                    radius,
                    nana_ui::runtime::BoxPaint::fill(PLACEHOLDER),
                );
            }
            Tone::Folder => {
                let warning = cx.color(SemanticColorRole::Warning);
                let scrim = cx.theme().effects().media_scrim.as_rgba_array();
                let shaded = mix_rgb(warning, scrim, FOLDER_SHADE);
                cx.rounded_rect(
                    bounds,
                    radius,
                    nana_ui::runtime::BoxPaint::fill(folder_gradient(bounds, warning, shaded)),
                );
            }
        }
        if let Some(icon) = self.icon {
            let size = self.icon_size;
            let rect = LayoutBox {
                x: bounds.x + (bounds.width - size) / 2.0,
                y: bounds.y + (bounds.height - size) / 2.0,
                width: size,
                height: size,
            };
            cx.icon(rect, icon, PREVIEW_ICON);
        }
    }

    fn paint_key(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.tone.hash(&mut hasher);
        self.icon.map(|icon| icon.name()).hash(&mut hasher);
        self.icon_size.to_bits().hash(&mut hasher);
        format!("{:?}", self.radius).hash(&mut hasher);
        hasher.finish()
    }
}

/// `#RRGGBB` 转成 0–1 的 RGBA。不是合法颜色时返回 `None`。
pub(crate) fn hex_rgba(hex: &str) -> Option<[f32; 4]> {
    let digits = hex.strip_prefix('#')?;
    if digits.len() != 6 || !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let channel = |index: usize| {
        u8::from_str_radix(&digits[index..index + 2], 16)
            .ok()
            .map(|value| f32::from(value) / 255.0)
    };
    Some([channel(0)?, channel(2)?, channel(4)?, 1.0])
}

/// 色板里有没有能画出来的颜色：一个都没有时色板整行不占位，和建色块时跳过不合法颜色同一规则。
pub(crate) fn any_color(colors: &[String]) -> bool {
    colors.iter().any(|color| hex_rgba(color).is_some())
}

/// 拼进组装键的用户文本。NanaUI 用 `/` 连接键路径，键里带 `/` 会让整棵树挂载失败；
/// 反斜杠一并换成 `∕`，和条目键的写法一致。
pub(crate) fn key_part(text: &str) -> String {
    text.replace(['/', '\\'], "\u{2215}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_part_never_contains_the_path_separator() {
        assert_eq!(key_part("notes/page.pdf"), "notes\u{2215}page.pdf");
        assert_eq!(key_part("a\\b"), "a\u{2215}b");
        assert_eq!(key_part("封面"), "封面");
    }

    #[test]
    fn hex_colors_parse_only_full_rgb() {
        assert_eq!(hex_rgba("#ff0000"), Some([1.0, 0.0, 0.0, 1.0]));
        assert_eq!(
            hex_rgba("#C7A566").map(|rgba| (rgba[0] * 255.0).round() as u8),
            Some(199)
        );
        assert_eq!(hex_rgba("ff0000"), None);
        assert_eq!(hex_rgba("#fff"), None);
        assert_eq!(hex_rgba("#gg0000"), None);
    }

    #[test]
    fn folder_gradient_runs_from_top_left_to_bottom_right() {
        let gradient = folder_gradient(
            LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 130.0,
                height: 112.0,
            },
            [1.0; 4],
            [0.0, 0.0, 0.0, 1.0],
        );
        let nana_ui::runtime::GradientShape::Linear { start, end } = gradient.shape else {
            panic!("文件夹底色应是线性渐变");
        };
        assert!(
            start[0] < end[0] && start[1] < end[1],
            "135° 从左上到右下：{start:?} → {end:?}"
        );
        let length = ((end[0] - start[0]).powi(2) + (end[1] - start[1]).powi(2)).sqrt();
        assert!((length - (130.0 + 112.0) * std::f32::consts::FRAC_1_SQRT_2).abs() < 0.01);
    }

    /// 行高落在行盒档上的取行盒档，字号在字阶上的按字号档的倍数写，其余照写像素；都写进主题令牌。
    #[test]
    fn line_heights_land_on_theme_tiers() {
        let mut layout = LayoutStyle::default();
        line_height(&mut layout, Some(TypeRole::Meta), 12.0, 16.0);
        assert_eq!(
            layout.theme.line_height,
            Some(nana_ui_core::ThemeToken::Line(LineRole::Line).into())
        );
        line_height(&mut layout, Some(TypeRole::Section), 14.0, 14.0 * LINE_BODY);
        assert!(
            matches!(layout.line_height, Some(LineHeightSpec::Absolute(px)) if (px - 21.7).abs() < 0.001)
        );
        assert!(layout.theme.line_height.is_some());
        line_height(&mut layout, None, 17.0, 20.0);
        assert_eq!(layout.line_height, Some(LineHeightSpec::Absolute(20.0)));
        assert_eq!(layout.theme.line_height, None);
    }
}
