//! 播放消息归约。
//!
//! 播放集列表由壳层的 `apply_playlist_list` 同时交给侧栏和这里。过期详情在这里先跳过，避免播放条切到别的仓库。
//! 播放集成员变了（右键加入、移出，按路径加入）以后，照 Vue `refreshPlaylists` 让侧栏重读播放集列表和成员索引；
//! 页面上点开的那张受影响时跟着刷新。

use super::super::files::repository_is_writable;
use super::super::{ShellMessage, ShellViewModel};
use super::{PlayerEffect, PlayerMessage};

/// 处理播放条消息，并记下随后到达的播放列表结果。
pub(crate) fn reduce_message(model: &mut ShellViewModel, message: ShellMessage) -> Option<ShellMessage> {
    match message {
        ShellMessage::Player(message) => {
            let next = reduce_player(model, message);
            super::super::inspect::sync_preview_frame(model);
            next
        }
        ShellMessage::PlaylistPlayersLoaded(Ok(players)) => {
            model.player.note_players(players.clone());
            Some(ShellMessage::PlaylistPlayersLoaded(Ok(players)))
        }
        ShellMessage::PlaylistDetailLoaded(Ok(detail)) => {
            let stale = model.sidebar.bound_repo_id().is_some_and(|repo_id| repo_id != detail.playlist.repo_id);
            if !stale {
                model.player.note_detail(&detail);
                if model.sidebar.take_pending_play(&detail.playlist.playlist_id) {
                    model.player.play_listed(None, &mut model.inspect);
                }
                model.player.publish(&mut model.inspect);
            }
            Some(ShellMessage::PlaylistDetailLoaded(Ok(detail)))
        }
        other => Some(other),
    }
}

/// 按路径加入（目录、虚拟条目）的结果。成功以后重读播放集列表和成员索引；页面上点开的正是加入的那张时，和读回
/// 详情一样换上它（条目、项数、排序用的编号），返回读回详情的消息交给壳层接着归约；不是它时不动页面上点开的那张
/// （Vue 会把页面换成加入的那张，不照抄）。
fn paths_added(model: &mut ShellViewModel, result: Result<crate::backend::services::repository::PlaylistDetail, String>) -> Option<ShellMessage> {
    let detail = model.player.note_paths_added(result)?;
    model.sidebar.reload_playlists(None);
    if model.player.lists(&detail.playlist.playlist_id) {
        return reduce_message(model, ShellMessage::PlaylistDetailLoaded(Ok(detail)));
    }
    None
}

/// 归约一条播放消息。返回值是还要交给壳层接着归约的消息，只有按路径加入了点开的那张时才有。
fn reduce_player(model: &mut ShellViewModel, message: PlayerMessage) -> Option<ShellMessage> {
    let writable = model.workspace.active_repository().is_some_and(|repository| repository_is_writable(&repository.status, &repository.capabilities));
    let repo_id = model.workspace.active_repo_id.clone();
    let playlist_id = model.selected_playlist_id.clone();
    let item_ids = model.playlist_item_ids.clone();
    match message {
        PlayerMessage::SetCandidates(candidates) => model.player.candidates = candidates,
        PlayerMessage::SetPreference { capability_id, plugin_id } => model.player.set_preference(capability_id, plugin_id),
        PlayerMessage::PlayListed { item_id } => model.player.play_listed(item_id, &mut model.inspect),
        PlayerMessage::PlayItem { item_id } => {
            model.player.play_item(&item_id, true);
            model.player.publish(&mut model.inspect);
        }
        PlayerMessage::PlayNext { natural_end } => model.player.play_next(natural_end, &mut model.inspect),
        PlayerMessage::PlayPrevious => model.player.play_previous(&mut model.inspect),
        PlayerMessage::CycleMode => model.player.cycle_mode(),
        PlayerMessage::SetPlaying(playing) => model.player.set_playing(playing, &mut model.inspect),
        PlayerMessage::Seek(position) => model.player.seek(position, &mut model.inspect),
        PlayerMessage::SetVolume(volume) => model.player.set_volume(volume, &mut model.inspect),
        PlayerMessage::SetImageDuration(value) => model.player.set_image_duration(value),
        PlayerMessage::SetObjectFit { cover } => {
            model.player.settings.object_fit_cover = cover;
            model.player.effects.push(PlayerEffect::PersistSettings);
        }
        PlayerMessage::ToggleQueue => model.player.queue_open = !model.player.queue_open,
        PlayerMessage::BarResized(width) => {
            // 半像素以内的抖动不算变化，避免排版回报来回重建。
            if width.is_finite() && (width - model.player.bar_width).abs() > 0.5 {
                model.player.bar_width = width;
            }
        }
        PlayerMessage::Stop { repo_id, clear_stored } => model.player.stop_for(repo_id, clear_stored, &mut model.inspect),
        PlayerMessage::ToggleMembership { playlist_id, kind, extension, asset_id, is_virtual, path } => {
            model.player.toggle_membership(&playlist_id, &kind, &extension, &asset_id, is_virtual, &path, writable, repo_id.as_deref());
        }
        PlayerMessage::MembershipsLoaded { repo_id, result } => {
            model.player.note_memberships(&repo_id, result, model.workspace.active_repo_id.as_deref());
        }
        PlayerMessage::MembershipSaved(result) => {
            let saved = result.is_ok();
            let changed = model.player.note_membership_saved(result);
            if saved {
                // 点开的那张加入或移出了条目时连它的详情一起重读，页面条目和项数跟着变；Vue 只换页眉的摘要，条目不变。
                let listed = model.player.listed.as_ref().map(|detail| detail.playlist.playlist_id.clone());
                model.sidebar.reload_playlists(listed.filter(|id| changed.contains(id)));
            }
        }
        PlayerMessage::PathsAdded(result) => return paths_added(model, result),
        PlayerMessage::Reorder { source, before } => {
            model.player.reorder(&source, before.as_deref(), playlist_id.as_deref(), &item_ids, writable, repo_id.as_deref());
        }
        PlayerMessage::OpenPreview { item_id } => {
            model.player.open_preview(
                item_id.as_deref(),
                &mut model.workspace.panel,
                &mut model.workspace.library_category,
                &mut model.selected_path,
                &mut model.inspect,
            );
        }
        PlayerMessage::ItemLoaded { item_id, generation, still, result } => {
            super::clip::finish_load(&mut model.player, &item_id, generation, still, result);
            model.player.publish(&mut model.inspect);
        }
        PlayerMessage::RestoreDetail(Ok(detail)) => {
            model.player.note_detail(&detail);
            model.player.publish(&mut model.inspect);
        }
        PlayerMessage::RestoreDetail(Err(error)) => {
            eprintln!("Nana 恢复播放会话时读取播放列表失败：{error}");
            if let Some(repo_id) = model.player.repo_id.clone().or(model.workspace.active_repo_id.clone()) {
                model.player.clear_stored(&repo_id);
            }
            model.player.restore_playlist_id = None;
        }
    }
    None
}
