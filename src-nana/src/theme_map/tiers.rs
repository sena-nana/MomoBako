//! 主题档位：间距、行高、圆角、边框宽度、动效、不透明度、投影都从这里取。
//!
//! 每个写进样式的函数都写两份：默认主题下的数值进布局字段，主题令牌进 `layout.theme`。解析时主题令牌盖过
//! 数值，装上的主题换刻度时跟着变；只写数值的地方在自带令牌的控件上不生效。容器优先用 NanaUI 自己的
//! `gap_step` / `padding_step` / `radius` 构建方法，这里给直接改 `LayoutStyle` 的地方用。
//!
//! 间距刻度（默认主题）：Xxs 2、Xs 4、Sm 6、Md 8、Lg 10、Xl 12、Xxl 14、Xxxl 16、PageTight 20、Page 24。
//! 0 不是档位，照写 0、不挂令牌。

use std::time::Duration;

use nana_ui::theme::UI_METRICS;
use nana_ui_core::{
    BorderTokens, BorderWidth, Easing, EasingRole, LayoutStyle, LengthSpec, LineHeightSpec,
    LineRole, MotionRole, MotionTokens, OpacityTokens, RadiusTier, SpacingStep, SpacingTokens,
    StateLayer, ThemeExpr, ThemeToken, TypeRole, TypographyTokens,
};

/// 盒子的一条边。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Top,
    Right,
    Bottom,
    Left,
}

impl Edge {
    /// 四条边，按上、右、下、左。
    pub const ALL: [Edge; 4] = [Edge::Top, Edge::Right, Edge::Bottom, Edge::Left];
    /// 左右两条边。
    pub const X: [Edge; 2] = [Edge::Left, Edge::Right];
    /// 上下两条边。
    pub const Y: [Edge; 2] = [Edge::Top, Edge::Bottom];
}

/// 间距档在默认主题下的像素。
pub const fn space_px(step: SpacingStep) -> f32 {
    SpacingTokens::DEFAULT.resolve(step)
}

/// 间距档的主题表达式。
pub fn space_expr(step: SpacingStep) -> ThemeExpr {
    ThemeToken::Space(step).into()
}

/// 内边距：给出的每条边都取这一档。
pub fn padding(layout: &mut LayoutStyle, edges: &[Edge], step: SpacingStep) {
    let px = Some(LengthSpec::Px(space_px(step)));
    for edge in edges {
        match edge {
            Edge::Top => {
                layout.padding_top = px;
                layout.theme.padding_top = Some(space_expr(step));
            }
            Edge::Right => {
                layout.padding_right = px;
                layout.theme.padding_right = Some(space_expr(step));
            }
            Edge::Bottom => {
                layout.padding_bottom = px;
                layout.theme.padding_bottom = Some(space_expr(step));
            }
            Edge::Left => {
                layout.padding_left = px;
                layout.theme.padding_left = Some(space_expr(step));
            }
        }
    }
}

/// 内边距：左右一档、上下一档。
pub fn padding_xy(layout: &mut LayoutStyle, x: SpacingStep, y: SpacingStep) {
    padding(layout, &Edge::X, x);
    padding(layout, &Edge::Y, y);
}

/// 外边距：给出的每条边都取这一档。
pub fn margin(layout: &mut LayoutStyle, edges: &[Edge], step: SpacingStep) {
    let px = Some(LengthSpec::Px(space_px(step)));
    for edge in edges {
        match edge {
            Edge::Top => {
                layout.margin_top = px;
                layout.theme.margin_top = Some(space_expr(step));
            }
            Edge::Right => {
                layout.margin_right = px;
                layout.theme.margin_right = Some(space_expr(step));
            }
            Edge::Bottom => {
                layout.margin_bottom = px;
                layout.theme.margin_bottom = Some(space_expr(step));
            }
            Edge::Left => {
                layout.margin_left = px;
                layout.theme.margin_left = Some(space_expr(step));
            }
        }
    }
}

/// 子项间距，行列两个方向都取这一档。
pub fn gap(layout: &mut LayoutStyle, step: SpacingStep) {
    layout.gap = Some(LengthSpec::Px(space_px(step)));
    layout.theme.gap = Some(space_expr(step));
}

/// 换行以后行与行之间的间距。
pub fn row_gap(layout: &mut LayoutStyle, step: SpacingStep) {
    layout.row_gap = Some(LengthSpec::Px(space_px(step)));
    layout.theme.row_gap = Some(space_expr(step));
}

/// 同一行里列与列之间的间距。
pub fn column_gap(layout: &mut LayoutStyle, step: SpacingStep) {
    layout.column_gap = Some(LengthSpec::Px(space_px(step)));
    layout.theme.column_gap = Some(space_expr(step));
}

/// 行高取主题的行盒档：Line 16、Tall 18。
pub fn line(layout: &mut LayoutStyle, role: LineRole) {
    layout.line_height = Some(LineHeightSpec::Absolute(
        TypographyTokens::DEFAULT.line_height(role),
    ));
    layout.theme.line_height = Some(ThemeToken::Line(role).into());
}

/// 行高取字号档的倍数，例如正文 1.55 倍。主题换字阶时行高跟着字号一起变，比例不变。
pub fn line_ratio(layout: &mut LayoutStyle, role: TypeRole, ratio: f32) {
    layout.line_height = Some(LineHeightSpec::Absolute(
        TypographyTokens::DEFAULT.size(role) * ratio,
    ));
    layout.theme.line_height = Some(ThemeExpr {
        constant: 0.0,
        terms: vec![(ThemeToken::Type(role), ratio)],
    });
}

/// 圆角档在默认主题下的像素。
pub const fn radius_px(tier: RadiusTier) -> f32 {
    tier.resolve(UI_METRICS)
}

/// 圆角取主题的圆角档。
pub fn radius(layout: &mut LayoutStyle, tier: RadiusTier) {
    layout.border_radius = Some(radius_px(tier));
    layout.theme.border_radius = Some(ThemeToken::Radius(tier).into());
}

/// 发丝边在默认主题下的像素。
pub const fn hairline_px() -> f32 {
    BorderTokens::DEFAULT.resolve(BorderWidth::Hairline)
}

/// 边框宽度取发丝边的整数倍：1 是一根发丝边，2 是强调用的双倍边。
pub fn border(layout: &mut LayoutStyle, hairlines: u8) {
    let count = f32::from(hairlines);
    layout.border_width = Some(hairline_px() * count);
    layout.theme.border_width = Some(ThemeExpr {
        constant: 0.0,
        terms: vec![(ThemeToken::Border(BorderWidth::Hairline), count)],
    });
}

/// 动效时长档（默认主题）。
pub const fn motion(role: MotionRole) -> Duration {
    MotionTokens::DEFAULT.duration(role)
}

/// 动效时长档的毫秒数（默认主题），写进样式表的 `transition` 用。
pub const fn motion_ms(role: MotionRole) -> u16 {
    MotionTokens::DEFAULT.millis(role)
}

/// 缓动档（默认主题）。
pub const fn easing(role: EasingRole) -> Easing {
    MotionTokens::DEFAULT.easing(role)
}

/// 状态层的不透明度档（深色主题和浅色主题各有一套）。
pub const fn opacity(layer: StateLayer, dark: bool) -> f32 {
    if dark {
        OpacityTokens::DARK.resolve(layer)
    } else {
        OpacityTokens::LIGHT.resolve(layer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_helper_writes_the_default_value_and_the_theme_token() {
        let mut layout = LayoutStyle::default();
        padding_xy(&mut layout, SpacingStep::Xl, SpacingStep::Md);
        assert_eq!(layout.padding_left, Some(LengthSpec::Px(12.0)));
        assert_eq!(layout.padding_top, Some(LengthSpec::Px(8.0)));
        assert_eq!(
            layout.theme.padding_right,
            Some(ThemeToken::Space(SpacingStep::Xl).into())
        );
        gap(&mut layout, SpacingStep::Sm);
        assert_eq!(layout.gap, Some(LengthSpec::Px(6.0)));
        line_ratio(&mut layout, TypeRole::Body, 1.5);
        assert_eq!(layout.line_height, Some(LineHeightSpec::Absolute(19.5)));
        radius(&mut layout, RadiusTier::Full);
        assert!(layout.theme.border_radius.is_some());
        border(&mut layout, 2);
        assert_eq!(layout.border_width, Some(2.0));
        assert_eq!(motion_ms(MotionRole::HoverColor), 120);
    }
}
