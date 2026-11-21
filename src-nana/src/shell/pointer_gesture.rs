//! 实况文档上的指针拖动和框选。
//!
//! Nana 视图没有通用的拖动事件。每帧 `prepare`（`window_host::prepare_motion`）读主键指针的按下目标和
//! 坐标，再交给输入归约。卡片里的 `momobako-entry:{kind}:{path}` 文本是命中标记。

use nana_ui::runtime::{LayoutBox, RuntimeDocument, StableNodeId, UiWorld};

use super::input::{InputMessage, LiveGesture};
use super::{ShellMessage, ShellViewModel, WorkspacePanel, WorkspaceRepository};

const POINTER_ID: u64 = 1;
const PATH_MARK: &str = "momobako-entry:";
/// 文件条目拖动起步的阈值。
const ENTRY_DRAG_PX: f32 = 7.0;
/// 同一文件里框选开始移动的阈值：任一轴超过 3px。
const BOX_DRAG_PX: f32 = 3.0;

mod folder_drop;
pub(crate) use folder_drop::{note_external_folder_drag, tick_external_folder_hover};

#[derive(Clone)]
struct RowMark {
    kind: String,
    path: String,
    node: StableNodeId,
    bounds: LayoutBox,
}

/// 读当前文档的主键指针。返回 true 表示手势还按着：这一帧调用方只同步热字段、不做结构同步，
/// 免得按着的节点被换掉。
pub fn observe_live_pointer(model: &mut ShellViewModel, document: &RuntimeDocument) -> bool {
    let document_id = document.document();
    let context = document.context();
    let world = context.world();
    let pressed = world.pointer_press(document_id, POINTER_ID);
    let position = context.pointer_position(document_id, POINTER_ID);
    note_chrome_hover(model, world, document_id, position);
    if pressed.is_none() {
        model.input.model_drag = None;
        release_gesture(model, world, document_id, position);
        return false;
    }
    let Some((x, y)) = position else {
        eprintln!("Nana 实况指针已按下，但没有坐标");
        return model.input.live_gesture.is_some() || model.input.model_drag.is_some();
    };
    if track_model_drag(model, document, pressed, x, y) {
        return true;
    }
    let marks = collect_rows(world, document_id);
    let rows = folder_drop::file_rows(&marks);
    if rows.is_empty()
        && model.input.live_gesture.is_none()
        && file_rows_should_be_mounted(model)
        && !model.files.entry_names().is_empty()
    {
        eprintln!("Nana 实况文件行没有路径标记，指针拖动无法开始");
    }
    let list = list_bounds(world, &rows);
    let over_browser = list.is_some_and(|bounds| contains(bounds, x, y));
    let hit = pressed.expect("按下目标");
    let on_row = row_at(world, hit, &rows).or_else(|| row_at_point(&rows, x, y));
    let (width, height) = viewport_size(world, document_id);
    // 框选按下时按着 Ctrl 或 Meta 就是追加。
    let append = model.input.modifiers.command;

    if model.input.live_gesture.is_none() {
        if on_row.is_none() && !over_browser {
            return false;
        }
        model.input.live_gesture = Some(LiveGesture {
            row: on_row,
            origin_x: x,
            origin_y: y,
            x,
            y,
            armed: false,
            append,
        });
        super::files_selection_box::track(model, list);
        return true;
    }

    let Some(mut gesture) = model.input.live_gesture.clone() else {
        return false;
    };
    gesture.x = x;
    gesture.y = y;
    if let Some(path) = gesture.row.clone() {
        let distance = pointer_distance(gesture.origin_x, gesture.origin_y, x, y);
        let hover_folder = directory_at(&marks, x, y, &path);
        if !gesture.armed && distance >= ENTRY_DRAG_PX {
            gesture.armed = true;
            begin_entry_drag(model, &path, x, y, over_browser, hover_folder.clone());
        }
        if gesture.armed {
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
    } else if !gesture.armed
        && (dx(gesture.origin_x, x) > BOX_DRAG_PX || dx(gesture.origin_y, y) > BOX_DRAG_PX)
    {
        gesture.armed = true;
    }
    if gesture.row.is_none() && gesture.armed {
        let scroll = list_scroll_y(world, &rows);
        let mut paths = intersecting(&rows, gesture.origin_x, gesture.origin_y, x, y, scroll);
        extend_virtual_rows(
            model,
            &rows,
            &mut paths,
            gesture.origin_x,
            gesture.origin_y,
            x,
            y,
            scroll,
        );
        model.reduce(ShellMessage::Input(InputMessage::BoxSelect {
            paths,
            append: gesture.append,
        }));
    }
    model.input.live_gesture = Some(gesture);
    super::files_selection_box::track(model, list);
    true
}

/// 松手。框选没拖动且不是追加时清空选择；条目拖动已经起步才结束会话。
fn release_gesture(
    model: &mut ShellViewModel,
    world: &UiWorld,
    document_id: nana_ui::runtime::DocumentId,
    position: Option<(f32, f32)>,
) {
    let Some(gesture) = model.input.live_gesture.take() else {
        return;
    };
    super::files_selection_box::clear(model);
    if let Some(previous) = model.sidebar.hover_folder.clone() {
        model.reduce(ShellMessage::Sidebar(super::SidebarMessage::Gap(
            super::GapMessage::FolderLeave(previous),
        )));
    }
    let marks = collect_rows(world, document_id);
    let rows = folder_drop::file_rows(&marks);
    let list = list_bounds(world, &rows);
    let (x, y) = position.unwrap_or((gesture.x, gesture.y));
    let over_browser = list.is_some_and(|bounds| contains(bounds, x, y));
    if let Some(path) = gesture.row.clone() {
        if gesture.armed {
            let hover_folder = directory_at(&marks, x, y, &path);
            model.reduce(ShellMessage::Input(InputMessage::EntryDragEnd {
                hover_folder,
                over_browser,
                has_pointer: position.is_some(),
            }));
        }
        return;
    }
    if !gesture.armed && !gesture.append {
        model.reduce(ShellMessage::Input(InputMessage::BoxSelect {
            paths: Vec::new(),
            append: false,
        }));
    }
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

fn directory_at(rows: &[RowMark], x: f32, y: f32, exclude: &str) -> Option<String> {
    folder_drop::directory_at(rows, x, y, exclude).map(|hit| hit.path)
}

/// 记下指针是不是停在侧栏底部入口那一排上。
fn note_chrome_hover(
    model: &mut ShellViewModel,
    world: &UiWorld,
    document_id: nana_ui::runtime::DocumentId,
    position: Option<(f32, f32)>,
) {
    // 整棵重挂后悬停目标随旧节点一起没了，指针不动就不会重新命中：按最后一次指针位置补命中。
    let hover = world
        .pointer_hover(document_id, POINTER_ID)
        .or_else(|| position.and_then(|(x, y)| world.hit_test(document_id, x, y)));
    model.input.footer_hover = hover
        .is_some_and(|id| label_in(world, id, &["设置", "拓展", "日志"]) || footer_task(world, id));
}

fn label_in(world: &UiWorld, id: StableNodeId, labels: &[&str]) -> bool {
    let mut current = Some(id);
    while let Some(node) = current {
        if node_names(world, node).any(|name| labels.contains(&name.as_str())) {
            return true;
        }
        current = world.parent_id(node);
    }
    false
}

fn footer_task(world: &UiWorld, id: StableNodeId) -> bool {
    let mut current = Some(id);
    while let Some(node) = current {
        if node_names(world, node).any(|name| name == "任务" || name.starts_with("任务 ")) {
            return true;
        }
        current = world.parent_id(node);
    }
    false
}

/// 节点的文字和无障碍名称。图标按钮没有文字，名称只在无障碍状态里。
fn node_names(world: &UiWorld, node: StableNodeId) -> impl Iterator<Item = String> {
    let text = world.text(node).map(str::to_string);
    let label = world
        .accessibility(node)
        .and_then(|state| state.label.as_deref().map(str::to_string));
    text.into_iter().chain(label)
}

/// 文件表面已经挂上时，行里应该有路径标记。
fn file_rows_should_be_mounted(model: &ShellViewModel) -> bool {
    model.workspace.startup.status == super::StartupStatus::Ready
        && model.workspace.main_region() == super::MainRegion::HasRepository
        && matches!(
            model.workspace.panel,
            WorkspacePanel::Files | WorkspacePanel::Trash | WorkspacePanel::SmartFolder
        )
        && !matches!(
            model.page,
            super::ShellPage::Settings | super::ShellPage::SettingsError
        )
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

fn pointer_distance(x0: f32, y0: f32, x1: f32, y1: f32) -> f32 {
    let dx = f64::from(x1 - x0);
    let dy = f64::from(y1 - y0);
    dx.hypot(dy) as f32
}

fn collect_rows(world: &UiWorld, document_id: nana_ui::runtime::DocumentId) -> Vec<RowMark> {
    let mut rows = Vec::new();
    for id in world.document_order(document_id) {
        let Some(text) = world.text(id) else {
            continue;
        };
        let Some(rest) = text.strip_prefix(PATH_MARK) else {
            continue;
        };
        let Some((kind, path)) = rest.split_once(':') else {
            continue;
        };
        if path.is_empty() {
            continue;
        }
        let Some(node) = world.parent_id(id) else {
            continue;
        };
        let Some(bounds) = world.layout_box(node) else {
            continue;
        };
        if bounds.width <= 0.0 || bounds.height <= 0.0 {
            continue;
        }
        rows.push(RowMark {
            kind: kind.to_string(),
            path: path.to_string(),
            node,
            bounds,
        });
    }
    rows
}

/// 文件列表的滚动区：从任意一张卡片往上找到的第一个 `ScrollView`。
/// 卡片在虚拟行、分组和内容列里套了好几层，按组件类型找，不按层数猜。
fn list_bounds(world: &UiWorld, rows: &[RowMark]) -> Option<LayoutBox> {
    let mut cursor = world.parent_id(rows.first()?.node);
    while let Some(node) = cursor {
        let scroll = world.component_type(node).is_some_and(|kind| {
            std::borrow::Borrow::<str>::borrow(kind)
                == nana_ui::runtime::component_descriptors::SCROLL_VIEW.type_id
        });
        if scroll {
            return world
                .layout_box(node)
                .filter(|bounds| bounds.width > 0.0 && bounds.height > 0.0);
        }
        cursor = world.parent_id(node);
    }
    eprintln!("Nana 文件卡片上面没有找到列表滚动区");
    None
}

/// 指针落在行盒子里，即使命中被父级列表吃掉也算这一行。
fn row_at_point(rows: &[RowMark], x: f32, y: f32) -> Option<String> {
    let mut best: Option<&RowMark> = None;
    for row in rows {
        if !contains(row.bounds, x, y) {
            continue;
        }
        let smaller = best.is_none_or(|current| {
            row.bounds.width * row.bounds.height < current.bounds.width * current.bounds.height
        });
        if smaller {
            best = Some(row);
        }
    }
    best.map(|row| row.path.clone())
}

fn row_at(world: &UiWorld, hit: StableNodeId, rows: &[RowMark]) -> Option<String> {
    let mut current = Some(hit);
    while let Some(id) = current {
        if let Some(row) = rows.iter().find(|row| row.node == id) {
            return Some(row.path.clone());
        }
        current = world.parent_id(id);
    }
    None
}

fn intersecting(
    rows: &[RowMark],
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    scroll_y: f32,
) -> Vec<String> {
    let left = x0.min(x1);
    let right = x0.max(x1);
    let (top, bottom) = selection_y(rows, y0, y1, scroll_y);
    rows.iter()
        .filter(|row| {
            let bounds = row.bounds;
            bounds.x + bounds.width >= left
                && bounds.x <= right
                && bounds.y + bounds.height >= top
                && bounds.y <= bottom
        })
        .map(|row| row.path.clone())
        .collect()
}

/// 卡片框停在没滚动的内容坐标时，指针纵坐标加上滚动量才和它对齐。
/// 框已经是视口坐标时不加。没滚动时两种算法相同。
fn selection_y(rows: &[RowMark], y0: f32, y1: f32, scroll_y: f32) -> (f32, f32) {
    let top = y0.min(y1);
    let bottom = y0.max(y1);
    if content_boxes(rows, scroll_y) {
        (top + scroll_y, bottom + scroll_y)
    } else {
        (top, bottom)
    }
}

/// 滚过以后，内容坐标的盒子减去滚动量会明显上移；视口坐标的盒子减了会跑到滚动区外面。
fn content_boxes(rows: &[RowMark], scroll_y: f32) -> bool {
    if scroll_y < 1.0 {
        return false;
    }
    let Some(sample) = rows.iter().min_by(|left, right| {
        left.bounds
            .y
            .partial_cmp(&right.bounds.y)
            .unwrap_or(std::cmp::Ordering::Equal)
    }) else {
        return false;
    };
    let shifted = sample.bounds.y - scroll_y;
    shifted.abs() + 800.0 < sample.bounds.y.abs()
}

/// 按虚拟行的位置补上还没建出来的卡片。已经命中的路径保持在前面。
fn extend_virtual_rows(
    model: &ShellViewModel,
    rows: &[RowMark],
    paths: &mut Vec<String>,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    scroll_y: f32,
) {
    let ctx = super::files::FileContext::from_model(model);
    let listed = model.files.listed_rows(&ctx);
    if listed.is_empty() {
        return;
    }
    let width = model
        .files
        .list_width
        .unwrap_or_else(|| super::files_view::estimated_list_width(model));
    let slots =
        super::files_view::row_slots(&listed, model.files.display_mode, width, !ctx.is_virtual());
    let Some((dx, dy)) = slot_origin(rows, &slots) else {
        eprintln!("Nana 框选没有挂上的卡片可对齐，未建行的位置对不上");
        return;
    };
    let left = x0.min(x1);
    let right = x0.max(x1);
    let (top, bottom) = selection_y(rows, y0, y1, scroll_y);
    for slot in slots {
        let x = slot.x + dx;
        let y = slot.y + dy;
        let hits = x + slot.width >= left && x <= right && y + slot.height >= top && y <= bottom;
        if hits && !paths.iter().any(|path| path == &slot.path) {
            paths.push(slot.path);
        }
    }
}

/// 用一张已经挂上的卡片，把虚拟行的内容坐标平移到布局盒的坐标系。
fn slot_origin(rows: &[RowMark], slots: &[super::files_view::RowSlot]) -> Option<(f32, f32)> {
    let row = rows
        .iter()
        .find(|row| slots.iter().any(|slot| slot.path == row.path))?;
    let slot = slots.iter().find(|slot| slot.path == row.path)?;
    Some((row.bounds.x - slot.x, row.bounds.y - slot.y))
}

/// 列表滚动区的纵向滚动量。没有滚动区时是 0。
fn list_scroll_y(world: &UiWorld, rows: &[RowMark]) -> f32 {
    let Some(first) = rows.first() else {
        return 0.0;
    };
    let mut cursor = world.parent_id(first.node);
    while let Some(node) = cursor {
        let scroll = world.component_type(node).is_some_and(|kind| {
            std::borrow::Borrow::<str>::borrow(kind)
                == nana_ui::runtime::component_descriptors::SCROLL_VIEW.type_id
        });
        if scroll {
            return world
                .scroll_offset(node)
                .map(|offset| offset.y)
                .unwrap_or(0.0);
        }
        cursor = world.parent_id(node);
    }
    0.0
}

/// 指针按在有网格的模型画面上时，拖拽旋转和缩放。按钮自己处理点击，这里不抢走。
fn track_model_drag(
    model: &mut ShellViewModel,
    document: &RuntimeDocument,
    pressed: Option<StableNodeId>,
    x: f32,
    y: f32,
) -> bool {
    let Some(hit) = pressed else {
        return false;
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

fn viewport_size(world: &UiWorld, document_id: nana_ui::runtime::DocumentId) -> (f32, f32) {
    let mut width: f32 = 0.0;
    let mut height: f32 = 0.0;
    for id in world.document_order(document_id) {
        if let Some(bounds) = world.layout_box(id) {
            width = width.max(bounds.x + bounds.width);
            height = height.max(bounds.y + bounds.height);
        }
    }
    if width <= 0.0 || height <= 0.0 {
        eprintln!("Nana 实况拖动读不到窗口尺寸");
        return (1200.0, 800.0);
    }
    (width, height)
}

fn contains(bounds: LayoutBox, x: f32, y: f32) -> bool {
    x >= bounds.x && y >= bounds.y && x < bounds.x + bounds.width && y < bounds.y + bounds.height
}

fn dx(origin: f32, current: f32) -> f32 {
    (current - origin).abs()
}

#[cfg(test)]
mod tests;
