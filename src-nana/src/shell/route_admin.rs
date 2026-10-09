//! 管理路由：首页里的日志、拓展和仓库动作面板。
//!
//! 三个面板共用一个入口，按工作区面板选内容。仍由旧视图函数整块建出，ViewModel 每次归约后
//! 主区重挂这一分支。

use nana_ui::runtime::view::AnyView;

use super::ShellViewModel;

pub(super) fn view(model: &ShellViewModel) -> AnyView {
    let panel = super::workbench::page(vec![super::admin::admin_surface(model)]);
    super::route_home::page(model, super::route_home::scroll_body(model, panel))
}
