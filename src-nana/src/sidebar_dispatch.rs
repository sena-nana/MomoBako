//! 把侧栏副作用交给已有的仓库树、智能文件夹、播放集和附加协议。

use nana_ui::runtime::Task;
use nana_ui::RuntimeProgramContext;

use crate::backend::services::mutsuki_runner::PROTOCOL_REPOSITORY_ATTACH;
use crate::backend::services::repository::{FileBrowserRequest, RepositoryFolderRequest};
use crate::shell::{
    ShellMessage, SidebarEffect, SidebarFolder, SidebarMessage, SidebarPlaylist, SidebarSmartFolder,
};
use crate::MomoBakoApplication;

/// 执行侧栏归约留下的请求。提交失败时把错误写回侧栏，避免刷新一直停在进行中。
pub fn dispatch_sidebar_effects(app: &mut MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>) {
    for effect in app.shell.sidebar.take_effects() {
        match effect {
            SidebarEffect::LoadTree { repo_id } => dispatch_tree(app, context, repo_id),
            SidebarEffect::LoadSmartFolders { repo_id } => dispatch_smart_folders(app, context, repo_id),
            SidebarEffect::QuerySmartFolder { repo_id, smart_folder_id } => {
                dispatch_smart_query(app, context, repo_id, smart_folder_id);
            }
            SidebarEffect::LoadPlaylists { repo_id } => dispatch_playlists(app, context, repo_id),
            SidebarEffect::LoadPlaylistPlayers { repo_id } => dispatch_players(app, context, repo_id),
            SidebarEffect::LoadPlaylistDetail { repo_id, playlist_id } => {
                dispatch_playlist_detail(app, context, repo_id, playlist_id);
            }
            SidebarEffect::Browse { repo_id, path, trash } => dispatch_browse(app, context, repo_id, path, trash),
            SidebarEffect::AttachRepository { path } => dispatch_attach(app, context, path),
        }
    }
}

fn services(app: &MomoBakoApplication) -> Option<&crate::services::NativeServices> {
    app.services.as_ref()
}

fn dispatch_tree(app: &mut MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>, repo_id: String) {
    let Some(services) = services(app) else {
        eprintln!("Nana 目录树需要领域服务，当前服务未启动");
        app.shell.reduce(sidebar_message(SidebarMessage::SidebarTreeLoaded {
            repo_id,
            result: Err("领域服务未启动".into()),
        }));
        return;
    };
    let browser = services.file_browser.clone();
    let executor = services.executor.clone();
    let task_repo = repo_id.clone();
    if let Err(error) = context.run_task(Task::new(async move {
        let result = executor.block_on(browser.get_repository_tree(task_repo.clone())).map(|snapshot| {
            snapshot.tree.iter().map(SidebarFolder::from_file_node).collect()
        });
        sidebar_message(SidebarMessage::SidebarTreeLoaded { repo_id: task_repo, result })
    })) {
        eprintln!("Nana 目录树任务提交失败：{error}");
        app.shell.reduce(sidebar_message(SidebarMessage::SidebarTreeLoaded {
            repo_id,
            result: Err(format!("目录树任务提交失败：{error}")),
        }));
    }
}

fn dispatch_smart_folders(app: &mut MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>, repo_id: String) {
    let Some(services) = services(app) else {
        eprintln!("Nana 智能文件夹需要领域服务，当前服务未启动");
        app.shell.reduce(sidebar_message(SidebarMessage::SidebarSmartFoldersLoaded {
            repo_id,
            result: Err("领域服务未启动".into()),
        }));
        return;
    };
    let interaction = services.repository_interaction.clone();
    let executor = services.executor.clone();
    let task_repo = repo_id.clone();
    if let Err(error) = context.run_task(Task::new(async move {
        let result = executor.block_on(interaction.list_smart_folders(task_repo.clone())).map(|folders| {
            folders.iter().map(SidebarSmartFolder::from_tree_node).collect()
        });
        sidebar_message(SidebarMessage::SidebarSmartFoldersLoaded { repo_id: task_repo, result })
    })) {
        eprintln!("Nana 智能文件夹任务提交失败：{error}");
        app.shell.reduce(sidebar_message(SidebarMessage::SidebarSmartFoldersLoaded {
            repo_id,
            result: Err(format!("智能文件夹任务提交失败：{error}")),
        }));
    }
}

fn dispatch_smart_query(
    app: &mut MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    repo_id: String,
    smart_folder_id: String,
) {
    let Some(services) = services(app) else {
        eprintln!("Nana 智能文件夹查询需要领域服务，当前服务未启动");
        app.shell.reduce(sidebar_message(SidebarMessage::SidebarSmartFolderQueried {
            repo_id,
            smart_folder_id,
            result: Err("领域服务未启动".into()),
        }));
        return;
    };
    let interaction = services.repository_interaction.clone();
    let executor = services.executor.clone();
    let task_repo = repo_id.clone();
    let task_id = smart_folder_id.clone();
    if let Err(error) = context.run_task(Task::new(async move {
        let result = executor
            .block_on(interaction.query_smart_folder(task_repo.clone(), task_id.clone()))
            .map(|snapshot| snapshot.results.len());
        sidebar_message(SidebarMessage::SidebarSmartFolderQueried { repo_id: task_repo, smart_folder_id: task_id, result })
    })) {
        eprintln!("Nana 智能文件夹查询任务提交失败：{error}");
        app.shell.reduce(sidebar_message(SidebarMessage::SidebarSmartFolderQueried {
            repo_id,
            smart_folder_id,
            result: Err(format!("智能文件夹查询任务提交失败：{error}")),
        }));
    }
}

fn dispatch_playlists(app: &mut MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>, repo_id: String) {
    let Some(services) = services(app) else {
        eprintln!("Nana 播放集列表需要领域服务，当前服务未启动");
        app.shell.reduce(sidebar_message(SidebarMessage::SidebarPlaylistsLoaded {
            repo_id,
            result: Err("领域服务未启动".into()),
        }));
        return;
    };
    let interaction = services.repository_interaction.clone();
    let executor = services.executor.clone();
    let task_repo = repo_id.clone();
    if let Err(error) = context.run_task(Task::new(async move {
        let result = executor.block_on(interaction.list_playlists(task_repo.clone())).map(|playlists| {
            playlists.iter().map(SidebarPlaylist::from_summary).collect()
        });
        sidebar_message(SidebarMessage::SidebarPlaylistsLoaded { repo_id: task_repo, result })
    })) {
        eprintln!("Nana 播放集列表任务提交失败：{error}");
        app.shell.reduce(sidebar_message(SidebarMessage::SidebarPlaylistsLoaded {
            repo_id,
            result: Err(format!("播放集列表任务提交失败：{error}")),
        }));
    }
}

fn dispatch_players(app: &mut MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>, repo_id: String) {
    let Some(services) = services(app) else {
        eprintln!("Nana 播放器类型需要领域服务，当前服务未启动");
        app.shell.reduce(sidebar_message(SidebarMessage::SidebarPlaylistPlayersLoaded {
            repo_id,
            result: Err("领域服务未启动".into()),
        }));
        return;
    };
    let plugin = services.plugin.clone();
    let executor = services.executor.clone();
    let task_repo = repo_id.clone();
    if let Err(error) = context.run_task(Task::new(async move {
        let result = executor.block_on(plugin.list_playlist_players()).map(|players| {
            players.into_iter().map(|player| player.player_type_id).collect()
        });
        sidebar_message(SidebarMessage::SidebarPlaylistPlayersLoaded { repo_id: task_repo, result })
    })) {
        eprintln!("Nana 播放器类型任务提交失败：{error}");
        app.shell.reduce(sidebar_message(SidebarMessage::SidebarPlaylistPlayersLoaded {
            repo_id,
            result: Err(format!("播放器类型任务提交失败：{error}")),
        }));
    }
}

fn dispatch_playlist_detail(
    app: &mut MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    repo_id: String,
    playlist_id: String,
) {
    let Some(services) = services(app) else {
        eprintln!("Nana 播放集详情需要领域服务，当前服务未启动");
        app.shell.reduce(ShellMessage::PlaylistDetailLoaded(Err("领域服务未启动".into())));
        return;
    };
    let interaction = services.repository_interaction.clone();
    let executor = services.executor.clone();
    if let Err(error) = context.run_task(Task::new(async move {
        ShellMessage::PlaylistDetailLoaded(executor.block_on(interaction.get_playlist_detail(repo_id, playlist_id)))
    })) {
        eprintln!("Nana 播放集详情任务提交失败：{error}");
        app.shell.reduce(ShellMessage::PlaylistDetailLoaded(Err(format!("播放集详情任务提交失败：{error}"))));
    }
}

fn dispatch_browse(
    app: &mut MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    repo_id: String,
    path: String,
    trash: bool,
) {
    let Some(services) = services(app) else {
        eprintln!("Nana 侧栏目录浏览需要领域服务，当前服务未启动");
        app.shell.reduce(ShellMessage::FileBrowserLoaded(Err("领域服务未启动".into())));
        return;
    };
    let browser = services.file_browser.clone();
    let executor = services.executor.clone();
    let request = FileBrowserRequest {
        repo_id,
        directory_path: if trash || path.is_empty() { None } else { Some(path) },
        include_tree: Some(false),
        special_location: if trash { Some("trash".into()) } else { None },
        offset: Some(0),
        limit: Some(200),
    };
    if let Err(error) = context.run_task(Task::new(async move {
        ShellMessage::FileBrowserLoaded(executor.block_on(browser.get_file_browser(request)))
    })) {
        eprintln!("Nana 侧栏目录浏览任务提交失败：{error}");
        app.shell.reduce(ShellMessage::FileBrowserLoaded(Err(format!("目录浏览任务提交失败：{error}"))));
    }
}

fn dispatch_attach(app: &mut MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>, path: String) {
    let Some(services) = services(app) else {
        eprintln!("Nana 附加资源库需要领域服务，当前服务未启动");
        app.shell.reduce(sidebar_message(SidebarMessage::RepositoryAttachFinished(Err("领域服务未启动".into()))));
        return;
    };
    let tasks = services.tasks.clone();
    let executor = services.executor.clone();
    if let Err(error) = context.run_task(Task::new(async move {
        let result = executor
            .block_on(tasks.execute(PROTOCOL_REPOSITORY_ATTACH, RepositoryFolderRequest { path }))
            .map(|_| ());
        sidebar_message(SidebarMessage::RepositoryAttachFinished(result))
    })) {
        eprintln!("Nana 附加资源库任务提交失败：{error}");
        app.shell.reduce(sidebar_message(SidebarMessage::RepositoryAttachFinished(Err(format!("附加资源库任务提交失败：{error}")))));
    }
}

fn sidebar_message(message: SidebarMessage) -> ShellMessage {
    ShellMessage::Sidebar(message)
}
