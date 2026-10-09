//! 设置路由：整页滚动的设置页。
//!
//! 仍由旧视图函数整块建出，ViewModel 每次归约后主区重挂这一分支。

use nana_ui::runtime::view::AnyView;

use super::ShellViewModel;

/// 设置页只有一张整页，滚动容器用固定键：同一页重挂时保留滚动位置。
pub(super) fn view(model: &ShellViewModel) -> AnyView {
    super::route_home::scroll_route(super::admin::settings_page(model), "settings-scroll")
}
