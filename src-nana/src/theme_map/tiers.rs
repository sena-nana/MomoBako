//! 主题档位：间距、行高、圆角、边框宽度、控件尺寸、动效、不透明度、投影都从这里取。
//!
//! 控件尺寸按 MomoBako 的控件度量（[`CONTROL_METRICS`]）折像素：小、中、大号控件 28、32、36，
//! 图标按钮 22。
//!
//! 每个写进样式的函数都写两份：默认主题下的数值进布局字段，主题表达式进 `layout.theme`。解析时主题表达式盖过
//! 数值，装上的主题换刻度时跟着变；只写数值的地方在自带令牌的控件上不生效。容器优先用 NanaUI 自己的
//! `gap_step` / `gap_theme` / `padding_step` / `radius` 构建方法，这里给直接改 `LayoutStyle` 的地方和
//! 按边写的地方用。
//!
//! 间距刻度（默认主题）：Xxs 2、Xs 4、Sm 6、Md 8、Lg 10、Xl 12、Xxl 14、Xxxl 16、PageTight 20、Page 24。
//! 0 不是档位，照写 0、不挂令牌。

use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::Duration;

use nana_ui::runtime::{
    BoxPaint, LayoutBox, NodeStyle, PaintContext, Painter, SemanticColorRole, Stack, Text,
};
use nana_ui::theme::UI_METRICS;
use nana_ui_core::{
    BorderTokens, BorderWidth, ControlPadding, ControlSize, Easing, EasingRole, ElevationRole,
    LayoutStyle, LengthSpec, LineHeightSpec, LineRole, MotionRole, MotionTokens, OpacityTokens,
    RadiusTier, SpacingStep, SpacingTokens, SquareSize, StateLayer, ThemeAppearance, ThemeExpr,
    ThemeToken, TypeRole, TypographyTokens, builtin_theme,
};

use super::CONTROL_METRICS;

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

/// 主题表达式在默认主题下的像素值。
pub fn default_px(expr: &ThemeExpr) -> f32 {
    expr.resolve(builtin_theme(ThemeAppearance::Dark))
}

// ---- 间距 ----

/// 间距档在默认主题下的像素。
pub const fn space_px(step: SpacingStep) -> f32 {
    SpacingTokens::DEFAULT.resolve(step)
}

/// 间距档的主题表达式。
pub fn space_expr(step: SpacingStep) -> ThemeExpr {
    ThemeToken::Space(step).into()
}

/// 盒子四边上的一类长度。
#[derive(Clone, Copy)]
enum Side {
    Padding,
    Margin,
    Offset,
}

/// 给一条边写长度：默认主题下的像素进布局字段，表达式进 `layout.theme`。
fn write_side(layout: &mut LayoutStyle, side: Side, edge: Edge, expr: &ThemeExpr) {
    let px = Some(LengthSpec::Px(default_px(expr)));
    let theme = &mut layout.theme;
    let (value, token) = match (side, edge) {
        (Side::Padding, Edge::Top) => (&mut layout.padding_top, &mut theme.padding_top),
        (Side::Padding, Edge::Right) => (&mut layout.padding_right, &mut theme.padding_right),
        (Side::Padding, Edge::Bottom) => (&mut layout.padding_bottom, &mut theme.padding_bottom),
        (Side::Padding, Edge::Left) => (&mut layout.padding_left, &mut theme.padding_left),
        (Side::Margin, Edge::Top) => (&mut layout.margin_top, &mut theme.margin_top),
        (Side::Margin, Edge::Right) => (&mut layout.margin_right, &mut theme.margin_right),
        (Side::Margin, Edge::Bottom) => (&mut layout.margin_bottom, &mut theme.margin_bottom),
        (Side::Margin, Edge::Left) => (&mut layout.margin_left, &mut theme.margin_left),
        (Side::Offset, Edge::Top) => (&mut layout.offset_top, &mut theme.offset_top),
        (Side::Offset, Edge::Right) => (&mut layout.offset_right, &mut theme.offset_right),
        (Side::Offset, Edge::Bottom) => (&mut layout.offset_bottom, &mut theme.offset_bottom),
        (Side::Offset, Edge::Left) => (&mut layout.offset_left, &mut theme.offset_left),
    };
    *value = px;
    *token = Some(expr.clone());
}

/// 内边距：给出的每条边都取这一档。
pub fn padding(layout: &mut LayoutStyle, edges: &[Edge], step: SpacingStep) {
    padding_by(layout, edges, space_expr(step));
}

/// 内边距：给出的每条边都取这个主题表达式（例如发丝边宽 [`hairline_expr`]）。
pub fn padding_by(layout: &mut LayoutStyle, edges: &[Edge], expr: ThemeExpr) {
    for edge in edges {
        write_side(layout, Side::Padding, *edge, &expr);
    }
}

/// 内边距：左右一档、上下一档。
pub fn padding_xy(layout: &mut LayoutStyle, x: SpacingStep, y: SpacingStep) {
    padding(layout, &Edge::X, x);
    padding(layout, &Edge::Y, y);
}

/// 外边距：给出的每条边都取这一档。
pub fn margin(layout: &mut LayoutStyle, edges: &[Edge], step: SpacingStep) {
    margin_by(layout, edges, space_expr(step));
}

/// 外边距：给出的每条边都取这个主题表达式。往回让一根发丝边写 `hairline_expr(-1.0)`。
pub fn margin_by(layout: &mut LayoutStyle, edges: &[Edge], expr: ThemeExpr) {
    for edge in edges {
        write_side(layout, Side::Margin, *edge, &expr);
    }
}

/// 绝对定位时给出的每条边的偏移都取这一档：盖在父级上、四周内缩一档的浮层，或浮在父级角上的小件用。
pub fn offset(layout: &mut LayoutStyle, edges: &[Edge], step: SpacingStep) {
    let expr = space_expr(step);
    for edge in edges {
        write_side(layout, Side::Offset, *edge, &expr);
    }
}

/// 子项间距，行列两个方向都取这一档。
pub fn gap(layout: &mut LayoutStyle, step: SpacingStep) {
    gap_by(layout, space_expr(step));
}

/// 子项间距取主题表达式，例如「一档间距再加随窗口宽变的一段」。
pub fn gap_by(layout: &mut LayoutStyle, expr: ThemeExpr) {
    layout.gap = Some(LengthSpec::Px(default_px(&expr)));
    layout.theme.gap = Some(expr);
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

/// 给出的几条边取同一档内边距，其余边不动。
pub fn pad(stack: Stack, edges: &[Edge], step: SpacingStep) -> Stack {
    stack.with_layout(|layout| padding(layout, edges, step))
}

/// 左右一档、上下一档的内边距。
pub fn pad_xy(stack: Stack, x: SpacingStep, y: SpacingStep) -> Stack {
    stack.with_layout(|layout| padding_xy(layout, x, y))
}

/// 四边同一档的内边距。
pub fn inset(stack: Stack, step: SpacingStep) -> Stack {
    pad(stack, &Edge::ALL, step)
}

// ---- 行高 ----

/// 主题字阶上的字号档，按字号从小到大。
const TYPE_ROLES: [TypeRole; 8] = [
    TypeRole::Hint,
    TypeRole::Meta,
    TypeRole::Body,
    TypeRole::Section,
    TypeRole::CardTitle,
    TypeRole::Heading,
    TypeRole::Title,
    TypeRole::Display,
];

/// 默认主题下正好是 `size` 的字号档；落在台阶之间时没有。
pub fn type_role_at(size: f32) -> Option<TypeRole> {
    TYPE_ROLES
        .into_iter()
        .find(|role| TypographyTokens::DEFAULT.size(*role) == size)
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

/// 字号 `size` 的字写行高 `line_px`，挂主题档位，默认主题下的数值不变：等于行盒档（Line 16、Tall 18）时
/// 取行盒档；字号在主题字阶上时取字号档的倍数，主题换字阶时行高跟着字号按比例变；字号不在字阶上
/// （随窗口宽连续变的标题一类）时写成字号的倍数，跟着这段字自己的字号走。
pub fn line_px(layout: &mut LayoutStyle, size: f32, line_px: f32) {
    if let Some(role) = [LineRole::Line, LineRole::Tall]
        .into_iter()
        .find(|role| TypographyTokens::DEFAULT.line_height(*role) == line_px)
    {
        line(layout, role);
        return;
    }
    match type_role_at(size) {
        Some(role) => line_ratio(layout, role, line_px / size),
        None if size > 0.0 => {
            layout.line_height = Some(LineHeightSpec::Relative(line_px / size));
            layout.theme.line_height = None;
        }
        None => {
            eprintln!("Nana 行高没有可用的字号：{size}，行高按 {line_px} 像素写");
            layout.line_height = Some(LineHeightSpec::Absolute(line_px));
            layout.theme.line_height = None;
        }
    }
}

/// 给一段字写行高，见 [`line_px`]。字号按节点上写的（没写时按正文档）。
pub fn text_line_px(mut node: Text, line_height: f32) -> Text {
    let layout = Arc::make_mut(&mut node.style.layout);
    let size = layout
        .font_size
        .unwrap_or(TypographyTokens::DEFAULT.size(TypeRole::Body));
    line_px(layout, size, line_height);
    node
}

/// 一段字的行高取字号档的倍数，见 [`line_ratio`]。
pub fn lined(mut node: Text, role: TypeRole, ratio: f32) -> Text {
    line_ratio(Arc::make_mut(&mut node.style.layout), role, ratio);
    node
}

// ---- 圆角 ----

/// 圆角档在默认主题下的像素。
pub const fn radius_px(tier: RadiusTier) -> f32 {
    tier.resolve(UI_METRICS)
}

/// 圆角取主题的圆角档。
pub fn radius(layout: &mut LayoutStyle, tier: RadiusTier) {
    layout.border_radius = Some(radius_px(tier));
    layout.theme.border_radius = Some(ThemeToken::Radius(tier).into());
}

// ---- 边框 ----

/// 发丝边在默认主题下的像素。
pub const fn hairline_px() -> f32 {
    BorderTokens::DEFAULT.resolve(BorderWidth::Hairline)
}

/// `count` 根发丝边宽的主题表达式：边框宽度、给边框预留的内边距、往回让一根发丝边的负外边距用。
pub fn hairline_expr(count: f32) -> ThemeExpr {
    ThemeExpr {
        constant: 0.0,
        terms: vec![(ThemeToken::Border(BorderWidth::Hairline), count)],
    }
}

/// 边框宽度取发丝边的整数倍：1 是一根发丝边，2 是强调用的双倍边。
pub fn border(layout: &mut LayoutStyle, hairlines: u8) {
    let count = f32::from(hairlines);
    layout.border_width = Some(hairline_px() * count);
    layout.theme.border_width = Some(hairline_expr(count));
}

/// 只给盒子的一条边写边框宽度，取发丝边的整数倍（[`border`] 是四条边一起）。
pub fn border_side(layout: &mut LayoutStyle, edge: Edge, hairlines: u8) {
    let count = f32::from(hairlines);
    let px = Some(hairline_px() * count);
    let expr = Some(hairline_expr(count));
    let sides = &mut layout.border_sides;
    let theme = &mut layout.theme;
    match edge {
        Edge::Top => (sides.top_width, theme.border_top_width) = (px, expr),
        Edge::Right => (sides.right_width, theme.border_right_width) = (px, expr),
        Edge::Bottom => (sides.bottom_width, theme.border_bottom_width) = (px, expr),
        Edge::Left => (sides.left_width, theme.border_left_width) = (px, expr),
    }
}

/// 一圈发丝边：边色取 `role`，线宽是主题的发丝边，主题换发丝边时跟着变。
pub trait Hairline {
    fn hairline(self, role: SemanticColorRole) -> Self;
}

impl Hairline for Stack {
    fn hairline(self, role: SemanticColorRole) -> Self {
        self.border(role, hairline_px())
            .with_layout(|layout| border(layout, 1))
    }
}

// ---- 控件尺寸 ----

/// 控件高度取主题的控件档：`exact` 定高，否则是最小高、内容可以撑高。
pub fn control_height(layout: &mut LayoutStyle, size: ControlSize, exact: bool) {
    let px = Some(LengthSpec::Px(control_px(size)));
    let expr = Some(ThemeToken::Control(size).into());
    layout.min_height = px;
    layout.theme.min_height = expr.clone();
    if exact {
        layout.height = px;
        layout.theme.height = expr;
    }
}

/// 控件宽度取主题的控件档：宽和最小宽都是这一档的控件高（例如和中号控件一样宽的关闭钮）。
pub fn control_width(layout: &mut LayoutStyle, size: ControlSize) {
    let px = Some(LengthSpec::Px(control_px(size)));
    let expr: Option<ThemeExpr> = Some(ThemeToken::Control(size).into());
    layout.width = px;
    layout.min_width = px;
    layout.theme.width = expr.clone();
    layout.theme.min_width = expr;
}

/// 控件档在 MomoBako 度量下的像素高。
pub const fn control_px(size: ControlSize) -> f32 {
    size.height_in(CONTROL_METRICS)
}

/// 行或控件的高度：主题的控件档；套一圈发丝边框的控件档（框里的控件是这一档，框比它高两根发丝边，
/// 工具条里的描边框和跟它们站在一起的按钮一类）；或者档里没有的像素高（照写）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Height {
    Control(ControlSize),
    Framed(ControlSize),
    Px(f32),
}

impl Height {
    /// MomoBako 度量下的像素高。
    pub const fn px(self) -> f32 {
        match self {
            Self::Control(size) => control_px(size),
            Self::Framed(size) => control_px(size) + hairline_px() * 2.0,
            Self::Px(px) => px,
        }
    }

    /// 主题表达式；照写的像素高没有。
    pub fn expr(self) -> Option<ThemeExpr> {
        match self {
            Self::Control(size) => Some(ThemeToken::Control(size).into()),
            Self::Framed(size) => {
                let mut expr = hairline_expr(2.0);
                expr.terms.push((ThemeToken::Control(size), 1.0));
                Some(expr)
            }
            Self::Px(_) => None,
        }
    }
}

/// 定高：高和最小高同值，档位同时写数值和主题表达式，像素高照写。
pub fn height(layout: &mut LayoutStyle, height: Height) {
    min_height(layout, height);
    layout.height = Some(LengthSpec::Px(height.px()));
    layout.theme.height = height.expr();
}

/// 最小高：内容可以撑高。档位同时写数值和主题表达式，像素高照写。
pub fn min_height(layout: &mut LayoutStyle, height: Height) {
    layout.min_height = Some(LengthSpec::Px(height.px()));
    layout.theme.min_height = height.expr();
}

/// 方形控件的边长：主题的方块档，或者档里没有的像素边长（照写）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Square {
    Tier(SquareSize),
    Px(f32),
}

impl Square {
    /// MomoBako 度量下的像素边长。
    pub const fn px(self) -> f32 {
        match self {
            Self::Tier(size) => size.resolve(CONTROL_METRICS),
            Self::Px(px) => px,
        }
    }
}

/// 给节点定方形边长。档位交给 NanaUI 的 `square`：按装上的度量写最小宽高，宽高不写（写了的轴不跟方块档），
/// 内容比方块小时盒子就是方块；像素边长照写宽高和最小宽高。
pub fn square(style: &mut NodeStyle, square: Square) {
    let layout = Arc::make_mut(&mut style.layout);
    match square {
        Square::Tier(size) => {
            style.square = Some(size);
            layout.width = None;
            layout.height = None;
            layout.min_width = None;
            layout.min_height = None;
        }
        Square::Px(px) => {
            style.square = None;
            let edge = Some(LengthSpec::Px(px));
            layout.width = edge;
            layout.height = edge;
            layout.min_width = edge;
            layout.min_height = edge;
        }
    }
}

/// 控件左右内边距取主题的控件内边距档。
pub fn control_padding_x(layout: &mut LayoutStyle, padding: ControlPadding) {
    let px = Some(LengthSpec::Px(padding.resolve(CONTROL_METRICS)));
    let expr: ThemeExpr = ThemeToken::ControlPadding(padding).into();
    layout.padding_left = px;
    layout.padding_right = px;
    layout.theme.padding_left = Some(expr.clone());
    layout.theme.padding_right = Some(expr);
}

// ---- 动效 ----

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

// ---- 不透明度 ----

/// 状态层的不透明度档（深色主题和浅色主题各有一套）。
pub const fn opacity(layer: StateLayer, dark: bool) -> f32 {
    if dark {
        OpacityTokens::DARK.resolve(layer)
    } else {
        OpacityTokens::LIGHT.resolve(layer)
    }
}

/// 停用、锁住时整块淡下去的不透明度：主题状态层的 `Disabled` 档（默认 0.5），右键菜单项和滑杆也是这个值。
/// 有的控件直接写节点透明度，有的在 sRGB 里和所在底色混成不透明色（`theme_map::disabled_paint`），都用这一个值。
/// 遮罩的 0.45（`appearance.rs` 的 `SCRIM_CSS_ALPHA`）是另一个意思，不用它。
///
/// 节点不透明度在建树时就要定，状态层分深浅两套，禁用这一档两套相同，所以这里不分外观；哪天两套分开了，
/// 下面的断言编译不过，要改成按外观取（`opacity(StateLayer::Disabled, dark)`）。
pub const DISABLED_OPACITY: f32 = opacity(StateLayer::Disabled, true);
const _: () = assert!(opacity(StateLayer::Disabled, true) == opacity(StateLayer::Disabled, false));

// ---- 投影 ----

/// 节点后面一层主题高度档的落影，沿节点的圆角档（没有圆角时是直角）画。投影颜色、偏移和模糊都按装上的
/// 主题解析，深浅主题各有一套；底色、边和内容仍走节点自己的外观。播放条卡片和 PDF 页纸用它。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Elevated {
    role: ElevationRole,
    radius: Option<RadiusTier>,
}

impl Elevated {
    /// 直角节点的落影。
    pub const fn square(role: ElevationRole) -> Self {
        Self { role, radius: None }
    }

    /// 圆角是 `radius` 档的节点的落影。
    pub const fn rounded(role: ElevationRole, radius: RadiusTier) -> Self {
        Self {
            role,
            radius: Some(radius),
        }
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
        self.hash(&mut hasher);
        hasher.finish()
    }
}

impl Hash for Elevated {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.role.hash(state);
        self.radius.hash(state);
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

    /// 默认主题解析出来的数值和各档的默认像素一致：按表达式写的边和按档位写的边是同一个数。
    #[test]
    fn expressions_resolve_to_the_default_scale() {
        for step in [SpacingStep::Xxs, SpacingStep::Md, SpacingStep::Page] {
            assert_eq!(default_px(&space_expr(step)), space_px(step));
        }
        assert_eq!(default_px(&hairline_expr(-1.0)), -hairline_px());
        let mut layout = LayoutStyle::default();
        margin_by(&mut layout, &[Edge::Left], hairline_expr(-1.0));
        assert_eq!(layout.margin_left, Some(LengthSpec::Px(-1.0)));
        assert!(layout.theme.margin_left.is_some());
        offset(&mut layout, &Edge::ALL, SpacingStep::Sm);
        assert_eq!(layout.offset_bottom, Some(LengthSpec::Px(6.0)));
        assert!(layout.theme.offset_bottom.is_some());
        border_side(&mut layout, Edge::Bottom, 1);
        assert_eq!(layout.border_sides.bottom_width, Some(hairline_px()));
        assert!(layout.theme.border_bottom_width.is_some());
        assert_eq!(
            DISABLED_OPACITY,
            OpacityTokens::DARK.resolve(StateLayer::Disabled)
        );
    }

    /// 控件尺寸按 MomoBako 度量折像素，同时挂主题表达式；方块档交给 NanaUI 的 `square`，不写宽高。
    #[test]
    fn control_sizes_write_the_metrics_and_their_tokens() {
        let mut layout = LayoutStyle::default();
        height(&mut layout, Height::Framed(ControlSize::Medium));
        assert_eq!(layout.height, Some(LengthSpec::Px(34.0)));
        assert_eq!(default_px(layout.theme.height.as_ref().unwrap()), 34.0);
        assert_eq!(layout.theme.min_height, layout.theme.height);
        height(&mut layout, Height::Px(24.0));
        assert_eq!(layout.min_height, Some(LengthSpec::Px(24.0)));
        assert_eq!(layout.theme.height, None);
        control_width(&mut layout, ControlSize::Medium);
        assert_eq!(layout.width, Some(LengthSpec::Px(32.0)));
        assert!(layout.theme.min_width.is_some());

        let mut style = NodeStyle::default();
        square(&mut style, Square::Px(26.0));
        assert_eq!(style.layout.width, Some(LengthSpec::Px(26.0)));
        square(&mut style, Square::Tier(SquareSize::IconButton));
        assert_eq!(style.square, Some(SquareSize::IconButton));
        assert_eq!(style.layout.width, None);
        assert_eq!(style.layout.min_height, None);
        assert_eq!(Square::Tier(SquareSize::IconButton).px(), 22.0);
        assert_eq!(
            Square::Tier(SquareSize::Control(ControlSize::Small)).px(),
            28.0
        );
    }

    /// 行高落在行盒档上的取行盒档，字号在字阶上的按字号档的倍数写，字号不在字阶上的写成字号的倍数。
    #[test]
    fn line_heights_land_on_theme_tiers() {
        let mut layout = LayoutStyle::default();
        line_px(&mut layout, 12.0, 16.0);
        assert_eq!(
            layout.theme.line_height,
            Some(ThemeToken::Line(LineRole::Line).into())
        );
        line_px(&mut layout, 14.0, 14.0 * 1.55);
        assert!(
            matches!(layout.line_height, Some(LineHeightSpec::Absolute(px)) if (px - 21.7).abs() < 0.001)
        );
        assert!(layout.theme.line_height.is_some());
        line_px(&mut layout, 21.6, 21.6 * 1.24);
        assert!(
            matches!(layout.line_height, Some(LineHeightSpec::Relative(ratio)) if (ratio - 1.24).abs() < 0.0001)
        );
        assert_eq!(layout.theme.line_height, None);
    }
}
