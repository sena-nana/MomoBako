//! 文件页本地样式：字号、行高、行分隔、阴影和缩略图底色的自绘。
//!
//! 数值来自 Vue `workspace.css` 的 files-* / asset-* 与 `@lilia/theme` 令牌，颜色只用主题角色；
//! 缩略图占位底色随主题在 #d8dce2 与 #3a3a3a 间切换，文件夹是固定的棕色渐变，二者走自绘。
//! `workbench.rs` 的公共小部件外观不改，文件卡片、详情和元数据编辑只用这里的辅助函数。

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use nana_ui::runtime::{
    Gradient, ImageFit, LayoutBox, LengthSpec, PaintContext, Painter, RadiusTier, SemanticColorRole, Stack, Text,
    TextHorizontalAlignment,
};
use nana_ui_core::{BoxShadowSpec, Icon, ThemeMode};

/// Vue 正文行高 1.55。
const LINE_BODY: f32 = 1.55;

/// 一段文字。行高按 CSS 的倍数折成像素，和 Vue 计算样式一致。
pub(crate) fn text(label: impl Into<String>, size: f32, weight: u16, role: SemanticColorRole, line_height: f32) -> Text {
    Text::new(label.into()).font_size(size).font_weight(weight).color(role).line_height(line_height)
}

/// 正文：14px、400、主文本色，行高 21.7。
pub(crate) fn body(label: impl Into<String>) -> Text {
    text(label, 14.0, 400, SemanticColorRole::Text, 14.0 * LINE_BODY)
}

/// 次级说明：12px、400、弱化色，行高 18.6。
pub(crate) fn small_muted(label: impl Into<String>) -> Text {
    text(label, 12.0, 400, SemanticColorRole::Muted, 12.0 * LINE_BODY)
}

/// `asset-meta__row > span:first-child`：11px、700、最弱色，字距 0.4。
pub(crate) fn row_label(label: impl Into<String>) -> Text {
    let mut node = text(label, 11.0, 700, SemanticColorRole::Faint, 11.0 * LINE_BODY);
    std::sync::Arc::make_mut(&mut node.style.layout).letter_spacing = Some(0.4);
    node
}

/// `asset-browser__eyebrow`：11px、600、最弱色，字距 0.4。
pub(crate) fn eyebrow(label: impl Into<String>) -> Text {
    let mut node = text(label, 11.0, 600, SemanticColorRole::Faint, 11.0 * LINE_BODY);
    std::sync::Arc::make_mut(&mut node.style.layout).letter_spacing = Some(0.4);
    node.nowrap(true)
}

/// 详情标题 `files-detail__section h2`：18px、700，行高 27.9。
pub(crate) fn heading(label: impl Into<String>) -> Text {
    wrapping(text(label, 18.0, 700, SemanticColorRole::Text, 18.0 * LINE_BODY))
}

/// `files-detail__subline`：14px 弱化色，长路径按列宽断行。
pub(crate) fn subline(label: impl Into<String>) -> Text {
    wrapping(text(label, 14.0, 400, SemanticColorRole::Muted, 14.0 * LINE_BODY))
}

/// 值文本 `asset-meta__value`：14px 主文本色，长值按列宽断行。
pub(crate) fn value(label: impl Into<String>) -> Text {
    wrapping(body(label))
}

/// 按父列宽折行，任意位置可断，和 `overflow-wrap: anywhere` 一致。
pub(crate) fn wrapping(mut node: Text) -> Text {
    let layout = std::sync::Arc::make_mut(&mut node.style.layout);
    layout.width = Some(LengthSpec::Fill);
    layout.min_width = Some(LengthSpec::Px(0.0));
    layout.overflow_wrap = Some(nana_ui_core::OverflowWrapSpec::Anywhere);
    node
}

/// 文字居中。卡片标题和对话框按钮用。
pub(crate) fn centered(mut node: Text) -> Text {
    node.style.text_horizontal_alignment = TextHorizontalAlignment::Center;
    node
}

/// `asset-meta__row`：标签在上、值在下，间距 6；除第一行外上边一条 1px 分隔，上内边距 12。
pub(crate) fn meta_row(first: bool) -> Stack {
    let row = Stack::column(6.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0));
    if first { row } else { top_rule(row, 12.0) }
}

/// 上边一条 `--border-soft` 发丝线，并留出指定的上内边距。
pub(crate) fn top_rule(stack: Stack, padding_top: f32) -> Stack {
    let mut style = stack.node_style();
    style.border = Some(SemanticColorRole::BorderSoft);
    let layout = std::sync::Arc::make_mut(&mut style.layout);
    layout.border_top_width = Some(1.0);
    layout.padding_top = Some(LengthSpec::Px(padding_top));
    stack.style(style)
}

/// 下边一条 `--border-soft` 发丝线。
pub(crate) fn bottom_rule(stack: Stack) -> Stack {
    let mut style = stack.node_style();
    style.border = Some(SemanticColorRole::BorderSoft);
    std::sync::Arc::make_mut(&mut style.layout).border_bottom_width = Some(1.0);
    stack.style(style)
}

/// CSS `box-shadow` 的一层，颜色是固定的 RGBA。
pub(crate) fn shadow(offset_y: f32, blur: f32, spread: f32, alpha: f32) -> BoxShadowSpec {
    BoxShadowSpec { offset_x: 0.0, offset_y, blur_radius: blur, spread_radius: spread, color: [0.0, 0.0, 0.0, alpha], inset: false }
}

/// 给容器挂上一组阴影。
pub(crate) fn with_shadows(stack: Stack, shadows: Vec<BoxShadowSpec>) -> Stack {
    stack.with_layout(move |layout| layout.paint.box_shadows = shadows)
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

/// 缩略图盒的底色：无底、主题占位灰或文件夹棕色渐变。
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum Tone {
    None,
    Placeholder,
    Folder,
}

/// 缩略图盒的自绘：底色、已有像素（cover 铺满）或居中的白色类型图标。
///
/// 对应 Vue `.files-list__preview` / `.files-detail__preview`：底是 `fileTone`，图标是
/// `rgba(255, 255, 255, 0.92)`。占位灰随主题切换，主题切换时运行时会重录。
#[derive(Clone, Debug)]
pub(crate) struct PreviewPainter {
    pub tone: Tone,
    pub icon: Option<Icon>,
    pub icon_size: f32,
    pub radius: RadiusTier,
    /// 已经缩小好的像素，PNG data URL。
    pub image: Option<Arc<str>>,
}

/// Vue `--thumbnail-placeholder-bg`：浅色 #d8dce2，深色 #3a3a3a。
fn placeholder_rgba(mode: ThemeMode) -> [f32; 4] {
    match mode {
        ThemeMode::Light => [216.0 / 255.0, 220.0 / 255.0, 226.0 / 255.0, 1.0],
        ThemeMode::Dark => [58.0 / 255.0, 58.0 / 255.0, 58.0 / 255.0, 1.0],
    }
}

/// `linear-gradient(135deg, #c7a566 0%, #73552f 100%)` 的渐变线。
///
/// 算法：CSS 渐变线穿过盒子中心，方向角 135°，长度是 |w·sinθ| + |h·cosθ|，
/// 两端各取半长，得到起点和终点。
fn folder_gradient(bounds: LayoutBox) -> Gradient {
    let angle = 135f32.to_radians();
    let (sin, cos) = angle.sin_cos();
    let half = (bounds.width * sin.abs() + bounds.height * cos.abs()) / 2.0;
    let center = [bounds.x + bounds.width / 2.0, bounds.y + bounds.height / 2.0];
    let direction = [sin, -cos];
    let start = [center[0] - direction[0] * half, center[1] - direction[1] * half];
    let end = [center[0] + direction[0] * half, center[1] + direction[1] * half];
    Gradient::linear(start, end)
        .stop(0.0, [199.0 / 255.0, 165.0 / 255.0, 102.0 / 255.0, 1.0])
        .stop(1.0, [115.0 / 255.0, 85.0 / 255.0, 47.0 / 255.0, 1.0])
}

impl Painter for PreviewPainter {
    fn paint(&self, cx: &mut PaintContext<'_>) {
        let bounds = cx.bounds();
        let radius = self.radius;
        match self.tone {
            Tone::None => {}
            Tone::Placeholder => {
                let fill = placeholder_rgba(cx.theme_mode());
                cx.rounded_rect(bounds, radius, nana_ui::runtime::BoxPaint::fill(fill));
            }
            Tone::Folder => {
                cx.rounded_rect(bounds, radius, nana_ui::runtime::BoxPaint::fill(folder_gradient(bounds)));
            }
        }
        if let Some(image) = &self.image {
            cx.image(bounds, Arc::clone(image), ImageFit::Cover, radius);
            return;
        }
        if let Some(icon) = self.icon {
            let size = self.icon_size;
            let rect = LayoutBox {
                x: bounds.x + (bounds.width - size) / 2.0,
                y: bounds.y + (bounds.height - size) / 2.0,
                width: size,
                height: size,
            };
            cx.icon(rect, icon, [1.0, 1.0, 1.0, 0.92]);
        }
    }

    fn paint_key(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.tone.hash(&mut hasher);
        self.icon.map(|icon| icon.name()).hash(&mut hasher);
        self.icon_size.to_bits().hash(&mut hasher);
        format!("{:?}", self.radius).hash(&mut hasher);
        self.image.as_deref().map(str::len).hash(&mut hasher);
        self.image.as_deref().map(|url| &url[url.len().saturating_sub(64)..]).hash(&mut hasher);
        hasher.finish()
    }
}

/// 按主题取固定色：浅色和深色各一组 sRGB。
#[derive(Clone, Copy, Debug)]
pub(crate) struct ThemedFill {
    pub light: [f32; 4],
    pub dark: [f32; 4],
    pub radius: RadiusTier,
}

impl Painter for ThemedFill {
    fn paint(&self, cx: &mut PaintContext<'_>) {
        let fill = match cx.theme_mode() {
            ThemeMode::Light => self.light,
            ThemeMode::Dark => self.dark,
        };
        let bounds = cx.bounds();
        cx.rounded_rect(bounds, self.radius, nana_ui::runtime::BoxPaint::fill(fill));
    }

    fn paint_key(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        for channel in self.light.iter().chain(self.dark.iter()) {
            channel.to_bits().hash(&mut hasher);
        }
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
    let channel = |index: usize| u8::from_str_radix(&digits[index..index + 2], 16).ok().map(|value| f32::from(value) / 255.0);
    Some([channel(0)?, channel(2)?, channel(4)?, 1.0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_colors_parse_only_full_rgb() {
        assert_eq!(hex_rgba("#ff0000"), Some([1.0, 0.0, 0.0, 1.0]));
        assert_eq!(hex_rgba("#C7A566").map(|rgba| (rgba[0] * 255.0).round() as u8), Some(199));
        assert_eq!(hex_rgba("ff0000"), None);
        assert_eq!(hex_rgba("#fff"), None);
        assert_eq!(hex_rgba("#gg0000"), None);
    }

    #[test]
    fn folder_gradient_runs_from_top_left_to_bottom_right() {
        let gradient = folder_gradient(LayoutBox { x: 0.0, y: 0.0, width: 130.0, height: 112.0 });
        let nana_ui::runtime::GradientShape::Linear { start, end } = gradient.shape else {
            panic!("文件夹底色应是线性渐变");
        };
        assert!(start[0] < end[0] && start[1] < end[1], "135° 从左上到右下：{start:?} → {end:?}");
        let length = ((end[0] - start[0]).powi(2) + (end[1] - start[1]).powi(2)).sqrt();
        assert!((length - (130.0 + 112.0) * std::f32::consts::FRAC_1_SQRT_2).abs() < 0.01);
    }
}
