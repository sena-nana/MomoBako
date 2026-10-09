//! 播放集路由：播放表面，以及不能播放的条目。名称和当前目录不在这一页另开输入。
//!
//! 仍由旧视图函数整块建出，ViewModel 每次归约后主区重挂这一分支。

use nana_ui::runtime::view::{text, AnyView, IntoView};

use super::ShellViewModel;

pub(super) fn view(model: &ShellViewModel) -> AnyView {
    let mut body = Vec::new();
    if model.player_surface_visible() {
        body.push(super::player_view::player_surface(model));
    }
    if !model.playlist_item_status.is_empty() {
        body.push(text(format!("不可播放项目：{}", model.playlist_item_status)).key("playlist-item-status").into_any());
    }
    super::route_home::page(model, super::route_home::scroll_body(model, super::workbench::page(body)))
}
