//! 把侧栏副作用交给已有的仓库树、智能文件夹、播放集和附加协议。

use nana_ui::runtime::Task;
use nana_ui::RuntimeProgramContext;

use crate::backend::services::mutsuki_runner::PROTOCOL_REPOSITORY_ATTACH;
use crate::backend::services::repository::{FileBrowserRequest, RecentAccessHistoryClearRequest, RepositoryFolderRequest};
use crate::shell::{
    FileRow, ShellMessage, SidebarEffect, SidebarMessage, SidebarSmartFolder, SidebarTree,
    VirtualQuery,
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
            SidebarEffect::CreateSmartFolder { repo_id } => dispatch_create_smart(app, context, repo_id),
            SidebarEffect::UpdateSmartFolder { repo_id } => dispatch_update_smart(app, context, repo_id),
            SidebarEffect::DeleteSmartFolder { repo_id, smart_folder_id } => {
                dispatch_delete_smart(app, context, repo_id, smart_folder_id);
            }
            SidebarEffect::DeletePlaylist { repo_id, playlist_id } => dispatch_delete_playlist(app, context, repo_id, playlist_id),
            SidebarEffect::CreateBackendRepository { name, path, plugin_id, config } => {
                dispatch_create_backend(app, context, name, path, plugin_id, config);
            }
            SidebarEffect::LoadPlaylists { repo_id } => dispatch_playlists(app, context, repo_id),
            SidebarEffect::LoadPlaylistDetail { repo_id, playlist_id } => {
                dispatch_playlist_detail(app, context, repo_id, playlist_id);
            }
            SidebarEffect::Browse { repo_id, path, trash } => {
                app.shell.note_sidebar_browse(&path);
                dispatch_browse_request(app, context, browse_request(repo_id, path, trash));
            }
            SidebarEffect::AttachRepository { path } => dispatch_attach(app, context, path),
            SidebarEffect::ClearRecent { repo_id } => dispatch_clear_recent(app, context, repo_id),
        }
    }
}

fn services(app: &MomoBakoApplication) -> Option<&crate::services::NativeServices> {
    app.services.as_ref()
}

/// 清空最近使用。服务没起来时把错误写回侧栏，不假装已经清空。
fn dispatch_clear_recent(app: &mut MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>, repo_id: String) {
    let Some(services) = services(app) else {
        eprintln!("Nana 清空最近使用需要领域服务，当前服务未启动");
        app.shell.reduce(sidebar_message(SidebarMessage::RecentCleared {
            repo_id,
            result: Err("领域服务未启动".into()),
        }));
        return;
    };
    let interaction = services.repository_interaction.clone();
    let executor = services.executor.clone();
    let task_repo = repo_id.clone();
    if let Err(error) = context.run_task(Task::new(async move {
        let result = executor
            .block_on(interaction.clear_recent_access_history(RecentAccessHistoryClearRequest { repo_id: task_repo.clone() }))
            .map(|response| response.cleared_count);
        sidebar_message(SidebarMessage::RecentCleared { repo_id: task_repo, result })
    })) {
        eprintln!("Nana 清空最近使用任务提交失败：{error}");
        app.shell.reduce(sidebar_message(SidebarMessage::RecentCleared {
            repo_id,
            result: Err(format!("清空最近使用任务提交失败：{error}")),
        }));
    }
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
        let result = executor
            .block_on(browser.get_repository_tree(task_repo.clone()))
            .map(|snapshot| SidebarTree::from_nodes(&snapshot.tree));
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

fn dispatch_create_smart(app: &mut MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>, repo_id: String) {
    let Some(services) = services(app) else {
        eprintln!("Nana 新建智能文件夹需要领域服务，当前服务未启动");
        app.shell.reduce(sidebar_message(SidebarMessage::SmartFolderSaved {
            repo_id,
            result: Err("领域服务未启动".into()),
        }));
        return;
    };
    let request = app.shell.sidebar.smart_create_request(&repo_id);
    let interaction = services.repository_interaction.clone();
    let executor = services.executor.clone();
    let task_repo = repo_id.clone();
    if let Err(error) = context.run_task(Task::new(async move {
        let result = executor.block_on(interaction.create_smart_folder(request)).map(|response| {
            response.smart_folders.iter().map(SidebarSmartFolder::from_tree_node).collect()
        });
        sidebar_message(SidebarMessage::SmartFolderSaved { repo_id: task_repo, result })
    })) {
        eprintln!("Nana 新建智能文件夹任务提交失败：{error}");
        app.shell.reduce(sidebar_message(SidebarMessage::SmartFolderSaved {
            repo_id,
            result: Err(format!("智能文件夹任务提交失败：{error}")),
        }));
    }
}

fn dispatch_update_smart(app: &mut MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>, repo_id: String) {
    let Some(services) = services(app) else {
        eprintln!("Nana 编辑智能文件夹需要领域服务，当前服务未启动");
        app.shell.reduce(sidebar_message(SidebarMessage::SmartFolderSaved {
            repo_id,
            result: Err("领域服务未启动".into()),
        }));
        return;
    };
    let request = app.shell.sidebar.smart_update_request(&repo_id);
    let interaction = services.repository_interaction.clone();
    let executor = services.executor.clone();
    let task_repo = repo_id.clone();
    if let Err(error) = context.run_task(Task::new(async move {
        let result = executor.block_on(interaction.update_smart_folder(request)).map(|response| {
            response.smart_folders.iter().map(SidebarSmartFolder::from_tree_node).collect()
        });
        sidebar_message(SidebarMessage::SmartFolderSaved { repo_id: task_repo, result })
    })) {
        eprintln!("Nana 编辑智能文件夹任务提交失败：{error}");
        app.shell.reduce(sidebar_message(SidebarMessage::SmartFolderSaved {
            repo_id,
            result: Err(format!("智能文件夹任务提交失败：{error}")),
        }));
    }
}

fn dispatch_delete_smart(
    app: &mut MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    repo_id: String,
    smart_folder_id: String,
) {
    let Some(services) = services(app) else {
        eprintln!("Nana 删除智能文件夹需要领域服务，当前服务未启动");
        app.shell.reduce(sidebar_message(SidebarMessage::SmartFolderSaved {
            repo_id,
            result: Err("领域服务未启动".into()),
        }));
        return;
    };
    let interaction = services.repository_interaction.clone();
    let executor = services.executor.clone();
    let task_repo = repo_id.clone();
    if let Err(error) = context.run_task(Task::new(async move {
        let result = executor.block_on(interaction.delete_smart_folder(task_repo.clone(), smart_folder_id)).map(|response| {
            response.smart_folders.iter().map(SidebarSmartFolder::from_tree_node).collect()
        });
        sidebar_message(SidebarMessage::SmartFolderSaved { repo_id: task_repo, result })
    })) {
        eprintln!("Nana 删除智能文件夹任务提交失败：{error}");
        app.shell.reduce(sidebar_message(SidebarMessage::SmartFolderSaved {
            repo_id,
            result: Err(format!("智能文件夹任务提交失败：{error}")),
        }));
    }
}

fn dispatch_delete_playlist(
    app: &mut MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    repo_id: String,
    playlist_id: String,
) {
    let Some(services) = services(app) else {
        eprintln!("Nana 移除播放集需要领域服务，当前服务未启动");
        app.shell.reduce(ShellMessage::PlaylistsLoaded { repo_id, result: Err("领域服务未启动".into()), open: None });
        return;
    };
    let interaction = services.repository_interaction.clone();
    let executor = services.executor.clone();
    let task_repo = repo_id.clone();
    if let Err(error) = context.run_task(Task::new(async move {
        let result = executor.block_on(interaction.delete_playlist(task_repo.clone(), playlist_id)).map(|response| response.playlists);
        ShellMessage::PlaylistsLoaded { repo_id: task_repo, result, open: None }
    })) {
        eprintln!("Nana 移除播放集任务提交失败：{error}");
        app.shell.reduce(ShellMessage::PlaylistsLoaded { repo_id, result: Err(format!("移除播放集任务提交失败：{error}")), open: None });
    }
}

fn dispatch_create_backend(
    app: &mut MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    name: String,
    path: String,
    plugin_id: String,
    config: Option<serde_json::Value>,
) {
    let Some(services) = services(app) else {
        eprintln!("Nana 创建资源库需要领域服务，当前服务未启动");
        app.shell.reduce(sidebar_message(SidebarMessage::RepositoryAttachFinished(Err("领域服务未启动".into()))));
        return;
    };
    let tasks = services.tasks.clone();
    let executor = services.executor.clone();
    let request = crate::backend::services::repository::RepositoryMutationRequest {
        repo_id: None,
        name,
        path,
        backend_plugin_id: Some(plugin_id),
        backend_config: config,
        skip_initial_sync: false,
    };
    if let Err(error) = context.run_task(Task::new(async move {
        let result = executor
            .block_on(tasks.execute(crate::backend::services::mutsuki_runner::PROTOCOL_REPOSITORY_CREATE, request))
            .map(|_| ());
        sidebar_message(SidebarMessage::RepositoryAttachFinished(result))
    })) {
        eprintln!("Nana 创建资源库任务提交失败：{error}");
        app.shell.reduce(sidebar_message(SidebarMessage::RepositoryAttachFinished(Err(format!("创建资源库任务提交失败：{error}")))));
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
            .map(|snapshot| VirtualQuery {
                count: snapshot.results.len(),
                rows: snapshot.results.iter().map(FileRow::from_entry).collect(),
            });
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
        let result = executor.block_on(interaction.list_playlists(task_repo.clone()));
        sidebar_message(SidebarMessage::SidebarPlaylistsLoaded { repo_id: task_repo, result })
    })) {
        eprintln!("Nana 播放集列表任务提交失败：{error}");
        app.shell.reduce(sidebar_message(SidebarMessage::SidebarPlaylistsLoaded {
            repo_id,
            result: Err(format!("播放集列表任务提交失败：{error}")),
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

/// 交出 `prepare` 暂存的目录浏览，并写成和领域服务相同的请求。
pub(crate) fn dispatch_prepared_browses(shell: &mut crate::shell::ShellViewModel) -> Vec<FileBrowserRequest> {
    let staged = std::mem::take(&mut shell.staged_browses);
    staged
        .into_iter()
        .filter_map(|effect| match effect {
            SidebarEffect::Browse { repo_id, path, trash } => Some(browse_request(repo_id, path, trash)),
            _ => None,
        })
        .collect()
}

fn browse_request(repo_id: String, path: String, trash: bool) -> FileBrowserRequest {
    FileBrowserRequest {
        repo_id,
        directory_path: if trash || path.is_empty() { None } else { Some(path) },
        include_tree: Some(false),
        special_location: if trash { Some("trash".into()) } else { None },
        offset: Some(0),
        limit: Some(200),
    }
}

pub(crate) fn dispatch_browse_request(
    app: &mut MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    request: FileBrowserRequest,
) {
    let Some(services) = services(app) else {
        eprintln!("Nana 侧栏目录浏览需要领域服务，当前服务未启动");
        app.shell.reduce(ShellMessage::FileBrowserLoaded(Err("领域服务未启动".into())));
        return;
    };
    let browser = services.file_browser.clone();
    let executor = services.executor.clone();
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
