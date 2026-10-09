//! 播放集页和底部播放条的组合，对齐 Vue `WorkspacePlaylistPage`。
//!
//! 播放集页是一张 bg-elev 面板：眉题、标题、「播放器 · N 项」和右上角播放；下面每条是
//! 白底圆角行（拖动柄、扩展名方块、文件名和路径、播放与移除），最后是播放条。
//! 没点开播放集时只有虚线空框。播放集页本身（投影、信号和视图）在 `player_playlist_page.rs`。
//! 播放条仍是旧视图：文件页、预览页和播放集页都把它登记成岛，版本按 [`bar_stamp`] 算。

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

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

/// 播放条，下载进行时下面多一行进度。文件页、预览页和播放集页都用它。
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

/// 播放条岛的版本：播放条读到的状态（`player_bar.rs` 的 `BarProps`、窄排法、圆角、下载进度）和岛该不该有内容
/// （在不在预览、显不显示播放表面、点开的播放集）。
///
/// 播放条量到自己的宽度后会发 `BarResized`，每次归约都重建会量了又建、建了又量，所以这里不按 ViewModel
/// 版本，只按它真正读到的值。播放进度和时间走热信号，不在里面。播放条改读别的字段时这里跟着加。
pub(super) fn bar_stamp(model: &ShellViewModel) -> u64 {
    let mut hasher = DefaultHasher::new();
    let player = &model.player;
    let session = &player.session;
    (super::files_view::previewing(model), model.player_surface_visible()).hash(&mut hasher);
    (player.download_text(), &player.current_id, player.can_play, player.queue_open, &player.repo_id).hash(&mut hasher);
    format!("{:?}", player.mode).hash(&mut hasher);
    (player.settings.image_duration_ms, player.settings.object_fit_cover).hash(&mut hasher);
    (&session.status, &session.error, session.volume.to_bits(), session.duration_ms).hash(&mut hasher);
    player.listed.as_ref().map(|detail| &detail.playlist.playlist_id).hash(&mut hasher);
    for item in &player.queue {
        (&item.id, &item.playlist_id, &item.asset_id, &item.path, &item.filename, &item.extension, &item.status).hash(&mut hasher);
        (&item.status_reason, item.transient, &item.player_type_id, &item.player_label, &item.file_class, &item.thumbnail_path).hash(&mut hasher);
    }
    if let Some(item) = player.current_item() {
        let contribution = player.contributions.iter().find(|entry| entry.player_type_id == item.player_type_id);
        contribution.map(|entry| (&entry.file_class, entry.supports_seek, entry.supports_volume)).hash(&mut hasher);
        let candidate = player.candidates.iter().find(|entry| entry.player_type_id == item.player_type_id);
        candidate.map(|entry| (&entry.file_class, entry.supports_seek, entry.supports_volume)).hash(&mut hasher);
        let thumbnail = item.thumbnail_path.as_deref();
        let ready = model.files.rows.iter().any(|row| row.texture_ready && row.thumbnail_path.as_deref() == thumbnail);
        (thumbnail, ready).hash(&mut hasher);
    }
    (player.bar_width.to_bits(), model.narrow_viewport(), model.admin.corner_radius.to_bits()).hash(&mut hasher);
    hasher.finish()
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
