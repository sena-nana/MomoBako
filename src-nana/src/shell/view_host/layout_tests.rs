//! 间距、行高、圆角和边框宽度的回归：写在样式上的这几样数值都要挂主题档位。
//!
//! 只写数值的样式在装上的主题换刻度时不跟着变，在自带令牌的控件上还会被令牌盖掉。这里把每个验收场景挂起来，
//! 逐个节点看作者写的样式：内外边距、间距、行高、圆角、边框宽度写了非零数值的，要么同一字段挂着
//! `layout.theme` 的主题表达式，要么节点上有对应的档位意图（`gap_step`、`padding_step`、控件内边距档、
//! 面板内边距档、`radius` / `corner_radii` / `shape`）。相对行高（字号的倍数）跟着字号档走，不算写死。
//! 源码里也不许直接写这几样数值：辅助函数在 `theme_map/tiers.rs`。

use std::path::Path;

use nana_ui_core::{LayoutStyle, LengthSpec, LineHeightSpec, ThemeExpr};

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
fn authored_fields(style: &nana_ui::runtime::NodeStyle) -> Vec<Field> {
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

/// 允许写死数值的节点：组件类型（如 `nana.form-field`）或装配路径里的一段，加字段名，再加原因。
/// 只收 NanaUI 组件内部、壳层管不到的样式。
const ALLOWED: &[(&str, &str, &str)] = &[
    (
        "nana.form-field",
        "padding_top",
        "FormField 给标签留的位置是 NanaUI 按标签行高加间距算的",
    ),
    (
        "nana.select",
        "border_width",
        "Select 的发丝边是 NanaUI 写的 HAIRLINE",
    ),
    (
        "nana.popover",
        "border_width",
        "Popover 面板的发丝边是 NanaUI 写的 HAIRLINE",
    ),
    (
        "nana.path-field",
        "gap",
        "PathField 输入框和浏览按钮之间的间距是 NanaUI 写的",
    ),
    (
        "nana.chip",
        "border_radius",
        "Chip 的胶囊圆角是 NanaUI 写的",
    ),
    (
        "nana.status-badge",
        "border_radius",
        "StatusBadge 的胶囊圆角是 NanaUI 写的",
    ),
    (
        "export-target/",
        "border_radius",
        "SegmentedControl 的选项圆角是 NanaUI 按轨道算的",
    ),
    (
        "export-target/",
        "padding_left",
        "SegmentedControl 的选项内边距是 NanaUI 按轨道算的",
    ),
    (
        "export-target/",
        "padding_right",
        "SegmentedControl 的选项内边距是 NanaUI 按轨道算的",
    ),
];

/// 每个验收场景里，写了非零间距、行高、圆角或边框宽度的节点都挂主题档位。
#[test]
#[ignore = "三个区域的档位改动合并以前，别的区域还有写死数值；合并后去掉"]
fn authored_layout_numbers_follow_theme_tiers() {
    let mut loose = Vec::new();
    for (scene, model) in crate::shell::acceptance_gap_models() {
        let harness = ShellHarness::mount(model);
        let document = harness.document();
        let context = document.context();
        let world = context.world();
        for id in world.document_order(document.document()) {
            let Some(style) = world.node_style(id) else {
                continue;
            };
            let path = context.assembly_path(id).unwrap_or_default();
            let kind = world
                .component_type(id)
                .map(|kind| kind.as_str().to_owned())
                .unwrap_or_default();
            for field in authored_fields(&style) {
                let allowed = ALLOWED.iter().any(|(part, name, _)| {
                    (kind == *part || path.contains(part)) && *name == field.name
                });
                if !field.backed && !allowed {
                    loose.push(format!(
                        "{scene} {path} [{kind}]: {} 写 {} 没挂主题档位",
                        field.name, field.value
                    ));
                }
            }
        }
    }
    loose.sort();
    loose.dedup();
    assert!(
        loose.is_empty(),
        "{} 处间距、行高、圆角或边框宽度写死了数值：\n{}",
        loose.len(),
        loose.join("\n")
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
/// 写 0 不算；值本身是从档位折出来的（同一行用了 `tiers::`）也不算，那种写法旁边要另挂主题表达式，
/// 有没有挂由上面挂场景的回归检查。
#[test]
#[ignore = "三个区域的档位改动合并以前，别的区域还有写死数值；合并后去掉"]
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
            if !in_tests
                && !zero
                && !derived
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
