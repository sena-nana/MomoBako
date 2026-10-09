//! 实况文档上的指针拖动和框选。
//!
//! Nana 视图没有通用的拖动事件。`prepare` 在拆树之前读按下目标和坐标，
//! 再交给已经对照过 Vue 的输入归约。行上的 `momobako-path:` 文本是命中标记。

use nana_ui::runtime::{LayoutBox, RuntimeDocument, StableNodeId, UiWorld};

use super::input::{InputMessage, LiveGesture};
use super::{ShellMessage, ShellViewModel, WorkspacePanel, WorkspaceRepository};

const POINTER_ID: u64 = 1;
const PATH_MARK: &str = "momobako-entry:";
/// Vue `useFileBrowserPanelViewModel.ts` 的 `dragStartThreshold`。
const ENTRY_DRAG_PX: f32 = 7.0;
/// 同一文件里框选开始移动的阈值：任一轴超过 3px。
const BOX_DRAG_PX: f32 = 3.0;

struct RowMark {
    kind: String,
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
    note_chrome_hover(model, world, document_id, position);
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
        let hover_folder = directory_at(&rows, x, y, &path);
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
    if let Some(previous) = model.sidebar.hover_folder.clone() {
        model.reduce(ShellMessage::Sidebar(super::SidebarMessage::Gap(super::GapMessage::FolderLeave(previous))));
    }
    let rows = collect_rows(world, document_id);
    let list = list_bounds(world, &rows);
    let (x, y) = position.unwrap_or((gesture.x, gesture.y));
    let over_browser = list.is_some_and(|bounds| contains(bounds, x, y));
    if let Some(path) = gesture.row.clone() {
        if gesture.armed {
            let hover_folder = directory_at(&rows, x, y, &path);
            model.reduce(ShellMessage::Input(InputMessage::EntryDragEnd {
                hover_folder,
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

/// 离开当前行或换到另一行时清掉 450ms 计时，再为新行重新开始。
fn note_folder_hover(model: &mut ShellViewModel, hit: Option<String>) {
    if let Some(previous) = model.sidebar.hover_folder.clone() {
        if hit.as_deref() != Some(previous.as_str()) {
            model.reduce(ShellMessage::Sidebar(super::SidebarMessage::Gap(super::GapMessage::FolderLeave(previous))));
        }
    }
    if let Some(folder) = hit {
        model.reduce(ShellMessage::Sidebar(super::SidebarMessage::Gap(super::GapMessage::FolderHover {
            path: folder,
            now_ms: model.sidebar.hover_now(),
            dragging: true,
        })));
    }
}

fn begin_entry_drag(model: &mut ShellViewModel, path: &str, x: f32, y: f32, over_browser: bool, hover_folder: Option<String>) {
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
        hover_folder,
        over_browser,
    }));
}

fn directory_at(rows: &[RowMark], x: f32, y: f32, exclude: &str) -> Option<String> {
    rows.iter()
        .find(|row| row.kind == "directory" && row.path != exclude && contains(row.bounds, x, y))
        .map(|row| row.path.clone())
}

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
    let over_tools = hover.is_some_and(|id| label_in(world, id, &["快捷方式", "快捷访问", "文件夹", "智能文件夹", "播放集"]));
    let over_footer = hover.is_some_and(|id| label_in(world, id, &["设置", "拓展", "日志"]) || footer_task(world, id));
    model.motion.set_tools_hover(over_tools);
    model.motion.set_footer_hover(over_footer);
}

fn label_in(world: &UiWorld, id: StableNodeId, labels: &[&str]) -> bool {
    let mut current = Some(id);
    while let Some(node) = current {
        if node_names(world, node).any(|name| labels.contains(&name)) {
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
fn node_names(world: &UiWorld, node: StableNodeId) -> impl Iterator<Item = &str> {
    world
        .text(node)
        .into_iter()
        .chain(world.accessibility(node).and_then(|state| state.label.as_deref()))
}

/// 文件表面已经挂上时，行里应该有路径标记。
fn file_rows_should_be_mounted(model: &ShellViewModel) -> bool {
    model.workspace.startup.status == super::StartupStatus::Ready
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
        rows.push(RowMark { kind: kind.to_string(), path: path.to_string(), node, bounds });
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
    fn live_folder_button_escape_and_prefetch_use_the_shell_path() {
        let mut model = files_model();
        model.sidebar.current_directory = "photos".into();
        model.sidebar.folders.push(crate::shell::SidebarFolder {
            path: "photos".into(),
            label: "照片".into(),
            children: Vec::new(),
        });
        let mut window = mounted(&model);
        let mut input = bind(&mut window);
        let create = labeled_center(&window, "在当前目录新建文件夹");
        pointer(&mut window, &mut input, PointerPhase::Down, create.0, create.1);
        pointer(&mut window, &mut input, PointerPhase::Up, create.0, create.1);
        reduce_queued(&mut model, &mut window);
        assert!(model.sidebar.folder_dialog.open);
        assert_eq!(model.sidebar.folder_dialog.title(), "新建文件夹");

        let mut window = mounted(&model);
        let mut input = bind(&mut window);
        prepare_motion(&mut model, &mut window);
        window.document.flush(LayoutViewport::new(1200.0, 800.0), &mut NanaTextShaper::default()).expect("布局");
        assert!(model.sidebar.folder_dialog.open, "对话框还在时不能把它当成已经关掉");
        let field = labeled_input_id(&window, "文件夹名称");
        let document_id = window.document.document();
        window.document.context_mut().focus_node(document_id, field).expect("焦点");
        press_escape(&mut window, &mut input);
        prepare_motion(&mut model, &mut window);
        assert!(!model.sidebar.folder_dialog.open, "焦点在对话框输入框时 Escape 应该关掉它");

        let mut model = files_model();
        model.reduce(ShellMessage::FileBrowserLoaded(Ok(thumbnail_snapshot())));
        assert!(model.files.prefetch_pending());
        assert!(model.files.take_effects().iter().all(|effect| !matches!(effect, crate::shell::FilesEffect::DecodeThumbnails { .. })));
        let mut window = mounted(&model);
        prepare_motion(&mut model, &mut window);
        assert!(model.files.take_effects().iter().all(|effect| !matches!(effect, crate::shell::FilesEffect::DecodeThumbnails { .. })));
        for _ in 0..30 {
            prepare_motion(&mut model, &mut window);
        }
        assert!(
            model.files.take_effects().iter().any(|effect| matches!(effect, crate::shell::FilesEffect::DecodeThumbnails { paths } if paths == &["cover.png".to_string()])),
            "空闲 420ms 后才解码缩略图"
        );
    }

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

        let mut model = files_model();
        let mut window = mounted(&model);
        let mut input = bind(&mut window);
        let cover = row_center(&window, "cover.png");
        let photos = row_center(&window, "photos");
        pointer(&mut window, &mut input, PointerPhase::Down, cover.0, cover.1);
        observe_live_pointer(&mut model, &window.document);
        pointer(&mut window, &mut input, PointerPhase::Move, photos.0, photos.1);
        observe_live_pointer(&mut model, &window.document);
        pointer(&mut window, &mut input, PointerPhase::Up, photos.0, photos.1);
        observe_live_pointer(&mut model, &window.document);
        let effects = model.files.take_effects();
        assert!(
            effects.iter().any(|effect| matches!(effect, crate::shell::FilesEffect::Move { parent, .. } if parent == "photos")),
            "拖到 photos 上应该移进该文件夹：{effects:?}"
        );
    }

    fn prepare_motion_tracks(model: &mut ShellViewModel, window: &mut ApplicationWindow) -> bool {
        prepare_motion(model, window);
        model.input.live_gesture.is_some()
    }

    fn mounted(model: &ShellViewModel) -> ApplicationWindow {
        mounted_at(model, 1200.0, 800.0)
    }

    fn mounted_at(model: &ShellViewModel, width: f32, height: f32) -> ApplicationWindow {
        let mut window = ApplicationWindow::new();
        window.document = acceptance_document_for_model(model.clone()).expect("生产文档");
        window.document.flush(LayoutViewport::new(width, height), &mut NanaTextShaper::default()).expect("布局");
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

    /// Vue `.workspace-sidebar__footer:hover`：悬停任一底部入口，整排从半透明变成不透明。
    #[test]
    fn hovering_a_footer_button_lights_up_the_whole_footer() {
        let mut model = files_model();
        let mut window = mounted(&model);
        let mut input = bind(&mut window);
        assert!(model.motion.footer_opacity() < 1.0, "底部入口平时半透明");
        let settings = labeled_center(&window, "设置");
        pointer(&mut window, &mut input, PointerPhase::Move, settings.0, settings.1);
        for _ in 0..40 {
            prepare_motion(&mut model, &mut window);
            window.document.flush(LayoutViewport::new(1200.0, 800.0), &mut NanaTextShaper::default()).expect("布局");
        }
        assert_eq!(model.motion.footer_opacity(), 1.0, "悬停底部入口时整排不透明");
    }

    #[test]
    fn live_popover_opens_under_the_anchor_inside_the_viewport() {
        let mut model = files_model();
        let mut window = mounted_at(&model, 420.0, 280.0);
        let mut input = bind(&mut window);
        let anchor = labeled_bounds(&window, "资源库 · 动画素材");
        let center = (anchor.x + anchor.width / 2.0, anchor.y + anchor.height / 2.0);
        pointer(&mut window, &mut input, PointerPhase::Down, center.0, center.1);
        pointer(&mut window, &mut input, PointerPhase::Up, center.0, center.1);
        reduce_queued(&mut model, &mut window);
        crate::shell::mount_shell(&mut window.document, &model).expect("挂上弹层");
        window.document.flush(LayoutViewport::new(420.0, 280.0), &mut NanaTextShaper::default()).expect("布局");
        prepare_motion(&mut model, &mut window);
        window.document.flush(LayoutViewport::new(420.0, 280.0), &mut NanaTextShaper::default()).expect("布局");
        // Vue `getPopoverPosition`：左边对齐仓库头按钮，顶边在按钮下方 6px，左右留 8px 视口边距。
        let row = labeled_bounds(&window, "切换资源库 动画素材");
        assert!(row.x >= 8.0 && row.x + row.width <= 420.0 - 8.0, "弹层应留在视口里：{row:?}");
        assert!(row.y >= anchor.y + anchor.height + 6.0, "弹层应在仓库头下方：{row:?} {anchor:?}");
    }

    #[test]
    fn live_folder_hover_opens_after_the_idle_clock_reaches_450ms() {
        let mut model = hover_model();
        let mut window = mounted(&model);
        let mut input = bind(&mut window);
        let cover = row_center(&window, "cover.png");
        let photos = row_center(&window, "photos");
        pointer(&mut window, &mut input, PointerPhase::Down, cover.0, cover.1);
        prepare_motion(&mut model, &mut window);
        pointer(&mut window, &mut input, PointerPhase::Move, photos.0, photos.1);
        prepare_motion(&mut model, &mut window);
        assert_ne!(model.current_directory, "photos", "刚悬停还不到 450ms");
        for _ in 0..40 {
            prepare_motion(&mut model, &mut window);
        }
        assert_eq!(model.current_directory, "photos");
        assert!(!tree_selects_folder(&window, "照片"), "按着的时候不拆树");
        pointer(&mut window, &mut input, PointerPhase::Up, photos.0, photos.1);
        prepare_motion(&mut model, &mut window);
        let requests = crate::sidebar_dispatch::dispatch_prepared_browses(&mut model);
        window.document.flush(LayoutViewport::new(1200.0, 800.0), &mut NanaTextShaper::default()).expect("布局");
        assert!(
            requests.iter().any(|request| request.directory_path.as_deref() == Some("photos")),
            "目录浏览已提交：{requests:?}"
        );
        assert!(
            model.sidebar.take_effects().iter().all(|effect| !matches!(effect, crate::shell::SidebarEffect::Browse { path, .. } if path == "photos")),
            "浏览请求不能继续留在侧栏队列"
        );
        assert_eq!(model.files.current_path, "photos");
        assert_eq!(model.files.activity, "正在读取目录…");
        assert_eq!(model.current_directory, "photos");
        assert!(tree_selects_folder(&window, "照片"), "松手后的树应选中 photos");

        let mut model = hover_model();
        let mut window = mounted(&model);
        let mut input = bind(&mut window);
        let cover = row_center(&window, "cover.png");
        let photos = row_center(&window, "photos");
        pointer(&mut window, &mut input, PointerPhase::Down, cover.0, cover.1);
        prepare_motion(&mut model, &mut window);
        pointer(&mut window, &mut input, PointerPhase::Move, photos.0, photos.1);
        prepare_motion(&mut model, &mut window);
        pointer(&mut window, &mut input, PointerPhase::Move, cover.0, cover.1);
        for _ in 0..32 {
            prepare_motion(&mut model, &mut window);
        }
        pointer(&mut window, &mut input, PointerPhase::Move, photos.0, photos.1);
        prepare_motion(&mut model, &mut window);
        let requests = crate::sidebar_dispatch::dispatch_prepared_browses(&mut model);
        assert!(requests.is_empty(), "离开后再进入不应立刻打开：{requests:?}");
        assert_ne!(model.files.current_path, "photos");
        assert_ne!(model.current_directory, "photos");
    }

    #[test]
    fn live_file_drop_imports_and_empty_drop_attaches() {
        use std::path::PathBuf;
        use nana_ui::{FileDragInput, FileDragKind, InputModifiers, InputPayload};

        let mut model = files_model();
        let mut window = mounted(&model);
        let mut input = bind(&mut window);
        let point = drop_point(&window, "files");
        drag_file(&mut window, &mut input, FileDragKind::Hover, "D:\\other\\c.png", point);
        reduce_queued(&mut model, &mut window);
        assert!(model.input.dragging_files, "悬停外部文件应标成正在拖入");
        drag_file(&mut window, &mut input, FileDragKind::Drop, "D:\\other\\c.png", point);
        reduce_queued(&mut model, &mut window);
        let effects = model.files.take_effects();
        assert!(
            effects.iter().any(|effect| matches!(effect, crate::shell::FilesEffect::Import { sources, .. } if sources == &["D:\\other\\c.png".to_string()])),
            "外部放下应该导入：{effects:?}"
        );

        let mut model = empty_library();
        let mut window = mounted(&model);
        let mut input = bind(&mut window);
        let point = drop_point(&window, "empty");
        drag_file(&mut window, &mut input, FileDragKind::Drop, "C:\\library", point);
        reduce_queued(&mut model, &mut window);
        let effects = model.sidebar.take_effects();
        assert!(
            effects.iter().any(|effect| matches!(effect, crate::shell::SidebarEffect::AttachRepository { path } if path == "C:\\library")),
            "空库放下应该附加文件夹：{effects:?}"
        );

        fn drag_file(window: &mut ApplicationWindow, input: &mut HeadlessInput, kind: FileDragKind, path: &str, point: (f32, f32)) {
            input.route(window.document.context_mut(), InputPayload::FileDrag(FileDragInput {
                kind,
                paths: vec![PathBuf::from(path)],
                position: Some(point),
                modifiers: InputModifiers::default(),
            })).expect("文件拖放");
        }
    }

    fn empty_library() -> ShellViewModel {
        let mut model = ShellViewModel::default();
        model.workspace.startup.finish();
        model.workspace.apply_repository_list(None, Ok(Vec::new()));
        model
    }

    fn drop_point(window: &ApplicationWindow, kind: &str) -> (f32, f32) {
        let mark = format!("momobako-drop:{kind}");
        let document = window.document.document();
        let world = window.document.context().world();
        let marker = world.document_order(document).into_iter().find(|id| world.text(*id) == Some(mark.as_str())).unwrap_or_else(|| panic!("没有拖放标记 {kind}"));
        let host = world.parent_id(marker).unwrap_or(marker);
        let bounds = world.layout_box(host).unwrap_or_else(|| panic!("拖放目标没有布局"));
        assert!(bounds.width >= 40.0 && bounds.height >= 40.0, "拖放目标太小：{bounds:?}");
        (bounds.x + bounds.width / 2.0, bounds.y + bounds.height / 2.0)
    }

    fn hover_model() -> ShellViewModel {
        let mut model = files_model();
        model.sidebar.folders.push(crate::shell::SidebarFolder {
            path: "photos".into(),
            label: "照片".into(),
            children: Vec::new(),
        });
        model
    }

    /// 目录树里名为 `label` 的文件夹行处于选中态。
    fn tree_selects_folder(window: &ApplicationWindow, label: &str) -> bool {
        accessibility(window)
            .into_iter()
            .any(|node| node.label.as_deref() == Some(label) && node.selected == Some(true))
    }

    fn press_escape(window: &mut ApplicationWindow, input: &mut HeadlessInput) {
        let escape = nana_ui::KeyInput {
            physical: nana_ui_platform::PhysicalKey("Escape".into()),
            logical: nana_ui_platform::LogicalKey("Escape".into()),
            state: nana_ui::KeyState::Pressed,
            repeat: false,
            modifiers: nana_ui::InputModifiers::default(),
        };
        input.press(window.document.context_mut(), escape, None, None).expect("Escape");
    }

    fn labeled_center(window: &ApplicationWindow, label: &str) -> (f32, f32) {
        let node = accessibility(window).into_iter().find(|node| node.label.as_deref() == Some(label)).unwrap_or_else(|| panic!("没有 {label}"));
        (node.bounds.x + node.bounds.width / 2.0, node.bounds.y + node.bounds.height / 2.0)
    }

    /// 对话框字段上方有同名的可见标签，按输入框角色找。
    fn labeled_input_id(window: &ApplicationWindow, label: &str) -> nana_ui::runtime::StableNodeId {
        accessibility(window)
            .into_iter()
            .find(|node| node.role == nana_ui::runtime::AccessibilityRole::TextInput && node.label.as_deref() == Some(label))
            .unwrap_or_else(|| panic!("没有输入框 {label}"))
            .id
    }

    fn labeled_bounds(window: &ApplicationWindow, label: &str) -> LayoutBox {
        accessibility(window).into_iter().find(|node| node.label.as_deref() == Some(label)).unwrap_or_else(|| panic!("没有 {label}")).bounds
    }

    fn accessibility(window: &ApplicationWindow) -> Vec<nana_ui::runtime::AccessibilityNode> {
        let document = window.document.document();
        window.document.context().world().project_accessibility(document)
    }

    fn reduce_queued(model: &mut ShellViewModel, window: &mut ApplicationWindow) {
        let queued = window.document.context_mut().take_program_messages();
        assert!(!queued.is_empty(), "点击没有进入程序消息");
        for message in queued {
            let message = message.downcast::<ShellMessage>().expect("壳层消息");
            model.reduce(*message);
        }
    }

    fn thumbnail_snapshot() -> FileBrowserSnapshot {
        let mut shot = snapshot();
        shot.has_more = true;
        shot.entries[2].thumbnail_path = Some("cover.png".into());
        shot
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
