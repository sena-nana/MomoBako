//! 搜索结果面板，对应 Vue `pages/workspace/SearchPanel.vue`。
//!
//! 页头是范围眉题、「搜索结果」、摘要和仓库数、结果数两个计数胶囊。下面依次是错误行、搜索中提示、
//! 空状态（没有资源库 / 等待搜索条件 / 跑完没有命中）和结果列表。结果行左边是文件图标块，中间是
//! 文件名和「仓库 / 路径」，右边是格式、前三个标签、颜色、形状和评分芯片。
//!
//! 视图只建一次（[`resident_search_panel`]），只读 [`SearchPanelSignals`]：文案按字段绑定，状态块用
//! `.visible` 显隐，结果行是按「仓库 + 素材」做键的 `each`，再搜一次只换变了的行。
//! 筛选栏的视图在 `search_filter_bar.rs`，放在哪里由壳层决定。

use nana_ui::icons_tabler::LOADER_2;
use nana_ui::runtime::view::{computed, css, each, fields, widget, AnyView, IntoView, Item, Signal, Store, StoreList, StorePath};
use nana_ui::runtime::{
    Activate, AlignSpec, IconGlyph, JustifySpec, LengthSpec, ListItem, RadiusTier, SemanticColorRole, Stack,
};
use nana_ui_core::BorderStyle;

use super::inspect::InspectMessage;
use super::ShellMessage;

#[path = "search_filter_bar.rs"]
mod filter_bar_view;
#[path = "search_filter_state.rs"]
mod filter_state;
#[path = "search_paint.rs"]
mod paint;
#[path = "search_panel_state.rs"]
mod panel_state;
#[path = "search_presenter.rs"]
mod presenter;
#[path = "search_widgets.rs"]
mod widgets;

pub(crate) use filter_bar_view::{filter_bar, resident_filter_bar};
pub(crate) use filter_state::{FilterBarSignals, FilterBarView};
pub(crate) use panel_state::{SearchPanelSignals, SearchPanelView};
use panel_state::{HitChip, HitRow, SearchHead, SearchStatus};

/// 结果行在 Store 里的句柄。
type HitItem = Item<Store<Vec<HitRow>>, String, HitRow>;

/// 常驻搜索面板：只读 `signals`，高度随内容，内容更高时随页面主体一起滚动。
///
/// Vue 的样式写了面板 `min-height: 100%`、空状态 `flex: 1`，但父级高度不定，实际渲染是内容高度。
/// 撑满整页的大虚线框违背 DESIGN.md「避免装饰面板」的克制原则，这里保留实际的内容高度。
pub(crate) fn resident_search_panel(signals: SearchPanelSignals) -> AnyView {
    let status = signals.status;
    let results = signals
        .hits
        .keyed(HitRow::key)
        .each(result_row)
        .gap(10.0)
        .visible(move || status.with(|status| status.listed))
        .key("search-workbench-results");
    widget(
        Stack::column(16.0)
            .padding(23.0)
            .surface(SemanticColorRole::Surface)
            .radius(RadiusTier::Xl)
            .grow(0.0)
            .shrink(0.0),
    )
    .children((header(signals.head), error_state(status), searching_state(status), empty_state(status), results))
    .key("search-workbench-panel")
    .into_any()
}

/// 页头：左边眉题、标题和摘要，右边仓库数和结果数。
fn header(head: Signal<SearchHead>) -> AnyView {
    let title = widget(
        Stack::column(0.0)
            .width(LengthSpec::Shrink)
            .min_width(LengthSpec::Px(0.0))
            .grow(1.0)
            .shrink(1.0),
    )
    .children((
        widgets::bound(widgets::eyebrow(String::new()), move || head.with(|head| head.scope.clone())).key("search-workbench-scope"),
        widget(widgets::margin_top(widgets::label("搜索结果", 24.0, 700, SemanticColorRole::Text), 4.0)).key("search-workbench-title"),
        widgets::bound(
            widgets::margin_top(widgets::wrapping_label(String::new(), 14.0, 400, SemanticColorRole::Muted), 8.0),
            move || head.with(|head| head.summary.clone()),
        )
        .key("search-workbench-summary"),
    ));
    let stats = widget(Stack::row(10.0).align(AlignSpec::Center).wrap(true).grow(0.0).shrink(0.0))
        .children((
            widgets::stat(move || head.with(|head| head.repositories.clone()), "search-workbench-repositories"),
            widgets::stat(move || head.with(|head| head.hits.clone()), "search-workbench-hits"),
        ))
        .key("search-workbench-stats");
    widget(Stack::bar(16.0).align(AlignSpec::Start).justify(JustifySpec::SpaceBetween))
        .children((title, stats))
        .key("search-workbench-header")
        .into_any()
}

/// Vue `.asset-browser__state`：搜索进行中的一行提示。
fn searching_state(status: Signal<SearchStatus>) -> AnyView {
    widget(state_box().outline(SemanticColorRole::Border, 1.0).surface(SemanticColorRole::Background))
        .visible(move || status.with(|status| status.searching))
        .children((
            widget(IconGlyph::new(LOADER_2).size(16.0)).key("search-workbench-spinner"),
            widget(widgets::label("正在执行全局搜索", 14.0, 400, SemanticColorRole::Muted)).key("search-workbench-searching-text"),
        ))
        .key("search-workbench-searching")
        .into_any()
}

/// 搜索失败。Vue 只把错误写进全局状态、面板里不显示；这里用 `.asset-browser__state--error`
/// 的危险色写出真实错误，不让失败看起来像「没有结果」。
fn error_state(status: Signal<SearchStatus>) -> AnyView {
    widget(state_box().outline(SemanticColorRole::Danger, 1.0))
        .visible(move || status.with(|status| !status.error.is_empty()))
        .children((widgets::bound(widgets::wrapping_label(String::new(), 14.0, 400, SemanticColorRole::Danger), move || {
            status.with(|status| status.error.clone())
        })
        .key("search-workbench-error-text"),))
        .key("search-workbench-error")
        .into_any()
}

fn state_box() -> Stack {
    Stack::row(8.0)
        .align(AlignSpec::Center)
        .padding_xy(12.0, 10.0)
        .radius(RadiusTier::Xl)
        .width(LengthSpec::Fill)
        .min_width(LengthSpec::Px(0.0))
        .with_layout(|layout| {
            layout.margin_top = Some(LengthSpec::Px(16.0));
            layout.margin_left = Some(LengthSpec::Px(20.0));
            layout.margin_right = Some(LengthSpec::Px(20.0));
        })
}

/// Vue `.search-workbench__empty`：虚线框，`--bg-subtle` 底，标题 18px、说明 14px。
fn empty_state(status: Signal<SearchStatus>) -> AnyView {
    let text = move |pick: fn(&(String, String)) -> &String| {
        move || status.with(|status| status.empty.as_ref().map(|empty| pick(empty).clone()).unwrap_or_default())
    };
    widget(
        Stack::column(8.0)
            .justify(JustifySpec::Center)
            .padding(24.0)
            .surface(SemanticColorRole::Subtle)
            .outline(SemanticColorRole::BorderStrong, 1.0)
            .radius(RadiusTier::Xl)
            .grow(0.0)
            .shrink(0.0)
            .with_layout(|layout| {
                layout.border_style = Some(BorderStyle::Dashed);
                layout.border_width = Some(1.0);
            }),
    )
    .visible(move || status.with(|status| status.empty.is_some()))
    .children((
        widgets::bound(widgets::label(String::new(), 18.0, 700, SemanticColorRole::Text), text(|empty| &empty.0)).key("search-workbench-empty-title"),
        widgets::bound(widgets::wrapping_label(String::new(), 14.0, 400, SemanticColorRole::Muted), text(|empty| &empty.1))
            .key("search-workbench-empty-message"),
    ))
    .key("search-workbench-empty")
    .into_any()
}

/// 行内一段文字：行已经删掉时读空值，不去读不存在的行。
fn hit_text(item: HitItem, pick: fn(&HitRow) -> &String) -> impl Fn() -> String + Send + 'static {
    move || item.try_with(|row| pick(row).clone()).unwrap_or_default()
}

/// 结果行右侧芯片的排版：`horizontal(6)` 之外补上和旧视图
/// `Stack::row(6).wrap(true).justify(End).grow(0).shrink(1)` 相同的字段。
fn chips_style() -> nana_ui::runtime::view::InlineStyle {
    css! { flex-wrap: wrap; justify-content: flex-end; flex-grow: 0; flex-shrink: 1; }
}

/// 一条结果：图标块、文件名和「仓库 / 路径」、上下文芯片。点一下打开这条命中。
/// 行的身份（仓库和素材 id）建行时就定了，处理器只带它们。
fn result_row(item: HitItem) -> AnyView {
    let (key, repo_id, asset_id) = item.get_untracked().identity();
    let mut list_item = ListItem::new(String::new()).auto_height(true).gap(12.0);
    {
        let style = &mut list_item.style;
        style.background = Some(SemanticColorRole::Background);
        style.radius = Some(RadiusTier::Xl);
        style.control_height = None;
        style.control_padding_x = None;
        style.interaction.hovered.background = Some(SemanticColorRole::Hover);
        let layout = std::sync::Arc::make_mut(&mut style.layout);
        layout.width = Some(LengthSpec::Fill);
        layout.min_width = Some(LengthSpec::Px(0.0));
        layout.min_height = None;
        layout.padding_left = Some(LengthSpec::Px(17.0));
        layout.padding_right = Some(LengthSpec::Px(17.0));
        layout.padding_top = Some(LengthSpec::Px(15.0));
        layout.padding_bottom = Some(LengthSpec::Px(15.0));
        layout.white_space_nowrap = false;
        layout.text_overflow_ellipsis = false;
        layout.font_size = Some(14.0);
        layout.font_weight = Some(500);
    }
    let tile = widget(
        Stack::row(0.0)
            .width(LengthSpec::Px(40.0))
            .height(LengthSpec::Px(40.0))
            .min_width(LengthSpec::Px(40.0))
            .grow(0.0)
            .shrink(0.0)
            .radius(RadiusTier::Lg)
            .with_layout(|layout| layout.paint.background_image = Some(paint::hit_tile_background()))
            .painter(paint::HitIconPainter),
    )
    .key(format!("search-hit-icon-{key}"));
    let body = widget(Stack::column(4.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0)).grow(1.0).shrink(1.0))
        .children((
            widgets::bound(widgets::wrapping_label(String::new(), 14.0, 700, SemanticColorRole::Text), hit_text(item, |row| &row.filename))
                .key(format!("search-hit-name-{key}")),
            widgets::bound(widgets::wrapping_label(String::new(), 14.0, 500, SemanticColorRole::Muted), hit_text(item, |row| &row.detail))
                .key(format!("search-hit-path-{key}")),
        ))
        .key(format!("search-hit-body-{key}"));
    // 行删掉时芯片读成空表，不去读不存在的行。
    let chips = computed(move || item.try_with(|row| row.chips.clone()).unwrap_or_default());
    let chip_key = key.clone();
    let tags = each(chips, HitChip::clone, move |chip: HitChip| widgets::hint_chip(&chip.text, format!("search-hit-chip-{chip_key}-{}", chip.index)))
        .horizontal(6.0)
        .css(chips_style())
        .key(format!("search-hit-tags-{key}"));
    widget(list_item)
        .prop::<String, fields::list_item::label>(hit_text(item, |row| &row.filename))
        .key(format!("search-hit-{key}"))
        .leading(tile)
        .content(body)
        .trailing(tags)
        .on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program_all(ShellMessage::Inspect(InspectMessage::OpenHit {
                repo_id: repo_id.clone(),
                asset_id: asset_id.clone(),
            }));
        })
        .into_any()
}

impl HitRow {
    /// 节点键、仓库和素材 id。
    fn identity(self) -> (String, String, String) {
        (self.node_key, self.repo_id, self.asset_id)
    }
}

#[cfg(test)]
#[path = "search_view_tests.rs"]
mod tests;
