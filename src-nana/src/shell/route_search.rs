//! 搜索路由（常驻）：有仓库时的搜索结果，以及还没有资源库时的搜索面板。
//!
//! 两条路由的分支只在进入时建一次，之后同步只写信号：搜索面板读 [`SearchPanelSignals`]，有仓库时
//! 首页外框上方的筛选栏读 [`FilterBarSignals`]。常驻首页路由共用的外框和滚动主体也在这里
//! （[`home_page`]、[`home_scroll`]），排版和旧视图的 `route_home::page`、`scroll_body` 相同。

use nana_ui::runtime::view::{widget, AnyView, FieldWrite, IntoProp, IntoView};
use nana_ui::runtime::{LengthSpec, ScrollAxes, ScrollView, Stack};

use super::inspect_search_view::{resident_filter_bar, resident_search_panel, FilterBarSignals, SearchPanelSignals};
use super::render::{PRIMARY_INSET_X, PRIMARY_INSET_Y};

/// 有仓库时的搜索结果：筛选栏常驻在上，下面是纵向滚动的搜索面板。
pub(super) fn view(filter: FilterBarSignals, search: SearchPanelSignals) -> AnyView {
    let panel = super::workbench::page(vec![resident_search_panel(search)]);
    home_page(Some(resident_filter_bar(filter)), home_scroll("workspace-page-scroll-HasRepository-Search", false, panel))
}

/// 没有资源库时标题栏搜索也会切到搜索面板，由搜索面板画「还没有可搜索的资源库」。没有仓库就没有筛选栏。
pub(super) fn empty_library(search: SearchPanelSignals) -> AnyView {
    home_page(None, home_scroll("workspace-page-scroll-EmptyRepository-Search", false, resident_search_panel(search)))
}

/// 常驻首页路由的外框：占满主区、自身不滚动，内边距左右 24、上下 20；有筛选栏时它在主体上方。
pub(super) fn home_page(filter: Option<AnyView>, body: AnyView) -> AnyView {
    widget(
        Stack::fill_column(0.0)
            .min_height(LengthSpec::Px(0.0))
            .padding_xy(PRIMARY_INSET_X, PRIMARY_INSET_Y),
    )
    .children((filter, body))
    .key("workspace-page")
    .into_any()
}

/// 常驻首页路由的纵向滚动主体（`.workspace-page__body` 的 `overflow: auto`）。`key` 写区域和面板，
/// 换面板时从顶部开始；`follow_end` 可以绑定，日志追踪时跟随末尾。
pub(super) fn home_scroll(key: &'static str, follow_end: impl IntoProp<bool>, panel: AnyView) -> AnyView {
    let scroll = ScrollView::new(ScrollAxes::Vertical).with_layout(|layout| {
        layout.flex_grow = Some(1.0);
        layout.flex_shrink = Some(1.0);
        layout.min_height = Some(LengthSpec::Px(0.0));
        layout.height = Some(LengthSpec::Fill);
        layout.flex_basis = Some(LengthSpec::Px(0.0));
    });
    widget(scroll).prop::<bool, FollowEnd>(follow_end).children((panel,)).key(key).into_any()
}

/// 滚动容器是否跟随末尾。写成真时运行时当场滚到底，之后内容变长也跟着。
pub(super) struct FollowEnd;

impl FieldWrite<ScrollView, bool> for FollowEnd {
    const FIELD: &'static str = "ScrollView.follow_end";

    fn write(target: &mut ScrollView, follow: bool) {
        target.follow_end = follow;
    }

    fn differs(target: &ScrollView, follow: &bool) -> bool {
        target.follow_end != *follow
    }
}

#[cfg(test)]
#[path = "route_search_tests.rs"]
mod tests;
