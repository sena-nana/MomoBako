//! 文件列表的虚拟行：只构建视口和预取范围内的卡片。
//!
//! Nana 每次更新都整棵重挂，把上千张卡片全建出来会让选中、缩略图到达这类小更新都变慢。
//! 这里把一组条目先排成「行」再交给 `each_virtual`：列表模式一张卡片一行；网格和自适应按列表
//! 宽度把卡片装进一行（行内、行间都隔 14）；瀑布流先按 CSS 多列的规则分好列，每列各自虚拟化。
//! 所有虚拟列表都跟着同一个列表滚动区滚动（`within`），行高按内容量（`measured`）。
//! 每一行下面都带组内间距，行高就等于估算值（卡片设计尺寸 + 间距），量过以后位置也不跳；
//! 最后一行多出来的间距由外层的负下边距抵掉。

use std::sync::Arc;

use nana_ui::runtime::view::{each_virtual, signal, widget, AnyView, IntoView, NodeRef};
use nana_ui::runtime::{AlignSpec, LengthSpec, Stack};

use super::super::files::{DisplayMode, FileRow};
use super::cards::{self, CardBox, CardState};

/// 组内卡片的行列间距（`.files-list__files--grid` 等的 `gap: 14px`）。
pub(super) const GAP: f32 = 14.0;
/// 列表模式卡片之间的间距（`.files-list__files--list` 的 `gap: 8px`）。
const LIST_GAP: f32 = 8.0;
/// Vue 瀑布流的 `column-width`。
pub(super) const MASONRY_COLUMN: f32 = 164.0;
/// 视口上下各多建 240px 的行，滚动时不露白，也不把视口以外太多的卡片建出来。
const OVERSCAN: f32 = 240.0;
/// 列表卡片：1px 边、上下 8 内边距、56 缩略图，至少 72 高（`min-height: 72px`）。
const LIST_CARD_HEIGHT: f32 = 74.0;
/// 自适应卡片：1px 边、上下 8 内边距、120 缩略图、8 间距、两行标题区 38。
const ADAPTIVE_CARD_HEIGHT: f32 = 184.0;

/// 一张要画的卡片：条目和选中 / 放置状态。行在虚拟列表里构建时拿不到视图模型，状态先算好。
pub(super) struct CardSpec {
    pub row: FileRow,
    pub state: CardState,
}

/// 行里的一格：条目、外框尺寸和状态。
struct CardCell {
    row: FileRow,
    geometry: CardBox,
    state: CardState,
}

/// 虚拟列表的一行：键（第一张卡片的条目键）和卡片。
/// 虚拟列表每次同步都会克隆全部行，所以行放在 `Arc` 里，克隆只加引用计数。
struct CardLine {
    key: Arc<str>,
    cells: Vec<CardCell>,
}

impl CardLine {
    fn new(cells: Vec<CardCell>) -> Arc<Self> {
        let key = cells.first().map(|cell| cell.row.key()).unwrap_or_default();
        Arc::new(Self { key: key.into(), cells })
    }
}

/// 一组条目的虚拟列表。`key` 是这组列表在内容里的名字，`width` 是列表内容宽。
pub(super) fn group(specs: Vec<CardSpec>, mode: DisplayMode, width: f32, scroll: NodeRef, key: &'static str) -> AnyView {
    match mode {
        DisplayMode::List => virtual_lines(list_lines(specs), mode, LIST_CARD_HEIGHT, LIST_GAP, scroll, key),
        DisplayMode::Grid => virtual_lines(wrap_lines(specs, width, |_| cards::GRID_BOX), mode, cards::GRID_BOX.height, GAP, scroll, key),
        DisplayMode::Adaptive => virtual_lines(wrap_lines(specs, width, cards::adaptive_box), mode, ADAPTIVE_CARD_HEIGHT, GAP, scroll, key),
        DisplayMode::Masonry => masonry(specs, width, scroll, key),
    }
}

/// 列表模式：一张卡片一行。
fn list_lines(specs: Vec<CardSpec>) -> Vec<Arc<CardLine>> {
    specs
        .into_iter()
        .map(|spec| CardLine::new(vec![CardCell { row: spec.row, geometry: cards::LIST_BOX, state: spec.state }]))
        .collect()
}

/// 网格和自适应：按卡片自身宽度顺序装行，和 flex-wrap / `repeat(auto-fill, 148px)` 一样靠左排。
///
/// 算法：当前行已有卡片时，加上间距和下一张的宽度超过列表宽就换行；每行至少一张，
/// 窄到放不下一张时那一张单独成行。
fn wrap_lines(specs: Vec<CardSpec>, width: f32, geometry: impl Fn(&FileRow) -> CardBox) -> Vec<Arc<CardLine>> {
    let width = width.max(1.0);
    let mut rows: Vec<Vec<CardCell>> = Vec::new();
    let mut used = 0.0;
    for spec in specs {
        let card = geometry(&spec.row);
        let start_new = rows.last().is_none_or(|cells| !cells.is_empty() && used + GAP + card.width > width + 0.01);
        if start_new {
            rows.push(Vec::new());
            used = card.width;
        } else {
            used += GAP + card.width;
        }
        if let Some(cells) = rows.last_mut() {
            cells.push(CardCell { row: spec.row, geometry: card, state: spec.state });
        }
    }
    rows.into_iter().map(CardLine::new).collect()
}

/// 把行交给 `each_virtual`：跟外层列表滚动，按内容量行高。每行下面带 `gap`，估算行高是
/// 卡片高加间距；外层用 `-gap` 的下边距抵掉最后一行多出的间距，和不虚拟化时的排法一样。
fn virtual_lines(lines: Vec<Arc<CardLine>>, mode: DisplayMode, card_height: f32, gap: f32, scroll: NodeRef, key: &'static str) -> AnyView {
    let list = each_virtual(signal(lines), |line: &Arc<CardLine>| line.key.clone(), card_height + gap, move |line| line_view(&line, mode, gap))
        .measured()
        .overscan(OVERSCAN)
        .within(scroll)
        .key(key);
    widget(Stack::column(0.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0)).with_layout(move |layout| {
        layout.margin_bottom = Some(LengthSpec::Px(-gap));
    }))
    .children((list,))
    .key(format!("{key}-frame"))
    .into_any()
}

/// 一行卡片，下面留 `gap`。列表模式竖排（只有一张），其余横排靠左、顶端对齐。
fn line_view(line: &CardLine, mode: DisplayMode, gap: f32) -> AnyView {
    let frame = if mode.is_list() { Stack::column(0.0) } else { Stack::row(GAP).align(AlignSpec::Start) };
    let cards = line.cells.iter().map(|cell| cards::card(&cell.row, mode, cell.geometry, cell.state)).collect::<Vec<_>>();
    widget(frame.width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0)).with_layout(move |layout| {
        layout.padding_bottom = Some(LengthSpec::Px(gap));
    }))
    .children(cards)
    .into_any()
}

/// 瀑布流：和 CSS `column-width: 164px; column-gap: 14px` 一样定列数和列宽，条目按顺序装进各列，
/// 每列各自虚拟化。卡片下面留 14（Vue 的 `margin-bottom: 14px`），最后一张也留。
fn masonry(specs: Vec<CardSpec>, width: f32, scroll: NodeRef, key: &'static str) -> AnyView {
    let (count, column_width) = masonry_columns(width);
    let heights = specs.iter().map(|spec| cards::masonry_height(&spec.row, column_width) + GAP).collect::<Vec<_>>();
    let split = balanced_columns(&heights, count);
    let mut specs = specs.into_iter();
    let mut columns = Vec::with_capacity(split.len());
    let mut start = 0;
    for (index, end) in split.into_iter().enumerate() {
        let estimate = heights[start..end].iter().sum::<f32>() / (end - start).max(1) as f32;
        let lines = specs
            .by_ref()
            .take(end - start)
            .map(|spec| {
                let geometry = cards::masonry_box(&spec.row, column_width);
                CardLine::new(vec![CardCell { row: spec.row, geometry, state: spec.state }])
            })
            .collect::<Vec<_>>();
        let list = each_virtual(signal(lines), |line: &Arc<CardLine>| line.key.clone(), estimate, |line| masonry_cell(&line))
            .measured()
            .overscan(OVERSCAN)
            .within(scroll)
            .key(format!("{key}-col-{index}"));
        columns.push(
            widget(Stack::column(0.0).width(LengthSpec::Px(column_width)).grow(0.0).shrink(0.0))
                .children((list,))
                .key(format!("{key}-column-{index}"))
                .into_any(),
        );
        start = end;
    }
    widget(Stack::row(GAP).align(AlignSpec::Start).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0)))
        .children(columns)
        .into_any()
}

/// 瀑布流的一格：卡片撑满列宽，下面留 14。
fn masonry_cell(line: &CardLine) -> AnyView {
    let cards = line
        .cells
        .iter()
        .map(|cell| cards::card(&cell.row, DisplayMode::Masonry, cell.geometry, cell.state))
        .collect::<Vec<_>>();
    widget(Stack::column(0.0).width(LengthSpec::Fill).with_layout(|layout| layout.margin_bottom = Some(LengthSpec::Px(GAP))))
        .children(cards)
        .into_any()
}

/// CSS 多列的列数和列宽：能放下几列 164 宽（含 14 间距）就放几列，至少一列，再把宽度均分。
pub(super) fn masonry_columns(width: f32) -> (usize, f32) {
    let width = width.max(1.0);
    let count = (((width + GAP) / (MASONRY_COLUMN + GAP)).floor() as usize).max(1);
    let column = ((width - GAP * (count - 1) as f32) / count as f32).max(1.0);
    (count, column)
}

/// 按顺序把条目切成至多 `count` 列，让最高的一列尽量矮（CSS 多列的列平衡）。返回每列的结束下标。
///
/// 算法：列高上限越大越容易装下，「能否在 `count` 列内顺序装完」对上限单调。最矮的上限
/// 落在「最高的单条」和「总高」之间，二分到 0.01px 以内，再按这个上限顺序装填。
/// 每次装填 O(n)，二分至多 48 次；总高很大时 f32 的间隔会大于 0.01，所以次数要封顶。
pub(super) fn balanced_columns(heights: &[f32], count: usize) -> Vec<usize> {
    if heights.is_empty() {
        return Vec::new();
    }
    let count = count.max(1);
    let tallest = heights.iter().copied().fold(0.0_f32, f32::max);
    let total = heights.iter().sum::<f32>().max(tallest);
    if let Some(split) = pack(heights, count, tallest) {
        return split;
    }
    let (mut low, mut high) = (tallest, total);
    for _ in 0..48 {
        if high - low <= 0.01 {
            break;
        }
        let middle = low + (high - low) / 2.0;
        if pack(heights, count, middle).is_some() {
            high = middle;
        } else {
            low = middle;
        }
    }
    pack(heights, count, high).unwrap_or_else(|| {
        eprintln!("Nana 瀑布流分列没有找到可行高度，退回单列：{} 条", heights.len());
        vec![heights.len()]
    })
}

/// 在 `limit` 高度内顺序装填，装得下就返回各列结束下标。
fn pack(heights: &[f32], count: usize, limit: f32) -> Option<Vec<usize>> {
    let mut split = Vec::new();
    let mut used = 0.0;
    for (index, height) in heights.iter().enumerate() {
        if used > 0.0 && used + height > limit + 0.01 {
            split.push(index);
            used = 0.0;
            if split.len() >= count {
                return None;
            }
        }
        used += height;
    }
    split.push(heights.len());
    Some(split)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::backend::services::repository::FileBrowserEntry;

    /// 一个文件条目。`width` 给出时按 宽:100 的原始尺寸算比例。
    fn spec(path: &str, width: Option<u32>) -> CardSpec {
        let mut metadata = BTreeMap::new();
        if let Some(width) = width {
            metadata.insert("width".into(), serde_json::json!(width));
            metadata.insert("height".into(), serde_json::json!(100));
        }
        let mut row = FileRow::from_entry(&FileBrowserEntry {
            path: path.into(),
            name: path.into(),
            kind: "file".into(),
            extension: Some("png".into()),
            size_bytes: None,
            size_label: None,
            modified_at: None,
            asset_id: Some(path.into()),
            status: None,
            thumbnail_path: None,
            thumbnail_custom: false,
            hardlink_group_id: None,
            hardlink_state: None,
            tags: Vec::new(),
            alias_paths: Vec::new(),
            folder_metadata: None,
            metadata,
            is_virtual: false,
            provider_id: None,
            provider_item_id: None,
            source_payload: None,
            local_absolute_path: None,
        });
        if let Some(width) = width {
            row.pixel_width = width;
            row.pixel_height = 100;
        }
        CardSpec { row, state: CardState::default() }
    }

    fn specs(count: usize) -> Vec<CardSpec> {
        (0..count).map(|index| spec(&format!("{index}.png"), None)).collect()
    }

    #[test]
    fn masonry_columns_follow_css_column_width() {
        assert_eq!(masonry_columns(516.0).0, 2);
        assert!((masonry_columns(516.0).1 - 251.0).abs() < 0.01);
        assert_eq!(masonry_columns(276.0).0, 1);
        assert_eq!(masonry_columns(534.0).0, 3);
        assert_eq!(masonry_columns(10.0).0, 1);
    }

    #[test]
    fn balanced_columns_keep_order_and_minimize_the_tallest() {
        assert_eq!(balanced_columns(&[100.0, 100.0], 2), vec![1, 2]);
        assert_eq!(balanced_columns(&[100.0], 3), vec![1]);
        assert_eq!(balanced_columns(&[50.0, 50.0, 100.0], 2), vec![2, 3]);
        assert_eq!(balanced_columns(&[10.0, 10.0, 10.0], 2), vec![2, 3]);
        assert!(balanced_columns(&[], 2).is_empty());
    }

    #[test]
    fn balancing_two_thousand_cards_stays_fast_and_ordered() {
        let heights = (0..2000).map(|index| 180.0 + (index % 7) as f32 * 23.5).collect::<Vec<_>>();
        let started = std::time::Instant::now();
        let split = balanced_columns(&heights, 3);
        assert!(started.elapsed().as_millis() < 200, "两千条分列不能卡住：{:?}", started.elapsed());
        assert_eq!(split.len(), 3);
        assert_eq!(*split.last().unwrap(), heights.len());
        assert!(split.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn grid_lines_fill_left_to_right_like_auto_fill() {
        let lines = wrap_lines(specs(7), 516.0, |_| cards::GRID_BOX);
        assert_eq!(lines.iter().map(|line| line.cells.len()).collect::<Vec<_>>(), vec![3, 3, 1]);
        assert_eq!(&*lines[1].key, "file:3.png", "行键是这一行第一张卡片的条目键");
        let narrow = wrap_lines(specs(2), 100.0, |_| cards::GRID_BOX);
        assert_eq!(narrow.len(), 2, "放不下一张时每张单独成行");
    }

    #[test]
    fn adaptive_lines_wrap_by_each_card_width() {
        let specs = vec![spec("wide.png", Some(240)), spec("tall.png", Some(55)), spec("square.png", Some(100))];
        let lines = wrap_lines(specs, 300.0, cards::adaptive_box);
        for line in &lines {
            let used = line.cells.iter().map(|cell| cell.geometry.width).sum::<f32>() + GAP * (line.cells.len() - 1) as f32;
            assert!(line.cells.len() == 1 || used <= 300.0 + 0.01, "一行装不下时要换行：{used}");
        }
        assert_eq!(lines.iter().map(|line| line.cells.len()).sum::<usize>(), 3);
    }

    #[test]
    fn list_lines_put_one_card_on_each_line() {
        let lines = list_lines(specs(3));
        assert_eq!(lines.len(), 3);
        assert!(lines.iter().all(|line| line.cells.len() == 1));
        assert_eq!(&*lines[1].key, "file:1.png");
    }
}
