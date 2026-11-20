//! 目录打开、设置包、圆角、外部连接导出、日志清空、文件插件与来源播放集、仓库动作的处理。

use super::*;

/// 查到了插件的设置目录：交给宿主打开，打开的结果回来（[`settle_directory_open`]）才写提示。
/// 查不到时写错误。
pub(super) fn finish_directory(
    model: &mut ShellViewModel,
    name: &str,
    result: Result<String, String>,
) {
    model.admin.managing = false;
    match result {
        Ok(path) => {
            model.admin.pending_directory = Some((path.clone(), name.to_string()));
            model
                .input
                .host_requests
                .push(crate::host_api::HostRequest::OpenExternal(
                    crate::host_api::ExternalOpenRequest {
                        target: path,
                        reveal: true,
                    },
                ));
            model.admin.action_error.clear();
        }
        Err(error) => {
            eprintln!("Nana 插件设置目录读取失败：{error}");
            model.admin.action_error = if error.is_empty() {
                "插件设置目录打开失败。".into()
            } else {
                error
            };
        }
    }
}

/// 宿主执行完一次外部打开（`host_bridge::perform`）。是插件「打开目录」等着的那一次：结果写在插件面板上，
/// 交回 `None`；别的打开把结果原样交回，失败由宿主写进状态区。
pub(crate) fn settle_directory_open(
    model: &mut ShellViewModel,
    target: &str,
    result: Result<(), String>,
) -> Option<Result<(), String>> {
    let Some((_, name)) = model
        .admin
        .pending_directory
        .take_if(|(path, _)| path == target)
    else {
        return Some(result);
    };
    match result {
        Ok(()) => {
            model.admin.action_error.clear();
            model.admin.action_message = format!("已打开“{name}”设置目录。");
        }
        Err(error) => {
            eprintln!("Nana 插件“{name}”的设置目录打不开：{target}：{error}");
            model.admin.action_message.clear();
            model.admin.action_error = format!("插件设置目录打开失败：{error}");
        }
    }
    model.mark_surface_dirty();
    None
}

/// 设置页四份数据。任何一份失败整批不写，只记下错误。
pub(super) fn apply_bundle(
    model: &mut ShellViewModel,
    plugins: Result<Vec<PluginManifest>, String>,
    hooks: Result<Vec<crate::backend::services::repository::PluginHookExecutionRecord>, String>,
    api: Result<crate::backend::services::repository::ApiDesignSnapshot, String>,
    external: Result<crate::backend::services::runtime::ExternalApiConnectionStatus, String>,
) {
    model.admin.loading_settings = false;
    // 首屏启动在等的设置包：成功时插件列表换新、接着读播放器类型，启动等它回来；失败时吞掉错误，启动就此结束。
    let complete = plugins.is_ok() && hooks.is_ok() && api.is_ok() && external.is_ok();
    model.workspace.note_settings_loaded(complete);
    match (plugins, hooks, api, external) {
        (Ok(plugins), Ok(hooks), Ok(api), Ok(external)) => {
            publish_plugins(model, &plugins);
            model.admin.store_plugins(plugins);
            model.admin.hook_executions = hooks;
            model.admin.api_design = Some(api);
            model.admin.external = Some(external);
            model.admin.load_error.clear();
            // API Playground 挂载时选中第一个端点并带出方法、目标和请求体，契约换新后同样重选。
            let _ = super::super::api::selected(model);
        }
        (plugins, hooks, api, external) => {
            let error = [plugins.err(), hooks.err(), api.err(), external.err()]
                .into_iter()
                .flatten()
                .next()
                .unwrap_or_default();
            eprintln!("Nana 设置页数据读取失败：{error}");
            model.admin.load_error = error;
        }
    }
}

pub(super) fn set_corner_style(model: &mut ShellViewModel, style: &str) {
    if style != "smooth" && style != "round" {
        eprintln!("Nana 忽略未知圆角样式：{style}");
        return;
    }
    model.admin.corner_style = style.to_string();
    model.admin.push_effect(AdminEffect::PersistCorners);
}

pub(super) fn set_corner_radius(model: &mut ShellViewModel, value: &str) {
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

pub(super) fn copy_external(model: &mut ShellViewModel, label: &str, value: &str) {
    model.admin.external_message.clear();
    model.admin.external_error.clear();
    if value.is_empty() {
        model.admin.external_error = format!("{label} 尚未加载。");
        return;
    }
    model
        .admin
        .push_effect(AdminEffect::CopyText(value.to_string()));
    model.admin.external_message = format!("{label} 已复制。");
}

pub(super) fn export_external(model: &mut ShellViewModel) {
    let content = external_json(model);
    model.admin.external_message.clear();
    model.admin.external_error.clear();
    if content.is_empty() {
        model.admin.external_error = "连接 JSON 尚未加载。".into();
        return;
    }
    model
        .admin
        .push_effect(AdminEffect::RequestSaveDialog { content });
}

pub(super) fn complete_export(model: &mut ShellViewModel, path: Option<String>) {
    let Some(path) = path.filter(|path| !path.trim().is_empty()) else {
        return;
    };
    let content = external_json(model);
    if content.is_empty() {
        model.admin.external_error = "连接 JSON 尚未加载。".into();
        return;
    }
    model.admin.push_effect(AdminEffect::WriteFile {
        path,
        bytes: content.into_bytes(),
    });
}

pub(super) fn external_json(model: &ShellViewModel) -> String {
    let Some(connection) = &model.admin.external else {
        return String::new();
    };
    support::connection_json(
        &connection.base_url,
        &connection.token,
        &connection.version,
        &connection.started_at,
    )
}

/// 清空系统日志的结果。成功把手上的日志清空、不再重读（之后的新记录照常经宿主事件合并进来）；
/// 失败时日志留着，原因写进状态区，让用户知道没清掉。
pub(super) fn note_logs_cleared(model: &mut ShellViewModel, result: Result<(), String>) {
    model.admin.logs_clearing = false;
    match result {
        Ok(()) => model.admin.replace_logs(Vec::new()),
        Err(error) => {
            eprintln!("Nana 清空系统日志失败：{error}");
            model
                .status
                .fail(FailureSource::Logs, format!("清空系统日志失败：{error}"));
        }
    }
}

/// 清理缓存、重新获取播放资源的结果。成功后和其它来源动作一样重读资源库列表，再按
/// 当前仓库同步、重读摘要和目录；失败写进文件列。下载的收尾在 `shell/admin/source_download.rs`。
pub(super) fn finish_file_plugin(
    model: &mut ShellViewModel,
    method: &str,
    result: Result<serde_json::Value, String>,
) {
    match result {
        Ok(_) => {
            model.files.error.clear();
            model.workspace.request_repository_refresh();
        }
        Err(error) => {
            eprintln!("Nana 文件插件动作失败：{method}：{error}");
            model.files.error = if error.is_empty() {
                format!("{method} 调用失败。")
            } else {
                error
            };
        }
    }
}

/// 「创建播放列表」交给服务：没有当前仓库或来源歌单标识时只报错，不建。
pub(super) fn create_source_playlist(
    model: &mut ShellViewModel,
    playlist: super::super::super::entry_actions::SourcePlaylist,
    repository_id: Option<String>,
) {
    let Some(repo_id) = repository_id.filter(|id| !id.trim().is_empty()) else {
        eprintln!("Nana 创建来源播放集没有当前仓库：{}", playlist.path);
        model.files.error = "没有当前资源库，不能创建播放列表。".into();
        return;
    };
    let Some(playlist_id) = playlist.playlist_id else {
        eprintln!("Nana 来源目录缺少播放列表标识：{}", playlist.path);
        model.files.error = "来源目录缺少播放列表标识".into();
        return;
    };
    model.admin.push_effect(AdminEffect::CreateSourcePlaylist {
        repo_id,
        playlist_id,
        name: playlist.name,
        player_type_id: playlist.player_type_id,
        path: playlist.path,
    });
}

/// 建好以后和别的来源动作一样刷新资源库，新播放集随摘要出现在侧栏。
/// 失败写进文件列表的错误，让用户看得到。
pub(super) fn finish_source_playlist(model: &mut ShellViewModel, result: Result<(), String>) {
    match result {
        Ok(()) => {
            model.files.error.clear();
            model.workspace.request_repository_refresh();
        }
        Err(error) => {
            eprintln!("Nana 创建来源播放集失败：{error}");
            model.files.error = if error.is_empty() {
                "创建播放列表失败。".into()
            } else {
                error
            };
        }
    }
}

pub(super) fn selected_action<'a>(
    model: &'a ShellViewModel,
    action_id: &Option<String>,
) -> Option<&'a RepositoryAction> {
    let requested = action_id
        .as_deref()
        .or(model.admin.active_action_id.as_deref());
    model
        .admin
        .actions
        .iter()
        .find(|action| Some(action.action_id.as_str()) == requested)
        .or(model.admin.actions.first())
}

/// 取消排队或运行中的后台任务。仓库操作行和已经在取消的任务只记日志，不发请求。
pub(super) fn cancel_task(model: &mut ShellViewModel, task_id: &str) {
    let cancellable = model.task_progress.iter().any(|task| {
        task.task_id == task_id && matches!(task.status.as_str(), "queued" | "running")
    });
    if !cancellable {
        eprintln!("Nana 任务不能取消：{task_id}");
        return;
    }
    model
        .admin
        .push_effect(AdminEffect::CancelTask(task_id.to_string()));
}

pub(super) fn run_action(model: &mut ShellViewModel, action_id: Option<String>) {
    let Some(repo_id) = model.repository_id.clone() else {
        eprintln!("Nana 没有活动仓库，不能执行仓库动作");
        return;
    };
    let Some(action) = selected_action(model, &action_id).cloned() else {
        eprintln!("Nana 没有可执行的仓库动作");
        return;
    };
    let selected = model.files.selected_paths().len();
    if !support::action_can_run(
        &action.status,
        action.enabled,
        selected,
        model.admin.actions_running,
    ) {
        eprintln!("Nana 仓库动作当前不能执行：{}", action.action_id);
        return;
    }
    let paths = model.files.selected_paths().to_vec();
    model.admin.actions_running = true;
    model.admin.actions_error.clear();
    model.admin.push_effect(AdminEffect::RunAction {
        repo_id,
        action_id: action.action_id,
        paths,
    });
}

pub(super) fn apply_actions(
    model: &mut ShellViewModel,
    repo_id: &str,
    result: Result<Vec<RepositoryAction>, String>,
) {
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
            if model
                .admin
                .active_action_id
                .as_ref()
                .is_some_and(|id| !actions.iter().any(|action| &action.action_id == id))
            {
                model.admin.active_action_id = None;
            }
            model.admin.active_action_id = model
                .admin
                .active_action_id
                .clone()
                .or_else(|| actions.first().map(|action| action.action_id.clone()));
            model.admin.actions = actions;
        }
        Err(error) => {
            eprintln!("Nana 仓库动作读取失败：{error}");
            model.admin.actions_error = error;
        }
    }
}

pub(super) fn finish_action(model: &mut ShellViewModel, result: Result<RepositoryAction, String>) {
    model.admin.actions_running = false;
    match result {
        Ok(action) => {
            let repo_id = action.repo_id.clone();
            if let Some(current) = model
                .admin
                .actions
                .iter_mut()
                .find(|item| item.action_id == action.action_id)
            {
                *current = action;
            }
            // 动作改了仓库：重读摘要和当前目录。
            model.reload_after_mutation(&repo_id);
        }
        Err(error) => {
            eprintln!("Nana 仓库动作执行失败：{error}");
            model.admin.actions_error = error;
        }
    }
}
