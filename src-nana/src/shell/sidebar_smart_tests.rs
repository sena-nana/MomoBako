//! 智能文件夹对话框在真实视图里的回归：名称框按回车提交（Vue 名称输入框的 `@keydown.enter.prevent`）。

use crate::shell::sidebar::SidebarEffect;
use crate::shell::view_harness::ShellHarness;
use crate::shell::{ShellMessage, ShellViewModel, SidebarMessage};

/// 验收底子里的仓库。
const REPO: &str = "acceptance-repo";

fn scene(name: &str) -> ShellViewModel {
    crate::shell::acceptance_gap_models()
        .into_iter()
        .find(|(scene, _)| *scene == name)
        .unwrap_or_else(|| panic!("没有场景 {name}"))
        .1
}

/// 归约这段时间里视图发出的消息。
fn settle(harness: &mut ShellHarness) {
    let messages = harness.take_messages();
    harness.settle(messages);
}

/// 新建智能文件夹：名称框里打好名字按回车，和点「创建」一样提交；名称为空时回车不提交。
#[test]
fn enter_in_the_name_field_submits_the_dialog() {
    let mut harness = ShellHarness::mount(scene("live-files-plain"));
    harness.apply(ShellMessage::Sidebar(SidebarMessage::OpenSmartFolderDialog));
    harness.finish_motion();
    let name = harness.input("名称");
    harness.focus(name);
    harness.press_key("Enter");
    assert!(harness.model.sidebar.take_effects().is_empty(), "名称为空时回车不提交");

    harness.type_text("高评分");
    settle(&mut harness);
    assert_eq!(harness.model.sidebar.smart_draft.name, "高评分");
    harness.press_key("Enter");
    let effects = harness.model.sidebar.take_effects();
    assert!(
        effects.iter().any(|effect| matches!(effect, SidebarEffect::CreateSmartFolder { repo_id } if repo_id == REPO)),
        "回车应该提交新建：{effects:?}"
    );
    assert!(harness.model.sidebar.smart_draft.busy, "提交以后对话框处理中");
}
