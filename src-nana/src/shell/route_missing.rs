//! 缺失仓库路由：重定向、刷新和删除资源库。
//!
//! 仍由旧视图函数整块建出，ViewModel 每次归约后主区重挂这一分支。

use nana_ui::runtime::view::AnyView;

use super::ShellViewModel;

pub(super) fn view(model: &ShellViewModel) -> AnyView {
    let body = super::route_home::scroll_body(model, super::startup_view::missing_repository_section(model));
    super::route_home::page(model, body)
}
