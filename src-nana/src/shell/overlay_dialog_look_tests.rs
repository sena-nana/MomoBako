//! 对话框外观的回归：每个对话框照 Vue 自己的宽度类给宽度、危险对话框用 NanaUI 的危险口气、Vue 标题前
//! 有图标的放进标题图标槽；装上 MomoBako 主题以后卡片按配方距顶 12vh，窄窗口下宽度按 CSS 的
//! `min(..)` 让给窗口。

use nana_ui::icons_tabler::{ALERT_TRIANGLE, COPY, DOWNLOAD, PENCIL};
use nana_ui::runtime::{
    AccessibilityRole, ConfirmDialog, Dialog, Entity, IconGlyph, LengthAtom, LengthSpec, ModalSurface, StableNodeId, ViewportAxis,
};
use nana_ui::theme::ThemeAppearance;
use nana_ui::{DialogSize, Icon};

use super::dialog::MODAL_CARD;
use super::dialog_tests::{cases, mounted, scene};
use crate::appearance::{self, Appearance};
use crate::shell::files::{FileDialog, FilesMessage};
use crate::shell::view_harness::ShellHarness;
use crate::shell::ShellMessage;

/// 一个对话框在 Vue 里的样子：宽度类、是不是危险口气、标题前的图标。
struct Look {
    size: DialogSize,
    danger: bool,
    icon: Option<Icon>,
}

/// `.smart-folder-dialog { width: min(720px, calc(100vw - 40px)) }`。
const SMART_FOLDER: DialogSize = DialogSize::Width(LengthSpec::Min2(
    LengthAtom::Px(720.0),
    LengthAtom::CalcViewport { axis: ViewportAxis::Width, value: 100.0, offset_px: -40.0 },
));

/// 照 Vue 各对话框的模板和样式：`.modal-card` 520，导出 560，智能文件夹 720；处理文件夹、删除
/// 智能文件夹、删除资源库和删除插件是危险口气；复制、硬链接、导出和删除插件标题前有图标。
fn vue_look(name: &str) -> Look {
    let plain = |size| Look { size, danger: false, icon: None };
    match name {
        "folder-delete" | "smart-delete" | "repo-delete-dialog" => Look { size: MODAL_CARD, danger: true, icon: None },
        "plugin-delete" => Look { size: MODAL_CARD, danger: true, icon: Some(ALERT_TRIANGLE) },
        "copy-dialog" | "hardlink-dialog" => Look { size: MODAL_CARD, danger: false, icon: Some(COPY) },
        "export-dialog" => Look { size: DialogSize::capped(560.0, 92.0), danger: false, icon: Some(DOWNLOAD) },
        "smart-folder-dialog" => plain(SMART_FOLDER),
        "folder-create-dialog" | "source-playlist" | "playlist-create-dialog" | "close-confirm" => plain(MODAL_CARD),
        other => panic!("没写 {other} 在 Vue 里的样子"),
    }
}

/// 对话框的宽度、口气和标题图标槽，`Dialog` 和 `ConfirmDialog` 都读。
fn surface_look(harness: &ShellHarness) -> (DialogSize, bool, Option<StableNodeId>) {
    let surface = harness.keyed("dialog-surface").expect("对话框");
    let context = harness.document().context();
    context
        .read(Entity::<Dialog>::from_stable_id(surface), |dialog| (dialog.size, dialog.danger, dialog.slots().title_icon))
        .or_else(|_| {
            context.read(Entity::<ConfirmDialog>::from_stable_id(surface), |dialog| (dialog.size, dialog.danger, dialog.slots().title_icon))
        })
        .expect("对话框节点是 Dialog 或 ConfirmDialog")
}

fn size_and_tone(harness: &ShellHarness) -> (DialogSize, bool) {
    let (size, danger, _) = surface_look(harness);
    (size, danger)
}

/// 标题图标槽里的图标；没有图标时为 `None`。
fn title_icon(harness: &ShellHarness) -> Option<Icon> {
    let (_, _, slot) = surface_look(harness);
    let icon = slot?;
    let context = harness.document().context();
    Some(context.read(Entity::<IconGlyph>::from_stable_id(icon), |glyph| glyph.icon).expect("标题图标是 IconGlyph"))
}

#[test]
fn every_dialog_takes_its_vue_width_tone_and_title_icon() {
    for case in cases() {
        let harness = mounted(&case);
        let look = vue_look(case.name);
        let (size, danger) = size_and_tone(&harness);
        assert_eq!(size, look.size, "{}：宽度不是 Vue 的宽度类", case.name);
        assert_eq!(danger, look.danger, "{}：口气不对", case.name);
        assert_eq!(title_icon(&harness).map(Icon::as_ptr), look.icon.map(Icon::as_ptr), "{}：标题图标不对", case.name);
    }
}

/// 重命名文件是 `.workspace-rename-dialog`：460 宽，标题前一支铅笔。
#[test]
fn rename_dialog_is_narrower_and_has_a_pencil() {
    let mut model = scene("copy-dialog");
    model.reduce(ShellMessage::Files(FilesMessage::OpenDialog(FileDialog::Rename)));
    assert_eq!(model.files.dialog, FileDialog::Rename);
    let harness = ShellHarness::mount(model);
    assert_eq!(size_and_tone(&harness), (DialogSize::capped(460.0, 92.0), false));
    assert_eq!(title_icon(&harness).map(Icon::as_ptr), Some(PENCIL.as_ptr()));
}

/// 装上 MomoBako 主题后卡片的布局盒：距顶 12vh，宽度照 CSS 的 `min(..)` 在窄窗口下让给窗口。
fn card_box(model: crate::shell::ShellViewModel, width: f32, height: f32) -> (f32, f32) {
    let mut harness = ShellHarness::mount_at(model.clone(), width, height);
    let theme = Appearance::for_mode(&model, ThemeAppearance::Light);
    assert!(appearance::install(&mut harness.window.document, theme).is_some(), "主题装不上");
    harness.flush();
    let card = harness
        .nodes()
        .into_iter()
        .find(|node| matches!(node.role, AccessibilityRole::Dialog | AccessibilityRole::AlertDialog))
        .expect("对话框");
    (card.bounds.y, card.bounds.width)
}

#[test]
fn dialog_cards_stand_at_12vh_and_give_way_to_narrow_windows() {
    let widths = [
        // 场景、1200 宽、700 宽：92vw 是 644，100vw - 40 是 660。
        ("copy-dialog", 520.0, 520.0),
        ("export-dialog", 560.0, 560.0),
        ("smart-folder-dialog", 720.0, 660.0),
        ("folder-create-dialog", 520.0, 520.0),
    ];
    for (name, wide, narrow) in widths {
        let (top, width) = card_box(scene(name), 1200.0, 800.0);
        assert!((width - wide).abs() < 0.5, "{name}：1200 宽时卡片 {width}");
        assert!((top - 96.0).abs() < 0.5, "{name}：距顶应该是 12vh（96），实际 {top}");
        let (_, width) = card_box(scene(name), 700.0, 800.0);
        assert!((width - narrow).abs() < 0.5, "{name}：700 宽时卡片 {width}");
    }
    let (top, _) = card_box(scene("copy-dialog"), 960.0, 600.0);
    assert!((top - 72.0).abs() < 0.5, "600 高时距顶应该是 72，实际 {top}");
}
