//! 播放集路由：播放表面，以及不能播放的条目。名称和当前目录不在这一页另开输入。
//!
//! 仍由旧视图函数整块建出，ViewModel 每次归约后主区重挂这一分支。播放集页本身已经写成投影加绑定
//! （`player_playlist_page.rs`），重挂时在挂载作用域里新建信号。改成常驻还差：页面信号放进
//! `RouteSignals` 并在同步时写入，播放条换成旧视图岛（没点开播放集时岛里不放东西），筛选栏换成常驻版本，
//! 以及这里的播放表面开关（只在播放集面板显示）和「不可播放项目」也改成绑定。

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
