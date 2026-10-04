//! 把播放列表副作用交给已有的仓库交互和下载协议。
//!
//! 成员、排序、按路径添加和下载都没有替身。服务没启动或任务提交失败时写回错误，
//! 避免界面停在“正在提交”。

use nana_ui::runtime::Task;
use nana_ui::RuntimeProgramContext;

use crate::backend::services::mutsuki_runner::PROTOCOL_PLAYLIST_DOWNLOAD;
use crate::shell::player::{
    PlayerEffect, PlayerMessage, preferences_path, sessions_path, settings_path,
};
use crate::shell::ShellMessage;
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
            PlayerEffect::Download(request) => dispatch_download(app, context, request),
            PlayerEffect::RestoreDetail { repo_id, playlist_id } => dispatch_restore(app, context, repo_id, playlist_id),
        }
    }
}

fn dispatch_memberships(app: &mut MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>, repo_id: String) {
    let Some(services) = app.services.as_ref() else {
        eprintln!("Nana 播放列表成员需要领域服务，当前服务未启动");
        app.shell.reduce(player(PlayerMessage::MembershipsLoaded {
            repo_id,
            result: Err("领域服务未启动".into()),
        }));
        return;
    };
    let interaction = services.repository_interaction.clone();
    let executor = services.executor.clone();
    let task_repo = repo_id.clone();
    if let Err(error) = context.run_task(Task::new(async move {
        player(PlayerMessage::MembershipsLoaded {
            repo_id: task_repo.clone(),
            result: executor.block_on(interaction.list_playlist_memberships(task_repo)).map(|index| index.memberships),
        })
    })) {
        eprintln!("Nana 播放列表成员任务提交失败：{error}");
        app.shell.reduce(player(PlayerMessage::MembershipsLoaded {
            repo_id,
            result: Err(format!("播放列表成员任务提交失败：{error}")),
        }));
    }
}

fn dispatch_set_membership(
    app: &mut MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    request: crate::backend::services::repository::PlaylistMembershipRequest,
) {
    let Some(services) = app.services.as_ref() else {
        eprintln!("Nana 更新播放列表成员需要领域服务，当前服务未启动");
        app.shell.reduce(player(PlayerMessage::MembershipSaved(Err("领域服务未启动".into()))));
        return;
    };
    let interaction = services.repository_interaction.clone();
    let executor = services.executor.clone();
    if let Err(error) = context.run_task(Task::new(async move {
        player(PlayerMessage::MembershipSaved(executor.block_on(interaction.set_playlist_membership(request))))
    })) {
        eprintln!("Nana 更新播放列表成员任务提交失败：{error}");
        app.shell.reduce(player(PlayerMessage::MembershipSaved(Err(format!("播放列表成员任务提交失败：{error}")))));
    }
}

fn dispatch_add_paths(
    app: &mut MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    request: crate::backend::services::repository::PlaylistItemsByPathsAddRequest,
) {
    let Some(services) = app.services.as_ref() else {
        eprintln!("Nana 按路径加入播放列表需要领域服务，当前服务未启动");
        app.shell.reduce(ShellMessage::PlaylistDetailLoaded(Err("领域服务未启动".into())));
        return;
    };
    let interaction = services.repository_interaction.clone();
    let executor = services.executor.clone();
    if let Err(error) = context.run_task(Task::new(async move {
        ShellMessage::PlaylistDetailLoaded(executor.block_on(interaction.add_playlist_items_by_paths(request)))
    })) {
        eprintln!("Nana 按路径加入播放列表任务提交失败：{error}");
        app.shell.reduce(ShellMessage::PlaylistDetailLoaded(Err(format!("播放列表添加任务提交失败：{error}"))));
    }
}

fn dispatch_reorder(
    app: &mut MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    request: crate::backend::services::repository::PlaylistItemsOrderRequest,
) {
    let Some(services) = app.services.as_ref() else {
        eprintln!("Nana 播放列表排序需要领域服务，当前服务未启动");
        app.shell.reduce(ShellMessage::PlaylistDetailLoaded(Err("领域服务未启动".into())));
        return;
    };
    let interaction = services.repository_interaction.clone();
    let executor = services.executor.clone();
    if let Err(error) = context.run_task(Task::new(async move {
        ShellMessage::PlaylistDetailLoaded(executor.block_on(interaction.reorder_playlist_items(request)))
    })) {
        eprintln!("Nana 播放列表排序任务提交失败：{error}");
        app.shell.reduce(ShellMessage::PlaylistDetailLoaded(Err(format!("播放列表排序任务提交失败：{error}"))));
    }
}

fn dispatch_download(
    app: &mut MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    request: crate::backend::services::repository::DownloaderPlaylistRequest,
) {
    let Some(services) = app.services.as_ref() else {
        eprintln!("Nana 播放列表下载需要领域服务，当前服务未启动");
        app.shell.reduce(player(PlayerMessage::DownloadCompleted(Err("领域服务未启动".into()))));
        return;
    };
    let tasks = services.tasks.clone();
    let executor = services.executor.clone();
    if let Err(error) = context.run_task(Task::new(async move {
        player(PlayerMessage::DownloadCompleted(executor.block_on(tasks.execute(PROTOCOL_PLAYLIST_DOWNLOAD, request))))
    })) {
        eprintln!("Nana 播放列表下载任务提交失败：{error}");
        app.shell.reduce(player(PlayerMessage::DownloadCompleted(Err(format!("播放列表下载任务提交失败：{error}")))));
    }
}

fn dispatch_restore(
    app: &mut MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    repo_id: String,
    playlist_id: String,
) {
    let Some(services) = app.services.as_ref() else {
        eprintln!("Nana 恢复播放会话需要领域服务，当前服务未启动");
        app.shell.reduce(player(PlayerMessage::RestoreDetail(Err("领域服务未启动".into()))));
        return;
    };
    let interaction = services.repository_interaction.clone();
    let executor = services.executor.clone();
    if let Err(error) = context.run_task(Task::new(async move {
        player(PlayerMessage::RestoreDetail(executor.block_on(interaction.get_playlist_detail(repo_id, playlist_id))))
    })) {
        eprintln!("Nana 恢复播放会话任务提交失败：{error}");
        app.shell.reduce(player(PlayerMessage::RestoreDetail(Err(format!("播放列表详情任务提交失败：{error}")))));
    }
}

fn player(message: PlayerMessage) -> ShellMessage {
    ShellMessage::Player(message)
}
