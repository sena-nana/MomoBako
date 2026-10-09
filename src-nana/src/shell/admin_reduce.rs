//! 设置、插件、日志、任务和来源登录的消息归约。
//!
//! 规则照 Vue 对应组件：插件操作的提示文案、字段规范化、日志筛选、任务弹层的开关、
//! 主题和圆角的即时保存。只改状态并排副作用，真正的服务调用在 `admin_dispatch`。

use crate::backend::services::repository::{PluginConfigSnapshot, PluginManifest, RepositoryAction};

use super::super::status::FailureSource;
use super::super::workspace_refresh::SilentMessage;
use super::super::{ShellMessage, ShellPage, ShellViewModel, WorkspacePanel};
use super::support::{self, FieldChange};
use super::{source_provision as flow, AdminEffect, AdminMessage, PluginCallOrigin};

/// 处理管理消息，并吃掉壳层里属于设置、插件、日志和任务的分支。
pub(crate) fn reduce_message(model: &mut ShellViewModel, message: ShellMessage) -> Option<ShellMessage> {
    match message {
        ShellMessage::Admin(message) => {
            reduce_admin(model, message);
            None
        }
        ShellMessage::OpenSettings => {
            open_settings_page(model, None);
            None
        }
        ShellMessage::SetWorkspacePanel(panel) => {
            if panel == WorkspacePanel::Actions {
                model.admin.queue_actions(model.repository_id.clone());
            }
            if panel == WorkspacePanel::Logs {
                model.admin.begin_logs_load();
            }
            if panel == WorkspacePanel::Extensions && model.admin.plugins.is_empty() && !model.admin.loading_settings {
                model.admin.begin_settings_load();
            }
            Some(ShellMessage::SetWorkspacePanel(panel))
        }
        ShellMessage::SelectWorkspaceRepository(repo_id) => {
            if model.workspace.panel == WorkspacePanel::Actions {
                model.admin.queue_actions(Some(repo_id.clone()));
            }
            Some(ShellMessage::SelectWorkspaceRepository(repo_id))
        }
        ShellMessage::RepositoriesLoaded(Ok(items)) => {
            model.admin.note_backends(&items);
            Some(ShellMessage::RepositoriesLoaded(Ok(items)))
        }
        ShellMessage::WorkspaceListLoaded { generation, result: Ok(items) } => {
            model.admin.note_backends(&items);
            Some(ShellMessage::WorkspaceListLoaded { generation, result: Ok(items) })
        }
        ShellMessage::SilentWorkspace(SilentMessage::Repositories(Ok(items))) => {
            model.admin.note_backends(&items);
            Some(ShellMessage::SilentWorkspace(SilentMessage::Repositories(Ok(items))))
        }
        other => consume_legacy(model, other),
    }
}

fn consume_legacy(model: &mut ShellViewModel, message: ShellMessage) -> Option<ShellMessage> {
    match message {
        ShellMessage::PluginConfigLoaded(Ok(config)) => {
            apply_config(model, &config);
            model.admin.apply_success();
            model.admin.managing = false;
            None
        }
        ShellMessage::PluginConfigLoaded(Err(error)) => {
            eprintln!("Nana 插件设置读取或保存失败：{error}");
            model.admin.managing = false;
            model.admin.apply_failure(&error);
            None
        }
        ShellMessage::LogsLoaded(Ok(page)) => {
            model.admin.logs_loading = false;
            model.admin.replace_logs(page.records);
            None
        }
        ShellMessage::LogsLoaded(Err(error)) => {
            eprintln!("Nana 系统日志读取失败：{error}");
            model.admin.logs_loading = false;
            model.status.fail(FailureSource::Logs, format!("无法读取系统日志：{error}"));
            model.detail = format!("无法读取系统日志：{error}");
            None
        }
        ShellMessage::ClearLogs => {
            model.detail = "正在清理系统日志…".into();
            None
        }
        ShellMessage::TaskProgressLoaded(progress) => {
            model.task_progress = progress;
            None
        }
        ShellMessage::SettingsLoaded(Ok((settings, diagnostic))) => {
            model.settings = settings;
            model.detail = diagnostic.clone().unwrap_or_else(|| "应用设置已加载".into());
            model.settings_error = diagnostic;
            None
        }
        ShellMessage::SettingsLoaded(Err(error)) => {
            eprintln!("Nana 应用设置读取失败：{error}");
            model.status.fail(FailureSource::Settings, format!("无法读取应用设置：{error}"));
            model.detail = format!("无法读取应用设置：{error}");
            model.settings_error = Some(error);
            None
        }
        ShellMessage::SettingsThemeChanged(theme) => {
            if theme != "light" && theme != "dark" && theme != "system" {
                eprintln!("Nana 忽略未知主题：{theme}");
                return None;
            }
            model.settings.theme = theme;
            model.settings_error = None;
            model.admin.push_effect(AdminEffect::SaveSettings);
            None
        }
        // 主题改动后宿主直接写设置文件，这里只收结果；结果不切页面。
        ShellMessage::SettingsSaved(Ok(settings)) => {
            model.settings = settings;
            model.settings_error = None;
            model.detail = "应用设置已保存".into();
            None
        }
        ShellMessage::SettingsSaved(Err(error)) => {
            eprintln!("Nana 应用设置保存失败：{error}");
            model.status.fail(FailureSource::Settings, format!("保存应用设置失败：{error}"));
            model.detail = format!("设置校验失败：{error}");
            model.settings_error = Some(error);
            None
        }
        ShellMessage::CancelTask(task_id) => {
            model.detail = format!("已请求取消任务 {task_id}");
            None
        }
        other => Some(other),
    }
}

fn reduce_admin(model: &mut ShellViewModel, message: AdminMessage) {
    match message {
        AdminMessage::SetKeyword(value) => model.admin.keyword = value,
        AdminMessage::SetEnabled { plugin_id, enabled } => {
            model.admin.reset_action();
            model.admin.managing = true;
            model.admin.remember_outcome(if enabled { "插件已启用。" } else { "插件已禁用。" }, "插件状态更新失败。");
            model.admin.push_effect(AdminEffect::SetEnabled { plugin_id, enabled });
        }
        AdminMessage::RequestDelete(plugin_id) => request_delete(model, &plugin_id),
        AdminMessage::CancelDelete => model.admin.pending_delete = None,
        AdminMessage::ConfirmDelete => confirm_delete(model),
        AdminMessage::ToggleSettings(plugin_id) => open_settings(model, &plugin_id),
        AdminMessage::RoutePlugin(plugin_id) => open_route(model, &plugin_id),
        AdminMessage::ConfigInput { plugin_id, key, text, checked } => apply_field(model, &plugin_id, &key, &text, checked),
        AdminMessage::FieldDraft { plugin_id, key, value } => {
            model.admin.field_drafts.entry(plugin_id).or_default().insert(key, value);
        }
        AdminMessage::JsonDraft { plugin_id, key, value } => {
            model.admin.json_drafts.entry(plugin_id).or_default().insert(key, value);
        }
        AdminMessage::SaveJson { plugin_id, key } => save_json(model, &plugin_id, &key),
        AdminMessage::ResetConfig { plugin_id, key } => reset_config(model, &plugin_id, &key),
        AdminMessage::RefreshPlugins => {
            model.admin.reset_action();
            model.admin.begin_settings_load();
        }
        AdminMessage::ChooseArchive => {
            model.admin.reset_action();
            model.admin.push_effect(AdminEffect::RequestOpenDialog);
        }
        AdminMessage::InstallArchive(path) => install_archive(model, path),
        AdminMessage::PluginsReplaced(result) => replace_plugins(model, result),
        AdminMessage::OpenDataDirectory(plugin_id) => {
            model.admin.reset_action();
            model.admin.managing = true;
            let name = model
                .admin
                .plugins
                .iter()
                .find(|plugin| plugin.plugin_id == plugin_id)
                .map(|plugin| plugin.name.clone())
                .unwrap_or_else(|| plugin_id.clone());
            model.admin.push_effect(AdminEffect::OpenDataDirectory { plugin_id, name });
        }
        AdminMessage::DataDirectoryFinished { name, result } => finish_directory(model, &name, result),
        AdminMessage::HooksLoaded(Ok(records)) => model.admin.hook_executions = records,
        AdminMessage::HooksLoaded(Err(error)) => {
            eprintln!("Nana 插件钩子记录读取失败：{error}");
            model.admin.load_error = error;
        }
        AdminMessage::CacheLoaded(Ok(snapshot)) => model.admin.cache = Some(snapshot),
        AdminMessage::CacheLoaded(Err(error)) => eprintln!("Nana 缓存快照读取失败：{error}"),
        AdminMessage::ApiDesignLoaded(Ok(snapshot)) => model.admin.api_design = Some(snapshot),
        AdminMessage::ApiDesignLoaded(Err(error)) => eprintln!("Nana API 设计快照读取失败：{error}"),
        AdminMessage::SettingsBundleLoaded { plugins, hooks, cache, api, external } => apply_bundle(model, plugins, hooks, cache, api, external),
        AdminMessage::SetCornerStyle(style) => set_corner_style(model, &style),
        AdminMessage::SetCornerRadius(value) => set_corner_radius(model, &value),
        AdminMessage::CopyExternal { label, value } => copy_external(model, &label, &value),
        AdminMessage::ExportExternal => export_external(model),
        AdminMessage::CompleteExport(path) => complete_export(model, path),
        AdminMessage::WriteFinished(Ok(())) => {
            model.admin.external_error.clear();
            model.admin.external_message = "external-api.json 已导出。".into();
        }
        AdminMessage::WriteFinished(Err(error)) => {
            eprintln!("Nana 外部连接导出失败：{error}");
            model.admin.external_message.clear();
            model.admin.external_error = format!("导出失败：{error}");
        }
        AdminMessage::SelectRepository(repo_id) => {
            let repo_id = repo_id.trim();
            if !repo_id.is_empty() {
                model.reduce(ShellMessage::SelectWorkspaceRepository(repo_id.to_string()));
            }
        }
        AdminMessage::SetAudioPlayer(plugin_id) => model.set_audio_preference(plugin_id),
        AdminMessage::ToggleLogLevel(level) => {
            model.admin.log_levels = support::toggle_value(&model.admin.log_levels, &level);
            model.admin.note_log_scroll();
        }
        AdminMessage::ToggleLogKind(kind) => {
            model.admin.log_kinds = support::toggle_value(&model.admin.log_kinds, &kind);
            model.admin.note_log_scroll();
        }
        AdminMessage::SetLogPlugin(plugin_id) => {
            model.admin.log_plugin_id = plugin_id;
            model.admin.note_log_scroll();
        }
        AdminMessage::SetLogRepo(repo_id) => {
            model.admin.log_repo_id = repo_id;
            model.admin.note_log_scroll();
        }
        AdminMessage::SetLogSearch(value) => {
            model.admin.log_search = value;
            model.admin.note_log_scroll();
        }
        AdminMessage::ResetLogFilters => {
            model.admin.log_levels.clear();
            model.admin.log_kinds.clear();
            model.admin.log_plugin_id.clear();
            model.admin.log_repo_id.clear();
            model.admin.log_search.clear();
            model.admin.note_log_scroll();
        }
        AdminMessage::SetLogPaused(paused) => {
            model.admin.log_paused = paused;
            if paused {
                model.admin.log_would_scroll = false;
            }
        }
        AdminMessage::ToggleLogContext(id) => {
            if !model.admin.log_context_open.remove(&id) {
                model.admin.log_context_open.insert(id);
            }
        }
        AdminMessage::ToggleTaskPopover => model.admin.popover_open = !model.admin.popover_open,
        AdminMessage::CloseTaskPopover | AdminMessage::TaskUnmount => model.admin.popover_open = false,
        AdminMessage::TaskEscape => model.admin.popover_open = false,
        AdminMessage::TaskOutside { inside } => {
            if !inside {
                model.admin.popover_open = false;
            }
        }
        AdminMessage::SetOperation(operation) => model.admin.operation = operation,
        AdminMessage::SelectAction(action_id) => {
            model.admin.active_action_id = Some(action_id);
            model.workspace.panel = WorkspacePanel::Actions;
        }
        AdminMessage::RunAction(action_id) => run_action(model, action_id),
        AdminMessage::ActionsLoaded { repo_id, result } => apply_actions(model, &repo_id, result),
        AdminMessage::ActionRunFinished { result } => finish_action(model, result),
        AdminMessage::SetToolPages(pages) => {
            model.admin.active_tool_page_id = support::apply_tool_page_selection(&pages, model.admin.active_tool_page_id.as_deref());
            model.admin.tool_pages = pages;
        }
        AdminMessage::SelectToolPage(page_id) => {
            if model.admin.tool_pages.iter().any(|page| page.id == page_id) {
                model.admin.active_tool_page_id = Some(page_id);
            }
        }
        AdminMessage::Api(message) => super::api::reduce(model, message),
        AdminMessage::CallFilePlugin { plugin_id, method, payload, repository_id } => {
            if plugin_id.trim().is_empty() || method.trim().is_empty() {
                eprintln!("Nana 文件插件动作缺少方法");
                return;
            }
            model.admin.push_effect(AdminEffect::CallPlugin { plugin_id, method, payload, repository_id, origin: PluginCallOrigin::FileMenu });
        }
        AdminMessage::FilePluginFinished { method, result } => finish_file_plugin(model, &method, result),
        AdminMessage::BeginSourceAuth { plugin_id, repo_id } => flow::begin(model, &plugin_id, repo_id),
        AdminMessage::CheckSourceAuth { plugin_id, repo_id } => flow::refresh_status(model, &plugin_id, &repo_id),
        AdminMessage::ClearSourceAuth { plugin_id, repo_id } => flow::clear(model, &plugin_id, &repo_id),
        AdminMessage::CancelSourceAuth => flow::cancel(model),
        AdminMessage::PollSourceAuth { plugin_id } => flow::poll(model, &plugin_id),
        AdminMessage::ChooseSourceCache => {
            if !model.admin.source_auth.busy {
                model.input.queue_source_cache_dialog();
            }
        }
        AdminMessage::SetSourceCachePath(path) => {
            let path = path.trim().to_string();
            if !path.is_empty() {
                model.admin.source_auth.cache_path = path;
            }
        }
        AdminMessage::SourceStepFinished { step, result } => flow::finish(model, step, result),
    }
}

/// 插件列表换新：重算筛选栏的库类型快捷方式，再重读插件登记的播放器类型。
/// Vue 每次拿到新列表都同步前端插件注册表（`syncPreviewPlugins`），播放器跟着换。
fn publish_plugins(model: &mut ShellViewModel, plugins: &[PluginManifest]) {
    model.inspect.shortcuts = super::super::inspect_shortcuts::shortcuts_from_plugins(plugins);
    model.admin.push_effect(AdminEffect::LoadPlaylistPlayers);
}

fn apply_config(model: &mut ShellViewModel, config: &PluginConfigSnapshot) {
    model.admin.config_snapshots.insert(config.plugin_id.clone(), config.clone());
    model.admin.sync_json_drafts(&config.plugin_id);
}

/// 打开设置页：读设置包（Vue `Settings.vue` 挂载时的 `loadSettingsData`）和应用设置。
/// 带插件时照 Vue 路由的 `?plugin=` 展开它的设置，插件列表里还没有它时不展开。
pub(crate) fn open_settings_page(model: &mut ShellViewModel, plugin_id: Option<&str>) {
    model.page = ShellPage::Settings;
    model.admin.begin_settings_load();
    model.admin.push_effect(AdminEffect::LoadAppSettings);
    if let Some(plugin_id) = plugin_id {
        open_route(model, plugin_id);
    }
}

fn request_delete(model: &mut ShellViewModel, plugin_id: &str) {
    let Some(plugin) = model.admin.plugins.iter().find(|plugin| plugin.plugin_id == plugin_id).cloned() else {
        eprintln!("Nana 找不到要删除的插件：{plugin_id}");
        return;
    };
    if !support::can_delete_plugin(&plugin) {
        return;
    }
    model.admin.pending_delete = Some(plugin.plugin_id);
}

/// 确认删除。成功后对话框关闭；失败时对话框保留，提示写在面板上。
fn confirm_delete(model: &mut ShellViewModel) {
    let Some(plugin_id) = model.admin.pending_delete.clone() else {
        return;
    };
    model.admin.reset_action();
    model.admin.managing = true;
    model.admin.remember_outcome("插件已删除。", "插件删除失败。");
    model.admin.push_effect(AdminEffect::DeletePlugin(plugin_id));
}

/// 打开或收起插件设置。打开时读一次配置；换插件时来源登录状态重置。
fn open_settings(model: &mut ShellViewModel, plugin_id: &str) {
    model.admin.reset_action();
    if model.admin.active_settings_plugin_id.as_deref() == Some(plugin_id) {
        model.admin.active_settings_plugin_id = None;
        flow::bind_plugin(model, None);
        return;
    }
    model.admin.active_settings_plugin_id = Some(plugin_id.to_string());
    flow::bind_plugin(model, Some(plugin_id));
    model.admin.managing = true;
    model.admin.pending_notice = None;
    model.admin.pending_failure = Some("插件设置读取失败。".into());
    model.admin.push_effect(AdminEffect::LoadConfig(plugin_id.to_string()));
}

/// 路由带插件标识时打开它的设置；已经打开并有快照就不重复读。
fn open_route(model: &mut ShellViewModel, plugin_id: &str) {
    let plugin_id = plugin_id.trim();
    if plugin_id.is_empty() || !model.admin.plugins.iter().any(|plugin| plugin.plugin_id == plugin_id) {
        return;
    }
    if model.admin.active_settings_plugin_id.as_deref() == Some(plugin_id) && model.admin.config_snapshots.contains_key(plugin_id) {
        return;
    }
    open_settings(model, plugin_id);
}

fn field_of(model: &ShellViewModel, plugin_id: &str, key: &str) -> Option<support::ConfigField> {
    let plugin = model.admin.plugins.iter().find(|plugin| plugin.plugin_id == plugin_id)?;
    support::settings_fields(plugin).into_iter().find(|field| field.key == key)
}

fn apply_field(model: &mut ShellViewModel, plugin_id: &str, key: &str, text: &str, checked: Option<bool>) {
    let Some(field) = field_of(model, plugin_id, key) else {
        eprintln!("Nana 插件 {plugin_id} 没有配置字段 {key}");
        return;
    };
    model.admin.reset_action();
    match support::normalize_field_input(&field, text, checked) {
        FieldChange::Reset => reset_config(model, plugin_id, key),
        FieldChange::Invalid(error) => model.admin.action_error = error,
        FieldChange::Set(value) => {
            model.admin.managing = true;
            model.admin.remember_outcome("插件设置已保存。", "插件设置保存失败。");
            model.admin.push_effect(AdminEffect::SetConfig { plugin_id: plugin_id.to_string(), key: key.to_string(), value });
        }
    }
}

fn reset_config(model: &mut ShellViewModel, plugin_id: &str, key: &str) {
    model.admin.reset_action();
    model.admin.managing = true;
    model.admin.remember_outcome("插件设置已重置。", "插件设置重置失败。");
    model.admin.push_effect(AdminEffect::DeleteConfig { plugin_id: plugin_id.to_string(), key: key.to_string() });
}

fn save_json(model: &mut ShellViewModel, plugin_id: &str, key: &str) {
    let Some(field) = field_of(model, plugin_id, key) else {
        eprintln!("Nana 插件 {plugin_id} 没有配置字段 {key}");
        return;
    };
    if field.field_type != "json" {
        eprintln!("Nana 插件字段 {key} 不是 JSON");
        return;
    }
    let draft = model.admin.json_drafts.get(plugin_id).and_then(|drafts| drafts.get(key)).cloned().unwrap_or_default();
    model.admin.reset_action();
    match support::parse_json_draft(&field.label, &draft) {
        Ok(value) => {
            model.admin.managing = true;
            model.admin.remember_outcome("插件设置已保存。", "插件设置保存失败。");
            model.admin.push_effect(AdminEffect::SetConfig { plugin_id: plugin_id.to_string(), key: key.to_string(), value });
        }
        Err(error) => model.admin.action_error = error,
    }
}

fn install_archive(model: &mut ShellViewModel, path: Option<String>) {
    let Some(path) = path.filter(|path| !path.trim().is_empty()) else {
        return;
    };
    model.admin.reset_action();
    model.admin.managing = true;
    model.admin.remember_outcome("插件已安装。", "插件安装失败。");
    model.admin.push_effect(AdminEffect::Install(path));
}

/// 安装、删除、启停之后的新列表。删除成功时收起确认框。
fn replace_plugins(model: &mut ShellViewModel, result: Result<Vec<PluginManifest>, String>) {
    model.admin.managing = false;
    match result {
        Ok(plugins) => {
            publish_plugins(model, &plugins);
            model.admin.store_plugins(plugins);
            model.admin.pending_delete = None;
            model.admin.apply_success();
        }
        Err(error) => {
            eprintln!("Nana 插件操作失败：{error}");
            model.admin.apply_failure(&error);
        }
    }
}

fn finish_directory(model: &mut ShellViewModel, name: &str, result: Result<String, String>) {
    model.admin.managing = false;
    match result {
        Ok(path) => {
            model.input.host_requests.push(crate::host_api::HostRequest::OpenExternal(
                crate::host_api::ExternalOpenRequest { target: path, reveal: true },
            ));
            model.admin.action_error.clear();
            model.admin.action_message = format!("已打开“{name}”设置目录。");
        }
        Err(error) => {
            eprintln!("Nana 插件设置目录读取失败：{error}");
            model.admin.action_error = if error.is_empty() { "插件设置目录打开失败。".into() } else { error };
        }
    }
}

/// 设置页五份数据。任何一份失败整批不写，只记下错误，和 Vue 的 `Promise.all` 一致。
fn apply_bundle(
    model: &mut ShellViewModel,
    plugins: Result<Vec<PluginManifest>, String>,
    hooks: Result<Vec<crate::backend::services::repository::PluginHookExecutionRecord>, String>,
    cache: Result<crate::backend::services::repository::CacheSnapshot, String>,
    api: Result<crate::backend::services::repository::ApiDesignSnapshot, String>,
    external: Result<crate::backend::services::runtime::ExternalApiConnectionStatus, String>,
) {
    model.admin.loading_settings = false;
    match (plugins, hooks, cache, api, external) {
        (Ok(plugins), Ok(hooks), Ok(cache), Ok(api), Ok(external)) => {
            publish_plugins(model, &plugins);
            model.admin.store_plugins(plugins);
            model.admin.hook_executions = hooks;
            model.admin.cache = Some(cache);
            model.admin.api_design = Some(api);
            model.admin.external = Some(external);
            model.admin.load_error.clear();
            // API Playground 挂载时选中第一个端点并带出方法、目标和请求体，契约换新后同样重选。
            let _ = super::api::selected(model);
        }
        (plugins, hooks, cache, api, external) => {
            let error = [plugins.err(), hooks.err(), cache.err(), api.err(), external.err()].into_iter().flatten().next().unwrap_or_default();
            eprintln!("Nana 设置页数据读取失败：{error}");
            model.admin.load_error = error;
        }
    }
}

fn set_corner_style(model: &mut ShellViewModel, style: &str) {
    if style != "smooth" && style != "round" {
        eprintln!("Nana 忽略未知圆角样式：{style}");
        return;
    }
    model.admin.corner_style = style.to_string();
    model.admin.push_effect(AdminEffect::PersistCorners);
}

fn set_corner_radius(model: &mut ShellViewModel, value: &str) {
    let Ok(parsed) = value.trim().parse::<f64>() else {
        eprintln!("Nana 圆角半径不是数字：{value}");
        return;
    };
    let Some(radius) = support::clamp_radius(parsed) else {
        eprintln!("Nana 圆角半径不是有限数字：{value}");
        return;
    };
    model.admin.corner_radius = radius;
    model.admin.push_effect(AdminEffect::PersistCorners);
}

fn copy_external(model: &mut ShellViewModel, label: &str, value: &str) {
    model.admin.external_message.clear();
    model.admin.external_error.clear();
    if value.is_empty() {
        model.admin.external_error = format!("{label} 尚未加载。");
        return;
    }
    model.admin.push_effect(AdminEffect::CopyText(value.to_string()));
    model.admin.external_message = format!("{label} 已复制。");
}

fn export_external(model: &mut ShellViewModel) {
    let content = external_json(model);
    model.admin.external_message.clear();
    model.admin.external_error.clear();
    if content.is_empty() {
        model.admin.external_error = "连接 JSON 尚未加载。".into();
        return;
    }
    model.admin.push_effect(AdminEffect::RequestSaveDialog { content });
}

fn complete_export(model: &mut ShellViewModel, path: Option<String>) {
    let Some(path) = path.filter(|path| !path.trim().is_empty()) else {
        return;
    };
    let content = external_json(model);
    if content.is_empty() {
        model.admin.external_error = "连接 JSON 尚未加载。".into();
        return;
    }
    model.admin.push_effect(AdminEffect::WriteFile { path, bytes: content.into_bytes() });
}

fn external_json(model: &ShellViewModel) -> String {
    let Some(connection) = &model.admin.external else {
        return String::new();
    };
    support::connection_json(&connection.base_url, &connection.token, &connection.version, &connection.started_at)
}

fn finish_file_plugin(model: &mut ShellViewModel, method: &str, result: Result<serde_json::Value, String>) {
    match result {
        Ok(_) => {
            model.files.error.clear();
            model.files.activity = format!("已调用 {method}。");
        }
        Err(error) => {
            eprintln!("Nana 文件插件动作失败：{method}：{error}");
            model.files.error = if error.is_empty() { format!("{method} 调用失败。") } else { error };
        }
    }
}

fn selected_action<'a>(model: &'a ShellViewModel, action_id: &Option<String>) -> Option<&'a RepositoryAction> {
    let requested = action_id.as_deref().or(model.admin.active_action_id.as_deref());
    model.admin.actions.iter().find(|action| Some(action.action_id.as_str()) == requested).or(model.admin.actions.first())
}

fn run_action(model: &mut ShellViewModel, action_id: Option<String>) {
    let Some(repo_id) = model.repository_id.clone() else {
        eprintln!("Nana 没有活动仓库，不能执行仓库动作");
        return;
    };
    let Some(action) = selected_action(model, &action_id).cloned() else {
        eprintln!("Nana 没有可执行的仓库动作");
        return;
    };
    let selected = model.files.selected_paths().len();
    if !support::action_can_run(&action.status, action.enabled, selected, model.admin.actions_running) {
        eprintln!("Nana 仓库动作当前不能执行：{}", action.action_id);
        return;
    }
    let paths = model.files.selected_paths().to_vec();
    model.admin.actions_running = true;
    model.admin.actions_error.clear();
    model.admin.push_effect(AdminEffect::RunAction { repo_id, action_id: action.action_id, paths });
}

fn apply_actions(model: &mut ShellViewModel, repo_id: &str, result: Result<Vec<RepositoryAction>, String>) {
    if model.admin.actions_repo_id.as_deref() != Some(repo_id) {
        eprintln!("Nana 忽略过期的仓库动作：{repo_id}");
        return;
    }
    model.admin.actions_loading = false;
    if model.repository_id.as_deref() != Some(repo_id) {
        eprintln!("Nana 忽略不属于当前仓库的动作列表：{repo_id}");
        return;
    }
    match result {
        Ok(actions) => {
            if model.admin.active_action_id.as_ref().is_some_and(|id| !actions.iter().any(|action| &action.action_id == id)) {
                model.admin.active_action_id = None;
            }
            model.admin.active_action_id = model.admin.active_action_id.clone().or_else(|| actions.first().map(|action| action.action_id.clone()));
            model.admin.actions = actions;
        }
        Err(error) => {
            eprintln!("Nana 仓库动作读取失败：{error}");
            model.admin.actions_error = error;
        }
    }
}

fn finish_action(model: &mut ShellViewModel, result: Result<RepositoryAction, String>) {
    model.admin.actions_running = false;
    match result {
        Ok(action) => {
            let repo_id = action.repo_id.clone();
            if let Some(current) = model.admin.actions.iter_mut().find(|item| item.action_id == action.action_id) {
                *current = action;
            }
            model.admin.push_effect(AdminEffect::ReloadBrowser { repo_id });
        }
        Err(error) => {
            eprintln!("Nana 仓库动作执行失败：{error}");
            model.admin.actions_error = error;
        }
    }
}
