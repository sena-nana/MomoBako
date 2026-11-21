//! 间距、行高、圆角和边框宽度的回归：写在样式上的这几样数值都要挂主题档位。
//!
//! 只写数值的样式在装上的主题换刻度时不跟着变，在自带令牌的控件上还会被令牌盖掉。这里把每个验收场景挂起来，
//! 逐个节点看作者写的样式：内外边距、间距、行高、圆角、边框宽度写了非零数值的，要么同一字段挂着
//! `layout.theme` 的主题表达式，要么节点上有对应的档位意图（`gap_step`、`padding_step`、控件内边距档、
//! 面板内边距档、`radius` / `corner_radii` / `shape`）。相对行高（字号的倍数）跟着字号走，不算写死。
//! NanaUI 组件自己写的数值另列清单（[`NANA_DEFAULTS`]），每条注明 NanaUI 源码位置。
//! 源码里也不许直接写这几样数值：辅助函数在 `theme_map/tiers.rs`。

use std::collections::BTreeMap;
use std::path::Path;

use nana_ui::runtime::{NodeKind, NodeStyle};
use nana_ui_core::{
    ControlSize, LayoutStyle, LengthSpec, LineHeightSpec, SemanticColorRole as Role, ShapeRole,
    SquareSize, ThemeExpr,
};

use crate::shell::view_harness::ShellHarness;

/// 节点上一个挂不挂主题档位的字段：名字、作者写的非零数值、有没有主题档位。
struct Field {
    name: &'static str,
    value: f32,
    backed: bool,
}

/// 长度字段的像素值：只看 `Px`，0 不算（不是档位，照写 0）。百分比、`Fill` 这类不是间距档。
fn px(spec: Option<LengthSpec>) -> Option<f32> {
    match spec {
        Some(LengthSpec::Px(value)) if value != 0.0 => Some(value),
        _ => None,
    }
}

/// 作者样式里写了非零数值的间距、行高、圆角、边框宽度字段，各自有没有主题档位。
fn authored_fields(style: &NodeStyle) -> Vec<Field> {
    let layout: &LayoutStyle = &style.layout;
    let theme = &layout.theme;
    let some = |expr: &Option<ThemeExpr>| expr.is_some();
    let padding_x = style.padding_step.is_some()
        || style.control_padding_x.is_some()
        || style.surface_padding.is_some()
        || some(&theme.padding);
    let padding_y = style.padding_step.is_some()
        || style.control_padding_y.is_some()
        || style.surface_padding.is_some()
        || some(&theme.padding);
    let corner = style.radius.is_some()
        || style.corner_radii.is_some()
        || style.shape.is_some()
        || some(&theme.border_radius);
    let gap = style.gap_step.is_some() || some(&theme.gap);
    let border = some(&theme.border_width);
    let mut fields = Vec::new();
    let mut push = |name, value: Option<f32>, backed: bool| {
        if let Some(value) = value.filter(|value| *value != 0.0) {
            fields.push(Field {
                name,
                value,
                backed,
            });
        }
    };
    let padding = px(layout.padding);
    push("padding", padding, padding_x && padding_y);
    push(
        "padding_top",
        px(layout.padding_top),
        padding_y || some(&theme.padding_top),
    );
    push(
        "padding_bottom",
        px(layout.padding_bottom),
        padding_y || some(&theme.padding_bottom),
    );
    push(
        "padding_left",
        px(layout.padding_left),
        padding_x || some(&theme.padding_left),
    );
    push(
        "padding_right",
        px(layout.padding_right),
        padding_x || some(&theme.padding_right),
    );
    let margin = some(&theme.margin);
    push("margin", px(layout.margin), margin);
    push(
        "margin_top",
        px(layout.margin_top),
        margin || some(&theme.margin_top),
    );
    push(
        "margin_right",
        px(layout.margin_right),
        margin || some(&theme.margin_right),
    );
    push(
        "margin_bottom",
        px(layout.margin_bottom),
        margin || some(&theme.margin_bottom),
    );
    push(
        "margin_left",
        px(layout.margin_left),
        margin || some(&theme.margin_left),
    );
    push("gap", px(layout.gap), gap);
    push("row_gap", px(layout.row_gap), gap || some(&theme.row_gap));
    push(
        "column_gap",
        px(layout.column_gap),
        gap || some(&theme.column_gap),
    );
    if let Some(LineHeightSpec::Absolute(value)) = layout.line_height {
        push("line_height", Some(value), some(&theme.line_height));
    }
    push("border_radius", layout.border_radius, corner);
    if let Some(radii) = layout.paint.border_radii {
        for spec in radii {
            push("border_radii", px(Some(spec)), corner);
        }
    }
    push("border_width", layout.border_width, border);
    let sides = &layout.border_sides;
    push(
        "border_top_width",
        sides.top_width,
        border || some(&theme.border_top_width),
    );
    push(
        "border_right_width",
        sides.right_width,
        border || some(&theme.border_right_width),
    );
    push(
        "border_bottom_width",
        sides.bottom_width,
        border || some(&theme.border_bottom_width),
    );
    push(
        "border_left_width",
        sides.left_width,
        border || some(&theme.border_left_width),
    );
    fields
}

/// NanaUI 组件自己写的、壳层管不到的数值：组件类型（如 `nana.card`）或元素标签（如 `segmented-option`），
/// 字段，允许的数值，再按需加形状、边色限定，最后是 NanaUI 源码里写这个数的地方（路径相对
/// `crates/nana-ui-runtime/src`）。壳层自己写的样式一条都不收：调用方传进 NanaUI 构造函数的数也算壳层的。
struct NanaDefault {
    node: &'static str,
    fields: &'static [&'static str],
    values: &'static [f32],
    shape: Option<ShapeRole>,
    border: Option<Role>,
    unkeyed: bool,
    #[allow(dead_code)]
    source: &'static str,
}

impl NanaDefault {
    const fn new(
        node: &'static str,
        fields: &'static [&'static str],
        values: &'static [f32],
        source: &'static str,
    ) -> Self {
        Self {
            node,
            fields,
            values,
            shape: None,
            border: None,
            unkeyed: false,
            source,
        }
    }

    const fn shape(mut self, shape: ShapeRole) -> Self {
        self.shape = Some(shape);
        self
    }

    const fn border(mut self, role: Role) -> Self {
        self.border = Some(role);
        self
    }

    /// 只收没有装配路径的节点：NanaUI 在组件里自己搭的，壳层没给它们起键。
    const fn unkeyed(mut self) -> Self {
        self.unkeyed = true;
        self
    }

    fn covers(&self, node: &Node<'_>, style: &NodeStyle, field: &Field) -> bool {
        (node.kind == self.node || node.tag == self.node)
            && (!self.unkeyed || node.path.is_empty())
            && self.fields.contains(&field.name)
            && self.values.contains(&field.value)
            && self.shape.is_none_or(|shape| style.shape == Some(shape))
            && self.border.is_none_or(|role| style.border == Some(role))
    }
}

/// 一个节点的组件类型和元素标签。
struct Node<'a> {
    kind: &'a str,
    tag: &'a str,
    path: &'a str,
}

const PADDING_X: &[&str] = &["padding_left", "padding_right"];

const NANA_DEFAULTS: &[NanaDefault] = &[
    NanaDefault::new(
        "segmented-option",
        PADDING_X,
        &[10.0, 12.0],
        "selection.rs:1066 分段选项的左右内边距按装上的度量算成像素（控件内边距档加 Xxs）",
    ),
    NanaDefault::new(
        "segmented-option",
        &["border_radius"],
        &[3.0],
        "selection.rs:1035 分段选项的圆角按轨道圆角减去轨道内边距算成像素",
    ),
    NanaDefault::new(
        "app-title-bar-controls",
        PADDING_X,
        &[6.0],
        "shell.rs:708 窗口按钮组左右写 `WINDOW_CONTROL_PADDING` 6.0",
    ),
    NanaDefault::new(
        "app-title-bar-controls",
        &["gap"],
        &[2.0],
        "shell.rs:695 窗口按钮之间写 `WINDOW_CONTROL_GAP` 2.0",
    ),
];

/// 每个验收场景里，写了非零间距、行高、圆角或边框宽度的节点都挂主题档位。同一个节点、同一个字段只报一次。
#[test]
fn authored_layout_numbers_follow_theme_tiers() {
    let mut loose = BTreeMap::<String, String>::new();
    for (scene, model) in crate::shell::acceptance_gap_models() {
        let harness = ShellHarness::mount(model);
        let document = harness.document();
        let context = document.context();
        let world = context.world();
        for id in world.document_order(document.document()) {
            let Some(style) = world.node_style(id) else {
                continue;
            };
            let fields = authored_fields(&style);
            if fields.iter().all(|field| field.backed) {
                continue;
            }
            let path = context.assembly_path(id).unwrap_or_default();
            let kind = world
                .component_type(id)
                .map(|kind| kind.as_str().to_owned())
                .unwrap_or_default();
            let tag = match world.node(id).map(|node| node.kind) {
                Some(NodeKind::Element { tag }) => tag,
                _ => String::new(),
            };
            let node = Node {
                kind: &kind,
                tag: &tag,
                path: &path,
            };
            for field in fields {
                let allowed = NANA_DEFAULTS
                    .iter()
                    .any(|default| default.covers(&node, &style, &field));
                if !field.backed && !allowed {
                    loose
                        .entry(format!(
                            "{path} [{kind} {tag}]: {} 写 {} 没挂主题档位",
                            field.name, field.value
                        ))
                        .or_insert_with(|| scene.to_string());
                }
            }
        }
    }
    let report: Vec<_> = loose
        .iter()
        .map(|(line, scene)| format!("{line}（场景 {scene}）"))
        .collect();
    assert!(
        report.is_empty(),
        "{} 处间距、行高、圆角或边框宽度写死了数值：\n{}",
        report.len(),
        report.join("\n")
    );
}

/// 源码里直接写数值间距、圆角、边框宽度的写法。行尾注释不算。
const NUMERIC_PATTERNS: &[&str] = &[
    "border_radius = Some(",
    "border_width = Some(",
    "top_width = Some(",
    "right_width = Some(",
    "bottom_width = Some(",
    "left_width = Some(",
    "padding = Some(LengthSpec::Px(",
    "padding_top = Some(LengthSpec::Px(",
    "padding_right = Some(LengthSpec::Px(",
    "padding_bottom = Some(LengthSpec::Px(",
    "padding_left = Some(LengthSpec::Px(",
    "margin = Some(LengthSpec::Px(",
    "margin_top = Some(LengthSpec::Px(",
    "margin_right = Some(LengthSpec::Px(",
    "margin_bottom = Some(LengthSpec::Px(",
    "margin_left = Some(LengthSpec::Px(",
    "gap = Some(LengthSpec::Px(",
    ".padding_xy(",
    ".radius_px(",
];

/// 源码里不直接写数值间距、圆角、边框宽度：只有 `theme_map/tiers.rs` 把档位折成默认主题的数值。
/// 写 0 不算；往 `layout.theme` 里写主题表达式不算；值本身是从档位折出来的（同一行用了 `tiers::`）也不算，
/// 那种写法旁边要另挂主题表达式，有没有挂由上面挂场景的回归检查。
#[test]
fn shell_sources_do_not_write_numeric_layout_metrics() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = vec![root.clone()];
    let mut found = Vec::new();
    while let Some(path) = files.pop() {
        if path.is_dir() {
            for entry in std::fs::read_dir(&path).expect("读源码目录") {
                files.push(entry.expect("读目录项").path());
            }
            continue;
        }
        let relative = path.strip_prefix(&root).expect("在 src 下");
        if path.extension().is_none_or(|ext| ext != "rs")
            || relative == Path::new("theme_map/tiers.rs")
            || relative == Path::new("shell/view_host/layout_tests.rs")
            || relative.to_string_lossy().ends_with("tests.rs")
            || relative
                .components()
                .any(|part| part.as_os_str() == "tests")
        {
            continue;
        }
        let source = std::fs::read_to_string(&path).expect("读源码");
        let mut in_tests = false;
        for (index, line) in source.lines().enumerate() {
            if line.trim_start().starts_with("#[cfg(test)]") {
                in_tests = true;
            }
            let code = line.split("//").next().unwrap_or("");
            let zero = code.contains("Some(0.0)")
                || code.contains("Px(0.0)")
                || code.contains("(0.0, 0.0)");
            let derived = code.contains("tiers::");
            let theme_expr = code.contains(".theme.");
            if !in_tests
                && !zero
                && !derived
                && !theme_expr
                && NUMERIC_PATTERNS
                    .iter()
                    .any(|pattern| code.contains(pattern))
            {
                found.push(format!(
                    "{}:{}: {}",
                    relative.display(),
                    index + 1,
                    line.trim()
                ));
            }
        }
    }
    found.sort();
    assert!(
        found.is_empty(),
        "{} 处数值间距、圆角或边框宽度写进了样式：\n{}",
        found.len(),
        found.join("\n")
    );
}

/// 控件类组件：尺寸该从控件档、图标按钮档取的节点。
const CONTROL_KINDS: &[&str] = &[
    "nana.button",
    "nana.icon-button",
    "nana.text-input",
    "nana.textarea",
    "nana.select",
    "nana.list-item",
    "nana.sidebar-row",
    "nana.chip",
    "nana.action-menu",
    "nana.action-menu-item",
    "nana.popover",
    "nana.switch",
    "nana.checkbox",
    "nana.segmented",
    "nana.range-field",
    "nana.path-field",
];

/// NanaUI 组件自己写的控件尺寸，写法同 [`NANA_DEFAULTS`]。
const NANA_CONTROL_SIZES: &[NanaDefault] = &[
    NanaDefault::new(
        "nana.icon-button",
        &["width", "height"],
        &[28.0],
        "shell.rs:2331 标题栏窗口按钮的盒子按 nana-ui-core geometry.rs 的 `WINDOW_CONTROL_WIDTH` 28 排",
    )
    .unkeyed(),
    NanaDefault::new(
        "nana.range-field",
        &["min_height"],
        &[28.0],
        "view_components.rs:7909 RangeField 把控件档、行盒和滑块里最高的一个按装上的度量算成像素写进最小高",
    ),
];

/// 控件档在 MomoBako 度量下的像素：小、中、大号控件高和图标按钮边长。
fn control_tiers() -> [f32; 4] {
    let metrics = crate::theme_map::CONTROL_METRICS;
    [
        SquareSize::Control(ControlSize::Small).resolve(metrics),
        SquareSize::Control(ControlSize::Medium).resolve(metrics),
        SquareSize::Control(ControlSize::Large).resolve(metrics),
        SquareSize::IconButton.resolve(metrics),
    ]
}

/// 控件上写了等于某个控件档的宽、高、最小宽高的数值，却没挂档位：该挂 `ThemeToken::Control`、
/// `control_height` 或 `square`。不在档上的尺寸（24、26、30 这类）照写，不查。
fn unbacked_control_sizes(style: &NodeStyle, tiers: &[f32]) -> Vec<Field> {
    let layout: &LayoutStyle = &style.layout;
    let theme = &layout.theme;
    let height = style.control_height.is_some();
    let square = style.square.is_some();
    [
        ("width", layout.width, theme.width.is_some()),
        ("height", layout.height, theme.height.is_some() || height),
        (
            "min_width",
            layout.min_width,
            theme.min_width.is_some() || square,
        ),
        (
            "min_height",
            layout.min_height,
            theme.min_height.is_some() || height || square,
        ),
    ]
    .into_iter()
    .filter_map(|(name, spec, backed)| {
        let value = px(spec)?;
        (!backed && tiers.contains(&value)).then_some(Field {
            name,
            value,
            backed,
        })
    })
    .collect()
}

/// 每个验收场景里，控件的宽高等于控件档或图标按钮档时都挂着档位，装上的主题换度量时跟着变。
#[test]
fn control_sizes_on_tiers_follow_the_metrics() {
    let tiers = control_tiers();
    let mut loose = BTreeMap::<String, String>::new();
    for (scene, model) in crate::shell::acceptance_gap_models() {
        let harness = ShellHarness::mount(model);
        let document = harness.document();
        let context = document.context();
        let world = context.world();
        for id in world.document_order(document.document()) {
            let Some(style) = world.node_style(id) else {
                continue;
            };
            let kind = world
                .component_type(id)
                .map(|kind| kind.as_str().to_owned())
                .unwrap_or_default();
            if !CONTROL_KINDS.contains(&kind.as_str()) {
                continue;
            }
            let tag = match world.node(id).map(|node| node.kind) {
                Some(NodeKind::Element { tag }) => tag,
                _ => String::new(),
            };
            let path = context.assembly_path(id).unwrap_or_default();
            let node = Node {
                kind: &kind,
                tag: &tag,
                path: &path,
            };
            for field in unbacked_control_sizes(&style, &tiers) {
                if NANA_CONTROL_SIZES
                    .iter()
                    .any(|default| default.covers(&node, &style, &field))
                {
                    continue;
                }
                loose
                    .entry(format!(
                        "{path} [{kind} {tag}]: {} 写 {} 没挂控件档",
                        field.name, field.value
                    ))
                    .or_insert_with(|| scene.to_string());
            }
        }
    }
    let report: Vec<_> = loose
        .iter()
        .map(|(line, scene)| format!("{line}（场景 {scene}）"))
        .collect();
    assert!(
        report.is_empty(),
        "{} 处控件尺寸等于控件档却写死了数值：\n{}",
        report.len(),
        report.join("\n")
    );
}
