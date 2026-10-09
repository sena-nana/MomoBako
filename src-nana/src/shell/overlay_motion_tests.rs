//! 对话框声明式开合的回归：对话框靠 `open` 开合，关上以后连退场一起留在浮层层里，放完才卸，卸掉以后
//! 点击落到下面；宿主自己关掉对话框时不发消息、不改 ViewModel，ViewModel 还要它开着时下一帧重新打开；
//! 对话框换成另一个时，焦点按「关旧的、开新的」的先后还回去。

use nana_ui::runtime::{Dialog, Entity, OverlayHost, StableNodeId};

use super::dialog_tests::scene;
use super::popover_tests::{blocks, layer_takes_pointer, shell_and_layer};
use crate::shell::input::InputMessage;
use crate::shell::view_harness::ShellHarness;
use crate::shell::{GapMessage, ShellMessage, SidebarMessage, WindowAction};

/// 对话框节点的 `open` 字段。
fn dialog_open(harness: &ShellHarness, surface: StableNodeId) -> bool {
    harness.document().context().read(Entity::<Dialog>::from_stable_id(surface), |dialog| dialog.open).expect("对话框")
}

fn active(harness: &ShellHarness, host: StableNodeId) -> Option<StableNodeId> {
    harness.document().context().world().overlay_host(host).and_then(|state| state.active)
}

#[test]
fn a_closed_dialog_stays_in_the_layer_until_its_exit_has_played() {
    let mut harness = ShellHarness::mount(scene("folder-create-dialog"));
    let host = harness.keyed("dialog-host").expect("对话框宿主");
    let surface = harness.keyed("dialog-surface").expect("对话框");
    assert!(dialog_open(&harness, surface), "对话框应该声明成开着");
    assert_eq!(active(&harness, host), Some(surface), "宿主照 open 打开了对话框");
    assert!(harness.keyed("dialog-activated").is_none(), "不再借结构块激活");
    let block = blocks(&harness);
    assert_eq!(block.len(), 1);
    assert!(layer_takes_pointer(&harness));

    assert!(harness.press_escape(), "Escape 发关闭消息");
    assert!(!harness.model.sidebar.folder_dialog.open);
    assert_eq!(harness.content_roots().0, None, "现在没有显示的浮层");
    assert!(harness.overlay_leaving(), "关上的对话框还在放退场");
    assert_eq!(blocks(&harness), block, "放退场时这块留在浮层层里");
    assert!(!dialog_open(&harness, surface), "对话框声明成关上");
    assert_eq!(active(&harness, host), Some(surface), "退场放完以前宿主还留着它");
    for _ in 0..6 {
        harness.frame();
        assert_eq!(blocks(&harness), block, "退场没放完不该卸");
    }
    assert!(layer_takes_pointer(&harness), "放退场时和 Vue 一样挡着点击");

    harness.finish_motion();
    assert!(!harness.overlay_leaving());
    assert!(blocks(&harness).is_empty(), "退场放完以后卸掉这块");
    assert!(!harness.document().context().world().contains(surface), "对话框节点随这块卸掉");
    assert!(!layer_takes_pointer(&harness), "浮层层空了，点击落到下面");
    harness.take_messages();
    let trash = harness.node("回收站");
    let (x, y) = harness.center(trash);
    assert!(!harness.click_messages(x, y).is_empty(), "对话框卸掉以后点得到侧栏");
    harness.assert_same_as_fresh_mount();
}

/// 宿主自己关掉对话框（被别的浮层顶替、被停放）时发 `DialogToggled { open: false }`：这不是用户的
/// 意思，不发消息、不改 ViewModel；ViewModel 还要它开着，下一帧照样声明打开。
#[test]
fn a_dialog_the_host_closes_on_its_own_opens_again_without_a_message() {
    let mut harness = ShellHarness::mount(scene("folder-create-dialog"));
    // 第一次布局时播放条量到宽度发的消息，和对话框无关。
    harness.take_messages();
    let host = harness.keyed("dialog-host").expect("对话框宿主");
    let surface = harness.keyed("dialog-surface").expect("对话框");
    let context = harness.window.document.context_mut();
    assert!(context.dismiss_overlay(Entity::<OverlayHost>::from_stable_id(host)).expect("宿主关掉对话框"));
    harness.flush();
    assert!(harness.take_messages().is_empty(), "宿主自己关掉对话框不该发消息");
    assert!(harness.model.sidebar.folder_dialog.open, "ViewModel 还要它开着");
    assert!(!dialog_open(&harness, surface), "宿主关掉以后 open 写回假");
    harness.finish_motion();
    harness.frame();
    assert!(dialog_open(&harness, surface), "下一帧照 ViewModel 重新声明打开");
    assert_eq!(active(&harness, host), Some(surface), "对话框重新打开");
    assert_eq!(harness.keyed("dialog-surface"), Some(surface), "还是原来那个对话框");

    // 停放：这块被拿出浮层层，宿主放开对话框；放回来以后照样重新打开。
    let block = blocks(&harness);
    let (_, layer) = shell_and_layer(&harness);
    let context = harness.window.document.context_mut();
    context.reconcile_children(layer, &[]).expect("停放这块");
    harness.flush();
    assert!(harness.take_messages().is_empty(), "停放不该发消息");
    assert!(!dialog_open(&harness, surface), "停放以后 open 写回假");
    let context = harness.window.document.context_mut();
    context.reconcile_children(layer, &block).expect("放回来");
    harness.frame();
    assert_eq!(active(&harness, host), Some(surface), "放回来以后重新打开");
    harness.assert_same_as_fresh_mount();
}

/// 一个对话框直接换成另一个（关闭确认压在新建文件夹上面，取消后新建文件夹回来）：同一次刷新里
/// 先关旧的、再开新的，每个对话框记下的「打开前的焦点」都是上一个还回去的那个，最后回到标题栏搜索框。
#[test]
fn switching_dialogs_hands_focus_back_in_order() {
    let mut harness = ShellHarness::mount(scene("live-files-plain"));
    let search = harness.input("全局搜索");
    harness.focus(search);
    harness.apply(ShellMessage::Sidebar(SidebarMessage::Gap(GapMessage::OpenFolderCreate(String::new()))));
    harness.flush();
    let folder = harness.content_roots().0.expect("新建文件夹");
    let focus_in_overlay = |harness: &ShellHarness| {
        let focused = harness.focused().expect("焦点");
        harness.document().context().world().is_descendant_or_self(focused, harness.content_roots().0.expect("浮层"))
    };
    assert!(focus_in_overlay(&harness), "新建文件夹打开后焦点在对话框里");

    harness.model.settings.close_behavior = "confirm".into();
    harness.apply(ShellMessage::WindowAction(WindowAction::Close));
    harness.flush();
    assert!(harness.model.input.pending_close, "关闭行为是确认时应该先确认");
    assert_ne!(harness.content_roots().0, Some(folder), "关闭确认顶替了新建文件夹");
    assert!(focus_in_overlay(&harness), "关闭确认打开后焦点在确认框里");

    harness.apply(ShellMessage::Input(InputMessage::ConfirmCloseAnswer(false)));
    harness.flush();
    assert!(harness.model.sidebar.folder_dialog.open, "取消关闭后新建文件夹还开着");
    assert!(focus_in_overlay(&harness), "新建文件夹回来以后焦点在对话框里");

    assert!(harness.press_escape());
    assert!(!harness.model.sidebar.folder_dialog.open);
    assert_eq!(harness.focused(), Some(search), "关掉最后一个对话框，焦点回到打开第一个之前的搜索框");
    harness.assert_same_as_fresh_mount();
}
