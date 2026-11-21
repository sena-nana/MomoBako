//! 预览页自绘：插件预览框顶部的 accent-soft 光晕、音频舞台卡片的浅光和落影、唱片和中间的封面。
//!
//! 唱片纹路是写死的 #151515 / #202020 / #181818，和主题无关；其它颜色都取主题角色。
//! 正圆后面有落影。有封面图时按 `object-fit: cover` 画进中间 58% 的圆里；没有图时仍是封面圆。
//! 卡片上的浅光和 `0 18px 40px` 落影在 [`AudioCardFace`]，不改唱片那条 `0 22px 60px`。
//! 音视频准备卡的 `0 12px 32px` 落影在 [`LoadingCardShadow`]；准备卡和读取胶囊的进度轨是 NanaUI 的不定进度 `Progress`。

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use nana_ui::runtime::LengthSpec;
use nana_ui::runtime::view::{AnyView, IntoView, widget};

use nana_ui::runtime::{
    BoxPaint, Gradient, ImageFit, LayoutBox, PaintColor, PaintContext, PaintPath, PaintShadow,
    Painter, Progress, SemanticColorRole,
};
use nana_ui_core::SemanticColorMix;

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
        // 预览框有 1px 透明边，CSS 渐变按内边距盒定位：圆心在内边距盒顶边中点，到最远角为止；
        // 边框那 1px 落在重复图块的透明尾部，只剩 bg。
        let inner = LayoutBox {
            x: 1.0,
            y: 1.0,
            width: (width - 2.0).max(0.0),
            height: (height - 2.0).max(0.0),
        };
        let reach = ((inner.width / 2.0).powi(2) + inner.height.powi(2))
            .sqrt()
            .max(1.0);
        let glow = Gradient::radial([width / 2.0, 1.0], reach)
            .stop(0.0, SemanticColorRole::AccentSoft)
            .stop(0.38, PaintColor::Rgba([0.0, 0.0, 0.0, 0.0]));
        cx.rounded_rect(inner, (radius - 1.0).max(0.0), BoxPaint::fill(glow));
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
        let subtle = cx.color(SemanticColorRole::Subtle);
        let fade = Gradient::linear([0.0, height], [0.0, 0.0])
            .stop(
                0.0,
                PaintColor::Rgba([subtle[0], subtle[1], subtle[2], subtle[3] * 0.82]),
            )
            .stop(
                0.34,
                PaintColor::Rgba([subtle[0], subtle[1], subtle[2], 0.0]),
            );
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
/// 圆心和半径按 1px 边框内侧的内边距盒算，半径是到该盒最远角的像素距离（CSS 缺省的 `circle`）。
/// 落影在底色下面：`box-shadow: 0 18px 40px color-mix(in srgb, var(--bg) 84%, transparent)`，跟着圆角卡片走。
pub(super) struct AudioCardFace;

/// 卡片边框宽度，和舞台外壳的 `outline(..., 1.0)` 同一条。
const CARD_BORDER: f32 = 1.0;
/// 浅光圆心在内边距盒上的位置：宽的 28%、高的 32%。
const GLOW_AT_X: f32 = 0.28;
const GLOW_AT_Y: f32 = 0.32;
/// 浅光在半径的这个比例处淡到透明。
const GLOW_FADE: f32 = 0.28;
/// `color-mix(in srgb, var(--accent) 12%, transparent)` 里强调色的比例。
const GLOW_MIX: f32 = 0.12;
/// 卡片落影的垂直偏移，对应 `box-shadow: 0 18px 40px`。
const CARD_SHADOW_Y: f32 = 18.0;
/// 卡片落影的模糊半径。
const CARD_SHADOW_BLUR: f32 = 40.0;
/// `color-mix(in srgb, var(--bg) 84%, transparent)` 里背景色的比例。
const CARD_SHADOW_MIX: f32 = 0.84;

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
        cx.rounded_rect(
            rect,
            radius,
            BoxPaint::default().shadow(PaintShadow::Custom {
                color: PaintColor::Mix(SemanticColorMix::alpha(
                    SemanticColorRole::Background,
                    CARD_SHADOW_MIX,
                )),
                offset: [0.0, CARD_SHADOW_Y],
                blur: CARD_SHADOW_BLUR,
                spread: 0.0,
                inset: false,
            }),
        );
        // 底色和 border-soft 细边仍走外壳自己的外观，浅光盖在底色上、不盖住这 1px 边。
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
            .stop(GLOW_FADE, PaintColor::Rgba([0.0, 0.0, 0.0, 0.0]));
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

/// 唱片落影的垂直偏移，对应 `box-shadow: 0 22px 60px`。
const RECORD_SHADOW_Y: f32 = 22.0;
/// 唱片落影的模糊半径。
const RECORD_SHADOW_BLUR: f32 = 60.0;
/// `color-mix(in srgb, var(--bg) 78%, transparent)` 里背景色的比例。
const RECORD_SHADOW_ALPHA: f32 = 0.78;

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
        edge.ellipse(LayoutBox {
            x: center[0] - radius + 0.5,
            y: center[1] - radius + 0.5,
            width: radius * 2.0 - 1.0,
            height: radius * 2.0 - 1.0,
        });
        let line = cx.color(SemanticColorRole::BorderStrong);
        cx.stroke_path(
            &edge,
            nana_ui::runtime::StrokeStyle::new(1.0),
            PaintColor::Rgba([line[0], line[1], line[2], line[3] * 0.54]),
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
        let mut rim = PaintPath::new();
        rim.ellipse(LayoutBox {
            x: rect.x + 0.5,
            y: rect.y + 0.5,
            width: rect.width - 1.0,
            height: rect.height - 1.0,
        });
        cx.stroke_path(
            &rim,
            nana_ui::runtime::StrokeStyle::new(1.0),
            SemanticColorRole::BorderSoft,
        );
    }

    fn paint_key(&self) -> u64 {
        self.key
    }
}

/// 正圆后面的落影：
/// `box-shadow: 0 22px 60px color-mix(in srgb, var(--bg) 78%, transparent)`。
/// 跟画出的圆走，不跟外框：格子不是正方形时仍是圆，不铺成方块。
fn disc_shadow(cx: &mut PaintContext<'_>, center: [f32; 2], radius: f32) {
    let mut path = PaintPath::new();
    path.ellipse(LayoutBox {
        x: center[0] - radius,
        y: center[1] - radius,
        width: radius * 2.0,
        height: radius * 2.0,
    });
    cx.shadow(
        &path,
        PaintShadow::Custom {
            color: PaintColor::Mix(SemanticColorMix::alpha(
                SemanticColorRole::Background,
                RECORD_SHADOW_ALPHA,
            )),
            offset: [0.0, RECORD_SHADOW_Y],
            blur: RECORD_SHADOW_BLUR,
            spread: 0.0,
            inset: false,
        },
    );
}

/// 封面圆：bg-hover 底，顶部一圈 accent 14% 光晕（淡到 56% 处），border-soft 细边。
fn record_art(cx: &mut PaintContext<'_>, center: [f32; 2], radius: f32) {
    fill_circle(
        cx,
        center,
        radius,
        PaintColor::Role(SemanticColorRole::Hover),
    );
    let accent = cx.color(SemanticColorRole::Accent);
    let top = [center[0], center[1] - radius];
    let glow = Gradient::radial(top, radius * 2.0)
        .stop(
            0.0,
            PaintColor::Rgba([accent[0], accent[1], accent[2], 0.14]),
        )
        .stop(
            0.56,
            PaintColor::Rgba([accent[0], accent[1], accent[2], 0.0]),
        );
    let mut disc = PaintPath::new();
    disc.ellipse(LayoutBox {
        x: center[0] - radius,
        y: center[1] - radius,
        width: radius * 2.0,
        height: radius * 2.0,
    });
    cx.fill_path(&disc, glow);
    let mut edge = PaintPath::new();
    edge.ellipse(LayoutBox {
        x: center[0] - radius + 0.5,
        y: center[1] - radius + 0.5,
        width: radius * 2.0 - 1.0,
        height: radius * 2.0 - 1.0,
    });
    cx.stroke_path(
        &edge,
        nana_ui::runtime::StrokeStyle::new(1.0),
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

/// 准备卡落影的垂直偏移，`box-shadow: 0 12px 32px`。
const LOADING_SHADOW_Y: f32 = 12.0;
/// 准备卡落影的模糊半径。
const LOADING_SHADOW_BLUR: f32 = 32.0;
/// `color-mix(in srgb, var(--bg) 70%, transparent)` 里背景色的比例。
const LOADING_SHADOW_MIX: f32 = 0.7;
/// 音视频准备卡的落影。
///
/// `box-shadow: 0 12px 32px color-mix(in srgb, var(--bg) 70%, transparent)`，跟着卡片的 md 圆角。
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
        cx.rounded_rect(
            rect,
            radius,
            BoxPaint::default().shadow(PaintShadow::Custom {
                color: PaintColor::Mix(SemanticColorMix::alpha(
                    SemanticColorRole::Background,
                    LOADING_SHADOW_MIX,
                )),
                offset: [0.0, LOADING_SHADOW_Y],
                blur: LOADING_SHADOW_BLUR,
                spread: 0.0,
                inset: false,
            }),
        );
        cx.draw_default();
    }

    fn paint_key(&self) -> u64 {
        6
    }
}

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
        .thickness(6.0)
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

/// 灰阶 sRGB 字节换成 0..=1 的 RGBA。
fn srgb(level: u8) -> [f32; 4] {
    let value = f32::from(level) / 255.0;
    [value, value, value, 1.0]
}
