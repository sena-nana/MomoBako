//! 文件路由：文件面板（含回收站、智能文件夹和文件预览）。
//!
//! 主体固定高度，由面板内部自己滚（`.workspace-page__body--fixed`）。仍由旧视图函数整块建出，
//! ViewModel 每次归约后主区重挂这一分支。

use nana_ui::runtime::view::{widget, AnyView, IntoView};
use nana_ui::runtime::{LengthSpec, Stack};

use super::ShellViewModel;

pub(super) fn view(model: &ShellViewModel) -> AnyView {
    let body = widget(Stack::fill_column(0.0).min_height(LengthSpec::Px(0.0)))
        .children((super::files_view::live_file_column(model),))
        .key("workspace-page-body")
        .into_any();
    super::route_home::page(model, body)
}
