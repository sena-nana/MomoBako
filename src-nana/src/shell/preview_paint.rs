//! 预览页自绘：插件预览框顶部的 accent-soft 光晕、音频舞台的唱片和中间的封面圆。
//!
//! 唱片纹路是 Vue 写死的 #151515 / #202020 / #181818，和主题无关，这里照用；其它颜色都取主题角色。

use nana_ui::runtime::{BoxPaint, Gradient, LayoutBox, PaintColor, PaintContext, PaintPath, Painter, SemanticColorRole};

/// 插件预览框：bg 底，顶部中间一圈 accent-soft 径向光晕，38% 处淡到透明。
/// 对应 `radial-gradient(circle at 50% 0%, var(--accent-soft), transparent 38%), var(--bg)`。
pub(super) struct PluginGlow;

impl Painter for PluginGlow {
    fn paint(&self, cx: &mut PaintContext<'_>) {
        let [width, height] = cx.size();
        let rect = LayoutBox { x: 0.0, y: 0.0, width, height };
        let radius = cx.radius(nana_ui_core::RadiusTier::Xl);
        cx.rounded_rect(rect, radius, BoxPaint::fill(SemanticColorRole::Background));
        // CSS 圆形渐变默认到最远角：从顶边中点到底角的距离。
        let reach = ((width / 2.0).powi(2) + height.powi(2)).sqrt().max(1.0);
        let glow = Gradient::radial([width / 2.0, 0.0], reach)
            .stop(0.0, SemanticColorRole::AccentSoft)
            .stop(0.38, PaintColor::Rgba([0.0, 0.0, 0.0, 0.0]));
        cx.rounded_rect(rect, radius, BoxPaint::fill(glow));
    }

    fn paint_key(&self) -> u64 {
        1
    }
}

/// 唱片：#181818 底上交替的 3px 深环和 4px 浅环，外圈一道 border-strong 发丝线；
/// 中间 58% 是封面圆。盒子不是正方形时按较小的一边画，圆心在盒子中央。
pub(super) struct VinylRecord;

impl Painter for VinylRecord {
    fn paint(&self, cx: &mut PaintContext<'_>) {
        let [width, height] = cx.size();
        let radius = width.min(height) / 2.0;
        if radius <= 0.0 {
            return;
        }
        let center = [width / 2.0, height / 2.0];
        fill_circle(cx, center, radius, PaintColor::Rgba(srgb(0x18)));
        // repeating-radial-gradient(circle, #151515 0 3px, #202020 4px 7px)：每 7px 一圈。
        let mut outer = radius;
        while outer > 0.0 {
            let ring_start = (outer / 7.0).floor() * 7.0;
            let dark_edge = ring_start + 3.0;
            if outer > dark_edge {
                fill_circle(cx, center, outer, PaintColor::Rgba(srgb(0x20)));
                outer = dark_edge;
                continue;
            }
            fill_circle(cx, center, outer, PaintColor::Rgba(srgb(0x15)));
            outer = ring_start - 0.001;
        }
        let mut edge = PaintPath::new();
        edge.ellipse(LayoutBox { x: center[0] - radius + 0.5, y: center[1] - radius + 0.5, width: radius * 2.0 - 1.0, height: radius * 2.0 - 1.0 });
        let line = cx.color(SemanticColorRole::BorderStrong);
        cx.stroke_path(&edge, nana_ui::runtime::StrokeStyle::new(1.0), PaintColor::Rgba([line[0], line[1], line[2], line[3] * 0.54]));
        record_art(cx, center, radius * 0.58);
    }

    fn paint_key(&self) -> u64 {
        2
    }
}

/// 封面圆：bg-hover 底，顶部一圈 accent 14% 光晕（淡到 56% 处），border-soft 细边。
fn record_art(cx: &mut PaintContext<'_>, center: [f32; 2], radius: f32) {
    fill_circle(cx, center, radius, PaintColor::Role(SemanticColorRole::Hover));
    let accent = cx.color(SemanticColorRole::Accent);
    let top = [center[0], center[1] - radius];
    let glow = Gradient::radial(top, radius * 2.0)
        .stop(0.0, PaintColor::Rgba([accent[0], accent[1], accent[2], 0.14]))
        .stop(0.56, PaintColor::Rgba([accent[0], accent[1], accent[2], 0.0]));
    let mut disc = PaintPath::new();
    disc.ellipse(LayoutBox { x: center[0] - radius, y: center[1] - radius, width: radius * 2.0, height: radius * 2.0 });
    cx.fill_path(&disc, glow);
    let mut edge = PaintPath::new();
    edge.ellipse(LayoutBox { x: center[0] - radius + 0.5, y: center[1] - radius + 0.5, width: radius * 2.0 - 1.0, height: radius * 2.0 - 1.0 });
    cx.stroke_path(&edge, nana_ui::runtime::StrokeStyle::new(1.0), SemanticColorRole::BorderSoft);
}

fn fill_circle(cx: &mut PaintContext<'_>, center: [f32; 2], radius: f32, color: PaintColor) {
    let mut path = PaintPath::new();
    path.ellipse(LayoutBox { x: center[0] - radius, y: center[1] - radius, width: radius * 2.0, height: radius * 2.0 });
    cx.fill_path(&path, color);
}

/// 灰阶 sRGB 字节换成 0..=1 的 RGBA。
fn srgb(level: u8) -> [f32; 4] {
    let value = f32::from(level) / 255.0;
    [value, value, value, 1.0]
}
