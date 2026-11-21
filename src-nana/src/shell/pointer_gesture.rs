//! 文件列表上的条目拖动和框选，以及模型画面上的拖拽旋转。
//!
//! 拖动由 NanaUI 的 `PointerDragged` 报告：卡片接住按在卡片上的拖动，列表框接住按在空白处的
//! （`files_view/cards.rs`、`files_selection_box.rs`）。框架负责拖动容差、指针抓取和「拖过以后松手
//! 不算点击」；采样经输入归约记进 [`LiveGesture`]。每帧 `prepare`（`window_host::prepare_motion`）在这里
//! 结算：把按下点换成窗口坐标，按 NanaUI 的条目拖放目标找指针底下的文件夹，再交给拖动和框选归约。
//! 拖放目标和它们对应的路径见 [`targets`]。

use nana_ui::runtime::{RuntimeDocument, StableNodeId};

use super::input::{InputMessage, LiveGesture};
use super::{ShellMessage, ShellViewModel, WorkspacePanel, WorkspaceRepository};

const POINTER_ID: u64 = 1;
/// 条目拖动起步的阈值。框架越过 4px 就开始报拖动，走过这么远才真正开始移动条目。
pub(crate) const ENTRY_DRAG_PX: f32 = 7.0;

mod box_select;
mod routed;
mod targets;
pub(crate) use routed::{note_routed_input, tick_external_folder_hover};
#[cfg(test)]
pub(crate) use targets::cards as mounted_cards;
pub(crate) use targets::{EntryKind, EntryNode, register as register_entry};

/// 结算这一帧的拖动。返回 true 表示手势还没收尾：这一帧调用方只同步热字段、不做结构同步，
/// 免得抓着指针的节点被换掉。
pub fn observe_live_pointer(model: &mut ShellViewModel, document: &RuntimeDocument) -> bool {
    let orbiting = track_model_drag(model, document);
    settle_gesture(model, document) || orbiting
}

/// 按记下的采样推进手势：条目拖动越过阈值才起步，之后每帧更新悬停和放下目标；
/// 框选按两个角重新算框住的条目。松手或取消以后收尾，清掉手势。
fn settle_gesture(model: &mut ShellViewModel, document: &RuntimeDocument) -> bool {
    let Some(mut gesture) = model.input.live_gesture.clone() else {
        return false;
    };
    if gesture.origin.is_none() {
        let Some(origin) = press_point(document, &gesture) else {
            eprintln!(
                "Nana 拖动的按下节点已经不在，放弃这次拖动：{:?}",
                gesture.row
            );
            abandon(model, &gesture);
            return false;
        };
        gesture.origin = Some(origin);
    }
    let Some((x, y)) = gesture.point() else {
        return false;
    };
    let over_browser = targets::over_browser(document, x, y);
    match gesture.row.clone() {
        Some(path) => drag_entry(model, document, &mut gesture, &path, x, y, over_browser),
        None => {
            gesture.armed = true;
            let origin = gesture.origin.unwrap_or((x, y));
            if gesture.finished != Some(true) {
                let paths = box_select::paths(model, document, origin, (x, y));
                model.reduce(ShellMessage::Input(InputMessage::BoxSelect {
                    paths,
                    append: gesture.append,
                }));
            }
        }
    }
    if gesture.finished.is_some() {
        release(model);
        return false;
    }
    model.input.live_gesture = Some(gesture);
    let list = targets::browser(document).map(|(_, bounds)| bounds);
    super::files_selection_box::track(model, list);
    true
}

/// 条目拖动的一帧。走过 [`ENTRY_DRAG_PX`] 才开始会话；松手时交给放下归约，取消时什么也不移动。
#[allow(clippy::too_many_arguments)]
fn drag_entry(
    model: &mut ShellViewModel,
    document: &RuntimeDocument,
    gesture: &mut LiveGesture,
    path: &str,
    x: f32,
    y: f32,
    over_browser: bool,
) {
    let cancelled = gesture.finished == Some(true);
    let hover_folder = targets::folder_at(document, x, y, path).map(|hit| hit.path);
    if !gesture.armed && !cancelled && gesture.travel() >= ENTRY_DRAG_PX {
        gesture.armed = true;
        begin_entry_drag(model, path, x, y, over_browser, hover_folder.clone());
    }
    if !gesture.armed {
        return;
    }
    if gesture.finished.is_some() {
        model.reduce(ShellMessage::Input(InputMessage::EntryDragEnd {
            hover_folder: hover_folder.filter(|_| !cancelled),
            over_browser: over_browser && !cancelled,
            has_pointer: true,
        }));
        return;
    }
    let (width, height) = window_size(document, gesture.anchor);
    model.reduce(ShellMessage::Input(InputMessage::EntryDragMove {
        x,
        y,
        bounds_width: width,
        bounds_height: height,
        hover_folder: hover_folder.clone(),
        over_browser,
    }));
    note_folder_hover(model, hover_folder);
}

/// 收尾：收起选框，清掉侧栏文件夹的悬停计时和手势。选择由框选归约留下。
fn release(model: &mut ShellViewModel) {
    model.input.live_gesture = None;
    super::files_selection_box::clear(model);
    if let Some(previous) = model.sidebar.hover_folder.clone() {
        model.reduce(ShellMessage::Sidebar(super::SidebarMessage::Gap(
            super::GapMessage::FolderLeave(previous),
        )));
    }
}

/// 按下节点没了：已经起步的条目拖动按取消结束，其余直接收尾。
fn abandon(model: &mut ShellViewModel, gesture: &LiveGesture) {
    if gesture.row.is_some() && gesture.armed {
        model.reduce(ShellMessage::Input(InputMessage::EntryDragEnd {
            hover_folder: None,
            over_browser: false,
            has_pointer: true,
        }));
    }
    release(model);
}

/// 窗口里的按下点：收到拖动的节点的布局盒加上它自己坐标里的按下点，再按祖先滚动换到窗口。
fn press_point(document: &RuntimeDocument, gesture: &LiveGesture) -> Option<(f32, f32)> {
    let anchor = gesture.anchor?;
    let world = document.context().world();
    let bounds = world.layout_box(anchor)?;
    let (x, y) = gesture.press;
    world.layout_pointer_position(anchor, bounds.x + x, bounds.y + y)
}

/// 离开当前行或换到另一行时清掉 450ms 计时，再为新行重新开始。
fn note_folder_hover(model: &mut ShellViewModel, hit: Option<String>) {
    if let Some(previous) = model.sidebar.hover_folder.clone() {
        if hit.as_deref() != Some(previous.as_str()) {
            model.reduce(ShellMessage::Sidebar(super::SidebarMessage::Gap(
                super::GapMessage::FolderLeave(previous),
            )));
        }
    }
    if let Some(folder) = hit {
        model.reduce(ShellMessage::Sidebar(super::SidebarMessage::Gap(
            super::GapMessage::FolderHover {
                path: folder,
                now_ms: model.sidebar.hover_now(),
                dragging: true,
            },
        )));
    }
}

fn begin_entry_drag(
    model: &mut ShellViewModel,
    path: &str,
    x: f32,
    y: f32,
    over_browser: bool,
    hover_folder: Option<String>,
) {
    let repository = model.workspace.active_repository();
    let writable = repository
        .is_some_and(|item| super::files::repository_is_writable(&item.status, &item.capabilities));
    let trash = model.workspace.panel == WorkspacePanel::Trash;
    let smart_folder = model.workspace.panel == WorkspacePanel::SmartFolder;
    let backend_kind = repository.map(drag_backend_kind).unwrap_or_default();
    let repo_root = repository.map(|item| item.path.clone()).unwrap_or_default();
    if !writable || trash || smart_folder || backend_kind != "filesystem" {
        eprintln!(
            "Nana 实况拖动被拒绝：writable={writable} trash={trash} smart={smart_folder} kind={backend_kind}"
        );
    }
    let selected = model.files.selected_paths().to_vec();
    model.reduce(ShellMessage::Input(InputMessage::BeginEntryDrag {
        path: path.to_string(),
        selected,
        x,
        y,
        writable,
        trash,
        smart_folder,
        backend_kind,
        repo_root,
        hover_folder,
        over_browser,
    }));
}

/// 本地插件和 `localRootPath` 能力都按文件系统拖放处理。
fn drag_backend_kind(repository: &WorkspaceRepository) -> String {
    if repository.backend_plugin_id == "filesystem"
        || repository.backend_plugin_id == "local"
        || repository
            .capabilities
            .iter()
            .any(|capability| capability == "localRootPath")
    {
        "filesystem".into()
    } else {
        repository.backend_plugin_id.clone()
    }
}

/// 窗口尺寸：从拖动节点往上找到文档根，用它的布局盒。拖出窗口边缘时交给系统拖放要用。
fn window_size(document: &RuntimeDocument, anchor: Option<StableNodeId>) -> (f32, f32) {
    let world = document.context().world();
    let mut root = anchor;
    while let Some(parent) = root.and_then(|node| world.parent_id(node)) {
        root = Some(parent);
    }
    match root.and_then(|node| world.layout_box(node)) {
        Some(bounds) if bounds.width > 0.0 && bounds.height > 0.0 => {
            (bounds.x + bounds.width, bounds.y + bounds.height)
        }
        _ => {
            eprintln!("Nana 实况拖动读不到窗口尺寸");
            (1200.0, 800.0)
        }
    }
}

/// 指针按在有网格的模型画面上时，拖拽旋转和缩放。按钮自己处理点击，这里不抢走。
fn track_model_drag(model: &mut ShellViewModel, document: &RuntimeDocument) -> bool {
    let document_id = document.document();
    let context = document.context();
    let Some(hit) = context.world().pointer_press(document_id, POINTER_ID) else {
        model.input.model_drag = None;
        return false;
    };
    let Some((x, y)) = context.pointer_position(document_id, POINTER_ID) else {
        eprintln!("Nana 实况指针已按下，但没有坐标");
        return model.input.model_drag.is_some();
    };
    if !model.inspect.has_mesh() || !model_surface(document, hit) {
        model.input.model_drag = None;
        return false;
    }
    let Some((last_x, last_y)) = model.input.model_drag else {
        model.input.model_drag = Some((x, y));
        return true;
    };
    let (yaw, zoom) = super::inspect::orbit_drag(x - last_x, y - last_y);
    model.input.model_drag = Some((x, y));
    if yaw.abs() > f32::EPSILON || (zoom - 1.0).abs() > 0.0001 {
        model.reduce(ShellMessage::Inspect(super::InspectMessage::Orbit {
            yaw,
            zoom,
        }));
    }
    true
}

/// 命中在模型画面上，且不在旋转按钮上。
fn model_surface(document: &RuntimeDocument, hit: StableNodeId) -> bool {
    let context = document.context();
    let mut current = Some(hit);
    let mut on_model = false;
    let mut on_controls = false;
    while let Some(node) = current {
        if let Some(path) = context.assembly_path(node) {
            if path.contains("inspect-orbit") {
                on_controls = true;
            }
            if path.contains("inspect-model") {
                on_model = true;
            }
        }
        current = context.world().parent_id(node);
    }
    on_model && !on_controls
}

fn contains(bounds: nana_ui::runtime::LayoutBox, x: f32, y: f32) -> bool {
    x >= bounds.x && y >= bounds.y && x < bounds.x + bounds.width && y < bounds.y + bounds.height
}

#[cfg(test)]
mod tests;
