//! 设置、插件、日志、任务和来源登录的消息归约。
//!
//! 包括插件操作的提示文案、字段规范化、日志筛选、任务弹层的开关、
//! 主题和圆角的即时保存。只改状态并排副作用，真正的服务调用在 `dispatch::admin`。

use crate::backend::services::repository::{
    PluginConfigSnapshot, PluginManifest, RepositoryAction,
};

use super::super::status::FailureSource;
use super::super::workspace_refresh::SilentMessage;
use super::super::{ShellMessage, ShellPage, ShellViewModel, WorkspacePanel};
use super::support::{self, FieldChange};
use super::{AdminEffect, AdminMessage, PluginCallOrigin, source_provision as flow};

mod plugin_config;
mod results;

pub(crate) use plugin_config::*;
pub(crate) use results::*;

/// 处理管理消息，并吃掉壳层里属于设置、插件、日志和任务的分支。
pub(crate) fn reduce_message(
    model: &mut ShellViewModel,
    message: ShellMessage,
) -> Option<ShellMessage> {
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
            if panel == WorkspacePanel::Extensions
                && model.admin.plugins.is_empty()
                && !model.admin.loading_settings
            {
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
        ShellMessage::WorkspaceListLoaded {
            generation,
            result: Ok(items),
        } => {
            model.admin.note_backends(&items);
            Some(ShellMessage::WorkspaceListLoaded {
                generation,
                result: Ok(items),
            })
        }
        ShellMessage::SilentWorkspace(SilentMessage::Repositories(Ok(items))) => {
            model.admin.note_backends(&items);
            Some(ShellMessage::SilentWorkspace(SilentMessage::Repositories(
                Ok(items),
            )))
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
            model
                .status
                .fail(FailureSource::Logs, format!("无法读取系统日志：{error}"));
            None
        }
        // 清空中按钮禁用，结果回来以前日志页不变。
        ShellMessage::ClearLogs => {
            if model.admin.logs_clearing {
                eprintln!("Nana 系统日志正在清空，忽略重复点击");
            } else {
                model.admin.logs_clearing = true;
                model.admin.push_effect(AdminEffect::ClearLogs);
            }
            None
        }
        ShellMessage::TaskProgressLoaded(progress) => {
            model.task_progress = progress;
            None
        }
        // 读的这段时间里改过设置、或者有保存在写时，读到的可能是旧内容，不换上（`SettingsSync`）。
        ShellMessage::SettingsLoaded(Ok((settings, diagnostic))) => {
            if let Some(diagnostic) = diagnostic {
                eprintln!("Nana {diagnostic}");
            }
            if model.admin.settings_sync.finish_load() {
                model.settings = settings;
            } else {
                eprintln!("Nana 读设置文件的这段时间里改过设置，读到的旧内容不换上");
            }
            None
        }
        ShellMessage::SettingsLoaded(Err(error)) => {
            model.admin.settings_sync.finish_load();
            eprintln!("Nana 应用设置读取失败：{error}");
            model.status.fail(
                FailureSource::Settings,
                format!("无法读取应用设置：{error}"),
            );
            None
        }
        // 主题：暗色、浅色和跟随系统，改了立即生效并写进设置文件；和现在一样时不写。
        ShellMessage::SettingsThemeChanged(theme) => {
            if theme != "light" && theme != "dark" && theme != "system" {
                eprintln!("Nana 忽略未知主题：{theme}");
                return None;
            }
            if model.settings.theme == theme {
                return None;
            }
            model.settings.theme = theme;
            if model.admin.settings_sync.note_change() {
                model.admin.push_effect(AdminEffect::SaveSettings);
            }
            None
        }
        // 写设置文件的结果。内存里的设置不跟着结果改（它总比这次写的新或者一样）；写的时候又改过，接着把
        // 最新的写一次。结果不切页面。
        ShellMessage::SettingsSaved(result) => {
            if let Err(error) = &result {
                eprintln!("Nana 应用设置保存失败：{error}");
                model.status.fail(
                    FailureSource::Settings,
                    format!("保存应用设置失败：{error}"),
                );
            }
            if model.admin.settings_sync.note_saved() {
                model.admin.push_effect(AdminEffect::SaveSettings);
            }
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
            model.admin.remember_outcome(
                if enabled {
                    "插件已启用。"
                } else {
                    "插件已禁用。"
                },
                "插件状态更新失败。",
            );
            model
                .admin
                .push_effect(AdminEffect::SetEnabled { plugin_id, enabled });
        }
        AdminMessage::RequestDelete(plugin_id) => request_delete(model, &plugin_id),
        AdminMessage::CancelDelete => model.admin.pending_delete = None,
        AdminMessage::ConfirmDelete => confirm_delete(model),
        AdminMessage::ToggleSettings(plugin_id) => open_settings(model, &plugin_id),
        AdminMessage::RefreshDownloaderStatus => super::downloader::request(model),
        AdminMessage::DownloaderStatusLoaded(result) => super::downloader::finish(model, result),
        AdminMessage::RefreshOfficeStatus => super::office::request(model, false),
        AdminMessage::SelectOfficeRepository(repo_id) => {
            super::office::select_repository(model, repo_id)
        }
        AdminMessage::RunOfficeSelfCheck => super::office::self_check(model),
        AdminMessage::ShutdownOfficeDaemon => super::office::shutdown(model),
        AdminMessage::ClearOfficeCache => super::office::clear_cache(model),
        AdminMessage::OfficeCallFinished { call, result } => {
            super::office::finish(model, call, result)
        }
        AdminMessage::RoutePlugin(plugin_id) => open_route(model, &plugin_id),
        AdminMessage::ConfigInput {
            plugin_id,
            key,
            text,
            checked,
        } => apply_field(model, &plugin_id, &key, &text, checked),
        AdminMessage::FieldDraft {
            plugin_id,
            key,
            value,
        } => {
            model
                .admin
                .field_drafts
                .entry(plugin_id)
                .or_default()
                .insert(key, value);
        }
        AdminMessage::JsonDraft {
            plugin_id,
            key,
            value,
        } => {
            model
                .admin
                .json_drafts
                .entry(plugin_id)
                .or_default()
                .insert(key, value);
        }
        AdminMessage::CommitDraft { plugin_id, key } => commit_draft(model, &plugin_id, &key),
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
            model
                .admin
                .push_effect(AdminEffect::OpenDataDirectory { plugin_id, name });
        }
        AdminMessage::DataDirectoryFinished { name, result } => {
            finish_directory(model, &name, result)
        }
        AdminMessage::HooksLoaded(Ok(records)) => model.admin.hook_executions = records,
        AdminMessage::HooksLoaded(Err(error)) => {
            eprintln!("Nana 插件钩子记录读取失败：{error}");
            model.admin.load_error = error;
        }
        AdminMessage::CacheLoaded(Ok(snapshot)) => model.admin.cache = Some(snapshot),
        AdminMessage::CacheLoaded(Err(error)) => eprintln!("Nana 缓存快照读取失败：{error}"),
        AdminMessage::ApiDesignLoaded(Ok(snapshot)) => model.admin.api_design = Some(snapshot),
        AdminMessage::ApiDesignLoaded(Err(error)) => {
            eprintln!("Nana API 设计快照读取失败：{error}")
        }
        AdminMessage::SettingsBundleLoaded {
            plugins,
            hooks,
            api,
            external,
        } => apply_bundle(model, plugins, hooks, api, external),
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
        AdminMessage::LogsCleared(result) => note_logs_cleared(model, result),
        AdminMessage::ToggleTaskPopover => model.admin.popover_open = !model.admin.popover_open,
        AdminMessage::CloseTaskPopover => model.admin.popover_open = false,
        AdminMessage::TaskPopoverToggled(open) => model.admin.popover_open = open,
        AdminMessage::CancelTask(task_id) => cancel_task(model, &task_id),
        AdminMessage::SelectAction(action_id) => {
            model.admin.active_action_id = Some(action_id);
            model.workspace.panel = WorkspacePanel::Actions;
        }
        AdminMessage::RunAction(action_id) => run_action(model, action_id),
        AdminMessage::ActionsLoaded { repo_id, result } => apply_actions(model, &repo_id, result),
        AdminMessage::ActionRunFinished { result } => finish_action(model, result),
        AdminMessage::SetToolPages(pages) => {
            model.admin.active_tool_page_id = support::apply_tool_page_selection(
                &pages,
                model.admin.active_tool_page_id.as_deref(),
            );
            model.admin.tool_pages = pages;
        }
        AdminMessage::SelectToolPage(page_id) => {
            if model.admin.tool_pages.iter().any(|page| page.id == page_id) {
                model.admin.active_tool_page_id = Some(page_id);
            }
        }
        AdminMessage::Api(message) => super::api::reduce(model, message),
        AdminMessage::CallFilePlugin {
            plugin_id,
            method,
            payload,
            repository_id,
        } => {
            if plugin_id.trim().is_empty() || method.trim().is_empty() {
                eprintln!("Nana 文件插件动作缺少方法");
                return;
            }
            model.admin.push_effect(AdminEffect::CallPlugin {
                plugin_id,
                method,
                payload,
                repository_id,
                origin: PluginCallOrigin::FileMenu,
            });
        }
        AdminMessage::FilePluginFinished { method, result } => {
            finish_file_plugin(model, &method, result)
        }
        AdminMessage::DownloadFinished { run, result } => {
            super::source_download::finish(model, run, result)
        }
        AdminMessage::LogsWritten(Ok(())) => {}
        AdminMessage::LogsWritten(Err(error)) => eprintln!("Nana 系统日志没有写进去：{error}"),
        AdminMessage::BeginRepositoryDownload(download) => {
            super::source_target::begin(model, download)
        }
        AdminMessage::ChooseDownloadRepository(repo_id) => {
            super::source_target::choose(model, &repo_id)
        }
        AdminMessage::CancelDownloadRepository => super::source_target::cancel(model),
        AdminMessage::CreateSourcePlaylist {
            playlist,
            repository_id,
        } => create_source_playlist(model, playlist, repository_id),
        AdminMessage::SourcePlaylistFinished(result) => finish_source_playlist(model, result),
        AdminMessage::BeginSourceAuth { plugin_id, repo_id } => {
            flow::begin(model, &plugin_id, repo_id)
        }
        AdminMessage::CheckSourceAuth { plugin_id, repo_id } => {
            flow::refresh_status(model, &plugin_id, &repo_id)
        }
        AdminMessage::ClearSourceAuth { plugin_id, repo_id } => {
            flow::clear(model, &plugin_id, &repo_id)
        }
        AdminMessage::CancelSourceAuth => flow::cancel(model),
        AdminMessage::PollSourceAuth { plugin_id } => flow::poll(model, &plugin_id),
        AdminMessage::ChooseSourceCache => {
            if !model.admin.source_auth.busy {
                let name = flow::cache_dialog_plugin_name(model);
                model.input.queue_source_cache_dialog(name);
            }
        }
        AdminMessage::SetSourceCachePath(path) => {
            let path = path.trim().to_string();
            if !path.is_empty() {
                model.admin.source_auth.cache_path = path;
            }
        }
        AdminMessage::ChooseEagleLibrary => super::tool_native::begin_library_dialog(model),
        AdminMessage::SetEagleLibraryPath(path) => {
            super::tool_native::set_library_path(model, &path)
        }
        AdminMessage::RunEagleImport(mode) => {
            let busy = model.files.mutating;
            super::tool_native::run_import(model, &mode);
            if !busy && model.files.mutating {
                model.admin.eagle_import = None;
            }
        }
        AdminMessage::SourceStepFinished { step, result } => flow::finish(model, step, result),
        AdminMessage::SourceRepositoryCreated { name, result } => {
            flow::finish_created(model, &name, result)
        }
        AdminMessage::SourceRepositorySynced { repo_id, result } => {
            flow::finish_synced(model, &repo_id, result)
        }
    }
}
