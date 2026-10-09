//! 搜索路由（常驻）：有仓库时的搜索结果，以及还没有资源库时的搜索面板。
//!
//! 两条路由的分支只在进入时建一次，之后同步只写信号：搜索面板读 [`SearchPanelSignals`]，有仓库时
//! 首页外框上方的筛选栏读 [`FilterBarSignals`]。外框和滚动主体是首页共用的那一套（`route_home.rs`）。

use nana_ui::runtime::view::AnyView;

use super::inspect_search_view::{resident_filter_bar, resident_search_panel, FilterBarSignals, SearchPanelSignals};
use super::route_home::{home_page, home_scroll};

/// 有仓库时的搜索结果：筛选栏常驻在上，下面是纵向滚动的搜索面板。
pub(super) fn view(filter: FilterBarSignals, search: SearchPanelSignals) -> AnyView {
    let panel = super::workbench::page(vec![resident_search_panel(search)]);
    home_page(Some(resident_filter_bar(filter)), home_scroll("workspace-page-scroll-HasRepository-Search", false, panel))
}

/// 没有资源库时标题栏搜索也会切到搜索面板，由搜索面板画「还没有可搜索的资源库」。没有仓库就没有筛选栏。
pub(super) fn empty_library(search: SearchPanelSignals) -> AnyView {
    home_page(None, home_scroll("workspace-page-scroll-EmptyRepository-Search", false, resident_search_panel(search)))
}

#[cfg(test)]
#[path = "route_search_tests.rs"]
mod tests;
