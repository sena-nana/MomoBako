//! 主区路由共用的页面外框：首页（`Home.vue` 的 `.workspace-page`）和整页滚动。
//!
//! 首页各路由（文件、搜索、播放集、管理面板、缺失和空库）都放在同一个外框里（[`home_page`]）：有仓库的
//! 首页路由在上面嵌常驻筛选栏，下面是主体。文件面板主体固定高度，由面板内部自己滚；其余面板的主体是
//! 纵向滚动（[`home_scroll`]）。设置和启动页是整页滚动（[`scroll_route`]），内边距跟着内容一起滚。
//! 外框和滚动容器都常驻，只建一次，滚动偏移自然留着。

use nana_ui::runtime::view::{widget, AnyView, FieldWrite, IntoProp, IntoView};
use nana_ui::runtime::{LengthSpec, ScrollAxes, ScrollView, Stack};

use super::render::{PRIMARY_INSET_X, PRIMARY_INSET_Y};

/// 首页外框：占满主区、自身不滚动，内边距左右 24、上下 20；有筛选栏时它在主体上方。
/// 缺失仓库和空库区域不显示筛选栏，传 `None`。
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

/// 首页的纵向滚动主体（`.workspace-page__body` 的 `overflow: auto`）。`key` 写区域和面板，换面板时
/// 从顶部开始；`follow_end` 可以绑定，日志追踪时跟随末尾。
pub(super) fn home_scroll(key: &'static str, follow_end: impl IntoProp<bool>, panel: AnyView) -> AnyView {
    widget(scroll_view()).prop::<bool, FollowEnd>(follow_end).children((panel,)).key(key).into_any()
}

/// 主区内容层的 `overflow: auto`：内边距在滚动内容里，随内容一起滚动。
/// `key` 标出显示的内容，滚动容器常驻，偏移自然留着。
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
