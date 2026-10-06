//! 把设置、插件、日志和仓库动作的副作用交给已有领域服务。
//!
//! 剪贴板写入系统剪贴板。保存和打开对话框排队为平台文件对话框。
//! 插件、缓存、动作和写文件没有替身；服务没启动时把错误写回状态机。

use nana_ui::runtime::Task;
use nana_ui::RuntimeProgramContext;

use crate::backend::services::mutsuki_runner::PROTOCOL_REPOSITORY_ACTION_RUN;
use crate::backend::services::repository::{
    BinaryFileWriteRequest, FileBrowserRequest, PluginConfigDeleteRequest, PluginConfigSetRequest, PluginEnabledRequest,
    PluginHookExecutionListRequest, PluginInstallRequest, RepositoryAction, RepositoryActionRunRequest,
};
use crate::shell::admin::{AdminEffect, AdminMessage};
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

fn dispatch_one(app: &mut MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>, effect: AdminEffect) {
    match effect {
        AdminEffect::PersistCorners => app.shell.admin.save_corners_file(),
        AdminEffect::CopyText(text) => {
            if !crate::host_bridge::copy_text(&text) {
                eprintln!("Nana 宿主剪贴板写入失败");
                app.shell.admin.external_message.clear();
                app.shell.admin.external_error = "复制失败：宿主剪贴板尚未接通".into();
            }
        }
        AdminEffect::RequestOpenDialog => app.shell.input.queue_plugin_dialog(),
        AdminEffect::RequestSaveDialog { .. } => app.shell.input.queue_save_dialog(),
        AdminEffect::LoadSettingsBundle => load_bundle(app, context),
        AdminEffect::Install(path) => install(app, context, path),
        AdminEffect::DeletePlugin(plugin_id) => delete_plugin(app, context, plugin_id),
        AdminEffect::SetEnabled { plugin_id, enabled } => set_enabled(app, context, plugin_id, enabled),
        AdminEffect::SetConfig { plugin_id, key, value } => set_config(app, context, plugin_id, key, value),
        AdminEffect::DeleteConfig { plugin_id, key } => delete_config(app, context, plugin_id, key),
        AdminEffect::LoadConfig(plugin_id) => load_config(app, context, plugin_id),
        AdminEffect::OpenDataDirectory { plugin_id, name } => open_directory(app, context, plugin_id, name),
        AdminEffect::LoadActions { repo_id } => load_actions(app, context, repo_id),
        AdminEffect::RunAction { repo_id, action_id, paths } => run_action(app, context, repo_id, action_id, paths),
        AdminEffect::ReloadBrowser { repo_id } => reload_browser(app, context, repo_id),
        AdminEffect::WriteFile { path, bytes } => write_file(app, context, path, bytes),
    }
}

fn load_bundle(app: &mut MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>) {
    let Some(services) = app.services.as_ref() else {
        eprintln!("Nana 设置页数据需要领域服务，当前服务未启动");
        app.shell.reduce(admin(failed_bundle("领域服务未启动")));
        return;
    };
    let plugin = services.plugin.clone();
    let executor = services.executor.clone();
    if let Err(error) = context.run_task(Task::new(async move {
        let plugins = executor.block_on(plugin.list_plugins());
        let hooks = executor.block_on(plugin.list_plugin_hook_executions(Some(PluginHookExecutionListRequest {
            plugin_id: None,
            limit: Some(200),
        })));
        let cache = executor.block_on(plugin.get_cache_snapshot());
        let api = executor.block_on(plugin.get_api_design_snapshot());
        admin(AdminMessage::SettingsBundleLoaded {
            plugins,
            hooks: hooks.map(|response| response.records),
            cache,
            api,
        })
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
        api: Err(error),
    }
}

fn install(app: &mut MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>, path: String) {
    let Some(services) = app.services.as_ref() else {
        eprintln!("Nana 插件安装需要领域服务，当前服务未启动");
        app.shell.reduce(admin(AdminMessage::PluginsReplaced(Err("领域服务未启动".into()))));
        return;
    };
    let plugin = services.plugin.clone();
    let executor = services.executor.clone();
    if let Err(error) = context.run_task(Task::new(async move {
        let result = executor.block_on(plugin.install_plugin_from_archive(PluginInstallRequest { package_path: path }));
        admin(AdminMessage::PluginsReplaced(result.map(|response| response.plugins)))
    })) {
        eprintln!("Nana 插件安装任务提交失败：{error}");
        app.shell.reduce(admin(AdminMessage::PluginsReplaced(Err(format!("插件安装任务提交失败：{error}")))));
    }
}

fn delete_plugin(app: &mut MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>, plugin_id: String) {
    let Some(services) = app.services.as_ref() else {
        eprintln!("Nana 插件删除需要领域服务，当前服务未启动");
        app.shell.reduce(admin(AdminMessage::PluginsReplaced(Err("领域服务未启动".into()))));
        return;
    };
    let plugin = services.plugin.clone();
    let executor = services.executor.clone();
    if let Err(error) = context.run_task(Task::new(async move {
        let result = executor.block_on(plugin.delete_plugin(plugin_id));
        admin(AdminMessage::PluginsReplaced(result.map(|response| response.plugins)))
    })) {
        eprintln!("Nana 插件删除任务提交失败：{error}");
        app.shell.reduce(admin(AdminMessage::PluginsReplaced(Err(format!("插件删除任务提交失败：{error}")))));
    }
}

fn set_enabled(app: &mut MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>, plugin_id: String, enabled: bool) {
    let Some(services) = app.services.as_ref() else {
        eprintln!("Nana 插件启停需要领域服务，当前服务未启动");
        app.shell.reduce(admin(AdminMessage::PluginsReplaced(Err("领域服务未启动".into()))));
        return;
    };
    let plugin = services.plugin.clone();
    let executor = services.executor.clone();
    if let Err(error) = context.run_task(Task::new(async move {
        let result = executor.block_on(plugin.set_plugin_enabled(PluginEnabledRequest { plugin_id, enabled }));
        admin(AdminMessage::PluginsReplaced(result.map(|response| response.plugins)))
    })) {
        eprintln!("Nana 插件启停任务提交失败：{error}");
        app.shell.reduce(admin(AdminMessage::PluginsReplaced(Err(format!("插件启停任务提交失败：{error}")))));
    }
}

fn set_config(app: &mut MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>, plugin_id: String, key: String, value: serde_json::Value) {
    let Some(services) = app.services.as_ref() else {
        eprintln!("Nana 插件设置保存需要领域服务，当前服务未启动");
        app.shell.reduce(ShellMessage::PluginConfigLoaded(Err("领域服务未启动".into())));
        return;
    };
    let plugin = services.plugin.clone();
    let executor = services.executor.clone();
    if let Err(error) = context.run_task(Task::new(async move {
        ShellMessage::PluginConfigLoaded(executor.block_on(plugin.set_plugin_config_value(PluginConfigSetRequest { plugin_id, key, value })))
    })) {
        eprintln!("Nana 插件设置保存任务提交失败：{error}");
        app.shell.reduce(ShellMessage::PluginConfigLoaded(Err(format!("插件设置保存任务提交失败：{error}"))));
    }
}

fn delete_config(app: &mut MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>, plugin_id: String, key: String) {
    let Some(services) = app.services.as_ref() else {
        eprintln!("Nana 插件设置重置需要领域服务，当前服务未启动");
        app.shell.reduce(ShellMessage::PluginConfigLoaded(Err("领域服务未启动".into())));
        return;
    };
    let plugin = services.plugin.clone();
    let executor = services.executor.clone();
    if let Err(error) = context.run_task(Task::new(async move {
        ShellMessage::PluginConfigLoaded(executor.block_on(plugin.delete_plugin_config_value(PluginConfigDeleteRequest { plugin_id, key })))
    })) {
        eprintln!("Nana 插件设置重置任务提交失败：{error}");
        app.shell.reduce(ShellMessage::PluginConfigLoaded(Err(format!("插件设置重置任务提交失败：{error}"))));
    }
}

fn load_config(app: &mut MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>, plugin_id: String) {
    let Some(services) = app.services.as_ref() else {
        eprintln!("Nana 插件设置读取需要领域服务，当前服务未启动");
        app.shell.reduce(ShellMessage::PluginConfigLoaded(Err("领域服务未启动".into())));
        return;
    };
    let plugin = services.plugin.clone();
    let executor = services.executor.clone();
    if let Err(error) = context.run_task(Task::new(async move {
        ShellMessage::PluginConfigLoaded(executor.block_on(plugin.get_plugin_config(plugin_id)))
    })) {
        eprintln!("Nana 插件设置读取任务提交失败：{error}");
        app.shell.reduce(ShellMessage::PluginConfigLoaded(Err(format!("插件设置读取任务提交失败：{error}"))));
    }
}

fn open_directory(app: &mut MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>, plugin_id: String, name: String) {
    let Some(services) = app.services.as_ref() else {
        eprintln!("Nana 插件设置目录需要领域服务，当前服务未启动");
        app.shell.reduce(admin(AdminMessage::DataDirectoryFinished { name, result: Err("领域服务未启动".into()) }));
        return;
    };
    let plugin = services.plugin.clone();
    let executor = services.executor.clone();
    if let Err(error) = context.run_task(Task::new(async move {
        let result = executor.block_on(plugin.get_plugin_data_directory(plugin_id)).map(|response| response.path);
        admin(AdminMessage::DataDirectoryFinished { name, result })
    })) {
        eprintln!("Nana 插件设置目录任务提交失败：{error}");
        app.shell.reduce(admin(AdminMessage::DataDirectoryFinished { name: String::new(), result: Err(format!("插件设置目录任务提交失败：{error}")) }));
    }
}

fn load_actions(app: &mut MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>, repo_id: String) {
    let Some(services) = app.services.as_ref() else {
        eprintln!("Nana 仓库动作需要领域服务，当前服务未启动");
        app.shell.reduce(admin(AdminMessage::ActionsLoaded { repo_id, result: Err("领域服务未启动".into()) }));
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
        app.shell.reduce(admin(AdminMessage::ActionRunFinished { result: Err("领域服务未启动".into()) }));
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
        app.shell.reduce(admin(AdminMessage::WriteFinished(Err("领域服务未启动".into()))));
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
