//! 语义色的浅底：`--err-soft`、`--ok-soft` 这类带透明度的底色。
//!
//! 这类底色是语义色叠一层透明度。Nana 调色板没有对应的静止角色，这里用节点自绘：透明度取装上主题的
//! 状态层「提示底」档（`StateLayer::NoticeTint`，默认深浅都是 0.10），描边宽度取发丝边的整数倍，
//! 圆角写主题档位，都在画的时候按装上的主题解析，形状交给渲染层。

use nana_ui::runtime::{BoxPaint, PaintContext, Painter, Radius};
use nana_ui_core::{BorderWidth, RadiusTier, SemanticColorMix, SemanticColorRole, StateLayer};

/// 一块浅色语义底，可带同色系描边。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SoftFill {
    role: SemanticColorRole,
    layer: StateLayer,
    radius: RadiusTier,
    /// 描边：`(角色, 几根发丝边宽)`。步骤圆标的当前态、完成态和失败态用。
    border: Option<(SemanticColorRole, u8)>,
}

impl SoftFill {
    /// `--err-soft`。
    pub(crate) fn err() -> Self {
        Self {
            role: SemanticColorRole::Danger,
            layer: StateLayer::NoticeTint,
            radius: RadiusTier::Sm,
            border: None,
        }
    }

    /// `--ok-soft`。
    pub(crate) fn ok() -> Self {
        Self {
            role: SemanticColorRole::Success,
            layer: StateLayer::NoticeTint,
            radius: RadiusTier::Sm,
            border: None,
        }
    }

    pub(crate) fn radius(mut self, radius: RadiusTier) -> Self {
        self.radius = radius;
        self
    }

    /// 同色系描边，宽 `hairlines` 根发丝边。
    pub(crate) fn border(mut self, role: SemanticColorRole, hairlines: u8) -> Self {
        self.border = Some((role, hairlines));
        self
    }

    /// 胶囊形状（`RadiusTier::Full`，画的时候收到短边的一半）。
    pub(crate) fn pill(self) -> Self {
        self.radius(RadiusTier::Full)
    }

    fn key(&self) -> u64 {
        (self.role as u64) << 48
            ^ (self.layer as u64) << 16
            ^ (self.radius as u64) << 40
            ^ self
                .border
                .map(|(role, hairlines)| (role as u64) << 32 ^ u64::from(hairlines) << 8)
                .unwrap_or(0)
    }
}

impl Painter for SoftFill {
    fn paint(&self, cx: &mut PaintContext<'_>) {
        let theme = cx.theme();
        let alpha = theme.opacity().resolve(self.layer);
        let hairline = theme.border().resolve(BorderWidth::Hairline);
        let mut paint = BoxPaint::fill(SemanticColorMix::alpha(self.role, alpha));
        if let Some((role, hairlines)) = self.border {
            paint = paint.border(role, hairline * f32::from(hairlines));
        }
        let bounds = cx.bounds();
        cx.rounded_rect(bounds, Radius::Tier(self.radius), paint);
    }

    fn paint_key(&self) -> u64 {
        self.key()
    }
}
