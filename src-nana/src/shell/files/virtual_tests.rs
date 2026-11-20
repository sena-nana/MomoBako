//! 大目录下文件列表的虚拟化测试：挂上完整壳层，数实际建出来的卡片。
//!
//! 五千条的目录里，列表和网格模式挂载后只建视口附近的行；滚到中间后，视口里正好是中间那几条；
//! 再来一次无关的更新（选中一张卡片）整棵重挂，滚动位置和可见行都不变。
//! 瀑布流每列各自虚拟化，同样有上限。另有一个默认跳过的计时测试，量两千条时整棵重挂的耗时。

use std::collections::BTreeMap;
use std::time::Instant;

use nana_ui::runtime::{
    Entity, LayoutBox, LayoutViewport, ScrollOffset, ScrollView, component_descriptors,
};
use nana_ui::{ApplicationWindow, HeadlessInput, NanaTextShaper, PointerPhase};

use crate::backend::services::repository::FileBrowserEntry;
use crate::shell::{GapMessage, ShellMessage, ShellViewModel, SidebarMessage};

use super::{DisplayMode, FileRow, FilesMessage};

const MARK: &str = "momobako-entry:";

/// `index` 号文件。名字带五位序号，从卡片上就能读出它是第几条。
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
        asset_id: Some(name),
        status: None,
        thumbnail_path: None,
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
    let mut model = crate::shell::acceptance_gap_models()
        .into_iter()
        .find(|(name, _)| *name == "live-files")
        .expect("文件页场景")
        .1;
    model.files.import_open = false;
    model.files.eagle_open = false;
    model.files.display_mode = mode;
    model.files.rows = (0..count).map(numbered_file).collect();
    model
}

/// 按 1200×800 挂载，多排几遍：量过行高的虚拟行要在下一遍放到量出的位置。
fn settled(model: &ShellViewModel) -> ApplicationWindow {
    let mut window = ApplicationWindow::new();
    window.document = crate::acceptance_document_for_model(model.clone()).expect("挂载壳层");
    settle(&mut window);
    window
}

fn settle(window: &mut ApplicationWindow) {
    for _ in 0..6 {
        window
            .document
            .flush(
                LayoutViewport::new(1200.0, 800.0),
                &mut NanaTextShaper::default(),
            )
            .expect("排版");
        let _ = window.document.context_mut().take_program_messages();
    }
}

/// 建出来的卡片：序号和卡片框。
fn built_cards(window: &ApplicationWindow) -> Vec<(usize, LayoutBox)> {
    let document = window.document.document();
    let world = window.document.context().world();
    world
        .document_order(document)
        .into_iter()
        .filter_map(|id| {
            let path = world.text(id)?.strip_prefix(MARK)?.split_once(':')?.1;
            let index = path
                .strip_prefix("file-")?
                .strip_suffix(".png")?
                .parse::<usize>()
                .ok()?;
            let bounds = world.layout_box(world.parent_id(id)?)?;
            Some((index, bounds))
        })
        .collect()
}

/// 列表滚动区当前的纵向滚动量。
fn scrolled(window: &ApplicationWindow, scroll: Entity<ScrollView>) -> f32 {
    window
        .document
        .context()
        .world()
        .scroll_offset(scroll.stable_id())
        .map_or(0.0, |offset| offset.y)
}

/// 文件列表的滚动区和它的框。
fn list_scroll(window: &ApplicationWindow) -> (Entity<ScrollView>, LayoutBox) {
    let document = window.document.document();
    let context = window.document.context();
    let id = context
        .world()
        .nodes_of_component(document, component_descriptors::SCROLL_VIEW.type_id)
        .find(|node| {
            context.assembly_path(*node).is_some_and(|path| {
                path.rsplit('/')
                    .next()
                    .is_some_and(|key| key.starts_with("files-scroll-"))
            })
        })
        .expect("文件列表滚动区");
    (
        Entity::from_stable_id(id),
        context.world().layout_box(id).expect("滚动区有框"),
    )
}

/// 落在视口里的卡片序号，从上到下、从左到右。卡片框是没滚动时的位置，减去滚动量再比。
fn visible(cards: &[(usize, LayoutBox)], viewport: LayoutBox, scrolled: f32) -> Vec<usize> {
    let mut shown = cards
        .iter()
        .filter(|(_, bounds)| {
            let top = bounds.y - scrolled;
            top + bounds.height > viewport.y && top < viewport.y + viewport.height
        })
        .map(|(index, bounds)| (bounds.y.round() as i64, bounds.x.round() as i64, *index))
        .collect::<Vec<_>>();
    shown.sort();
    shown.into_iter().map(|(_, _, index)| index).collect()
}

/// 视口能完整放下多少张：按建出来的卡片量出行距和每行张数。
fn viewport_capacity(cards: &[(usize, LayoutBox)], viewport: LayoutBox) -> usize {
    let mut tops = cards
        .iter()
        .map(|(_, bounds)| bounds.y.round() as i64)
        .collect::<Vec<_>>();
    tops.sort();
    tops.dedup();
    let pitch = tops
        .windows(2)
        .map(|pair| (pair[1] - pair[0]) as f32)
        .fold(f32::MAX, f32::min);
    let per_row = cards.len() / tops.len().max(1);
    let rows = (viewport.height / pitch.max(1.0)).ceil() as usize + 1;
    rows * per_row.max(1)
}

/// 五千条里只建视口附近的卡片；滚到中间后，视口里是中间那几条，建出来的仍然有限。
fn check_mode(mode: DisplayMode) {
    let total = 5000;
    let model = bulk_model(mode, total);
    let mut window = settled(&model);
    let (scroll, viewport) = list_scroll(&window);
    let cards = built_cards(&window);
    assert!(!cards.is_empty(), "{mode:?}：首屏要有卡片");
    let capacity = viewport_capacity(&cards, viewport);
    assert!(
        cards.len() <= capacity * 3,
        "{mode:?}：首屏只能建视口附近的卡片，建了 {} 张，视口放得下约 {capacity} 张",
        cards.len()
    );
    assert_eq!(
        visible(&cards, viewport, 0.0).first(),
        Some(&0),
        "{mode:?}：首屏从第一条开始"
    );

    let pitch = {
        let shown = visible(&cards, viewport, 0.0);
        let first = cards
            .iter()
            .find(|(index, _)| *index == shown[0])
            .expect("首张")
            .1;
        let next_row = cards
            .iter()
            .map(|(_, bounds)| bounds.y)
            .filter(|y| *y > first.y + 1.0)
            .fold(f32::MAX, f32::min);
        let per_row = cards
            .iter()
            .filter(|(_, bounds)| (bounds.y - first.y).abs() < 1.0)
            .count();
        (next_row - first.y, per_row)
    };
    let middle_row = (total / 2) / pitch.1;
    let offset = middle_row as f32 * pitch.0;
    window
        .document
        .context_mut()
        .scroll_to(scroll, ScrollOffset { x: 0.0, y: offset })
        .expect("滚动");
    settle(&mut window);
    let cards = built_cards(&window);
    let shown = visible(&cards, viewport, scrolled(&window, scroll));
    let built = cards.iter().map(|(index, _)| *index).collect::<Vec<_>>();
    assert!(
        !shown.is_empty(),
        "{mode:?}：滚到中间后视口里要有卡片，滚动量 {}，建了 {built:?}",
        scrolled(&window, scroll)
    );
    assert!(
        cards.len() <= capacity * 3,
        "{mode:?}：滚动后仍只建视口附近的卡片，建了 {} 张",
        cards.len()
    );
    let expected = middle_row * pitch.1;
    let slack = pitch.1 * 2;
    assert!(
        shown[0] + slack >= expected && shown[0] <= expected + slack,
        "{mode:?}：滚到第 {middle_row} 行，视口第一张应在 {expected} 附近，实际 {shown:?}"
    );
    assert!(
        shown.windows(2).all(|pair| pair[1] == pair[0] + 1),
        "{mode:?}：视口里的卡片连续：{shown:?}"
    );
}

#[test]
fn list_mode_builds_only_the_rows_near_the_viewport() {
    check_mode(DisplayMode::List);
}

#[test]
fn grid_mode_builds_only_the_rows_near_the_viewport() {
    check_mode(DisplayMode::Grid);
}

/// 壳层每次更新都整棵重挂：卸载前记下滚动位置，挂好后用 `scroll_to` 写回，虚拟列表按写回的视口挂行。
#[test]
fn an_unrelated_update_keeps_the_scroll_position_and_the_visible_rows() {
    let mut model = bulk_model(DisplayMode::List, 5000);
    let mut window = settled(&model);
    let (scroll, viewport) = list_scroll(&window);
    window
        .document
        .context_mut()
        .scroll_to(
            scroll,
            ScrollOffset {
                x: 0.0,
                y: 2500.0 * 82.0,
            },
        )
        .expect("滚动");
    settle(&mut window);
    let before_offset = scrolled(&window, scroll);
    let before = visible(&built_cards(&window), viewport, before_offset);
    assert!(
        before.first().is_some_and(|first| *first > 2000),
        "先滚到中间：{before:?}"
    );

    model.reduce(ShellMessage::Files(FilesMessage::ActivateRow(format!(
        "file-{:05}.png",
        before[1]
    ))));
    crate::shell::mount_shell(&mut window.document, &model).expect("重挂壳层");
    settle(&mut window);
    let (scroll, viewport) = list_scroll(&window);
    let after_offset = scrolled(&window, scroll);
    let after = visible(&built_cards(&window), viewport, after_offset);
    assert!(
        (after_offset - before_offset).abs() < 0.5,
        "重挂后滚动位置不变：{before_offset} → {after_offset}"
    );
    assert_eq!(after, before, "重挂后视口里还是同一批卡片");
}

#[test]
fn masonry_columns_are_windowed_too() {
    let model = bulk_model(DisplayMode::Masonry, 2000);
    let window = settled(&model);
    let (_, viewport) = list_scroll(&window);
    let cards = built_cards(&window);
    assert!(!cards.is_empty());
    assert!(
        cards.len() < 60,
        "瀑布流每列只建视口附近的卡片，建了 {} 张",
        cards.len()
    );
    assert!(
        visible(&cards, viewport, 0.0).contains(&0),
        "第一列从第一条开始"
    );
}

/// 框选覆盖滚动范围里还没建出来的行。不把整表卡片建出来。
#[test]
fn box_select_includes_unmounted_rows_in_the_scroll_range() {
    let mut model = bulk_model(DisplayMode::List, 80);
    let mut window = settled(&model);
    let (scroll, viewport) = list_scroll(&window);
    window
        .document
        .context_mut()
        .scroll_to(
            scroll,
            ScrollOffset {
                x: 0.0,
                y: 20.0 * 82.0,
            },
        )
        .expect("滚动");
    settle(&mut window);
    let built = built_cards(&window);
    assert!(
        built.len() < 40,
        "框选前不能把整表建出来，实际 {}",
        built.len()
    );
    let built_ids = built
        .iter()
        .map(|(index, _)| *index)
        .collect::<std::collections::HashSet<_>>();
    let document_id = window.document.document();
    let mut input = HeadlessInput::bind(window.document.context_mut(), document_id);
    let start = (viewport.x + 2.0, viewport.y + 8.0);
    let end = (viewport.x + viewport.width * 0.5, viewport.y - 500.0);
    input
        .pointer(
            window.document.context_mut(),
            PointerPhase::Down,
            start.0,
            start.1,
        )
        .expect("按下");
    crate::shell::observe_live_pointer(&mut model, &window.document);
    input
        .pointer(
            window.document.context_mut(),
            PointerPhase::Move,
            end.0,
            end.1,
        )
        .expect("拖动");
    crate::shell::observe_live_pointer(&mut model, &window.document);
    let selected = model.files.selected_paths();
    let extra = selected.iter().any(|path| {
        path.strip_prefix("file-")
            .and_then(|rest| rest.strip_suffix(".png"))
            .and_then(|index| index.parse::<usize>().ok())
            .is_some_and(|index| !built_ids.contains(&index))
    });
    assert!(extra, "框选应包含滚动范围里还没建出的行：选中 {selected:?}");
    assert!(
        built_cards(&window).len() < 40,
        "框选之后仍然不能把整表建出来"
    );
}

/// 文件右键菜单或文件夹右键菜单开着时，列表一滚就收起，和 Vue 捕获阶段的 `scroll` 一样。
/// 刚打开只记下偏移；菜单关着时滚过的距离，重新打开后不拿来关。不开窗口。
#[test]
fn scrolling_closes_open_context_menus() {
    let mut model = bulk_model(DisplayMode::List, 80);
    model.reduce(ShellMessage::Files(FilesMessage::OpenEntryMenu {
        path: "file-00000.png".into(),
        x: 48.0,
        y: 64.0,
    }));
    assert!(model.files.entry_menu.is_some(), "文件右键菜单应打开");
    model.sidebar.folder_menu = Some(super::super::sidebar::FolderMenu {
        path: "assets".into(),
        label: "assets".into(),
        x: 24.0,
        y: 36.0,
    });
    let mut window = settled(&model);

    assert!(
        super::super::dismiss_context_menus_on_scroll(&mut model, &window.document).is_empty(),
        "刚打开只记下偏移"
    );
    assert!(
        super::super::dismiss_context_menus_on_scroll(&mut model, &window.document).is_empty(),
        "偏移没变不关"
    );
    assert!(model.files.entry_menu.is_some() && model.sidebar.folder_menu.is_some());

    let (scroll, _) = list_scroll(&window);
    window
        .document
        .context_mut()
        .scroll_to(scroll, ScrollOffset { x: 0.0, y: 180.0 })
        .expect("滚动");
    settle(&mut window);
    let (scroll, _) = list_scroll(&window);
    let y = window
        .document
        .context()
        .world()
        .scroll_offset(scroll.stable_id())
        .map(|offset| offset.y)
        .unwrap_or(0.0);
    assert!(y >= 100.0, "滚动应留下偏移，实际 {y}");

    let messages = super::super::dismiss_context_menus_on_scroll(&mut model, &window.document);
    assert!(
        messages
            .iter()
            .any(|message| matches!(message, ShellMessage::Files(FilesMessage::CloseEntryMenu))),
        "滚了文件列表应收起文件菜单"
    );
    assert!(
        messages.iter().any(|message| matches!(
            message,
            ShellMessage::Sidebar(SidebarMessage::Gap(GapMessage::CloseFolderMenu))
        )),
        "同一下滚动也收起文件夹菜单"
    );
    assert_eq!(messages.len(), 2, "只关这两份右键菜单");
    assert!(
        super::super::dismiss_context_menus_on_scroll(&mut model, &window.document).is_empty(),
        "同一偏移不重复关"
    );
    for message in messages {
        model.reduce(message);
    }
    assert!(model.files.entry_menu.is_none() && model.sidebar.folder_menu.is_none());

    model.reduce(ShellMessage::Files(FilesMessage::OpenEntryMenu {
        path: "file-00010.png".into(),
        x: 48.0,
        y: 90.0,
    }));
    assert!(
        super::super::dismiss_context_menus_on_scroll(&mut model, &window.document).is_empty(),
        "重新打开先记下当前偏移"
    );
    let (scroll, _) = list_scroll(&window);
    window
        .document
        .context_mut()
        .scroll_to(scroll, ScrollOffset { x: 0.0, y: 40.0 })
        .expect("再滚");
    settle(&mut window);
    let messages = super::super::dismiss_context_menus_on_scroll(&mut model, &window.document);
    assert!(
        messages
            .iter()
            .any(|message| matches!(message, ShellMessage::Files(FilesMessage::CloseEntryMenu))),
        "重新打开后再滚才关"
    );
}

/// 滚到「滚动继续加载」上方 320px 以内就续读下一页，和 Vue 的 IntersectionObserver 一样。
/// 停在列表前半段不续读。贴到底时滚轮自己也会发出续读。不开窗口。
#[test]
fn scrolling_near_the_end_continues_the_directory() {
    let mut model = bulk_model(DisplayMode::List, 80);
    model.files.has_more = true;
    model.files.total_entries = 240;
    let mut window = settled(&model);
    let (scroll, _viewport) = list_scroll(&window);
    let max = window
        .document
        .context()
        .world()
        .scroll_metrics(scroll.stable_id())
        .expect("度量")
        .max_offset()
        .y;
    assert!(max > 400.0, "列表要比视口高出一截，实际最大滚动量 {max}");

    assert!(
        super::super::load_more_if_sentinel_near(&mut model, &window.document).is_none(),
        "停在顶部不续读"
    );

    window
        .document
        .context_mut()
        .scroll_to(scroll, ScrollOffset { x: 0.0, y: 80.0 })
        .expect("前半段");
    settle(&mut window);
    let (scroll, _viewport) = list_scroll(&window);
    assert!(
        super::super::load_more_if_sentinel_near(&mut model, &window.document).is_none(),
        "前半段不续读"
    );

    let max = window
        .document
        .context()
        .world()
        .scroll_metrics(scroll.stable_id())
        .expect("度量")
        .max_offset()
        .y;
    window
        .document
        .context_mut()
        .scroll_to(
            scroll,
            ScrollOffset {
                x: 0.0,
                y: (max - 200.0).max(0.0),
            },
        )
        .expect("靠近底部");
    settle(&mut window);
    let message = super::super::load_more_if_sentinel_near(&mut model, &window.document);
    let Some(message) = message else {
        panic!("距底部 200px，应在 320px 哨兵内续读")
    };
    model.reduce(message);
    assert!(model.files.loading_more, "续读已经排上");
    assert!(
        super::super::load_more_if_sentinel_near(&mut model, &window.document).is_none(),
        "同一段靠近不重复续读"
    );

    let mut again = bulk_model(DisplayMode::List, 80);
    again.files.has_more = true;
    again.files.total_entries = 240;
    let mut window = settled(&again);
    let (scroll, _viewport) = list_scroll(&window);
    let max = window
        .document
        .context()
        .world()
        .scroll_metrics(scroll.stable_id())
        .expect("度量")
        .max_offset()
        .y;
    window
        .document
        .context_mut()
        .scroll_to(
            scroll,
            ScrollOffset {
                x: 0.0,
                y: (max - 20.0).max(0.0),
            },
        )
        .expect("贴底");
    settle(&mut window);
    let _ = window.document.context_mut().take_program_messages();
    let (scroll, viewport) = list_scroll(&window);
    let _ = scroll;
    let document_id = window.document.document();
    let mut input = HeadlessInput::bind(window.document.context_mut(), document_id);
    let pointer_id =
        nana_ui::PointerInput::mouse(PointerPhase::Move, viewport.x + 8.0, viewport.y + 8.0)
            .pointer_id;
    let wheel = nana_ui::WheelInput {
        pointer_id,
        x: viewport.x + 8.0,
        y: viewport.y + 8.0,
        delta_x: 0.0,
        delta_y: -80.0,
        unit: nana_ui::WheelUnit::Pixels,
        modifiers: nana_ui::InputModifiers::default(),
    };
    input
        .route(
            window.document.context_mut(),
            nana_ui::InputPayload::Wheel(wheel),
        )
        .expect("滚轮");
    let messages: Vec<ShellMessage> = window
        .document
        .context_mut()
        .take_program_messages()
        .into_iter()
        .map(|message| *message.downcast::<ShellMessage>().expect("壳层消息"))
        .collect();
    let loads = messages
        .iter()
        .filter(|message| matches!(message, ShellMessage::Files(FilesMessage::LoadMore)))
        .count();
    assert!(
        loads > 0,
        "贴到底的滚轮应续读，这次没有续读消息，共 {} 条",
        messages.len()
    );
}

/// 窗口失焦时收起文件和文件夹右键菜单，和 Vue `installContextMenu` 的 `blur` 一样。
/// 窗口还聚焦时菜单留着，菜单项仍能点。失焦发出的只有关闭，不代替点菜单项。不开窗口。
#[test]
fn focus_loss_closes_open_context_menus() {
    let mut model = bulk_model(DisplayMode::List, 8);
    model.reduce(ShellMessage::Files(FilesMessage::OpenEntryMenu {
        path: "file-00000.png".into(),
        x: 48.0,
        y: 64.0,
    }));
    assert!(model.files.entry_menu.is_some(), "文件右键菜单应打开");
    model.sidebar.folder_menu = Some(super::super::sidebar::FolderMenu {
        path: "assets".into(),
        label: "assets".into(),
        x: 24.0,
        y: 36.0,
    });

    let staying =
        crate::shell::focus_loss_messages(&model, &nana_ui::InputPayload::Focus { focused: true });
    assert!(staying.is_empty(), "窗口还聚焦时不关菜单");
    assert!(
        model.files.entry_menu.is_some() && model.sidebar.folder_menu.is_some(),
        "菜单还在，项可以点"
    );

    let messages =
        crate::shell::focus_loss_messages(&model, &nana_ui::InputPayload::Focus { focused: false });
    assert!(
        matches!(
            messages.as_slice(),
            [
                ShellMessage::Files(FilesMessage::CloseEntryMenu),
                ShellMessage::Sidebar(SidebarMessage::Gap(GapMessage::CloseFolderMenu)),
            ]
        ),
        "失焦只关两份右键菜单，不点菜单项：{} 条",
        messages.len()
    );
    for message in messages {
        model.reduce(message);
    }
    assert!(
        model.files.entry_menu.is_none() && model.sidebar.folder_menu.is_none(),
        "失焦后两份菜单都收起"
    );
}

/// 窗口尺寸变了收起文件和文件夹右键菜单，和 Vue `installContextMenu` 的 `resize` 一样。
/// 第一次只记下尺寸；同一像素（含不到 1px 的抖动）和无效尺寸不关，菜单留着，项仍能点。
/// 只发关闭，不把内部拖动交给系统。不开窗口。
#[test]
fn window_resize_closes_open_context_menus() {
    let mut model = bulk_model(DisplayMode::List, 8);
    open_both_menus(&mut model);
    model.input.internal_active = true;

    let first = crate::shell::window_resize_messages(&mut model, 1200.0, 800.0);
    assert!(first.is_empty(), "第一次只记尺寸，不关菜单");
    assert!(
        model.files.entry_menu.is_some() && model.sidebar.folder_menu.is_some(),
        "菜单还在，项可以点"
    );

    let same = crate::shell::window_resize_messages(&mut model, 1200.4, 800.2);
    assert!(same.is_empty(), "不到 1px 不算尺寸变了");
    assert!(
        model.files.entry_menu.is_some() && model.sidebar.folder_menu.is_some(),
        "尺寸没变，菜单项仍能点"
    );

    let invalid = crate::shell::window_resize_messages(&mut model, f32::NAN, 900.0);
    assert!(invalid.is_empty(), "无效尺寸不关菜单");
    assert!(model.files.entry_menu.is_some() && model.sidebar.folder_menu.is_some());

    let width = crate::shell::window_resize_messages(&mut model, 1280.0, 800.0);
    assert_menu_closes(&width);
    for message in width {
        model.reduce(message);
    }
    assert!(
        model.files.entry_menu.is_none() && model.sidebar.folder_menu.is_none(),
        "加宽后两份菜单都收起"
    );
    assert!(model.input.internal_active, "缩放不把拖动交给系统");

    open_both_menus(&mut model);
    let height = crate::shell::window_resize_messages(&mut model, 1280.0, 900.0);
    assert_menu_closes(&height);
    for message in height {
        model.reduce(message);
    }
    assert!(
        model.files.entry_menu.is_none() && model.sidebar.folder_menu.is_none(),
        "加高后两份菜单都收起"
    );
}

fn open_both_menus(model: &mut ShellViewModel) {
    model.reduce(ShellMessage::Files(FilesMessage::OpenEntryMenu {
        path: "file-00000.png".into(),
        x: 48.0,
        y: 64.0,
    }));
    model.sidebar.folder_menu = Some(super::super::sidebar::FolderMenu {
        path: "assets".into(),
        label: "assets".into(),
        x: 24.0,
        y: 36.0,
    });
    assert!(
        model.files.entry_menu.is_some() && model.sidebar.folder_menu.is_some(),
        "两份右键菜单应打开"
    );
}

fn assert_menu_closes(messages: &[ShellMessage]) {
    assert!(
        matches!(
            messages,
            [
                ShellMessage::Files(FilesMessage::CloseEntryMenu),
                ShellMessage::Sidebar(SidebarMessage::Gap(GapMessage::CloseFolderMenu)),
            ]
        ),
        "缩放只关两份右键菜单，不点菜单项：{} 条",
        messages.len()
    );
}

/// 整棵重挂一次（卸载、挂载、排两遍版）的耗时，八十条和两千条对比。默认跳过：
/// `cargo test -p momobako-nana --lib remount_timing -- --ignored --nocapture`。
#[test]
#[ignore]
fn remount_timing_with_two_thousand_entries() {
    for count in [80, 2000] {
        for mode in [
            DisplayMode::List,
            DisplayMode::Grid,
            DisplayMode::Adaptive,
            DisplayMode::Masonry,
        ] {
            let model = bulk_model(mode, count);
            let mut window = settled(&model);
            let rounds = 10;
            let started = Instant::now();
            for _ in 0..rounds {
                crate::shell::mount_shell(&mut window.document, &model).expect("重挂壳层");
                for _ in 0..2 {
                    window
                        .document
                        .flush(
                            LayoutViewport::new(1200.0, 800.0),
                            &mut NanaTextShaper::default(),
                        )
                        .expect("排版");
                }
            }
            let cards = built_cards(&window).len();
            eprintln!(
                "Nana 文件页 {count} 条 {mode:?}：重挂一次平均 {:?}，建了 {cards} 张卡片",
                started.elapsed() / rounds
            );
        }
    }
}
