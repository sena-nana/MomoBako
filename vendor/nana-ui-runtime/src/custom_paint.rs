//! 节点自绘（Issue #217）：业务侧像 Qt `paintEvent` 一样重写一个节点的外观。
//!
//! [`Painter`] 挂在 [`NodeStyle::painter`](crate::NodeStyle) 上。Runtime 在抽取
//! 节点时调用它，把 [`PaintContext`] 录下的命令解析成 [`PaintRecording`]，随
//! [`ExtractedNode::custom_paint`](crate::ExtractedNode) 进 UiScene。录制不是
//! 立即模式：同一节点在（`paint_key`、布局尺寸、主题代数）不变时直接复用上一次
//! 的录制，不再调用 `paint`。
//!
//! 颜色写 [`SemanticColorRole`]、圆角写 [`RadiusTier`]、阴影写 [`ElevationRole`]，
//! 由 `cx` 按当前主题解析，所以切换主题会自动重录。这里不暴露 GPU。

use std::any::TypeId;
use std::fmt;
use std::sync::Arc;

use nana_ui_core::{
    CompiledTheme, ElevationRole, Icon, RadiusTier, SemanticColorMix, SemanticColorRole,
    ThemeMetrics, ThemeMode,
};

use crate::{ComponentElevation, LayoutBox, TextHorizontalAlignment, TextVerticalAlignment};

/// 节点的自绘逻辑。
///
/// Rust 没有继承，这里用组合实现「重写」：`paint` 就是这个节点的 `paintEvent`。
/// 它只接管节点自己的外观（背景、边框、阴影、装饰），子节点照常布局和绘制。
pub trait Painter: Send + Sync + 'static {
    /// 画在子节点下面。不调用 [`PaintContext::draw_default`] 就等于完全替换内建外观。
    fn paint(&self, cx: &mut PaintContext<'_>);

    /// 画在子节点上面（前景）。默认不画。
    fn paint_over_children(&self, _cx: &mut PaintContext<'_>) {}

    /// 自定义命中形状：`local` 是节点内坐标（原点在节点左上角），`size` 是
    /// 布局尺寸。返回 `Some(false)` 时这一点点穿到下面；返回 `None` 交给
    /// [`Self::hit_painted_outline`] 决定。只在布局矩形以内被问到。
    fn hit_test(&self, _local: [f32; 2], _size: [f32; 2]) -> Option<bool> {
        None
    }

    /// 命中区域是否等于录下的内容（填充、描边、圆角矩形、图片、文字框和
    /// `draw_default()` 所占的节点矩形，按局部变换和 `push_clip` 计算）。
    /// 默认 `false`：整个布局矩形都能命中。
    fn hit_painted_outline(&self) -> bool {
        false
    }

    /// 绘制输入的摘要：两次返回相同的值，就保证 `paint` 录出同样的命令。
    ///
    /// 布局尺寸和主题不用算进来，Runtime 已经把它们放进缓存键。
    fn paint_key(&self) -> u64;
}

/// 挂在 [`NodeStyle`](crate::NodeStyle) 上的 [`Painter`]。
///
/// 相等按（painter 类型，`paint_key`）比较：视图每帧重建出一个同键的新
/// painter 不算样式变化，不会让节点重新抽取；换了键才重录。
#[derive(Clone)]
pub struct NodePainter {
    painter: Arc<dyn Painter>,
    type_id: TypeId,
}

impl NodePainter {
    pub fn new<P: Painter>(painter: P) -> Self {
        Self {
            painter: Arc::new(painter),
            type_id: TypeId::of::<P>(),
        }
    }

    pub fn painter(&self) -> &dyn Painter {
        self.painter.as_ref()
    }

    pub(crate) fn cache_key(
        &self,
        size: [f32; 2],
        theme_epoch: u64,
        state: PaintState,
    ) -> PaintCacheKey {
        PaintCacheKey {
            painter: self.type_id,
            paint_key: self.painter.paint_key(),
            size: [size[0].to_bits(), size[1].to_bits()],
            theme_epoch,
            state,
        }
    }
}

impl<P: Painter> From<P> for NodePainter {
    fn from(painter: P) -> Self {
        Self::new(painter)
    }
}

impl PartialEq for NodePainter {
    fn eq(&self, other: &Self) -> bool {
        self.type_id == other.type_id
            && (Arc::ptr_eq(&self.painter, &other.painter)
                || self.painter.paint_key() == other.painter.paint_key())
    }
}

impl fmt::Debug for NodePainter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NodePainter")
            .field("paint_key", &self.painter.paint_key())
            .finish_non_exhaustive()
    }
}

/// 录制缓存键：节点之外的那一半（节点由缓存表本身区分）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PaintCacheKey {
    painter: TypeId,
    paint_key: u64,
    size: [u32; 2],
    theme_epoch: u64,
    state: PaintState,
}

/// 节点当前的交互状态。状态变化会让节点重录，painter 不必把它算进
/// `paint_key`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct PaintState {
    /// 有指针悬停在节点上。
    pub hovered: bool,
    /// 有指针在节点上按下未抬起。
    pub pressed: bool,
    /// 节点持有可见焦点（键盘导航得到的焦点；鼠标点击得到的焦点不算）。
    pub focused: bool,
    /// 无障碍状态为禁用。
    pub disabled: bool,
    /// 无障碍状态为选中或勾选（含半选）。
    pub selected: bool,
}

/// 一段文字排出来的尺寸，逻辑像素。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TextSize {
    pub width: f32,
    pub height: f32,
    /// 首行基线到文字框顶部的距离。
    pub baseline: f32,
}

/// 与下层内容的混合方式，同 CSS `mix-blend-mode`。
pub type BlendMode = nana_ui_core::MixBlendMode;

/// 测量一段文字：内容、样式、最大宽度（`None` 不折行）。
pub(crate) type TextMeasure<'a> = &'a dyn Fn(&PaintText, f32, Option<f32>) -> TextSize;

/// 颜色：语义角色、角色混色，或者直接给定的 RGBA。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PaintColor {
    Role(SemanticColorRole),
    Mix(SemanticColorMix),
    Rgba([f32; 4]),
}

impl From<SemanticColorRole> for PaintColor {
    fn from(role: SemanticColorRole) -> Self {
        Self::Role(role)
    }
}

impl From<SemanticColorMix> for PaintColor {
    fn from(mix: SemanticColorMix) -> Self {
        Self::Mix(mix)
    }
}

impl From<[f32; 4]> for PaintColor {
    fn from(rgba: [f32; 4]) -> Self {
        Self::Rgba(rgba)
    }
}

/// 长度：像素，或主题圆角档位。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Radius {
    Px(f32),
    Tier(RadiusTier),
}

impl From<f32> for Radius {
    fn from(px: f32) -> Self {
        Self::Px(px)
    }
}

impl From<RadiusTier> for Radius {
    fn from(tier: RadiusTier) -> Self {
        Self::Tier(tier)
    }
}

/// 四个角的圆角，顺序为左上、右上、右下、左下。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CornerRadii(pub [Radius; 4]);

impl<R: Into<Radius>> From<R> for CornerRadii {
    fn from(radius: R) -> Self {
        let radius = radius.into();
        Self([radius; 4])
    }
}

impl<R: Into<Radius> + Copy> From<[R; 4]> for CornerRadii {
    fn from(radii: [R; 4]) -> Self {
        Self(radii.map(Into::into))
    }
}

/// 阴影：主题的高度档位，或者自定义参数（CSS `box-shadow` 语义，逻辑像素）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PaintShadow {
    Elevation(ElevationRole),
    Custom {
        color: PaintColor,
        offset: [f32; 2],
        blur: f32,
        spread: f32,
        /// CSS `box-shadow: inset`：画在路径以内。
        inset: bool,
    },
}

impl From<ElevationRole> for PaintShadow {
    fn from(role: ElevationRole) -> Self {
        Self::Elevation(role)
    }
}

/// 填充规则，同 SVG `fill-rule` / Canvas `fill(rule)`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FillRule {
    /// 环绕数不为 0 的区域（缺省）。
    #[default]
    NonZero,
    /// 环绕数为奇数的区域：自交处和内环挖空。
    EvenOdd,
}

/// 描边拐角的连接方式，同 Canvas `lineJoin`，缺省与 Canvas、SVG 一致为尖角。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineJoin {
    /// 尖角，超过 [`StrokeStyle::miter_limit`] 时改为斜切。
    #[default]
    Miter,
    /// 圆角。
    Round,
    /// 斜切。
    Bevel,
}

/// 开放路径端点的线帽，同 Canvas `lineCap`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineCap {
    /// 平头，止于端点（缺省）。
    #[default]
    Butt,
    /// 圆头，伸出半个线宽。
    Round,
    /// 方头，伸出半个线宽。
    Square,
}

/// 描边参数，长度都在录制时的局部坐标系（见 [`PaintContext::transform`]）里。
#[derive(Debug, Clone, PartialEq)]
pub struct StrokeStyle {
    pub width: f32,
    pub join: LineJoin,
    pub cap: LineCap,
    /// Canvas `setLineDash`：实段、空段交替的长度。空表示实线；奇数个时重复
    /// 一遍凑成偶数；含负数或非有限值、或总长为 0 时按实线处理。
    pub dash: Arc<[f32]>,
    /// Canvas `lineDashOffset`：虚线从图案的这个位置开始。
    pub dash_offset: f32,
    /// Canvas `miterLimit`：尖角连接的斜接长度超过线宽的这个倍数就改为斜切。
    /// 只对 [`LineJoin::Miter`] 生效，默认 10。
    pub miter_limit: f32,
}

impl StrokeStyle {
    pub fn new(width: f32) -> Self {
        Self {
            width,
            join: LineJoin::default(),
            cap: LineCap::default(),
            dash: Arc::from(Vec::new()),
            dash_offset: 0.0,
            miter_limit: 10.0,
        }
    }

    pub fn miter_limit(mut self, limit: f32) -> Self {
        self.miter_limit = limit;
        self
    }

    /// 虚线，见 [`Self::dash`]。每一段都按 [`Self::cap`] 收尾。
    pub fn dash(mut self, pattern: impl Into<Arc<[f32]>>, offset: f32) -> Self {
        self.dash = pattern.into();
        self.dash_offset = offset;
        self
    }

    pub fn join(mut self, join: LineJoin) -> Self {
        self.join = join;
        self
    }

    pub fn cap(mut self, cap: LineCap) -> Self {
        self.cap = cap;
        self
    }
}

impl From<f32> for StrokeStyle {
    fn from(width: f32) -> Self {
        Self::new(width)
    }
}

/// 渐变的一个色标，`offset` 在 `0..=1`。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColorStop {
    pub offset: f32,
    pub color: PaintColor,
}

/// 渐变几何，坐标在录制时的局部坐标系里，随 [`PaintContext::transform`] 一起变换。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GradientShape {
    /// 沿 `start → end`，`t = 0` 在 `start`。
    Linear { start: [f32; 2], end: [f32; 2] },
    /// 以 `center` 为圆心，`t = 0` 在圆心、`t = 1` 在半径处。
    Radial { center: [f32; 2], radius: f32 },
    /// 绕 `center` 一周，`t = 0` 在 `start_angle`（弧度，0 指向 +x，顺时针为正，y 轴向下）。
    Conic { center: [f32; 2], start_angle: f32 },
}

/// `t` 落在 `0..=1` 之外时怎么取色。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GradientExtend {
    /// 取端点色标。
    #[default]
    Pad,
    Repeat,
    Reflect,
}

/// 线性 / 径向 / 锥形渐变。色标在 premultiplied sRGB 里插值（与 CSS 一致）。
#[derive(Debug, Clone, PartialEq)]
pub struct Gradient {
    pub shape: GradientShape,
    pub stops: Vec<ColorStop>,
    pub extend: GradientExtend,
}

impl Gradient {
    pub fn linear(start: [f32; 2], end: [f32; 2]) -> Self {
        Self::new(GradientShape::Linear { start, end })
    }

    pub fn radial(center: [f32; 2], radius: f32) -> Self {
        Self::new(GradientShape::Radial { center, radius })
    }

    pub fn conic(center: [f32; 2], start_angle: f32) -> Self {
        Self::new(GradientShape::Conic {
            center,
            start_angle,
        })
    }

    fn new(shape: GradientShape) -> Self {
        Self {
            shape,
            stops: Vec::new(),
            extend: GradientExtend::default(),
        }
    }

    pub fn stop(mut self, offset: f32, color: impl Into<PaintColor>) -> Self {
        self.stops.push(ColorStop {
            offset,
            color: color.into(),
        });
        self
    }

    pub fn extend(mut self, extend: GradientExtend) -> Self {
        self.extend = extend;
        self
    }
}

/// 填充或描边用什么上色：纯色或渐变。
#[derive(Debug, Clone, PartialEq)]
pub enum Paint {
    Color(PaintColor),
    Gradient(Gradient),
}

impl<C: Into<PaintColor>> From<C> for Paint {
    fn from(color: C) -> Self {
        Self::Color(color.into())
    }
}

impl From<Gradient> for Paint {
    fn from(gradient: Gradient) -> Self {
        Self::Gradient(gradient)
    }
}

/// 按主题解析后的渐变：色标已排序并夹到 `0..=1`，颜色为 sRGB RGBA。
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedGradient {
    pub shape: GradientShape,
    pub stops: Vec<(f32, [f32; 4])>,
    pub extend: GradientExtend,
}

/// 按主题解析后的上色。
#[derive(Debug, Clone, PartialEq)]
pub enum ResolvedPaint {
    Solid([f32; 4]),
    Gradient(Arc<ResolvedGradient>),
}

impl ResolvedPaint {
    /// 画出来是否完全透明。
    pub fn is_invisible(&self) -> bool {
        match self {
            Self::Solid(color) => color[3] <= 0.0,
            Self::Gradient(gradient) => gradient.stops.iter().all(|(_, color)| color[3] <= 0.0),
        }
    }
}

/// 图片在目标矩形里怎么摆，同 CSS `object-fit`，居中对齐。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImageFit {
    /// 拉伸铺满，不保持宽高比。
    Fill,
    /// 完整放进矩形，保持宽高比。
    #[default]
    Contain,
    /// 铺满矩形，保持宽高比，超出部分裁掉。
    Cover,
    /// 原始尺寸。
    None,
    /// `None` 与 `Contain` 中较小的一个。
    ScaleDown,
}

/// 2D 仿射变换 `[a, b, c, d, e, f]`，同 Canvas `setTransform`：
/// `x' = a·x + c·y + e`，`y' = b·x + d·y + f`。
pub type Affine = [f32; 6];

/// 恒等变换。
pub const AFFINE_IDENTITY: Affine = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// `outer ∘ inner`：先 `inner` 再 `outer`。
pub fn affine_multiply(outer: Affine, inner: Affine) -> Affine {
    let [a, b, c, d, e, f] = outer;
    let [ia, ib, ic, id, ie, iff] = inner;
    [
        a * ia + c * ib,
        b * ia + d * ib,
        a * ic + c * id,
        b * ic + d * id,
        a * ie + c * iff + e,
        b * ie + d * iff + f,
    ]
}

/// 路径命令。圆弧在构建时已经换成三次贝塞尔。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PathVerb {
    MoveTo([f32; 2]),
    LineTo([f32; 2]),
    QuadTo([f32; 2], [f32; 2]),
    CubicTo([f32; 2], [f32; 2], [f32; 2]),
    Close,
}

/// 节点内坐标系下的路径（原点是节点左上角，单位逻辑像素）。
///
/// 构建方式与 Canvas / `QPainterPath` 相同。[`Self::arc_to`] 用来给任意拐角倒圆，
/// 凹角也一样。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PaintPath {
    verbs: Vec<PathVerb>,
    fill_rule: FillRule,
    start: Option<[f32; 2]>,
    current: Option<[f32; 2]>,
    /// 整条路径只是一个 [`Self::rect`] / [`Self::rounded_rect`] 时的矩形与四角半径。
    shape_hint: Option<(LayoutBox, [f32; 4])>,
}

impl PaintPath {
    /// 空路径，填充规则为 [`FillRule::NonZero`]。
    pub fn new() -> Self {
        Self::default()
    }

    /// 录下的路径命令。
    pub fn verbs(&self) -> &[PathVerb] {
        &self.verbs
    }

    /// 填充规则。
    pub fn fill_rule(&self) -> FillRule {
        self.fill_rule
    }

    /// 是否一条命令也没有。
    pub fn is_empty(&self) -> bool {
        self.verbs.is_empty()
    }

    /// 换一种填充规则，按值链式使用：`PaintPath::new().with_fill_rule(..)`。
    pub fn with_fill_rule(mut self, rule: FillRule) -> Self {
        self.fill_rule = rule;
        self
    }

    /// 换一种填充规则，与其他构建方法一样可在 `&mut` 链上调用。
    pub fn set_fill_rule(&mut self, rule: FillRule) -> &mut Self {
        self.fill_rule = rule;
        self
    }

    /// 点是否落在路径按填充规则围成的区域里，同 Canvas `isPointInPath`。
    pub fn contains(&self, point: [f32; 2]) -> bool {
        path_contains(self, AFFINE_IDENTITY, point)
    }

    /// 点是否落在按 `stroke` 描出的线上（含虚线的空段判断），同 Canvas
    /// `isPointInStroke`；线帽和连接按圆形算。
    pub fn stroke_contains(&self, point: [f32; 2], stroke: &StrokeStyle) -> bool {
        stroke_contains(self, stroke, AFFINE_IDENTITY, point)
    }

    /// 所有点（含曲线控制点）的外接矩形；空路径为 `None`。曲线本身不会超出它。
    pub fn bounds(&self) -> Option<LayoutBox> {
        let mut points = self.verbs.iter().flat_map(|verb| match *verb {
            PathVerb::MoveTo(p) | PathVerb::LineTo(p) => vec![p],
            PathVerb::QuadTo(c, p) => vec![c, p],
            PathVerb::CubicTo(a, b, p) => vec![a, b, p],
            PathVerb::Close => Vec::new(),
        });
        let first = points.next()?;
        let (min, max) = points.fold((first, first), |(min, max), p| {
            (
                [min[0].min(p[0]), min[1].min(p[1])],
                [max[0].max(p[0]), max[1].max(p[1])],
            )
        });
        Some(LayoutBox {
            x: min[0],
            y: min[1],
            width: max[0] - min[0],
            height: max[1] - min[1],
        })
    }

    /// 经 `transform` 变换后的新路径，填充规则不变。
    pub fn transformed(&self, transform: Affine) -> Self {
        let [a, b, c, d, e, f] = transform;
        let map = |p: [f32; 2]| [a * p[0] + c * p[1] + e, b * p[0] + d * p[1] + f];
        Self {
            verbs: self
                .verbs
                .iter()
                .map(|verb| match *verb {
                    PathVerb::MoveTo(p) => PathVerb::MoveTo(map(p)),
                    PathVerb::LineTo(p) => PathVerb::LineTo(map(p)),
                    PathVerb::QuadTo(c, p) => PathVerb::QuadTo(map(c), map(p)),
                    PathVerb::CubicTo(x, y, p) => PathVerb::CubicTo(map(x), map(y), map(p)),
                    PathVerb::Close => PathVerb::Close,
                })
                .collect(),
            fill_rule: self.fill_rule,
            start: self.start.map(map),
            current: self.current.map(map),
            shape_hint: None,
        }
    }

    pub fn move_to(&mut self, x: f32, y: f32) -> &mut Self {
        self.shape_hint = None;
        self.verbs.push(PathVerb::MoveTo([x, y]));
        self.start = Some([x, y]);
        self.current = Some([x, y]);
        self
    }

    pub fn line_to(&mut self, x: f32, y: f32) -> &mut Self {
        if self.current.is_none() {
            return self.move_to(x, y);
        }
        self.shape_hint = None;
        self.verbs.push(PathVerb::LineTo([x, y]));
        self.current = Some([x, y]);
        self
    }

    pub fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) -> &mut Self {
        if self.current.is_none() {
            self.move_to(cx, cy);
        }
        self.shape_hint = None;
        self.verbs.push(PathVerb::QuadTo([cx, cy], [x, y]));
        self.current = Some([x, y]);
        self
    }

    /// 三次贝塞尔曲线到 `(x, y)`，控制点 `(c1x, c1y)`、`(c2x, c2y)`，同 Canvas
    /// `bezierCurveTo`。
    pub fn cubic_to(
        &mut self,
        c1x: f32,
        c1y: f32,
        c2x: f32,
        c2y: f32,
        x: f32,
        y: f32,
    ) -> &mut Self {
        self.cubic([c1x, c1y], [c2x, c2y], [x, y])
    }

    fn cubic(&mut self, c1: [f32; 2], c2: [f32; 2], to: [f32; 2]) -> &mut Self {
        if self.current.is_none() {
            self.move_to(c1[0], c1[1]);
        }
        self.shape_hint = None;
        self.verbs.push(PathVerb::CubicTo(c1, c2, to));
        self.current = Some(to);
        self
    }

    pub fn close(&mut self) -> &mut Self {
        if self.current.is_some() {
            self.shape_hint = None;
            self.verbs.push(PathVerb::Close);
            self.current = self.start;
        }
        self
    }

    /// Canvas `arcTo(x1, y1, x2, y2, radius)`：从当前点朝拐角 `(x1, y1)` 走，
    /// 在拐角处以 `radius` 倒圆后转向 `(x2, y2)`，停在第二条边的切点上。凸角、
    /// 凹角一视同仁。
    ///
    /// 半径超过两条边能容下的长度时自动缩小；三点共线时退化成直线到拐角。
    pub fn arc_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, radius: f32) -> &mut Self {
        self.corner_arc([x1, y1], [x2, y2], radius)
    }

    fn corner_arc(&mut self, corner: [f32; 2], to: [f32; 2], radius: f32) -> &mut Self {
        let Some(from) = self.current else {
            return self.move_to(corner[0], corner[1]);
        };
        let v1 = sub(from, corner);
        let v2 = sub(to, corner);
        let (l1, l2) = (len(v1), len(v2));
        if radius <= 0.0 || l1 < 1e-6 || l2 < 1e-6 {
            return self.line_to(corner[0], corner[1]);
        }
        let (u1, u2) = (scale(v1, 1.0 / l1), scale(v2, 1.0 / l2));
        let cos = (u1[0] * u2[0] + u1[1] * u2[1]).clamp(-1.0, 1.0);
        let theta = cos.acos();
        if theta < 1e-4 || (std::f32::consts::PI - theta) < 1e-4 {
            return self.line_to(corner[0], corner[1]);
        }
        let half = theta * 0.5;
        let mut distance = radius / half.tan();
        let mut radius = radius;
        let limit = l1.min(l2);
        if distance > limit {
            distance = limit;
            radius = distance * half.tan();
        }
        let t1 = add(corner, scale(u1, distance));
        let t2 = add(corner, scale(u2, distance));
        let bisector = add(u1, u2);
        let center = add(
            corner,
            scale(bisector, (radius / half.sin()) / len(bisector)),
        );
        let start = (t1[1] - center[1]).atan2(t1[0] - center[0]);
        let end = (t2[1] - center[1]).atan2(t2[0] - center[0]);
        let mut sweep = end - start;
        if sweep > std::f32::consts::PI {
            sweep -= std::f32::consts::TAU;
        } else if sweep < -std::f32::consts::PI {
            sweep += std::f32::consts::TAU;
        }
        self.line_to(t1[0], t1[1]);
        self.arc_segments(center, radius, start, sweep);
        self
    }

    /// 以 `(cx, cy)` 为圆心的圆弧，角度为弧度，`sweep` 是扫过的角度（与
    /// `QPainterPath::arcTo` 一样，不是 Canvas 的终止角），为正时顺时针（y 轴
    /// 向下），超过一整圈按一整圈画。已有当前点时先连一条直线到弧的起点。
    pub fn arc(&mut self, cx: f32, cy: f32, radius: f32, start: f32, sweep: f32) -> &mut Self {
        let center = [cx, cy];
        let begin = [
            center[0] + radius * start.cos(),
            center[1] + radius * start.sin(),
        ];
        if self.current.is_some() {
            self.line_to(begin[0], begin[1]);
        } else {
            self.move_to(begin[0], begin[1]);
        }
        self.arc_segments(center, radius, start, sweep);
        self
    }

    fn arc_segments(&mut self, center: [f32; 2], radius: f32, start: f32, sweep: f32) {
        // Canvas clamps a sweep past a full turn; a non-finite one draws nothing.
        if !sweep.is_finite() || !start.is_finite() || !radius.is_finite() {
            return;
        }
        let sweep = sweep.clamp(-std::f32::consts::TAU, std::f32::consts::TAU);
        let count = (sweep.abs() / std::f32::consts::FRAC_PI_2).ceil().max(1.0) as usize;
        let step = sweep / count as f32;
        let k = 4.0 / 3.0 * (step / 4.0).tan() * radius;
        let mut angle = start;
        for _ in 0..count {
            let next = angle + step;
            let (s0, c0) = angle.sin_cos();
            let (s1, c1) = next.sin_cos();
            let p0 = [center[0] + radius * c0, center[1] + radius * s0];
            let p3 = [center[0] + radius * c1, center[1] + radius * s1];
            self.cubic(
                [p0[0] - k * s0, p0[1] + k * c0],
                [p3[0] + k * s1, p3[1] - k * c1],
                p3,
            );
            angle = next;
        }
    }

    /// 矩形子路径。
    pub fn rect(&mut self, rect: LayoutBox) -> &mut Self {
        let alone = self.verbs.is_empty();
        self.move_to(rect.x, rect.y)
            .line_to(rect.x + rect.width, rect.y)
            .line_to(rect.x + rect.width, rect.y + rect.height)
            .line_to(rect.x, rect.y + rect.height)
            .close();
        self.shape_hint = alone.then_some((rect, [0.0; 4]));
        self
    }

    /// 四角独立的圆角矩形子路径，半径按左上、右上、右下、左下，超出时按 CSS
    /// 规则等比缩小。
    pub fn rounded_rect(&mut self, rect: LayoutBox, radii: [f32; 4]) -> &mut Self {
        let alone = self.verbs.is_empty();
        let fitted = fit_radii(rect, radii);
        let [tl, tr, br, bl] = fitted;
        let (x, y, w, h) = (rect.x, rect.y, rect.width, rect.height);
        self.move_to(x + tl, y).line_to(x + w - tr, y);
        if tr > 0.0 {
            self.corner_arc([x + w, y], [x + w, y + h], tr);
        }
        self.line_to(x + w, y + h - br);
        if br > 0.0 {
            self.corner_arc([x + w, y + h], [x, y + h], br);
        }
        self.line_to(x + bl, y + h);
        if bl > 0.0 {
            self.corner_arc([x, y + h], [x, y], bl);
        }
        self.line_to(x, y + tl);
        if tl > 0.0 {
            self.corner_arc([x, y], [x + w, y], tl);
        }
        self.close();
        self.shape_hint = alone.then_some((rect, fitted));
        self
    }

    /// 内切于 `rect` 的椭圆子路径。
    pub fn ellipse(&mut self, rect: LayoutBox) -> &mut Self {
        let (rx, ry) = (rect.width * 0.5, rect.height * 0.5);
        let (cx, cy) = (rect.x + rx, rect.y + ry);
        let k = 0.552_284_8;
        self.move_to(cx + rx, cy)
            .cubic(
                [cx + rx, cy + ry * k],
                [cx + rx * k, cy + ry],
                [cx, cy + ry],
            )
            .cubic(
                [cx - rx * k, cy + ry],
                [cx - rx, cy + ry * k],
                [cx - rx, cy],
            )
            .cubic(
                [cx - rx, cy - ry * k],
                [cx - rx * k, cy - ry],
                [cx, cy - ry],
            )
            .cubic(
                [cx + rx * k, cy - ry],
                [cx + rx, cy - ry * k],
                [cx + rx, cy],
            )
            .close()
    }

    /// 这条路径若只是一个轴对齐的（圆角）矩形，返回它和四角半径（左上、右上、
    /// 右下、左下，已按 CSS 规则缩到放得下）。
    ///
    /// Scene 用它把这样的填充和描边交给矩形图元画，不必三角化。
    pub fn as_rounded_rect_corners(&self) -> Option<(LayoutBox, [f32; 4])> {
        self.shape_hint
    }

    /// 这条路径若只是一个轴对齐的（圆角）矩形，返回它和统一的圆角半径。
    ///
    /// Scene 用它把裁剪精确地交给 GPU 裁剪区，而不是退回外接矩形。
    pub fn as_rounded_rect(&self) -> Option<(LayoutBox, f32)> {
        self.shape_hint
            .as_ref()
            .copied()
            .filter(|(_, radii)| radii.iter().all(|r| (r - radii[0]).abs() < 1e-4))
            .map(|(rect, radii)| (rect, radii[0]))
    }
}

fn fit_radii(rect: LayoutBox, radii: [f32; 4]) -> [f32; 4] {
    let radii = radii.map(|r| if r.is_finite() { r.max(0.0) } else { 0.0 });
    let [tl, tr, br, bl] = radii;
    let mut factor = 1.0f32;
    for (sum, side) in [
        (tl + tr, rect.width),
        (tr + br, rect.height),
        (br + bl, rect.width),
        (bl + tl, rect.height),
    ] {
        if sum > side.max(0.0) && sum > 0.0 {
            factor = factor.min(side.max(0.0) / sum);
        }
    }
    radii.map(|r| r * factor)
}

fn sub(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

fn add(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] + b[0], a[1] + b[1]]
}

fn scale(a: [f32; 2], s: f32) -> [f32; 2] {
    [a[0] * s, a[1] * s]
}

fn len(a: [f32; 2]) -> f32 {
    (a[0] * a[0] + a[1] * a[1]).sqrt()
}

/// 圆角矩形的外观：填充、描边、阴影都可选。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BoxPaint {
    pub fill: Option<Paint>,
    /// 颜色与宽度（逻辑像素），描在矩形内侧。
    pub border: Option<(PaintColor, f32)>,
    pub shadow: Option<PaintShadow>,
}

impl BoxPaint {
    pub fn fill(paint: impl Into<Paint>) -> Self {
        Self {
            fill: Some(paint.into()),
            ..Self::default()
        }
    }

    pub fn border(mut self, color: impl Into<PaintColor>, width: f32) -> Self {
        self.border = Some((color.into(), width));
        self
    }

    pub fn shadow(mut self, shadow: impl Into<PaintShadow>) -> Self {
        self.shadow = Some(shadow.into());
        self
    }
}

/// 一段文字。字号缺省为主题正文字号。
#[derive(Debug, Clone, PartialEq)]
pub struct PaintText {
    pub content: Arc<str>,
    /// 纯色或渐变。
    pub paint: Paint,
    pub size: Option<f32>,
    pub weight: Option<u16>,
    pub italic: bool,
    /// 行高，逻辑像素；`None` 用字体默认行高。
    pub line_height: Option<f32>,
    /// 超出宽度时折行；否则单行，超长省略。
    pub wrap: bool,
    /// 最多几行，超出省略。
    pub max_lines: Option<u16>,
    pub horizontal: TextHorizontalAlignment,
    pub vertical: TextVerticalAlignment,
}

impl PaintText {
    pub fn new(content: impl Into<Arc<str>>) -> Self {
        Self {
            content: content.into(),
            paint: Paint::Color(PaintColor::Role(SemanticColorRole::Text)),
            size: None,
            weight: None,
            italic: false,
            line_height: None,
            wrap: false,
            max_lines: None,
            horizontal: TextHorizontalAlignment::Start,
            vertical: TextVerticalAlignment::Center,
        }
    }

    /// 纯色或渐变。
    pub fn paint(mut self, paint: impl Into<Paint>) -> Self {
        self.paint = paint.into();
        self
    }

    pub fn color(self, color: impl Into<PaintColor>) -> Self {
        self.paint(color.into())
    }

    pub fn italic(mut self, italic: bool) -> Self {
        self.italic = italic;
        self
    }

    pub fn line_height(mut self, line_height: f32) -> Self {
        self.line_height = Some(line_height);
        self
    }

    pub fn wrap(mut self, wrap: bool) -> Self {
        self.wrap = wrap;
        self
    }

    pub fn max_lines(mut self, lines: u16) -> Self {
        self.max_lines = Some(lines);
        self
    }

    pub fn size(mut self, size: f32) -> Self {
        self.size = Some(size);
        self
    }

    pub fn weight(mut self, weight: u16) -> Self {
        self.weight = Some(weight);
        self
    }

    pub fn align(
        mut self,
        horizontal: TextHorizontalAlignment,
        vertical: TextVerticalAlignment,
    ) -> Self {
        self.horizontal = horizontal;
        self.vertical = vertical;
        self
    }
}

/// 绘制阶段：子节点下面（背景）或上面（前景）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaintPhase {
    BehindChildren,
    OverChildren,
}

/// 录下的一条命令，颜色和尺寸已按主题解析，坐标在节点内坐标系。
#[derive(Debug, Clone, PartialEq)]
pub enum PaintOp {
    /// 该节点原本的内建外观（StandardVisual、背景、边框、文字）。
    DrawDefault,
    /// 之后的命令都在这个局部变换下（相对节点左上角），直到下一条
    /// `SetTransform`。录制开始时是恒等变换。
    SetTransform(Affine),
    /// 之后每条命令各自乘上这个不透明度（Canvas `globalAlpha`），直到下一条
    /// `SetOpacity`。录制开始时是 1。
    SetOpacity(f32),
    /// 之后的命令先画进一个图层，到配对的 [`Self::PopLayer`] 时按
    /// `opacity` 和 `blend` 整体合成到下面。
    PushLayer {
        opacity: f32,
        blend: BlendMode,
    },
    PopLayer,
    FillPath {
        path: Arc<PaintPath>,
        paint: ResolvedPaint,
    },
    StrokePath {
        path: Arc<PaintPath>,
        paint: ResolvedPaint,
        stroke: StrokeStyle,
    },
    Shadow {
        path: Arc<PaintPath>,
        shadow: ComponentElevation,
    },
    RoundedRect {
        rect: LayoutBox,
        radii: [f32; 4],
        fill: Option<ResolvedPaint>,
        border: Option<([f32; 4], f32)>,
        shadow: Option<ComponentElevation>,
    },
    Image {
        rect: LayoutBox,
        /// 与 CSS `url()` 相同的来源：`data:`、文件路径或经宿主 fetch 的 URL。
        source: Arc<str>,
        fit: ImageFit,
        /// 录制时的 [`PaintContext::set_image_sampling`]。
        sampling: nana_ui_core::ImageSampling,
        radii: [f32; 4],
    },
    /// 图片摆进 `rect`，只露出 `path` 以内的部分。
    FillImage {
        path: Arc<PaintPath>,
        rect: LayoutBox,
        source: Arc<str>,
        fit: ImageFit,
        sampling: nana_ui_core::ImageSampling,
    },
    Text {
        rect: LayoutBox,
        content: Arc<str>,
        paint: ResolvedPaint,
        size: f32,
        weight: Option<u16>,
        italic: bool,
        line_height: Option<f32>,
        wrap: bool,
        max_lines: Option<u16>,
        horizontal: TextHorizontalAlignment,
        vertical: TextVerticalAlignment,
    },
    Icon {
        rect: LayoutBox,
        icon: Icon,
        paint: ResolvedPaint,
    },
    /// 之后的命令裁剪到这条路径以内，直到配对的 [`Self::PopClip`]。
    PushClip {
        path: Arc<PaintPath>,
    },
    PopClip,
}

/// 一个节点的录制结果。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PaintRecording {
    pub behind_children: Vec<PaintOp>,
    pub over_children: Vec<PaintOp>,
}

impl PaintRecording {
    /// 节点内坐标 `local` 是否落在录下的内容里：填充、描边、圆角矩形、图片、
    /// 文字和图标的框，以及 `draw_default()` 所占的节点矩形；按当时的局部变换
    /// 和 `push_clip` 计算。阴影不算。
    pub fn contains(&self, local: [f32; 2], size: [f32; 2]) -> bool {
        phase_contains(&self.behind_children, local, size)
            || phase_contains(&self.over_children, local, size)
    }

    /// `draw_default()` 录在哪个阶段；没调用则为 `None`。
    pub fn default_phase(&self) -> Option<PaintPhase> {
        if self.behind_children.contains(&PaintOp::DrawDefault) {
            Some(PaintPhase::BehindChildren)
        } else if self.over_children.contains(&PaintOp::DrawDefault) {
            Some(PaintPhase::OverChildren)
        } else {
            None
        }
    }
}

fn phase_contains(ops: &[PaintOp], p: [f32; 2], size: [f32; 2]) -> bool {
    let mut t = AFFINE_IDENTITY;
    let mut clips: Vec<(&PaintPath, Affine)> = Vec::new();
    let mut visible = true;
    // Layers open, each whether it composites at all.
    let mut layers: Vec<bool> = Vec::new();
    for op in ops {
        let hit = match op {
            PaintOp::SetTransform(transform) => {
                t = *transform;
                false
            }
            PaintOp::PushClip { path } => {
                clips.push((path, t));
                false
            }
            PaintOp::PopClip => {
                clips.pop();
                false
            }
            PaintOp::FillPath { path, .. } | PaintOp::FillImage { path, .. } => {
                path_contains(path, t, p)
            }
            PaintOp::StrokePath { path, stroke, .. } => stroke_contains(path, stroke, t, p),
            PaintOp::RoundedRect { rect, radii, .. } | PaintOp::Image { rect, radii, .. } => {
                inverse_apply(t, p).is_some_and(|q| rounded_rect_contains(*rect, *radii, q))
            }
            PaintOp::Text { rect, .. } | PaintOp::Icon { rect, .. } => {
                inverse_apply(t, p).is_some_and(|q| rounded_rect_contains(*rect, [0.0; 4], q))
            }
            PaintOp::DrawDefault => inverse_apply(t, p)
                .is_some_and(|q| q[0] >= 0.0 && q[1] >= 0.0 && q[0] <= size[0] && q[1] <= size[1]),
            PaintOp::SetOpacity(opacity) => {
                // What draws fully transparent is not there to hit.
                visible = *opacity > 0.0;
                false
            }
            PaintOp::PushLayer { opacity, .. } => {
                layers.push(*opacity > 0.0);
                false
            }
            PaintOp::PopLayer => {
                layers.pop();
                false
            }
            PaintOp::Shadow { .. } => false,
        };
        if hit
            && visible
            && layers.iter().all(|shown| *shown)
            && clips.iter().all(|(clip, ct)| path_contains(clip, *ct, p))
        {
            return true;
        }
    }
    false
}

fn inverse_apply(t: Affine, p: [f32; 2]) -> Option<[f32; 2]> {
    let [a, b, c, d, e, f] = t;
    let det = a * d - b * c;
    if !det.is_finite() || det.abs() < 1e-12 {
        return None;
    }
    let (x, y) = (p[0] - e, p[1] - f);
    Some([(d * x - c * y) / det, (a * y - b * x) / det])
}

/// Sub-paths as polylines in the path's own coordinates, with whether each
/// was closed. Coarse, for hit testing: a curve is sixteen segments.
fn polylines(path: &PaintPath) -> Vec<(Vec<[f32; 2]>, bool)> {
    let mut out: Vec<(Vec<[f32; 2]>, bool)> = Vec::new();
    let mut current: Vec<[f32; 2]> = Vec::new();
    let mut start = [0.0f32; 2];
    let finish = |current: &mut Vec<[f32; 2]>, out: &mut Vec<(Vec<[f32; 2]>, bool)>, closed| {
        if current.len() > 1 {
            out.push((std::mem::take(current), closed));
        } else {
            current.clear();
        }
    };
    for verb in path.verbs() {
        let from = current.last().copied().unwrap_or(start);
        match *verb {
            PathVerb::MoveTo(p) => {
                finish(&mut current, &mut out, false);
                current.push(p);
                start = p;
            }
            PathVerb::LineTo(p) => {
                if current.is_empty() {
                    current.push(from);
                }
                current.push(p);
            }
            PathVerb::QuadTo(c, p) => {
                if current.is_empty() {
                    current.push(from);
                }
                for i in 1..=16 {
                    let k = i as f32 / 16.0;
                    let m = 1.0 - k;
                    current.push([
                        m * m * from[0] + 2.0 * m * k * c[0] + k * k * p[0],
                        m * m * from[1] + 2.0 * m * k * c[1] + k * k * p[1],
                    ]);
                }
            }
            PathVerb::CubicTo(c1, c2, p) => {
                if current.is_empty() {
                    current.push(from);
                }
                for i in 1..=16 {
                    let k = i as f32 / 16.0;
                    let m = 1.0 - k;
                    let (w0, w1, w2, w3) = (m * m * m, 3.0 * m * m * k, 3.0 * m * k * k, k * k * k);
                    current.push([
                        w0 * from[0] + w1 * c1[0] + w2 * c2[0] + w3 * p[0],
                        w0 * from[1] + w1 * c1[1] + w2 * c2[1] + w3 * p[1],
                    ]);
                }
            }
            PathVerb::Close => {
                finish(&mut current, &mut out, true);
                current.push(start);
            }
        }
    }
    finish(&mut current, &mut out, false);
    out
}

fn path_contains(path: &PaintPath, t: Affine, p: [f32; 2]) -> bool {
    let Some(q) = inverse_apply(t, p) else {
        return false;
    };
    let mut winding = 0i32;
    for (points, _) in polylines(path) {
        let n = points.len();
        for i in 0..n {
            let (a, b) = (points[i], points[(i + 1) % n]);
            if (a[1] <= q[1]) != (b[1] <= q[1]) {
                let side = (b[0] - a[0]) * (q[1] - a[1]) - (b[1] - a[1]) * (q[0] - a[0]);
                if b[1] > a[1] && side > 0.0 {
                    winding += 1;
                } else if b[1] <= a[1] && side < 0.0 {
                    winding -= 1;
                }
            }
        }
    }
    match path.fill_rule() {
        FillRule::NonZero => winding != 0,
        FillRule::EvenOdd => winding % 2 != 0,
    }
}

fn stroke_contains(path: &PaintPath, stroke: &StrokeStyle, t: Affine, p: [f32; 2]) -> bool {
    let Some(q) = inverse_apply(t, p) else {
        return false;
    };
    let reach = stroke.width * 0.5;
    let pattern = dash_pattern(&stroke.dash);
    polylines(path).into_iter().any(|(mut points, closed)| {
        if closed {
            points.push(points[0]);
        }
        let pieces = match &pattern {
            Some(pattern) if !too_many_dashes(&points, pattern) => {
                dash_pieces(&points, pattern, stroke.dash_offset)
            }
            _ => vec![points],
        };
        pieces.iter().any(|piece| {
            piece
                .windows(2)
                .any(|segment| segment_distance(q, segment[0], segment[1]) <= reach)
        })
    })
}

/// The dash pattern a stroke draws with, `None` for a solid one. Mirrors the
/// scene's reading of Canvas `setLineDash`.
fn dash_pattern(dash: &[f32]) -> Option<Vec<f32>> {
    if dash.is_empty()
        || dash
            .iter()
            .any(|length| !length.is_finite() || *length < 0.0)
    {
        return None;
    }
    let mut pattern = dash.to_vec();
    if pattern.len() % 2 == 1 {
        pattern.extend_from_within(..);
    }
    (pattern.iter().sum::<f32>() > 1e-6).then_some(pattern)
}

/// Past this many dashes a stroke is drawn solid, as the scene does: the
/// pieces would be sub-pixel and walking them would stall the frame.
const MAX_DASHES: f32 = 10_000.0;

fn too_many_dashes(points: &[[f32; 2]], pattern: &[f32]) -> bool {
    let length: f32 = points
        .windows(2)
        .map(|s| ((s[1][0] - s[0][0]).powi(2) + (s[1][1] - s[0][1]).powi(2)).sqrt())
        .sum();
    let period: f32 = pattern.iter().sum();
    let dashes = length / period;
    dashes.is_nan() || dashes > MAX_DASHES
}

/// The "on" pieces of a polyline under a dash pattern.
fn dash_pieces(points: &[[f32; 2]], pattern: &[f32], offset: f32) -> Vec<Vec<[f32; 2]>> {
    let period: f32 = pattern.iter().sum();
    let mut phase = offset.rem_euclid(period);
    let mut index = 0;
    while phase >= pattern[index] {
        phase -= pattern[index];
        index = (index + 1) % pattern.len();
    }
    let mut left = pattern[index] - phase;
    let mut on = index % 2 == 0;
    let mut pieces = Vec::new();
    let mut current = if on { vec![points[0]] } else { Vec::new() };
    for segment in points.windows(2) {
        let (mut from, to) = (segment[0], segment[1]);
        let mut length = ((to[0] - from[0]).powi(2) + (to[1] - from[1]).powi(2)).sqrt();
        while length > left {
            let k = left / length;
            let cut = [
                from[0] + (to[0] - from[0]) * k,
                from[1] + (to[1] - from[1]) * k,
            ];
            if on {
                current.push(cut);
                pieces.push(std::mem::take(&mut current));
            } else {
                current = vec![cut];
            }
            on = !on;
            length -= left;
            from = cut;
            index = (index + 1) % pattern.len();
            left = pattern[index];
        }
        left -= length;
        if on {
            current.push(to);
        }
    }
    if on && current.len() >= 2 {
        pieces.push(current);
    }
    pieces
}

fn segment_distance(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let ab = [b[0] - a[0], b[1] - a[1]];
    let ap = [p[0] - a[0], p[1] - a[1]];
    let length = ab[0] * ab[0] + ab[1] * ab[1];
    let k = if length > 0.0 {
        ((ap[0] * ab[0] + ap[1] * ab[1]) / length).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let d = [ap[0] - ab[0] * k, ap[1] - ab[1] * k];
    (d[0] * d[0] + d[1] * d[1]).sqrt()
}

fn rounded_rect_contains(rect: LayoutBox, radii: [f32; 4], q: [f32; 2]) -> bool {
    let (x0, y0, x1, y1) = (rect.x, rect.y, rect.x + rect.width, rect.y + rect.height);
    if q[0] < x0 || q[1] < y0 || q[0] > x1 || q[1] > y1 {
        return false;
    }
    let [tl, tr, br, bl] = fit_radii(rect, radii);
    for (radius, cx, cy, left, top) in [
        (tl, x0 + tl, y0 + tl, true, true),
        (tr, x1 - tr, y0 + tr, false, true),
        (br, x1 - br, y1 - br, false, false),
        (bl, x0 + bl, y1 - bl, true, false),
    ] {
        let in_corner =
            (if left { q[0] < cx } else { q[0] > cx }) && (if top { q[1] < cy } else { q[1] > cy });
        if radius > 0.0 && in_corner {
            let (dx, dy) = (q[0] - cx, q[1] - cy);
            return dx * dx + dy * dy <= radius * radius;
        }
    }
    true
}

/// 录制绘制命令的上下文，相当于 `QPainter`。
///
/// 坐标原点是节点布局盒的左上角，单位逻辑像素。节点的 transform、裁剪、
/// 不透明度、层叠顺序和合成器动画由 Scene 统一套上，这里不用管。
pub struct PaintContext<'a> {
    theme: &'a CompiledTheme,
    size: [f32; 2],
    state: PaintState,
    measure: TextMeasure<'a>,
    ops: Vec<PaintOp>,
    default_drawn: bool,
    clip_depth: usize,
    layer_depth: usize,
    /// What is open, innermost last: `true` a layer, `false` a clip.
    open: Vec<bool>,
    transform: Affine,
    opacity: f32,
    blend: BlendMode,
    image_sampling: nana_ui_core::ImageSampling,
    /// The transform and opacity the ops recorded so far are under.
    recorded_transform: Affine,
    recorded_opacity: f32,
    saved: Vec<SavedState>,
}

type SavedState = (Affine, f32, BlendMode, nana_ui_core::ImageSampling, usize);

impl<'a> PaintContext<'a> {
    fn new(
        theme: &'a CompiledTheme,
        size: [f32; 2],
        state: PaintState,
        measure: TextMeasure<'a>,
        default_drawn: bool,
    ) -> Self {
        Self {
            theme,
            size,
            state,
            measure,
            ops: Vec::new(),
            default_drawn,
            clip_depth: 0,
            layer_depth: 0,
            open: Vec::new(),
            transform: AFFINE_IDENTITY,
            opacity: 1.0,
            blend: BlendMode::Normal,
            image_sampling: nana_ui_core::ImageSampling::Resample,
            recorded_transform: AFFINE_IDENTITY,
            recorded_opacity: 1.0,
            saved: Vec::new(),
        }
    }

    /// Record a state op the scene tracks, if it moved.
    fn sync_state(&mut self) {
        if self.transform != self.recorded_transform {
            self.recorded_transform = self.transform;
            self.ops.push(PaintOp::SetTransform(self.transform));
        }
        if self.opacity != self.recorded_opacity {
            self.recorded_opacity = self.opacity;
            self.ops.push(PaintOp::SetOpacity(self.opacity));
        }
    }

    /// Record a drawing op under the current transform, opacity and blend.
    fn push(&mut self, op: PaintOp) {
        self.sync_state();
        if self.blend == BlendMode::Normal {
            self.ops.push(op);
        } else {
            self.ops.push(PaintOp::PushLayer {
                opacity: 1.0,
                blend: self.blend,
            });
            self.ops.push(op);
            self.ops.push(PaintOp::PopLayer);
        }
    }

    /// 节点当前的交互状态。
    pub fn state(&self) -> PaintState {
        self.state
    }

    /// 按节点字体和当前字体集测量一段文字；`max_width` 为 `None` 时不折行。
    /// 字号缺省为主题正文字号。与 [`Self::text`] 画出来的尺寸一致。
    pub fn measure_text(&self, text: &PaintText, max_width: Option<f32>) -> TextSize {
        let size = text
            .size
            .unwrap_or_else(|| self.theme.style_model().base_text_size());
        (self.measure)(text, size, max_width)
    }

    /// 之后每条命令的不透明度（Canvas `globalAlpha`），`0..=1`。
    pub fn set_opacity(&mut self, opacity: f32) {
        if opacity.is_finite() {
            self.opacity = opacity.clamp(0.0, 1.0);
        }
    }

    pub fn opacity(&self) -> f32 {
        self.opacity
    }

    /// 之后每条命令与下层内容的混合方式；非 `Normal` 时每条命令单独成层。
    pub fn set_blend(&mut self, blend: BlendMode) {
        self.blend = blend;
    }

    /// 之后的 [`Self::image`] / [`Self::fill_image`] 怎样准备采样（类似 Canvas
    /// `imageSmoothingQuality`）。默认按实际绘制的设备像素重采样一层；
    /// 同一张图同时以多种尺寸出现、或尺寸持续变化（缩放动画）时用
    /// [`nana_ui_core::ImageSampling::Mipmap`]。
    pub fn set_image_sampling(&mut self, sampling: nana_ui_core::ImageSampling) {
        self.image_sampling = sampling;
    }

    pub fn image_sampling(&self) -> nana_ui_core::ImageSampling {
        self.image_sampling
    }

    /// 开一个图层：之后的命令先画进图层，[`Self::pop_layer`] 时按 `opacity`
    /// 和 `blend` 整体合成，重叠部分不会叠加透明度。
    pub fn push_layer(&mut self, opacity: f32, blend: BlendMode) {
        self.sync_state();
        self.layer_depth += 1;
        self.open.push(true);
        self.ops.push(PaintOp::PushLayer {
            opacity: if opacity.is_finite() {
                opacity.clamp(0.0, 1.0)
            } else {
                1.0
            },
            blend,
        });
    }

    /// 合成最近一个 [`Self::push_layer`] 打开的图层；没有打开的图层时什么都
    /// 不做。
    pub fn pop_layer(&mut self) {
        if let Some(at) = self.open.iter().rposition(|layer| *layer) {
            self.open.remove(at);
            self.layer_depth -= 1;
            self.ops.push(PaintOp::PopLayer);
        }
    }

    /// 当前局部变换。
    pub fn transform(&self) -> Affine {
        self.transform
    }

    /// 替换当前局部变换（Canvas `setTransform`）。
    pub fn set_transform(&mut self, transform: Affine) {
        if transform.iter().all(|value| value.is_finite()) {
            self.transform = transform;
        }
    }

    /// 在当前变换之后再叠一层（Canvas `transform`）：之后的坐标先经 `transform`。
    pub fn concat(&mut self, transform: Affine) {
        self.set_transform(affine_multiply(self.transform, transform));
    }

    pub fn translate(&mut self, x: f32, y: f32) {
        self.concat([1.0, 0.0, 0.0, 1.0, x, y]);
    }

    pub fn scale(&mut self, x: f32, y: f32) {
        self.concat([x, 0.0, 0.0, y, 0.0, 0.0]);
    }

    /// 旋转 `angle` 弧度，y 轴向下时正值顺时针。
    pub fn rotate(&mut self, angle: f32) {
        let (sin, cos) = angle.sin_cos();
        self.concat([cos, sin, -sin, cos, 0.0, 0.0]);
    }

    /// 保存当前变换、不透明度、混合方式、图片采样方式和裁剪层数，与
    /// [`Self::restore`] 配对。图层另由 `push_layer` / `pop_layer` 管。
    pub fn save(&mut self) {
        self.saved.push((
            self.transform,
            self.opacity,
            self.blend,
            self.image_sampling,
            self.clip_depth,
        ));
    }

    /// 恢复最近一次 [`Self::save`] 时的状态，并像 Canvas 的 `restore()` 一样
    /// 弹出其后压入的裁剪；没有可恢复的就什么都不做。
    pub fn restore(&mut self) {
        if let Some((transform, opacity, blend, image_sampling, clip_depth)) = self.saved.pop() {
            self.transform = transform;
            self.opacity = opacity;
            self.blend = blend;
            self.image_sampling = image_sampling;
            while self.clip_depth > clip_depth {
                self.pop_clip();
            }
        }
    }

    /// 按当前主题解析上色。
    pub fn paint(&self, paint: impl Into<Paint>) -> ResolvedPaint {
        match paint.into() {
            Paint::Color(color) => ResolvedPaint::Solid(self.color(color)),
            Paint::Gradient(gradient) => {
                let mut stops: Vec<(f32, [f32; 4])> = gradient
                    .stops
                    .iter()
                    .filter(|stop| stop.offset.is_finite())
                    .map(|stop| (stop.offset.clamp(0.0, 1.0), self.color(stop.color)))
                    .collect();
                // Stable, so equal offsets keep their order: a hard edge.
                stops.sort_by(|a, b| a.0.total_cmp(&b.0));
                if stops.is_empty() {
                    stops.push((0.0, [0.0; 4]));
                }
                ResolvedPaint::Gradient(Arc::new(ResolvedGradient {
                    shape: gradient.shape,
                    stops,
                    extend: gradient.extend,
                }))
            }
        }
    }

    /// 节点布局尺寸 `[宽, 高]`。
    pub fn size(&self) -> [f32; 2] {
        self.size
    }

    /// 节点自身的矩形，原点为 `(0, 0)`。
    pub fn bounds(&self) -> LayoutBox {
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: self.size[0],
            height: self.size[1],
        }
    }

    pub fn theme_mode(&self) -> ThemeMode {
        self.theme.style_model().theme_mode
    }

    pub fn metrics(&self) -> ThemeMetrics {
        self.theme.metrics()
    }

    /// 按当前主题解析颜色。
    pub fn color(&self, color: impl Into<PaintColor>) -> [f32; 4] {
        let model = self.theme.style_model();
        match color.into() {
            PaintColor::Role(role) => model.color(role).as_rgba_array(),
            PaintColor::Mix(mix) => mix.resolve(model).as_rgba_array(),
            PaintColor::Rgba(rgba) => rgba,
        }
    }

    /// 按当前主题解析圆角档位。
    pub fn radius(&self, radius: impl Into<Radius>) -> f32 {
        match radius.into() {
            Radius::Px(px) => px,
            Radius::Tier(tier) => tier.resolve(self.theme.metrics()),
        }
    }

    /// 按当前主题解析阴影。
    pub fn elevation(&self, shadow: impl Into<PaintShadow>) -> ComponentElevation {
        match shadow.into() {
            PaintShadow::Elevation(role) => {
                ComponentElevation::from_shadow(self.theme.shadow(role))
            }
            PaintShadow::Custom {
                color,
                offset,
                blur,
                spread,
                inset,
            } => ComponentElevation {
                color: self.color(color),
                offset_x: offset[0],
                offset_y: offset[1],
                blur_radius: blur.max(0.0),
                spread_radius: spread,
                inset,
            },
        }
    }

    /// 画出该节点原本的内建外观，相当于在重写里调用基类的 `paintEvent`。
    /// 每个节点只生效一次。
    pub fn draw_default(&mut self) {
        if !self.default_drawn {
            self.default_drawn = true;
            self.push(PaintOp::DrawDefault);
        }
    }

    /// 填充路径，纯色或渐变。
    pub fn fill_path(&mut self, path: &PaintPath, paint: impl Into<Paint>) {
        if path.is_empty() {
            return;
        }
        let paint = self.paint(paint);
        self.push(PaintOp::FillPath {
            path: Arc::new(path.clone()),
            paint,
        });
    }

    /// 描边路径，纯色或渐变；虚线见 [`StrokeStyle::dash`]。
    pub fn stroke_path(
        &mut self,
        path: &PaintPath,
        stroke: impl Into<StrokeStyle>,
        paint: impl Into<Paint>,
    ) {
        let stroke = stroke.into();
        if path.is_empty() || stroke.width.is_nan() || stroke.width <= 0.0 {
            return;
        }
        let paint = self.paint(paint);
        self.push(PaintOp::StrokePath {
            path: Arc::new(path.clone()),
            paint,
            stroke,
        });
    }

    /// 沿路径轮廓的投影（CSS `box-shadow` 语义）。外阴影画在路径以外，
    /// `inset` 阴影画在路径以内。
    pub fn shadow(&mut self, path: &PaintPath, shadow: impl Into<PaintShadow>) {
        if path.is_empty() {
            return;
        }
        let shadow = self.elevation(shadow);
        self.push(PaintOp::Shadow {
            path: Arc::new(path.clone()),
            shadow,
        });
    }

    /// 四角独立的圆角矩形。
    pub fn rounded_rect(
        &mut self,
        rect: LayoutBox,
        radii: impl Into<CornerRadii>,
        paint: BoxPaint,
    ) {
        let CornerRadii(radii) = radii.into();
        let radii = fit_radii(rect, radii.map(|radius| self.radius(radius)));
        let fill = paint.fill.map(|fill| self.paint(fill));
        let border = paint
            .border
            .filter(|(_, width)| *width > 0.0)
            .map(|(color, width)| (self.color(color), width));
        let shadow = paint.shadow.map(|shadow| self.elevation(shadow));
        if fill.is_none() && border.is_none() && shadow.is_none() {
            return;
        }
        self.push(PaintOp::RoundedRect {
            rect,
            radii,
            fill,
            border,
            shadow,
        });
    }

    /// 一段文字，排在 `rect` 里。纯色或渐变；`wrap` / `max_lines` 控制折行。
    pub fn text(&mut self, rect: LayoutBox, text: PaintText) {
        if text.content.is_empty() {
            return;
        }
        let paint = self.paint(text.paint.clone());
        let size = text
            .size
            .unwrap_or_else(|| self.theme.style_model().base_text_size());
        self.push(PaintOp::Text {
            rect,
            content: text.content,
            paint,
            size,
            weight: text.weight,
            italic: text.italic,
            line_height: text.line_height,
            wrap: text.wrap,
            max_lines: text.max_lines,
            horizontal: text.horizontal,
            vertical: text.vertical,
        });
    }

    /// 一个图标，纯色或渐变。
    pub fn icon(&mut self, rect: LayoutBox, icon: Icon, paint: impl Into<Paint>) {
        let paint = self.paint(paint);
        self.push(PaintOp::Icon { rect, icon, paint });
    }

    /// 用图片填充路径：图片按 `fit` 摆进 `rect`，只露出 `path` 以内的部分。
    pub fn fill_path_with_image(
        &mut self,
        path: &PaintPath,
        rect: LayoutBox,
        source: impl Into<Arc<str>>,
        fit: ImageFit,
    ) {
        let source = source.into();
        if path.is_empty() || source.is_empty() {
            return;
        }
        self.push(PaintOp::FillImage {
            path: Arc::new(path.clone()),
            rect,
            source,
            fit,
            sampling: self.image_sampling,
        });
    }

    /// 一张图片，按 `fit` 摆进 `rect`，裁到四角圆角以内。`source` 与 CSS
    /// `url()` 同源：`data:`、文件路径或经宿主 fetch 的 URL，异步加载，加载完成前
    /// 不画。
    pub fn image(
        &mut self,
        rect: LayoutBox,
        source: impl Into<Arc<str>>,
        fit: ImageFit,
        radii: impl Into<CornerRadii>,
    ) {
        let source = source.into();
        if source.is_empty() {
            return;
        }
        let CornerRadii(radii) = radii.into();
        let radii = fit_radii(rect, radii.map(|radius| self.radius(radius)));
        self.push(PaintOp::Image {
            rect,
            source,
            fit,
            sampling: self.image_sampling,
            radii,
        });
    }

    /// 之后的绘制裁剪到 `path` 以内（与已有裁剪取交集），直到 [`Self::pop_clip`]。
    ///
    /// 只作用于这个 painter 录下的内容：节点自己的阴影若画在 `push_clip` 之前，
    /// 不会被裁掉；子节点也不受影响。
    pub fn push_clip(&mut self, path: &PaintPath) {
        self.clip_depth += 1;
        self.open.push(false);
        self.sync_state();
        self.ops.push(PaintOp::PushClip {
            path: Arc::new(path.clone()),
        });
    }

    /// 撤掉最近一个 [`Self::push_clip`]；没有裁剪时什么都不做。
    pub fn pop_clip(&mut self) {
        if let Some(at) = self.open.iter().rposition(|layer| !*layer) {
            self.open.remove(at);
            self.clip_depth -= 1;
            self.ops.push(PaintOp::PopClip);
        }
    }

    fn finish(mut self) -> (Vec<PaintOp>, bool) {
        // What was left open closes innermost first.
        while let Some(layer) = self.open.pop() {
            self.ops.push(if layer {
                PaintOp::PopLayer
            } else {
                PaintOp::PopClip
            });
        }
        (self.ops, self.default_drawn)
    }
}

/// What a painter's text is shaped as: the node's resolved style with the
/// text's own size, weight, slant and line height.
pub(crate) fn paint_text_request(
    base: &crate::ComputedStyle,
    text: &PaintText,
    size: f32,
    max_width: Option<f32>,
) -> (
    crate::ComputedStyle,
    crate::TextShapeConstraints,
    crate::TextContent,
) {
    let mut style = base.clone();
    style.font_size = size;
    style.font_weight = text.weight.or(style.font_weight);
    style.italic = text.italic;
    style.letter_spacing = 0.0;
    style.line_height = text.line_height.map(nana_ui_core::LineHeightSpec::Absolute);
    let constraints = crate::TextShapeConstraints {
        max_width,
        wrap: text.wrap && max_width.is_some(),
        max_lines: text.max_lines,
        ..crate::TextShapeConstraints::default()
    };
    let content = crate::TextContent {
        value: text.content.to_string().into(),
    };
    (style, constraints, content)
}

pub(crate) fn paint_text_size(metrics: &crate::TextMetrics, size: f32) -> TextSize {
    TextSize {
        width: metrics.width,
        height: metrics.height,
        baseline: metrics
            .ascent
            .unwrap_or(size * nana_ui_core::TEXT_APPROX_ASCENT_EM),
    }
}

impl PaintRecording {
    /// 在给定主题、尺寸和交互状态下录一次 `painter`，文字按 em 估算测量。
    ///
    /// 用于在没有 `UiWorld` 的地方检查一个 painter 录下了什么，比如它的单元
    /// 测试；挂在节点上时由框架录制，文字用宿主的排版引擎测量。
    pub fn record(
        painter: &dyn Painter,
        theme: &CompiledTheme,
        size: [f32; 2],
        state: PaintState,
    ) -> Self {
        let base = crate::ComputedStyle::default();
        let measure = |text: &PaintText, font: f32, max_width: Option<f32>| {
            let (style, constraints, content) = paint_text_request(&base, text, font, max_width);
            paint_text_size(
                &crate::components::measure_em_text(&content, &style, constraints),
                font,
            )
        };
        record(painter, theme, size, state, &measure)
    }
}

/// 录一次：先背景阶段，再前景阶段。
pub(crate) fn record(
    painter: &dyn Painter,
    theme: &CompiledTheme,
    size: [f32; 2],
    state: PaintState,
    measure: TextMeasure<'_>,
) -> PaintRecording {
    let mut behind = PaintContext::new(theme, size, state, measure, false);
    painter.paint(&mut behind);
    let (behind_children, default_drawn) = behind.finish();
    let mut over = PaintContext::new(theme, size, state, measure, default_drawn);
    painter.paint_over_children(&mut over);
    let (over_children, _) = over.finish();
    PaintRecording {
        behind_children,
        over_children,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DocumentId, MutationQueue, NodeKind, NodeStyle, StableNodeId, UiWorld};
    use nana_ui_core::{SemanticPalette, ThemeDefinition, UI_METRICS};
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Fills its box with a role and a tier, and counts how often it is asked.
    struct Counting {
        key: u64,
        role: SemanticColorRole,
        calls: Arc<AtomicUsize>,
    }

    impl Painter for Counting {
        fn paint(&self, cx: &mut PaintContext<'_>) {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let radius = cx.radius(RadiusTier::Md);
            let mut path = PaintPath::new();
            path.rounded_rect(cx.bounds(), [radius; 4]);
            cx.fill_path(&path, self.role);
        }

        fn paint_key(&self) -> u64 {
            self.key
        }
    }

    fn no_measure(_: &PaintText, _: f32, _: Option<f32>) -> TextSize {
        TextSize::default()
    }

    fn record_plain(painter: &dyn Painter, size: [f32; 2]) -> PaintRecording {
        record(
            painter,
            nana_ui_core::builtin_theme_arc(ThemeMode::Dark).as_ref(),
            size,
            PaintState::default(),
            &no_measure,
        )
    }

    fn node() -> StableNodeId {
        StableNodeId::new(1).unwrap()
    }

    fn painted_world(painter: Counting) -> UiWorld {
        painted_world_with(painter)
    }

    fn painted_world_with(painter: impl Painter) -> UiWorld {
        let mut world = UiWorld::new();
        let mut queue = MutationQueue::new();
        queue.create(
            node(),
            DocumentId::new(1).unwrap(),
            NodeKind::Element { tag: "div".into() },
        );
        queue.set_style(node(), NodeStyle::default().painter(painter));
        queue.write_layout(
            node(),
            LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 40.0,
            },
        );
        world.commit(queue).unwrap();
        let work = world.take_system_work();
        world.resolve_styles(&work.style).unwrap();
        world
    }

    fn filled(world: &UiWorld) -> ([f32; 4], Arc<PaintPath>) {
        let extracted = world.extract_nodes(&[node()]);
        let recording = extracted[0]
            .custom_paint
            .clone()
            .expect("a painted node carries its recording");
        match &recording.behind_children[..] {
            [
                PaintOp::FillPath {
                    path,
                    paint: ResolvedPaint::Solid(color),
                },
            ] => (*color, Arc::clone(path)),
            other => panic!("unexpected recording {other:?}"),
        }
    }

    fn commit(world: &mut UiWorld, queue: MutationQueue) -> crate::SystemWork {
        world.commit(queue).unwrap();
        let work = world.take_system_work();
        world.resolve_styles(&work.style).unwrap();
        work
    }

    #[test]
    fn a_path_answers_what_it_covers_and_moves_and_parses_from_svg() {
        let path = PaintPath::from_svg("M0 0 H20 V10 H0 Z").unwrap();
        assert!(path.contains([10.0, 5.0]) && !path.contains([25.0, 5.0]));
        let stroke = StrokeStyle::new(2.0);
        assert!(path.stroke_contains([20.5, 5.0], &stroke));
        assert!(!path.stroke_contains([10.0, 5.0], &stroke));
        let moved = path.transformed([2.0, 0.0, 0.0, 1.0, 5.0, 0.0]);
        let bounds = moved.bounds().unwrap();
        assert_eq!(
            (bounds.x, bounds.y, bounds.width, bounds.height),
            (5.0, 0.0, 40.0, 10.0)
        );
        assert!(PaintPath::from_svg("M0 0 L").is_err());
        assert_eq!(PaintPath::new().bounds(), None);
    }

    #[test]
    fn a_painter_records_on_its_own_and_what_it_leaves_open_closes_innermost_first() {
        struct Open;
        impl Painter for Open {
            fn paint(&self, cx: &mut PaintContext<'_>) {
                cx.push_layer(0.5, BlendMode::Normal);
                cx.push_clip(&PaintPath::from_svg("M0 0 H10 V10 Z").unwrap());
                cx.push_layer(1.0, BlendMode::Normal);
            }
            fn paint_key(&self) -> u64 {
                0
            }
        }
        let theme = nana_ui_core::builtin_theme_arc(ThemeMode::Dark);
        let recording = PaintRecording::record(&Open, &theme, [10.0, 10.0], PaintState::default());
        let closes: Vec<_> = recording.behind_children[3..]
            .iter()
            .map(|op| matches!(op, PaintOp::PopLayer))
            .collect();
        assert_eq!(closes, [true, false, true]);
    }

    #[test]
    fn an_unchanged_node_is_never_re_recorded() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut world = painted_world(Counting {
            key: 1,
            role: SemanticColorRole::Surface,
            calls: Arc::clone(&calls),
        });
        let (color, _) = filled(&world);
        assert_eq!(color, SemanticPalette::dark().surface.as_rgba_array());
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        world.extract_nodes(&[node()]);
        assert_eq!(calls.load(Ordering::SeqCst), 1, "extraction alone");

        // A view rebuilt every frame hands in a fresh painter with the same key.
        let mut queue = MutationQueue::new();
        queue.set_style(
            node(),
            NodeStyle::default().painter(Counting {
                key: 1,
                role: SemanticColorRole::Surface,
                calls: Arc::clone(&calls),
            }),
        );
        let work = commit(&mut world, queue);
        assert!(
            work.render_extraction.is_empty(),
            "same key is the same style"
        );
        world.extract_nodes(&[node()]);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_new_key_re_records_without_relayout() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut world = painted_world(Counting {
            key: 1,
            role: SemanticColorRole::Surface,
            calls: Arc::clone(&calls),
        });
        filled(&world);
        let mut queue = MutationQueue::new();
        queue.set_style(
            node(),
            NodeStyle::default().painter(Counting {
                key: 2,
                role: SemanticColorRole::Accent,
                calls: Arc::clone(&calls),
            }),
        );
        let work = commit(&mut world, queue);
        assert!(work.layout.is_empty(), "a colour role is paint, not layout");
        assert_eq!(work.render_extraction, vec![node()]);
        let (color, _) = filled(&world);
        assert_eq!(color, SemanticPalette::dark().accent.as_rgba_array());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_resize_re_records_at_the_new_size() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut world = painted_world(Counting {
            key: 1,
            role: SemanticColorRole::Surface,
            calls: Arc::clone(&calls),
        });
        filled(&world);
        let mut queue = MutationQueue::new();
        queue.write_layout(
            node(),
            LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 160.0,
                height: 40.0,
            },
        );
        commit(&mut world, queue);
        let (_, path) = filled(&world);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert!(
            path.verbs()
                .iter()
                .any(|verb| matches!(verb, PathVerb::LineTo([x, _]) if (*x - 160.0).abs() < 1e-3)),
            "the outline reaches the new right edge"
        );
    }

    #[test]
    fn a_theme_switch_follows_the_new_palette_and_radius_tier() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut world = painted_world(Counting {
            key: 1,
            role: SemanticColorRole::Surface,
            calls: Arc::clone(&calls),
        });
        let (_, dark_path) = filled(&world);

        let mut metrics = UI_METRICS;
        metrics.radius_md += 6.0;
        let theme = ThemeDefinition::for_mode(ThemeMode::Light)
            .with_metrics(metrics)
            .with_palette(SemanticPalette::light())
            .compile()
            .expect("a test theme compiles");
        let mut queue = MutationQueue::new();
        queue.set_theme_tokens(Arc::new(theme));
        commit(&mut world, queue);

        let (color, light_path) = filled(&world);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(color, SemanticPalette::light().surface.as_rgba_array());
        // The first straight edge starts where the top-left corner ends.
        let start = |path: &PaintPath| match path.verbs()[0] {
            PathVerb::MoveTo([x, _]) => x,
            other => panic!("unexpected {other:?}"),
        };
        assert_eq!(start(&dark_path), UI_METRICS.radius_md);
        assert_eq!(start(&light_path), metrics.radius_md);
    }

    #[test]
    fn draw_default_is_recorded_once_and_clips_are_balanced() {
        struct Layered;
        impl Painter for Layered {
            fn paint(&self, cx: &mut PaintContext<'_>) {
                let mut clip = PaintPath::new();
                clip.rect(cx.bounds());
                cx.push_clip(&clip);
                cx.draw_default();
                cx.draw_default();
            }
            fn paint_over_children(&self, cx: &mut PaintContext<'_>) {
                cx.draw_default();
                cx.pop_clip();
            }
            fn paint_key(&self) -> u64 {
                0
            }
        }
        let recording = record_plain(&Layered, [10.0, 10.0]);
        assert_eq!(recording.default_phase(), Some(PaintPhase::BehindChildren));
        assert!(matches!(
            &recording.behind_children[..],
            [
                PaintOp::PushClip { .. },
                PaintOp::DrawDefault,
                PaintOp::PopClip
            ]
        ));
        assert!(recording.over_children.is_empty());
    }

    #[test]
    fn arc_to_rounds_a_concave_corner_tangent_to_both_edges() {
        // Down the left edge of a cut-out, then along its floor to the right:
        // the corner at (0, 20) turns the other way from the outline's
        // convex corners.
        let mut path = PaintPath::new();
        path.move_to(0.0, 0.0).arc_to(0.0, 20.0, 40.0, 20.0, 8.0);
        let verbs = path.verbs();
        assert_eq!(verbs[1], PathVerb::LineTo([0.0, 12.0]));
        let PathVerb::CubicTo(c1, c2, end) = verbs[2] else {
            panic!("an arc follows the tangent point");
        };
        assert!((end[0] - 8.0).abs() < 1e-4 && (end[1] - 20.0).abs() < 1e-4);
        // Tangent continuity: the handles leave along the incoming edge and
        // arrive along the outgoing one.
        assert!(c1[0].abs() < 1e-4 && c1[1] > 12.0);
        assert!((c2[1] - 20.0).abs() < 1e-4 && c2[0] < 8.0);
    }

    #[test]
    fn a_path_that_is_one_rounded_rect_says_so() {
        let rect = LayoutBox {
            x: 1.0,
            y: 2.0,
            width: 30.0,
            height: 20.0,
        };
        let mut path = PaintPath::new();
        path.rounded_rect(rect, [4.0; 4]);
        assert_eq!(path.as_rounded_rect(), Some((rect, 4.0)));
        path.line_to(0.0, 0.0);
        assert_eq!(path.as_rounded_rect(), None);
    }

    #[test]
    fn the_transform_stack_is_recorded_ahead_of_the_ops_it_governs() {
        struct Turned;
        impl Painter for Turned {
            fn paint(&self, cx: &mut PaintContext<'_>) {
                let mut dot = PaintPath::new();
                dot.rect(LayoutBox {
                    x: 0.0,
                    y: 0.0,
                    width: 1.0,
                    height: 1.0,
                });
                cx.save();
                cx.translate(10.0, 20.0);
                cx.scale(2.0, 2.0);
                cx.translate(1.0, 0.0);
                cx.fill_path(&dot, [1.0; 4]);
                cx.fill_path(&dot, [1.0; 4]);
                cx.restore();
                cx.fill_path(&dot, [1.0; 4]);
                // Changing the transform and drawing nothing records nothing.
                cx.rotate(1.0);
            }
            fn paint_key(&self) -> u64 {
                0
            }
        }
        let recording = record_plain(&Turned, [10.0, 10.0]);
        let ops = &recording.behind_children;
        assert!(
            matches!(
                ops[..],
                [
                    PaintOp::SetTransform([2.0, 0.0, 0.0, 2.0, 12.0, 20.0]),
                    PaintOp::FillPath { .. },
                    PaintOp::FillPath { .. },
                    PaintOp::SetTransform(AFFINE_IDENTITY),
                    PaintOp::FillPath { .. },
                ]
            ),
            "{ops:?}"
        );
    }

    #[test]
    fn a_gradient_resolves_its_roles_and_sorts_its_stops() {
        let theme = nana_ui_core::builtin_theme_arc(ThemeMode::Dark);
        let measure = no_measure;
        let cx = PaintContext::new(
            theme.as_ref(),
            [10.0, 10.0],
            PaintState::default(),
            &measure,
            false,
        );
        let ResolvedPaint::Gradient(gradient) = cx.paint(
            Gradient::linear([0.0, 0.0], [10.0, 0.0])
                .stop(1.4, SemanticColorRole::Accent)
                .stop(0.25, [1.0, 0.0, 0.0, 1.0])
                .stop(f32::NAN, [0.0; 4])
                .extend(GradientExtend::Reflect),
        ) else {
            panic!("a gradient");
        };
        assert_eq!(
            gradient.stops,
            vec![
                (0.25, [1.0, 0.0, 0.0, 1.0]),
                (1.0, SemanticPalette::dark().accent.as_rgba_array()),
            ]
        );
        assert_eq!(gradient.extend, GradientExtend::Reflect);
    }

    /// Records the state it was painted in.
    struct Stateful(Arc<std::sync::Mutex<Vec<PaintState>>>);

    impl Painter for Stateful {
        fn paint(&self, cx: &mut PaintContext<'_>) {
            self.0.lock().unwrap().push(cx.state());
        }
        fn paint_key(&self) -> u64 {
            0
        }
    }

    #[test]
    fn a_state_change_re_records_with_the_new_state() {
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut world = UiWorld::new();
        let mut queue = MutationQueue::new();
        let document = DocumentId::new(1).unwrap();
        queue.create(node(), document, NodeKind::Element { tag: "div".into() });
        queue.set_style(
            node(),
            NodeStyle::default().painter(Stateful(Arc::clone(&seen))),
        );
        queue.set_interaction(
            node(),
            crate::InteractionState {
                pointer_events: true,
                focusable: false,
            },
        );
        queue.write_layout(
            node(),
            LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            },
        );
        commit(&mut world, queue);
        let recording = |world: &UiWorld| {
            world.extract_nodes(&[node()])[0]
                .custom_paint
                .clone()
                .expect("painted")
        };
        let before = recording(&world);
        world.set_pointer_hover(document, 1, Some(node())).unwrap();
        let work = world.take_system_work();
        assert!(
            work.render_extraction.contains(&node()),
            "hover repaints a painter"
        );
        let after = recording(&world);
        world.extract_nodes(&[node()]);
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 2, "once per state, not per extraction");
        assert!(!seen[0].hovered && seen[1].hovered, "{seen:?}");
        // It paints the same whatever the state: the scene keeps its
        // triangles, which it reuses by identity.
        assert!(Arc::ptr_eq(&before, &after));
    }

    #[test]
    fn a_painter_that_measured_text_re_records_on_a_font_change_only() {
        struct Measures(Arc<AtomicUsize>);
        impl Painter for Measures {
            fn paint(&self, cx: &mut PaintContext<'_>) {
                self.0.fetch_add(1, Ordering::SeqCst);
                cx.measure_text(&PaintText::new("label"), None);
            }
            fn paint_key(&self) -> u64 {
                0
            }
        }
        let calls = Arc::new(AtomicUsize::new(0));
        let mut world = painted_world_with(Measures(Arc::clone(&calls)));
        world.extract_nodes(&[node()]);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let restyle = |world: &mut UiWorld, edit: &dyn Fn(&mut nana_ui_core::LayoutStyle)| {
            let mut style = NodeStyle::default().painter(Measures(Arc::clone(&calls)));
            edit(Arc::make_mut(&mut style.layout));
            let mut queue = MutationQueue::new();
            queue.set_style(node(), style);
            commit(world, queue);
            world.extract_nodes(&[node()]);
        };
        // A new resolved style that measures the same: no re-record.
        restyle(&mut world, &|layout| {
            layout.color = Some([1.0, 0.0, 0.0, 1.0])
        });
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        // Another family measures differently.
        restyle(&mut world, &|layout| {
            layout.color = Some([1.0, 0.0, 0.0, 1.0]);
            layout.font_family = Some("Serif".into());
        });
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn text_is_measured_and_wraps_when_narrowed() {
        struct Measures(Arc<std::sync::Mutex<Vec<TextSize>>>);
        impl Painter for Measures {
            fn paint(&self, cx: &mut PaintContext<'_>) {
                let text = PaintText::new("a label long enough to wrap")
                    .size(12.0)
                    .wrap(true);
                let mut sizes = self.0.lock().unwrap();
                sizes.push(cx.measure_text(&text, None));
                sizes.push(cx.measure_text(&text, Some(40.0)));
            }
            fn paint_key(&self) -> u64 {
                0
            }
        }
        let sizes = Arc::new(std::sync::Mutex::new(Vec::new()));
        let world = painted_world_with(Measures(Arc::clone(&sizes)));
        world.extract_nodes(&[node()]);
        let sizes = sizes.lock().unwrap();
        let (line, wrapped) = (sizes[0], sizes[1]);
        assert!(line.width > 40.0 && line.height > 0.0, "{line:?}");
        assert!(wrapped.width <= 40.0 + 1e-3, "{wrapped:?}");
        assert!(
            wrapped.height > line.height * 1.5,
            "{wrapped:?} vs {line:?}"
        );
        assert!(line.baseline > 0.0 && line.baseline < line.height);
    }

    #[test]
    fn opacity_layers_and_blends_are_recorded_around_the_ops_they_govern() {
        struct Layered;
        impl Painter for Layered {
            fn paint(&self, cx: &mut PaintContext<'_>) {
                let mut dot = PaintPath::new();
                dot.rect(cx.bounds());
                cx.set_opacity(0.5);
                cx.fill_path(&dot, [1.0; 4]);
                cx.push_layer(0.25, BlendMode::Normal);
                cx.set_blend(BlendMode::Multiply);
                cx.fill_path(&dot, [1.0; 4]);
                // An unbalanced layer is closed at the end of the phase.
            }
            fn paint_key(&self) -> u64 {
                0
            }
        }
        let ops = record_plain(&Layered, [10.0, 10.0]).behind_children;
        assert!(
            matches!(
                ops[..],
                [
                    PaintOp::SetOpacity(0.5),
                    PaintOp::FillPath { .. },
                    PaintOp::PushLayer {
                        opacity: 0.25,
                        blend: BlendMode::Normal
                    },
                    PaintOp::PushLayer {
                        opacity: 1.0,
                        blend: BlendMode::Multiply
                    },
                    PaintOp::FillPath { .. },
                    PaintOp::PopLayer,
                    PaintOp::PopLayer,
                ]
            ),
            "{ops:?}"
        );
    }

    #[test]
    fn a_painter_decides_which_points_of_its_box_hit() {
        /// Hits only in a disc, and paints only its left half.
        struct Disc {
            painted: bool,
        }
        impl Painter for Disc {
            fn paint(&self, cx: &mut PaintContext<'_>) {
                let mut half = PaintPath::new();
                half.rect(LayoutBox {
                    x: 0.0,
                    y: 0.0,
                    width: 50.0,
                    height: 100.0,
                });
                cx.fill_path(&half, [1.0; 4]);
            }
            fn hit_test(&self, local: [f32; 2], size: [f32; 2]) -> Option<bool> {
                (!self.painted).then(|| {
                    let (dx, dy) = (local[0] - size[0] / 2.0, local[1] - size[1] / 2.0);
                    dx * dx + dy * dy <= 50.0 * 50.0
                })
            }
            fn hit_painted_outline(&self) -> bool {
                self.painted
            }
            fn paint_key(&self) -> u64 {
                u64::from(self.painted)
            }
        }
        for painted in [false, true] {
            let mut world = UiWorld::new();
            let document = DocumentId::new(1).unwrap();
            let mut queue = MutationQueue::new();
            queue.create(node(), document, NodeKind::Element { tag: "div".into() });
            queue.set_style(node(), NodeStyle::default().painter(Disc { painted }));
            queue.set_interaction(
                node(),
                crate::InteractionState {
                    pointer_events: true,
                    focusable: false,
                },
            );
            queue.write_layout(
                node(),
                LayoutBox {
                    x: 10.0,
                    y: 10.0,
                    width: 100.0,
                    height: 100.0,
                },
            );
            commit(&mut world, queue);
            world.rebuild_hit_test(document);
            let hit = |x, y| world.hit_test(document, x, y) == Some(node());
            if painted {
                assert!(
                    hit(30.0, 100.0) && !hit(90.0, 30.0),
                    "only the painted half"
                );
            } else {
                assert!(hit(60.0, 60.0), "the disc's centre");
                assert!(!hit(13.0, 13.0), "a corner of the box, outside the disc");
            }
        }
    }

    #[test]
    fn a_painted_outline_hit_follows_a_re_record_without_an_index_rebuild() {
        /// Paints its right half only while selected.
        struct Grows;
        impl Painter for Grows {
            fn paint(&self, cx: &mut PaintContext<'_>) {
                let mut half = PaintPath::new();
                half.rect(LayoutBox {
                    x: 0.0,
                    y: 0.0,
                    width: if cx.state().selected { 100.0 } else { 50.0 },
                    height: 100.0,
                });
                cx.fill_path(&half, [1.0; 4]);
            }
            fn hit_painted_outline(&self) -> bool {
                true
            }
            fn paint_key(&self) -> u64 {
                0
            }
        }
        let mut world = UiWorld::new();
        let document = DocumentId::new(1).unwrap();
        let mut queue = MutationQueue::new();
        queue.create(node(), document, NodeKind::Element { tag: "div".into() });
        queue.set_style(node(), NodeStyle::default().painter(Grows));
        queue.set_interaction(
            node(),
            crate::InteractionState {
                pointer_events: true,
                focusable: false,
            },
        );
        queue.write_layout(
            node(),
            LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
            },
        );
        commit(&mut world, queue);
        world.rebuild_hit_test(document);
        assert_ne!(world.hit_test(document, 80.0, 50.0), Some(node()));
        let mut queue = MutationQueue::new();
        queue.set_accessibility(
            node(),
            crate::AccessibilityState {
                selected: Some(true),
                ..Default::default()
            },
        );
        commit(&mut world, queue);
        // The frame extracts (and re-records) the node; the hit index keeps
        // the entry it built, which reads the recording through its slot.
        world.extract_nodes(&[node()]);
        assert_eq!(world.hit_test(document, 80.0, 50.0), Some(node()));
    }

    #[test]
    fn a_dashed_stroke_hits_only_on_its_dashes() {
        let mut line = PaintPath::new();
        line.move_to(0.0, 5.0).line_to(40.0, 5.0);
        let recording = PaintRecording {
            behind_children: vec![PaintOp::StrokePath {
                path: Arc::new(line),
                paint: ResolvedPaint::Solid([1.0; 4]),
                stroke: StrokeStyle::new(4.0).dash(vec![10.0, 10.0], 0.0),
            }],
            over_children: Vec::new(),
        };
        assert!(recording.contains([5.0, 5.0], [40.0, 10.0]));
        assert!(!recording.contains([15.0, 5.0], [40.0, 10.0]), "a gap");
        assert!(recording.contains([25.0, 6.5], [40.0, 10.0]));
    }

    #[test]
    fn a_painter_set_on_the_node_outlives_style_rewrites() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut world = painted_world(Counting {
            key: 1,
            role: SemanticColorRole::Surface,
            calls: Arc::clone(&calls),
        });
        let mut queue = MutationQueue::new();
        queue.set_painter(
            node(),
            Some(NodePainter::new(Counting {
                key: 7,
                role: SemanticColorRole::Accent,
                calls: Arc::clone(&calls),
            })),
        );
        commit(&mut world, queue);
        let (color, _) = filled(&world);
        assert_eq!(
            color,
            SemanticPalette::dark().accent.as_rgba_array(),
            "the node's painter wins"
        );
        // A component projecting a style without a painter leaves it on.
        let mut queue = MutationQueue::new();
        queue.set_style(node(), NodeStyle::default());
        commit(&mut world, queue);
        let (color, _) = filled(&world);
        assert_eq!(color, SemanticPalette::dark().accent.as_rgba_array());
        let mut queue = MutationQueue::new();
        queue.set_painter(node(), None);
        let work = commit(&mut world, queue);
        assert_eq!(work.render_extraction, vec![node()]);
        assert!(world.extract_nodes(&[node()])[0].custom_paint.is_none());
    }
}
