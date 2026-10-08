//! 文件卡片的列表区，对应 Vue `.files-list` 和读取、处理、出错三种状态框。
//!
//! 列表纵向滚动，内边距 16 / 20 / 20。文件夹一组、分隔线、文件一组，各段保持自身高度、段间距 14，
//! 内容比列表高时由列表滚动。Vue 把这三段放进 CSS grid 的自动行，内容矮时行被拉伸、内容高时
//! 行被压扁让卡片盖到分隔线上，这两种怪癖都不照抄。

use std::cell::RefCell;
use std::sync::Arc;
use std::time::Instant;

use nana_ui::icons_tabler::LOADER_2;
use nana_ui::runtime::view::{widget, AnyView, IntoView};
use nana_ui::runtime::{
    Activate, AlignSpec, LengthSpec, RadiusTier, ScrollAxes, ScrollView, SemanticColorRole, SizeChanged, Stack,
};

use super::super::files::{DisplayMode, FileContext, FileRow, FilesMessage};
use super::super::ShellViewModel;
use super::cards::{self, CardState};
use super::file_message;
use super::style;

/// 列表区左右内边距。
const LIST_PADDING_X: f32 = 20.0;
/// 组内和组间的间距。
const GAP: f32 = 14.0;
/// Vue 瀑布流的 `column-width`。
const MASONRY_COLUMN: f32 = 164.0;

thread_local! {
    static LAST_ROW_CLICK: RefCell<Option<(String, Instant)>> = const { RefCell::new(None) };
}

/// 400 毫秒内连点同一条算双击，第二次才进入目录或打开预览。
pub(super) fn row_activation(path: &str) -> FilesMessage {
    let open = LAST_ROW_CLICK.with(|slot| {
        let mut slot = slot.borrow_mut();
        let now = Instant::now();
        let open = slot.as_ref().is_some_and(|(previous, at)| previous == path && now.duration_since(*at).as_millis() < 400);
        *slot = Some((path.to_string(), now));
        open
    });
    if open {
        FilesMessage::OpenRow(path.to_string())
    } else {
        FilesMessage::ActivateRow(path.to_string())
    }
}

/// 列表区：出错、读取中、处理中时只显示状态框，否则是分组列表。
pub(super) fn body(model: &ShellViewModel) -> AnyView {
    let files = &model.files;
    if !files.error.is_empty() {
        return state_box(files.error.clone(), true, false, model);
    }
    if files.loading {
        return state_box("正在读取目录".into(), false, true, model);
    }
    if files.mutating {
        return state_box("正在处理文件".into(), false, true, model);
    }
    list(model)
}

/// `.asset-browser__state`：圆角小框，白底描边；出错时是浅红底红字。
fn state_box(label: String, error: bool, spinner: bool, model: &ShellViewModel) -> AnyView {
    let (fill, border, color) = if error {
        (None, SemanticColorRole::Danger, SemanticColorRole::Danger)
    } else {
        (Some(SemanticColorRole::Background), SemanticColorRole::Border, SemanticColorRole::Muted)
    };
    let mut frame = Stack::row(8.0).align(AlignSpec::Center).padding_xy(12.0, 10.0).radius(RadiusTier::Xl).with_layout(|layout| {
        layout.margin_top = Some(LengthSpec::Px(16.0));
        layout.margin_left = Some(LengthSpec::Px(LIST_PADDING_X));
        layout.margin_right = Some(LengthSpec::Px(LIST_PADDING_X));
        layout.align_self = Some(AlignSpec::Start);
    });
    let mut node_style = frame.node_style();
    node_style.background = fill;
    node_style.border = Some(border);
    if error {
        node_style.interaction.base.background_mix = Some(nana_ui_core::SemanticColorMix::alpha(SemanticColorRole::Danger, 0.1));
        node_style.interaction.base.border_mix = Some(nana_ui_core::SemanticColorMix::alpha(SemanticColorRole::Danger, 0.36));
    }
    Arc::make_mut(&mut node_style.layout).border_width = Some(1.0);
    frame = frame.style(node_style);
    let icon = spinner.then(|| {
        let mut glyph = nana_ui::runtime::IconGlyph::new(LOADER_2).size(16.0).role(color);
        Arc::make_mut(&mut glyph.style.layout).transform = Some(super::super::motion::spin_transform(model.motion.spinner_degrees()));
        widget(glyph).key("file-state-spinner")
    });
    widget(frame)
        .children((icon, widget(style::text(label, 14.0, 400, color, 21.7)).key("file-state-text")))
        .key(if error { "file-error" } else { "file-state" })
        .into_any()
}

/// 分组列表。宽度按实际布局回报；还没回报时按窗口宽估算，保证首帧就是对的列数。
fn list(model: &ShellViewModel) -> AnyView {
    let ctx = FileContext::from_model(model);
    let rows = model.files.visible_rows(&ctx);
    let virtual_view = ctx.is_virtual();
    let (directories, files): (Vec<FileRow>, Vec<FileRow>) = if virtual_view {
        (Vec::new(), rows)
    } else {
        rows.into_iter().partition(|row| row.kind == "directory")
    };
    let mode = model.files.display_mode;
    let width = model.files.list_width.unwrap_or_else(|| estimated_list_width(model));
    let picked = picked_paths(&model.files);
    let drop_folder = (model.input.internal_active || model.input.dragging_files).then(|| model.input.hover_folder.clone()).flatten();
    let state = |row: &FileRow| CardState {
        selected: picked.iter().any(|path| path == &row.path),
        drop_target: row.kind == "directory" && drop_folder.as_deref() == Some(row.path.as_str()),
    };
    let mut sections = Vec::new();
    if !directories.is_empty() {
        sections.push(section(group(&directories, mode, width, &state), "file-group-directories"));
    }
    if !directories.is_empty() && !files.is_empty() {
        sections.push(section(divider(), "file-group-divider"));
    }
    if !files.is_empty() {
        sections.push(section(group(&files, mode, width, &state), "file-group-files"));
    }
    if let Some(sentinel) = load_more(model) {
        sections.push(section(sentinel, "file-group-more"));
    }
    let content = widget(
        Stack::column(GAP)
            .width(LengthSpec::Fill)
            .height(LengthSpec::Fill)
            .min_width(LengthSpec::Px(0.0))
            .grow(1.0)
            .with_layout(|layout| {
                layout.padding_top = Some(LengthSpec::Px(16.0));
                layout.padding_left = Some(LengthSpec::Px(LIST_PADDING_X));
                layout.padding_right = Some(LengthSpec::Px(LIST_PADDING_X));
                layout.padding_bottom = Some(LengthSpec::Px(20.0));
            }),
    )
    .children(sections)
    .key("file-list-content")
    .on_cx(|_, event: &SizeChanged, cx| {
        cx.dispatch_program(file_message(FilesMessage::ListResized(event.width - LIST_PADDING_X * 2.0)));
    })
    .into_any();
    widget(ScrollView::new(ScrollAxes::Vertical).with_layout(|layout| {
        layout.flex_grow = Some(1.0);
        layout.flex_shrink = Some(1.0);
        layout.flex_basis = Some(LengthSpec::Px(0.0));
        layout.min_height = Some(LengthSpec::Px(0.0));
        layout.min_width = Some(LengthSpec::Px(0.0));
        layout.width = Some(LengthSpec::Fill);
    }))
    .children((content,))
    .key("file-wrap-list")
    .into_any()
}

/// 一段：保持自身高度，段间距 14。
fn section(content: AnyView, key: &'static str) -> AnyView {
    let section = Stack::column(0.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0));
    widget(section).children((content,)).key(key).into_any()
}

/// `.files-list__divider`：1px `--border-strong`，上下各 2px。
fn divider() -> AnyView {
    let line = Stack::column(0.0)
        .width(LengthSpec::Fill)
        .height(LengthSpec::Px(1.0))
        .min_height(LengthSpec::Px(1.0))
        .surface(SemanticColorRole::BorderStrong)
        .with_layout(|layout| {
            layout.margin_top = Some(LengthSpec::Px(2.0));
            layout.margin_bottom = Some(LengthSpec::Px(2.0));
        });
    widget(line).key("file-list-divider").into_any()
}

/// 一组条目，按展示方式排。
fn group(rows: &[FileRow], mode: DisplayMode, width: f32, state: &dyn Fn(&FileRow) -> CardState) -> AnyView {
    match mode {
        DisplayMode::List => {
            let cards = rows.iter().map(|row| cards::card(row, mode, cards::LIST_BOX, state(row))).collect::<Vec<_>>();
            widget(Stack::column(8.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0))).children(cards).into_any()
        }
        DisplayMode::Grid => wrap(rows.iter().map(|row| cards::card(row, mode, cards::GRID_BOX, state(row))).collect()),
        DisplayMode::Adaptive => wrap(rows.iter().map(|row| cards::card(row, mode, cards::adaptive_box(row), state(row))).collect()),
        DisplayMode::Masonry => masonry(rows, width, state),
    }
}

/// 网格和自适应：按卡片自身宽度换行，行列间距 14，靠左排。
fn wrap(cards: Vec<AnyView>) -> AnyView {
    widget(
        Stack::row(GAP)
            .wrap(true)
            .align(AlignSpec::Start)
            .width(LengthSpec::Fill)
            .min_width(LengthSpec::Px(0.0))
            .with_layout(|layout| layout.row_gap = Some(LengthSpec::Px(GAP))),
    )
    .children(cards)
    .into_any()
}

/// 瀑布流：和 CSS `column-width: 164px; column-gap: 14px` 一样定列数和列宽，条目按顺序竖着装进各列。
fn masonry(rows: &[FileRow], width: f32, state: &dyn Fn(&FileRow) -> CardState) -> AnyView {
    let (count, column_width) = masonry_columns(width);
    let heights = rows.iter().map(|row| cards::masonry_height(row, column_width) + GAP).collect::<Vec<_>>();
    let split = balanced_columns(&heights, count);
    let mut columns = Vec::new();
    let mut start = 0;
    for end in split {
        let cards = rows[start..end]
            .iter()
            .map(|row| {
                widget(Stack::column(0.0).width(LengthSpec::Fill).with_layout(|layout| layout.margin_bottom = Some(LengthSpec::Px(GAP))))
                    .children((cards::card(row, DisplayMode::Masonry, cards::masonry_box(row, column_width), state(row)),))
                    .into_any()
            })
            .collect::<Vec<_>>();
        columns.push(
            widget(Stack::column(0.0).width(LengthSpec::Px(column_width)).grow(0.0).shrink(0.0))
                .children(cards)
                .into_any(),
        );
        start = end;
    }
    widget(Stack::row(GAP).align(AlignSpec::Start).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0)))
        .children(columns)
        .into_any()
}

/// CSS 多列的列数和列宽：能放下几列 164 宽（含 14 间距）就放几列，至少一列，再把宽度均分。
pub(super) fn masonry_columns(width: f32) -> (usize, f32) {
    let width = width.max(1.0);
    let count = (((width + GAP) / (MASONRY_COLUMN + GAP)).floor() as usize).max(1);
    let column = ((width - GAP * (count - 1) as f32) / count as f32).max(1.0);
    (count, column)
}

/// 按顺序把条目切成至多 `count` 列，让最高的一列尽量矮（CSS 多列的列平衡）。
///
/// 算法：最矮的可能列高至少是最高的单条；从这里起，逐个候选高度（各前缀和）尝试
/// 顺序装填，第一个能在 `count` 列内装下的高度就是平衡高度。返回每列的结束下标。
pub(super) fn balanced_columns(heights: &[f32], count: usize) -> Vec<usize> {
    if heights.is_empty() {
        return Vec::new();
    }
    let count = count.max(1);
    let tallest = heights.iter().copied().fold(0.0_f32, f32::max);
    let mut candidates = Vec::new();
    for start in 0..heights.len() {
        let mut total = 0.0;
        for height in &heights[start..] {
            total += height;
            if total >= tallest - 0.01 {
                candidates.push(total);
            }
        }
    }
    candidates.sort_by(f32::total_cmp);
    for limit in candidates {
        if let Some(split) = pack(heights, count, limit) {
            return split;
        }
    }
    vec![heights.len()]
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

/// 还没有布局回报时，按窗口宽估算列表内容宽：减去侧栏、主区左右 24、详情 300 和间距 18、
/// 卡片描边和列表左右内边距。窗口不超过 900 时详情排到下面，不再减详情宽。
fn estimated_list_width(model: &ShellViewModel) -> f32 {
    let sidebar = model.motion.sidebar_presented_width();
    let detail = if model.viewport_width <= 900.0 { 0.0 } else { 300.0 + 18.0 };
    (model.viewport_width - sidebar - 48.0 - detail - 2.0 - LIST_PADDING_X * 2.0).max(MASONRY_COLUMN)
}

/// 多选路径加主选。主选不在多选里时也算选中。
fn picked_paths(files: &super::super::files::FilesState) -> Vec<String> {
    let mut paths = files.selected.clone();
    if let Some(primary) = &files.primary {
        if !paths.iter().any(|item| item == primary) {
            paths.push(primary.clone());
        }
    }
    paths
}

/// 还有下一页或正在续读时，跟在卡片后面的「滚动继续加载」。点一下也会续读。
fn load_more(model: &ShellViewModel) -> Option<AnyView> {
    let ctx = FileContext::from_model(model);
    let loading = model.files.loading_more;
    if !model.files.has_more && !loading {
        return None;
    }
    let label = if loading { "继续读取目录..." } else { "滚动继续加载" };
    let mut button = style::styled_button(label, loading.then_some(LOADER_2), style::ButtonLook {
        height: 32.0,
        padding_x: 0.0,
        hover: None,
        weight: 400,
        ..style::ButtonLook::TOOLBAR
    });
    button = button.disabled(!model.files.can_load_more(&ctx) && !loading);
    Some(
        widget(button)
            .key("file-load-more")
            .on_cx(|_, _: &Activate, cx| cx.dispatch_program(file_message(FilesMessage::LoadMore)))
            .into_any(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
