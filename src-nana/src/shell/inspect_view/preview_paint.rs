//! 预览页自绘：插件预览框顶部的 accent-soft 光晕、音频舞台卡片的浅光和落影、唱片和中间的封面。
//!
//! 唱片纹路是深浅主题都一样暗的三档灰（#151515 / #202020 / #181818），从主题的媒体遮罩色往媒体前景色
//! 按灰阶混出来；其它颜色都取主题角色和角色混色。落影都取主题的高度档：卡片和准备卡是
//! [`ElevationRole::Surface`]，唱片是更高一档的 [`ElevationRole::Overlay`]。
//! 有封面图时按 `object-fit: cover` 画进中间 58% 的圆里；没有图时仍是封面圆。
//! 卡片上的浅光和落影在 [`AudioCardFace`]，音视频准备卡的落影在 [`LoadingCardShadow`]；
//! 准备卡和读取胶囊的进度轨是 NanaUI 的不定进度 `Progress`。

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use nana_ui::runtime::LengthSpec;
use nana_ui::runtime::view::{AnyView, IntoView, widget};

use nana_ui::runtime::{
    BoxPaint, Gradient, ImageFit, LayoutBox, PaintColor, PaintContext, PaintPath, Painter,
    Progress, SemanticColorRole,
};
use nana_ui_core::{ElevationRole, SemanticColorMix};

use crate::theme_map::tiers;

/// 一根发丝边（默认主题）：预览框和卡片的边框、唱片上的描线都是这一根。
const HAIRLINE: f32 = tiers::hairline_px();

/// 角色的全透明版本：渐变淡出的终点，和起点同色相，只把不透明度收到 0。
const fn clear(role: SemanticColorRole) -> PaintColor {
    PaintColor::Mix(SemanticColorMix::alpha(role, 0.0))
}

/// 插件预览框：bg 底，顶部中间一圈 accent-soft 径向光晕，38% 处淡到透明。
/// 对应 `radial-gradient(circle at 50% 0%, var(--accent-soft), transparent 38%), var(--bg)`。
pub(super) struct PluginGlow;

impl Painter for PluginGlow {
    fn paint(&self, cx: &mut PaintContext<'_>) {
        let [width, height] = cx.size();
        let rect = LayoutBox {
            x: 0.0,
            y: 0.0,
            width,
            height,
        };
        let radius = cx.radius(nana_ui_core::RadiusTier::Xl);
        cx.rounded_rect(rect, radius, BoxPaint::fill(SemanticColorRole::Background));
        // 预览框有一根发丝边宽的透明边，CSS 渐变按内边距盒定位：圆心在内边距盒顶边中点，到最远角为止；
        // 边框那一圈落在重复图块的透明尾部，只剩 bg。
        let inner = LayoutBox {
            x: HAIRLINE,
            y: HAIRLINE,
            width: (width - HAIRLINE * 2.0).max(0.0),
            height: (height - HAIRLINE * 2.0).max(0.0),
        };
        let reach = ((inner.width / 2.0).powi(2) + inner.height.powi(2))
            .sqrt()
            .max(1.0);
        let glow = Gradient::radial([width / 2.0, HAIRLINE], reach)
            .stop(0.0, SemanticColorRole::AccentSoft)
            .stop(0.38, clear(SemanticColorRole::AccentSoft));
        cx.rounded_rect(inner, (radius - HAIRLINE).max(0.0), BoxPaint::fill(glow));
    }

    fn paint_key(&self) -> u64 {
        1
    }
}

/// PDF 页纸滚动区的底：bg 铺满，底边往上 34% 是 bg-subtle 82% 淡到透明。
/// 对应 `linear-gradient(0deg, color-mix(in srgb, var(--bg-subtle) 82%, transparent), transparent 34%), var(--bg)`。
pub(super) struct PageViewerBackdrop;

impl Painter for PageViewerBackdrop {
    fn paint(&self, cx: &mut PaintContext<'_>) {
        let [width, height] = cx.size();
        let rect = LayoutBox {
            x: 0.0,
            y: 0.0,
            width,
            height,
        };
        cx.rounded_rect(rect, 0.0, BoxPaint::fill(SemanticColorRole::Background));
        let fade = Gradient::linear([0.0, height], [0.0, 0.0])
            .stop(
                0.0,
                PaintColor::Mix(SemanticColorMix::alpha(SemanticColorRole::Subtle, 0.82)),
            )
            .stop(0.34, clear(SemanticColorRole::Subtle));
        cx.rounded_rect(rect, 0.0, BoxPaint::fill(fade));
    }

    fn paint_key(&self) -> u64 {
        4
    }
}

/// 音频舞台卡片的径向浅光和落影。
///
/// 浅光在卡片底色之上、唱片和歌词之下：
/// `radial-gradient(circle at 28% 32%, color-mix(in srgb, var(--accent) 12%, transparent), transparent 28%)`。
/// 圆心和半径按发丝边内侧的内边距盒算，半径是到该盒最远角的像素距离（CSS 缺省的 `circle`）。
/// 落影在底色下面，取主题的表面高度档，跟着圆角卡片走。
pub(super) struct AudioCardFace;

/// 卡片边框宽度，和舞台外壳的发丝边同一条。
const CARD_BORDER: f32 = HAIRLINE;
/// 浅光圆心在内边距盒上的位置：宽的 28%、高的 32%。
const GLOW_AT_X: f32 = 0.28;
const GLOW_AT_Y: f32 = 0.32;
/// 浅光在半径的这个比例处淡到透明。
const GLOW_FADE: f32 = 0.28;
/// `color-mix(in srgb, var(--accent) 12%, transparent)` 里强调色的比例。
const GLOW_MIX: f32 = 0.12;
/// 卡片和准备卡的落影：主题的表面高度档。
pub(super) const CARD_ELEVATION: ElevationRole = ElevationRole::Surface;
/// 唱片的落影：比卡片高一档。
pub(super) const RECORD_ELEVATION: ElevationRole = ElevationRole::Overlay;

impl Painter for AudioCardFace {
    fn paint(&self, cx: &mut PaintContext<'_>) {
        let [width, height] = cx.size();
        if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
            eprintln!("Nana 音频卡片没有可画的盒子：{width} x {height}");
            return;
        }
        let radius = cx.radius(nana_ui_core::RadiusTier::Xl);
        let rect = LayoutBox {
            x: 0.0,
            y: 0.0,
            width,
            height,
        };
        cx.rounded_rect(rect, radius, BoxPaint::default().shadow(CARD_ELEVATION));
        // 底色和 border-soft 细边仍走外壳自己的外观，浅光盖在底色上、不盖住这根发丝边。
        cx.draw_default();
        let inner_w = width - CARD_BORDER * 2.0;
        let inner_h = height - CARD_BORDER * 2.0;
        if inner_w <= 0.0 || inner_h <= 0.0 {
            eprintln!("Nana 音频卡片边框占满了盒子，浅光没有内边距盒：{width} x {height}");
            return;
        }
        let origin = [CARD_BORDER, CARD_BORDER];
        let center = [
            origin[0] + inner_w * GLOW_AT_X,
            origin[1] + inner_h * GLOW_AT_Y,
        ];
        let glow = Gradient::radial(center, farthest_corner(center, origin, [inner_w, inner_h]))
            .stop(
                0.0,
                PaintColor::Mix(SemanticColorMix::alpha(SemanticColorRole::Accent, GLOW_MIX)),
            )
            .stop(GLOW_FADE, clear(SemanticColorRole::Accent));
        let inner = LayoutBox {
            x: origin[0],
            y: origin[1],
            width: inner_w,
            height: inner_h,
        };
        cx.rounded_rect(inner, (radius - CARD_BORDER).max(0.0), BoxPaint::fill(glow));
    }

    fn paint_key(&self) -> u64 {
        5
    }
}

/// 圆心到矩形四个角里最远的像素距离。矩形左上角是 `origin`，宽高是 `size`。
/// CSS `radial-gradient(circle)` 没写大小时，结束形状就是这个圆。
fn farthest_corner(center: [f32; 2], origin: [f32; 2], size: [f32; 2]) -> f32 {
    let right = origin[0] + size[0];
    let bottom = origin[1] + size[1];
    let mut reach = 1.0_f32;
    for corner in [
        origin,
        [right, origin[1]],
        [origin[0], bottom],
        [right, bottom],
    ] {
        let dist = (corner[0] - center[0]).hypot(corner[1] - center[1]);
        if dist > reach {
            reach = dist;
        }
    }
    reach
}

/// 唱片的底色和两档纹路的灰阶（0–255）：#181818 底、#151515 深环、#202020 浅环。
const VINYL_BASE: u8 = 0x18;
const VINYL_DARK: u8 = 0x15;
const VINYL_LIGHT: u8 = 0x20;
/// 唱片外圈描线：`--border-strong` 54% 不透明。
const VINYL_EDGE: SemanticColorMix = SemanticColorMix::alpha(SemanticColorRole::BorderStrong, 0.54);

/// 唱片上的一档灰：主题的媒体遮罩色往媒体前景色按灰阶混。深浅主题的媒体色都是黑底白字，唱片两边一样暗。
fn vinyl_tone(cx: &PaintContext<'_>, level: u8) -> PaintColor {
    let scrim = cx.theme().effects().media_scrim.as_rgba_array();
    let light = cx.color(SemanticColorRole::OnMedia);
    PaintColor::Rgba(super::super::files_view::style::mix_rgb(
        scrim,
        light,
        f32::from(level) / 255.0,
    ))
}

/// 唱片：#181818 底上交替的 3px 深环和 4px 浅环，外圈一道 border-strong 发丝线；
/// 中间 58% 是封面。`art` 是已经解码的封面 data URL，空字符串表示没有图，画封面圆。
/// 盒子不是正方形时按较小的一边画，圆心在盒子中央。落影跟这个圆走。
pub(super) struct VinylRecord {
    art: Arc<str>,
    key: u64,
}

impl VinylRecord {
    /// `art` 有值时把这张图铺进封面圆；没有时只画封面圆。
    pub(super) fn with_art(art: Option<&str>) -> Self {
        let art: Arc<str> = art.unwrap_or("").into();
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        art.hash(&mut hasher);
        Self {
            art,
            key: hasher.finish(),
        }
    }
}

impl Painter for VinylRecord {
    fn paint(&self, cx: &mut PaintContext<'_>) {
        let [width, height] = cx.size();
        if !width.is_finite() || !height.is_finite() {
            eprintln!("Nana 唱片没有可画的盒子：{width} x {height}");
            return;
        }
        let radius = width.min(height) / 2.0;
        if radius <= 0.0 {
            return;
        }
        let center = [width / 2.0, height / 2.0];
        disc_shadow(cx, center, radius);
        let (base, dark, light) = (
            vinyl_tone(cx, VINYL_BASE),
            vinyl_tone(cx, VINYL_DARK),
            vinyl_tone(cx, VINYL_LIGHT),
        );
        fill_circle(cx, center, radius, base);
        // repeating-radial-gradient(circle, #151515 0 3px, #202020 4px 7px)：每 7px 一圈。
        let mut outer = radius;
        while outer > 0.0 {
            let ring_start = (outer / 7.0).floor() * 7.0;
            let dark_edge = ring_start + 3.0;
            if outer > dark_edge {
                fill_circle(cx, center, outer, light);
                outer = dark_edge;
                continue;
            }
            fill_circle(cx, center, outer, dark);
            outer = ring_start - 0.001;
        }
        cx.stroke_path(
            &inner_ring(center, radius),
            nana_ui::runtime::StrokeStyle::new(HAIRLINE),
            VINYL_EDGE,
        );
        let art_radius = radius * 0.58;
        record_art(cx, center, art_radius);
        if self.art.is_empty() {
            return;
        }
        // 图盖住封面圆的底色，圆角收到直径的一半，和 CSS `border-radius: pill` 一样裁成圆。
        let rect = LayoutBox {
            x: center[0] - art_radius,
            y: center[1] - art_radius,
            width: art_radius * 2.0,
            height: art_radius * 2.0,
        };
        cx.image(rect, Arc::clone(&self.art), ImageFit::Cover, art_radius);
        cx.stroke_path(
            &inner_ring(center, art_radius),
            nana_ui::runtime::StrokeStyle::new(HAIRLINE),
            SemanticColorRole::BorderSoft,
        );
    }

    fn paint_key(&self) -> u64 {
        self.key
    }
}

/// 圆内侧的一圈发丝线：描边中线缩进半根发丝边，整根线落在圆里。
fn inner_ring(center: [f32; 2], radius: f32) -> PaintPath {
    let mut ring = PaintPath::new();
    ring.ellipse(LayoutBox {
        x: center[0] - radius + HAIRLINE / 2.0,
        y: center[1] - radius + HAIRLINE / 2.0,
        width: radius * 2.0 - HAIRLINE,
        height: radius * 2.0 - HAIRLINE,
    });
    ring
}

/// 正圆后面的落影：主题的覆盖层高度档。
/// 跟画出的圆走，不跟外框：格子不是正方形时仍是圆，不铺成方块。
fn disc_shadow(cx: &mut PaintContext<'_>, center: [f32; 2], radius: f32) {
    let mut path = PaintPath::new();
    path.ellipse(LayoutBox {
        x: center[0] - radius,
        y: center[1] - radius,
        width: radius * 2.0,
        height: radius * 2.0,
    });
    cx.shadow(&path, RECORD_ELEVATION);
}

/// 封面圆：bg-hover 底，顶部一圈 accent 14% 光晕（淡到 56% 处），border-soft 细边。
fn record_art(cx: &mut PaintContext<'_>, center: [f32; 2], radius: f32) {
    fill_circle(
        cx,
        center,
        radius,
        PaintColor::Role(SemanticColorRole::Hover),
    );
    let top = [center[0], center[1] - radius];
    let glow = Gradient::radial(top, radius * 2.0)
        .stop(
            0.0,
            PaintColor::Mix(SemanticColorMix::alpha(SemanticColorRole::Accent, 0.14)),
        )
        .stop(0.56, clear(SemanticColorRole::Accent));
    let mut disc = PaintPath::new();
    disc.ellipse(LayoutBox {
        x: center[0] - radius,
        y: center[1] - radius,
        width: radius * 2.0,
        height: radius * 2.0,
    });
    cx.fill_path(&disc, glow);
    cx.stroke_path(
        &inner_ring(center, radius),
        nana_ui::runtime::StrokeStyle::new(HAIRLINE),
        SemanticColorRole::BorderSoft,
    );
}

fn fill_circle(cx: &mut PaintContext<'_>, center: [f32; 2], radius: f32, color: PaintColor) {
    let mut path = PaintPath::new();
    path.ellipse(LayoutBox {
        x: center[0] - radius,
        y: center[1] - radius,
        width: radius * 2.0,
        height: radius * 2.0,
    });
    cx.fill_path(&path, color);
}

/// 音视频准备卡的落影：主题的表面高度档，跟着卡片的 md 圆角。
/// 底色和 border-soft 细边仍走外壳自己的外观。
pub(super) struct LoadingCardShadow;

impl Painter for LoadingCardShadow {
    fn paint(&self, cx: &mut PaintContext<'_>) {
        let [width, height] = cx.size();
        if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
            eprintln!("Nana 准备卡没有可画的盒子：{width} x {height}");
            return;
        }
        let radius = cx.radius(nana_ui_core::RadiusTier::Md);
        let rect = LayoutBox {
            x: 0.0,
            y: 0.0,
            width,
            height,
        };
        cx.rounded_rect(rect, radius, BoxPaint::default().shadow(CARD_ELEVATION));
        cx.draw_default();
    }

    fn paint_key(&self) -> u64 {
        6
    }
}

/// 视频画面的底：主题的媒体遮罩色，深浅主题都是近黑。画面本身（节点的内容图或纹理子节点）盖在上面。
pub(super) struct MediaBackdrop;

impl Painter for MediaBackdrop {
    fn paint(&self, cx: &mut PaintContext<'_>) {
        let [width, height] = cx.size();
        let rect = LayoutBox {
            x: 0.0,
            y: 0.0,
            width,
            height,
        };
        let scrim = cx.theme().effects().media_scrim.as_rgba_array();
        cx.rounded_rect(rect, 0.0, BoxPaint::fill(PaintColor::Rgba(scrim)));
        cx.draw_default();
    }

    fn paint_key(&self) -> u64 {
        7
    }
}

/// 节点后面一层主题高度档的落影，跟着节点的圆角档（没有圆角时是直角）。
/// 底色、边和内容仍走节点自己的外观。PDF 页纸用它。
pub(super) struct Elevated {
    role: ElevationRole,
    radius: Option<nana_ui_core::RadiusTier>,
}

impl Elevated {
    /// 直角节点的落影。
    pub(super) const fn square(role: ElevationRole) -> Self {
        Self { role, radius: None }
    }
}

impl Painter for Elevated {
    fn paint(&self, cx: &mut PaintContext<'_>) {
        let [width, height] = cx.size();
        if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
            eprintln!("Nana 落影没有可画的盒子：{width} x {height}");
            return;
        }
        let radius = self.radius.map_or(0.0, |tier| cx.radius(tier));
        let rect = LayoutBox {
            x: 0.0,
            y: 0.0,
            width,
            height,
        };
        cx.rounded_rect(rect, radius, BoxPaint::default().shadow(self.role));
        cx.draw_default();
    }

    fn paint_key(&self) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.role.hash(&mut hasher);
        self.radius.hash(&mut hasher);
        hasher.finish()
    }
}

/// 进度轨粗 6：NanaUI 的进度条没有粗细档，照写像素。
const TRACK_THICKNESS: f32 = 6.0;

/// 准备卡上的 6px 进度轨：NanaUI 的不定进度 `Progress`，铺满准备卡宽。
/// 扫动是组件自己的循环时间线，减弱动态效果时换成整条淡填充。
pub(super) fn loading_progress() -> AnyView {
    widget(sweep_track(LengthSpec::Fill))
        .key("inspect-media-loading-progress")
        .into_any()
}

/// 图片读取胶囊里的进度轨：同一种不定进度，宽 132，在状态胶囊里固定宽度。
pub(super) fn status_progress() -> AnyView {
    widget(sweep_track(LengthSpec::Px(132.0)))
        .key("inspect-status-progress")
        .into_any()
}

/// 不知道还要多久的 6px 进度轨，轨道是 `--border-soft`，填充走主题的指示色。
fn sweep_track(width: LengthSpec) -> Progress {
    let mut track = Progress::new(0.0, 1.0)
        .indeterminate(true)
        .thickness(TRACK_THICKNESS)
        .track(SemanticColorRole::BorderSoft);
    let layout = Arc::make_mut(&mut track.style.layout);
    layout.width = Some(width);
    layout.flex_grow = Some(0.0);
    layout.flex_shrink = Some(0.0);
    track
}

#[cfg(test)]
mod record_shadow_tests;

#[cfg(test)]
mod card_face_tests;

#[cfg(test)]
mod loading_card_tests;

#[cfg(test)]
mod status_pill_tests;
