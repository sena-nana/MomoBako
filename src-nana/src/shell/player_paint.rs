//! 播放条自绘：8px 进度轨，以及音量、停留时长两条细滑杆。
//!
//! Vue 用原生 range 输入，靠 webkit 伪元素画轨道和圆钮。Nana 的 `RangeField`
//! 保留拖动、键盘和无障碍，外观由这里的 Painter 接管，颜色全部取主题角色。

use std::hash::{DefaultHasher, Hash, Hasher};

use nana_ui::runtime::{BoxPaint, LayoutBox, PaintContext, PaintPath, Painter, SemanticColorRole};
use nana_ui::Icon;
use nana_ui_core::SemanticColorMix;

/// 进度轨：bg-subtle 胶囊，accent 填充从左边按比例铺开，被胶囊裁出圆头。
pub(crate) struct ProgressTrack {
    pub ratio: f32,
}

impl Painter for ProgressTrack {
    fn paint(&self, cx: &mut PaintContext<'_>) {
        let [width, height] = cx.size();
        let rect = LayoutBox { x: 0.0, y: 0.0, width, height };
        let radius = height / 2.0;
        cx.rounded_rect(rect, radius, BoxPaint::fill(SemanticColorRole::Subtle));
        let filled = self.ratio.clamp(0.0, 1.0) * width;
        if filled <= 0.0 {
            return;
        }
        let mut clip = PaintPath::new();
        clip.rounded_rect(rect, [radius; 4]);
        cx.push_clip(&clip);
        cx.rounded_rect(LayoutBox { x: 0.0, y: 0.0, width: filled, height }, 0.0, BoxPaint::fill(SemanticColorRole::Accent));
        cx.pop_clip();
    }

    fn paint_key(&self) -> u64 {
        u64::from(self.ratio.to_bits())
    }
}

/// 细滑杆：4px 胶囊轨道是 accent 24% 混 border-soft；12px accent 圆钮带 2px bg-elev 外圈。
/// 圆钮中心从 6px 走到「宽 - 6px」，和 webkit 原生 range 的行程一致。`dim` 为真时按禁用淡化。
pub(super) struct SliderTrack {
    pub ratio: f32,
    pub dim: bool,
}

/// 轨道高度。
const TRACK: f32 = 4.0;
/// 圆钮直径。
const THUMB: f32 = 12.0;
/// 圆钮外圈宽度，对应 `box-shadow: 0 0 0 2px var(--bg-elev)`。
const RING: f32 = 2.0;

impl Painter for SliderTrack {
    fn paint(&self, cx: &mut PaintContext<'_>) {
        let [width, height] = cx.size();
        let track = LayoutBox { x: 0.0, y: (height - TRACK) / 2.0, width, height: TRACK };
        let track_color = cx.color(SemanticColorMix::new(SemanticColorRole::Accent, SemanticColorRole::BorderSoft, 0.24));
        let track_color = dim_on_surface(cx, track_color, self.dim);
        cx.rounded_rect(track, TRACK / 2.0, BoxPaint::fill(track_color));
        // 圆钮中心按比例落在两端各留半个圆钮的行程上。圆钮是正圆，不跟随平滑圆角。
        let center_x = THUMB / 2.0 + self.ratio.clamp(0.0, 1.0) * (width - THUMB).max(0.0);
        let center_y = height / 2.0;
        let ring_color = dim_on_surface(cx, cx.color(SemanticColorRole::Surface), self.dim);
        let thumb_color = dim_on_surface(cx, cx.color(SemanticColorRole::Accent), self.dim);
        fill_circle(cx, center_x, center_y, THUMB / 2.0 + RING, ring_color);
        fill_circle(cx, center_x, center_y, THUMB / 2.0, thumb_color);
    }

    fn paint_key(&self) -> u64 {
        u64::from(self.ratio.to_bits()) | (u64::from(self.dim) << 32)
    }
}

/// Vue 禁用控件整体 0.45 不透明，盖在 bg-elev 卡片上。这里在 sRGB 里和卡片底色直接混，
/// 和浏览器的合成结果一致；NanaUI 的线性合成会比 Vue 亮。
pub(super) const DIM_RATIO: f32 = 0.45;

fn dim_on_surface(cx: &PaintContext<'_>, color: [f32; 4], dim: bool) -> [f32; 4] {
    if !dim {
        return color;
    }
    let base = cx.color(SemanticColorRole::Surface);
    std::array::from_fn(|index| if index == 3 { color[3] } else { color[index] * DIM_RATIO + base[index] * (1.0 - DIM_RATIO) })
}

fn fill_circle(cx: &mut PaintContext<'_>, center_x: f32, center_y: f32, radius: f32, color: [f32; 4]) {
    let mut path = PaintPath::new();
    path.ellipse(circle(center_x, center_y, radius));
    cx.fill_path(&path, color);
}

/// 只有图标的按钮：悬停铺 bg-hover、按下铺 bg-active，图标按给定像素居中。
/// 对应 Vue `.ui-icon-button` 和全局 `button:hover`。
pub(super) struct GlyphButton {
    pub icon: Icon,
    pub size: f32,
    pub color: SemanticColorRole,
    /// 禁用时按 Vue 的 0.45 不透明度和卡片底色混。
    pub dim: bool,
}

impl Painter for GlyphButton {
    fn paint(&self, cx: &mut PaintContext<'_>) {
        state_fill(cx, BUTTON_RADIUS);
        let [width, height] = cx.size();
        let size = self.size.min(width).min(height);
        let rect = LayoutBox { x: (width - size) / 2.0, y: (height - size) / 2.0, width: size, height: size };
        let color = dim_on_surface(cx, cx.color(self.color), self.dim);
        cx.icon(rect, self.icon, color);
    }

    fn paint_key(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.icon.hash(&mut hasher);
        self.size.to_bits().hash(&mut hasher);
        (self.color as u32).hash(&mut hasher);
        self.dim.hash(&mut hasher);
        hasher.finish()
    }
}

/// 透明的可点区域：只在悬停、按下时铺底色。文字和封面由兄弟节点画在它上面。
pub(super) struct HoverSurface {
    pub radius: f32,
}

impl Painter for HoverSurface {
    fn paint(&self, cx: &mut PaintContext<'_>) {
        state_fill(cx, self.radius);
    }

    fn paint_key(&self) -> u64 {
        u64::from(self.radius.to_bits())
    }
}

/// Vue 全局按钮的圆角 `--radius-sm`。
const BUTTON_RADIUS: f32 = 6.0;

/// 按交互状态铺底：按下 bg-active，悬停 bg-hover，其余透明。
fn state_fill(cx: &mut PaintContext<'_>, radius: f32) {
    let state = cx.state();
    let role = if state.disabled {
        None
    } else if state.pressed {
        Some(SemanticColorRole::Active)
    } else if state.hovered {
        Some(SemanticColorRole::Hover)
    } else {
        None
    };
    if let Some(role) = role {
        let [width, height] = cx.size();
        cx.rounded_rect(LayoutBox { x: 0.0, y: 0.0, width, height }, radius, BoxPaint::fill(role));
    }
}

/// 什么也不画。进度条上盖的透明拖动层用它，Vue 的进度 range 轨道和圆钮都是透明的。
pub(super) struct Invisible;

impl Painter for Invisible {
    fn paint(&self, _cx: &mut PaintContext<'_>) {}

    fn paint_key(&self) -> u64 {
        0
    }
}

fn circle(center_x: f32, center_y: f32, radius: f32) -> LayoutBox {
    LayoutBox { x: center_x - radius, y: center_y - radius, width: radius * 2.0, height: radius * 2.0 }
}
