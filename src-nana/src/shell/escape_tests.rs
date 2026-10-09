//! 全局 Escape 的回归：对话框、弹层、右键菜单和筛选栏上按 Escape，关掉的层和改动前一致。
//!
//! 改动前 Escape 只登记在按钮和输入框上，这里的用例都把焦点放在这样的控件上（对话框自己的输入框、
//! 按钮，或标题栏搜索框），两种做法关掉的是同一层。控件自己处理掉的 Escape（下拉框收起选项）
//! 不再关别的层。最后一个用例是新做法的行为：焦点不在按钮或输入框上时也照样关。

use nana_ui::runtime::{component_descriptors, Entity, Select, StableNodeId};

use crate::shell::view_harness::ShellHarness;
use crate::shell::{AdminMessage, GapMessage, ShellMessage, ShellViewModel, SidebarMessage};

fn scene(name: &str) -> ShellViewModel {
    crate::shell::acceptance_gap_models()
        .into_iter()
        .find(|(scene, _)| *scene == name)
        .unwrap_or_else(|| panic!("没有场景 {name}"))
        .1
}

/// 焦点放在名为 `label` 的输入框上，按一下 Escape。
fn escape_from_input(harness: &mut ShellHarness, label: &str) {
    let input = harness.input(label);
    harness.focus(input);
    harness.press_escape();
}

#[test]
fn escape_closes_a_dialog_from_its_own_input() {
    let cases: [(&str, &str, fn(&ShellViewModel) -> bool); 3] = [
        ("folder-create-dialog", "文件夹名称", |model| model.sidebar.folder_dialog.open),
        ("smart-folder-dialog", "名称", |model| model.sidebar.smart_draft.open),
        ("playlist-create-dialog", "名称", |model| model.playlist_dialog_open),
    ];
    for (name, field, open) in cases {
        let mut harness = ShellHarness::mount(scene(name));
        assert!(open(&harness.model) && harness.content_roots().0.is_some(), "{name} 应该开着对话框");
        escape_from_input(&mut harness, field);
        assert!(!open(&harness.model), "{name}：Escape 应该关掉对话框");
        assert!(harness.content_roots().0.is_none(), "{name}：关掉后浮层槽位应该空着");
        harness.assert_same_as_fresh_mount();
    }
}

/// 删除资源库对话框不归 Escape 管，只能点取消：按了也还开着。
#[test]
fn escape_leaves_the_repository_delete_dialog_open() {
    let mut harness = ShellHarness::mount(scene("repo-delete-dialog"));
    let cancel = harness.node("取消");
    harness.focus(cancel);
    harness.press_escape();
    assert!(harness.model.workspace.delete_dialog_open(), "删除资源库对话框不该被 Escape 关掉");
    assert!(harness.content_roots().0.is_some());
    harness.assert_same_as_fresh_mount();
}

/// 仓库弹层和任务弹层：焦点在标题栏搜索框时 Escape 关掉弹层。
#[test]
fn escape_closes_popovers() {
    let mut harness = ShellHarness::mount(scene("repo-switcher"));
    assert_ne!(harness.model.sidebar.popover, Default::default(), "场景里应该开着仓库弹层");
    escape_from_input(&mut harness, "全局搜索");
    assert_eq!(harness.model.sidebar.popover, Default::default(), "Escape 应该关掉仓库弹层");
    assert!(harness.content_roots().0.is_none());

    let mut harness = ShellHarness::mount(scene("live-files-plain"));
    harness.apply(ShellMessage::Admin(AdminMessage::ToggleTaskPopover));
    harness.flush();
    assert!(harness.model.admin.popover_open);
    escape_from_input(&mut harness, "全局搜索");
    assert!(!harness.model.admin.popover_open, "Escape 应该关掉任务弹层");
    assert!(harness.content_roots().0.is_none());
    harness.assert_same_as_fresh_mount();
}

/// 文件右键菜单和文件夹右键菜单：Escape 关掉菜单，别的状态不动。
#[test]
fn escape_closes_context_menus() {
    let mut harness = ShellHarness::mount(scene("live-menu"));
    assert!(harness.model.files.entry_menu.is_some());
    escape_from_input(&mut harness, "全局搜索");
    assert!(harness.model.files.entry_menu.is_none(), "Escape 应该关掉文件右键菜单");
    assert!(harness.content_roots().0.is_none());

    let mut harness = ShellHarness::mount(scene("live-files-plain"));
    harness.apply(ShellMessage::Sidebar(SidebarMessage::Gap(GapMessage::OpenFolderMenu {
        path: "assets".into(),
        label: "assets".into(),
        x: 120.0,
        y: 200.0,
    })));
    harness.flush();
    assert!(harness.model.sidebar.folder_menu.is_some(), "文件夹右键菜单没有打开");
    assert!(harness.content_roots().0.is_some());
    escape_from_input(&mut harness, "全局搜索");
    assert!(harness.model.sidebar.folder_menu.is_none(), "Escape 应该关掉文件夹右键菜单");
    assert!(harness.content_roots().0.is_none());
    harness.assert_same_as_fresh_mount();
}

/// 筛选栏不归 Escape 管：焦点在筛选输入框里按 Escape，筛选栏和输入都还在。
#[test]
fn escape_in_the_filter_bar_closes_nothing() {
    let mut harness = ShellHarness::mount(scene("filter-bar"));
    assert!(harness.model.inspect.filter_bar_open);
    let field = harness.input_within("workspace-filter-bar");
    harness.focus(field);
    harness.press_escape();
    assert!(harness.model.inspect.filter_bar_open, "Escape 不该关掉筛选栏");
    assert!(harness.keyed("workspace-filter-bar").is_some());
    harness.assert_same_as_fresh_mount();
}

/// 下拉框的选项开着时，Escape 只收起选项：运行时已经处理掉这次按键，开着的任务弹层不跟着关。
#[test]
fn escape_consumed_by_a_select_leaves_the_popover_open() {
    let mut harness = ShellHarness::mount(scene("filter-bar"));
    harness.apply(ShellMessage::Admin(AdminMessage::ToggleTaskPopover));
    harness.flush();
    let select = first_select(&harness);
    harness.focus(select);
    let entity = Entity::<Select>::from_stable_id(select);
    let document = &mut harness.window.document;
    document.context_mut().update_component(entity, |select, _| select.opened = true).expect("展开下拉框");
    harness.flush();
    assert!(!harness.press_escape(), "下拉框处理掉的 Escape 不该再发关闭消息");
    let opened = harness.document().context().read(entity, |select| select.opened).expect("下拉框");
    assert!(!opened, "Escape 应该收起下拉框的选项");
    assert!(harness.model.admin.popover_open, "任务弹层不该跟着关");
}

/// 新做法：焦点不在按钮或输入框上（这里没有焦点）时 Escape 也关掉最上面一层。
#[test]
fn escape_closes_the_top_layer_without_focus() {
    let mut harness = ShellHarness::mount(scene("folder-create-dialog"));
    assert_eq!(harness.focused(), None);
    assert!(harness.press_escape());
    assert!(!harness.model.sidebar.folder_dialog.open);
    assert!(!harness.press_escape(), "没有能关的层时不发消息");
}

/// 文档里第一个下拉框。
fn first_select(harness: &ShellHarness) -> StableNodeId {
    let document = harness.document();
    let world = document.context().world();
    world
        .document_order(document.document())
        .into_iter()
        .find(|id| world.component_type(*id).is_some_and(|kind| kind.as_str() == component_descriptors::SELECT.type_id))
        .expect("场景里有下拉框")
}
