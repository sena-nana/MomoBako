//! 搜索面板和筛选栏的自绘：颜色芯片前的色块、结果行的文件图标块。
//!
//! 色块要画元数据里的真实颜色，不能折成语义色，所以用 `PaintColor::Rgba`；
//! 色块外圈和 Vue 一样是 70% 的 `--border-strong`。结果图标块是 Vue
//! `.search-workbench__item-icon` 的 135° 渐变底加 lucide `FileImage` 线框。

use std::hash::{Hash, Hasher};

use nana_ui::runtime::{BoxPaint, LayoutBox, LineCap, LineJoin, PaintColor, PaintContext, PaintPath, Painter, StrokeStyle};
use nana_ui_core::{BackgroundImage, CssGradient, GradientStop, LinearGradient, SemanticColorMix, SemanticColorRole};

use super::presenter::SwatchColor;

/// 色块直径，对应 `.workspace-filter-chip__swatch` 的 9px。
pub(crate) const SWATCH_SIZE: f32 = 9.0;
/// 色块外圈宽度，对应 `box-shadow: 0 0 0 1px`。
const SWATCH_RING: f32 = 1.0;

/// 颜色芯片：先画芯片自己的外观，再在左内边距里画色块。
#[derive(Clone, Copy, Debug)]
pub(crate) struct SwatchChipPainter {
    pub color: SwatchColor,
    /// 色块左缘到芯片左缘的距离（边框加内边距）。
    pub inset: f32,
}

impl Painter for SwatchChipPainter {
    fn paint(&self, cx: &mut PaintContext<'_>) {
        cx.draw_default();
    }

    fn paint_over_children(&self, cx: &mut PaintContext<'_>) {
        let [_, height] = cx.size();
        let top = ((height - SWATCH_SIZE) * 0.5).max(0.0);
        let ring = LayoutBox {
            x: self.inset - SWATCH_RING,
            y: top - SWATCH_RING,
            width: SWATCH_SIZE + SWATCH_RING * 2.0,
            height: SWATCH_SIZE + SWATCH_RING * 2.0,
        };
        let dot = LayoutBox { x: self.inset, y: top, width: SWATCH_SIZE, height: SWATCH_SIZE };
        let fill = match self.color {
            SwatchColor::Rgb([red, green, blue]) => {
                PaintColor::Rgba([f32::from(red) / 255.0, f32::from(green) / 255.0, f32::from(blue) / 255.0, 1.0])
            }
            SwatchColor::Accent => PaintColor::Role(SemanticColorRole::Accent),
        };
        // 全圆角矩形走和控件相同的圆角着色，平滑圆角设置下和 Vue 一样是超椭圆。
        // 外圈按 70% 混进芯片底色，和 Vue 在 sRGB 里合成的 `color-mix` 一致。
        let ring_color = PaintColor::Mix(SemanticColorMix::new(SemanticColorRole::BorderStrong, SemanticColorRole::Background, 0.7));
        cx.rounded_rect(ring, 999.0, BoxPaint::fill(ring_color));
        cx.rounded_rect(dot, 999.0, BoxPaint::fill(fill));
    }

    fn paint_key(&self) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        match self.color {
            SwatchColor::Rgb(rgb) => rgb.hash(&mut hasher),
            SwatchColor::Accent => 0u8.hash(&mut hasher),
        }
        self.inset.to_bits().hash(&mut hasher);
        hasher.finish()
    }
}

/// 结果行图标块的渐变底：`linear-gradient(135deg, #8ba8b6 0%, #35525f 100%)`。
/// 图标块自己用 `Lg` 圆角，渐变跟着圆角裁剪。
pub(crate) fn hit_tile_background() -> BackgroundImage {
    let stop = |position: f32, [red, green, blue]: [u8; 3]| GradientStop {
        paint_color: None,
        position,
        color: [f32::from(red) / 255.0, f32::from(green) / 255.0, f32::from(blue) / 255.0, 1.0],
    };
    BackgroundImage::Gradient(CssGradient::Linear(LinearGradient {
        angle_deg: 135.0,
        stops: vec![stop(0.0, [0x8b, 0xa8, 0xb6]), stop(1.0, [0x35, 0x52, 0x5f])],
    }))
}

/// 结果行左侧 40px 图标块上的白色 18px 文件图片线框（lucide `FileImage`）。
#[derive(Clone, Copy, Debug)]
pub(crate) struct HitIconPainter;

/// lucide 图标的视框边长。
const ICON_VIEWBOX: f32 = 24.0;
/// 图标绘制尺寸，对应 `<FileImage :size="18" />`。
const ICON_SIZE: f32 = 18.0;

impl Painter for HitIconPainter {
    fn paint(&self, cx: &mut PaintContext<'_>) {
        cx.draw_default();
        let [width, height] = cx.size();
        let scale = ICON_SIZE / ICON_VIEWBOX;
        let origin = [(width - ICON_SIZE) * 0.5, (height - ICON_SIZE) * 0.5];
        let stroke = StrokeStyle::new(2.0 * scale).cap(LineCap::Round).join(LineJoin::Round);
        let white = PaintColor::Rgba([1.0, 1.0, 1.0, 1.0]);
        for path in file_image_paths(origin, scale) {
            cx.stroke_path(&path, stroke.clone(), white);
        }
    }

    fn paint_key(&self) -> u64 {
        1
    }
}

/// lucide `file-image`（v1.24）四段路径，按视框坐标缩放到 `origin` 起的 18px 方块。
fn file_image_paths(origin: [f32; 2], scale: f32) -> Vec<PaintPath> {
    let point = |x: f32, y: f32| (origin[0] + x * scale, origin[1] + y * scale);
    let mut sheet = PaintPath::new();
    // M6 22 a2 2 0 0 1-2-2 V4 a2 2 0 0 1 2-2 h8 a2.4 2.4 … l3.588 3.588 A2.4 2.4 0 0 1 20 8 v12 a2 2 0 0 1-2 2 z
    let (x, y) = point(6.0, 22.0);
    sheet.move_to(x, y);
    let (cx1, cy1) = point(4.0, 22.0);
    let (x2, y2) = point(4.0, 20.0);
    sheet.arc_to(cx1, cy1, x2, y2, 2.0 * scale);
    let (cx2, cy2) = point(4.0, 2.0);
    let (x3, y3) = point(6.0, 2.0);
    sheet.arc_to(cx2, cy2, x3, y3, 2.0 * scale);
    let (cx3, cy3) = point(15.0, 2.0);
    let (x4, y4) = point(19.292, 6.294);
    sheet.arc_to(cx3, cy3, x4, y4, 2.4 * scale);
    let (cx4, cy4) = point(20.0, 7.0);
    let (x5, y5) = point(20.0, 20.0);
    sheet.arc_to(cx4, cy4, x5, y5, 2.4 * scale);
    let (cx5, cy5) = point(20.0, 22.0);
    let (x6, y6) = point(18.0, 22.0);
    sheet.arc_to(cx5, cy5, x6, y6, 2.0 * scale);
    sheet.close();

    let mut fold = PaintPath::new();
    // M14 2 v5 a1 1 0 0 0 1 1 h5
    let (x, y) = point(14.0, 2.0);
    fold.move_to(x, y);
    let (cx1, cy1) = point(14.0, 8.0);
    let (x2, y2) = point(20.0, 8.0);
    fold.arc_to(cx1, cy1, x2, y2, scale);
    fold.line_to(x2, y2);

    let mut sun = PaintPath::new();
    let (left, top) = point(8.0, 10.0);
    sun.ellipse(LayoutBox { x: left, y: top, width: 4.0 * scale, height: 4.0 * scale });

    let mut ridge = PaintPath::new();
    // m20 17 -1.296-1.296 a2.41 2.41 0 0 0-3.408 0 L9 22
    let (x, y) = point(20.0, 17.0);
    ridge.move_to(x, y);
    let (cx1, cy1) = point(17.0, 14.0);
    let (x2, y2) = point(9.0, 22.0);
    ridge.arc_to(cx1, cy1, x2, y2, 2.41 * scale);
    ridge.line_to(x2, y2);

    vec![sheet, fold, sun, ridge]
}
