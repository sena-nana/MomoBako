//! 大目录下文件列表的虚拟化测试：挂上完整壳层，数实际建出来的卡片。
//!
//! 五千条的目录里，列表和网格模式挂载后只建视口附近的行；滚到中间后，视口里正好是中间那几条；
//! 再来一次无关的更新（选中一张卡片）整棵重挂，滚动位置和可见行都不变。
//! 瀑布流每列各自虚拟化，同样有上限。另有一个默认跳过的计时测试，量两千条时整棵重挂的耗时。

use std::collections::BTreeMap;
use std::time::Instant;

use nana_ui::runtime::{component_descriptors, Entity, LayoutBox, LayoutViewport, ScrollOffset, ScrollView};
use nana_ui::{ApplicationWindow, NanaTextShaper};

use crate::backend::services::repository::FileBrowserEntry;
use crate::shell::{ShellMessage, ShellViewModel};

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
        window.document.flush(LayoutViewport::new(1200.0, 800.0), &mut NanaTextShaper::default()).expect("排版");
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
            let index = path.strip_prefix("file-")?.strip_suffix(".png")?.parse::<usize>().ok()?;
            let bounds = world.layout_box(world.parent_id(id)?)?;
            Some((index, bounds))
        })
        .collect()
}

/// 列表滚动区当前的纵向滚动量。
fn scrolled(window: &ApplicationWindow, scroll: Entity<ScrollView>) -> f32 {
    window.document.context().world().scroll_offset(scroll.stable_id()).map_or(0.0, |offset| offset.y)
}

/// 文件列表的滚动区和它的框。
fn list_scroll(window: &ApplicationWindow) -> (Entity<ScrollView>, LayoutBox) {
    let document = window.document.document();
    let context = window.document.context();
    let id = context
        .world()
        .nodes_of_component(document, component_descriptors::SCROLL_VIEW.type_id)
        .find(|node| {
            context
                .assembly_path(*node)
                .is_some_and(|path| path.rsplit('/').next().is_some_and(|key| key.starts_with("files-scroll-")))
        })
        .expect("文件列表滚动区");
    (Entity::from_stable_id(id), context.world().layout_box(id).expect("滚动区有框"))
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
    let mut tops = cards.iter().map(|(_, bounds)| bounds.y.round() as i64).collect::<Vec<_>>();
    tops.sort();
    tops.dedup();
    let pitch = tops.windows(2).map(|pair| (pair[1] - pair[0]) as f32).fold(f32::MAX, f32::min);
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
    assert_eq!(visible(&cards, viewport, 0.0).first(), Some(&0), "{mode:?}：首屏从第一条开始");

    let pitch = {
        let shown = visible(&cards, viewport, 0.0);
        let first = cards.iter().find(|(index, _)| *index == shown[0]).expect("首张").1;
        let next_row = cards.iter().map(|(_, bounds)| bounds.y).filter(|y| *y > first.y + 1.0).fold(f32::MAX, f32::min);
        let per_row = cards.iter().filter(|(_, bounds)| (bounds.y - first.y).abs() < 1.0).count();
        (next_row - first.y, per_row)
    };
    let middle_row = (total / 2) / pitch.1;
    let offset = middle_row as f32 * pitch.0;
    window.document.context_mut().scroll_to(scroll, ScrollOffset { x: 0.0, y: offset }).expect("滚动");
    settle(&mut window);
    let cards = built_cards(&window);
    let shown = visible(&cards, viewport, scrolled(&window, scroll));
    let built = cards.iter().map(|(index, _)| *index).collect::<Vec<_>>();
    assert!(!shown.is_empty(), "{mode:?}：滚到中间后视口里要有卡片，滚动量 {}，建了 {built:?}", scrolled(&window, scroll));
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
    assert!(shown.windows(2).all(|pair| pair[1] == pair[0] + 1), "{mode:?}：视口里的卡片连续：{shown:?}");
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
    window.document.context_mut().scroll_to(scroll, ScrollOffset { x: 0.0, y: 2500.0 * 82.0 }).expect("滚动");
    settle(&mut window);
    let before_offset = scrolled(&window, scroll);
    let before = visible(&built_cards(&window), viewport, before_offset);
    assert!(before.first().is_some_and(|first| *first > 2000), "先滚到中间：{before:?}");

    model.reduce(ShellMessage::Files(FilesMessage::ActivateRow(format!("file-{:05}.png", before[1]))));
    crate::shell::mount_shell(&mut window.document, &model).expect("重挂壳层");
    settle(&mut window);
    let (scroll, viewport) = list_scroll(&window);
    let after_offset = scrolled(&window, scroll);
    let after = visible(&built_cards(&window), viewport, after_offset);
    assert!((after_offset - before_offset).abs() < 0.5, "重挂后滚动位置不变：{before_offset} → {after_offset}");
    assert_eq!(after, before, "重挂后视口里还是同一批卡片");
}

#[test]
fn masonry_columns_are_windowed_too() {
    let model = bulk_model(DisplayMode::Masonry, 2000);
    let window = settled(&model);
    let (_, viewport) = list_scroll(&window);
    let cards = built_cards(&window);
    assert!(!cards.is_empty());
    assert!(cards.len() < 60, "瀑布流每列只建视口附近的卡片，建了 {} 张", cards.len());
    assert!(visible(&cards, viewport, 0.0).contains(&0), "第一列从第一条开始");
}

/// 整棵重挂一次（卸载、挂载、排两遍版）的耗时，八十条和两千条对比。默认跳过：
/// `cargo test -p momobako-nana --lib remount_timing -- --ignored --nocapture`。
#[test]
#[ignore]
fn remount_timing_with_two_thousand_entries() {
    for count in [80, 2000] {
        for mode in [DisplayMode::List, DisplayMode::Grid, DisplayMode::Adaptive, DisplayMode::Masonry] {
            let model = bulk_model(mode, count);
            let mut window = settled(&model);
            let rounds = 10;
            let started = Instant::now();
            for _ in 0..rounds {
                crate::shell::mount_shell(&mut window.document, &model).expect("重挂壳层");
                for _ in 0..2 {
                    window.document.flush(LayoutViewport::new(1200.0, 800.0), &mut NanaTextShaper::default()).expect("排版");
                }
            }
            let cards = built_cards(&window).len();
            eprintln!("Nana 文件页 {count} 条 {mode:?}：重挂一次平均 {:?}，建了 {cards} 张卡片", started.elapsed() / rounds);
        }
    }
}
