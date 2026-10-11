//! 键盘打开的菜单：菜单按钮上的方向键、文件卡片和文件夹行上的菜单键打开菜单，焦点落在首项或末项，
//! 上下键在本层的项之间走、首尾相接；Escape 关掉菜单以后焦点回到打开它的控件。指针打开的菜单不动焦点。

use nana_ui::runtime::{RovingEdge, SecondaryPress, StableNodeId};
use nana_ui::InputModifiers;

use super::{request, MenuEdge, MenuFocusRequest, MenuSurface};
use crate::shell::sidebar::{backend_options, PopoverMode};
use crate::shell::view_harness::ShellHarness;
use crate::shell::{FilesMessage, GapMessage, ShellMessage, ShellViewModel, SidebarMessage};

fn scene(name: &str) -> ShellViewModel {
    crate::shell::acceptance_gap_models()
        .into_iter()
        .find(|(scene, _)| *scene == name)
        .unwrap_or_else(|| panic!("没有场景 {name}"))
        .1
}

/// 消息的种类，断言按键发了什么用。
fn kind(message: &ShellMessage) -> &'static str {
    match message {
        ShellMessage::Sidebar(SidebarMessage::OpenRepositorySwitcher) => "open-switcher",
        ShellMessage::Files(FilesMessage::OpenImportMenu) => "open-import",
        ShellMessage::Files(FilesMessage::OpenEntryMenu { .. }) => "open-entry",
        ShellMessage::Sidebar(SidebarMessage::Gap(GapMessage::OpenFolderMenu { .. })) => "open-folder",
        ShellMessage::Sidebar(SidebarMessage::Gap(GapMessage::CloseFolderMenu)) => "close-folder",
        ShellMessage::Sidebar(SidebarMessage::Gap(GapMessage::OpenFolderCreate(_))) => "create-folder",
        ShellMessage::FocusMenu(_) => "focus-menu",
        _ => "other",
    }
}

/// 焦点放在无障碍名为 `label` 的控件上，按 `key`。返回这次按键发出的消息的种类。
fn press_on(harness: &mut ShellHarness, label: &str, key: &'static str) -> Vec<&'static str> {
    let id = harness.node(label);
    harness.focus(id);
    press(harness, key)
}

/// 在现在的焦点上按 `key`：发出的消息逐条归约、同步；同步里挪焦点又发的消息（分组展开）照生产下一帧
/// 那样接着归约，最后刷新一帧。返回这次按键发出的消息的种类。
fn press(harness: &mut ShellHarness, key: &'static str) -> Vec<&'static str> {
    press_with(harness, key, InputModifiers::default())
}

/// 同 [`press`]，按着 `modifiers`。
fn press_with(harness: &mut ShellHarness, key: &'static str, modifiers: InputModifiers) -> Vec<&'static str> {
    let messages = harness.key_messages_with(key, modifiers);
    let sent = messages.iter().map(kind).collect();
    harness.settle(messages);
    sent
}

/// 焦点所在控件的无障碍名。
fn focused(harness: &ShellHarness) -> Option<String> {
    harness.focused_label()
}

#[test]
fn arrows_on_the_repository_header_open_the_switcher_onto_either_end() {
    let mut harness = ShellHarness::mount(scene("live-files-plain"));
    assert_eq!(press_on(&mut harness, "资源库 · 默认资源库", "ArrowDown"), ["open-switcher", "focus-menu"]);
    assert_eq!(harness.model.sidebar.popover, PopoverMode::Switcher, "ArrowDown 打开切换弹层");
    assert_eq!(focused(&harness).as_deref(), Some("切换资源库 默认资源库"), "ArrowDown 聚焦首项");
    press(&mut harness, "ArrowDown");
    assert_eq!(focused(&harness).as_deref(), Some("添加资源库"), "上下键在项之间走");
    harness.press_escape();
    assert_eq!(harness.model.sidebar.popover, PopoverMode::Closed);
    assert_eq!(focused(&harness).as_deref(), Some("资源库 · 默认资源库"), "关掉以后焦点回到仓库头");

    press_on(&mut harness, "资源库 · 默认资源库", "ArrowUp");
    assert_eq!(focused(&harness).as_deref(), Some("删除当前资源库"), "ArrowUp 聚焦末项");
    press(&mut harness, "ArrowDown");
    assert_eq!(focused(&harness).as_deref(), Some("切换资源库 默认资源库"), "越过末项回到首项");
}

#[test]
fn the_repository_popover_keeps_keyboard_focus_through_its_pages() {
    let mut harness = ShellHarness::mount(scene("live-files-plain"));
    press_on(&mut harness, "资源库 · 默认资源库", "ArrowDown");
    press(&mut harness, "ArrowDown");
    assert_eq!(focused(&harness).as_deref(), Some("添加资源库"));
    press(&mut harness, "Enter");
    assert_eq!(harness.model.sidebar.popover, PopoverMode::AddMenu);
    let first = backend_options(&harness.model.admin.plugins).into_iter().find(|option| option.enabled).expect("有可用的来源").label;
    assert_eq!(focused(&harness), Some(first), "换到添加菜单，焦点落在它的首项");
    harness.press_escape();
    assert_eq!(harness.model.sidebar.popover, PopoverMode::Closed);
    assert_eq!(focused(&harness).as_deref(), Some("资源库 · 默认资源库"), "整个弹层关掉才还给仓库头");
}

#[test]
fn arrows_on_import_open_the_menu_without_toggling_it_shut() {
    let mut harness = ShellHarness::mount(scene("live-files-plain"));
    assert_eq!(press_on(&mut harness, "导入", "ArrowDown"), ["open-import", "focus-menu"]);
    assert!(harness.model.files.import_open, "ArrowDown 打开导入菜单");
    assert_eq!(focused(&harness).as_deref(), Some("从文件夹导入"));
    press_on(&mut harness, "导入", "ArrowUp");
    assert!(harness.model.files.import_open, "菜单开着时再按不收起");
    assert_eq!(focused(&harness).as_deref(), Some("从 Eagle 导入"), "Eagle 的两项藏着，末项是「从 Eagle 导入」");
    harness.press_escape();
    assert!(!harness.model.files.import_open);
    assert_eq!(focused(&harness).as_deref(), Some("导入"), "收起的菜单里不留焦点");
}

#[test]
fn the_menu_key_on_a_card_opens_its_menu_onto_the_first_item() {
    let mut harness = ShellHarness::mount(scene("live-files-plain"));
    assert_eq!(press_on(&mut harness, "cover.png", "ContextMenu"), ["open-entry", "focus-menu"]);
    assert!(harness.model.files.entry_menu.is_some(), "菜单键打开条目菜单");
    assert_eq!(focused(&harness).as_deref(), Some("预览"));
    press(&mut harness, "ArrowUp");
    assert_eq!(focused(&harness).as_deref(), Some("删除"), "越过首项到末项");
    press(&mut harness, "ArrowUp");
    press(&mut harness, "ArrowUp");
    assert_eq!(focused(&harness).as_deref(), Some("缩略图"), "分组也是一项");
    assert_eq!(harness.model.files.menu_branch.open(), Some("thumbnail"), "焦点进到分组就展开子菜单");
    press(&mut harness, "ArrowUp");
    assert_eq!(focused(&harness).as_deref(), Some("复制到…"), "上下键不走进子菜单");
    harness.press_escape();
    assert!(harness.model.files.entry_menu.is_none());
    assert_eq!(focused(&harness).as_deref(), Some("cover.png"), "关掉以后焦点回到卡片");
}

/// 文件夹行的右键菜单和条目菜单一样能用键盘：菜单键和 Shift+F10 打开、焦点落在首项，上下键首尾相接，
/// Escape 关掉以后焦点回到文件夹行，Enter 执行这一项。
#[test]
fn the_menu_key_on_a_folder_row_drives_the_folder_menu() {
    let mut harness = ShellHarness::mount(scene("live-files-plain"));
    let folder = harness.node("assets");
    assert!(harness.keyed("folder-row-assets").is_some_and(|row| row == folder), "第一个 assets 是侧栏的文件夹行");
    assert_eq!(press_on(&mut harness, "assets", "ContextMenu"), ["open-folder", "focus-menu"]);
    assert!(harness.model.sidebar.folder_menu.as_ref().is_some_and(|menu| menu.path == "assets"), "菜单键打开这一行的菜单");
    assert_eq!(focused(&harness).as_deref(), Some("打开"), "焦点落在首项");
    press(&mut harness, "ArrowUp");
    assert_eq!(focused(&harness).as_deref(), Some("删除"), "越过首项到末项");
    press(&mut harness, "ArrowDown");
    assert_eq!(focused(&harness).as_deref(), Some("打开"), "越过末项回到首项");
    harness.press_escape();
    assert!(harness.model.sidebar.folder_menu.is_none(), "Escape 关掉菜单");
    assert_eq!(harness.focused(), Some(folder), "关掉以后焦点回到文件夹行");

    let shift = InputModifiers { shift: true, ..InputModifiers::default() };
    assert_eq!(press_with(&mut harness, "F10", shift), ["open-folder", "focus-menu"], "Shift+F10 也打开");
    assert_eq!(focused(&harness).as_deref(), Some("打开"));
    press(&mut harness, "ArrowDown");
    assert_eq!(focused(&harness).as_deref(), Some("新建子文件夹"));
    assert_eq!(press(&mut harness, "Enter"), ["close-folder", "create-folder"], "Enter 关掉菜单、执行这一项");
    assert!(harness.model.sidebar.folder_menu.is_none());
    assert!(harness.model.sidebar.folder_dialog.open, "新建子文件夹的对话框打开");
    assert_eq!(harness.model.sidebar.folder_dialog.parent, "assets", "在这一行下面新建");
}

/// 键盘打开的菜单不困住焦点：焦点从文件夹菜单回到页面、在仓库头上打开切换列表时两层同时开着，
/// 显示的是仓库弹层，Escape 也先关它，再关文件夹菜单。
#[test]
fn escape_closes_the_shown_menu_first_when_two_are_open() {
    let mut harness = ShellHarness::mount(scene("live-files-plain"));
    press_on(&mut harness, "assets", "ContextMenu");
    assert!(harness.model.sidebar.folder_menu.is_some());
    press_on(&mut harness, "资源库 · 默认资源库", "ArrowDown");
    assert_eq!(harness.model.sidebar.popover, PopoverMode::Switcher);
    assert!(harness.model.sidebar.folder_menu.is_some(), "文件夹菜单还开着，排在仓库弹层后面");
    assert!(harness.keyed("folder-context-menu").is_none(), "同时只显示一层");
    harness.press_escape();
    assert_eq!(harness.model.sidebar.popover, PopoverMode::Closed, "Escape 先关显示着的仓库弹层");
    assert!(harness.model.sidebar.folder_menu.is_some());
    assert!(harness.keyed("folder-context-menu").is_some(), "仓库弹层关掉后文件夹菜单露出来");
    harness.press_escape();
    assert!(harness.model.sidebar.folder_menu.is_none());
}

#[test]
fn only_the_keyboard_asks_for_menu_focus() {
    let press = |keyboard, focus| SecondaryPress { target: StableNodeId::new(1).expect("编号"), x: 0.0, y: 0.0, keyboard, focus };
    assert_eq!(MenuEdge::of_press(&press(false, None)), None, "右键不动焦点");
    assert_eq!(MenuEdge::of_press(&press(true, None)), Some(MenuEdge::First), "菜单键和 Shift+F10 聚焦首项");
    assert_eq!(MenuEdge::of_press(&press(true, Some(RovingEdge::Start))), Some(MenuEdge::First));
    assert_eq!(MenuEdge::of_press(&press(true, Some(RovingEdge::End))), Some(MenuEdge::Last));
}

#[test]
fn a_request_for_a_closed_menu_is_dropped() {
    let mut model = scene("live-files-plain");
    request(&mut model, MenuFocusRequest { surface: MenuSurface::Import, edge: MenuEdge::First });
    assert_eq!(model.menu_focus.latest(), None, "导入菜单没开，不记请求");
    model.reduce(ShellMessage::Files(FilesMessage::OpenImportMenu));
    model.reduce(ShellMessage::FocusMenu(MenuFocusRequest { surface: MenuSurface::Import, edge: MenuEdge::Last }));
    assert_eq!(model.menu_focus.latest().map(|(_, request)| request.edge), Some(MenuEdge::Last));
    model.reduce(ShellMessage::Sidebar(SidebarMessage::OpenRepositorySwitcher));
    assert!(MenuSurface::RepositorySwitcher.is_open(&model));
}
