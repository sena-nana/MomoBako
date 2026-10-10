//! 文件右键菜单的子菜单，照 Vue `.ctx-menu__slot:hover` / `:focus-within`：
//! 指针移到分组上展开、移开收起，没有延迟；从分组移进子菜单、在子菜单里移动不闪关；
//! 点分组本身不收起，焦点留在分组里时指针移开也留着；Tab 进分组展开、再 Tab 走进子菜单；
//! 禁用的分组不展开。

use nana_ui::{InputModifiers, PointerPhase};

use crate::shell::files::{BranchSource, FilesMessage};
use crate::shell::view_harness::ShellHarness;
use crate::shell::{ShellMessage, WorkspacePanel};

/// 「缩略图」子菜单的第一项和最后一项。
const FIRST_CHILD: &str = "自定义缩略图（选择文件）";
const LAST_CHILD: &str = "刷新缩略图";

/// 打开着 cover.png 右键菜单的文件页。
fn menu() -> ShellHarness {
    let model = crate::shell::acceptance_gap_models()
        .into_iter()
        .find(|(scene, _)| *scene == "live-menu")
        .expect("场景 live-menu")
        .1;
    let mut harness = ShellHarness::mount(model);
    harness.take_messages();
    harness
}

/// 键是 `key` 的节点的中心。
fn point(harness: &ShellHarness, key: &str) -> (f32, f32) {
    harness.center(harness.keyed(key).unwrap_or_else(|| panic!("菜单缺少 {key}")))
}

/// 无障碍名是 `label` 的节点的中心。
fn labeled(harness: &ShellHarness, label: &str) -> (f32, f32) {
    harness.center(harness.node(label))
}

/// 子菜单的一次进出：分组、来源、进还是出。
type BranchEvent = (String, BranchSource, bool);

/// 归约并同步这些消息，返回其中子菜单的进出。
fn settle(harness: &mut ShellHarness, messages: Vec<ShellMessage>) -> Vec<BranchEvent> {
    let events = messages
        .iter()
        .filter_map(|message| match message {
            ShellMessage::Files(FilesMessage::MenuBranchChanged { branch, source, inside }) => Some((branch.clone(), *source, *inside)),
            _ => None,
        })
        .collect();
    if !messages.is_empty() {
        for message in messages {
            harness.apply(message);
        }
        harness.flush();
    }
    events
}

/// 把指针移到 `(x, y)`，返回这一步发出的子菜单进出。
fn hover(harness: &mut ShellHarness, (x, y): (f32, f32)) -> Vec<BranchEvent> {
    harness.pointer(PointerPhase::Move, x, y, InputModifiers::default());
    let messages = harness.take_messages();
    settle(harness, messages)
}

fn event(branch: &str, source: BranchSource, inside: bool) -> BranchEvent {
    (branch.to_string(), source, inside)
}

fn submenu_open(harness: &ShellHarness) -> bool {
    harness.find(LAST_CHILD).is_some()
}

#[test]
fn hovering_a_group_opens_its_submenu_and_leaving_closes_it() {
    let mut harness = menu();
    assert!(!submenu_open(&harness), "子菜单一开始收着");
    let group = point(&harness, "file-menu-thumbnail");

    assert_eq!(hover(&mut harness, group), [event("thumbnail", BranchSource::Pointer, true)]);
    assert!(submenu_open(&harness), "指针移到分组上没有展开子菜单");

    let rename = point(&harness, "file-menu-rename");
    assert_eq!(hover(&mut harness, rename), [event("thumbnail", BranchSource::Pointer, false)]);
    assert!(!submenu_open(&harness), "移到别的行没有收起子菜单");

    hover(&mut harness, group);
    assert!(submenu_open(&harness));
    assert_eq!(hover(&mut harness, (1150.0, 760.0)), [event("thumbnail", BranchSource::Pointer, false)]);
    assert!(!submenu_open(&harness), "移出菜单没有收起子菜单");
    assert!(harness.model.files.entry_menu.is_some(), "悬停进出不关菜单");
    harness.assert_same_as_fresh_mount();
}

/// 从分组横着一像素一像素移进子菜单（越过父菜单的内边距和描边），再在子菜单里竖着移过每一行、
/// 行距和内边距：每一步都还在分组这一行里，不发离开，子菜单一直开着。
#[test]
fn moving_between_a_group_and_its_submenu_does_not_flicker() {
    let mut harness = menu();
    let (x, y) = point(&harness, "file-menu-thumbnail");
    hover(&mut harness, (x, y));
    let (first_x, first_y) = labeled(&harness, FIRST_CHILD);
    assert!(first_x > x, "子菜单在分组右边");
    let mut px = x;
    while px <= first_x {
        let events = hover(&mut harness, (px, y));
        assert!(events.is_empty() && submenu_open(&harness), "移到 ({px}, {y}) 时子菜单动了：{events:?}");
        px += 1.0;
    }
    let (_, last_y) = labeled(&harness, LAST_CHILD);
    let mut py = first_y - 14.0 - 4.0;
    while py <= last_y + 14.0 + 4.0 {
        let events = hover(&mut harness, (first_x, py));
        assert!(events.is_empty() && submenu_open(&harness), "移到 ({first_x}, {py}) 时子菜单动了：{events:?}");
        py += 1.0;
    }
    let (back_x, back_y) = point(&harness, "file-menu-thumbnail");
    assert!(hover(&mut harness, (back_x, back_y)).is_empty(), "从子菜单回到分组不该发进出");
    assert!(submenu_open(&harness));
}

/// 点分组本身不收起，也不发别的消息；按下时焦点落在分组上，指针移开以后子菜单照 `:focus-within`
/// 留着；焦点移到别的行才收起。
#[test]
fn a_clicked_group_keeps_its_submenu_until_focus_leaves() {
    let mut harness = menu();
    let group = point(&harness, "file-menu-thumbnail");
    hover(&mut harness, group);
    let clicked = harness.click_messages(group.0, group.1);
    assert!(
        clicked.iter().all(|message| matches!(message, ShellMessage::Files(FilesMessage::MenuBranchChanged { .. }))),
        "点分组只该报进出：{} 条消息",
        clicked.len()
    );
    assert_eq!(settle(&mut harness, clicked), [event("thumbnail", BranchSource::Focus, true)], "按下时焦点没有落进分组");
    assert!(submenu_open(&harness), "点分组收起了子菜单");

    let rename = point(&harness, "file-menu-rename");
    assert_eq!(hover(&mut harness, rename), [event("thumbnail", BranchSource::Pointer, false)]);
    assert!(submenu_open(&harness), "焦点还在分组里，指针移开不该收起");

    let rename = harness.keyed("file-menu-rename").expect("重命名");
    harness.focus(rename);
    let messages = harness.take_messages();
    assert_eq!(settle(&mut harness, messages), [event("thumbnail", BranchSource::Focus, false)]);
    assert!(!submenu_open(&harness), "焦点离开分组以后子菜单还开着");
}

/// 键盘：焦点进分组就展开；Tab 接着走进子菜单的各项，子菜单留着；走出子菜单就收起。
#[test]
fn tab_opens_a_group_and_walks_into_its_submenu() {
    let mut harness = menu();
    let group = harness.keyed("file-menu-thumbnail").expect("缩略图");
    harness.focus(group);
    let messages = harness.take_messages();
    assert_eq!(settle(&mut harness, messages), [event("thumbnail", BranchSource::Focus, true)]);
    assert!(submenu_open(&harness), "焦点进分组没有展开子菜单");

    let first = harness.node(FIRST_CHILD);
    let tabbed = harness.key_messages("Tab");
    assert!(settle(&mut harness, tabbed).is_empty(), "从分组 Tab 进子菜单不该收起");
    assert_eq!(harness.focused(), Some(first), "Tab 没有走进子菜单");

    let mut left = Vec::new();
    for _ in 0..4 {
        let tabbed = harness.key_messages("Tab");
        left = settle(&mut harness, tabbed);
        if !left.is_empty() {
            break;
        }
    }
    assert_eq!(left, [event("thumbnail", BranchSource::Focus, false)], "Tab 走出子菜单以后没有收起");
    assert!(!submenu_open(&harness));
}

/// 回收站里「缩略图」是禁用的：指针移上去不展开子菜单。Vue 的 `:hover` 不看按钮禁用，照样弹出、
/// 里面的项还能点。
#[test]
fn a_disabled_group_does_not_open() {
    let mut model = menu().model;
    model.workspace.panel = WorkspacePanel::Trash;
    let mut harness = ShellHarness::mount(model);
    harness.take_messages();
    let group = point(&harness, "file-menu-thumbnail");
    assert!(hover(&mut harness, group).is_empty(), "禁用的分组不该报进出");
    assert!(!submenu_open(&harness), "禁用的分组展开了子菜单");
}
