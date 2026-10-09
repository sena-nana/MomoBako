//! 文件列表的虚拟行：只构建视口和预取范围内的卡片。
//!
//! 一组条目先排成「行」再交给 `each_virtual`：列表模式一张卡片一行；网格和自适应按列表
//! 宽度把卡片装进一行（行内、行间都隔 14）；瀑布流先按 CSS 多列的规则分好列，每列各自虚拟化。
//! 所有虚拟列表都跟着同一个列表滚动区滚动（`within`），行高按内容量（`measured`）。
//! 每一行下面都带组内间距，行高就等于估算值（卡片设计尺寸 + 间距），量过以后位置也不跳；
//! 最后一行多出来的间距由外层的负下边距抵掉。
//!
//! 列表常驻：行的身份（[`LineKey`]）是这一行每张卡片的条目键、外框尺寸和标题下的小字行数。
//! 身份没变的行一直留着，卡片的字段绑在卡片仓上；插入、删除、改名或换列宽重新装行时只有身份变了的行重建。
//! 展示方式变了，或者瀑布流的列数、列宽变了（列宽跟着列表宽度连续变），整组按新排法重建。

use std::sync::Arc;

use nana_ui::runtime::view::{computed, dynamic, each_virtual, widget, AnyView, Computed, IntoView, NodeRef, Signal};
use nana_ui::runtime::{AlignSpec, LengthSpec, Stack};

use super::super::files::DisplayMode;
use super::cards::{self, CardBox, CardCell, CardFace, Cards};

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

/// 行里一格的身份：条目键、外框尺寸（按位）和标题下的小字行数。小字行数变了卡片高度跟着变，
/// 按内容量高的虚拟行要整行重建、重新量。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct CellKey {
    key: Arc<str>,
    geometry: [u32; 4],
    lines: usize,
}

/// 一行的身份：这一行每张卡片的条目键和外框尺寸。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct LineKey(Arc<[CellKey]>);

/// 虚拟列表的一行。虚拟列表每次同步都会克隆全部行，所以行放在 `Arc` 里，克隆只加引用计数。
#[derive(Debug)]
pub(super) struct CardLine {
    pub key: LineKey,
    pub cells: Vec<CardCell>,
}

impl CardLine {
    fn new(cells: Vec<CardCell>) -> Arc<Self> {
        let key = cells.iter().map(|cell| CellKey { key: cell.key.clone(), geometry: cell.geometry.bits(), lines: cell.lines }).collect();
        Arc::new(Self { key: LineKey(key), cells })
    }
}

/// 同一身份的行画出来一样：卡片的字段另由卡片仓绑定。
impl PartialEq for CardLine {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl Eq for CardLine {}

/// 一组卡片的排法：展示方式，瀑布流再加列数和列宽。排法变了整组重建。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct GroupShape {
    pub mode: DisplayMode,
    pub columns: usize,
    pub column_width: f32,
}

impl Default for GroupShape {
    fn default() -> Self {
        Self { mode: DisplayMode::Adaptive, columns: 1, column_width: 0.0 }
    }
}

/// 一组卡片排好的行：每列一串（瀑布流以外只有一列），以及每列行高的估计。
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct GroupLines {
    pub columns: Vec<Vec<Arc<CardLine>>>,
    pub estimates: Vec<f32>,
}

/// 把一组卡片按展示方式和列表内容宽排成行。
pub(super) fn pack(faces: &[Arc<CardFace>], mode: DisplayMode, width: f32) -> (GroupShape, GroupLines) {
    let single = |lines: Vec<Arc<CardLine>>, estimate: f32| GroupLines { columns: vec![lines], estimates: vec![estimate] };
    let shape = GroupShape { mode, columns: 1, column_width: 0.0 };
    match mode {
        DisplayMode::List => (shape, single(list_lines(faces), LIST_CARD_HEIGHT + LIST_GAP)),
        DisplayMode::Grid => (shape, single(wrap_lines(faces, width, |_| cards::GRID_BOX), cards::GRID_BOX.height + GAP)),
        DisplayMode::Adaptive => {
            (shape, single(wrap_lines(faces, width, |face| cards::adaptive_box(face.aspect())), ADAPTIVE_CARD_HEIGHT + GAP))
        }
        DisplayMode::Masonry => masonry(faces, width),
    }
}

/// 一格：条目键、类型、路径和外框尺寸。
fn cell(face: &CardFace, geometry: CardBox) -> CardCell {
    CardCell { key: face.key.clone(), kind: face.kind.clone(), path: face.path.clone(), geometry, lines: face.body_lines() }
}

/// 列表模式：一张卡片一行。
fn list_lines(faces: &[Arc<CardFace>]) -> Vec<Arc<CardLine>> {
    faces.iter().map(|face| CardLine::new(vec![cell(face, cards::LIST_BOX)])).collect()
}

/// 网格和自适应：按卡片自身宽度顺序装行，和 flex-wrap / `repeat(auto-fill, 148px)` 一样靠左排。
///
/// 算法：当前行已有卡片时，加上间距和下一张的宽度超过列表宽就换行；每行至少一张，
/// 窄到放不下一张时那一张单独成行。
fn wrap_lines(faces: &[Arc<CardFace>], width: f32, geometry: impl Fn(&CardFace) -> CardBox) -> Vec<Arc<CardLine>> {
    let width = width.max(1.0);
    let mut rows: Vec<Vec<CardCell>> = Vec::new();
    let mut used = 0.0;
    for face in faces {
        let card = geometry(face);
        let start_new = rows.last().is_none_or(|cells| !cells.is_empty() && used + GAP + card.width > width + 0.01);
        if start_new {
            rows.push(Vec::new());
            used = card.width;
        } else {
            used += GAP + card.width;
        }
        if let Some(cells) = rows.last_mut() {
            cells.push(cell(face, card));
        }
    }
    rows.into_iter().map(CardLine::new).collect()
}

/// 瀑布流：和 CSS `column-width: 164px; column-gap: 14px` 一样定列数和列宽，条目按顺序装进各列。
/// 卡片下面留 14（Vue 的 `margin-bottom: 14px`），最后一张也留。每列的估计行高是这一列的平均高。
fn masonry(faces: &[Arc<CardFace>], width: f32) -> (GroupShape, GroupLines) {
    let (count, column_width) = masonry_columns(width);
    let heights = faces.iter().map(|face| cards::masonry_height(face, column_width) + GAP).collect::<Vec<_>>();
    let split = balanced_columns(&heights, count);
    let mut group = GroupLines::default();
    let mut start = 0;
    for end in split {
        group.estimates.push(heights[start..end].iter().sum::<f32>() / (end - start).max(1) as f32);
        group.columns.push(
            faces[start..end]
                .iter()
                .map(|face| CardLine::new(vec![cell(face, cards::masonry_box(face.aspect(), column_width))]))
                .collect(),
        );
        start = end;
    }
    let shape = GroupShape { mode: DisplayMode::Masonry, columns: group.columns.len(), column_width };
    (shape, group)
}

/// 一组卡片的视图：按排法建一次，排法变了整组重建。`key` 是这组列表在内容里的名字。
pub(super) fn group_view(shape: Signal<GroupShape>, lines: Signal<GroupLines>, cards: Cards, scroll: NodeRef, key: &'static str) -> AnyView {
    dynamic(shape, move |shape: &GroupShape| shaped(*shape, lines, cards, scroll, key)).into_any()
}

fn shaped(shape: GroupShape, lines: Signal<GroupLines>, cards: Cards, scroll: NodeRef, key: &'static str) -> AnyView {
    match shape.mode {
        DisplayMode::Masonry => masonry_view(shape, lines, cards, scroll, key),
        DisplayMode::List => virtual_lines(lines, shape.mode, LIST_GAP, cards, scroll, key),
        mode => virtual_lines(lines, mode, GAP, cards, scroll, key),
    }
}

/// 一列的行：组里的行变了才重算，这一列的行没变时虚拟列表不动。
fn column(lines: Signal<GroupLines>, index: usize) -> Computed<Vec<Arc<CardLine>>> {
    computed(move || lines.with(|group| group.columns.get(index).cloned().unwrap_or_default()))
}

/// 这一列的估计行高，建列表时取一次：它只用于还没量过的行。
fn estimate(lines: Signal<GroupLines>, index: usize, fallback: f32) -> f32 {
    lines.with_untracked(|group| group.estimates.get(index).copied().unwrap_or(fallback))
}

/// 把行交给 `each_virtual`：跟外层列表滚动，按内容量行高。每行下面带 `gap`，估算行高是
/// 卡片高加间距；外层用 `-gap` 的下边距抵掉最后一行多出的间距，和不虚拟化时的排法一样。
fn virtual_lines(lines: Signal<GroupLines>, mode: DisplayMode, gap: f32, cards: Cards, scroll: NodeRef, key: &'static str) -> AnyView {
    let height = estimate(lines, 0, 1.0);
    let list = each_virtual(column(lines, 0), |line: &Arc<CardLine>| line.key.clone(), height, move |line| line_view(&line, mode, gap, cards))
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
fn line_view(line: &CardLine, mode: DisplayMode, gap: f32, cards: Cards) -> AnyView {
    let frame = if mode.is_list() { Stack::column(0.0) } else { Stack::row(GAP).align(AlignSpec::Start) };
    let views = line.cells.iter().map(|cell| cards::card(cell, mode, cards)).collect::<Vec<_>>();
    widget(frame.width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0)).with_layout(move |layout| {
        layout.padding_bottom = Some(LengthSpec::Px(gap));
    }))
    .children(views)
    .into_any()
}

/// 瀑布流：每列各自虚拟化，列宽固定。
fn masonry_view(shape: GroupShape, lines: Signal<GroupLines>, cards: Cards, scroll: NodeRef, key: &'static str) -> AnyView {
    let columns = (0..shape.columns)
        .map(|index| {
            let height = estimate(lines, index, 1.0);
            let list = each_virtual(column(lines, index), |line: &Arc<CardLine>| line.key.clone(), height, move |line| masonry_cell(&line, cards))
                .measured()
                .overscan(OVERSCAN)
                .within(scroll)
                .key(format!("{key}-col-{index}"));
            widget(Stack::column(0.0).width(LengthSpec::Px(shape.column_width)).grow(0.0).shrink(0.0))
                .children((list,))
                .key(format!("{key}-column-{index}"))
                .into_any()
        })
        .collect::<Vec<_>>();
    widget(Stack::row(GAP).align(AlignSpec::Start).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0)))
        .children(columns)
        .into_any()
}

/// 瀑布流的一格：卡片撑满列宽，下面留 14。
fn masonry_cell(line: &CardLine, cards: Cards) -> AnyView {
    let views = line.cells.iter().map(|cell| cards::card(cell, DisplayMode::Masonry, cards)).collect::<Vec<_>>();
    widget(Stack::column(0.0).width(LengthSpec::Fill).with_layout(|layout| layout.margin_bottom = Some(LengthSpec::Px(GAP))))
        .children(views)
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
    if let Some(split) = pack_columns(heights, count, tallest) {
        return split;
    }
    let (mut low, mut high) = (tallest, total);
    for _ in 0..48 {
        if high - low <= 0.01 {
            break;
        }
        let middle = low + (high - low) / 2.0;
        if pack_columns(heights, count, middle).is_some() {
            high = middle;
        } else {
            low = middle;
        }
    }
    pack_columns(heights, count, high).unwrap_or_else(|| {
        eprintln!("Nana 瀑布流分列没有找到可行高度，退回单列：{} 条", heights.len());
        vec![heights.len()]
    })
}

/// 在 `limit` 高度内顺序装填，装得下就返回各列结束下标。
fn pack_columns(heights: &[f32], count: usize, limit: f32) -> Option<Vec<usize>> {
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
    use crate::shell::files::FileRow;

    /// 一个文件条目的卡片。`width` 给出时按 宽:100 的原始尺寸算比例。
    fn face(path: &str, width: Option<u32>) -> Arc<CardFace> {
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
            metadata: BTreeMap::new(),
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
        Arc::new(CardFace::of(&row, &[]))
    }

    fn faces(count: usize) -> Vec<Arc<CardFace>> {
        (0..count).map(|index| face(&format!("{index}.png"), None)).collect()
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
        let lines = wrap_lines(&faces(7), 516.0, |_| cards::GRID_BOX);
        assert_eq!(lines.iter().map(|line| line.cells.len()).collect::<Vec<_>>(), vec![3, 3, 1]);
        assert_eq!(&*lines[1].cells[0].key, "file:3.png", "第二行从第四张开始");
        let narrow = wrap_lines(&faces(2), 100.0, |_| cards::GRID_BOX);
        assert_eq!(narrow.len(), 2, "放不下一张时每张单独成行");
    }

    #[test]
    fn adaptive_lines_wrap_by_each_card_width() {
        let faces = vec![face("wide.png", Some(240)), face("tall.png", Some(55)), face("square.png", Some(100))];
        let lines = wrap_lines(&faces, 300.0, |face| cards::adaptive_box(face.aspect()));
        for line in &lines {
            let used = line.cells.iter().map(|cell| cell.geometry.width).sum::<f32>() + GAP * (line.cells.len() - 1) as f32;
            assert!(line.cells.len() == 1 || used <= 300.0 + 0.01, "一行装不下时要换行：{used}");
        }
        assert_eq!(lines.iter().map(|line| line.cells.len()).sum::<usize>(), 3);
    }

    #[test]
    fn list_lines_put_one_card_on_each_line() {
        let lines = list_lines(&faces(3));
        assert_eq!(lines.len(), 3);
        assert!(lines.iter().all(|line| line.cells.len() == 1));
        assert_eq!(&*lines[1].cells[0].key, "file:1.png");
    }

    /// 行的身份是成员和尺寸：同样的卡片同样的宽度排出同样的行；换一张卡片或改了尺寸，那一行身份就变。
    #[test]
    fn line_identity_follows_members_and_geometry() {
        let first = pack(&faces(7), DisplayMode::Grid, 516.0).1;
        let again = pack(&faces(7), DisplayMode::Grid, 516.0).1;
        assert_eq!(first, again, "同样的输入排出同样的行");
        let mut renamed = faces(7);
        renamed[4] = face("renamed.png", None);
        let changed = pack(&renamed, DisplayMode::Grid, 516.0).1;
        assert_eq!(first.columns[0][0], changed.columns[0][0], "没动的行身份不变");
        assert_ne!(first.columns[0][1], changed.columns[0][1], "成员变了的那一行身份变了");
        assert_eq!(first.columns[0][2], changed.columns[0][2]);
        let wide = pack(&[face("a.png", Some(100))], DisplayMode::Adaptive, 516.0).1;
        let narrow = pack(&[face("a.png", Some(240))], DisplayMode::Adaptive, 516.0).1;
        assert_ne!(wide, narrow, "宽高比变了卡片尺寸就变，行身份跟着变");
    }
}
