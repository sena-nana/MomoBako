//! 空库路由：还没有资源库时的拖入引导，整节是系统文件拖放目标。
//!
//! 仍由旧视图函数整块建出，ViewModel 每次归约后主区重挂这一分支。

use nana_ui::runtime::view::AnyView;

use super::ShellViewModel;

pub(super) fn view(model: &ShellViewModel) -> AnyView {
    let body = super::route_home::scroll_body(model, super::input::empty_repository_panel(model));
    super::route_home::page(model, body)
}
