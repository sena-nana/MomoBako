//! 把播放列表副作用交给已有的仓库交互。
//!
//! 成员、排序和按路径添加都没有替身。当前项的字节经仓库服务读取后在任务里解码，
//! 条目里的相对路径不直接拿去读盘。任务都经 `task_guard` 派发：服务没起来、任务排不进去或任务 panic（解码器出错）
//! 时写回错误，避免界面停在“正在提交”或“读取中”。

use nana_ui::RuntimeProgramContext;

use crate::backend::services::repository::FileReadRequest;
use crate::shell::player::{
    decode_loaded, PlayerEffect, PlayerMessage, preferences_path, sessions_path, settings_path,
};
use crate::shell::ShellMessage;
use crate::task_guard::spawn_service;
use crate::MomoBakoApplication;

/// 执行播放器归约留下的请求。
pub fn dispatch_player_effects(app: &mut MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>) {
    for effect in app.shell.player.take_effects() {
        match effect {
            PlayerEffect::PersistSettings => app.shell.player.save_settings_file(&settings_path()),
            PlayerEffect::PersistSessions => app.shell.player.save_sessions_file(&sessions_path()),
            PlayerEffect::PersistPreferences => app.shell.player.save_preferences_file(&preferences_path()),
            PlayerEffect::LoadMemberships { repo_id } => dispatch_memberships(app, context, repo_id),
            PlayerEffect::SetMembership(request) => dispatch_set_membership(app, context, request),
            PlayerEffect::AddByPaths(request) => dispatch_add_paths(app, context, request),
            PlayerEffect::Reorder(request) => dispatch_reorder(app, context, request),
            PlayerEffect::RestoreDetail { repo_id, playlist_id } => dispatch_restore(app, context, repo_id, playlist_id),
            PlayerEffect::LoadItem { repo_id, item_id, path, extension, still, generation } => {
                dispatch_load_item(app, context, LoadRequest { repo_id, item_id, path, extension, still, generation });
            }
        }
    }
}

fn dispatch_memberships(app: &MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>, repo_id: String) {
    let failed_repo = repo_id.clone();
    let failed = move |error| player(PlayerMessage::MembershipsLoaded { repo_id: failed_repo.clone(), result: Err(error) });
    spawn_service(app.services.as_ref(), context, "播放集成员", failed, |services| {
        let interaction = services.repository_interaction.clone();
        let executor = services.executor.clone();
        move || {
            let result = executor.block_on(interaction.list_playlist_memberships(repo_id.clone())).map(|index| index.memberships);
            player(PlayerMessage::MembershipsLoaded { repo_id, result })
        }
    });
}

fn dispatch_set_membership(
    app: &MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    request: crate::backend::services::repository::PlaylistMembershipRequest,
) {
    let failed = |error| player(PlayerMessage::MembershipSaved(Err(error)));
    spawn_service(app.services.as_ref(), context, "播放集成员", failed, |services| {
        let interaction = services.repository_interaction.clone();
        let executor = services.executor.clone();
        move || player(PlayerMessage::MembershipSaved(executor.block_on(interaction.set_playlist_membership(request))))
    });
}

fn dispatch_add_paths(
    app: &MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    request: crate::backend::services::repository::PlaylistItemsByPathsAddRequest,
) {
    let failed = |error| player(PlayerMessage::PathsAdded(Err(error)));
    spawn_service(app.services.as_ref(), context, "播放集添加条目", failed, |services| {
        let interaction = services.repository_interaction.clone();
        let executor = services.executor.clone();
        move || player(PlayerMessage::PathsAdded(executor.block_on(interaction.add_playlist_items_by_paths(request))))
    });
}

fn dispatch_reorder(
    app: &MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    request: crate::backend::services::repository::PlaylistItemsOrderRequest,
) {
    let failed = |error| ShellMessage::PlaylistDetailLoaded(Err(error));
    spawn_service(app.services.as_ref(), context, "播放集排序", failed, |services| {
        let interaction = services.repository_interaction.clone();
        let executor = services.executor.clone();
        move || ShellMessage::PlaylistDetailLoaded(executor.block_on(interaction.reorder_playlist_items(request)))
    });
}

fn dispatch_restore(app: &MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>, repo_id: String, playlist_id: String) {
    let failed = |error| player(PlayerMessage::RestoreDetail(Err(error)));
    spawn_service(app.services.as_ref(), context, "播放集详情", failed, |services| {
        let interaction = services.repository_interaction.clone();
        let executor = services.executor.clone();
        move || player(PlayerMessage::RestoreDetail(executor.block_on(interaction.get_playlist_detail(repo_id, playlist_id))))
    });
}

/// 一次当前项读取请求。
struct LoadRequest {
    repo_id: String,
    item_id: String,
    path: String,
    extension: String,
    still: bool,
    generation: u64,
}

/// 经仓库服务读出当前项，在任务里解码成会话、PCM 和画面，或图片帧。
fn dispatch_load_item(app: &MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>, request: LoadRequest) {
    let LoadRequest { repo_id, item_id, path, extension, still, generation } = request;
    let failed_item = item_id.clone();
    let failed = move |error| player(PlayerMessage::ItemLoaded { item_id: failed_item.clone(), generation, still, result: Err(error) });
    spawn_service(app.services.as_ref(), context, "播放当前项读取", failed, |services| {
        let query = services.repository_query.clone();
        let executor = services.executor.clone();
        move || {
            let result = executor
                .block_on(query.read_file(FileReadRequest { repo_id: repo_id.clone(), path: path.clone() }))
                .map_err(|error| {
                    eprintln!("Nana 播放当前项读取失败：{path}：{error}");
                    format!("无法读取当前项：{error}")
                })
                .and_then(|bytes| decode_loaded(&repo_id, still, &extension, &bytes));
            player(PlayerMessage::ItemLoaded { item_id, generation, still, result })
        }
    });
}

fn player(message: PlayerMessage) -> ShellMessage {
    ShellMessage::Player(message)
}
