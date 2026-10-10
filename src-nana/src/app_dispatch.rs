//! 按壳层消息派发领域服务任务。
//!
//! `MomoBakoApplication::update` 在归约之前调用 [`dispatch_services`]：播放集条目移除、
//! 打开目录和读文件元数据这些消息各自提交一个后台任务，结果再作为新消息回到 `update`。页面数据的读取不在这里，
//! 由归约排出副作用，再由各 `*_dispatch` 交给领域服务。

use nana_ui::runtime::Task;
use nana_ui::RuntimeProgramContext;

use crate::backend::services::repository::{FileBrowserRequest, RepositorySummary};
use crate::backend::viewmodels::RepositoryQueryViewModel;
use crate::shell::ShellMessage;
use crate::MomoBakoApplication;

/// 归约前提交这条消息需要的服务任务。服务没启动时不提交，任务提交失败只记日志。
pub(crate) fn dispatch_services(
    app: &MomoBakoApplication,
    message: &ShellMessage,
    context: &RuntimeProgramContext<ShellMessage>,
) {
    dispatch_playlists(app, message, context);
    dispatch_browse_and_preview(app, message, context);
}

/// 附加、新建或重定向资源库的协议只回 JSON：按 `repository.repoId` 认出这个仓库，再从资源库列表里取出
/// 它的摘要（领域的仓库摘要不能从 JSON 读回）。在服务任务里调用。
pub(crate) fn mutated_repository(
    executor: &tokio::runtime::Runtime,
    query: &RepositoryQueryViewModel,
    output: &serde_json::Value,
) -> Result<RepositorySummary, String> {
    let Some(repo_id) = output.get("repository").and_then(|repository| repository.get("repoId")).and_then(serde_json::Value::as_str)
    else {
        eprintln!("Nana 资源库操作的结果里没有仓库标识：{output}");
        return Err("资源库操作没有返回仓库标识".into());
    };
    let items = executor.block_on(query.list_repositories())?;
    items.into_iter().find(|item| item.repo_id == repo_id).ok_or_else(|| {
        eprintln!("Nana 资源库列表里没有刚处理的仓库：{repo_id}");
        format!("资源库列表里没有刚处理的仓库：{repo_id}")
    })
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

/// 启动和换仓库的第 4 步：读根目录首屏。服务没起来或提交失败时按失败写回，启动不会停在第 4 步。
pub(crate) fn dispatch_first_screen(
    app: &mut MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    repo_id: String,
) {
    let prepared = app.services.as_ref().map(|services| (services.file_browser.clone(), services.executor.clone()));
    let Some((browser, executor)) = prepared else {
        eprintln!("Nana 首屏目录需要领域服务，当前服务未启动");
        app.shell.reduce(ShellMessage::FileBrowserLoaded(Err("领域服务未启动".into())));
        return;
    };
    let request = FileBrowserRequest {
        repo_id,
        directory_path: None,
        include_tree: Some(false),
        special_location: None,
        offset: Some(0),
        limit: Some(200),
    };
    if let Err(error) = context.run_task(Task::new(async move {
        ShellMessage::FileBrowserLoaded(executor.block_on(browser.get_file_browser(request)))
    })) {
        eprintln!("Nana 首屏目录任务提交失败：{error}");
        app.shell.reduce(ShellMessage::FileBrowserLoaded(Err(format!("首屏目录任务提交失败：{error}"))));
    }
}
