//! 设置、插件、日志和任务消息归约。
//!
//! 已经存在的页面文案留在这里，避免壳层匹配再涨过 1000 行。
//! 新分支只改 `admin` 状态；验收场景不读取这些实况字段。

use crate::backend::services::repository::{PluginConfigSnapshot, PluginManifest, RepositoryAction};

use super::super::{ShellMessage, ShellPage, ShellViewModel, WorkspacePanel};
use super::support::{self, FieldChange};
use super::{AdminEffect, AdminMessage};

/// 处理实况管理消息，并吃掉原来写在壳层里的插件、日志、设置和任务分支。
pub(crate) fn reduce_message(model: &mut ShellViewModel, message: ShellMessage) -> Option<ShellMessage> {
    match message {
        ShellMessage::Admin(message) => {
            reduce_admin(model, message);
            None
        }
        ShellMessage::Navigate(ShellPage::Settings) => {
            model.admin.begin_settings_load();
            Some(ShellMessage::Navigate(ShellPage::Settings))
        }
        ShellMessage::SetWorkspacePanel(panel) => {
            if panel == WorkspacePanel::Actions {
                model.admin.queue_actions(model.repository_id.clone());
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
        other => consume_legacy(model, other),
    }
}

fn consume_legacy(model: &mut ShellViewModel, message: ShellMessage) -> Option<ShellMessage> {
    match message {
        ShellMessage::PluginsLoaded(Ok(plugins)) => {
            publish_plugins(model, &plugins, true);
            model.admin.store_plugins(plugins);
            model.admin.managing = false;
            model.admin.apply_success();
            None
        }
        ShellMessage::PluginsLoaded(Err(error)) => {
            model.page = ShellPage::Error;
            model.detail = format!("无法读取插件列表：{error}");
            model.admin.managing = false;
            model.admin.apply_failure(&error);
            None
        }
        ShellMessage::SelectPlugin(plugin_id) => {
            model.page = ShellPage::PluginSettings;
            model.detail = format!("正在读取插件 {plugin_id} 的原生设置…");
            None
        }
        ShellMessage::TogglePlugin { plugin_id, enabled } => {
            model.detail = format!("正在{}插件 {plugin_id}…", if enabled { "启用" } else { "停用" });
            model.admin.reset_action();
            model.admin.remember_outcome(
                if enabled { "插件已启用。" } else { "插件已禁用。" },
                "插件状态更新失败。",
            );
            None
        }
        ShellMessage::DeletePlugin(plugin_id) => {
            model.detail = format!("正在删除插件 {plugin_id}…");
            model.admin.reset_action();
            model.admin.remember_outcome("插件已删除。", "插件删除失败。");
            None
        }
        ShellMessage::PluginConfigLoaded(Ok(config)) => {
            apply_config(model, &config);
            model.admin.apply_success();
            model.admin.managing = false;
            None
        }
        ShellMessage::PluginConfigLoaded(Err(error)) => {
            model.page = ShellPage::Error;
            model.detail = format!("无法读取插件设置：{error}");
            model.admin.managing = false;
            model.admin.apply_failure(&error);
            None
        }
        ShellMessage::DeletePluginConfig { plugin_id, key } => {
            model.detail = format!("正在删除插件 {plugin_id} 的配置 {key}…");
            None
        }
        ShellMessage::PluginConfigDraftChanged { key, value } => {
            model.plugin_config_drafts.insert(key, value);
            None
        }
        ShellMessage::SavePluginConfig { key, .. } => {
            model.detail = format!("正在保存插件配置 {key}…");
            None
        }
        ShellMessage::LogsLoaded(Ok(page)) => {
            model.page = ShellPage::Logs;
            model.admin.logs = page.records.clone();
            model.admin.note_log_scroll();
            model.log_entries = page
                .records
                .iter()
                .map(|record| format!("{} · {} · {}", record.level, record.category, record.message))
                .collect();
            model.detail = format!("最近日志 · {} 条记录", page.records.len());
            None
        }
        ShellMessage::LogsLoaded(Err(error)) => {
            model.page = ShellPage::Error;
            model.detail = format!("无法读取系统日志：{error}");
            None
        }
        ShellMessage::ClearLogs => {
            model.detail = "正在清理系统日志…".into();
            None
        }
        ShellMessage::TaskSnapshotLoaded { active, completed } => {
            model.page = ShellPage::TaskRunning;
            model.active_tasks = active;
            model.completed_tasks = completed;
            model.detail = format!("{active} 个运行中任务 · {completed} 个近期完成任务");
            None
        }
        ShellMessage::TaskProgressLoaded(progress) => {
            model.task_progress = progress;
            if let Some(snapshot) = model.task_progress.iter().find(|snapshot| snapshot.status == "running" || snapshot.status == "cancelling") {
                let label = snapshot.label.clone().unwrap_or_else(|| snapshot.protocol_id.clone());
                let percent = snapshot.percent.map(|value| format!("{value:.0}%")).unwrap_or_else(|| "处理中".into());
                model.detail = format!("{label} · {percent}");
            }
            None
        }
        ShellMessage::SystemStatusLoaded(Ok(status)) => {
            model.page = ShellPage::Settings;
            model.admin.external = Some(status.clone());
            model.system_status = Some(format!("{} · {}", if status.ready { "服务已就绪" } else { "服务未就绪" }, status.base_url));
            model.detail = model.system_status.clone().unwrap_or_default();
            None
        }
        ShellMessage::SystemStatusLoaded(Err(error)) => {
            model.page = ShellPage::Error;
            model.detail = format!("无法读取系统服务状态：{error}");
            None
        }
        ShellMessage::SettingsLoaded(Ok((settings, diagnostic))) => {
            model.page = ShellPage::Settings;
            model.settings_cache_limit_draft = settings.thumbnail_cache_limit_mb.to_string();
            model.settings = settings;
            model.settings_error = diagnostic;
            model.detail = model.settings_error.clone().unwrap_or_else(|| "应用设置已加载".into());
            None
        }
        ShellMessage::SettingsLoaded(Err(error)) => {
            model.page = ShellPage::SettingsError;
            model.settings_error = Some(error.clone());
            model.detail = format!("无法读取应用设置：{error}");
            None
        }
        ShellMessage::SettingsThemeChanged(theme) => {
            model.settings.theme = theme;
            model.settings_error = None;
            None
        }
        ShellMessage::SettingsCacheLimitChanged(value) => {
            model.settings_cache_limit_draft = value.clone();
            model.settings_error = None;
            match value.parse::<u32>() {
                Ok(limit) => model.settings.thumbnail_cache_limit_mb = limit,
                Err(_) => model.settings_error = Some("缩略图缓存上限必须是整数".into()),
            }
            None
        }
        ShellMessage::SettingsPlayerChanged(player) => {
            model.settings.default_playlist_player_type_id = if player.trim().is_empty() { None } else { Some(player) };
            model.settings_error = None;
            None
        }
        ShellMessage::SettingsCloseBehaviorChanged(behavior) => {
            model.settings.close_behavior = behavior;
            model.settings_error = None;
            None
        }
        ShellMessage::SaveSettings => {
            model.detail = "正在保存应用设置…".into();
            None
        }
        ShellMessage::SettingsSaved(Ok(settings)) => {
            model.page = ShellPage::Settings;
            model.settings_cache_limit_draft = settings.thumbnail_cache_limit_mb.to_string();
            model.settings = settings;
            model.settings_error = None;
            model.detail = "应用设置已保存".into();
            None
        }
        ShellMessage::SettingsSaved(Err(error)) => {
            model.page = ShellPage::SettingsError;
            model.settings_error = Some(error.clone());
            model.detail = format!("设置校验失败：{error}");
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
            model.admin.effects.push(AdminEffect::SetEnabled { plugin_id, enabled });
        }
        AdminMessage::RequestDelete(plugin_id) => request_delete(model, &plugin_id),
        AdminMessage::CancelDelete => model.admin.pending_delete = None,
        AdminMessage::ConfirmDelete => confirm_delete(model),
        AdminMessage::ToggleSettings(plugin_id) => open_settings(model, &plugin_id),
        AdminMessage::RoutePlugin(plugin_id) => open_route(model, &plugin_id),
        AdminMessage::ConfigInput { plugin_id, key, text, checked } => apply_field(model, &plugin_id, &key, &text, checked),
        AdminMessage::JsonDraft { plugin_id, key, value } => {
            model.admin.json_drafts.entry(plugin_id).or_default().insert(key, value);
        }
        AdminMessage::SaveJson { plugin_id, key } => save_json(model, &plugin_id, &key),
        AdminMessage::ResetConfig { plugin_id, key } => reset_config(model, &plugin_id, &key),
        AdminMessage::ChooseArchive => {
            model.admin.reset_action();
            model.admin.effects.push(AdminEffect::RequestOpenDialog);
            if support::open_dialog_available() {
                model.admin.action_message = "正在选择插件包…".into();
            }
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
            model.admin.effects.push(AdminEffect::OpenDataDirectory { plugin_id, name });
        }
        AdminMessage::DataDirectoryFinished { name, result } => finish_directory(model, &name, result),
        AdminMessage::HooksLoaded(Ok(records)) => model.admin.hook_executions = records,
        AdminMessage::HooksLoaded(Err(error)) => {
            eprintln!("Nana 插件钩子记录读取失败：{error}");
            model.admin.action_error = error;
        }
        AdminMessage::CacheLoaded(Ok(snapshot)) => model.admin.cache = Some(snapshot),
        AdminMessage::CacheLoaded(Err(error)) => eprintln!("Nana 缓存快照读取失败：{error}"),
        AdminMessage::ApiDesignLoaded(Ok(snapshot)) => model.admin.api_design = Some(snapshot),
        AdminMessage::ApiDesignLoaded(Err(error)) => eprintln!("Nana API 设计快照读取失败：{error}"),
        AdminMessage::SettingsBundleLoaded { plugins, hooks, cache, api } => apply_bundle(model, plugins, hooks, cache, api),
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
            model.admin.external_message.clear();
            model.admin.external_error = format!("导出失败：{error}");
        }
        AdminMessage::SelectRepository(repo_id) => {
            let repo_id = repo_id.trim();
            if repo_id.is_empty() {
                return;
            }
            let repo_id = repo_id.to_string();
            model.reduce(ShellMessage::SelectWorkspaceRepository(repo_id));
        }
        AdminMessage::SetAudioPlayer(plugin_id) => model.set_audio_preference(plugin_id),
        AdminMessage::MarkVueSettings(plugin_id) => {
            model.admin.vue_settings.insert(plugin_id);
        }
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
        AdminMessage::ToggleTaskPopover => model.admin.popover_open = !model.admin.popover_open,
        AdminMessage::CloseTaskPopover | AdminMessage::TaskUnmount => model.admin.popover_open = false,
        AdminMessage::TaskEscape => {
            if model.admin.popover_open {
                model.admin.popover_open = false;
            }
        }
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
    }
}

fn publish_plugins(model: &mut ShellViewModel, plugins: &[PluginManifest], switch_page: bool) {
    if switch_page {
        model.page = ShellPage::PluginSettings;
        model.detail = format!("{} 个插件 · 原生贡献接口优先", plugins.len());
    }
    model.plugin_entries = plugins.iter().map(|plugin| format!("{} {} · {}", plugin.name, plugin.version, plugin.status)).collect();
    model.plugin_entry_ids = plugins.iter().map(|plugin| plugin.plugin_id.clone()).collect();
    model.plugin_enabled = plugins.iter().map(|plugin| plugin.enabled).collect();
}

fn apply_config(model: &mut ShellViewModel, config: &PluginConfigSnapshot) {
    if !matches!(model.page, ShellPage::Settings | ShellPage::SettingsError) {
        model.page = ShellPage::PluginSettings;
    }
    model.selected_plugin_id = Some(config.plugin_id.clone());
    model.plugin_config_keys = config.values.keys().cloned().collect();
    model.plugin_config_drafts = config
        .values
        .iter()
        .map(|(key, value)| (key.clone(), value.as_str().map_or_else(|| value.to_string(), str::to_owned)))
        .collect();
    model.plugin_config_string_values = config.values.iter().filter(|(_, value)| value.is_string()).map(|(key, _)| key.clone()).collect();
    model.detail = format!("插件 {} · 已加载 {} 项配置", config.plugin_id, config.values.len());
    model.admin.config_snapshots.insert(config.plugin_id.clone(), config.clone());
    model.admin.active_settings_plugin_id = Some(config.plugin_id.clone());
    model.admin.sync_json_drafts(&config.plugin_id);
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

fn confirm_delete(model: &mut ShellViewModel) {
    let Some(plugin_id) = model.admin.pending_delete.clone() else {
        return;
    };
    model.admin.reset_action();
    model.admin.pending_delete = None;
    model.admin.managing = true;
    model.admin.remember_outcome("插件已删除。", "插件删除失败。");
    model.detail = format!("正在删除插件 {plugin_id}…");
    model.admin.effects.push(AdminEffect::DeletePlugin(plugin_id));
}

fn open_settings(model: &mut ShellViewModel, plugin_id: &str) {
    model.admin.reset_action();
    if model.admin.active_settings_plugin_id.as_deref() == Some(plugin_id) {
        model.admin.active_settings_plugin_id = None;
        return;
    }
    model.admin.active_settings_plugin_id = Some(plugin_id.to_string());
    model.admin.managing = true;
    model.admin.pending_failure = Some("插件设置读取失败。".into());
    model.admin.effects.push(AdminEffect::LoadConfig(plugin_id.to_string()));
}

fn open_route(model: &mut ShellViewModel, plugin_id: &str) {
    let plugin_id = plugin_id.trim();
    if plugin_id.is_empty() {
        return;
    }
    if !model.admin.plugins.iter().any(|plugin| plugin.plugin_id == plugin_id) {
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
            model.admin.effects.push(AdminEffect::SetConfig { plugin_id: plugin_id.to_string(), key: key.to_string(), value });
        }
    }
}

fn reset_config(model: &mut ShellViewModel, plugin_id: &str, key: &str) {
    model.admin.reset_action();
    model.admin.managing = true;
    model.admin.remember_outcome("插件设置已重置。", "插件设置重置失败。");
    model.detail = format!("正在删除插件 {plugin_id} 的配置 {key}…");
    model.admin.effects.push(AdminEffect::DeleteConfig { plugin_id: plugin_id.to_string(), key: key.to_string() });
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
            model.admin.effects.push(AdminEffect::SetConfig { plugin_id: plugin_id.to_string(), key: key.to_string(), value });
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
    model.admin.effects.push(AdminEffect::Install(path));
}

fn replace_plugins(model: &mut ShellViewModel, result: Result<Vec<PluginManifest>, String>) {
    model.admin.managing = false;
    model.admin.loading_settings = false;
    match result {
        Ok(plugins) => {
            publish_plugins(model, &plugins, false);
            model.admin.store_plugins(plugins);
            model.admin.apply_success();
        }
        Err(error) => model.admin.apply_failure(&error),
    }
}

fn finish_directory(model: &mut ShellViewModel, name: &str, result: Result<String, String>) {
    model.admin.managing = false;
    match result {
        Ok(path) => {
            if support::reveal_directory_available() {
                model.admin.action_message = format!("已打开“{name}”设置目录。");
            } else {
                eprintln!("Nana 宿主目录打开尚未接通：{path}");
                model.admin.action_error = "插件设置目录打开失败。".into();
            }
        }
        Err(error) => {
            eprintln!("Nana 插件设置目录读取失败：{error}");
            model.admin.action_error = if error.is_empty() { "插件设置目录打开失败。".into() } else { error };
        }
    }
}

fn apply_bundle(
    model: &mut ShellViewModel,
    plugins: Result<Vec<PluginManifest>, String>,
    hooks: Result<Vec<crate::backend::services::repository::PluginHookExecutionRecord>, String>,
    cache: Result<crate::backend::services::repository::CacheSnapshot, String>,
    api: Result<crate::backend::services::repository::ApiDesignSnapshot, String>,
) {
    model.admin.loading_settings = false;
    let error = plugins.as_ref().err().or(hooks.as_ref().err()).or(cache.as_ref().err()).or(api.as_ref().err()).cloned();
    if let Some(error) = error {
        eprintln!("Nana 设置页数据读取失败：{error}");
        model.admin.action_error = error;
        return;
    }
    if let (Ok(plugins), Ok(hooks), Ok(cache), Ok(api)) = (plugins, hooks, cache, api) {
        publish_plugins(model, &plugins, false);
        model.admin.store_plugins(plugins);
        model.admin.hook_executions = hooks;
        model.admin.cache = Some(cache);
        model.admin.api_design = Some(api);
    }
}

fn set_corner_style(model: &mut ShellViewModel, style: &str) {
    if style != "smooth" && style != "round" {
        eprintln!("Nana 忽略未知圆角样式：{style}");
        return;
    }
    model.admin.corner_style = style.to_string();
    model.admin.effects.push(AdminEffect::PersistCorners);
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
    model.admin.effects.push(AdminEffect::PersistCorners);
}

fn copy_external(model: &mut ShellViewModel, label: &str, value: &str) {
    model.admin.external_message.clear();
    model.admin.external_error.clear();
    if value.is_empty() {
        model.admin.external_error = format!("{label} 尚未加载。");
        return;
    }
    model.admin.effects.push(AdminEffect::CopyText(value.to_string()));
    if support::clipboard_available() {
        model.admin.external_message = format!("{label} 已复制。");
    } else {
        model.admin.external_error = format!("复制失败：宿主剪贴板尚未接通");
    }
}

fn export_external(model: &mut ShellViewModel) {
    let content = external_json(model);
    model.admin.external_message.clear();
    model.admin.external_error.clear();
    if content.is_empty() {
        model.admin.external_error = "连接 JSON 尚未加载。".into();
        return;
    }
    model.admin.effects.push(AdminEffect::RequestSaveDialog { content });
    if support::save_dialog_available() {
        model.admin.external_message = "正在选择导出位置…".into();
    } else {
        model.admin.external_error = "导出失败：宿主保存对话框尚未接通".into();
    }
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
    model.admin.effects.push(AdminEffect::WriteFile { path, bytes: content.into_bytes() });
}

fn external_json(model: &ShellViewModel) -> String {
    let Some(connection) = &model.admin.external else {
        return String::new();
    };
    support::connection_json(&connection.base_url, &connection.token, &connection.version, &connection.started_at)
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
    model.admin.effects.push(AdminEffect::RunAction { repo_id, action_id: action.action_id, paths });
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
            model.admin.effects.push(AdminEffect::ReloadBrowser { repo_id });
        }
        Err(error) => {
            eprintln!("Nana 仓库动作执行失败：{error}");
            model.admin.actions_error = error;
        }
    }
}
