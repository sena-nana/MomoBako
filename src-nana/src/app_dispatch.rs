//! 按壳层消息派发领域服务任务。
//!
//! `MomoBakoApplication::update` 在归约之前调用 [`dispatch_services`]：播放集条目移除、
//! 打开目录和读文件元数据这些消息各自提交一个后台任务，结果再作为新消息回到 `update`。页面数据的读取不在这里，
//! 由归约排出副作用，再由各 `*_dispatch` 交给领域服务。

use nana_ui::runtime::Task;
use nana_ui::RuntimeProgramContext;

use crate::backend::services::repository::FileBrowserRequest;
use crate::shell::{ShellMessage, StartupStatus};
use crate::{services, MomoBakoApplication};

/// 归约前提交这条消息需要的服务任务。服务没启动时不提交，任务提交失败只记日志。
pub(crate) fn dispatch_services(
    app: &MomoBakoApplication,
    message: &ShellMessage,
    context: &RuntimeProgramContext<ShellMessage>,
) {
    dispatch_playlists(app, message, context);
    dispatch_browse_and_preview(app, message, context);
}

/// 资源库摘要成功时它所属的仓库。归约后据此决定是否读取根目录。
pub(crate) fn snapshot_repo(message: &ShellMessage) -> Option<String> {
    if let ShellMessage::RepositorySnapshotLoaded(Ok(snapshot)) = message {
        Some(snapshot.repository.repo_id.clone())
    } else {
        None
    }
}

/// 归约之后：摘要确认属于当前仓库、启动没有失败且已走到第 4 步时读取根目录。
pub(crate) fn after_snapshot(
    app: &MomoBakoApplication,
    snapshot_repo_id: Option<String>,
    context: &RuntimeProgramContext<ShellMessage>,
) {
    if let Some(repo_id) = snapshot_repo_id
        && app.shell.repository_id.as_deref() == Some(repo_id.as_str())
        && app.shell.workspace.startup.status != StartupStatus::Error
        && (app.shell.workspace.startup.status != StartupStatus::Loading
            || app.shell.workspace.startup.current_step >= 4)
        && let Some(services) = app.services.as_ref()
    {
        schedule_root_browser(services, context, repo_id);
    }
}

/// 提交一个服务任务，提交失败按 `what` 记日志。
fn run(context: &RuntimeProgramContext<ShellMessage>, what: &str, task: Task<ShellMessage>) {
    if let Err(error) = context.run_task(task) {
        eprintln!("Nana {what}任务提交失败：{error}");
    }
}

/// 播放集页移除条目。
fn dispatch_playlists(
    app: &MomoBakoApplication,
    message: &ShellMessage,
    context: &RuntimeProgramContext<ShellMessage>,
) {
    if let ShellMessage::RemovePlaylistItem { playlist_id, item_id } = message
        && let Some(repository_id) = app.shell.repository_id.clone()
        && let Some(services) = app.services.as_ref()
    {
        let interaction = services.repository_interaction.clone();
        let executor = services.executor.clone();
        let request = crate::backend::services::repository::PlaylistItemRemoveRequest {
            repo_id: repository_id,
            playlist_id: playlist_id.clone(),
            playlist_item_id: item_id.clone(),
        };
        run(context, "播放列表项目移除", Task::new(async move {
            ShellMessage::PlaylistDetailLoaded(
                executor.block_on(interaction.remove_playlist_item(request)),
            )
        }));
    }
}

/// 打开目录、选中文件读元数据。
fn dispatch_browse_and_preview(
    app: &MomoBakoApplication,
    message: &ShellMessage,
    context: &RuntimeProgramContext<ShellMessage>,
) {
    if let ShellMessage::OpenDirectory(path) = message
        && let Some(repo_id) = app.shell.repository_id.clone()
        && let Some(services) = app.services.as_ref()
    {
        let browser = services.file_browser.clone();
        let executor = services.executor.clone();
        let request = FileBrowserRequest {
            repo_id, directory_path: Some(path.clone()), include_tree: Some(false),
            special_location: None, offset: Some(0), limit: Some(200),
        };
        run(context, "目录加载", Task::new(async move {
            ShellMessage::FileBrowserLoaded(executor.block_on(browser.get_file_browser(request)))
        }));
    }
    if let ShellMessage::SelectFile {
        asset_id: Some(asset_id),
        ..
    } = message
        && let Some(repository_id) = app.shell.repository_id.clone()
        && let Some(services) = app.services.as_ref()
    {
        let query = services.repository_query.clone();
        let executor = services.executor.clone();
        let asset_id = asset_id.clone();
        run(context, "文件元数据", Task::new(async move {
            ShellMessage::AssetDetailLoaded(
                executor.block_on(query.get_asset_detail(repository_id, asset_id)),
            )
        }));
    }
}

/// 摘要确认属于当前仓库后，读取根目录作为启动第 4 步。
fn schedule_root_browser(
    services: &services::NativeServices,
    context: &RuntimeProgramContext<ShellMessage>,
    repo_id: String,
) {
    let browser = services.file_browser.clone();
    let executor = services.executor.clone();
    let request = FileBrowserRequest {
        repo_id,
        directory_path: None,
        include_tree: Some(false),
        special_location: None,
        offset: Some(0),
        limit: Some(200),
    };
    run(context, "文件浏览", Task::new(async move {
        ShellMessage::FileBrowserLoaded(executor.block_on(browser.get_file_browser(request)))
    }));
}
