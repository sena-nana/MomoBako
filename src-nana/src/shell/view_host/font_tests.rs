//! 字号和字重的回归：写在样式上的字号、字重必须是节点真正用上的，字重只取主题的四档。
//!
//! 按钮、勾选框这类控件自带主题字号、字重令牌，解析时令牌盖过数值和字重台阶；输入框和下拉框的字又按控件尺寸档画。
//! 只改数值不生效，画出来的仍是控件的主题档。这里把每个验收场景挂起来，逐个节点比对：
//! 自己画字的控件看画出来的字号、字重，其余看解析后的，都要等于样式上写的。
//! 源码里也不许直接写数值字重：控件走 `theme_map::control_weight`，文字走 `TextWeight` 台阶。

use std::path::Path;

use nana_ui::runtime::WidgetPaintOp;
use nana_ui_core::{TextWeight, TypographyTokens};

use crate::shell::ShellViewModel;
use crate::shell::view_harness::ShellHarness;

/// 主题四档字重在默认主题下的数值。
const WEIGHT_TIERS: [TextWeight; 4] = [
    TextWeight::Regular,
    TextWeight::Medium,
    TextWeight::Semibold,
    TextWeight::Bold,
];

/// 验收场景，外加一份有时间的歌词：有时间的歌词行是按钮，验收场景里没有。
fn scenes() -> Vec<(&'static str, ShellViewModel)> {
    let mut scenes = crate::shell::acceptance_gap_models();
    let mut lyrics = scenes
        .iter()
        .find(|(scene, _)| *scene == "preview-audio")
        .map(|(_, model)| model.clone())
        .expect("有音频预览场景");
    lyrics.inspect.lyrics =
        crate::shell::inspect_lyrics::parse_lrc("[00:00.00]开头\n[00:01.00]中段\n作词：桃");
    scenes.push(("timed-lyrics", lyrics));
    scenes
}

/// 每个场景里，写了字号或字重的节点都按写的画字或排字；用上的字重都在主题四档上。
#[test]
fn authored_fonts_are_the_fonts_nodes_use() {
    let tiers = WEIGHT_TIERS.map(|tier| TypographyTokens::DEFAULT.weight(tier));
    let mut dead = Vec::new();
    for (scene, model) in scenes() {
        let harness = ShellHarness::mount(model);
        let document = harness.document();
        let context = document.context();
        let world = context.world();
        for id in world.document_order(document.document()) {
            let Some(style) = world.node_style(id) else {
                continue;
            };
            let painted = world.widget_paint(id).and_then(|paint| {
                paint
                    .owns_node_text
                    .then(|| {
                        paint.ops.iter().find_map(|op| match &op.op {
                            WidgetPaintOp::Text(text) => Some((text.font_size, text.font_weight)),
                            _ => None,
                        })
                    })
                    .flatten()
            });
            let computed = world.computed_style(id).map(|style| &style.inherited);
            let path = || context.assembly_path(id).unwrap_or_default();
            if let Some(authored) = style.layout.font_size {
                let used = painted
                    .map(|(size, _)| size)
                    .or_else(|| computed.map(|style| style.font_size));
                if used.is_some_and(|used| (used - authored).abs() > 0.01) {
                    dead.push(format!(
                        "{scene} {}: 字号写 {authored}，用 {used:?}",
                        path()
                    ));
                }
            }
            let used = painted
                .and_then(|(_, weight)| weight)
                .or_else(|| computed.and_then(|style| style.font_weight));
            let authored = [
                style.layout.font_weight,
                style
                    .type_weight
                    .map(|weight| TypographyTokens::DEFAULT.weight(weight)),
            ];
            for authored in authored.into_iter().flatten() {
                if used.is_some_and(|used| used != authored) {
                    dead.push(format!(
                        "{scene} {}: 字重写 {authored}，用 {used:?}",
                        path()
                    ));
                }
            }
            if used.is_some_and(|used| !tiers.contains(&used)) {
                dead.push(format!("{scene} {}: 字重 {used:?} 不在主题档上", path()));
            }
        }
    }
    assert!(dead.is_empty(), "字号或字重没生效：\n{}", dead.join("\n"));
}

/// 源码里不直接写数值字重：只有 `theme_map.rs` 把字重档折成默认主题的数值。
#[test]
fn shell_sources_do_not_write_numeric_weights() {
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
            || relative == Path::new("theme_map.rs")
            || relative == Path::new("shell/view_host/font_tests.rs")
        {
            continue;
        }
        let source = std::fs::read_to_string(&path).expect("读源码");
        for (index, line) in source.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("");
            if code.contains("font_weight") {
                found.push(format!(
                    "{}:{}: {}",
                    relative.display(),
                    index + 1,
                    line.trim()
                ));
            }
        }
    }
    assert!(
        found.is_empty(),
        "数值字重写进了样式：\n{}",
        found.join("\n")
    );
}
