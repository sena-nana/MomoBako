//! 实况文档上的指针拖动和框选。
//!
//! Nana 视图没有通用的拖动事件。`prepare` 在拆树之前读按下目标和坐标，
//! 再交给已经对照过 Vue 的输入归约。行上的 `momobako-path:` 文本是命中标记。

use nana_ui::runtime::{LayoutBox, RuntimeDocument, StableNodeId, UiWorld};

use super::input::{InputMessage, LiveGesture};
use super::{ShellMessage, ShellViewModel, WorkspacePanel, WorkspaceRepository};

const POINTER_ID: u64 = 1;
const PATH_MARK: &str = "momobako-path:";
/// Vue `useFileBrowserPanelViewModel.ts` 的 `dragStartThreshold`。
const ENTRY_DRAG_PX: f32 = 7.0;
/// 同一文件里框选开始移动的阈值：任一轴超过 3px。
const BOX_DRAG_PX: f32 = 3.0;

struct RowMark {
    path: String,
    node: StableNodeId,
    bounds: LayoutBox,
}

/// 读当前文档的主键指针。返回 true 表示手势还按着，调用方不要拆掉这棵树。
pub fn observe_live_pointer(model: &mut ShellViewModel, document: &RuntimeDocument) -> bool {
    let document_id = document.document();
    let context = document.context();
    let world = context.world();
    let pressed = world.pointer_press(document_id, POINTER_ID);
    let position = context.pointer_position(document_id, POINTER_ID);
    if pressed.is_none() {
        release_gesture(model, world, document_id, position);
        return false;
    }
    let Some((x, y)) = position else {
        eprintln!("Nana 实况指针已按下，但没有坐标");
        return model.input.live_gesture.is_some();
    };
    let rows = collect_rows(world, document_id);
    if rows.is_empty() && model.input.live_gesture.is_none() && file_rows_should_be_mounted(model) && !model.files.entry_names().is_empty() {
        eprintln!("Nana 实况文件行没有路径标记，指针拖动无法开始");
    }
    let list = list_bounds(world, &rows);
    let over_browser = list.is_some_and(|bounds| contains(bounds, x, y));
    let hit = pressed.expect("按下目标");
    let on_row = row_at(world, hit, &rows).or_else(|| row_at_point(&rows, x, y));
    let (width, height) = viewport_size(world, document_id);
    let append = additive_modifiers();

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
        return true;
    }

    let Some(mut gesture) = model.input.live_gesture.clone() else {
        return false;
    };
    gesture.x = x;
    gesture.y = y;
    if let Some(path) = gesture.row.clone() {
        let distance = pointer_distance(gesture.origin_x, gesture.origin_y, x, y);
        if !gesture.armed && distance >= ENTRY_DRAG_PX {
            gesture.armed = true;
            begin_entry_drag(model, &path, x, y, over_browser);
        }
        if gesture.armed {
            model.reduce(ShellMessage::Input(InputMessage::EntryDragMove {
                x,
                y,
                bounds_width: width,
                bounds_height: height,
                hover_folder: None,
                over_browser,
            }));
        }
    } else if !gesture.armed && (dx(gesture.origin_x, x) > BOX_DRAG_PX || dx(gesture.origin_y, y) > BOX_DRAG_PX) {
        gesture.armed = true;
    }
    if gesture.row.is_none() && gesture.armed {
        let paths = intersecting(&rows, gesture.origin_x, gesture.origin_y, x, y);
        model.reduce(ShellMessage::Input(InputMessage::BoxSelect { paths, append: gesture.append }));
    }
    model.input.live_gesture = Some(gesture);
    true
}

/// 松手。框选没拖动且不是追加时清空选择；条目拖动已经起步才结束会话。
fn release_gesture(model: &mut ShellViewModel, world: &UiWorld, document_id: nana_ui::runtime::DocumentId, position: Option<(f32, f32)>) {
    let Some(gesture) = model.input.live_gesture.take() else {
        return;
    };
    let rows = collect_rows(world, document_id);
    let list = list_bounds(world, &rows);
    let (x, y) = position.unwrap_or((gesture.x, gesture.y));
    let over_browser = list.is_some_and(|bounds| contains(bounds, x, y));
    if gesture.row.is_some() {
        if gesture.armed {
            model.reduce(ShellMessage::Input(InputMessage::EntryDragEnd {
                hover_folder: None,
                over_browser,
                has_pointer: position.is_some(),
            }));
        }
        return;
    }
    if !gesture.armed && !gesture.append {
        model.reduce(ShellMessage::Input(InputMessage::BoxSelect { paths: Vec::new(), append: false }));
    }
}

fn begin_entry_drag(model: &mut ShellViewModel, path: &str, x: f32, y: f32, over_browser: bool) {
    let repository = model.workspace.active_repository();
    let writable = repository.is_some_and(|item| super::files::repository_is_writable(&item.status, &item.capabilities));
    let trash = model.workspace.panel == WorkspacePanel::Trash;
    let smart_folder = model.workspace.panel == WorkspacePanel::SmartFolder;
    let backend_kind = repository.map(drag_backend_kind).unwrap_or_default();
    let repo_root = repository.map(|item| item.path.clone()).unwrap_or_default();
    if !writable || trash || smart_folder || backend_kind != "filesystem" {
        eprintln!("Nana 实况拖动被拒绝：writable={writable} trash={trash} smart={smart_folder} kind={backend_kind}");
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
        hover_folder: None,
        over_browser,
    }));
}

/// 文件表面已经挂上时，行里应该有路径标记。
fn file_rows_should_be_mounted(model: &ShellViewModel) -> bool {
    !model.acceptance_scene
        && model.workspace.startup.status == super::StartupStatus::Ready
        && model.workspace.main_region() == super::MainRegion::HasRepository
        && matches!(model.workspace.panel, WorkspacePanel::Files | WorkspacePanel::Trash | WorkspacePanel::SmartFolder)
        && !matches!(model.page, super::ShellPage::Settings | super::ShellPage::SettingsError)
}

/// 本地插件和 `localRootPath` 能力都按 Vue 的 filesystem 拖放处理。
fn drag_backend_kind(repository: &WorkspaceRepository) -> String {
    if repository.backend_plugin_id == "filesystem"
        || repository.backend_plugin_id == "local"
        || repository.capabilities.iter().any(|capability| capability == "localRootPath")
    {
        "filesystem".into()
    } else {
        repository.backend_plugin_id.clone()
    }
}

/// 路由之后文档不保留 Ctrl / Meta。没有修饰键时框选是替换，和 Vue 松开这些键相同。
fn additive_modifiers() -> bool {
    false
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
        let Some(path) = text.strip_prefix(PATH_MARK) else {
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
        rows.push(RowMark { path: path.to_string(), node, bounds });
    }
    rows
}

fn list_bounds(world: &UiWorld, rows: &[RowMark]) -> Option<LayoutBox> {
    let content = world.parent_id(rows.first()?.node)?;
    let host = world.parent_id(content).unwrap_or(content);
    world.layout_box(host).filter(|bounds| bounds.width > 0.0 && bounds.height > 0.0)
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

fn intersecting(rows: &[RowMark], x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<String> {
    let left = x0.min(x1);
    let right = x0.max(x1);
    let top = y0.min(y1);
    let bottom = y0.max(y1);
    rows.iter()
        .filter(|row| {
            let bounds = row.bounds;
            bounds.x + bounds.width >= left && bounds.x <= right && bounds.y + bounds.height >= top && bounds.y <= bottom
        })
        .map(|row| row.path.clone())
        .collect()
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
mod tests {
    use super::*;
    use crate::backend::services::repository::{
        FileBrowserEntry, FileBrowserSnapshot, RepositoryStructureCacheState,
    };
    use nana_ui::{ApplicationWindow, HeadlessInput, NanaTextShaper, PointerPhase};
    use nana_ui::runtime::LayoutViewport;

    use crate::acceptance_document_for_model;
    use crate::window_host::prepare_motion;

    #[test]
    fn live_pointer_drag_and_box_select_follow_the_vue_thresholds() {
        let mut model = files_model();
        let mut window = mounted(&model);
        let mut input = bind(&mut window);
        let cover = row_center(&window, "cover.png");
        pointer(&mut window, &mut input, PointerPhase::Down, cover.0, cover.1);
        assert!(prepare_motion_tracks(&mut model, &mut window), "按下条目应该记住手势");
        assert!(!model.input.internal_active);
        pointer(&mut window, &mut input, PointerPhase::Move, cover.0 + 4.0, cover.1);
        assert!(prepare_motion_tracks(&mut model, &mut window));
        assert!(!model.input.internal_active, "4px 还没到 Vue 的 7px 拖动阈值");

        let mut model = files_model();
        let mut window = mounted(&model);
        let mut input = bind(&mut window);
        let cover = row_center(&window, "cover.png");
        pointer(&mut window, &mut input, PointerPhase::Down, cover.0, cover.1);
        observe_live_pointer(&mut model, &window.document);
        pointer(&mut window, &mut input, PointerPhase::Move, cover.0 + 20.0, cover.1);
        observe_live_pointer(&mut model, &window.document);
        assert!(model.input.internal_active);
        assert_eq!(model.files.selected_paths(), &["cover.png".to_string()]);

        let mut model = files_model();
        model.files.set_selected_paths(vec!["photos".into()]);
        let mut window = mounted(&model);
        let mut input = bind(&mut window);
        let empty = empty_list_point(&window);
        pointer(&mut window, &mut input, PointerPhase::Down, empty.0, empty.1);
        observe_live_pointer(&mut model, &window.document);
        pointer(&mut window, &mut input, PointerPhase::Up, empty.0, empty.1);
        observe_live_pointer(&mut model, &window.document);
        assert!(model.files.selected_paths().is_empty(), "没拖动的框选松手要清空选择");

        let mut model = files_model();
        let mut window = mounted(&model);
        let mut input = bind(&mut window);
        let empty = empty_list_point(&window);
        let cover = row_center(&window, "cover.png");
        pointer(&mut window, &mut input, PointerPhase::Down, empty.0, empty.1);
        observe_live_pointer(&mut model, &window.document);
        pointer(&mut window, &mut input, PointerPhase::Move, cover.0, cover.1);
        observe_live_pointer(&mut model, &window.document);
        let selected = model.files.selected_paths();
        assert!(selected.iter().any(|path| path == "cover.png"), "框选应该盖住 cover.png：{selected:?}");
        assert!(selected.len() >= 2, "从列表空白拖到封面应该框住不止一行：{selected:?}");
    }

    fn prepare_motion_tracks(model: &mut ShellViewModel, window: &mut ApplicationWindow) -> bool {
        prepare_motion(model, window);
        model.input.live_gesture.is_some()
    }

    fn mounted(model: &ShellViewModel) -> ApplicationWindow {
        let mut window = ApplicationWindow::new();
        window.document = acceptance_document_for_model(model.clone()).expect("生产文档");
        window
            .document
            .flush(LayoutViewport::new(1200.0, 800.0), &mut NanaTextShaper::default())
            .expect("布局");
        window
    }

    fn bind(window: &mut ApplicationWindow) -> HeadlessInput {
        let document_id = window.document.document();
        HeadlessInput::bind(window.document.context_mut(), document_id)
    }

    fn pointer(window: &mut ApplicationWindow, input: &mut HeadlessInput, phase: PointerPhase, x: f32, y: f32) {
        input.pointer(window.document.context_mut(), phase, x, y).expect("指针");
    }

    fn row_center(window: &ApplicationWindow, path: &str) -> (f32, f32) {
        let rows = marks(window);
        let row = rows.iter().find(|row| row.path == path).unwrap_or_else(|| panic!("没有行 {path}"));
        (row.bounds.x + row.bounds.width / 2.0, row.bounds.y + row.bounds.height / 2.0)
    }

    fn empty_list_point(window: &ApplicationWindow) -> (f32, f32) {
        let rows = marks(window);
        let list = list_bounds(window.document.context().world(), &rows).expect("文件列表");
        let candidates = [
            (list.x + 2.0, list.y + list.height - 2.0),
            (list.x + list.width - 2.0, list.y + list.height - 2.0),
            (list.x + list.width - 2.0, list.y + 2.0),
            (list.x + 2.0, list.y + 2.0),
        ];
        candidates
            .into_iter()
            .find(|(x, y)| contains(list, *x, *y) && rows.iter().all(|row| !contains(row.bounds, *x, *y)))
            .unwrap_or_else(|| panic!("列表里没有空白点：{list:?} {:?}", rows.iter().map(|row| row.bounds).collect::<Vec<_>>()))
    }

    fn marks(window: &ApplicationWindow) -> Vec<RowMark> {
        let document = &window.document;
        collect_rows(document.context().world(), document.document())
    }

    fn files_model() -> ShellViewModel {
        let mut model = ShellViewModel::default();
        model.workspace.startup.finish();
        model.workspace.apply_repository_list(None, Ok(vec![WorkspaceRepository {
            repo_id: "repo".into(),
            name: "动画素材".into(),
            path: "D:/Libraries/Anime".into(),
            status: "ready".into(),
            backend_plugin_id: "local".into(),
            capabilities: vec!["write".into()],
            cache_required: false,
            cache_status: String::new(),
        }]));
        model.reduce(ShellMessage::FileBrowserLoaded(Ok(snapshot())));
        model
    }

    fn snapshot() -> FileBrowserSnapshot {
        let entries = ["photos", "audio", "cover.png"]
            .into_iter()
            .map(|name| FileBrowserEntry {
                path: name.into(),
                name: name.into(),
                kind: if name == "cover.png" { "file" } else { "directory" }.into(),
                extension: None,
                size_bytes: None,
                size_label: None,
                modified_at: None,
                asset_id: None,
                status: None,
                thumbnail_path: None,
                thumbnail_custom: false,
                hardlink_group_id: None,
                hardlink_state: None,
                tags: Vec::new(),
                alias_paths: Vec::new(),
                folder_metadata: None,
                metadata: std::collections::BTreeMap::new(),
                is_virtual: false,
                provider_id: None,
                provider_item_id: None,
                source_payload: None,
                local_absolute_path: None,
            })
            .collect::<Vec<_>>();
        FileBrowserSnapshot {
            repo_id: "repo".into(),
            root_path: "D:/Libraries/Anime".into(),
            backend_plugin_id: "local".into(),
            backend_kind: "local".into(),
            cache_state: RepositoryStructureCacheState::Ready,
            indexed_at: None,
            current_path: String::new(),
            total_entries: entries.len(),
            loaded_count: entries.len(),
            next_offset: None,
            has_more: false,
            special_location: None,
            tree: None,
            entries,
        }
    }
}
