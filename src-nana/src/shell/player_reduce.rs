//! 播放消息归约。
//!
//! 播放列表加载仍交回壳层更新标签。过期详情在这里先跳过，避免播放条切到别的仓库。

use super::super::files::repository_is_writable;
use super::super::{ShellMessage, ShellViewModel};
use super::{PlayerEffect, PlayerMessage};

/// 处理播放条消息，并记下随后到达的播放列表结果。
pub(crate) fn reduce_message(model: &mut ShellViewModel, message: ShellMessage) -> Option<ShellMessage> {
    match message {
        ShellMessage::Player(message) => {
            reduce_player(model, message);
            None
        }
        ShellMessage::PlaylistsLoaded(Ok(playlists)) => {
            if let Some(repo_id) = model.workspace.active_repo_id.clone().or(model.repository_id.clone()) {
                model.player.note_playlists(&repo_id, &playlists);
            }
            Some(ShellMessage::PlaylistsLoaded(Ok(playlists)))
        }
        ShellMessage::PlaylistPlayersLoaded(Ok(players)) => {
            model.player.note_players(players.clone());
            Some(ShellMessage::PlaylistPlayersLoaded(Ok(players)))
        }
        ShellMessage::PlaylistDetailLoaded(Ok(detail)) => {
            let stale = model.sidebar.bound_repo_id().is_some_and(|repo_id| repo_id != detail.playlist.repo_id);
            if !stale {
                model.player.note_detail(&detail);
                model.player.publish(&mut model.inspect);
            }
            Some(ShellMessage::PlaylistDetailLoaded(Ok(detail)))
        }
        other => Some(other),
    }
}

fn reduce_player(model: &mut ShellViewModel, message: PlayerMessage) {
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
        PlayerMessage::PlayEntry { repo_id, kind, extension, asset_id, is_virtual: _, path, filename } => {
            model.player.play_entry(&repo_id, &kind, &extension, &asset_id, &path, &filename, &mut model.inspect);
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
        PlayerMessage::Stop { repo_id, clear_stored } => model.player.stop_for(repo_id, clear_stored, &mut model.inspect),
        PlayerMessage::ToggleMembership { playlist_id, kind, extension, asset_id, is_virtual, path } => {
            model.player.toggle_membership(&playlist_id, &kind, &extension, &asset_id, is_virtual, &path, writable, repo_id.as_deref());
        }
        PlayerMessage::MembershipsLoaded { repo_id, result } => {
            model.player.note_memberships(&repo_id, result, model.workspace.active_repo_id.as_deref());
        }
        PlayerMessage::MembershipSaved(result) => model.player.note_membership_saved(result),
        PlayerMessage::StartDownload(request) => model.player.start_download(request),
        PlayerMessage::DownloadCompleted(result) => model.player.finish_download(result),
        PlayerMessage::CancelDownload => {
            eprintln!("Nana 下载任务还没有可取消的句柄");
            model.player.activity = "下载任务还没有可取消的句柄".into();
        }
        PlayerMessage::Reorder { source, before } => {
            model.player.reorder(&source, before.as_deref(), playlist_id.as_deref(), &item_ids, writable, repo_id.as_deref());
        }
        PlayerMessage::OpenPreview => {
            model.player.open_preview(
                &mut model.workspace.panel,
                &mut model.workspace.library_category,
                &mut model.selected_path,
                &mut model.inspect,
            );
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
            model.player.activity = error;
        }
    }
}
