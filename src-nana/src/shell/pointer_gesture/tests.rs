//! 指针手势的测试：文件夹按钮与预取、拖动与框选阈值、页脚悬停、弹层定位、文件夹悬停与文件投放。

use super::*;
use crate::backend::services::repository::{
    FileBrowserEntry, FileBrowserSnapshot, RepositoryStructureCacheState,
};
use nana_ui::runtime::LayoutViewport;
use nana_ui::{ApplicationWindow, HeadlessInput, NanaTextShaper, PointerPhase};

use crate::acceptance_document_for_model;
use crate::shell::view_host::prepare_registered as prepare_motion;
use nana_ui::runtime::LayoutBox;

include!("sidebar_drop_tests.rs");

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
    pointer(
        &mut model,
        &mut window,
        &mut input,
        PointerPhase::Down,
        create.0,
        create.1,
    );
    pointer(
        &mut model,
        &mut window,
        &mut input,
        PointerPhase::Up,
        create.0,
        create.1,
    );
    assert!(model.sidebar.folder_dialog.open);
    assert_eq!(model.sidebar.folder_dialog.title(), "新建文件夹");

    let mut window = mounted(&model);
    let mut input = bind(&mut window);
    prepare_motion(&mut model, &mut window);
    window
        .document
        .flush(
            LayoutViewport::new(1200.0, 800.0),
            &mut NanaTextShaper::default(),
        )
        .expect("布局");
    assert!(
        model.sidebar.folder_dialog.open,
        "对话框还在时不能把它当成已经关掉"
    );
    let field = labeled_input_id(&window, "文件夹名称");
    let document_id = window.document.document();
    window
        .document
        .context_mut()
        .focus_node(document_id, field)
        .expect("焦点");
    press_escape(&mut model, &mut window, &mut input);
    prepare_motion(&mut model, &mut window);
    assert!(
        !model.sidebar.folder_dialog.open,
        "焦点在对话框输入框时 Escape 应该关掉它"
    );

    let mut model = files_model();
    model.reduce(ShellMessage::FileBrowserLoaded(Ok(thumbnail_snapshot())));
    assert!(model.files.prefetch_pending());
    assert!(
        model
            .files
            .take_effects()
            .iter()
            .all(|effect| !matches!(effect, crate::shell::FilesEffect::DecodeThumbnails { .. }))
    );
    let mut window = mounted(&model);
    prepare_motion(&mut model, &mut window);
    assert!(
        model
            .files
            .take_effects()
            .iter()
            .all(|effect| !matches!(effect, crate::shell::FilesEffect::DecodeThumbnails { .. }))
    );
    for _ in 0..30 {
        prepare_motion(&mut model, &mut window);
    }
    assert!(
        model.files.take_effects().iter().any(|effect| matches!(effect, crate::shell::FilesEffect::DecodeThumbnails { paths } if paths == &["cover.png".to_string()])),
        "空闲 420ms 后才解码缩略图"
    );
}

/// 拖动容差归 NanaUI（4px）：走不出容差就是普通点击。条目拖动走过 7px 才起步，
/// 4–7px 之间松手仍按单击选中。空白处点一下清空选择，拖出框就是框选。
#[test]
fn live_pointer_drag_and_box_select_keep_their_thresholds() {
    let mut model = files_model();
    let mut window = mounted(&model);
    let mut input = bind(&mut window);
    let cover = row_center(&window, "cover.png");
    pointer(
        &mut model,
        &mut window,
        &mut input,
        PointerPhase::Down,
        cover.0,
        cover.1,
    );
    pointer(
        &mut model,
        &mut window,
        &mut input,
        PointerPhase::Move,
        cover.0 + 4.0,
        cover.1,
    );
    assert!(
        !prepare_motion_tracks(&mut model, &mut window),
        "4px 还在框架的拖动容差里，不是拖动"
    );
    pointer(
        &mut model,
        &mut window,
        &mut input,
        PointerPhase::Move,
        cover.0 + 6.0,
        cover.1,
    );
    assert!(
        prepare_motion_tracks(&mut model, &mut window),
        "越过容差以后卡片收到拖动"
    );
    assert!(
        !model.input.internal_active,
        "6px 还没到 7px 的条目拖动阈值"
    );
    pointer(
        &mut model,
        &mut window,
        &mut input,
        PointerPhase::Up,
        cover.0 + 6.0,
        cover.1,
    );
    assert!(!prepare_motion_tracks(&mut model, &mut window), "松手收尾");
    assert!(!model.input.internal_active);
    assert_eq!(
        model.files.selected_paths(),
        &["cover.png".to_string()],
        "没到 7px 就松手仍按单击选中"
    );

    let mut model = files_model();
    let mut window = mounted(&model);
    let mut input = bind(&mut window);
    let cover = row_center(&window, "cover.png");
    pointer(
        &mut model,
        &mut window,
        &mut input,
        PointerPhase::Down,
        cover.0,
        cover.1,
    );
    pointer(
        &mut model,
        &mut window,
        &mut input,
        PointerPhase::Move,
        cover.0 + 20.0,
        cover.1,
    );
    observe_live_pointer(&mut model, &window.document);
    assert!(model.input.internal_active);
    assert_eq!(model.files.selected_paths(), &["cover.png".to_string()]);

    let mut model = files_model();
    model.files.set_selected_paths(vec!["photos".into()]);
    let mut window = mounted(&model);
    let mut input = bind(&mut window);
    let empty = empty_list_point(&window);
    pointer(
        &mut model,
        &mut window,
        &mut input,
        PointerPhase::Down,
        empty.0,
        empty.1,
    );
    pointer(
        &mut model,
        &mut window,
        &mut input,
        PointerPhase::Up,
        empty.0,
        empty.1,
    );
    observe_live_pointer(&mut model, &window.document);
    assert!(
        model.files.selected_paths().is_empty(),
        "空白处点一下要清空选择"
    );

    let mut model = files_model();
    let mut window = mounted(&model);
    let mut input = bind(&mut window);
    let empty = empty_list_point(&window);
    let cover = row_center(&window, "cover.png");
    pointer(
        &mut model,
        &mut window,
        &mut input,
        PointerPhase::Down,
        empty.0,
        empty.1,
    );
    pointer(
        &mut model,
        &mut window,
        &mut input,
        PointerPhase::Move,
        cover.0,
        cover.1,
    );
    observe_live_pointer(&mut model, &window.document);
    assert!(model.input.selection_box.visible, "框选时画出选框");
    let selected = model.files.selected_paths();
    assert!(
        selected.iter().any(|path| path == "cover.png"),
        "框选应该盖住 cover.png：{selected:?}"
    );
    assert!(
        selected.len() >= 2,
        "从列表空白拖到封面应该框住不止一行：{selected:?}"
    );
    pointer(
        &mut model,
        &mut window,
        &mut input,
        PointerPhase::Up,
        cover.0,
        cover.1,
    );
    observe_live_pointer(&mut model, &window.document);
    assert!(!model.input.selection_box.visible, "松手收起选框");
    assert!(
        model.files.selected_paths().len() >= 2,
        "框选松手留下选择，不当成点空白"
    );

    let mut model = files_model();
    let mut window = mounted(&model);
    let mut input = bind(&mut window);
    let cover = row_center(&window, "cover.png");
    let photos = row_center(&window, "photos");
    pointer(
        &mut model,
        &mut window,
        &mut input,
        PointerPhase::Down,
        cover.0,
        cover.1,
    );
    pointer(
        &mut model,
        &mut window,
        &mut input,
        PointerPhase::Move,
        photos.0,
        photos.1,
    );
    observe_live_pointer(&mut model, &window.document);
    pointer(
        &mut model,
        &mut window,
        &mut input,
        PointerPhase::Up,
        photos.0,
        photos.1,
    );
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
    window
        .document
        .flush(
            LayoutViewport::new(width, height),
            &mut NanaTextShaper::default(),
        )
        .expect("布局");
    window
}

fn bind(window: &mut ApplicationWindow) -> HeadlessInput {
    let document_id = window.document.document();
    HeadlessInput::bind(window.document.context_mut(), document_id)
}

/// 照生产路径送一次鼠标输入：运行时路由，`input_event` 看路由完的输入，再把排着的消息归约掉。
fn pointer(
    model: &mut ShellViewModel,
    window: &mut ApplicationWindow,
    input: &mut HeadlessInput,
    phase: PointerPhase,
    x: f32,
    y: f32,
) {
    let payload = nana_ui::InputPayload::Pointer(nana_ui::PointerInput::mouse(phase, x, y));
    input
        .route(window.document.context_mut(), payload.clone())
        .expect("指针");
    crate::shell::note_routed_input(model, &window.document, &payload);
    for message in window.document.context_mut().take_program_messages() {
        model.reduce(*message.downcast::<ShellMessage>().expect("壳层消息"));
    }
}

fn row_center(window: &ApplicationWindow, path: &str) -> (f32, f32) {
    let (_, _, bounds) = targets::cards(&window.document)
        .into_iter()
        .find(|(_, row, _)| row == path)
        .unwrap_or_else(|| panic!("没有行 {path}"));
    (
        bounds.x + bounds.width / 2.0,
        bounds.y + bounds.height / 2.0,
    )
}

/// 列表框里不压在卡片上的一点。先试上方两角（内容上边距里），从那里拖到卡片会扫过第一行。
fn empty_list_point(window: &ApplicationWindow) -> (f32, f32) {
    let cards = targets::cards(&window.document);
    let (_, list) = targets::browser(&window.document).expect("文件列表");
    let candidates = [
        (list.x + list.width - 2.0, list.y + 2.0),
        (list.x + 2.0, list.y + 2.0),
        (list.x + list.width - 2.0, list.y + list.height - 2.0),
        (list.x + 2.0, list.y + list.height - 2.0),
    ];
    candidates
        .into_iter()
        .find(|(x, y)| {
            contains(list, *x, *y)
                && cards
                    .iter()
                    .all(|(_, _, bounds)| !contains(*bounds, *x, *y))
        })
        .unwrap_or_else(|| panic!("列表里没有空白点：{list:?}"))
}

/// 悬停任一底部入口，整排从半透明淡入到不透明：样式表的 `footer-entry` 平时 0.44、`lit` 时 1，
/// 中间按 `transition: opacity 0.35s ease` 由 NanaUI 推进，从这一刻的透明度起步。
#[test]
fn hovering_a_footer_button_lights_up_the_whole_footer() {
    use nana_ui::runtime::{AnimatableProperty, MotionValue};
    use std::time::Duration;
    let mut model = files_model();
    let mut window = mounted(&model);
    let mut input = bind(&mut window);
    let log = accessibility(&window)
        .into_iter()
        .find(|node| node.label.as_deref() == Some("日志"))
        .expect("日志入口")
        .id;
    let opacity = |window: &ApplicationWindow, at: Duration| match window
        .document
        .context()
        .world()
        .presentation_motion_value(log, AnimatableProperty::Opacity, at)
    {
        Some(MotionValue::Scalar(value)) => value,
        other => panic!("日志入口的透明度：{other:?}"),
    };
    let start = window.document.context().animation_now();
    assert!(!model.input.footer_hover);
    assert_eq!(opacity(&window, start), 0.44, "底部入口平时半透明");
    let settings = labeled_center(&window, "设置");
    pointer(
        &mut model,
        &mut window,
        &mut input,
        PointerPhase::Move,
        settings.0,
        settings.1,
    );
    prepare_motion(&mut model, &mut window);
    window
        .document
        .flush(
            LayoutViewport::new(1200.0, 800.0),
            &mut NanaTextShaper::default(),
        )
        .expect("布局");
    assert!(model.input.footer_hover, "指针停在底栏上");
    let world = window.document.context().world();
    assert_eq!(
        world.node_style(log).and_then(|style| style.layout.opacity),
        Some(1.0),
        "悬停底部入口时整排的终点是不透明"
    );
    let now = window.document.context().animation_now();
    assert_eq!(opacity(&window, now), 0.44, "从这一刻的透明度起步");
    let halfway = opacity(&window, now + Duration::from_millis(175));
    assert!(halfway > 0.44 && halfway < 1.0, "淡入中：{halfway}");
    assert_eq!(
        opacity(&window, now + Duration::from_millis(350)),
        1.0,
        "0.35s 以后不透明"
    );
}

#[test]
fn live_popover_opens_under_the_anchor_inside_the_viewport() {
    let mut model = files_model();
    let mut window = mounted_at(&model, 420.0, 280.0);
    let mut input = bind(&mut window);
    let anchor = labeled_bounds(&window, "资源库 · 动画素材");
    let center = (
        anchor.x + anchor.width / 2.0,
        anchor.y + anchor.height / 2.0,
    );
    pointer(
        &mut model,
        &mut window,
        &mut input,
        PointerPhase::Down,
        center.0,
        center.1,
    );
    pointer(
        &mut model,
        &mut window,
        &mut input,
        PointerPhase::Up,
        center.0,
        center.1,
    );
    crate::shell::mount_shell(&mut window.document, &model).expect("挂上弹层");
    window
        .document
        .flush(
            LayoutViewport::new(420.0, 280.0),
            &mut NanaTextShaper::default(),
        )
        .expect("布局");
    prepare_motion(&mut model, &mut window);
    window
        .document
        .flush(
            LayoutViewport::new(420.0, 280.0),
            &mut NanaTextShaper::default(),
        )
        .expect("布局");
    // 弹层左边对齐仓库头按钮，顶边在按钮下方 6px，左右留 8px 视口边距。
    let row = labeled_bounds(&window, "切换资源库 动画素材");
    assert!(
        row.x >= 8.0 && row.x + row.width <= 420.0 - 8.0,
        "弹层应留在视口里：{row:?}"
    );
    assert!(
        row.y >= anchor.y + anchor.height + 6.0,
        "弹层应在仓库头下方：{row:?} {anchor:?}"
    );
}

#[test]
fn live_folder_hover_opens_after_the_idle_clock_reaches_450ms() {
    let mut model = hover_model();
    let mut window = mounted(&model);
    let mut input = bind(&mut window);
    let cover = row_center(&window, "cover.png");
    let photos = row_center(&window, "photos");
    pointer(
        &mut model,
        &mut window,
        &mut input,
        PointerPhase::Down,
        cover.0,
        cover.1,
    );
    prepare_motion(&mut model, &mut window);
    pointer(
        &mut model,
        &mut window,
        &mut input,
        PointerPhase::Move,
        photos.0,
        photos.1,
    );
    prepare_motion(&mut model, &mut window);
    assert_ne!(model.current_directory, "photos", "刚悬停还不到 450ms");
    for _ in 0..40 {
        prepare_motion(&mut model, &mut window);
    }
    assert_eq!(model.current_directory, "photos");
    assert!(!tree_selects_folder(&window, "照片"), "按着的时候不拆树");
    pointer(
        &mut model,
        &mut window,
        &mut input,
        PointerPhase::Up,
        photos.0,
        photos.1,
    );
    prepare_motion(&mut model, &mut window);
    let requests = crate::dispatch::sidebar::dispatch_prepared_browses(&mut model);
    window
        .document
        .flush(
            LayoutViewport::new(1200.0, 800.0),
            &mut NanaTextShaper::default(),
        )
        .expect("布局");
    assert!(
        requests
            .iter()
            .any(|request| request.directory_path.as_deref() == Some("photos")),
        "目录浏览已提交：{requests:?}"
    );
    assert!(
        model.sidebar.take_effects().iter().all(|effect| !matches!(effect, crate::shell::SidebarEffect::Browse { path, .. } if path == "photos")),
        "浏览请求不能继续留在侧栏队列"
    );
    assert_eq!(model.files.current_path, "photos");
    assert!(model.files.loading, "目录读回来以前文件列表显示读取中");
    assert_eq!(model.current_directory, "photos");
    assert!(
        tree_selects_folder(&window, "照片"),
        "松手后的树应选中 photos"
    );

    let mut model = hover_model();
    let mut window = mounted(&model);
    let mut input = bind(&mut window);
    let cover = row_center(&window, "cover.png");
    let photos = row_center(&window, "photos");
    pointer(
        &mut model,
        &mut window,
        &mut input,
        PointerPhase::Down,
        cover.0,
        cover.1,
    );
    prepare_motion(&mut model, &mut window);
    pointer(
        &mut model,
        &mut window,
        &mut input,
        PointerPhase::Move,
        photos.0,
        photos.1,
    );
    prepare_motion(&mut model, &mut window);
    pointer(
        &mut model,
        &mut window,
        &mut input,
        PointerPhase::Move,
        cover.0,
        cover.1,
    );
    for _ in 0..32 {
        prepare_motion(&mut model, &mut window);
    }
    pointer(
        &mut model,
        &mut window,
        &mut input,
        PointerPhase::Move,
        photos.0,
        photos.1,
    );
    prepare_motion(&mut model, &mut window);
    let requests = crate::dispatch::sidebar::dispatch_prepared_browses(&mut model);
    assert!(
        requests.is_empty(),
        "离开后再进入不应立刻打开：{requests:?}"
    );
    assert_ne!(model.files.current_path, "photos");
    assert_ne!(model.current_directory, "photos");
}

#[test]
fn live_file_drop_imports_and_empty_drop_attaches() {
    use nana_ui::{FileDragInput, FileDragKind, InputModifiers, InputPayload};
    use std::path::PathBuf;

    let mut model = files_model();
    let mut window = mounted(&model);
    let mut input = bind(&mut window);
    let point = drop_point(&window, "files");
    drag_file(
        &mut window,
        &mut input,
        FileDragKind::Hover,
        "D:\\other\\c.png",
        point,
    );
    reduce_queued(&mut model, &mut window);
    assert!(model.input.dragging_files, "悬停外部文件应标成正在拖入");
    drag_file(
        &mut window,
        &mut input,
        FileDragKind::Drop,
        "D:\\other\\c.png",
        point,
    );
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
    drag_file(
        &mut window,
        &mut input,
        FileDragKind::Drop,
        "C:\\library",
        point,
    );
    reduce_queued(&mut model, &mut window);
    let effects = model.sidebar.take_effects();
    assert!(
        effects.iter().any(|effect| matches!(effect, crate::shell::SidebarEffect::AttachRepository { path } if path == "C:\\library")),
        "空库放下应该附加文件夹：{effects:?}"
    );

    fn drag_file(
        window: &mut ApplicationWindow,
        input: &mut HeadlessInput,
        kind: FileDragKind,
        path: &str,
        point: (f32, f32),
    ) {
        input
            .route(
                window.document.context_mut(),
                InputPayload::FileDrag(FileDragInput {
                    kind,
                    paths: vec![PathBuf::from(path)],
                    position: Some(point),
                    modifiers: InputModifiers::default(),
                }),
            )
            .expect("文件拖放");
    }
}

fn empty_library() -> ShellViewModel {
    let mut model = ShellViewModel::default();
    model.workspace.startup.finish();
    model.workspace.apply_repository_list(None, Ok(Vec::new()));
    model
}

/// 拖放目标的中心：文件区是 `file-column`，空库页是 `empty-repository-page`。
fn drop_point(window: &ApplicationWindow, kind: &str) -> (f32, f32) {
    let key = match kind {
        "files" => "file-column",
        "empty" => "empty-repository-page",
        other => panic!("没有拖放目标 {other}"),
    };
    let document = window.document.document();
    let context = window.document.context();
    let world = context.world();
    let host = world
        .document_order(document)
        .into_iter()
        .find(|id| {
            context
                .assembly_path(*id)
                .is_some_and(|path| path.rsplit('/').next() == Some(key))
        })
        .unwrap_or_else(|| panic!("没有拖放目标 {kind}"));
    let bounds = world
        .layout_box(host)
        .unwrap_or_else(|| panic!("拖放目标没有布局"));
    assert!(
        bounds.width >= 40.0 && bounds.height >= 40.0,
        "拖放目标太小：{bounds:?}"
    );
    (
        bounds.x + bounds.width / 2.0,
        bounds.y + bounds.height / 2.0,
    )
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

/// 按一下 Escape：先经运行时路由，再照生产 `input_event` 走全局 Escape，发出的消息直接归约。
fn press_escape(
    model: &mut ShellViewModel,
    window: &mut ApplicationWindow,
    input: &mut HeadlessInput,
) {
    let escape = nana_ui::KeyInput {
        physical: nana_ui_platform::PhysicalKey("Escape".into()),
        logical: nana_ui_platform::LogicalKey("Escape".into()),
        state: nana_ui::KeyState::Pressed,
        repeat: false,
        modifiers: nana_ui::InputModifiers::default(),
    };
    let payload = nana_ui::InputPayload::Key(escape.clone());
    let outcome = input
        .press(window.document.context_mut(), escape, None, None)
        .expect("Escape");
    // 排着的消息（激活的对话框在路由时发的关闭请求）和生产一样先归约，再走全局 Escape。
    let routed = window.document.context_mut().take_program_messages();
    let global =
        crate::window_host::escape_message(model, &payload, outcome.disposition().prevent_default);
    for message in routed {
        model.reduce(*message.downcast::<ShellMessage>().expect("壳层消息"));
    }
    if let Some(message) = global {
        model.reduce(message);
    }
}

fn labeled_center(window: &ApplicationWindow, label: &str) -> (f32, f32) {
    let node = accessibility(window)
        .into_iter()
        .find(|node| node.label.as_deref() == Some(label))
        .unwrap_or_else(|| panic!("没有 {label}"));
    (
        node.bounds.x + node.bounds.width / 2.0,
        node.bounds.y + node.bounds.height / 2.0,
    )
}

/// 对话框字段上方有同名的可见标签，按输入框角色找。
fn labeled_input_id(window: &ApplicationWindow, label: &str) -> nana_ui::runtime::StableNodeId {
    accessibility(window)
        .into_iter()
        .find(|node| {
            node.role == nana_ui::runtime::AccessibilityRole::TextInput
                && node.label.as_deref() == Some(label)
        })
        .unwrap_or_else(|| panic!("没有输入框 {label}"))
        .id
}

fn labeled_bounds(window: &ApplicationWindow, label: &str) -> LayoutBox {
    accessibility(window)
        .into_iter()
        .find(|node| node.label.as_deref() == Some(label))
        .unwrap_or_else(|| panic!("没有 {label}"))
        .bounds
}

fn accessibility(window: &ApplicationWindow) -> Vec<nana_ui::runtime::AccessibilityNode> {
    let document = window.document.document();
    window
        .document
        .context()
        .world()
        .project_accessibility_in_viewport(document)
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

fn files_model() -> ShellViewModel {
    let mut model = ShellViewModel::default();
    model.workspace.present_repository(WorkspaceRepository {
        repo_id: "repo".into(),
        name: "动画素材".into(),
        path: "D:/Libraries/Anime".into(),
        status: "ready".into(),
        backend_plugin_id: "local".into(),
        capabilities: vec!["write".into()],
        cache_required: false,
        cache_status: String::new(),
        cache_path: None,
        login_expired: false,
        logged_out: false,
    });
    model.reduce(ShellMessage::FileBrowserLoaded(Ok(snapshot())));
    model
}

fn snapshot() -> FileBrowserSnapshot {
    let entries = ["photos", "audio", "cover.png"]
        .into_iter()
        .map(|name| FileBrowserEntry {
            path: name.into(),
            name: name.into(),
            kind: if name == "cover.png" {
                "file"
            } else {
                "directory"
            }
            .into(),
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
