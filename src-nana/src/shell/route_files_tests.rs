//! 常驻文件路由的回归：五千条目录里缩略图到达、选中和后台消息只改该改的那一张卡片，别的卡片节点和
//! 滚动位置都不动；元数据输入框和筛选栏输入框在组合输入中不被别的更新打断；一串更新以后的文档和
//! 同一 ViewModel 新挂的一样（无障碍树、组装路径、整份样式和选中禁用等状态）；外部拖放按归约时的
//! 面板决定；播放条岛不因为自己量出的宽度反复重建。

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::time::Instant;

use nana_ui::runtime::{
    component_descriptors, DocumentId, Entity, LayoutViewport, RuntimeDocument, ScrollOffset, ScrollView, StableNodeId,
};
use nana_ui::{FileDragInput, FileDragKind, HeadlessInput, InputModifiers, InputPayload, NanaTextShaper};

use crate::backend::services::repository::{FileBrowserEntry, SystemLogLocation, SystemLogRecord, SystemLogSource};
use crate::shell::files::{DisplayMode, FileRow, FilesEffect, FilesMessage};
use crate::shell::host_events::HostMessage;
use crate::shell::player::PlayerMessage;
use crate::shell::view_harness::ShellHarness;
use crate::shell::{InspectMessage, ShellMessage, ShellView, ShellViewModel, ThumbnailFrame, WorkspacePanel};

fn scene(name: &str) -> ShellViewModel {
    crate::shell::acceptance_gap_models()
        .into_iter()
        .find(|(scene, _)| *scene == name)
        .unwrap_or_else(|| panic!("没有场景 {name}"))
        .1
}

/// `index` 号文件，带一张还没解码的缩略图。名字带五位序号。
fn numbered_file(index: usize) -> FileRow {
    let name = format!("file-{index:05}.png");
    FileRow::from_entry(&FileBrowserEntry {
        path: name.clone(),
        name: name.clone(),
        kind: "file".into(),
        extension: Some("png".into()),
        size_bytes: Some(1024),
        size_label: Some("1 KB".into()),
        modified_at: None,
        asset_id: Some(name.clone()),
        status: None,
        thumbnail_path: Some(format!("thumbs/{name}")),
        thumbnail_custom: false,
        hardlink_group_id: None,
        hardlink_state: None,
        tags: Vec::new(),
        alias_paths: Vec::new(),
        folder_metadata: None,
        metadata: BTreeMap::new(),
        is_virtual: false,
        provider_id: None,
        provider_item_id: None,
        source_payload: None,
        local_absolute_path: None,
    })
}

/// 文件页场景换成 `count` 个文件的目录，导入菜单收起，按 `mode` 展示。
fn bulk_model(mode: DisplayMode, count: usize) -> ShellViewModel {
    let mut model = scene("live-files");
    model.files.import_open = false;
    model.files.eagle_open = false;
    model.files.display_mode = mode;
    model.files.rows = (0..count).map(numbered_file).collect();
    model
}

/// 多排几遍：量过行高的虚拟行要在下一遍放到量出的位置，首帧回报的列表宽度也要归约进来。
fn settle(harness: &mut ShellHarness) {
    for _ in 0..6 {
        harness.flush();
        for message in harness.take_messages() {
            harness.apply(message);
        }
    }
    harness.flush();
}

/// 文件列表的滚动区。
fn list_scroll(harness: &ShellHarness) -> Entity<ScrollView> {
    let document = harness.document();
    let context = document.context();
    let id = context
        .world()
        .nodes_of_component(document.document(), component_descriptors::SCROLL_VIEW.type_id)
        .find(|node| context.assembly_path(*node).is_some_and(|path| path.rsplit('/').next().is_some_and(|key| key.starts_with("files-scroll-"))))
        .expect("文件列表滚动区");
    Entity::from_stable_id(id)
}

fn scrolled(harness: &ShellHarness, scroll: Entity<ScrollView>) -> f32 {
    harness.document().context().world().scroll_offset(scroll.stable_id()).map_or(0.0, |offset| offset.y)
}

/// 建出来的卡片：条目键 → 卡片节点。
fn built_cards(harness: &ShellHarness) -> HashMap<String, StableNodeId> {
    let document = harness.document();
    let context = document.context();
    context
        .world()
        .document_order(document.document())
        .into_iter()
        .filter_map(|id| {
            let path = context.assembly_path(id)?;
            let key = path.rsplit('/').next()?.strip_prefix("file-row-")?.to_string();
            Some((key, id))
        })
        .collect()
}

/// 卡片缩略图盒里的纹理节点藏着没有（还没有纹理时藏着）。
fn thumbnail_hidden(harness: &ShellHarness, path: &str) -> bool {
    let frame = harness.keyed(&format!("file-preview-file:{path}")).expect("缩略图盒");
    let world = harness.document().context().world();
    let thumb = world.node(frame).and_then(|node| node.children.first().copied()).expect("纹理节点");
    world.node_style(thumb).is_some_and(|style| style.layout.hidden)
}

fn log_message() -> ShellMessage {
    ShellMessage::Host(HostMessage::LogRecorded(SystemLogRecord {
        id: "log-1".into(),
        timestamp: "2026-10-09T08:00:00Z".into(),
        level: "info".into(),
        category: "repository".into(),
        action: "sync".into(),
        message: "后台同步完成".into(),
        source: SystemLogSource { kind: "host".into(), label: Some("MomoBako".into()), plugin_id: None, repo_id: None },
        location: SystemLogLocation::default(),
        context: serde_json::json!({}),
    }))
}

fn thumbnail(path: &str) -> ThumbnailFrame {
    ThumbnailFrame { path: format!("thumbs/{path}"), natural_width: 100, natural_height: 100, width: 2, height: 2, rgba: vec![255; 16] }
}

/// 文档里每个节点的组装路径、无障碍状态（禁用、选中、勾选、忙、无效）和整份样式（含显隐和画笔键），按文档顺序。
fn structure(document: &RuntimeDocument) -> Vec<String> {
    let context = document.context();
    let world = context.world();
    let states = world
        .project_accessibility(document.document())
        .into_iter()
        .map(|node| {
            let state = format!(
                "disabled={} selected={:?} checked={:?} busy={} invalid={}",
                node.disabled, node.selected, node.checked, node.busy, node.invalid
            );
            (node.id, state)
        })
        .collect::<HashMap<_, _>>();
    world
        .document_order(document.document())
        .into_iter()
        .map(|id| {
            let path = context.assembly_path(id).unwrap_or_default();
            let style = world.node_style(id).map(|style| format!("{style:?}")).unwrap_or_default();
            format!("{path} {} {style}", states.get(&id).map_or("", String::as_str))
        })
        .collect()
}

/// 增量文档和同一 ViewModel 新挂的文档逐个节点比较组装路径、无障碍状态和整份样式。
/// `assert_same_as_fresh_mount` 只比角色、名称、值和布局盒；绑在样式、选中和禁用上的字段在这里比。
fn assert_same_structure_as_fresh_mount(harness: &mut ShellHarness) {
    harness.flush();
    let ours = structure(harness.document());
    assert!(ours.iter().any(|line| line.contains("NodeStyle {")), "没有取到节点样式，结构比较不起作用");
    let mut fresh = RuntimeDocument::new(DocumentId::new(1).expect("文档编号"));
    let _view = ShellView::mount(&mut fresh, &harness.model).expect("新挂对照文档");
    fresh.flush(LayoutViewport::new(1200.0, 800.0), &mut NanaTextShaper::default()).expect("对照文档布局");
    let theirs = structure(&fresh);
    if ours == theirs {
        return;
    }
    let first = ours.iter().zip(&theirs).position(|(a, b)| a != b).unwrap_or(ours.len().min(theirs.len()));
    panic!(
        "增量文档和新挂的结构不一样（{} 对 {} 个节点，第 {first} 个起不同）\n增量：{}\n新挂：{}",
        ours.len(),
        theirs.len(),
        ours.get(first).map_or("-", String::as_str),
        theirs.get(first).map_or("-", String::as_str)
    );
}

/// 五千条的目录滚到中间：一张缩略图到达、选中一行、一条无关的后台消息之后，建出来的卡片一张都不换，
/// 滚动位置不动，主区分支也不重挂；变了的字段在原节点上改。
#[test]
fn five_thousand_rows_keep_their_nodes_and_scroll_through_updates() {
    let mut harness = ShellHarness::mount(bulk_model(DisplayMode::Grid, 5000));
    settle(&mut harness);
    let scroll = list_scroll(&harness);
    harness.window.document.context_mut().scroll_to(scroll, ScrollOffset { x: 0.0, y: 400_000.0 }).expect("滚动");
    settle(&mut harness);
    let offset = scrolled(&harness, scroll);
    assert!(offset > 100_000.0, "先滚到中间：{offset}");
    let cards = built_cards(&harness);
    assert!(!cards.is_empty() && cards.len() < 200, "只建视口附近的卡片：{}", cards.len());
    let mut visible = cards.keys().cloned().collect::<Vec<_>>();
    visible.sort();
    let target = visible[visible.len() / 2].trim_start_matches("file:").to_string();
    let selected = visible[visible.len() / 2 + 1].trim_start_matches("file:").to_string();
    let branch = harness.route_branch();
    let remounts = harness.view_stats().remounts;
    assert!(thumbnail_hidden(&harness, &target), "缩略图还没到时纹理节点藏着");

    harness.apply(ShellMessage::ThumbnailPixels(vec![thumbnail(&target)]));
    settle(&mut harness);
    assert_eq!(built_cards(&harness), cards, "缩略图到达换掉了卡片节点");
    assert!(!thumbnail_hidden(&harness, &target), "缩略图到达后纹理节点应该露出来");
    assert!((scrolled(&harness, scroll) - offset).abs() < 0.5, "缩略图到达改了滚动位置");

    harness.apply(ShellMessage::Files(FilesMessage::ActivateRow(selected.clone())));
    settle(&mut harness);
    assert_eq!(built_cards(&harness), cards, "选中一行换掉了卡片节点");
    let row = cards[&format!("file:{selected}")];
    let selected_now = harness.nodes().into_iter().find(|node| node.id == row).and_then(|node| node.selected);
    assert_eq!(selected_now, Some(true), "选中的卡片应在原节点上标成选中");
    assert!((scrolled(&harness, scroll) - offset).abs() < 0.5, "选中改了滚动位置");

    harness.apply(log_message());
    settle(&mut harness);
    assert_eq!(built_cards(&harness), cards, "无关的后台消息换掉了卡片节点");
    assert!((scrolled(&harness, scroll) - offset).abs() < 0.5, "无关的后台消息改了滚动位置");
    assert_eq!(harness.route_branch(), branch, "常驻文件路由的分支不该重挂");
    assert_eq!(harness.view_stats().remounts, remounts, "这几条更新都不该重挂任何内容");
    // 滚动位置是界面状态，新挂的文档在顶上；滚回顶上再和新挂的比。
    harness.window.document.context_mut().scroll_to(scroll, ScrollOffset { x: 0.0, y: 0.0 }).expect("滚回顶上");
    settle(&mut harness);
    harness.assert_same_as_fresh_mount();
    assert_same_structure_as_fresh_mount(&mut harness);
}

/// 一批缩略图到达的耗时：五千条里两百张同时到，只有到达的卡片重算。打印耗时，只拦明显的退化。
#[test]
fn a_batch_of_thumbnails_only_recomputes_the_cards_that_arrived() {
    let mut harness = ShellHarness::mount(bulk_model(DisplayMode::Grid, 5000));
    settle(&mut harness);
    let cards = built_cards(&harness);
    let frames = (0..200).map(|index| thumbnail(&format!("file-{index:05}.png"))).collect::<Vec<_>>();
    let started = Instant::now();
    harness.apply(ShellMessage::ThumbnailPixels(frames));
    let synced = started.elapsed();
    harness.flush();
    let flushed = started.elapsed();
    eprintln!("Nana 五千条里两百张缩略图到达：归约加同步 {synced:?}，连同刷新绑定和布局 {flushed:?}");
    assert_eq!(built_cards(&harness), cards, "缩略图批量到达换掉了卡片节点");
    let started = Instant::now();
    harness.apply(log_message());
    let synced = started.elapsed();
    harness.flush();
    eprintln!("Nana 五千条目录里一条无关消息：同步 {synced:?}，连同刷新 {:?}", started.elapsed());
    assert!(flushed.as_secs_f32() < 5.0, "缩略图批量到达太慢：{flushed:?}");
}

/// 在元数据的注释框里组合输入：后台消息、缩略图、别的字段的改动和动效帧都不换掉输入框、不打断预编辑；
/// 提交后文字进草稿，输入框还是原来那个。
#[test]
fn metadata_input_keeps_its_preedit_through_updates() {
    let mut harness = ShellHarness::mount(scene("files-selected-metadata"));
    settle(&mut harness);
    let comment = harness.input_within("inspect-comment");
    harness.focus(comment);
    harness.compose("zhushi");
    let updates = vec![
        log_message(),
        ShellMessage::ThumbnailPixels(vec![ThumbnailFrame {
            path: "cover.png".into(),
            natural_width: 2,
            natural_height: 2,
            width: 2,
            height: 2,
            rgba: vec![255; 16],
        }]),
        ShellMessage::Inspect(InspectMessage::SetLink("https://example.com/b".into())),
        ShellMessage::Inspect(InspectMessage::SetRating(4)),
        ShellMessage::Player(PlayerMessage::SetVolume(0.4)),
    ];
    for message in updates {
        harness.apply(message);
        harness.flush();
        assert_eq!(harness.input_within("inspect-comment"), comment, "更新换掉了注释输入框");
        assert_eq!(harness.preedit(comment).as_deref(), Some("zhushi"), "更新打断了预编辑");
        assert_eq!(harness.focused(), Some(comment), "更新让注释框失焦");
    }
    for _ in 0..12 {
        harness.frame();
        assert_eq!(harness.preedit(comment).as_deref(), Some("zhushi"), "动效帧打断了预编辑");
    }
    harness.commit("注释");
    for message in harness.take_messages() {
        harness.apply(message);
    }
    harness.flush();
    assert_eq!(harness.input_within("inspect-comment"), comment, "提交后输入框不该重建");
    assert!(harness.value(comment).ends_with("注释"), "提交的文字没有进输入框：{}", harness.value(comment));
    assert!(harness.model.inspect.draft_comment().ends_with("注释"), "提交的文字没有进草稿");
    harness.assert_same_as_fresh_mount();
}

/// 受控的注释框：按键的消息还排在后台消息后面没归约时，同步不能把较旧的草稿写回输入框。
#[test]
fn metadata_draft_survives_a_background_message_ahead_of_its_keystroke() {
    let mut harness = ShellHarness::mount(scene("files-selected-metadata"));
    settle(&mut harness);
    let comment = harness.input_within("inspect-comment");
    harness.focus(comment);
    let before = harness.value(comment);
    harness.type_text("a");
    let queued = harness.take_messages();
    harness.apply(log_message());
    harness.flush();
    assert_eq!(harness.value(comment), format!("{before}a"), "后台消息的同步把刚打的字回滚了");
    for message in queued {
        harness.apply(message);
    }
    harness.flush();
    assert_eq!(harness.model.inspect.draft_comment(), format!("{before}a"));
    assert_eq!(harness.input_within("inspect-comment"), comment);
}

/// 文件页上的常驻筛选栏：颜色输入框组合输入中，后台消息、缩略图和音量都不换输入框、不断预编辑，
/// 主区一次都不重挂；提交后字落在原节点上。改筛选条件会切到搜索面板，那是换路由，不在这里。
#[test]
fn filter_bar_on_the_files_route_keeps_its_input_through_updates() {
    let mut harness = ShellHarness::mount(scene("live-files-plain"));
    settle(&mut harness);
    harness.apply(ShellMessage::Inspect(InspectMessage::ToggleFilterBar));
    settle(&mut harness);
    let bar = harness.keyed("workspace-filter-bar").expect("筛选栏");
    let input = harness.input("输入文件颜色");
    harness.focus(input);
    harness.compose("hong");
    let remounts = harness.view_stats().remounts;
    let updates = vec![
        log_message(),
        ShellMessage::ThumbnailPixels(vec![thumbnail("cover.png")]),
        ShellMessage::Player(PlayerMessage::SetVolume(0.4)),
    ];
    for message in updates {
        harness.apply(message);
        harness.flush();
        assert_eq!(harness.input("输入文件颜色"), input, "更新换掉了筛选栏的颜色输入框");
        assert_eq!(harness.preedit(input).as_deref(), Some("hong"), "更新打断了预编辑");
        assert_eq!(harness.focused(), Some(input), "更新让颜色输入框失焦");
    }
    assert_eq!(harness.keyed("workspace-filter-bar"), Some(bar), "筛选栏不该重建");
    assert_eq!(harness.view_stats().remounts, remounts, "常驻筛选栏不该让主区重挂");
    harness.commit("红");
    for message in harness.take_messages() {
        harness.apply(message);
    }
    harness.flush();
    assert_eq!(crate::shell::view_part_primary::RouteKey::of(&harness.model), crate::shell::view_part_primary::RouteKey::Files);
    assert_eq!(harness.input("输入文件颜色"), input);
    assert_eq!(harness.value(input), "红");
    harness.assert_same_as_fresh_mount();
}

/// 一步操作：归约一条消息，或者在归约之外改 ViewModel 再同步（拖动中的放置态、加载更多、窄窗口、圆角）。
enum Step {
    Message(ShellMessage),
    Edit(fn(&mut ShellViewModel)),
}

/// 一串文件页上的操作：换展示方式、选中、评分、换目录、进回收站、打开预览再返回、编辑元数据、开关筛选栏和
/// 导入菜单，以及拖动中的放置态、加载更多、窄窗口和圆角这些绑在样式、图标和禁用上的状态。每一步之后
/// 增量同步的文档都和同一 ViewModel 新挂的一样。
#[test]
fn incremental_files_route_matches_a_fresh_mount_step_by_step() {
    let mut harness = ShellHarness::mount(scene("live-files-plain"));
    settle(&mut harness);
    let steps = vec![
        Step::Message(ShellMessage::Files(FilesMessage::SetDisplayMode(DisplayMode::List))),
        Step::Message(ShellMessage::Files(FilesMessage::ActivateRow("cover.png".into()))),
        Step::Message(ShellMessage::Inspect(InspectMessage::SetComment("草稿".into()))),
        Step::Message(ShellMessage::Inspect(InspectMessage::SetRating(3))),
        Step::Edit(|model| {
            model.input.internal_active = true;
            model.input.hover_folder = Some("assets".into());
        }),
        Step::Edit(|model| {
            model.input.internal_active = false;
            model.input.hover_folder = None;
            model.input.dragging_files = true;
        }),
        Step::Edit(|model| model.input.dragging_files = false),
        Step::Edit(|model| model.files.has_more = true),
        Step::Edit(|model| model.files.loading_more = true),
        Step::Edit(|model| {
            model.files.loading_more = false;
            model.files.has_more = false;
        }),
        Step::Message(ShellMessage::Files(FilesMessage::SetDisplayMode(DisplayMode::Masonry))),
        Step::Message(ShellMessage::Files(FilesMessage::ToggleImportMenu)),
        Step::Message(ShellMessage::Files(FilesMessage::ToggleImportMenu)),
        Step::Message(ShellMessage::Inspect(InspectMessage::ToggleFilterBar)),
        Step::Message(ShellMessage::Inspect(InspectMessage::ToggleFilterBar)),
        Step::Edit(|model| model.admin.corner_radius = 4.0),
        Step::Edit(|model| {
            model.set_viewport_width(880.0);
        }),
        Step::Message(ShellMessage::SelectFile { path: "notes/page.pdf".into(), asset_id: None }),
        Step::Edit(|model| {
            model.set_viewport_width(1200.0);
        }),
        Step::Message(ShellMessage::OpenDirectory(String::new())),
        Step::Message(ShellMessage::Files(FilesMessage::SetDisplayMode(DisplayMode::Adaptive))),
        Step::Message(ShellMessage::SetWorkspacePanel(WorkspacePanel::Trash)),
        Step::Message(ShellMessage::SetWorkspacePanel(WorkspacePanel::Files)),
    ];
    for (index, step) in steps.into_iter().enumerate() {
        match step {
            Step::Message(message) => harness.apply(message),
            Step::Edit(edit) => {
                edit(&mut harness.model);
                harness.model.mark_surface_dirty();
                harness.sync();
            }
        }
        settle(&mut harness);
        eprintln!("Nana 逐步对照第 {index} 步");
        harness.assert_same_as_fresh_mount();
        assert_same_structure_as_fresh_mount(&mut harness);
    }
}

/// 外部文件拖到文件列上：悬停时露出拖放提示，放下时导入。能不能放按归约时的面板算：
/// 进回收站（文件列常驻、不重建）以后悬停不进拖入状态，放下也不导入。
#[test]
fn external_file_drops_follow_the_current_panel() {
    let mut harness = ShellHarness::mount(scene("live-files-plain"));
    settle(&mut harness);
    let column = harness.keyed("file-column").expect("文件列");
    let bounds = harness.nodes().into_iter().find(|node| node.id == column).expect("文件列的布局盒").bounds;
    let point = (bounds.x + bounds.width / 2.0, bounds.y + bounds.height / 2.0);
    let document = harness.window.document.document();
    // 另起一路输入源送拖放事件，支撑自带的输入源不受影响。
    let mut input = HeadlessInput::bind_source(
        harness.window.document.context_mut(),
        nana_ui_platform::InputSourceId(7),
        nana_ui_platform::EndpointGeneration(1),
        document,
    )
    .expect("拖放输入源");
    let mut drag = |harness: &mut ShellHarness, kind: FileDragKind| {
        let payload = InputPayload::FileDrag(FileDragInput {
            kind,
            paths: vec![PathBuf::from("D:\\other\\c.png")],
            position: Some(point),
            modifiers: InputModifiers::default(),
        });
        input.route(harness.window.document.context_mut(), payload).expect("拖放事件");
        // 上一步刷新时的布局回报（拖入时卡片多一圈描边，列表宽度跟着变）也在这里一起归约。
        let messages = harness.take_messages();
        assert!(
            messages.iter().any(|message| matches!(message, ShellMessage::Files(FilesMessage::HostDrop(_)))),
            "文件列没有收到拖放事件"
        );
        for message in messages {
            harness.apply(message);
        }
        harness.flush();
    };
    harness.model.files.take_effects();
    drag(&mut harness, FileDragKind::Hover);
    assert!(harness.model.input.dragging_files, "悬停时进入拖入状态");
    assert!(harness.find("拖放到此处添加").is_some(), "悬停时露出拖放提示");
    drag(&mut harness, FileDragKind::Drop);
    assert!(!harness.model.input.dragging_files, "放下后退出拖入状态");
    assert!(harness.find("拖放到此处添加").is_none(), "放下后藏起拖放提示");
    let imported = harness
        .model
        .files
        .take_effects()
        .into_iter()
        .any(|effect| matches!(effect, FilesEffect::Import { sources, .. } if sources == ["D:\\other\\c.png"]));
    assert!(imported, "放下时导入拖进来的文件");

    harness.apply(ShellMessage::SetWorkspacePanel(WorkspacePanel::Trash));
    settle(&mut harness);
    assert_eq!(harness.keyed("file-column"), Some(column), "进回收站不重建文件列");
    harness.model.files.take_effects();
    drag(&mut harness, FileDragKind::Hover);
    assert!(!harness.model.input.dragging_files, "回收站里悬停不进拖入状态");
    drag(&mut harness, FileDragKind::Drop);
    assert!(harness.model.files.take_effects().is_empty(), "回收站里放下不导入");
}

/// 打开预览页：工作台藏起来、预览页露出来，主区分支和文件列不重建；返回后回到工作台。
#[test]
fn opening_a_preview_keeps_the_resident_column() {
    let mut harness = ShellHarness::mount(scene("live-files-plain"));
    settle(&mut harness);
    let column = harness.keyed("file-column").expect("文件列");
    let branch = harness.route_branch();
    harness.apply(ShellMessage::SelectFile { path: "notes/page.pdf".into(), asset_id: None });
    settle(&mut harness);
    assert_eq!(harness.keyed("file-column"), Some(column), "打开预览换掉了文件列");
    assert_eq!(harness.route_branch(), branch, "打开预览重挂了主区分支");
    assert!(harness.find("文件预览").is_some(), "预览页没有露出来");
    harness.assert_same_as_fresh_mount();
    harness.apply(ShellMessage::OpenDirectory(String::new()));
    settle(&mut harness);
    assert_eq!(harness.keyed("file-column"), Some(column));
    assert!(harness.find("文件预览").is_none(), "返回后预览页应藏起来");
    harness.assert_same_as_fresh_mount();
}

/// 播放条量到的宽度和原来一样时归约不改状态，播放条岛不重建；列表宽度的回报也一样。
/// 否则新的播放条又量一次、又发一次，变成每帧重建。
#[test]
fn resize_reports_do_not_rebuild_the_player_island() {
    let mut harness = ShellHarness::mount(scene("live-files-plain"));
    settle(&mut harness);
    let remounts = harness.view_stats().remounts;
    let width = harness.model.player.bar_width;
    let list = harness.model.files.list_width.expect("列表宽度已经回报");
    for _ in 0..3 {
        harness.apply(ShellMessage::Player(PlayerMessage::BarResized(width)));
        harness.apply(ShellMessage::Files(FilesMessage::ListResized(list)));
        harness.flush();
        assert!(harness.take_messages().is_empty(), "同样的宽度又引出了新的回报");
    }
    assert_eq!(harness.view_stats().remounts, remounts, "同样的宽度不该重建任何内容");
}
