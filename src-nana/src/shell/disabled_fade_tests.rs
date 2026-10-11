//! 停用的按钮只淡一次。
//!
//! NanaUI 的 `IconButton` 停用时只给按种类配色的按钮整枚淡到 50%；`colors_from_style` 的按钮照自己写的
//! `style.interaction.disabled` 画，节点不透明度照写的来。播放条的传输按钮由自绘按 Vue 的 0.45 在 sRGB 里
//! 和卡片底色混好停用色，宿主节点不能再整体淡化；侧栏的图标按钮颜色照写的来，框架也不再淡化。

use nana_ui::runtime::StableNodeId;

use crate::shell::view_harness::ShellHarness;
use crate::shell::{ShellPage, ShellViewModel};

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

#[test]
fn disabled_player_buttons_are_dimmed_only_by_their_painter() {
    let harness = ShellHarness::mount(scene("live-files"));
    for label in ["列表循环", "上一首", "播放", "下一首", "当前队列"] {
        let id = harness.node(label);
        assert!(disabled(&harness, id), "没有条目时「{label}」停用");
        assert_eq!(opacity(&harness, id), None, "「{label}」的停用色由自绘混好，节点不再整体淡化");
    }
}

#[test]
fn disabled_sidebar_tools_are_not_faded_by_nana() {
    let harness = ShellHarness::mount(ShellViewModel::for_page(ShellPage::EmptyRepository));
    for label in ["新建播放集", "在当前目录新建文件夹", "刷新文件夹树", "新建智能文件夹"] {
        let id = harness.node(label);
        assert!(disabled(&harness, id), "没有资源库时「{label}」停用");
        assert_eq!(opacity(&harness, id), None, "「{label}」颜色照写的来，NanaUI 不再整枚淡化");
    }
}
