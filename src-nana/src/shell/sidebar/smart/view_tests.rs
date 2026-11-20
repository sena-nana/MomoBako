//! 智能文件夹对话框在真实视图里的回归：名称框按回车提交（Vue 名称输入框的 `@keydown.enter.prevent`）。
//! 保存进行中名称和筛选仍可改，「创建」和「取消」禁用，回车不再另发一次。

use nana_ui::runtime::StableNodeId;

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
    assert!(
        harness.model.sidebar.take_effects().is_empty(),
        "名称为空时回车不提交"
    );

    harness.type_text("高评分");
    settle(&mut harness);
    assert_eq!(harness.model.sidebar.smart_draft.name, "高评分");
    harness.press_key("Enter");
    let effects = harness.model.sidebar.take_effects();
    assert!(
        effects.iter().any(|effect| matches!(effect, SidebarEffect::CreateSmartFolder { repo_id } if repo_id == REPO)),
        "回车应该提交新建：{effects:?}"
    );
    assert!(
        harness.model.sidebar.smart_draft.busy,
        "提交以后对话框处理中"
    );
}

fn node_disabled(harness: &ShellHarness, id: StableNodeId) -> bool {
    harness
        .nodes()
        .into_iter()
        .any(|node| node.id == id && node.disabled)
}

/// 归约输入框打字发出的消息。
fn settle_typed(harness: &mut ShellHarness) {
    let typed = harness.take_messages();
    harness.settle(typed);
}

/// 「创建」已经发出、服务还没回来时，名称和关键词照 Vue 仍可改。
/// 「创建」和「取消」禁用；再按回车或再点「创建」不另发一次，已经发出的请求仍用提交当时的名字。
#[test]
fn smart_folder_name_stays_editable_while_the_save_is_in_flight() {
    let mut harness = ShellHarness::mount(scene("smart-folder-dialog"));
    harness.finish_motion();
    let name = harness.input("名称");
    harness.focus(name);
    harness.type_text("高评分");
    settle_typed(&mut harness);
    harness.press_key("Enter");
    let first = harness.model.sidebar.take_effects();
    assert!(
        first.iter().any(|effect| matches!(effect, SidebarEffect::CreateSmartFolder { repo_id } if repo_id == REPO)),
        "第一次创建应交给侧栏：{first:?}"
    );
    assert_eq!(
        harness.model.sidebar.smart_create_request(REPO).name,
        "高评分"
    );
    assert!(harness.model.sidebar.smart_draft.busy, "创建已经交给服务");

    assert!(!node_disabled(&harness, name), "保存中名称输入框仍可改");
    let query = harness.input("关键词");
    assert!(!node_disabled(&harness, query), "保存中关键词输入框仍可改");
    let parent = harness.keyed("smart-field-parent").expect("父级");
    assert!(!node_disabled(&harness, parent), "保存中父级下拉仍可改");
    let save = harness.keyed("smart-create-submit").expect("创建");
    let cancel = harness.keyed("smart-dialog-cancel").expect("取消");
    assert!(node_disabled(&harness, save), "保存中「创建」应禁用");
    assert!(node_disabled(&harness, cancel), "保存中「取消」应禁用");

    harness.focus(name);
    harness.type_text("2");
    settle_typed(&mut harness);
    let edited = harness.value(name);
    assert!(
        edited.contains('2') && edited != "高评分",
        "打进去的字应留在名称框：{edited}"
    );
    assert_eq!(harness.model.sidebar.smart_draft.name, edited);
    assert_eq!(
        harness.model.sidebar.smart_create_request(REPO).name,
        "高评分",
        "已经发出的请求仍用提交当时的名字"
    );
    assert!(
        harness.model.sidebar.take_effects().is_empty(),
        "打字不应另发创建"
    );

    harness.focus(query);
    harness.type_text("封面");
    settle_typed(&mut harness);
    assert_eq!(
        harness.model.sidebar.smart_draft.query, "封面",
        "关键词也应写进草稿"
    );
    assert!(
        harness
            .model
            .sidebar
            .smart_create_request(REPO)
            .filter
            .query
            .is_none(),
        "已经发出的请求不应带上后来填的关键词"
    );

    harness.focus(name);
    harness.press_key("Enter");
    assert!(
        harness.model.sidebar.take_effects().is_empty(),
        "保存中回车不应另发创建"
    );
    assert!(harness.model.sidebar.smart_draft.open);
    assert_eq!(
        harness.model.sidebar.smart_draft.name, edited,
        "没提交成，名字留着"
    );
    assert!(harness.model.sidebar.smart_draft.busy);

    let (x, y) = harness.center(save);
    let clicks = harness.click_messages(x, y);
    harness.settle(clicks);
    assert!(
        harness.model.sidebar.take_effects().is_empty(),
        "保存中再点「创建」不应另发创建"
    );
    assert_eq!(harness.value(name), edited);
    assert!(harness.model.sidebar.smart_draft.busy);
}
