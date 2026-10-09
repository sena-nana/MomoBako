//! 主区路由共用的页面外框：首页（`Home.vue` 的 `.workspace-page`）和整页滚动。
//!
//! 首页各路由（文件、搜索、播放集、管理面板、缺失和空库）都放在同一个外框里：有仓库且筛选栏打开时
//! 筛选栏在上，下面是主体。文件面板主体固定高度，由面板内部自己滚；其余面板的主体是纵向滚动。
//! 设置和启动页是整页滚动，内边距跟着内容一起滚。

use nana_ui::runtime::view::{widget, AnyView, IntoView};
use nana_ui::runtime::{LengthSpec, ScrollAxes, ScrollView, Stack};

use super::render::{PRIMARY_INSET_X, PRIMARY_INSET_Y};
use super::{MainRegion, ShellViewModel};

/// 首页外框：占满主区、自身不滚动，内边距左右 24、上下 20。
pub(super) fn page(model: &ShellViewModel, body: AnyView) -> AnyView {
    let filter = (model.workspace.main_region() == MainRegion::HasRepository && model.inspect.filter_bar_open)
        .then(|| super::inspect_search_view::filter_bar(model));
    frame(filter, body)
}

/// 常驻路由的首页：外框里一个纵向滚动的主体，滚动容器用固定键，只建一次，偏移自然留着。
/// 缺失仓库和空库区域本来就不显示筛选栏，也不跟随日志末尾，和 [`page`] 加 [`scroll_body`] 建出的一样。
pub(super) fn resident_page(panel: AnyView, scroll_key: &'static str) -> AnyView {
    let body = widget(scroll_view().follow_end(false)).children((panel,)).key(scroll_key).into_any();
    frame(None, body)
}

fn frame(filter: Option<AnyView>, body: AnyView) -> AnyView {
    widget(
        Stack::fill_column(0.0)
            .min_height(LengthSpec::Px(0.0))
            .padding_xy(PRIMARY_INSET_X, PRIMARY_INSET_Y),
    )
    .children((filter, body))
    .key("workspace-page")
    .into_any()
}

/// 文件面板以外的首页主体：纵向滚动（`.workspace-page__body` 的 `overflow: auto`）。
///
/// 滚动容器按区域和面板取键：换面板时从顶部开始，同一面板重挂时保留滚动位置。日志面板追踪时跟随末尾。
pub(super) fn scroll_body(model: &ShellViewModel, panel: AnyView) -> AnyView {
    let region = model.workspace.main_region();
    widget(scroll_view().follow_end(model.admin_logs_follow_end()))
        .children((panel,))
        .key(format!("workspace-page-scroll-{region:?}-{:?}", model.workspace.panel))
        .into_any()
}

/// 首页没有可显示的面板时的空主体。
pub(super) fn blank(model: &ShellViewModel) -> AnyView {
    eprintln!("Nana 首页没有可显示的面板：{:?}", model.workspace.panel);
    page(model, scroll_body(model, widget(Stack::column(0.0)).into_any()))
}

/// 主区内容层的 `overflow: auto`：内边距在滚动内容里，随内容一起滚动。
/// `key` 标出显示的内容，重挂时同键的滚动容器保留滚动位置。
pub(super) fn scroll_route(content: AnyView, key: &'static str) -> AnyView {
    let padded = widget(Stack::column(0.0).padding_xy(PRIMARY_INSET_X, PRIMARY_INSET_Y))
        .children((content,))
        .into_any();
    widget(scroll_view()).children((padded,)).key(key).into_any()
}

/// 占满剩余高度的纵向滚动。
fn scroll_view() -> ScrollView {
    ScrollView::new(ScrollAxes::Vertical).with_layout(|layout| {
        layout.flex_grow = Some(1.0);
        layout.flex_shrink = Some(1.0);
        layout.min_height = Some(LengthSpec::Px(0.0));
        layout.height = Some(LengthSpec::Fill);
        layout.flex_basis = Some(LengthSpec::Px(0.0));
    })
}
