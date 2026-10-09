//! 播放集页和底部播放条的组合，对齐 Vue `WorkspacePlaylistPage`。
//!
//! 播放集页是一张 bg-elev 面板：眉题、标题、「播放器 · N 项」和右上角播放；下面每条是
//! 白底圆角行（拖动柄、扩展名方块、文件名和路径、播放与移除），最后是播放条。
//! 没点开播放集时只有虚线空框。播放集页本身（投影、信号和视图）在 `player_playlist_page.rs`。
//! 文件页只挂播放条，预览页的播放条由预览框架放。

use nana_ui::runtime::view::{widget, AnyView, IntoView};
use nana_ui::runtime::{SemanticColorRole, Stack, Text};

use super::player::PlayerMessage;
use super::{ShellMessage, ShellViewModel};

#[path = "player_icons.rs"]
pub(super) mod icons;
#[path = "player_paint.rs"]
pub(super) mod paint;
#[path = "player_bar.rs"]
pub(super) mod bar;
#[path = "player_playlist_page.rs"]
pub(super) mod playlist;

/// 拼进键的外部文本（条目编号、路径）：`/` 是键路径分隔符，`%` 和 `/` 转义成 `%25`、`%2F`，
/// 不同的原文不会撞成同一个键。
pub(crate) fn key_part(text: &str) -> String {
    text.replace('%', "%25").replace('/', "%2F")
}

/// 文件页和播放集页共用的播放表面。播放集面板时是整页，其余是播放条。
/// 文件预览页不经过这里：预览框架自己把播放条贴在页底（Vue `files-preview-page` 里的 `WorkspacePlayerBar`）。
pub(super) fn player_surface(model: &ShellViewModel) -> AnyView {
    if model.workspace.panel == super::workspace::WorkspacePanel::Playlist {
        // 没点开播放集时整块是空框，不建播放条。
        let player_bar = if model.player.listed.is_some() { hosted_bar(model) } else { ().into_any() };
        return playlist::page(model, player_bar);
    }
    hosted_bar(model)
}

/// 播放条，下载进行时下面多一行进度。预览页的页底也用它。
pub(super) fn hosted_bar(model: &ShellViewModel) -> AnyView {
    with_download(model, bar::player_bar(model))
}

/// 下载进度只在下载进行时写在播放条下面一行。
fn with_download(model: &ShellViewModel, bar: AnyView) -> AnyView {
    let download = model.player.download_text();
    if download.is_empty() {
        return bar;
    }
    widget(Stack::column(6.0))
        .children((bar, widget(Text::new(download).color(SemanticColorRole::Muted).font_size(12.0)).key("player-download")))
        .key("player-surface")
        .into_any()
}

/// 没有对应播放插件时，页眉和行上的播放都不可点。
pub(super) fn playlist_plugin_missing(model: &ShellViewModel, player_type_id: &str) -> bool {
    let ready = model.player.candidates.iter().any(|candidate| candidate.player_type_id == player_type_id)
        || model.player.contributions.iter().any(|player| player.player_type_id == player_type_id);
    !ready
}

fn player_message(message: PlayerMessage) -> ShellMessage {
    ShellMessage::Player(message)
}
