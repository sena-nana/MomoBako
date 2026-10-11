//! 停用的图标按钮照 Vue `button:disabled { opacity: 0.45 }` 只淡一次。
//!
//! NanaUI 的 `IconButton` 停用时只给按种类配色的按钮整枚淡到 50%（线性空间合成）；`colors_from_style` 的按钮
//! 照自己写的 `style.interaction.disabled` 画，节点不透明度照写的来。壳层里会停用的图标按钮都走后一种：
//! 侧栏标题工具、添加资源库表单和对话框的关闭位在样式里写好和所在底色在 sRGB 里混出的停用色，播放条的
//! 传输按钮由自绘按 `PaintState::disabled` 混；节点都不再整体淡化。

use nana_ui::runtime::{Entity, IconButton, SemanticColorRole, StableNodeId};
use nana_ui_core::SemanticColorMix;

use crate::shell::sidebar::PopoverMode;
use crate::shell::view_harness::ShellHarness;
use crate::shell::{ShellPage, ShellViewModel};
use crate::theme_map::DISABLED_OPACITY;

fn scene(name: &str) -> ShellViewModel {
    crate::shell::acceptance_gap_models()
        .into_iter()
        .find(|(scene, _)| *scene == name)
        .unwrap_or_else(|| panic!("没有场景 {name}"))
        .1
}

fn opacity(harness: &ShellHarness, id: StableNodeId) -> Option<f32> {
    harness.document().context().world().node_style(id).and_then(|style| style.layout.opacity)
}

fn disabled(harness: &ShellHarness, id: StableNodeId) -> bool {
    harness.document().context().world().accessibility(id).is_some_and(|state| state.disabled)
}

/// 节点停用时的字色混合。
fn disabled_mix(harness: &ShellHarness, id: StableNodeId) -> Option<SemanticColorMix> {
    harness.document().context().world().node_style(id).and_then(|style| style.interaction.disabled.foreground_mix)
}

/// 弱色叉号或图标盖在 `--bg-elev` 上按 0.45 混。
fn faint_over_surface() -> Option<SemanticColorMix> {
    Some(SemanticColorMix::new(SemanticColorRole::Faint, SemanticColorRole::Surface, DISABLED_OPACITY))
}

/// 对话框卡片里的关闭位（标题栏也有一个窗口的「关闭」）。
fn dialog_close(harness: &ShellHarness) -> StableNodeId {
    let surface = harness.keyed("dialog-surface").expect("对话框");
    let world = harness.document().context().world();
    harness
        .nodes()
        .into_iter()
        .find(|node| node.label.as_deref() == Some("关闭") && world.is_descendant_or_self(node.id, surface))
        .expect("导出对话框的关闭位")
        .id
}

/// 停用的图标按钮：不透明度为空（NanaUI 没有再淡一层），停用色是在侧栏或卡片底色上混好的弱色。
fn assert_mixed_over_surface(harness: &ShellHarness, id: StableNodeId, label: &str) {
    assert!(disabled(harness, id), "「{label}」应该停用");
    assert_eq!(opacity(harness, id), None, "「{label}」只淡一次，节点不再整体淡化");
    assert_eq!(disabled_mix(harness, id), faint_over_surface(), "「{label}」的停用色按 0.45 和底色在 sRGB 里混");
}

/// 传输按钮是 `IconButton`，自绘按停用状态混好颜色；提示照 Vue 只有循环模式有。
#[test]
fn disabled_player_buttons_are_dimmed_only_by_their_painter() {
    let harness = ShellHarness::mount(scene("live-files"));
    let context = harness.document().context();
    for label in ["列表循环", "上一首", "播放", "下一首", "当前队列"] {
        let id = harness.node(label);
        assert!(disabled(&harness, id), "没有条目时「{label}」停用");
        assert_eq!(opacity(&harness, id), None, "「{label}」的停用色由自绘混好，节点不再整体淡化");
        let (own_colors, painted, tooltip) = context
            .read(Entity::<IconButton>::from_stable_id(id), |button| {
                (button.colors_from_style, button.style.painter.is_some(), button.tooltip.as_ref().map(|tip| tip.label.to_string()))
            })
            .unwrap_or_else(|_| panic!("「{label}」应该是 IconButton"));
        assert!(own_colors && painted, "「{label}」颜色照写的来、由自绘画");
        let expected = (label == "列表循环").then(|| label.to_string());
        assert_eq!(tooltip, expected, "「{label}」的提示照 Vue：只有循环模式写了 title");
    }
}

#[test]
fn disabled_sidebar_tools_mix_their_ink_over_the_sidebar() {
    let harness = ShellHarness::mount(ShellViewModel::for_page(ShellPage::EmptyRepository));
    for label in ["新建播放集", "在当前目录新建文件夹", "刷新文件夹树", "新建智能文件夹"] {
        assert_mixed_over_surface(&harness, harness.node(label), label);
    }
}

#[test]
fn busy_close_buttons_mix_their_ink_over_their_card() {
    let mut model = scene("live-files");
    model.sidebar.popover = PopoverMode::BackendForm;
    model.sidebar.submitting = true;
    let harness = ShellHarness::mount(model);
    let close = harness.node("关闭添加资源库");
    assert_mixed_over_surface(&harness, close, "关闭添加资源库");

    let mut harness = ShellHarness::mount(scene("export-dialog"));
    let close = dialog_close(&harness);
    assert!(!disabled(&harness, close), "没在导出时关闭位能点");
    let resting = harness.document().context().world().node_style(close).and_then(|style| style.foreground);
    assert_eq!(resting, Some(SemanticColorRole::Faint), "关闭位平时是弱色叉号，不被种类配色换成次要文字色");
    harness.model.files.export.busy = true;
    harness.model.mark_surface_dirty();
    harness.sync();
    harness.flush();
    assert_mixed_over_surface(&harness, close, "关闭");
}
