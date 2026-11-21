//! 控件字号的回归：写在样式上的数值字号必须是节点真正用上的字号。
//!
//! 按钮、输入框这类控件自带主题字号令牌，解析时令牌盖过数值字号；输入框和下拉框的字又按控件尺寸档画。
//! 只改数值的字号不生效，画出来的仍是控件的主题档。这里把每个验收场景挂起来，逐个节点比对：
//! 自己画字的控件看画出来的字号，其余看解析后的字号，都要等于样式上写的数值。

use crate::shell::view_harness::ShellHarness;

/// 每个验收场景里，写了数值字号的节点都按这个字号画字或排字。
#[test]
fn authored_font_sizes_are_the_sizes_nodes_use() {
    let mut dead = Vec::new();
    for (scene, model) in crate::shell::acceptance_gap_models() {
        let harness = ShellHarness::mount(model);
        let document = harness.document();
        let context = document.context();
        let world = context.world();
        for id in world.document_order(document.document()) {
            let Some(authored) = world
                .node_style(id)
                .and_then(|style| style.layout.font_size)
            else {
                continue;
            };
            let painted = world.widget_paint(id).and_then(|paint| {
                paint
                    .owns_node_text
                    .then(|| {
                        paint.ops.iter().find_map(|op| match &op.op {
                            nana_ui::runtime::WidgetPaintOp::Text(text) => Some(text.font_size),
                            _ => None,
                        })
                    })
                    .flatten()
            });
            let used = painted.or_else(|| {
                world
                    .computed_style(id)
                    .map(|style| style.inherited.font_size)
            });
            if used.is_some_and(|used| (used - authored).abs() > 0.01) {
                let path = context.assembly_path(id).unwrap_or_default();
                dead.push(format!("{scene} {path}: 写 {authored}，用 {used:?}"));
            }
        }
    }
    assert!(dead.is_empty(), "数值字号没生效：\n{}", dead.join("\n"));
}
