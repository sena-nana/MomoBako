//! 把设置、插件、日志、来源登录和仓库动作的副作用交给领域服务。
//!
//! 剪贴板写入系统剪贴板，保存和打开对话框排队为平台文件对话框。插件、缓存、
//! 仓库配置、建仓、同步和写文件都走 `NativeServices`；任务都经 `task_guard` 派发，服务没启动、任务排不进去或
//! 任务 panic 时把错误写回状态机，不留在进行中。

use nana_ui::RuntimeProgramContext;

use crate::backend::services::mutsuki_runner::{
    PROTOCOL_REPOSITORY_ACTION_RUN, PROTOCOL_REPOSITORY_CREATE, PROTOCOL_REPOSITORY_SYNC,
};
use crate::backend::services::repository::{
    BinaryFileWriteRequest, NeteaseRepositoryCacheConfigureRequest, PluginCallRequest,
    PluginConfigDeleteRequest,
    PluginConfigSetRequest, PluginConfigSnapshot, PluginEnabledRequest, PluginHookExecutionListRequest, PluginInstallRequest,
    PlaylistItemsByPathsAddRequest, PlaylistMutationRequest, PluginManifest, RepositoryAction,
    RepositoryActionRunRequest,
    RepositoryBackendConfigUpdateRequest, RepositoryMutationRequest, SyncRequest, SystemLogQuery, SystemLogWriteRequest,
};
use crate::backend::viewmodels::PluginViewModel;
use crate::services::NativeServices;
use crate::shell::admin::api::{self, ApiMessage, HttpRequest};
use crate::shell::admin::source_download::DownloadOutcome;
use crate::shell::admin::{AdminEffect, AdminMessage, PluginCallOrigin, SourceStep};
use crate::shell::ShellMessage;
use crate::task_guard::{spawn, spawn_service};
use crate::MomoBakoApplication;

/// 执行管理归约留下的请求。同一次更新里新产生的请求会继续发出去。
pub fn dispatch_admin_effects(app: &mut MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>) {
    for _ in 0..8 {
        let effects = app.shell.admin.take_effects();
        if effects.is_empty() {
            break;
        }
        for effect in effects {
            dispatch_one(app, context, effect);
        }
    }
}

fn admin(message: AdminMessage) -> ShellMessage {
    ShellMessage::Admin(message)
}

type Executor = std::sync::Arc<tokio::runtime::Runtime>;

fn dispatch_one(app: &mut MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>, effect: AdminEffect) {
    match effect {
        AdminEffect::PersistCorners => app.shell.admin.save_corners_file(),
        AdminEffect::SaveSettings => save_settings(app, context),
        AdminEffect::CopyText(text) => {
            if !crate::host_bridge::copy_text(&text) {
                eprintln!("Nana 宿主剪贴板写入失败");
                app.shell.admin.external_message.clear();
                app.shell.admin.external_error = "复制失败：系统剪贴板写入失败".into();
            }
        }
        AdminEffect::RequestOpenDialog => app.shell.input.queue_plugin_dialog(),
        AdminEffect::RequestSaveDialog { .. } => app.shell.input.queue_save_dialog(),
        AdminEffect::LoadSettingsBundle => load_bundle(app, context),
        AdminEffect::LoadLogs => load_logs(app, context),
        AdminEffect::ClearLogs => clear_logs(app, context),
        AdminEffect::LoadAppSettings => load_app_settings(app, context),
        AdminEffect::LoadPlaylistPlayers => load_players(app, context),
        AdminEffect::Install(path) => plugins_task(app, context, "插件安装", move |plugin, executor| {
            executor.block_on(plugin.install_plugin_from_archive(PluginInstallRequest { package_path: path })).map(|response| response.plugins)
        }),
        AdminEffect::DeletePlugin(plugin_id) => plugins_task(app, context, "插件删除", move |plugin, executor| {
            executor.block_on(plugin.delete_plugin(plugin_id)).map(|response| response.plugins)
        }),
        AdminEffect::SetEnabled { plugin_id, enabled } => plugins_task(app, context, "插件启停", move |plugin, executor| {
            executor.block_on(plugin.set_plugin_enabled(PluginEnabledRequest { plugin_id, enabled })).map(|response| response.plugins)
        }),
        AdminEffect::SetConfig { plugin_id, key, value } => config_task(app, context, "插件设置保存", move |plugin, executor| {
            executor.block_on(plugin.set_plugin_config_value(PluginConfigSetRequest { plugin_id, key, value }))
        }),
        AdminEffect::DeleteConfig { plugin_id, key } => config_task(app, context, "插件设置重置", move |plugin, executor| {
            executor.block_on(plugin.delete_plugin_config_value(PluginConfigDeleteRequest { plugin_id, key }))
        }),
        AdminEffect::LoadConfig(plugin_id) => config_task(app, context, "插件设置读取", move |plugin, executor| {
            executor.block_on(plugin.get_plugin_config(plugin_id))
        }),
        AdminEffect::OpenDataDirectory { plugin_id, name } => open_directory(app, context, plugin_id, name),
        AdminEffect::LoadActions { repo_id } => load_actions(app, context, repo_id),
        AdminEffect::RunAction { repo_id, action_id, paths } => run_action(app, context, repo_id, action_id, paths),
        AdminEffect::WriteFile { path, bytes } => write_file(app, context, path, bytes),
        AdminEffect::CallPlugin { plugin_id, method, payload, repository_id, origin } => {
            call_plugin(app, context, plugin_id, method, payload, repository_id, origin);
        }
        AdminEffect::CreateSourcePlaylist { repo_id, playlist_id, name, player_type_id, path } => {
            create_source_playlist(app, context, repo_id, playlist_id, name, player_type_id, path);
        }
        AdminEffect::DownloadToRepository { run, payload, parent_path } => {
            crate::download_dispatch::download_to_repository(app, context, run, payload, parent_path);
        }
        AdminEffect::WriteLogs(requests) => write_logs(app, context, requests),
        AdminEffect::UpdateBackendConfig { repo_id, backend_config, step } => {
            source_task(app, context, "更新仓库后端配置", step, move |services| {
                let management = services.repository_management.clone();
                let executor = services.executor.clone();
                move || {
                    executor
                        .block_on(management.update_repository_backend_config(RepositoryBackendConfigUpdateRequest { repo_id, backend_config }))
                        .and_then(|response| serde_json::to_value(response).map_err(|error| format!("仓库配置结果无法序列化：{error}")))
                }
            });
        }
        AdminEffect::ConfigureSourceCache { repo_id, path, step } => {
            source_task(app, context, "配置来源缓存", step, move |services| {
                let management = services.repository_management.clone();
                let executor = services.executor.clone();
                move || {
                    let request = NeteaseRepositoryCacheConfigureRequest { repo_id, path, migrate_legacy_cache: true };
                    executor
                        .block_on(management.configure_netease_repository_cache(request))
                        .and_then(|response| serde_json::to_value(response).map_err(|error| format!("缓存配置结果无法序列化：{error}")))
                }
            });
        }
        AdminEffect::CreateSourceRepository { repo_id, name, path, backend_plugin_id, backend_config } => {
            let step = SourceStep::ProvisionCreate { repo_id: repo_id.clone(), name: name.clone() };
            let request = RepositoryMutationRequest {
                repo_id: Some(repo_id),
                name,
                path,
                backend_plugin_id: Some(backend_plugin_id),
                backend_config: Some(backend_config),
                skip_initial_sync: true,
            };
            source_task(app, context, "来源建仓", step, move |services| {
                let tasks = services.tasks.clone();
                let executor = services.executor.clone();
                move || executor.block_on(tasks.execute(PROTOCOL_REPOSITORY_CREATE, request)).map(|(output, _)| output)
            });
        }
        AdminEffect::SyncRepository { repo_id } => {
            let step = SourceStep::Sync { repo_id: repo_id.clone() };
            source_task(app, context, "来源仓库后台同步", step, move |services| {
                let tasks = services.tasks.clone();
                let executor = services.executor.clone();
                move || executor.block_on(tasks.execute(PROTOCOL_REPOSITORY_SYNC, SyncRequest { repo_id })).map(|(output, _)| output)
            });
        }
        AdminEffect::HttpRequest(request) => http_request(context, request),
    }
}

fn source_step(step: SourceStep, result: Result<serde_json::Value, String>) -> ShellMessage {
    admin(AdminMessage::SourceStepFinished { step, result })
}

/// 来源登录流程里的一步：`prepare` 从服务取出句柄、交回在任务里跑的 `work`，结果回 `SourceStepFinished`。
fn source_task<P, W>(app: &MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>, what: &str, step: SourceStep, prepare: P)
where
    P: FnOnce(&NativeServices) -> W,
    W: FnOnce() -> Result<serde_json::Value, String> + Send + 'static,
{
    let failed_step = step.clone();
    let failed = move |error| source_step(failed_step.clone(), Err(error));
    spawn_service(app.services.as_ref(), context, what, failed, |services| {
        let work = prepare(services);
        move || source_step(step, work())
    });
}

/// 主题等设置改动后立即写设置文件，结果回 `SettingsSaved`。
fn save_settings(app: &MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>) {
    let settings = app.shell.settings.clone();
    spawn_service(app.services.as_ref(), context, "应用设置保存", |error| ShellMessage::SettingsSaved(Err(error)), |services| {
        let store = services.settings.clone();
        move || ShellMessage::SettingsSaved(settings.validate().and_then(|()| store.save(&settings).map(|()| settings)))
    });
}

fn call_plugin(
    app: &MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    plugin_id: String,
    method: String,
    payload: serde_json::Value,
    repository_id: Option<String>,
    origin: PluginCallOrigin,
) {
    let (failed_origin, failed_method) = (origin.clone(), method.clone());
    let failed = move |error| plugin_result(failed_origin.clone(), failed_method.clone(), Err(error));
    spawn_service(app.services.as_ref(), context, "插件调用", failed, |services| {
        let plugin = services.plugin.clone();
        let executor = services.executor.clone();
        move || {
            let called = method.clone();
            let result = executor.block_on(plugin.call_plugin(PluginCallRequest { plugin_id, method, repository_id, payload }));
            let result = match origin {
                // Playground 显示整个调用结果，和 Vue `ctx.callPlugin` 的返回一致。
                PluginCallOrigin::Playground => result.and_then(|item| serde_json::to_value(item).map_err(|error| format!("插件调用结果无法序列化：{error}"))),
                _ => result.map(|item| item.payload),
            };
            plugin_result(origin, called, result)
        }
    });
}

/// 「创建播放列表」照 Vue `runPlaylistAction`：先列出播放集，没有这个标识才建，再把目录按路径加进去。
/// 哪一步失败都把原因交回，不留在进行中。
fn create_source_playlist(
    app: &MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    repo_id: String,
    playlist_id: String,
    name: String,
    player_type_id: String,
    path: String,
) {
    let failed = |error| admin(AdminMessage::SourcePlaylistFinished(Err(error)));
    spawn_service(app.services.as_ref(), context, "创建播放列表", failed, |services| {
        let interaction = services.repository_interaction.clone();
        let executor = services.executor.clone();
        move || {
            let result = (|| {
                let existing = executor.block_on(interaction.list_playlists(repo_id.clone()))?;
                if !existing.iter().any(|playlist| playlist.playlist_id == playlist_id) {
                    let request = PlaylistMutationRequest { repo_id: repo_id.clone(), playlist_id: Some(playlist_id.clone()), name, player_type_id };
                    executor.block_on(interaction.create_playlist(request))?;
                }
                let request = PlaylistItemsByPathsAddRequest { repo_id, playlist_id, paths: vec![path] };
                executor.block_on(interaction.add_playlist_items_by_paths(request)).map(|_| ())
            })();
            admin(AdminMessage::SourcePlaylistFinished(result))
        }
    });
}

/// 写系统日志：在任务里逐条写，写不进去的记下原因一起交回。
fn write_logs(app: &MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>, requests: Vec<SystemLogWriteRequest>) {
    let failed = |error| admin(AdminMessage::LogsWritten(Err(error)));
    spawn_service(app.services.as_ref(), context, "写系统日志", failed, |services| {
        let system = services.system.clone();
        let executor = services.executor.clone();
        move || {
            let failures: Vec<String> = requests
                .into_iter()
                .filter_map(|request| {
                    let action = request.action.clone();
                    executor.block_on(system.write_system_log(request)).err().map(|error| format!("{action}：{error}"))
                })
                .collect();
            admin(AdminMessage::LogsWritten(if failures.is_empty() { Ok(()) } else { Err(failures.join("；")) }))
        }
    });
}

/// 按来源把插件调用结果交回对应的状态机。
fn plugin_result(origin: PluginCallOrigin, method: String, result: Result<serde_json::Value, String>) -> ShellMessage {
    match origin {
        PluginCallOrigin::FileMenu => admin(AdminMessage::FilePluginFinished { method, result }),
        PluginCallOrigin::Download(run) => admin(AdminMessage::DownloadFinished { run, result: result.map(DownloadOutcome::plugin) }),
        PluginCallOrigin::SourceAuth(step) => source_step(step, result),
        PluginCallOrigin::Playground => admin(AdminMessage::Api(ApiMessage::PluginFinished(result))),
    }
}

/// API Playground 的外部 HTTP 请求在任务线程里同步发出。
fn http_request(context: &RuntimeProgramContext<ShellMessage>, request: HttpRequest) {
    let failed = |error| admin(AdminMessage::Api(ApiMessage::HttpFinished(Err(error))));
    spawn(context, "外部 HTTP 请求", failed, move || {
        let result = api::http::execute(&request);
        if let Err(error) = &result {
            eprintln!("Nana API Playground HTTP 请求失败：{} {}：{error}", request.method, request.url);
        }
        admin(AdminMessage::Api(ApiMessage::HttpFinished(result)))
    });
}

/// 设置页一次读五份数据：插件、钩子记录、缓存、API 设计和外部连接。首屏启动第 4 步等着它，失败（含任务 panic）时
/// 按五份都失败交回（[`failed_bundle_message`]），启动照常结束。
fn load_bundle(app: &MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>) {
    spawn_service(app.services.as_ref(), context, "设置页数据", failed_bundle_message, |services| {
        let plugin = services.plugin.clone();
        let system = services.system.clone();
        let executor = services.executor.clone();
        move || {
            let plugins = executor.block_on(plugin.list_plugins());
            let hooks = executor.block_on(plugin.list_plugin_hook_executions(Some(PluginHookExecutionListRequest { plugin_id: None, limit: Some(200) })));
            let cache = executor.block_on(plugin.get_cache_snapshot());
            let api = executor.block_on(plugin.get_api_design_snapshot());
            let external = executor.block_on(system.get_external_api_connection_status());
            admin(AdminMessage::SettingsBundleLoaded { plugins, hooks: hooks.map(|response| response.records), cache, api, external })
        }
    });
}

/// 设置包读不到时交回的消息：五份都按同一个原因失败。
pub(crate) fn failed_bundle_message(error: String) -> ShellMessage {
    admin(AdminMessage::SettingsBundleLoaded {
        plugins: Err(error.clone()),
        hooks: Err(error.clone()),
        cache: Err(error.clone()),
        api: Err(error.clone()),
        external: Err(error),
    })
}

/// 历史日志一次读的条数，和 Vue `loadSystemLogsInWorkspace(limit = 200)` 一致。
const LOG_PAGE_LIMIT: usize = 200;

/// 读最近一页系统日志。结果回 `LogsLoaded`。
fn load_logs(app: &MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>) {
    let query = SystemLogQuery { limit: Some(LOG_PAGE_LIMIT), ..Default::default() };
    spawn_service(app.services.as_ref(), context, "系统日志", |error| ShellMessage::LogsLoaded(Err(error)), |services| {
        let system = services.system.clone();
        let executor = services.executor.clone();
        move || ShellMessage::LogsLoaded(executor.block_on(system.list_system_logs(Some(query))))
    });
}

/// 清空系统日志的持久化文件。结果回 `LogsCleared`，成功后不再重读（Vue `clearSystemLogsInWorkspace`）。
fn clear_logs(app: &MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>) {
    spawn_service(app.services.as_ref(), context, "清空系统日志", |error| admin(AdminMessage::LogsCleared(Err(error))), |services| {
        let system = services.system.clone();
        let executor = services.executor.clone();
        move || admin(AdminMessage::LogsCleared(executor.block_on(system.clear_system_logs())))
    });
}

/// 读设置目录里的应用设置。结果回 `SettingsLoaded`。
fn load_app_settings(app: &MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>) {
    spawn_service(app.services.as_ref(), context, "应用设置读取", |error| ShellMessage::SettingsLoaded(Err(error)), |services| {
        let settings = services.settings.clone();
        move || ShellMessage::SettingsLoaded(settings.load_or_recover())
    });
}

/// 读插件登记的播放器类型。结果回 `PlaylistPlayersLoaded`，播放集侧栏、播放器和新建对话框共用；首屏启动第 4 步
/// 也等着它，失败（含任务 panic）时按失败交回，启动照常结束。
fn load_players(app: &MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>) {
    spawn_service(app.services.as_ref(), context, "播放器类型", players_failed, |services| {
        let plugin = services.plugin.clone();
        let executor = services.executor.clone();
        move || ShellMessage::PlaylistPlayersLoaded(executor.block_on(plugin.list_playlist_players()))
    });
}

/// 播放器类型读不到时交回的消息。
pub(crate) fn players_failed(error: String) -> ShellMessage {
    ShellMessage::PlaylistPlayersLoaded(Err(error))
}

/// 换插件列表的操作：安装、删除、启停。结果回 `PluginsReplaced`。
fn plugins_task(
    app: &MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    label: &'static str,
    run: impl FnOnce(PluginViewModel, Executor) -> Result<Vec<PluginManifest>, String> + Send + 'static,
) {
    spawn_service(app.services.as_ref(), context, label, |error| admin(AdminMessage::PluginsReplaced(Err(error))), |services| {
        let plugin = services.plugin.clone();
        let executor = services.executor.clone();
        move || admin(AdminMessage::PluginsReplaced(run(plugin, executor)))
    });
}

/// 读写插件配置。结果回 `PluginConfigLoaded`。
fn config_task(
    app: &MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    label: &'static str,
    run: impl FnOnce(PluginViewModel, Executor) -> Result<PluginConfigSnapshot, String> + Send + 'static,
) {
    spawn_service(app.services.as_ref(), context, label, |error| ShellMessage::PluginConfigLoaded(Err(error)), |services| {
        let plugin = services.plugin.clone();
        let executor = services.executor.clone();
        move || ShellMessage::PluginConfigLoaded(run(plugin, executor))
    });
}

fn open_directory(app: &MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>, plugin_id: String, name: String) {
    let failed_name = name.clone();
    let failed = move |error| admin(AdminMessage::DataDirectoryFinished { name: failed_name.clone(), result: Err(error) });
    spawn_service(app.services.as_ref(), context, "插件设置目录", failed, |services| {
        let plugin = services.plugin.clone();
        let executor = services.executor.clone();
        move || {
            let result = executor.block_on(plugin.get_plugin_data_directory(plugin_id)).map(|response| response.path);
            admin(AdminMessage::DataDirectoryFinished { name, result })
        }
    });
}

fn load_actions(app: &MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>, repo_id: String) {
    let failed_repo = repo_id.clone();
    let failed = move |error| admin(AdminMessage::ActionsLoaded { repo_id: failed_repo.clone(), result: Err(error) });
    spawn_service(app.services.as_ref(), context, "仓库动作", failed, |services| {
        let interaction = services.repository_interaction.clone();
        let executor = services.executor.clone();
        move || {
            let result = executor.block_on(interaction.list_repository_actions(repo_id.clone()));
            admin(AdminMessage::ActionsLoaded { repo_id, result })
        }
    });
}

fn run_action(app: &MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>, repo_id: String, action_id: String, paths: Vec<String>) {
    let failed = |error| admin(AdminMessage::ActionRunFinished { result: Err(error) });
    spawn_service(app.services.as_ref(), context, "仓库动作执行", failed, |services| {
        let tasks = services.tasks.clone();
        let executor = services.executor.clone();
        move || {
            let request = RepositoryActionRunRequest { repo_id, action_id, target_paths: Some(paths), asset_ids: None };
            let result = executor.block_on(tasks.execute(PROTOCOL_REPOSITORY_ACTION_RUN, request));
            admin(AdminMessage::ActionRunFinished { result: result.and_then(|(value, _)| action_from_value(value)) })
        }
    });
}

fn action_from_value(value: serde_json::Value) -> Result<RepositoryAction, String> {
    let action = value.get("action").cloned().ok_or_else(|| "动作结果缺少 action".to_string())?;
    serde_json::from_value(action).map_err(|error| format!("动作结果无法解析：{error}"))
}

fn write_file(app: &MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>, path: String, bytes: Vec<u8>) {
    let failed = |error| admin(AdminMessage::WriteFinished(Err(error)));
    spawn_service(app.services.as_ref(), context, "外部连接导出", failed, |services| {
        let system = services.system.clone();
        let executor = services.executor.clone();
        move || {
            let result = executor.block_on(system.write_binary_file(BinaryFileWriteRequest { path, bytes })).map(|_| ());
            admin(AdminMessage::WriteFinished(result))
        }
    });
}
