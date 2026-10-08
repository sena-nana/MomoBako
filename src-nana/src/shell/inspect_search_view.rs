//! 搜索结果面板，对应 Vue `pages/workspace/SearchPanel.vue`。
//!
//! 页头是范围眉题、「搜索结果」、摘要和仓库数、结果数两个计数胶囊。下面依次是搜索中提示、
//! 空状态（没有资源库 / 等待搜索条件）或结果列表。结果行左边是文件图标块，中间是文件名和
//! 「仓库 / 路径」，右边是格式、前三个标签、颜色、形状和评分芯片。
//! 筛选栏的视图在 `search_filter_bar.rs`，放在哪里由壳层决定。

use nana_ui::icons_tabler::LOADER_2;
use nana_ui::runtime::view::{widget, AnyView, IntoView};
use nana_ui::runtime::{
    Activate, AlignSpec, IconGlyph, JustifySpec, LengthSpec, ListItem, RadiusTier, SemanticColorRole, Stack,
};
use nana_ui_core::BorderStyle;

use super::inspect::{InspectMessage, SearchRow};
use super::{ShellMessage, ShellViewModel};

#[path = "search_filter_bar.rs"]
mod filter_bar_view;
#[path = "search_paint.rs"]
mod paint;
#[path = "search_presenter.rs"]
mod presenter;
#[path = "search_widgets.rs"]
mod widgets;

pub(super) use filter_bar_view::filter_bar;

/// 搜索结果面板。占满页面主体的可见高度，内容更高时随页面一起滚动。
pub(super) fn search_panel(model: &ShellViewModel) -> AnyView {
    let inspect = &model.inspect;
    let mut rows = vec![header(model)];
    if !inspect.search_error.is_empty() {
        rows.push(error_state(&inspect.search_error));
    }
    if inspect.searching {
        rows.push(searching_state());
    }
    if model.workspace.repositories.is_empty() {
        rows.push(empty_state("还没有可搜索的资源库", "先在资源库页面添加一个仓库，再执行跨仓库搜索。"));
    } else if !inspect.searching && inspect.results.is_empty() {
        rows.push(empty_state("等待搜索条件", "输入关键词、标签或评分条件后，这里会展示结果。"));
    } else {
        rows.push(results(model));
    }
    widget(
        Stack::column(16.0)
            .padding(23.0)
            .surface(SemanticColorRole::Surface)
            .radius(RadiusTier::Xl)
            .height(LengthSpec::Shrink)
            .min_height(LengthSpec::MinContent)
            .grow(1.0)
            .shrink(0.0),
    )
    .children(rows)
    .key("search-workbench-panel")
    .into_any()
}

/// 页头：左边眉题、标题和摘要，右边仓库数和结果数。
fn header(model: &ShellViewModel) -> AnyView {
    let inspect = &model.inspect;
    let title = widget(
        Stack::column(0.0)
            .width(LengthSpec::Shrink)
            .min_width(LengthSpec::Px(0.0))
            .grow(1.0)
            .shrink(1.0),
    )
    .children((
        widget(widgets::eyebrow(presenter::scope_label(model))).key("search-workbench-scope"),
        widget(widgets::margin_top(widgets::label("搜索结果", 24.0, 700, SemanticColorRole::Text), 4.0))
            .key("search-workbench-title"),
        widget(widgets::margin_top(widgets::wrapping_label(presenter::summary(model), 14.0, 400, SemanticColorRole::Muted), 8.0))
            .key("search-workbench-summary"),
    ));
    let stats = widget(Stack::row(10.0).align(AlignSpec::Center).wrap(true).grow(0.0).shrink(0.0))
        .children((
            widgets::stat(format!("{} 个仓库", model.workspace.repositories.len()), "search-workbench-repositories"),
            widgets::stat(format!("{} 条结果", inspect.results.len()), "search-workbench-hits"),
        ))
        .key("search-workbench-stats");
    widget(Stack::bar(16.0).align(AlignSpec::Start).justify(JustifySpec::SpaceBetween))
        .children((title, stats))
        .key("search-workbench-header")
        .into_any()
}

/// Vue `.asset-browser__state`：搜索进行中的一行提示。
fn searching_state() -> AnyView {
    widget(state_box().outline(SemanticColorRole::Border, 1.0).surface(SemanticColorRole::Background))
        .children((
            widget(IconGlyph::new(LOADER_2).size(16.0)).key("search-workbench-spinner"),
            widget(widgets::label("正在执行全局搜索", 14.0, 400, SemanticColorRole::Muted)).key("search-workbench-searching-text"),
        ))
        .key("search-workbench-searching")
        .into_any()
}

/// 搜索失败。Vue 只把错误写进全局状态、面板里不显示；这里用 `.asset-browser__state--error`
/// 的危险色写出真实错误，不让失败看起来像「没有结果」。
fn error_state(error: &str) -> AnyView {
    widget(state_box().outline(SemanticColorRole::Danger, 1.0))
        .children((widget(widgets::wrapping_label(error, 14.0, 400, SemanticColorRole::Danger)).key("search-workbench-error-text"),))
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

/// Vue `.search-workbench__empty`：虚线框，`--bg-subtle` 底，标题 18px、说明 14px，占满面板剩余高度。
fn empty_state(title: &str, message: &str) -> AnyView {
    widget(
        Stack::column(8.0)
            .justify(JustifySpec::Center)
            .padding(25.0)
            .surface(SemanticColorRole::Subtle)
            .outline(SemanticColorRole::BorderStrong, 1.0)
            .radius(RadiusTier::Xl)
            .grow(1.0)
            .shrink(0.0)
            .with_layout(|layout| {
                layout.border_style = Some(BorderStyle::Dashed);
                layout.border_width = Some(1.0);
            }),
    )
    .children((
        widget(widgets::label(title, 18.0, 700, SemanticColorRole::Text)).key("search-workbench-empty-title"),
        widget(widgets::wrapping_label(message, 14.0, 400, SemanticColorRole::Muted)).key("search-workbench-empty-message"),
    ))
    .key("search-workbench-empty")
    .into_any()
}

/// 结果列表：10px 间距的一列结果行。
fn results(model: &ShellViewModel) -> AnyView {
    let rows = model.inspect.results.iter().map(result_row).collect::<Vec<_>>();
    widget(Stack::column(10.0)).children(rows).key("search-workbench-results").into_any()
}

/// 一条结果：图标块、文件名和「仓库 / 路径」、上下文芯片。点一下打开这条命中。
fn result_row(row: &SearchRow) -> AnyView {
    let key = format!("{}:{}", row.repo_id, row.asset_id);
    let repo_id = row.repo_id.clone();
    let asset_id = row.asset_id.clone();
    let mut item = ListItem::new(row.filename.clone()).auto_height(true).gap(12.0);
    {
        let style = &mut item.style;
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
    let detail = format!("{} / {}", row.repo_name, row.path);
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
            widget(widgets::wrapping_label(row.filename.clone(), 14.0, 700, SemanticColorRole::Text)).key(format!("search-hit-name-{key}")),
            widget(widgets::wrapping_label(detail, 14.0, 500, SemanticColorRole::Muted)).key(format!("search-hit-path-{key}")),
        ))
        .key(format!("search-hit-body-{key}"));
    let chips = presenter::result_context(row)
        .into_iter()
        .enumerate()
        .map(|(index, item)| widgets::hint_chip(&item, format!("search-hit-chip-{key}-{index}")))
        .collect::<Vec<_>>();
    let tags = widget(Stack::row(6.0).align(AlignSpec::Center).wrap(true).justify(JustifySpec::End).grow(0.0).shrink(1.0))
        .children(chips)
        .key(format!("search-hit-tags-{key}"));
    widget(item)
        .key(format!("search-hit-{key}"))
        .leading(tile)
        .content(body)
        .trailing(tags)
        .on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program(ShellMessage::Inspect(InspectMessage::OpenHit {
                repo_id: repo_id.clone(),
                asset_id: asset_id.clone(),
            }));
        })
        .into_any()
}

#[cfg(test)]
#[path = "search_view_tests.rs"]
mod tests;
