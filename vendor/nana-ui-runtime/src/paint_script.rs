//! 数据驱动的节点自绘（Issue #217）：把一份 JSON 命令列表当作 [`Painter`]。
//!
//! 给拿不到 Rust trait 的消费方用，比如 Vue / JS：命令与 [`PaintContext`] 的方法
//! 一一对应，录制时按节点当前尺寸和交互状态回放。格式见
//! [`PaintScript::from_json`]。

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use nana_ui_core::{ElevationRole, RadiusTier, SemanticColorMix, SemanticColorRole};
use serde_json::Value;

use crate::{
    BlendMode, BoxPaint, ColorStop, CornerRadii, FillRule, Gradient, GradientExtend, ImageFit,
    LayoutBox, LineCap, LineJoin, Paint, PaintColor, PaintContext, PaintPath, PaintPhase,
    PaintShadow, PaintState, PaintText, Painter, Radius, StrokeStyle, TextHorizontalAlignment,
    TextVerticalAlignment,
};

/// 一份解析好的绘制脚本。相同的 JSON 得到相同的 `paint_key`。
#[derive(Debug, Clone)]
pub struct PaintScript {
    commands: Arc<[Command]>,
    hit: HitMode,
    key: u64,
}

/// Which points of the node a script's painter hits.
#[derive(Debug, Clone)]
enum HitMode {
    /// The whole layout box.
    Bounds,
    /// What the recording paints.
    Painted,
    /// This path, in the node's coordinates.
    Path(ScriptPath),
}

impl PaintScript {
    /// 解析脚本。顶层是命令数组，或 `{ "commands": [...], "hit": ... }`。`hit`
    /// 缺省为 `"bounds"`（整个布局矩形可命中），`"painted"` 为录下的内容，
    /// `{ "path": 路径 }` 为这条路径围成的区域。
    ///
    /// 每条命令是带 `"op"` 的对象，可选 `"phase": "behind" | "over"`（缺省
    /// `behind` 画在子节点下面，`over` 画在上面）和 `"when": { "hovered": true,
    /// ... }`（只在这些交互状态下执行，键为 `hovered` / `pressed` / `focused` /
    /// `disabled` / `selected`）。命令里出现它不认识的键、或布尔字段不是
    /// `true` / `false` 时整份脚本解析失败，不会悄悄按缺省值画。
    ///
    /// - 长度：数字为像素；字符串 `"50%"`、`"100% - 12"`、`"50% + 4"` 相对节点
    ///   宽（x、宽）或高（y、高）。半径类长度（圆角、`arc` / `arcTo` 半径、
    ///   径向渐变半径）的百分比相对节点较短的一边，`"50%"` 即内切圆。
    /// - 颜色：语义角色名（`"accent"`、`"surface"` …）、`"#rgb"`/`"#rgba"`/
    ///   `"#rrggbb"`/`"#rrggbbaa"`、`"rgb(…)"`/`"rgba(…)"`、`[r, g, b]` 或
    ///   `[r, g, b, a]`（`0..=1`）；主题相关的混色写 `{ "mix": [角色, 角色, 比例] }`
    ///   （比例是第一个角色的份量）或 `{ "alpha": [角色, 不透明度] }`。
    /// - 上色：颜色，或渐变 `{ "linear": [x0, y0, x1, y1] | "radial": [cx, cy, r]
    ///   | "conic": [cx, cy, 起始弧度], "stops": [[偏移, 颜色], …], "extend":
    ///   "pad" | "repeat" | "reflect" }`，色标在 premultiplied sRGB 里插值。
    /// - 路径：SVG path 字符串（`M L H V C S Q T A Z`，大小写均可），或分段数组
    ///   `[["M", x, y], ["L", x, y], ["Q", cx, cy, x, y], ["C", …6], ["arcTo",
    ///   拐角x, 拐角y, 终点x, 终点y, r], ["arc", cx, cy, r, 起始, 扫过], ["rect",
    ///   x, y, w, h], ["roundedRect", x, y, w, h, 圆角或四个圆角], ["ellipse", x, y,
    ///   w, h], ["Z"]]`，分段里的长度可以写相对值；要指定填充规则时写成
    ///   `{ "d": 上面任一种, "fillRule": "nonzero" | "evenodd" }`。
    /// - 圆角：长度，档位名 `"xs" | "sm" | "md" | "lg" | "xl"`，或四个这样的值（左上、
    ///   右上、右下、左下）。
    /// - 混合方式：CSS `mix-blend-mode` 的全部取值（`"normal"`、`"multiply"`、
    ///   `"screen"`、`"overlay"`、`"darken"`、`"lighten"`、`"color-dodge"`、
    ///   `"color-burn"`、`"hard-light"`、`"soft-light"`、`"difference"`、
    ///   `"exclusion"`、`"hue"`、`"saturation"`、`"color"`、`"luminosity"`）。
    ///
    /// 命令：`fill`（`path`、`paint`）、`stroke`（`path`、`paint`、`width` 缺省
    /// 1、`join: "miter" | "round" | "bevel"` 缺省 `miter`、`cap: "butt" |
    /// "round" | "square"` 缺省 `butt`、`dash`、`dashOffset`、`miterLimit` 缺省
    /// 10）、`shadow`（`path`，以及 `elevation: "surface" | "overlay"` 或
    /// `color`、`offset: [x, y]`、`blur`、`spread`、`inset`）、`roundedRect`
    /// （`rect: [x, y, w, h]`、`radii`、`fill`、`border: [颜色, 宽度]`、
    /// `shadow`：同 `shadow` 命令的字段）、`text`（`rect`、`text`、`paint`、
    /// `size`、`weight`、`italic`、`lineHeight`、`wrap`、`maxLines`、`align:
    /// ["start"|"center"|"end", "top"|"center"|"bottom"]`）、`image`（`rect`、
    /// `src`、`fit`、`radii`）、`icon`（`rect`、`name`：内建图标名，`paint`）、
    /// `fillImage`（`path`、`rect`、`src`、`fit`）、`save`、`restore`、
    /// `translate`（`x`、`y`，可写相对长度）、`scale`（`x`，`y` 缺省同 `x`）、
    /// `rotate`（`angle` 弧度）、`transform`（`matrix: [a, b, c, d, e, f]`，乘到
    /// 当前变换上）、`setTransform`（`matrix`，替换当前变换）、`opacity`
    /// （`value`）、`blend`（`mode`）、`pushLayer`（`opacity`、`blend`）、
    /// `popLayer`、`pushClip`（`path`）、`popClip`、`drawDefault`。`fit` 为
    /// `"fill" | "contain" | "cover" | "none" | "scale-down"`，缺省 `contain`。
    ///
    /// 脚本是静态的：需要读回结果的 `measure_text`、自定义 `hit_test` 逻辑只在
    /// Rust 的 [`Painter`] 里有。
    pub fn from_json(value: &Value) -> Result<Self, String> {
        if !numbers_fit_f32(value) {
            return Err("a number does not fit a 32-bit float".into());
        }
        let (list, hit) = match value {
            Value::Array(list) => (list.as_slice(), None),
            Value::Object(map) => {
                only_keys(map, &["commands", "hit"], "a paint script")?;
                (
                    map.get("commands")
                        .and_then(Value::as_array)
                        .map(Vec::as_slice)
                        .ok_or("a paint script object needs `commands`")?,
                    map.get("hit"),
                )
            }
            _ => return Err("a paint script is an array or an object".into()),
        };
        let hit = match hit {
            None | Some(Value::Null) => HitMode::Bounds,
            Some(Value::String(mode)) if mode == "bounds" => HitMode::Bounds,
            Some(Value::String(mode)) if mode == "painted" => HitMode::Painted,
            Some(Value::Object(shape)) if shape.len() == 1 && shape.contains_key("path") => {
                HitMode::Path(parse_path(&shape["path"]).map_err(|error| format!("hit: {error}"))?)
            }
            Some(_) => {
                return Err("`hit` is \"bounds\", \"painted\" or { \"path\": 路径 }".into());
            }
        };
        let commands = list
            .iter()
            .enumerate()
            .map(|(index, command)| {
                Command::parse(command).map_err(|error| format!("command {index}: {error}"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        value.to_string().hash(&mut hasher);
        Ok(Self {
            commands: commands.into(),
            hit,
            key: hasher.finish(),
        })
    }

    /// 从 JSON 文本解析，格式同 [`Self::from_json`]。
    pub fn from_json_str(text: &str) -> Result<Self, String> {
        let value: Value = serde_json::from_str(text).map_err(|error| error.to_string())?;
        Self::from_json(&value)
    }

    fn replay(&self, cx: &mut PaintContext<'_>, phase: PaintPhase) {
        let size = cx.size();
        let state = cx.state();
        for command in self
            .commands
            .iter()
            .filter(|command| command.phase == phase && command.when.admits(state))
        {
            command.op.apply(cx, size);
        }
    }
}

impl Painter for PaintScript {
    fn paint(&self, cx: &mut PaintContext<'_>) {
        self.replay(cx, PaintPhase::BehindChildren);
    }

    fn paint_over_children(&self, cx: &mut PaintContext<'_>) {
        self.replay(cx, PaintPhase::OverChildren);
    }

    fn hit_test(&self, local: [f32; 2], size: [f32; 2]) -> Option<bool> {
        match &self.hit {
            HitMode::Path(path) => Some(path.build(size).contains(local)),
            HitMode::Bounds | HitMode::Painted => None,
        }
    }

    fn hit_painted_outline(&self) -> bool {
        matches!(self.hit, HitMode::Painted)
    }

    fn paint_key(&self) -> u64 {
        self.key
    }
}

#[derive(Debug, Clone)]
struct Command {
    phase: PaintPhase,
    when: StateFilter,
    op: Op,
}

#[derive(Debug, Clone, Copy, Default)]
struct StateFilter {
    hovered: Option<bool>,
    pressed: Option<bool>,
    focused: Option<bool>,
    disabled: Option<bool>,
    selected: Option<bool>,
}

impl StateFilter {
    fn admits(self, state: PaintState) -> bool {
        [
            (self.hovered, state.hovered),
            (self.pressed, state.pressed),
            (self.focused, state.focused),
            (self.disabled, state.disabled),
            (self.selected, state.selected),
        ]
        .into_iter()
        .all(|(want, is)| want.is_none_or(|want| want == is))
    }
}

/// A radius: a length against the node's shorter side (`"50%"` of it makes a
/// circle of a square), or a theme tier.
#[derive(Debug, Clone, Copy)]
enum ScriptRadius {
    Len(Len),
    Tier(RadiusTier),
}

impl ScriptRadius {
    fn resolve(self, size: [f32; 2]) -> Radius {
        match self {
            Self::Len(len) => Radius::Px(len.resolve(size[0].min(size[1]))),
            Self::Tier(tier) => Radius::Tier(tier),
        }
    }
}

fn radii_of(radii: &[ScriptRadius; 4], size: [f32; 2]) -> CornerRadii {
    CornerRadii(radii.map(|radius| radius.resolve(size)))
}

/// A length against the node's shorter side, for a radius a path needs in
/// pixels.
fn short_side(len: Len, size: [f32; 2]) -> f32 {
    len.resolve(size[0].min(size[1]))
}

/// A length: px, or a fraction of the node's size on its axis plus px.
#[derive(Debug, Clone, Copy)]
struct Len {
    fraction: f32,
    px: f32,
}

impl Len {
    fn resolve(self, extent: f32) -> f32 {
        self.fraction * extent + self.px
    }
}

#[derive(Debug, Clone)]
enum Op {
    Fill {
        path: ScriptPath,
        paint: ScriptPaint,
    },
    Stroke {
        path: ScriptPath,
        paint: ScriptPaint,
        stroke: StrokeStyle,
    },
    Shadow {
        path: ScriptPath,
        shadow: PaintShadow,
    },
    RoundedRect {
        rect: [Len; 4],
        radii: [ScriptRadius; 4],
        fill: Option<ScriptPaint>,
        border: Option<(PaintColor, f32)>,
        shadow: Option<PaintShadow>,
    },
    Text {
        rect: [Len; 4],
        text: PaintTextSpec,
    },
    Image {
        rect: [Len; 4],
        src: Arc<str>,
        fit: ImageFit,
        radii: [ScriptRadius; 4],
    },
    Icon {
        rect: [Len; 4],
        icon: nana_ui_core::Icon,
        paint: ScriptPaint,
    },
    FillImage {
        path: ScriptPath,
        rect: [Len; 4],
        src: Arc<str>,
        fit: ImageFit,
    },
    Save,
    Restore,
    Translate(Len, Len),
    Scale(f32, f32),
    Rotate(f32),
    Transform([f32; 6]),
    SetTransform([f32; 6]),
    Opacity(f32),
    Blend(BlendMode),
    PushLayer(f32, BlendMode),
    PopLayer,
    PushClip(ScriptPath),
    PopClip,
    DrawDefault,
}

#[derive(Debug, Clone)]
struct PaintTextSpec {
    content: Arc<str>,
    paint: ScriptPaint,
    size: Option<f32>,
    weight: Option<u16>,
    italic: bool,
    line_height: Option<f32>,
    wrap: bool,
    max_lines: Option<u16>,
    horizontal: TextHorizontalAlignment,
    vertical: TextVerticalAlignment,
}

impl Op {
    fn apply(&self, cx: &mut PaintContext<'_>, size: [f32; 2]) {
        match self {
            Self::Fill { path, paint } => cx.fill_path(&path.build(size), paint.resolve(size)),
            Self::Stroke {
                path,
                paint,
                stroke,
            } => cx.stroke_path(&path.build(size), stroke.clone(), paint.resolve(size)),
            Self::Shadow { path, shadow } => cx.shadow(&path.build(size), *shadow),
            Self::RoundedRect {
                rect,
                radii,
                fill,
                border,
                shadow,
            } => cx.rounded_rect(
                rect_of(rect, size),
                radii_of(radii, size),
                BoxPaint {
                    fill: fill.as_ref().map(|fill| fill.resolve(size)),
                    border: *border,
                    shadow: *shadow,
                },
            ),
            Self::Text { rect, text } => {
                let mut paint_text = PaintText::new(Arc::clone(&text.content))
                    .paint(text.paint.resolve(size))
                    .italic(text.italic)
                    .wrap(text.wrap)
                    .align(text.horizontal, text.vertical);
                paint_text.size = text.size;
                paint_text.weight = text.weight;
                paint_text.line_height = text.line_height;
                paint_text.max_lines = text.max_lines;
                cx.text(rect_of(rect, size), paint_text);
            }
            Self::Image {
                rect,
                src,
                fit,
                radii,
            } => cx.image(
                rect_of(rect, size),
                Arc::clone(src),
                *fit,
                radii_of(radii, size),
            ),
            Self::Icon { rect, icon, paint } => {
                cx.icon(rect_of(rect, size), *icon, paint.resolve(size));
            }
            Self::FillImage {
                path,
                rect,
                src,
                fit,
            } => cx.fill_path_with_image(
                &path.build(size),
                rect_of(rect, size),
                Arc::clone(src),
                *fit,
            ),
            Self::Save => cx.save(),
            Self::Restore => cx.restore(),
            Self::Translate(x, y) => cx.translate(x.resolve(size[0]), y.resolve(size[1])),
            Self::Scale(x, y) => cx.scale(*x, *y),
            Self::Rotate(angle) => cx.rotate(*angle),
            Self::Transform(matrix) => cx.concat(*matrix),
            Self::SetTransform(matrix) => cx.set_transform(*matrix),
            Self::Opacity(value) => cx.set_opacity(*value),
            Self::Blend(mode) => cx.set_blend(*mode),
            Self::PushLayer(opacity, blend) => cx.push_layer(*opacity, *blend),
            Self::PopLayer => cx.pop_layer(),
            Self::PushClip(path) => cx.push_clip(&path.build(size)),
            Self::PopClip => cx.pop_clip(),
            Self::DrawDefault => cx.draw_default(),
        }
    }
}

fn rect_of(rect: &[Len; 4], size: [f32; 2]) -> LayoutBox {
    LayoutBox {
        x: rect[0].resolve(size[0]),
        y: rect[1].resolve(size[1]),
        width: rect[2].resolve(size[0]),
        height: rect[3].resolve(size[1]),
    }
}

#[derive(Debug, Clone)]
enum ScriptPaint {
    Color(PaintColor),
    Gradient {
        shape: GradientSpec,
        stops: Vec<(f32, PaintColor)>,
        extend: GradientExtend,
    },
}

#[derive(Debug, Clone, Copy)]
enum GradientSpec {
    Linear([Len; 4]),
    Radial([Len; 2], Len),
    Conic([Len; 2], f32),
}

impl ScriptPaint {
    fn resolve(&self, size: [f32; 2]) -> Paint {
        match self {
            Self::Color(color) => Paint::Color(*color),
            Self::Gradient {
                shape,
                stops,
                extend,
            } => {
                let point = |x: Len, y: Len| [x.resolve(size[0]), y.resolve(size[1])];
                let mut gradient = match *shape {
                    GradientSpec::Linear([x0, y0, x1, y1]) => {
                        Gradient::linear(point(x0, y0), point(x1, y1))
                    }
                    GradientSpec::Radial([x, y], radius) => {
                        Gradient::radial(point(x, y), short_side(radius, size))
                    }
                    GradientSpec::Conic([x, y], angle) => Gradient::conic(point(x, y), angle),
                };
                gradient.stops = stops
                    .iter()
                    .map(|(offset, color)| ColorStop {
                        offset: *offset,
                        color: *color,
                    })
                    .collect();
                Paint::Gradient(gradient.extend(*extend))
            }
        }
    }
}

#[derive(Debug, Clone)]
struct ScriptPath {
    segments: Arc<[Segment]>,
    fill_rule: FillRule,
}

#[derive(Debug, Clone)]
enum Segment {
    Move(Len, Len),
    Line(Len, Len),
    Quad([Len; 4]),
    Cubic([Len; 6]),
    ArcTo([Len; 4], Len),
    Arc([Len; 2], Len, f32, f32),
    Rect([Len; 4]),
    RoundedRect([Len; 4], [Len; 4]),
    Ellipse([Len; 4]),
    Close,
}

impl ScriptPath {
    fn build(&self, size: [f32; 2]) -> PaintPath {
        let x = |len: Len| len.resolve(size[0]);
        let y = |len: Len| len.resolve(size[1]);
        let rect = |r: &[Len; 4]| LayoutBox {
            x: x(r[0]),
            y: y(r[1]),
            width: x(r[2]),
            height: y(r[3]),
        };
        let mut path = PaintPath::new().with_fill_rule(self.fill_rule);
        for segment in self.segments.iter() {
            match segment {
                Segment::Move(a, b) => {
                    path.move_to(x(*a), y(*b));
                }
                Segment::Line(a, b) => {
                    path.line_to(x(*a), y(*b));
                }
                Segment::Quad(v) => {
                    path.quad_to(x(v[0]), y(v[1]), x(v[2]), y(v[3]));
                }
                Segment::Cubic(v) => {
                    path.cubic_to(x(v[0]), y(v[1]), x(v[2]), y(v[3]), x(v[4]), y(v[5]));
                }
                Segment::ArcTo(v, radius) => {
                    path.arc_to(
                        x(v[0]),
                        y(v[1]),
                        x(v[2]),
                        y(v[3]),
                        short_side(*radius, size),
                    );
                }
                Segment::Arc(center, radius, start, sweep) => {
                    path.arc(
                        x(center[0]),
                        y(center[1]),
                        short_side(*radius, size),
                        *start,
                        *sweep,
                    );
                }
                Segment::Rect(r) => {
                    path.rect(rect(r));
                }
                Segment::RoundedRect(r, radii) => {
                    path.rounded_rect(rect(r), radii.map(|radius| short_side(radius, size)));
                }
                Segment::Ellipse(r) => {
                    path.ellipse(rect(r));
                }
                Segment::Close => {
                    path.close();
                }
            }
        }
        path
    }
}

impl PaintPath {
    /// 按 SVG path 数据（`M L H V C S Q T A Z`，大小写均可）建路径，弧转成
    /// 三次曲线；数据不合法时返回原因。
    pub fn from_svg(data: &str) -> Result<Self, String> {
        let segments = parse_svg_path(data)?;
        Ok(ScriptPath {
            segments: segments.into(),
            fill_rule: FillRule::NonZero,
        }
        .build([0.0, 0.0]))
    }
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

type Map = serde_json::Map<String, Value>;

impl Command {
    fn parse(value: &Value) -> Result<Self, String> {
        let map = value.as_object().ok_or("a command is an object")?;
        let op = map
            .get("op")
            .and_then(Value::as_str)
            .ok_or("a command needs `op`")?;
        let phase = match map.get("phase").and_then(Value::as_str) {
            None | Some("behind") => PaintPhase::BehindChildren,
            Some("over") => PaintPhase::OverChildren,
            Some(other) => return Err(format!("unknown phase `{other}`")),
        };
        let mut when = StateFilter::default();
        if let Some(filter) = map.get("when") {
            let filter = filter.as_object().ok_or("`when` is an object")?;
            for (key, value) in filter {
                let value = value.as_bool().ok_or("`when` values are booleans")?;
                let slot = match key.as_str() {
                    "hovered" => &mut when.hovered,
                    "pressed" => &mut when.pressed,
                    "focused" => &mut when.focused,
                    "disabled" => &mut when.disabled,
                    "selected" => &mut when.selected,
                    other => return Err(format!("unknown state `{other}`")),
                };
                *slot = Some(value);
            }
        }
        Ok(Self {
            phase,
            when,
            op: parse_op(op, map)?,
        })
    }
}

/// The keys each op reads, past `op`, `phase` and `when`.
fn op_keys(op: &str) -> Option<&'static [&'static str]> {
    Some(match op {
        "fill" => &["path", "paint"],
        "stroke" => &[
            "path",
            "paint",
            "width",
            "join",
            "cap",
            "dash",
            "dashOffset",
            "miterLimit",
        ],
        "shadow" => &[
            "path",
            "elevation",
            "color",
            "offset",
            "blur",
            "spread",
            "inset",
        ],
        "roundedRect" => &["rect", "radii", "fill", "border", "shadow"],
        "text" => &[
            "rect",
            "text",
            "paint",
            "size",
            "weight",
            "italic",
            "lineHeight",
            "wrap",
            "maxLines",
            "align",
        ],
        "image" => &["rect", "src", "fit", "radii"],
        "icon" => &["rect", "name", "paint"],
        "fillImage" => &["path", "rect", "src", "fit"],
        "translate" | "scale" => &["x", "y"],
        "rotate" => &["angle"],
        "transform" | "setTransform" => &["matrix"],
        "opacity" => &["value"],
        "blend" => &["mode"],
        "pushLayer" => &["opacity", "blend"],
        "pushClip" => &["path"],
        "save" | "restore" | "popLayer" | "popClip" | "drawDefault" => &[],
        _ => return None,
    })
}

/// Reject a key nothing reads: a misspelt one would otherwise paint with a
/// default and say nothing.
fn only_keys(map: &Map, allowed: &[&str], what: &str) -> Result<(), String> {
    match map.keys().find(|key| !allowed.contains(&key.as_str())) {
        Some(key) => Err(format!("unknown key `{key}` in {what}")),
        None => Ok(()),
    }
}

/// A boolean field, `default` when absent.
fn flag(map: &Map, key: &str, default: bool) -> Result<bool, String> {
    match map.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(value) => value
            .as_bool()
            .ok_or_else(|| format!("`{key}` is true or false")),
    }
}

fn matrix(map: &Map) -> Result<[f32; 6], String> {
    let values = required(map, "matrix")?
        .as_array()
        .ok_or("`matrix` is six numbers")?
        .iter()
        .map(|v| {
            v.as_f64()
                .map(|v| v as f32)
                .ok_or("`matrix` is six numbers")
        })
        .collect::<Result<Vec<_>, _>>()?;
    values
        .try_into()
        .map_err(|_| "`matrix` is six numbers".to_string())
}

fn parse_op(op: &str, map: &Map) -> Result<Op, String> {
    let Some(keys) = op_keys(op) else {
        return Err(format!("unknown op `{op}`"));
    };
    if let Some(key) = map.keys().find(|key| {
        !matches!(key.as_str(), "op" | "phase" | "when") && !keys.contains(&key.as_str())
    }) {
        return Err(format!("unknown key `{key}` for `{op}`"));
    }
    let path = || required(map, "path").and_then(parse_path);
    let paint = |key: &str| required(map, key).and_then(parse_paint);
    let number = |key: &str, default: f32| optional_number(map, key).map(|v| v.unwrap_or(default));
    let rect = || required(map, "rect").and_then(|v| lens::<4>(v, "rect"));
    Ok(match op {
        "fill" => Op::Fill {
            path: path()?,
            paint: paint("paint")?,
        },
        "stroke" => {
            let mut stroke = StrokeStyle::new(number("width", 1.0)?)
                .join(match map.get("join").and_then(Value::as_str) {
                    None | Some("miter") => LineJoin::Miter,
                    Some("round") => LineJoin::Round,
                    Some("bevel") => LineJoin::Bevel,
                    Some(other) => return Err(format!("unknown join `{other}`")),
                })
                .cap(match map.get("cap").and_then(Value::as_str) {
                    None | Some("butt") => LineCap::Butt,
                    Some("round") => LineCap::Round,
                    Some("square") => LineCap::Square,
                    Some(other) => return Err(format!("unknown cap `{other}`")),
                })
                .miter_limit(number("miterLimit", 10.0)?);
            if let Some(dash) = map.get("dash") {
                let pattern = dash
                    .as_array()
                    .ok_or("`dash` is an array of numbers")?
                    .iter()
                    .map(|v| v.as_f64().map(|v| v as f32).ok_or("`dash` holds numbers"))
                    .collect::<Result<Vec<_>, _>>()?;
                stroke = stroke.dash(pattern, number("dashOffset", 0.0)?);
            }
            Op::Stroke {
                path: path()?,
                paint: paint("paint")?,
                stroke,
            }
        }
        "shadow" => Op::Shadow {
            path: path()?,
            shadow: parse_shadow(map)?,
        },
        "roundedRect" => Op::RoundedRect {
            rect: rect()?,
            radii: map
                .get("radii")
                .map_or(Ok([ScriptRadius::Len(px(0.0)); 4]), parse_radii)?,
            fill: map.get("fill").map(parse_paint).transpose()?,
            border: map
                .get("border")
                .map(|border| {
                    let pair = border.as_array().ok_or("`border` is [color, width]")?;
                    let [color, width] = pair.as_slice() else {
                        return Err("`border` is [color, width]".to_string());
                    };
                    Ok((
                        parse_color(color)?,
                        width.as_f64().ok_or("border width is a number")? as f32,
                    ))
                })
                .transpose()?,
            shadow: map
                .get("shadow")
                .map(|shadow| {
                    let shadow = shadow.as_object().ok_or("`shadow` is an object")?;
                    only_keys(
                        shadow,
                        &["elevation", "color", "offset", "blur", "spread", "inset"],
                        "`shadow`",
                    )?;
                    parse_shadow(shadow)
                })
                .transpose()?,
        },
        "text" => Op::Text {
            rect: rect()?,
            text: PaintTextSpec {
                content: Arc::from(
                    required(map, "text")?
                        .as_str()
                        .ok_or("`text` is a string")?,
                ),
                paint: map.get("paint").map_or(
                    Ok(ScriptPaint::Color(PaintColor::Role(
                        SemanticColorRole::Text,
                    ))),
                    parse_paint,
                )?,
                size: optional_number(map, "size")?,
                weight: optional_number(map, "weight")?.map(|w| w.clamp(1.0, 1000.0) as u16),
                italic: flag(map, "italic", false)?,
                line_height: optional_number(map, "lineHeight")?,
                wrap: flag(map, "wrap", false)?,
                max_lines: optional_number(map, "maxLines")?.map(|n| n.clamp(1.0, 65535.0) as u16),
                horizontal: TextHorizontalAlignment::Start,
                vertical: TextVerticalAlignment::Center,
            },
        }
        .with_align(map.get("align"))?,
        "image" => Op::Image {
            rect: rect()?,
            src: source(map)?,
            fit: parse_fit(map.get("fit"))?,
            radii: map
                .get("radii")
                .map_or(Ok([ScriptRadius::Len(px(0.0)); 4]), parse_radii)?,
        },
        "icon" => {
            let name = required(map, "name")?
                .as_str()
                .ok_or("`name` is a string")?;
            Op::Icon {
                rect: rect()?,
                icon: nana_ui_core::Icon::parse_name(name)
                    .ok_or_else(|| format!("unknown icon `{name}`"))?,
                paint: map.get("paint").map_or(
                    Ok(ScriptPaint::Color(PaintColor::Role(
                        SemanticColorRole::Text,
                    ))),
                    parse_paint,
                )?,
            }
        }
        "fillImage" => Op::FillImage {
            path: path()?,
            rect: rect()?,
            src: source(map)?,
            fit: parse_fit(map.get("fit"))?,
        },
        "save" => Op::Save,
        "restore" => Op::Restore,
        "translate" => Op::Translate(
            map.get("x").map_or(Ok(px(0.0)), parse_len)?,
            map.get("y").map_or(Ok(px(0.0)), parse_len)?,
        ),
        "scale" => {
            let x = number("x", 1.0)?;
            Op::Scale(x, number("y", x)?)
        }
        "rotate" => Op::Rotate(number("angle", 0.0)?),
        "transform" => Op::Transform(matrix(map)?),
        "setTransform" => Op::SetTransform(matrix(map)?),
        "opacity" => Op::Opacity(number("value", 1.0)?),
        "blend" => Op::Blend(parse_blend(map.get("mode"))?),
        "pushLayer" => Op::PushLayer(number("opacity", 1.0)?, parse_blend(map.get("blend"))?),
        "popLayer" => Op::PopLayer,
        "pushClip" => Op::PushClip(path()?),
        "popClip" => Op::PopClip,
        "drawDefault" => Op::DrawDefault,
        other => unreachable!("`{other}` has keys but no parser"),
    })
}

impl Op {
    fn with_align(mut self, align: Option<&Value>) -> Result<Self, String> {
        let (Self::Text { text, .. }, Some(align)) = (&mut self, align) else {
            return Ok(self);
        };
        let pair = align
            .as_array()
            .ok_or("`align` is [horizontal, vertical]")?;
        let word = |index: usize| pair.get(index).and_then(Value::as_str);
        text.horizontal = match word(0) {
            None | Some("start") => TextHorizontalAlignment::Start,
            Some("center") => TextHorizontalAlignment::Center,
            Some("end") => TextHorizontalAlignment::End,
            Some(other) => return Err(format!("unknown horizontal alignment `{other}`")),
        };
        text.vertical = match word(1) {
            None | Some("center") => TextVerticalAlignment::Center,
            Some("top") => TextVerticalAlignment::Top,
            Some("bottom") => TextVerticalAlignment::Bottom,
            Some(other) => return Err(format!("unknown vertical alignment `{other}`")),
        };
        Ok(self)
    }
}

fn required<'a>(map: &'a Map, key: &str) -> Result<&'a Value, String> {
    map.get(key).ok_or_else(|| format!("missing `{key}`"))
}

fn optional_number(map: &Map, key: &str) -> Result<Option<f32>, String> {
    match map.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_f64()
            .map(|value| Some(value as f32))
            .ok_or_else(|| format!("`{key}` is a number")),
    }
}

fn source(map: &Map) -> Result<Arc<str>, String> {
    required(map, "src")?
        .as_str()
        .map(Arc::from)
        .ok_or_else(|| "`src` is a string".to_string())
}

fn px(value: f32) -> Len {
    Len {
        fraction: 0.0,
        px: value,
    }
}

/// A number spelled in text, if it is finite: `NaN` and `inf` would reach
/// the tessellator.
fn finite(text: &str) -> Option<f32> {
    text.parse::<f32>().ok().filter(|value| value.is_finite())
}

/// Whether every JSON number stays finite as an `f32`.
fn numbers_fit_f32(value: &Value) -> bool {
    match value {
        Value::Number(number) => number
            .as_f64()
            .is_some_and(|number| (number as f32).is_finite()),
        Value::Array(list) => list.iter().all(numbers_fit_f32),
        Value::Object(map) => map.values().all(numbers_fit_f32),
        _ => true,
    }
}

/// `12`, `"12"`, `"50%"`, `"100% - 12"`, `"50% + 4"`.
fn parse_len(value: &Value) -> Result<Len, String> {
    if let Some(number) = value.as_f64() {
        return Ok(px(number as f32));
    }
    let text = value
        .as_str()
        .ok_or("a length is a number or a string like \"50% + 4\"")?
        .replace(' ', "");
    let bad = || format!("bad length `{text}`");
    let Some(percent_at) = text.find('%') else {
        return finite(&text).map(px).ok_or_else(bad);
    };
    let fraction = finite(&text[..percent_at]).ok_or_else(bad)? / 100.0;
    let rest = &text[percent_at + 1..];
    let offset = if rest.is_empty() {
        0.0
    } else {
        finite(rest).ok_or_else(bad)?
    };
    Ok(Len {
        fraction,
        px: offset,
    })
}

fn lens<const N: usize>(value: &Value, what: &str) -> Result<[Len; N], String> {
    let list = value
        .as_array()
        .filter(|list| list.len() == N)
        .ok_or_else(|| format!("`{what}` holds {N} lengths"))?;
    let parsed = list.iter().map(parse_len).collect::<Result<Vec<_>, _>>()?;
    parsed
        .try_into()
        .map_err(|_| format!("`{what}` holds {N} lengths"))
}

fn parse_color(value: &Value) -> Result<PaintColor, String> {
    if let Some(map) = value.as_object() {
        let role = |value: &Value| {
            value
                .as_str()
                .and_then(SemanticColorRole::from_css_token_name)
                .ok_or_else(|| "a colour mix names semantic roles".to_string())
        };
        let ratio = |value: &Value| {
            value
                .as_f64()
                .map(|v| v as f32)
                .ok_or_else(|| "a colour mix ratio is a number".to_string())
        };
        return match (map.get("mix"), map.get("alpha"), map.len()) {
            (Some(Value::Array(list)), None, 1) if list.len() == 3 => Ok(PaintColor::Mix(
                SemanticColorMix::new(role(&list[0])?, role(&list[1])?, ratio(&list[2])?),
            )),
            (None, Some(Value::Array(list)), 1) if list.len() == 2 => Ok(PaintColor::Mix(
                SemanticColorMix::alpha(role(&list[0])?, ratio(&list[1])?),
            )),
            _ => Err(
                "a colour object is {\"mix\": [role, role, ratio]} or {\"alpha\": [role, alpha]}"
                    .into(),
            ),
        };
    }
    if let Some(list) = value.as_array() {
        let channels = list
            .iter()
            .map(|v| {
                v.as_f64()
                    .map(|v| v as f32)
                    .ok_or("a colour array holds numbers")
            })
            .collect::<Result<Vec<_>, _>>()?;
        return match channels.as_slice() {
            [r, g, b] => Ok(PaintColor::Rgba([*r, *g, *b, 1.0])),
            [r, g, b, a] => Ok(PaintColor::Rgba([*r, *g, *b, *a])),
            _ => Err("a colour array is [r, g, b] or [r, g, b, a]".into()),
        };
    }
    let text = value.as_str().ok_or("a colour is a string or an array")?;
    if let Some(rgba) = parse_css_color(text) {
        return Ok(PaintColor::Rgba(rgba));
    }
    SemanticColorRole::from_css_token_name(text)
        .map(PaintColor::Role)
        .ok_or_else(|| format!("unknown colour `{text}`"))
}

/// `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`, `rgb(…)`, `rgba(…)`.
fn parse_css_color(text: &str) -> Option<[f32; 4]> {
    let text = text.trim();
    if let Some(hex) = text.strip_prefix('#') {
        if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let digit = |i: usize| u8::from_str_radix(&hex[i..i + 1], 16).ok().map(f32::from);
        let pair = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok().map(f32::from);
        return match hex.len() {
            3 | 4 => {
                let alpha = if hex.len() == 4 {
                    digit(3)? / 15.0
                } else {
                    1.0
                };
                Some([digit(0)? / 15.0, digit(1)? / 15.0, digit(2)? / 15.0, alpha])
            }
            6 | 8 => {
                let alpha = if hex.len() == 8 {
                    pair(6)? / 255.0
                } else {
                    1.0
                };
                Some([pair(0)? / 255.0, pair(2)? / 255.0, pair(4)? / 255.0, alpha])
            }
            _ => None,
        };
    }
    let inner = text
        .strip_prefix("rgba(")
        .or_else(|| text.strip_prefix("rgb("))?
        .strip_suffix(')')?;
    let parts: Vec<&str> = inner
        .split([',', '/', ' '])
        .filter(|part| !part.is_empty())
        .collect();
    let channel = |part: &str| -> Option<f32> {
        match part.strip_suffix('%') {
            Some(percent) => finite(percent).map(|v| v / 100.0),
            None => finite(part).map(|v| v / 255.0),
        }
    };
    let alpha = |part: &str| -> Option<f32> {
        match part.strip_suffix('%') {
            Some(percent) => finite(percent).map(|v| v / 100.0),
            None => finite(part),
        }
    };
    match parts.as_slice() {
        [r, g, b] => Some([channel(r)?, channel(g)?, channel(b)?, 1.0]),
        [r, g, b, a] => Some([channel(r)?, channel(g)?, channel(b)?, alpha(a)?]),
        _ => None,
    }
}

fn parse_paint(value: &Value) -> Result<ScriptPaint, String> {
    let Some(map) = value.as_object() else {
        return parse_color(value).map(ScriptPaint::Color);
    };
    if map.contains_key("mix") || map.contains_key("alpha") {
        return parse_color(value).map(ScriptPaint::Color);
    }
    only_keys(
        map,
        &["linear", "radial", "conic", "stops", "extend"],
        "a gradient",
    )?;
    let shape = if let Some(v) = map.get("linear") {
        GradientSpec::Linear(lens::<4>(v, "linear")?)
    } else if let Some(v) = map.get("radial") {
        let list = v
            .as_array()
            .filter(|l| l.len() == 3)
            .ok_or("`radial` is [cx, cy, r]")?;
        GradientSpec::Radial(
            [parse_len(&list[0])?, parse_len(&list[1])?],
            parse_len(&list[2])?,
        )
    } else if let Some(v) = map.get("conic") {
        let list = v
            .as_array()
            .filter(|l| l.len() == 3)
            .ok_or("`conic` is [cx, cy, angle]")?;
        GradientSpec::Conic(
            [parse_len(&list[0])?, parse_len(&list[1])?],
            list[2].as_f64().ok_or("the angle is a number")? as f32,
        )
    } else {
        return Err("a gradient has `linear`, `radial` or `conic`".into());
    };
    let stops = required(map, "stops")?
        .as_array()
        .ok_or("`stops` is an array")?
        .iter()
        .map(|stop| {
            let pair = stop.as_array().ok_or("a stop is [offset, colour]")?;
            let [offset, color] = pair.as_slice() else {
                return Err("a stop is [offset, colour]".to_string());
            };
            Ok((
                offset.as_f64().ok_or("a stop offset is a number")? as f32,
                parse_color(color)?,
            ))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let extend = match map.get("extend").and_then(Value::as_str) {
        None | Some("pad") => GradientExtend::Pad,
        Some("repeat") => GradientExtend::Repeat,
        Some("reflect") => GradientExtend::Reflect,
        Some(other) => return Err(format!("unknown extend `{other}`")),
    };
    Ok(ScriptPaint::Gradient {
        shape,
        stops,
        extend,
    })
}

fn parse_radius(value: &Value) -> Result<ScriptRadius, String> {
    match value.as_str() {
        Some("xs") => Ok(ScriptRadius::Tier(RadiusTier::Xs)),
        Some("sm") => Ok(ScriptRadius::Tier(RadiusTier::Sm)),
        Some("md") => Ok(ScriptRadius::Tier(RadiusTier::Md)),
        Some("lg") => Ok(ScriptRadius::Tier(RadiusTier::Lg)),
        Some("xl") => Ok(ScriptRadius::Tier(RadiusTier::Xl)),
        _ => parse_len(value)
            .map(ScriptRadius::Len)
            .map_err(|_| "a radius is a length or xs / sm / md / lg / xl".into()),
    }
}

fn parse_radii(value: &Value) -> Result<[ScriptRadius; 4], String> {
    match value.as_array() {
        Some(list) if list.len() == 4 => {
            let radii = list
                .iter()
                .map(parse_radius)
                .collect::<Result<Vec<_>, _>>()?;
            Ok([radii[0], radii[1], radii[2], radii[3]])
        }
        Some(_) => Err("`radii` is one radius or four".into()),
        None => parse_radius(value).map(|radius| [radius; 4]),
    }
}

fn parse_shadow(map: &Map) -> Result<PaintShadow, String> {
    if let Some(role) = map.get("elevation") {
        if map.contains_key("color") || map.contains_key("blur") {
            return Err("a shadow is an `elevation` or its own `color` / `blur` …".into());
        }
        return match role.as_str() {
            Some("surface") => Ok(PaintShadow::Elevation(ElevationRole::Surface)),
            Some("overlay") => Ok(PaintShadow::Elevation(ElevationRole::Overlay)),
            _ => Err("`elevation` is surface or overlay".into()),
        };
    }
    let offset = match map.get("offset") {
        None => [0.0, 0.0],
        Some(value) => {
            let list = value
                .as_array()
                .filter(|l| l.len() == 2)
                .ok_or("`offset` is [x, y]")?;
            [
                list[0].as_f64().ok_or("`offset` holds numbers")? as f32,
                list[1].as_f64().ok_or("`offset` holds numbers")? as f32,
            ]
        }
    };
    Ok(PaintShadow::Custom {
        color: map
            .get("color")
            .map_or(Ok(PaintColor::Rgba([0.0, 0.0, 0.0, 0.25])), parse_color)?,
        offset,
        blur: optional_number(map, "blur")?.unwrap_or(0.0),
        spread: optional_number(map, "spread")?.unwrap_or(0.0),
        inset: flag(map, "inset", false)?,
    })
}

fn parse_fit(value: Option<&Value>) -> Result<ImageFit, String> {
    Ok(match value.and_then(Value::as_str) {
        None | Some("contain") => ImageFit::Contain,
        Some("fill") => ImageFit::Fill,
        Some("cover") => ImageFit::Cover,
        Some("none") => ImageFit::None,
        Some("scale-down") => ImageFit::ScaleDown,
        Some(other) => return Err(format!("unknown fit `{other}`")),
    })
}

/// A CSS `mix-blend-mode` keyword.
fn parse_blend(value: Option<&Value>) -> Result<BlendMode, String> {
    match value {
        None | Some(Value::Null) => Ok(BlendMode::Normal),
        Some(value) => {
            let name = value.as_str().ok_or("a blend mode is a string")?;
            BlendMode::parse(name).ok_or_else(|| format!("unknown blend `{name}`"))
        }
    }
}

fn parse_path(value: &Value) -> Result<ScriptPath, String> {
    let (segments, fill_rule) = match value {
        Value::String(text) => (parse_svg_path(text)?, FillRule::NonZero),
        Value::Array(list) => (parse_segments(list)?, FillRule::NonZero),
        Value::Object(map) => {
            only_keys(map, &["d", "fillRule"], "a path object")?;
            let rule = match map.get("fillRule").and_then(Value::as_str) {
                None | Some("nonzero") => FillRule::NonZero,
                Some("evenodd") => FillRule::EvenOdd,
                Some(other) => return Err(format!("unknown fill rule `{other}`")),
            };
            let inner = parse_path(required(map, "d")?)?;
            return Ok(ScriptPath {
                segments: inner.segments,
                fill_rule: rule,
            });
        }
        _ => return Err("a path is an SVG path string or an array of segments".into()),
    };
    Ok(ScriptPath {
        segments: segments.into(),
        fill_rule,
    })
}

fn parse_segments(list: &[Value]) -> Result<Vec<Segment>, String> {
    list.iter()
        .map(|segment| {
            let parts = segment.as_array().ok_or("a segment is an array")?;
            let (name, args) = parts.split_first().ok_or("an empty segment")?;
            let name = name.as_str().ok_or("a segment starts with its name")?;
            let lengths = |n: usize| -> Result<Vec<Len>, String> {
                if args.len() < n {
                    return Err(format!("`{name}` needs {n} values"));
                }
                args[..n].iter().map(parse_len).collect()
            };
            let number = |i: usize| -> Result<f32, String> {
                args.get(i)
                    .and_then(Value::as_f64)
                    .map(|v| v as f32)
                    .ok_or_else(|| format!("`{name}` needs a number at {i}"))
            };
            let four = |v: Vec<Len>| [v[0], v[1], v[2], v[3]];
            Ok(match name {
                "M" => {
                    let v = lengths(2)?;
                    Segment::Move(v[0], v[1])
                }
                "L" => {
                    let v = lengths(2)?;
                    Segment::Line(v[0], v[1])
                }
                "Q" => Segment::Quad(four(lengths(4)?)),
                "C" => {
                    let v = lengths(6)?;
                    Segment::Cubic([v[0], v[1], v[2], v[3], v[4], v[5]])
                }
                "arcTo" => {
                    let v = lengths(5)?;
                    Segment::ArcTo([v[0], v[1], v[2], v[3]], v[4])
                }
                "arc" => {
                    let v = lengths(3)?;
                    Segment::Arc([v[0], v[1]], v[2], number(3)?, number(4)?)
                }
                "rect" => Segment::Rect(four(lengths(4)?)),
                "roundedRect" => {
                    let radii = match args.get(4) {
                        None => [px(0.0); 4],
                        Some(Value::Array(list)) if list.len() == 4 => {
                            let mut radii = [px(0.0); 4];
                            for (slot, value) in radii.iter_mut().zip(list) {
                                *slot = parse_len(value)?;
                            }
                            radii
                        }
                        Some(value) => [parse_len(value)?; 4],
                    };
                    Segment::RoundedRect(four(lengths(4)?), radii)
                }
                "ellipse" => Segment::Ellipse(four(lengths(4)?)),
                "Z" | "z" => Segment::Close,
                other => return Err(format!("unknown segment `{other}`")),
            })
        })
        .collect()
}

/// SVG path data, absolute and relative, with arcs converted to cubics.
fn parse_svg_path(data: &str) -> Result<Vec<Segment>, String> {
    let mut tokens = SvgTokens::new(data);
    let mut out = Vec::new();
    let mut current = [0.0f32; 2];
    let mut start = [0.0f32; 2];
    // The last control point, for S and T reflections.
    let mut last_cubic: Option<[f32; 2]> = None;
    let mut last_quad: Option<[f32; 2]> = None;
    let mut command = None;
    let point = |p: [f32; 2]| (px(p[0]), px(p[1]));
    while let Some(next) = tokens.next_command_or_number()? {
        let letter = match next {
            SvgToken::Command(letter) => {
                command = Some(letter);
                letter
            }
            SvgToken::Number => {
                tokens.unread();
                match command {
                    // Coordinates after a moveto are implicit linetos.
                    Some('M') => 'L',
                    Some('m') => 'l',
                    Some('Z' | 'z') => return Err("number after closepath".into()),
                    Some(letter) => letter,
                    None => return Err("path data starts with a number".into()),
                }
            }
        };
        let relative = letter.is_ascii_lowercase();
        let base = if relative { current } else { [0.0, 0.0] };
        let read_point = |tokens: &mut SvgTokens| -> Result<[f32; 2], String> {
            Ok([tokens.number()? + base[0], tokens.number()? + base[1]])
        };
        match letter.to_ascii_uppercase() {
            'M' => {
                let p = read_point(&mut tokens)?;
                let (x, y) = point(p);
                out.push(Segment::Move(x, y));
                current = p;
                start = p;
                last_cubic = None;
                last_quad = None;
            }
            'L' => {
                let p = read_point(&mut tokens)?;
                let (x, y) = point(p);
                out.push(Segment::Line(x, y));
                current = p;
                last_cubic = None;
                last_quad = None;
            }
            'H' => {
                let x = tokens.number()? + if relative { current[0] } else { 0.0 };
                current = [x, current[1]];
                out.push(Segment::Line(px(current[0]), px(current[1])));
                last_cubic = None;
                last_quad = None;
            }
            'V' => {
                let y = tokens.number()? + if relative { current[1] } else { 0.0 };
                current = [current[0], y];
                out.push(Segment::Line(px(current[0]), px(current[1])));
                last_cubic = None;
                last_quad = None;
            }
            'C' | 'S' => {
                let c1 = if letter.eq_ignore_ascii_case(&'S') {
                    last_cubic.map_or(current, |c| {
                        [2.0 * current[0] - c[0], 2.0 * current[1] - c[1]]
                    })
                } else {
                    read_point(&mut tokens)?
                };
                let c2 = read_point(&mut tokens)?;
                let p = read_point(&mut tokens)?;
                out.push(Segment::Cubic([
                    px(c1[0]),
                    px(c1[1]),
                    px(c2[0]),
                    px(c2[1]),
                    px(p[0]),
                    px(p[1]),
                ]));
                current = p;
                last_cubic = Some(c2);
                last_quad = None;
            }
            'Q' | 'T' => {
                let c = if letter.eq_ignore_ascii_case(&'T') {
                    last_quad.map_or(current, |c| {
                        [2.0 * current[0] - c[0], 2.0 * current[1] - c[1]]
                    })
                } else {
                    read_point(&mut tokens)?
                };
                let p = read_point(&mut tokens)?;
                out.push(Segment::Quad([px(c[0]), px(c[1]), px(p[0]), px(p[1])]));
                current = p;
                last_quad = Some(c);
                last_cubic = None;
            }
            'A' => {
                let rx = tokens.number()?.abs();
                let ry = tokens.number()?.abs();
                let rotation = tokens.number()?.to_radians();
                let large = tokens.flag()?;
                let sweep = tokens.flag()?;
                let p = read_point(&mut tokens)?;
                for cubic in svg_arc(current, p, rx, ry, rotation, large, sweep) {
                    out.push(Segment::Cubic(cubic.map(px)));
                }
                current = p;
                last_cubic = None;
                last_quad = None;
            }
            'Z' => {
                out.push(Segment::Close);
                current = start;
                last_cubic = None;
                last_quad = None;
            }
            other => return Err(format!("unknown path command `{other}`")),
        }
    }
    Ok(out)
}

enum SvgToken {
    Command(char),
    /// A number where a command letter could have been: the previous
    /// command repeats. Read again as a coordinate after `unread`.
    Number,
}

struct SvgTokens<'a> {
    text: &'a [u8],
    at: usize,
    last: usize,
}

impl<'a> SvgTokens<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            text: text.as_bytes(),
            at: 0,
            last: 0,
        }
    }

    fn skip_separators(&mut self) {
        while self.at < self.text.len()
            && (self.text[self.at].is_ascii_whitespace() || self.text[self.at] == b',')
        {
            self.at += 1;
        }
    }

    fn next_command_or_number(&mut self) -> Result<Option<SvgToken>, String> {
        self.skip_separators();
        self.last = self.at;
        let Some(&byte) = self.text.get(self.at) else {
            return Ok(None);
        };
        if byte.is_ascii_alphabetic() && byte != b'e' && byte != b'E' {
            self.at += 1;
            return Ok(Some(SvgToken::Command(byte as char)));
        }
        self.number().map(|_| Some(SvgToken::Number))
    }

    fn unread(&mut self) {
        self.at = self.last;
    }

    fn number(&mut self) -> Result<f32, String> {
        self.skip_separators();
        let begin = self.at;
        let mut end = self.at;
        let bytes = self.text;
        if matches!(bytes.get(end), Some(b'+' | b'-')) {
            end += 1;
        }
        let mut seen_dot = false;
        while let Some(&byte) = bytes.get(end) {
            if byte.is_ascii_digit() {
                end += 1;
            } else if byte == b'.' && !seen_dot {
                seen_dot = true;
                end += 1;
            } else if (byte == b'e' || byte == b'E')
                && bytes
                    .get(end + 1)
                    .is_some_and(|next| next.is_ascii_digit() || *next == b'-' || *next == b'+')
            {
                end += 2;
                while bytes.get(end).is_some_and(u8::is_ascii_digit) {
                    end += 1;
                }
                break;
            } else {
                break;
            }
        }
        let text = std::str::from_utf8(&bytes[begin..end]).unwrap_or_default();
        let value =
            finite(text).ok_or_else(|| format!("expected a finite number at byte {begin}"))?;
        self.at = end;
        Ok(value)
    }

    /// An arc flag: a single `0` or `1`, which may run into the next number.
    fn flag(&mut self) -> Result<bool, String> {
        self.skip_separators();
        match self.text.get(self.at) {
            Some(b'0') => {
                self.at += 1;
                Ok(false)
            }
            Some(b'1') => {
                self.at += 1;
                Ok(true)
            }
            _ => Err(format!("expected an arc flag at byte {}", self.at)),
        }
    }
}

/// An SVG elliptical arc from `from` to `to`, as cubic segments
/// `[c1x, c1y, c2x, c2y, x, y]` (SVG 1.1 appendix F.6).
fn svg_arc(
    from: [f32; 2],
    to: [f32; 2],
    mut rx: f32,
    mut ry: f32,
    rotation: f32,
    large: bool,
    sweep: bool,
) -> Vec<[f32; 6]> {
    if rx < 1e-6 || ry < 1e-6 || (from[0] - to[0]).abs() + (from[1] - to[1]).abs() < 1e-6 {
        return vec![[from[0], from[1], to[0], to[1], to[0], to[1]]];
    }
    let (sin, cos) = rotation.sin_cos();
    let dx = (from[0] - to[0]) / 2.0;
    let dy = (from[1] - to[1]) / 2.0;
    let x1 = cos * dx + sin * dy;
    let y1 = -sin * dx + cos * dy;
    let lambda = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
    if lambda > 1.0 {
        let scale = lambda.sqrt();
        rx *= scale;
        ry *= scale;
    }
    let numerator = (rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1).max(0.0);
    let denominator = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let mut k = (numerator / denominator.max(1e-12)).sqrt();
    if large == sweep {
        k = -k;
    }
    let cx1 = k * rx * y1 / ry;
    let cy1 = -k * ry * x1 / rx;
    let cx = cos * cx1 - sin * cy1 + (from[0] + to[0]) / 2.0;
    let cy = sin * cx1 + cos * cy1 + (from[1] + to[1]) / 2.0;
    let angle = |ux: f32, uy: f32, vx: f32, vy: f32| {
        let dot = ux * vx + uy * vy;
        let len = ((ux * ux + uy * uy) * (vx * vx + vy * vy)).sqrt();
        let mut a = (dot / len.max(1e-12)).clamp(-1.0, 1.0).acos();
        if ux * vy - uy * vx < 0.0 {
            a = -a;
        }
        a
    };
    let start = angle(1.0, 0.0, (x1 - cx1) / rx, (y1 - cy1) / ry);
    let mut delta = angle(
        (x1 - cx1) / rx,
        (y1 - cy1) / ry,
        (-x1 - cx1) / rx,
        (-y1 - cy1) / ry,
    );
    if !sweep && delta > 0.0 {
        delta -= std::f32::consts::TAU;
    } else if sweep && delta < 0.0 {
        delta += std::f32::consts::TAU;
    }
    let count = (delta.abs() / std::f32::consts::FRAC_PI_2).ceil().max(1.0) as usize;
    let step = delta / count as f32;
    let k = 4.0 / 3.0 * (step / 4.0).tan();
    let map = |x: f32, y: f32| {
        [
            cos * rx * x - sin * ry * y + cx,
            sin * rx * x + cos * ry * y + cy,
        ]
    };
    let mut out = Vec::with_capacity(count);
    let mut a = start;
    for _ in 0..count {
        let b = a + step;
        let (s0, c0) = a.sin_cos();
        let (s1, c1) = b.sin_cos();
        let p1 = map(c0 - k * s0, s0 + k * c0);
        let p2 = map(c1 + k * s1, s1 - k * c1);
        let p3 = map(c1, s1);
        out.push([p1[0], p1[1], p2[0], p2[1], p3[0], p3[1]]);
        a = b;
    }
    // Land exactly on the endpoint.
    if let Some(last) = out.last_mut() {
        last[4] = to[0];
        last[5] = to[1];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PaintOp, PathVerb, ResolvedPaint};

    fn record(script: &str, size: [f32; 2], state: PaintState) -> crate::PaintRecording {
        let script = PaintScript::from_json_str(script).expect("a valid script");
        crate::custom_paint::record(
            &script,
            nana_ui_core::builtin_theme_arc(nana_ui_core::ThemeMode::Dark).as_ref(),
            size,
            state,
            &|_, _, _| crate::TextSize::default(),
        )
    }

    #[test]
    fn a_script_replays_onto_the_context_at_the_node_size() {
        let recording = record(
            r##"[
                {"op": "fill", "path": [["rect", 0, 0, "100%", "50% - 2"]], "paint": "accent"},
                {"op": "stroke", "path": "M0 0 h10 v10 z", "paint": "#ff000080", "width": 2,
                 "dash": [3, 1], "phase": "over"},
                {"op": "fill", "path": "M0 0 L1 1 Z", "paint": "surface", "when": {"hovered": true}}
            ]"##,
            [80.0, 40.0],
            PaintState::default(),
        );
        let [PaintOp::FillPath { path, paint }] = &recording.behind_children[..] else {
            panic!("{recording:?}");
        };
        assert!(path.verbs().contains(&PathVerb::LineTo([80.0, 18.0])));
        assert_eq!(
            *paint,
            ResolvedPaint::Solid(nana_ui_core::SemanticPalette::dark().accent.as_rgba_array())
        );
        let [PaintOp::StrokePath { stroke, paint, .. }] = &recording.over_children[..] else {
            panic!("{recording:?}");
        };
        assert_eq!(stroke.dash.as_ref(), &[3.0, 1.0]);
        assert_eq!(*paint, ResolvedPaint::Solid([1.0, 0.0, 0.0, 128.0 / 255.0]));
        let hovered = record(
            r##"[{"op": "fill", "path": "M0 0 L1 1 Z", "paint": "surface", "when": {"hovered": true}}]"##,
            [10.0, 10.0],
            PaintState {
                hovered: true,
                ..PaintState::default()
            },
        );
        assert_eq!(hovered.behind_children.len(), 1);
    }

    #[test]
    fn malformed_input_fails_closed_instead_of_hanging_or_panicking() {
        // A number after closepath has no command to repeat.
        assert!(PaintScript::from_json_str(r#"[{"op":"fill","path":"M0 0 Z 5 5"}]"#).is_err());
        assert_eq!(parse_css_color("#éa"), None);
        assert_eq!(parse_css_color("#+f+f+f"), None);
        assert!(parse_len(&Value::from("NaN")).is_err());
        assert!(parse_len(&Value::from("inf% + 1")).is_err());
        assert!(parse_svg_path("M0 0 L1e99 0").is_err());
        assert!(PaintScript::from_json_str(r#"[{"op":"opacity","value":1e39}]"#).is_err());
    }

    #[test]
    fn a_runaway_arc_sweep_and_a_sub_pixel_dash_stay_bounded() {
        let recording = record(
            r##"{"hit": "painted", "commands": [
                {"op": "fill", "path": [["arc", 50, 50, 10, 0, 1e30]], "paint": "accent"},
                {"op": "stroke", "path": "M0 90 H1000", "paint": "text", "width": 4,
                 "dash": [0.00001, 0.00001]}
            ]}"##,
            [1000.0, 100.0],
            PaintState::default(),
        );
        let PaintOp::FillPath { path, .. } = &recording.behind_children[0] else {
            panic!("{recording:?}");
        };
        // A sweep past a full turn draws one turn, as Canvas does.
        assert!(path.verbs().len() <= 6, "{}", path.verbs().len());
        // Too many dashes to cut: hit as the solid stroke it draws as.
        assert!(recording.contains([500.0, 90.0], [1000.0, 100.0]));
        assert!(!recording.contains([500.0, 80.0], [1000.0, 100.0]));
    }

    #[test]
    fn restore_pops_the_clips_pushed_since_save_and_default_follows_the_transform() {
        let recording = record(
            r##"{"hit": "painted", "commands": [
                {"op": "save"},
                {"op": "pushClip", "path": [["rect", 0, 0, 10, 10]]},
                {"op": "restore"},
                {"op": "translate", "x": 50},
                {"op": "drawDefault"}
            ]}"##,
            [40.0, 40.0],
            PaintState::default(),
        );
        assert!(matches!(
            recording.behind_children[..],
            [PaintOp::PushClip { .. }, PaintOp::PopClip, ..]
        ));
        assert!(recording.contains([60.0, 20.0], [40.0, 40.0]));
        assert!(!recording.contains([20.0, 20.0], [40.0, 40.0]));

        // What an invisible layer holds is not there to hit either.
        let hidden = record(
            r##"{"hit": "painted", "commands": [
                {"op": "pushLayer", "opacity": 0},
                {"op": "fill", "path": [["rect", 0, 0, 10, 10]], "paint": "text"},
                {"op": "popLayer"}
            ]}"##,
            [40.0, 40.0],
            PaintState::default(),
        );
        assert!(!hidden.contains([5.0, 5.0], [40.0, 40.0]));
    }

    #[test]
    fn every_css_blend_mode_parses_and_typos_are_reported() {
        for mode in [
            "normal",
            "multiply",
            "screen",
            "overlay",
            "darken",
            "lighten",
            "color-dodge",
            "color-burn",
            "hard-light",
            "soft-light",
            "difference",
            "exclusion",
            "hue",
            "saturation",
            "color",
            "luminosity",
        ] {
            let script = format!(r#"[{{"op": "blend", "mode": "{mode}"}}]"#);
            assert!(PaintScript::from_json_str(&script).is_ok(), "{mode}");
        }
        let err = |script: &str| PaintScript::from_json_str(script).unwrap_err();
        assert!(err(r#"[{"op": "fill", "path": "M0 0", "colour": "red"}]"#).contains("colour"));
        assert!(
            err(r#"[{"op": "text", "rect": [0,0,1,1], "text": "a", "wrap": "yes"}]"#)
                .contains("wrap")
        );
        assert!(err(r#"{"commands": [], "hitt": "painted"}"#).contains("hitt"));
        assert!(
            err(r#"[{"op": "fill", "path": {"d": "M0 0", "rule": "evenodd"}, "paint": "text"}]"#)
                .contains("rule")
        );
    }

    #[test]
    fn a_script_reaches_set_transform_colour_mixes_relative_radii_and_a_hit_path() {
        let recording = record(
            r##"[
                {"op": "setTransform", "matrix": [1, 0, 0, 1, 5, 0]},
                {"op": "fill", "path": [["roundedRect", 0, 0, "100%", "100%", "50%"]],
                 "paint": {"mix": ["accent", "surface", 0.5]}},
                {"op": "fill", "path": [["arc", "50%", "50%", "25%", 0, 6.3]],
                 "paint": {"alpha": ["text", 0.25]}}
            ]"##,
            [40.0, 20.0],
            PaintState::default(),
        );
        let [
            PaintOp::SetTransform(t),
            PaintOp::FillPath { path: pill, paint },
            PaintOp::FillPath { path: disc, .. },
        ] = &recording.behind_children[..]
        else {
            panic!("{recording:?}");
        };
        assert_eq!(*t, [1.0, 0.0, 0.0, 1.0, 5.0, 0.0]);
        let palette = nana_ui_core::SemanticPalette::dark();
        let (a, b) = (
            palette.accent.as_rgba_array(),
            palette.surface.as_rgba_array(),
        );
        let ResolvedPaint::Solid(mixed) = paint else {
            panic!("{paint:?}");
        };
        assert!((mixed[0] - (a[0] + b[0]) / 2.0).abs() < 0.01, "{mixed:?}");
        // "50%" of the 20px short side: a pill, round at both ends.
        assert!(!pill.contains([0.5, 0.5]) && pill.contains([20.0, 10.0]));
        // A 5px (25% of 20) circle about the centre.
        let bounds = disc.bounds().unwrap();
        assert!((bounds.width - 10.0).abs() < 0.5, "{bounds:?}");

        let script = PaintScript::from_json_str(
            r#"{"commands": [], "hit": {"path": [["ellipse", 0, 0, "100%", "100%"]]}}"#,
        )
        .unwrap();
        assert_eq!(script.hit_test([20.0, 10.0], [40.0, 20.0]), Some(true));
        assert_eq!(script.hit_test([1.0, 1.0], [40.0, 20.0]), Some(false));
    }

    #[test]
    fn svg_path_data_parses_relative_smooth_and_arc_commands() {
        let segments =
            parse_svg_path("m10 10 l5 0 h5 v5 s5 5 10 0 t5 5 a5 5 0 0 1 10 0 z").unwrap();
        let path = ScriptPath {
            segments: segments.into(),
            fill_rule: FillRule::NonZero,
        }
        .build([0.0, 0.0]);
        let verbs = path.verbs();
        assert_eq!(verbs[0], PathVerb::MoveTo([10.0, 10.0]));
        assert_eq!(verbs[1], PathVerb::LineTo([15.0, 10.0]));
        assert_eq!(verbs[2], PathVerb::LineTo([20.0, 10.0]));
        assert_eq!(verbs[3], PathVerb::LineTo([20.0, 15.0]));
        // The arc ends where it says it does.
        let arc_end = verbs
            .iter()
            .rev()
            .find_map(|verb| match verb {
                PathVerb::CubicTo(_, _, to) => Some(*to),
                _ => None,
            })
            .unwrap();
        assert!(
            (arc_end[0] - 45.0).abs() < 1e-3 && (arc_end[1] - 20.0).abs() < 1e-3,
            "{arc_end:?}"
        );
        assert_eq!(verbs.last(), Some(&PathVerb::Close));
    }

    #[test]
    fn the_same_script_is_the_same_painter_and_bad_input_is_reported() {
        let a = crate::NodePainter::new(
            PaintScript::from_json_str(r##"[{"op": "drawDefault"}]"##).unwrap(),
        );
        let b = crate::NodePainter::new(
            PaintScript::from_json_str(r##"[{"op": "drawDefault"}]"##).unwrap(),
        );
        let c = crate::NodePainter::new(
            PaintScript::from_json_str(r##"[{"op": "popClip"}]"##).unwrap(),
        );
        assert_eq!(a, b);
        assert_ne!(a, c);
        let error = PaintScript::from_json_str(
            r##"[{"op": "fill", "path": "M0 0", "paint": "no-such-role"}]"##,
        )
        .unwrap_err();
        assert!(
            error.contains("command 0") && error.contains("no-such-role"),
            "{error}"
        );
        assert!(parse_css_color("rgb(255, 0, 0)") == Some([1.0, 0.0, 0.0, 1.0]));
        assert!(parse_css_color("#0f08") == Some([0.0, 1.0, 0.0, 8.0 / 15.0]));
    }

    #[test]
    fn an_icon_is_named_and_an_unknown_name_is_reported() {
        let recording = record(
            r##"[{"op": "icon", "rect": [0, 0, 16, 16], "name": "file", "paint": "accent"}]"##,
            [16.0, 16.0],
            PaintState::default(),
        );
        assert!(matches!(
            &recording.behind_children[..],
            [PaintOp::Icon { icon, .. }] if *icon == nana_ui_core::Icon::File
        ));
        let error = PaintScript::from_json_str(
            r##"[{"op": "icon", "rect": [0, 0, 1, 1], "name": "zzz"}]"##,
        )
        .unwrap_err();
        assert!(error.contains("zzz"), "{error}");
    }
}
