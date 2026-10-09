//! 启动路由：首屏加载时的启动页，加载失败时也在这里。
//!
//! 仍由旧视图函数整块建出，ViewModel 每次归约后主区重挂这一分支。

use nana_ui::runtime::view::AnyView;

use super::ShellViewModel;

/// Vue 主区内容层 `overflow: auto`，窗口矮时整页连同内边距一起滚动。
pub(super) fn view(model: &ShellViewModel) -> AnyView {
    super::route_home::scroll_route(super::startup_view::startup_section(model), "workspace-startup-scroll")
}
