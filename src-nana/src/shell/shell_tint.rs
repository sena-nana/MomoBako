//! 语义色的浅底：Vue 的 `--err-soft`、`--ok-soft` 这类带透明度的底色。
//!
//! 这类底色是语义色叠一层透明度，浅色和深色主题的透明度不同（`--err-soft` 浅色 0.10、
//! 深色 0.14）。Nana 调色板没有对应的静止角色，这里用节点自绘按当前主题模式取透明度，
//! 圆角写主题档位，形状交给渲染层。

use nana_ui::runtime::{BoxPaint, PaintContext, Painter, Radius};
use nana_ui_core::{RadiusTier, SemanticColorMix, SemanticColorRole, ThemeMode};

/// 一块浅色语义底，可带同色系描边。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SoftFill {
    role: SemanticColorRole,
    light_alpha: f32,
    dark_alpha: f32,
    radius: RadiusTier,
    /// 描边：`(角色, 宽度)`。步骤圆标的当前态、完成态和失败态用。
    border: Option<(SemanticColorRole, f32)>,
}

impl SoftFill {
    /// Vue `--err-soft`。
    pub(crate) fn err() -> Self {
        Self { role: SemanticColorRole::Danger, light_alpha: 0.10, dark_alpha: 0.14, radius: RadiusTier::Sm, border: None }
    }

    /// Vue `--ok-soft`。
    pub(crate) fn ok() -> Self {
        Self { role: SemanticColorRole::Success, light_alpha: 0.10, dark_alpha: 0.14, radius: RadiusTier::Sm, border: None }
    }

    pub(crate) fn radius(mut self, radius: RadiusTier) -> Self {
        self.radius = radius;
        self
    }

    pub(crate) fn border(mut self, role: SemanticColorRole, width: f32) -> Self {
        self.border = Some((role, width));
        self
    }

    /// 胶囊形状（`--radius-pill`）。
    pub(crate) fn pill(self) -> PillFill {
        PillFill(self)
    }

    fn alpha(&self, mode: ThemeMode) -> f32 {
        if mode == ThemeMode::Dark { self.dark_alpha } else { self.light_alpha }
    }

    fn paint_with(&self, cx: &mut PaintContext<'_>, radius: Radius) {
        let alpha = self.alpha(cx.theme_mode());
        let mut paint = BoxPaint::fill(SemanticColorMix::alpha(self.role, alpha));
        if let Some((role, width)) = self.border {
            paint = paint.border(role, width);
        }
        let bounds = cx.bounds();
        cx.rounded_rect(bounds, radius, paint);
    }

    fn key(&self) -> u64 {
        (self.role as u64) << 48
            ^ u64::from(self.light_alpha.to_bits()) << 16
            ^ u64::from(self.dark_alpha.to_bits())
            ^ (self.radius as u64) << 40
            ^ self.border.map(|(role, width)| (role as u64) << 32 ^ u64::from(width.to_bits()) << 8).unwrap_or(0)
    }
}

impl Painter for SoftFill {
    fn paint(&self, cx: &mut PaintContext<'_>) {
        self.paint_with(cx, Radius::Tier(self.radius));
    }

    fn paint_key(&self) -> u64 {
        self.key()
    }
}

/// 胶囊形的浅色语义底，圆角取高度的一半。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PillFill(SoftFill);

impl Painter for PillFill {
    fn paint(&self, cx: &mut PaintContext<'_>) {
        let [width, height] = cx.size();
        self.0.paint_with(cx, Radius::Px(width.min(height) / 2.0));
    }

    fn paint_key(&self) -> u64 {
        self.0.key() ^ 0x5049_4c4c
    }
}
