//! 框选框住哪些条目。
//!
//! 挂着的卡片按布局盒算；虚拟列表还没建出来的行按行位置补上，用一张挂着的卡片对齐坐标系。
//! 卡片的布局盒停在滚动前的内容坐标，指针的窗口坐标先按卡片所在滚动区换过去再比。

use nana_ui::runtime::{LayoutBox, RuntimeDocument, StableNodeId};

use super::ShellViewModel;
use super::targets;

/// 内容坐标里的框选矩形。
#[derive(Clone, Copy, Debug)]
struct Rect {
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
}

impl Rect {
    fn hits(&self, x: f32, y: f32, width: f32, height: f32) -> bool {
        x + width >= self.left && x <= self.right && y + height >= self.top && y <= self.bottom
    }
}

/// 从窗口里的 `origin` 拖到 `point` 框住的条目路径。挂着的卡片在前，补上的虚拟行在后。
pub(super) fn paths(
    model: &ShellViewModel,
    document: &RuntimeDocument,
    origin: (f32, f32),
    point: (f32, f32),
) -> Vec<String> {
    let cards = targets::cards(document);
    let Some(rect) = content_rect(document, &cards, origin, point) else {
        return Vec::new();
    };
    let mut paths = cards
        .iter()
        .filter(|(_, _, bounds)| rect.hits(bounds.x, bounds.y, bounds.width, bounds.height))
        .map(|(_, path, _)| path.clone())
        .collect::<Vec<_>>();
    extend_virtual_rows(model, &cards, &mut paths, rect);
    paths
}

/// 两个窗口点换到卡片的内容坐标后围成的矩形。没有挂着的卡片时无从换算。
fn content_rect(
    document: &RuntimeDocument,
    cards: &[(StableNodeId, String, LayoutBox)],
    origin: (f32, f32),
    point: (f32, f32),
) -> Option<Rect> {
    let world = document.context().world();
    let (reference, _, _) = cards.first()?;
    let (x0, y0) = world.pointer_layout_position(*reference, origin.0, origin.1)?;
    let (x1, y1) = world.pointer_layout_position(*reference, point.0, point.1)?;
    Some(Rect {
        left: x0.min(x1),
        top: y0.min(y1),
        right: x0.max(x1),
        bottom: y0.max(y1),
    })
}

/// 按虚拟行的位置补上还没建出来的卡片。已经命中的路径保持在前面。
fn extend_virtual_rows(
    model: &ShellViewModel,
    cards: &[(StableNodeId, String, LayoutBox)],
    paths: &mut Vec<String>,
    rect: Rect,
) {
    let ctx = super::super::files::FileContext::from_model(model);
    let listed = model.files.listed_rows(&ctx);
    if listed.is_empty() {
        return;
    }
    let width = model
        .files
        .list_width
        .unwrap_or_else(|| super::super::files_view::estimated_list_width(model));
    let slots = super::super::files_view::row_slots(
        &listed,
        model.files.display_mode,
        width,
        !ctx.is_virtual(),
    );
    let Some((dx, dy)) = slot_origin(cards, &slots) else {
        eprintln!("Nana 框选没有挂上的卡片可对齐，未建行的位置对不上");
        return;
    };
    for slot in slots {
        let hits = rect.hits(slot.x + dx, slot.y + dy, slot.width, slot.height);
        if hits && !paths.iter().any(|path| path == &slot.path) {
            paths.push(slot.path);
        }
    }
}

/// 用一张已经挂上的卡片，把虚拟行的内容坐标平移到布局盒的坐标系。
fn slot_origin(
    cards: &[(StableNodeId, String, LayoutBox)],
    slots: &[super::super::files_view::RowSlot],
) -> Option<(f32, f32)> {
    cards.iter().find_map(|(_, path, bounds)| {
        let slot = slots.iter().find(|slot| &slot.path == path)?;
        Some((bounds.x - slot.x, bounds.y - slot.y))
    })
}
