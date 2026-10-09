//! 把设置、插件、日志、来源登录和仓库动作的副作用交给领域服务。
//!
//! 剪贴板写入系统剪贴板，保存和打开对话框排队为平台文件对话框。插件、缓存、
//! 仓库配置、建仓、同步和写文件都走 `NativeServices`；服务没启动或任务提交失败时
//! 把错误写回状态机，不留在进行中。

use nana_ui::runtime::Task;
use nana_ui::RuntimeProgramContext;

use crate::backend::services::mutsuki_runner::{PROTOCOL_REPOSITORY_ACTION_RUN, PROTOCOL_REPOSITORY_CREATE, PROTOCOL_REPOSITORY_SYNC};
use crate::backend::services::repository::{
    BinaryFileWriteRequest, FileBrowserRequest, NeteaseRepositoryCacheConfigureRequest, PluginCallRequest, PluginConfigDeleteRequest,
    PluginConfigSetRequest, PluginConfigSnapshot, PluginEnabledRequest, PluginHookExecutionListRequest, PluginInstallRequest,
    PluginManifest, RepositoryAction, RepositoryActionRunRequest, RepositoryBackendConfigUpdateRequest, RepositoryMutationRequest,
    SyncRequest,
};
use crate::backend::viewmodels::PluginViewModel;
use crate::shell::admin::api::{self, ApiMessage, HttpRequest};
use crate::shell::admin::{AdminEffect, AdminMessage, PluginCallOrigin, SourceStep};
use crate::shell::ShellMessage;
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

/// 服务未启动时的统一错误。
const NO_SERVICES: &str = "领域服务未启动";

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
        AdminEffect::ReloadBrowser { repo_id } => reload_browser(app, context, repo_id),
        AdminEffect::WriteFile { path, bytes } => write_file(app, context, path, bytes),
        AdminEffect::CallPlugin { plugin_id, method, payload, repository_id, origin } => {
            call_plugin(app, context, plugin_id, method, payload, repository_id, origin);
        }
        AdminEffect::UpdateBackendConfig { repo_id, backend_config, step } => {
            let Some(services) = app.services.as_ref() else {
                eprintln!("Nana 更新仓库后端配置需要领域服务");
                app.shell.reduce(source_step(step, Err(NO_SERVICES.into())));
                return;
            };
            let management = services.repository_management.clone();
            let executor = services.executor.clone();
            let failed = step.clone();
            submit(app, context, failed, Task::new(async move {
                let result = executor
                    .block_on(management.update_repository_backend_config(RepositoryBackendConfigUpdateRequest { repo_id, backend_config }))
                    .and_then(|response| serde_json::to_value(response).map_err(|error| format!("仓库配置结果无法序列化：{error}")));
                source_step(step, result)
            }));
        }
        AdminEffect::ConfigureSourceCache { repo_id, path, step } => {
            let Some(services) = app.services.as_ref() else {
                eprintln!("Nana 配置来源缓存需要领域服务");
                app.shell.reduce(source_step(step, Err(NO_SERVICES.into())));
                return;
            };
            let management = services.repository_management.clone();
            let executor = services.executor.clone();
            let failed = step.clone();
            submit(app, context, failed, Task::new(async move {
                let request = NeteaseRepositoryCacheConfigureRequest { repo_id, path, migrate_legacy_cache: true };
                let result = executor
                    .block_on(management.configure_netease_repository_cache(request))
                    .and_then(|response| serde_json::to_value(response).map_err(|error| format!("缓存配置结果无法序列化：{error}")));
                source_step(step, result)
            }));
        }
        AdminEffect::CreateSourceRepository { repo_id, name, path, backend_plugin_id, backend_config } => {
            let step = SourceStep::ProvisionCreate { repo_id: repo_id.clone(), name: name.clone() };
            let Some(services) = app.services.as_ref() else {
                eprintln!("Nana 来源建仓需要领域服务");
                app.shell.reduce(source_step(step, Err(NO_SERVICES.into())));
                return;
            };
            let tasks = services.tasks.clone();
            let executor = services.executor.clone();
            let failed = step.clone();
            let request = RepositoryMutationRequest {
                repo_id: Some(repo_id),
                name,
                path,
                backend_plugin_id: Some(backend_plugin_id),
                backend_config: Some(backend_config),
                skip_initial_sync: true,
            };
            submit(app, context, failed, Task::new(async move {
                let result = executor.block_on(tasks.execute(PROTOCOL_REPOSITORY_CREATE, request)).map(|(output, _)| output);
                source_step(step, result)
            }));
        }
        AdminEffect::SyncRepository { repo_id } => {
            let step = SourceStep::Sync { repo_id: repo_id.clone() };
            let Some(services) = app.services.as_ref() else {
                eprintln!("Nana 来源仓库后台同步需要领域服务");
                app.shell.reduce(source_step(step, Err(NO_SERVICES.into())));
                return;
            };
            let tasks = services.tasks.clone();
            let executor = services.executor.clone();
            let failed = step.clone();
            submit(app, context, failed, Task::new(async move {
                let result = executor.block_on(tasks.execute(PROTOCOL_REPOSITORY_SYNC, SyncRequest { repo_id })).map(|(output, _)| output);
                source_step(step, result)
            }));
        }
        AdminEffect::HttpRequest(request) => http_request(app, context, request),
    }
}

fn source_step(step: SourceStep, result: Result<serde_json::Value, String>) -> ShellMessage {
    admin(AdminMessage::SourceStepFinished { step, result })
}

/// 提交来源流程里的一步。提交失败时直接按失败回写这一步。
fn submit(app: &mut MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>, step: SourceStep, task: Task<ShellMessage>) {
    if let Err(error) = context.run_task(task) {
        eprintln!("Nana 来源登录任务提交失败：{error}");
        app.shell.reduce(source_step(step, Err(format!("任务提交失败：{error}"))));
    }
}

/// 主题等设置改动后立即写设置文件，结果回 `SettingsSaved`。
fn save_settings(app: &mut MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>) {
    let Some(services) = app.services.as_ref() else {
        eprintln!("Nana 保存应用设置需要领域服务，当前服务未启动");
        return;
    };
    let settings = app.shell.settings.clone();
    let store = services.settings.clone();
    if let Err(error) = context.run_task(Task::new(async move {
        let result = settings.validate().and_then(|()| store.save(&settings).map(|()| settings));
        ShellMessage::SettingsSaved(result)
    })) {
        eprintln!("Nana 应用设置保存任务提交失败：{error}");
        app.shell.reduce(ShellMessage::SettingsSaved(Err(format!("应用设置保存任务提交失败：{error}"))));
    }
}

fn call_plugin(
    app: &mut MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    plugin_id: String,
    method: String,
    payload: serde_json::Value,
    repository_id: Option<String>,
    origin: PluginCallOrigin,
) {
    let Some(services) = app.services.as_ref() else {
        eprintln!("Nana 插件调用需要领域服务，当前服务未启动：{method}");
        app.shell.reduce(plugin_result(origin, method, Err(NO_SERVICES.into())));
        return;
    };
    let plugin = services.plugin.clone();
    let executor = services.executor.clone();
    let failed_origin = origin.clone();
    let failed_method = method.clone();
    let called = method.clone();
    if let Err(error) = context.run_task(Task::new(async move {
        let result = executor.block_on(plugin.call_plugin(PluginCallRequest { plugin_id, method, repository_id, payload }));
        let result = match origin {
            // Playground 显示整个调用结果，和 Vue `ctx.callPlugin` 的返回一致。
            PluginCallOrigin::Playground => result.and_then(|item| serde_json::to_value(item).map_err(|error| format!("插件调用结果无法序列化：{error}"))),
            _ => result.map(|item| item.payload),
        };
        plugin_result(origin, called, result)
    })) {
        eprintln!("Nana 插件调用任务提交失败：{error}");
        app.shell.reduce(plugin_result(failed_origin, failed_method, Err(format!("插件调用提交失败：{error}"))));
    }
}

/// 按来源把插件调用结果交回对应的状态机。
fn plugin_result(origin: PluginCallOrigin, method: String, result: Result<serde_json::Value, String>) -> ShellMessage {
    match origin {
        PluginCallOrigin::FileMenu => admin(AdminMessage::FilePluginFinished { method, result }),
        PluginCallOrigin::SourceAuth(step) => source_step(step, result),
        PluginCallOrigin::Playground => admin(AdminMessage::Api(ApiMessage::PluginFinished(result))),
    }
}

/// API Playground 的外部 HTTP 请求在任务线程里同步发出。
fn http_request(app: &mut MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>, request: HttpRequest) {
    if let Err(error) = context.run_task(Task::new(async move {
        let result = api::http::execute(&request);
        if let Err(error) = &result {
            eprintln!("Nana API Playground HTTP 请求失败：{} {}：{error}", request.method, request.url);
        }
        admin(AdminMessage::Api(ApiMessage::HttpFinished(result)))
    })) {
        eprintln!("Nana API Playground 请求任务提交失败：{error}");
        app.shell.reduce(admin(AdminMessage::Api(ApiMessage::HttpFinished(Err(format!("请求任务提交失败：{error}"))))));
    }
}

/// 设置页一次读五份数据：插件、钩子记录、缓存、API 设计和外部连接。
fn load_bundle(app: &mut MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>) {
    let Some(services) = app.services.as_ref() else {
        eprintln!("Nana 设置页数据需要领域服务，当前服务未启动");
        app.shell.reduce(admin(failed_bundle(NO_SERVICES)));
        return;
    };
    let plugin = services.plugin.clone();
    let system = services.system.clone();
    let executor = services.executor.clone();
    if let Err(error) = context.run_task(Task::new(async move {
        let plugins = executor.block_on(plugin.list_plugins());
        let hooks = executor.block_on(plugin.list_plugin_hook_executions(Some(PluginHookExecutionListRequest { plugin_id: None, limit: Some(200) })));
        let cache = executor.block_on(plugin.get_cache_snapshot());
        let api = executor.block_on(plugin.get_api_design_snapshot());
        let external = executor.block_on(system.get_external_api_connection_status());
        admin(AdminMessage::SettingsBundleLoaded { plugins, hooks: hooks.map(|response| response.records), cache, api, external })
    })) {
        eprintln!("Nana 设置页数据任务提交失败：{error}");
        app.shell.reduce(admin(failed_bundle(&format!("设置页数据任务提交失败：{error}"))));
    }
}

fn failed_bundle(error: &str) -> AdminMessage {
    let error = error.to_string();
    AdminMessage::SettingsBundleLoaded {
        plugins: Err(error.clone()),
        hooks: Err(error.clone()),
        cache: Err(error.clone()),
        api: Err(error.clone()),
        external: Err(error),
    }
}

/// 换插件列表的操作：安装、删除、启停。结果回 `PluginsReplaced`。
fn plugins_task(
    app: &mut MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    label: &'static str,
    run: impl FnOnce(PluginViewModel, Executor) -> Result<Vec<PluginManifest>, String> + Send + 'static,
) {
    let Some(services) = app.services.as_ref() else {
        eprintln!("Nana {label}需要领域服务，当前服务未启动");
        app.shell.reduce(admin(AdminMessage::PluginsReplaced(Err(NO_SERVICES.into()))));
        return;
    };
    let plugin = services.plugin.clone();
    let executor = services.executor.clone();
    if let Err(error) = context.run_task(Task::new(async move { admin(AdminMessage::PluginsReplaced(run(plugin, executor))) })) {
        eprintln!("Nana {label}任务提交失败：{error}");
        app.shell.reduce(admin(AdminMessage::PluginsReplaced(Err(format!("{label}任务提交失败：{error}")))));
    }
}

/// 读写插件配置。结果回 `PluginConfigLoaded`。
fn config_task(
    app: &mut MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    label: &'static str,
    run: impl FnOnce(PluginViewModel, Executor) -> Result<PluginConfigSnapshot, String> + Send + 'static,
) {
    let Some(services) = app.services.as_ref() else {
        eprintln!("Nana {label}需要领域服务，当前服务未启动");
        app.shell.reduce(ShellMessage::PluginConfigLoaded(Err(NO_SERVICES.into())));
        return;
    };
    let plugin = services.plugin.clone();
    let executor = services.executor.clone();
    if let Err(error) = context.run_task(Task::new(async move { ShellMessage::PluginConfigLoaded(run(plugin, executor)) })) {
        eprintln!("Nana {label}任务提交失败：{error}");
        app.shell.reduce(ShellMessage::PluginConfigLoaded(Err(format!("{label}任务提交失败：{error}"))));
    }
}

fn open_directory(app: &mut MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>, plugin_id: String, name: String) {
    let Some(services) = app.services.as_ref() else {
        eprintln!("Nana 插件设置目录需要领域服务，当前服务未启动");
        app.shell.reduce(admin(AdminMessage::DataDirectoryFinished { name, result: Err(NO_SERVICES.into()) }));
        return;
    };
    let plugin = services.plugin.clone();
    let executor = services.executor.clone();
    let failed_name = name.clone();
    if let Err(error) = context.run_task(Task::new(async move {
        let result = executor.block_on(plugin.get_plugin_data_directory(plugin_id)).map(|response| response.path);
        admin(AdminMessage::DataDirectoryFinished { name, result })
    })) {
        eprintln!("Nana 插件设置目录任务提交失败：{error}");
        app.shell.reduce(admin(AdminMessage::DataDirectoryFinished { name: failed_name, result: Err(format!("插件设置目录任务提交失败：{error}")) }));
    }
}

fn load_actions(app: &mut MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>, repo_id: String) {
    let Some(services) = app.services.as_ref() else {
        eprintln!("Nana 仓库动作需要领域服务，当前服务未启动");
        app.shell.reduce(admin(AdminMessage::ActionsLoaded { repo_id, result: Err(NO_SERVICES.into()) }));
        return;
    };
    let interaction = services.repository_interaction.clone();
    let executor = services.executor.clone();
    let request_id = repo_id.clone();
    if let Err(error) = context.run_task(Task::new(async move {
        let result = executor.block_on(interaction.list_repository_actions(request_id.clone()));
        admin(AdminMessage::ActionsLoaded { repo_id: request_id, result })
    })) {
        eprintln!("Nana 仓库动作任务提交失败：{error}");
        app.shell.reduce(admin(AdminMessage::ActionsLoaded { repo_id, result: Err(format!("仓库动作任务提交失败：{error}")) }));
    }
}

fn run_action(app: &mut MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>, repo_id: String, action_id: String, paths: Vec<String>) {
    let Some(services) = app.services.as_ref() else {
        eprintln!("Nana 仓库动作执行需要领域服务，当前服务未启动");
        app.shell.reduce(admin(AdminMessage::ActionRunFinished { result: Err(NO_SERVICES.into()) }));
        return;
    };
    let tasks = services.tasks.clone();
    let executor = services.executor.clone();
    if let Err(error) = context.run_task(Task::new(async move {
        let result = executor.block_on(tasks.execute(
            PROTOCOL_REPOSITORY_ACTION_RUN,
            RepositoryActionRunRequest { repo_id, action_id, target_paths: Some(paths), asset_ids: None },
        ));
        admin(AdminMessage::ActionRunFinished { result: result.and_then(|(value, _)| action_from_value(value)) })
    })) {
        eprintln!("Nana 仓库动作执行任务提交失败：{error}");
        app.shell.reduce(admin(AdminMessage::ActionRunFinished { result: Err(format!("仓库动作执行任务提交失败：{error}")) }));
    }
}

fn action_from_value(value: serde_json::Value) -> Result<RepositoryAction, String> {
    let action = value.get("action").cloned().ok_or_else(|| "动作结果缺少 action".to_string())?;
    serde_json::from_value(action).map_err(|error| format!("动作结果无法解析：{error}"))
}

fn reload_browser(app: &mut MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>, repo_id: String) {
    let Some(services) = app.services.as_ref() else {
        eprintln!("Nana 动作完成后的文件刷新需要领域服务，当前服务未启动");
        return;
    };
    let directory = app.shell.current_directory.clone();
    let browser = services.file_browser.clone();
    let executor = services.executor.clone();
    let request = FileBrowserRequest {
        repo_id,
        directory_path: (!directory.is_empty()).then_some(directory),
        include_tree: Some(false),
        special_location: None,
        offset: Some(0),
        limit: Some(200),
    };
    if let Err(error) = context.run_task(Task::new(async move {
        ShellMessage::FileBrowserLoaded(executor.block_on(browser.get_file_browser(request)))
    })) {
        eprintln!("Nana 动作完成后的文件刷新任务提交失败：{error}");
    }
}

fn write_file(app: &mut MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>, path: String, bytes: Vec<u8>) {
    let Some(services) = app.services.as_ref() else {
        eprintln!("Nana 外部连接导出需要领域服务，当前服务未启动");
        app.shell.reduce(admin(AdminMessage::WriteFinished(Err(NO_SERVICES.into()))));
        return;
    };
    let system = services.system.clone();
    let executor = services.executor.clone();
    if let Err(error) = context.run_task(Task::new(async move {
        let result = executor.block_on(system.write_binary_file(BinaryFileWriteRequest { path, bytes })).map(|_| ());
        admin(AdminMessage::WriteFinished(result))
    })) {
        eprintln!("Nana 外部连接导出任务提交失败：{error}");
        app.shell.reduce(admin(AdminMessage::WriteFinished(Err(format!("外部连接导出任务提交失败：{error}")))));
    }
}
