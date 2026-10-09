//! 按壳层消息派发领域服务任务。
//!
//! `MomoBakoApplication::update` 在归约之前调用 [`dispatch_services`]：导航、设置、插件、播放列表、
//! 日志、目录和预览这些消息各自提交一个后台任务，结果再作为新消息回到 `update`。少数消息要改道
//! （上移播放列表条目改成一次排序）或就地拦下（插件配置不是合法 JSON），由 [`Route`] 告诉调用方。

use nana_ui::runtime::Task;
use nana_ui::RuntimeProgramContext;

use crate::backend::services::repository::FileBrowserRequest;
use crate::shell::{ShellMessage, ShellPage, StartupStatus};
use crate::{decode_preview_pixels, services, MomoBakoApplication};

/// 派发之后这条消息怎么继续。
pub(crate) enum Route {
    /// 照常归约这条消息。
    Reduce,
    /// 这条消息已经换成另一条，调用方改为处理它，原消息不再归约。
    Replace(ShellMessage),
    /// 这条消息到此为止：不归约，只重绘。
    Stop,
}

/// 归约前提交这条消息需要的服务任务。服务没启动时不提交，任务提交失败只记日志。
pub(crate) fn dispatch_services(
    app: &mut MomoBakoApplication,
    message: &ShellMessage,
    context: &RuntimeProgramContext<ShellMessage>,
) -> Route {
    dispatch_admin_pages(app, message, context);
    dispatch_plugins(app, message, context);
    if let Some(route) = dispatch_playlists(app, message, context) {
        return route;
    }
    if let Some(route) = dispatch_plugin_config(app, message, context) {
        return route;
    }
    if let ShellMessage::CancelTask(task_id) = message
        && let Some(services) = app.services.as_ref()
        && !services.tasks.cancel(task_id)
    {
        eprintln!("Nana 任务取消请求未找到任务：{task_id}");
    }
    dispatch_browse_and_preview(app, message, context);
    Route::Reduce
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

/// 导航到插件、日志、播放列表、任务和设置页时读取页面数据；保存设置、清理日志也在这里。
fn dispatch_admin_pages(
    app: &mut MomoBakoApplication,
    message: &ShellMessage,
    context: &RuntimeProgramContext<ShellMessage>,
) {
    if matches!(message, ShellMessage::Navigate(ShellPage::PluginSettings))
        && let Some(services) = app.services.as_ref()
    {
        let plugin = services.plugin.clone();
        let executor = services.executor.clone();
        run(context, "插件列表", Task::new(async move {
            ShellMessage::PluginsLoaded(executor.block_on(plugin.list_plugins()))
        }));
    }
    if matches!(message, ShellMessage::Navigate(ShellPage::Logs))
        && let Some(services) = app.services.as_ref()
    {
        let system = services.system.clone();
        let executor = services.executor.clone();
        let query = crate::backend::services::repository::SystemLogQuery {
            limit: Some(100),
            ..Default::default()
        };
        run(context, "系统日志", Task::new(async move {
            ShellMessage::LogsLoaded(executor.block_on(system.list_system_logs(Some(query))))
        }));
    }
    if matches!(message, ShellMessage::Navigate(ShellPage::Playlists))
        && let Some(repository_id) = app.shell.repository_id.clone()
        && let Some(services) = app.services.as_ref()
    {
        let interaction = services.repository_interaction.clone();
        let plugin = services.plugin.clone();
        let executor = services.executor.clone();
        let players_executor = executor.clone();
        run(context, "播放列表", Task::new(async move {
            ShellMessage::PlaylistsLoaded(executor.block_on(interaction.list_playlists(repository_id)))
        }));
        run(context, "播放器类型", Task::new(async move {
            ShellMessage::PlaylistPlayersLoaded(players_executor.block_on(plugin.list_playlist_players()))
        }));
    }
    if matches!(message, ShellMessage::Navigate(ShellPage::TaskRunning))
        && let Some(services) = app.services.as_ref()
    {
        let (active, completed) = services.tasks.activity_snapshot();
        app.shell.active_task_ids = services.tasks.active_task_ids();
        app.shell.reduce(ShellMessage::TaskSnapshotLoaded { active, completed });
        app.shell.reduce(ShellMessage::TaskProgressLoaded(services.tasks.progress_snapshots()));
    }
    if matches!(message, ShellMessage::Navigate(ShellPage::Settings))
        && let Some(services) = app.services.as_ref()
    {
        schedule_settings_load(services, context);
    }
    if matches!(message, ShellMessage::SaveSettings)
        && let Some(services) = app.services.as_ref()
    {
        let settings = app.shell.settings.clone();
        let settings_service = services.settings.clone();
        let executor = services.executor.clone();
        run(context, "应用设置保存", Task::new(async move {
            ShellMessage::SettingsSaved(executor.block_on(async move {
                settings.validate()?;
                settings_service.save(&settings).map(|()| settings)
            }))
        }));
    }
    if matches!(message, ShellMessage::ClearLogs)
        && let Some(services) = app.services.as_ref()
    {
        let system = services.system.clone();
        let executor = services.executor.clone();
        let query = crate::backend::services::repository::SystemLogQuery {
            limit: Some(100),
            ..Default::default()
        };
        run(context, "系统日志清理", Task::new(async move {
            let result = executor.block_on(async {
                system.clear_system_logs().await?;
                system.list_system_logs(Some(query)).await
            });
            ShellMessage::LogsLoaded(result)
        }));
    }
}

/// 选择、启停和删除插件。
fn dispatch_plugins(
    app: &MomoBakoApplication,
    message: &ShellMessage,
    context: &RuntimeProgramContext<ShellMessage>,
) {
    let Some(services) = app.services.as_ref() else {
        return;
    };
    match message {
        ShellMessage::SelectPlugin(plugin_id) => {
            let plugin = services.plugin.clone();
            let executor = services.executor.clone();
            let plugin_id = plugin_id.clone();
            run(context, "插件设置", Task::new(async move {
                ShellMessage::PluginConfigLoaded(executor.block_on(plugin.get_plugin_config(plugin_id)))
            }));
        }
        ShellMessage::TogglePlugin { plugin_id, enabled } => {
            let plugin = services.plugin.clone();
            let executor = services.executor.clone();
            let request = crate::backend::services::repository::PluginEnabledRequest {
                plugin_id: plugin_id.clone(),
                enabled: *enabled,
            };
            run(context, "插件启停", Task::new(async move {
                ShellMessage::PluginsLoaded(
                    executor.block_on(plugin.set_plugin_enabled(request)).map(|response| response.plugins),
                )
            }));
        }
        ShellMessage::DeletePlugin(plugin_id) => {
            let plugin = services.plugin.clone();
            let executor = services.executor.clone();
            let plugin_id = plugin_id.clone();
            run(context, "插件删除", Task::new(async move {
                ShellMessage::PluginsLoaded(
                    executor.block_on(plugin.delete_plugin(plugin_id)).map(|response| response.plugins),
                )
            }));
        }
        _ => {}
    }
}

/// 播放列表的选择、增删、排序、改名和新建。上移下移改成一次排序消息再走一遍 `update`。
fn dispatch_playlists(
    app: &mut MomoBakoApplication,
    message: &ShellMessage,
    context: &RuntimeProgramContext<ShellMessage>,
) -> Option<Route> {
    if let ShellMessage::SelectPlaylist(playlist_id) = message
        && let Some(repository_id) = app.shell.repository_id.clone()
        && let Some(services) = app.services.as_ref()
    {
        let interaction = services.repository_interaction.clone();
        let executor = services.executor.clone();
        let playlist_id = playlist_id.clone();
        run(context, "播放列表详情", Task::new(async move {
            ShellMessage::PlaylistDetailLoaded(executor.block_on(
                interaction.get_playlist_detail(repository_id, playlist_id),
            ))
        }));
    }
    if let ShellMessage::DeletePlaylist(playlist_id) = message
        && let Some(repository_id) = app.shell.repository_id.clone()
        && let Some(services) = app.services.as_ref()
    {
        let interaction = services.repository_interaction.clone();
        let executor = services.executor.clone();
        let playlist_id = playlist_id.clone();
        run(context, "播放列表删除", Task::new(async move {
            let result = executor.block_on(async {
                interaction
                    .delete_playlist(repository_id.clone(), playlist_id)
                    .await?;
                interaction.list_playlists(repository_id).await
            });
            ShellMessage::PlaylistsLoaded(result)
        }));
    }
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
    if let ShellMessage::ReorderPlaylistItems { playlist_id, item_ids } = message
        && let Some(repository_id) = app.shell.repository_id.clone()
        && let Some(services) = app.services.as_ref()
    {
        let interaction = services.repository_interaction.clone();
        let executor = services.executor.clone();
        let request = crate::backend::services::repository::PlaylistItemsOrderRequest {
            repo_id: repository_id,
            playlist_id: playlist_id.clone(),
            item_ids: item_ids.clone(),
        };
        run(context, "播放列表排序", Task::new(async move {
            ShellMessage::PlaylistDetailLoaded(
                executor.block_on(interaction.reorder_playlist_items(request)),
            )
        }));
    }
    if let ShellMessage::MovePlaylistItem { item_id, direction } = message
        && let Some(playlist_id) = app.shell.selected_playlist_id.clone()
    {
        let mut item_ids = app.shell.playlist_item_ids.clone();
        if let Some(index) = item_ids.iter().position(|id| id == item_id) {
            let target = if *direction < 0 { index.checked_sub(1) } else { (index + 1 < item_ids.len()).then_some(index + 1) };
            if let Some(target) = target {
                item_ids.swap(index, target);
                app.shell.reduce(ShellMessage::ReorderPlaylistItems { playlist_id: playlist_id.clone(), item_ids: item_ids.clone() });
                // 排序请求走正常的服务派发。
                return Some(Route::Replace(ShellMessage::ReorderPlaylistItems { playlist_id, item_ids }));
            }
        }
    }
    if let ShellMessage::AddPlaylistItemsByPaths { playlist_id, paths } = message
        && let Some(repository_id) = app.shell.repository_id.clone()
        && let Some(services) = app.services.as_ref()
    {
        let interaction = services.repository_interaction.clone();
        let executor = services.executor.clone();
        let request = crate::backend::services::repository::PlaylistItemsByPathsAddRequest {
            repo_id: repository_id,
            playlist_id: playlist_id.clone(),
            paths: paths.clone(),
        };
        run(context, "播放列表添加", Task::new(async move {
            ShellMessage::PlaylistDetailLoaded(
                executor.block_on(interaction.add_playlist_items_by_paths(request)),
            )
        }));
    }
    if matches!(message, ShellMessage::SavePlaylistName)
        && let (Some(repository_id), Some(playlist_id), Some(player_type_id), Some(services)) = (
            app.shell.repository_id.clone(),
            app.shell.selected_playlist_id.clone(),
            app.shell.selected_playlist_player_type_id.clone(),
            app.services.as_ref(),
        )
    {
        let interaction = services.repository_interaction.clone();
        let executor = services.executor.clone();
        let name = app.shell.playlist_name_draft.trim().to_string();
        let request = crate::backend::services::repository::PlaylistUpdateRequest {
            repo_id: repository_id,
            playlist_id,
            name: Some(name),
            player_type_id: Some(player_type_id),
        };
        run(context, "播放列表名称保存", Task::new(async move {
            ShellMessage::PlaylistsLoaded(
                executor
                    .block_on(interaction.update_playlist(request))
                    .map(|response| response.playlists),
            )
        }));
    }
    if matches!(message, ShellMessage::CreatePlaylist)
        && let Some(repository_id) = app.shell.repository_id.clone()
        && let Some(player_type_id) = app.shell.selected_new_playlist_player_type_id.clone()
        && !app.shell.new_playlist_name.trim().is_empty()
        && let Some(services) = app.services.as_ref()
    {
        let interaction = services.repository_interaction.clone();
        let executor = services.executor.clone();
        let request = crate::backend::services::repository::PlaylistMutationRequest {
            repo_id: repository_id,
            playlist_id: None,
            name: app.shell.new_playlist_name.trim().to_string(),
            player_type_id,
        };
        run(context, "播放列表创建", Task::new(async move {
            ShellMessage::PlaylistsLoaded(
                executor
                    .block_on(interaction.create_playlist(request))
                    .map(|response| response.playlists),
            )
        }));
    }
    None
}

/// 删除和保存插件配置项。保存的值不是字符串配置时按 JSON 解析，解析失败写回说明并停下。
fn dispatch_plugin_config(
    app: &mut MomoBakoApplication,
    message: &ShellMessage,
    context: &RuntimeProgramContext<ShellMessage>,
) -> Option<Route> {
    if let ShellMessage::DeletePluginConfig { plugin_id, key } = message
        && let Some(services) = app.services.as_ref()
    {
        let plugin = services.plugin.clone();
        let executor = services.executor.clone();
        let request = crate::backend::services::repository::PluginConfigDeleteRequest {
            plugin_id: plugin_id.clone(),
            key: key.clone(),
        };
        run(context, "插件设置删除", Task::new(async move {
            ShellMessage::PluginConfigLoaded(
                executor.block_on(plugin.delete_plugin_config_value(request)),
            )
        }));
    }
    if let ShellMessage::SavePluginConfig { plugin_id, key } = message
        && let Some(services) = app.services.as_ref()
    {
        let plugin = services.plugin.clone();
        let executor = services.executor.clone();
        let value = app
            .shell
            .plugin_config_drafts
            .get(key)
            .cloned()
            .unwrap_or_default();
        let value = if app.shell.plugin_config_string_values.contains(key) {
            serde_json::Value::String(value)
        } else {
            match serde_json::from_str(&value) {
                Ok(value) => value,
                Err(error) => {
                    eprintln!("Nana 插件配置 JSON 解析失败 {key}：{error}");
                    app.shell.detail = format!("配置 {key} 不是有效 JSON：{error}");
                    return Some(Route::Stop);
                }
            }
        };
        let request = crate::backend::services::repository::PluginConfigSetRequest {
            plugin_id: plugin_id.clone(),
            key: key.clone(),
            value,
        };
        run(context, "插件设置保存", Task::new(async move {
            ShellMessage::PluginConfigLoaded(
                executor.block_on(plugin.set_plugin_config_value(request)),
            )
        }));
    }
    None
}

/// 打开目录、选中文件读元数据，以及验收场景的预览源和原生图片解码。
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
    if matches!(message, ShellMessage::PrimaryAction)
        && app.shell.acceptance_scene
        && app.shell.page == ShellPage::SelectedFile
        && let (Some(repository_id), Some(path), Some(services)) = (
            app.shell.repository_id.clone(),
            app.shell.selected_path.clone(),
            app.services.as_ref(),
        )
    {
        let query = services.repository_query.clone();
        let executor = services.executor.clone();
        let request = crate::backend::services::repository::FileReadRequest { repo_id: repository_id, path };
        run(context, "预览源", Task::new(async move {
            ShellMessage::PreviewSourceLoaded(
                executor.block_on(query.prepare_preview_file_source(request)),
            )
        }));
    }
    if let ShellMessage::PreviewSourceLoaded(Ok(source)) = message
        && let Some(services) = app.services.as_ref()
    {
        let query = services.repository_query.clone();
        let executor = services.executor.clone();
        let source = crate::backend::services::repository::FilePreviewSourceResponse {
            repo_id: source.repo_id.clone(),
            path: source.path.clone(),
            token: source.token.clone(),
            source_url: source.source_url.clone(),
            local_path: source.local_path.clone(),
            media_type: source.media_type.clone(),
            size_bytes: source.size_bytes,
            modified_at: source.modified_at.clone(),
        };
        let request = crate::backend::services::repository::FileReadRequest {
            repo_id: source.repo_id.clone(),
            path: source.path.clone(),
        };
        run(context, "原生图片预览", Task::new(async move {
            let pixels = executor.block_on(query.read_file(request)).and_then(|bytes| {
                if !source.media_type.starts_with("image/") {
                    return Err(format!("原生纹理预览暂不支持 {}", source.media_type));
                }
                decode_preview_pixels(&bytes)
            });
            ShellMessage::PreviewPixelsLoaded { source, pixels }
        }));
    }
}

/// 读取系统连接状态和应用设置。设置页和来源缓存问题共用这一次调度。
pub(crate) fn schedule_settings_load(
    services: &services::NativeServices,
    context: &RuntimeProgramContext<ShellMessage>,
) {
    let system = services.system.clone();
    let settings = services.settings.clone();
    let executor = services.executor.clone();
    run(context, "系统状态", Task::new(async move {
        ShellMessage::SystemStatusLoaded(
            executor.block_on(system.get_external_api_connection_status()),
        )
    }));
    let settings_executor = services.executor.clone();
    run(context, "应用设置加载", Task::new(async move {
        ShellMessage::SettingsLoaded(
            settings_executor.block_on(async move { settings.load_or_recover() }),
        )
    }));
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
