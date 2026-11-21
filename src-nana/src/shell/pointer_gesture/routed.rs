//! 运行时路由完的输入里，拖动之外要看的两件事：外部文件拖过文件夹，以及点列表空白处。
//!
//! 侧栏文件夹也是放下目标：内部拖动松手把条目移进这个文件夹，外部文件放下则导入到这里，
//! 指针停在侧栏这一行上满 450ms 就展开并打开它。
//! NanaUI 只把走出拖动容差的按下报成 `PointerDragged`，原地点一下列表空白处没有事件，
//! 所以按下和松手在这里认：按在列表框里、不在卡片上，松手前没走出容差，就清空选择。

use nana_ui::{InputPayload, PointerPhase};

use super::targets::{self, EntryKind};
use super::{POINTER_ID, ShellViewModel};
use crate::shell::input::BlankPress;

/// 鼠标按下后走多远算拖动，和 NanaUI 的拖动容差相同。
const DRAG_SLOP: f32 = nana_ui_core::space::XS;

/// 每个路由完的输入：外部拖放的文件夹悬停，和列表空白处的点按。
pub(crate) fn note_routed_input(
    model: &mut ShellViewModel,
    document: &nana_ui::runtime::RuntimeDocument,
    payload: &InputPayload,
) {
    note_external_folder_drag(model, document, payload);
    note_blank_press(model, document, payload);
}

/// 外部文件拖过文件夹时记下路径，离开就清掉。
///
/// 文件列里从一张卡片移到另一张不会再发
/// `FileDropEvent`，所以按每次 `FileDrag` 的坐标问条目拖放目标。侧栏这一行另外记下，文件列的
/// `Leave` 才不会把刚记上的目标清掉；停在侧栏行上时由准备帧把 450ms 走完。
/// 内部拖动自己在算悬停，这里不覆盖。回收站不记。坐标无效只记日志。
fn note_external_folder_drag(
    model: &mut ShellViewModel,
    document: &nana_ui::runtime::RuntimeDocument,
    payload: &InputPayload,
) {
    let InputPayload::FileDrag(drag) = payload else {
        return;
    };
    if model.input.internal_active || model.workspace.panel == super::WorkspacePanel::Trash {
        return;
    }
    let hit = match drag.kind {
        nana_ui::FileDragKind::Hover | nana_ui::FileDragKind::Drop => {
            drag.position.and_then(|(x, y)| {
                if !x.is_finite() || !y.is_finite() {
                    eprintln!("Nana 外部拖放坐标无效，文件夹悬停不更新：{x},{y}");
                    return None;
                }
                targets::folder_at(document, x, y, "")
            })
        }
        nana_ui::FileDragKind::Cancel => None,
    };
    match &hit {
        Some(folder) if model.input.hover_folder.as_deref() != Some(folder.path.as_str()) => {
            model.reduce(super::ShellMessage::Input(
                super::InputMessage::FolderHover {
                    path: folder.path.clone(),
                    trash: false,
                },
            ));
        }
        None => {
            if let Some(path) = model.input.hover_folder.clone() {
                model.reduce(super::ShellMessage::Input(
                    super::InputMessage::FolderLeave(path),
                ));
            }
        }
        Some(_) => {}
    }
    let sidebar = hit.as_ref().is_some_and(|folder| folder.sidebar);
    model.input.hover_on_sidebar = sidebar;
    let opening = sidebar && matches!(drag.kind, nana_ui::FileDragKind::Hover);
    if opening {
        model.input.external_active = true;
        model.input.dragging_files = true;
        super::note_folder_hover(model, hit.map(|folder| folder.path));
    } else {
        super::note_folder_hover(model, None);
    }
}

/// 列表空白处的点按：按下时记下，移动时记最远距离，松手时没走出容差、也没按着 Ctrl / Meta 就清空选择。
/// 走出容差的已经是框选，由列表框的 `PointerDragged` 处理。
fn note_blank_press(
    model: &mut ShellViewModel,
    document: &nana_ui::runtime::RuntimeDocument,
    payload: &InputPayload,
) {
    let InputPayload::Pointer(pointer) = payload else {
        return;
    };
    if pointer.pointer_id.0 != POINTER_ID {
        return;
    }
    match pointer.phase {
        PointerPhase::Down => {
            let blank = pointer.is_primary && pointer.button == 0 && pressed_blank(document);
            model.input.blank_press = blank.then_some(BlankPress {
                x: pointer.x,
                y: pointer.y,
                travel: 0.0,
                append: pointer.modifiers.control || pointer.modifiers.meta,
            });
        }
        PointerPhase::Move => {
            if let Some(press) = model.input.blank_press.as_mut() {
                let travel = (pointer.x - press.x).hypot(pointer.y - press.y);
                press.travel = press.travel.max(travel);
            }
        }
        PointerPhase::Up => {
            let Some(press) = model.input.blank_press.take() else {
                return;
            };
            if press.travel <= DRAG_SLOP && !press.append {
                model.reduce(super::ShellMessage::Input(super::InputMessage::BoxSelect {
                    paths: Vec::new(),
                    append: false,
                }));
            }
        }
        PointerPhase::Cancel => model.input.blank_press = None,
    }
}

/// 刚按下的节点在文件列表框里、不在任何卡片上，也没有控件抓住指针（滚动条）。
fn pressed_blank(document: &nana_ui::runtime::RuntimeDocument) -> bool {
    let document_id = document.document();
    let context = document.context();
    let world = context.world();
    if world.pointer_capture(document_id, POINTER_ID).is_some() {
        return false;
    }
    let mut current = world.pointer_press(document_id, POINTER_ID);
    while let Some(node) = current {
        match targets::entry_of(context, node).map(|entry| entry.kind) {
            Some(EntryKind::Card { .. }) => return false,
            Some(EntryKind::Browser) => return true,
            Some(EntryKind::Sidebar) | None => {}
        }
        current = world.parent_id(node);
    }
    false
}

/// 外部拖放停在侧栏文件夹上时，准备帧用已经拨过的悬停时钟再判一次 450ms。
/// 内部拖动由指针帧自己重判，这里不插进去。
pub(crate) fn tick_external_folder_hover(model: &mut ShellViewModel) {
    if model.input.internal_active
        || model.input.live_gesture.is_some()
        || !model.input.hover_on_sidebar
    {
        return;
    }
    if !model.input.dragging_files && !model.input.external_active {
        return;
    }
    let Some(path) = model.input.hover_folder.clone() else {
        return;
    };
    super::note_folder_hover(model, Some(path));
}
