//! 搜索路由：有仓库时的搜索结果，以及还没有资源库时的搜索面板。
//!
//! 仍由旧视图函数整块建出，ViewModel 每次归约后主区重挂这一分支。

use nana_ui::runtime::view::{widget, AnyView, IntoView};
use nana_ui::runtime::Stack;

use super::ShellViewModel;

/// 搜索结果：有检视目标或在搜索面板时显示检视表面。
pub(super) fn view(model: &ShellViewModel) -> AnyView {
    let inspect = if model.inspect_surface_visible() {
        super::inspect_view::inspect_surface(model)
    } else {
        widget(Stack::column(0.0)).into_any()
    };
    let body = super::route_home::scroll_body(model, super::workbench::page(vec![inspect]));
    super::route_home::page(model, body)
}

/// 没有资源库时标题栏搜索也会切到搜索面板，由搜索面板画「还没有可搜索的资源库」。
pub(super) fn empty_library(model: &ShellViewModel) -> AnyView {
    let body = super::route_home::scroll_body(model, super::inspect_search_view::search_panel(model));
    super::route_home::page(model, body)
}
