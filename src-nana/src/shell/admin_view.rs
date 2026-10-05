//! 设置、插件、日志、任务和仓库动作的实况表面。
//!
//! 只在不是验收场景时挂上。验收页继续用原来的按钮和文案。

use nana_ui::icons_tabler::{BORDER_CORNER_ROUNDED, BORDER_RADIUS, COPY, DOWNLOAD, JSON};
use nana_ui::runtime::view::{button, text, widget, AnyView, IntoView};
use nana_ui::runtime::{Activate, Icon, IconGlyph, Stack, TextChanged, TextInput};

use super::super::{ShellMessage, ShellViewModel, WorkspacePanel};
use super::support::{self, action_can_run, action_status_label};
use super::{AdminMessage, ToolPageEntry};

/// 把阶段 6 的实况控件接到同一棵 Runtime 树上。
pub(crate) fn admin_surface(model: &ShellViewModel) -> AnyView {
    let mut rows = Vec::new();
    if model.admin_settings_visible() {
        rows.extend(settings_rows(model));
    }
    if model.admin_workspace_visible(WorkspacePanel::Actions) {
        rows.extend(action_rows(model));
    }
    if model.admin_workspace_visible(WorkspacePanel::Extensions) {
        rows.extend(tool_rows(model));
    }
    if model.admin_workspace_visible(WorkspacePanel::Logs) {
        rows.extend(log_rows(model));
    }
    if model.admin_settings_visible() || model.admin_workspace_visible(WorkspacePanel::Extensions) {
        rows.extend(plugin_rows(model));
    }
    if task_visible(model) {
        rows.extend(task_rows(model));
    }
    widget(Stack::fill_column(8.0)).children(rows).key("admin-surface").into_any()
}

fn task_visible(model: &ShellViewModel) -> bool {
    !model.acceptance_scene
        && model.workspace.startup.status == super::super::StartupStatus::Ready
        && model.workspace.main_region() == super::super::MainRegion::HasRepository
}

fn settings_rows(model: &ShellViewModel) -> Vec<AnyView> {
    let (choices, selected, notice) = support::audio_choices(&model.player.candidates, &model.player.preferences);
    let enabled = support::audio_picker_enabled(&choices);
    let mut rows = vec![
        text("默认音频播放器").key("admin-audio-label").into_any(),
        text(if selected.is_empty() { "未选择".into() } else { selected }).key("admin-audio-selected").into_any(),
    ];
    if !notice.is_empty() {
        rows.push(text(notice).key("admin-audio-notice").into_any());
    }
    if enabled {
        rows.extend(choices.into_iter().filter(|choice| !choice.unavailable).map(|choice| {
            let plugin_id = choice.plugin_id.clone();
            button(choice.label)
                .key(format!("admin-audio-{}", choice.plugin_id))
                .on_cx(move |_, _: &Activate, cx| {
                    cx.dispatch_program(ShellMessage::Admin(AdminMessage::SetAudioPlayer(Some(plugin_id.clone()))));
                })
                .into_any()
        }));
    }
    rows.push(widget(Stack::bar(8.0)).children((
        text(format!("圆角：{} · {}px", corner_label(&model.admin.corner_style), model.admin.corner_radius)).key("admin-corner"),
        widget(Stack::spacer()),
        labeled_icon(BORDER_CORNER_ROUNDED, "平滑", "admin-corner-smooth", || {
            ShellMessage::Admin(AdminMessage::SetCornerStyle("smooth".into()))
        }),
        labeled_icon(BORDER_RADIUS, "普通", "admin-corner-round", || {
            ShellMessage::Admin(AdminMessage::SetCornerStyle("round".into()))
        }),
    )).into_any());
    let radius = model.admin.corner_radius.to_string();
    rows.push(widget(TextInput::new(radius).label("圆角半径")).on_cx(|_, event: &TextChanged, cx| {
        cx.dispatch_program(ShellMessage::Admin(AdminMessage::SetCornerRadius(event.value.to_string())));
    }).into_any());
    rows.push(text(format!("已注册仓库 {}", model.workspace.repositories.len())).key("admin-repo-count").into_any());
    rows.push(text(format!("已接入后端 {}", support::backend_summary(&model.admin.backends))).key("admin-backends").into_any());
    let ready = model.admin.external.as_ref().map(|status| status.ready);
    rows.push(text(format!("外部素材 {}", support::external_status_label(ready))).key("admin-external-status").into_any());
    let token = model.admin.external.as_ref().map(|status| support::mask_token(Some(status.token.as_str()))).unwrap_or_else(|| "未加载".into());
    rows.push(text(format!("Token {token}")).key("admin-external-token").into_any());
    let base_url = model.admin.external.as_ref().map(|status| status.base_url.clone()).unwrap_or_default();
    let token_value = model.admin.external.as_ref().map(|status| status.token.clone()).unwrap_or_default();
    let json = external_json(model);
    rows.push(widget(Stack::row(8.0)).children((
        copy_button(COPY, "复制 Base URL", "Base URL", base_url, "admin-copy-url"),
        copy_button(COPY, "复制 Token", "Token", token_value, "admin-copy-token"),
        copy_button(JSON, "复制 JSON", "连接 JSON", json, "admin-copy-json"),
        labeled_icon(DOWNLOAD, "导出 JSON", "admin-export-json", || {
            ShellMessage::Admin(AdminMessage::ExportExternal)
        }),
    )).into_any());
    if !model.admin.external_error.is_empty() {
        rows.push(text(model.admin.external_error.clone()).key("admin-external-error").into_any());
    } else if !model.admin.external_message.is_empty() {
        rows.push(text(model.admin.external_message.clone()).key("admin-external-message").into_any());
    }
    let metadata = model.admin.cache.as_ref().map(|cache| cache.config.metadata_capacity).unwrap_or(0);
    let thumbnail = model.admin.cache.as_ref().map(|cache| cache.config.thumbnail_capacity).unwrap_or(0);
    let query = model.admin.cache.as_ref().map(|cache| cache.config.query_capacity).unwrap_or(0);
    rows.push(text(format!("缓存 {metadata} / {thumbnail} / {query}")).key("admin-cache").into_any());
    let transport = model.admin.api_design.as_ref().map(|api| api.transport.clone()).unwrap_or_else(|| "本地服务契约未加载".into());
    rows.push(text(transport).key("admin-api-transport").into_any());
    rows
}

fn copy_button(icon: Icon, label: &'static str, field: &'static str, value: String, key: &'static str) -> AnyView {
    widget(Stack::row(6.0)).children((
        widget(IconGlyph::new(icon).size(14.0)),
        button(label).key(key).on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program(ShellMessage::Admin(AdminMessage::CopyExternal {
                label: field.into(),
                value: value.clone(),
            }));
        }),
    )).into_any()
}

fn labeled_icon(
    icon: Icon,
    label: &'static str,
    key: &'static str,
    message: impl Fn() -> ShellMessage + Send + 'static,
) -> AnyView {
    widget(Stack::row(6.0)).children((
        widget(IconGlyph::new(icon).size(14.0)),
        button(label).key(key).on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program(message());
        }),
    )).into_any()
}

fn external_json(model: &ShellViewModel) -> String {
    let Some(connection) = &model.admin.external else {
        return String::new();
    };
    support::connection_json(&connection.base_url, &connection.token, &connection.version, &connection.started_at)
}

fn corner_label(style: &str) -> &'static str {
    if style == "round" { "普通" } else { "平滑" }
}

fn action_rows(model: &ShellViewModel) -> Vec<AnyView> {
    let mut rows = vec![text(format!("仓库动作 {} 项", model.admin.actions.len())).key("admin-actions-count").into_any()];
    if model.admin.actions_loading {
        rows.push(text("正在加载动作").key("admin-actions-loading").into_any());
        return rows;
    }
    if model.admin.actions.is_empty() {
        rows.push(text("当前仓库没有导入动作。").key("admin-actions-empty").into_any());
        return rows;
    }
    let selected = model.files.selected_paths().len();
    let active = model.admin.active_action_id.as_deref();
    let current = model.admin.actions.iter().find(|action| Some(action.action_id.as_str()) == active).or(model.admin.actions.first());
    for action in &model.admin.actions {
        let action_id = action.action_id.clone();
        let label = format!("{} · {} · {}", action.name, action.source, action_status_label(&action.status, action.enabled));
        rows.push(button(label).key(format!("admin-action-{}", action.action_id)).on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program(ShellMessage::Admin(AdminMessage::SelectAction(action_id.clone())));
        }).into_any());
    }
    if let Some(action) = current {
        let can_run = action_can_run(&action.status, action.enabled, selected, model.admin.actions_running);
        let action_id = action.action_id.clone();
        let last = action.last_run.as_ref().map(|run| run.status.as_str()).unwrap_or("无");
        rows.push(text(format!("{} · 最近运行 {last}", action_status_label(&action.status, action.enabled))).key("admin-action-detail").into_any());
        rows.push(button("执行").key("admin-action-run").disabled(!can_run).on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program(ShellMessage::Admin(AdminMessage::RunAction(Some(action_id.clone()))));
        }).into_any());
    }
    if !model.admin.actions_error.is_empty() {
        rows.push(text(model.admin.actions_error.clone()).key("admin-actions-error").into_any());
    }
    rows
}

fn tool_rows(model: &ShellViewModel) -> Vec<AnyView> {
    let mut rows = Vec::new();
    rows.push(text("这里集中展示当前插件和后端能力。").key("admin-tools-subline").into_any());
    if model.admin.tool_pages.is_empty() {
        rows.push(text("没有原生工具页").key("admin-tools-empty").into_any());
        return rows;
    }
    for page in &model.admin.tool_pages {
        let id = page.id.clone();
        rows.push(button(page.label.clone()).key(format!("admin-tool-{}", page.id)).on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program(ShellMessage::Admin(AdminMessage::SelectToolPage(id.clone())));
        }).into_any());
    }
    if let Some(page) = active_tool(model) {
        if let Some(upgrade) = support::tool_page_upgrade(page) {
            rows.push(text(upgrade).key("admin-tool-upgrade").into_any());
        }
    }
    rows
}

fn active_tool(model: &ShellViewModel) -> Option<&ToolPageEntry> {
    model.admin.tool_pages.iter().find(|page| Some(page.id.as_str()) == model.admin.active_tool_page_id.as_deref()).or(model.admin.tool_pages.first())
}

fn log_rows(model: &ShellViewModel) -> Vec<AnyView> {
    let filtered = model.admin.filtered_logs();
    let mut rows = vec![
        text(format!("{} 条缓存", model.admin.logs.len())).key("admin-log-cached").into_any(),
        text(format!("{} 条命中", filtered.len())).key("admin-log-hits").into_any(),
        text(format!("筛选 {}", support::active_filter_count(&model.admin.log_levels, &model.admin.log_kinds, &model.admin.log_plugin_id, &model.admin.log_repo_id, &model.admin.log_search))).key("admin-log-filters").into_any(),
        text(if model.admin.log_paused { "日志滚动已暂停" } else if model.admin.log_would_scroll { "日志将滚到最新" } else { "日志停在当前位置" }).key("admin-log-scroll").into_any(),
    ];
    let levels = ["debug", "info", "warn", "error"].into_iter().map(|value| {
        button(support::level_label(value)).key(format!("admin-log-level-{value}")).on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program(ShellMessage::Admin(AdminMessage::ToggleLogLevel(value.into())));
        }).into_any()
    }).collect::<Vec<_>>();
    rows.push(widget(Stack::row(8.0)).children(levels).key("admin-log-levels").into_any());
    let kinds = ["host", "frontend-host", "frontend-plugin", "backend-plugin", "helper"].into_iter().map(|value| {
        button(support::source_kind_label(value)).key(format!("admin-log-kind-{value}")).on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program(ShellMessage::Admin(AdminMessage::ToggleLogKind(value.into())));
        }).into_any()
    }).collect::<Vec<_>>();
    let paused = model.admin.log_paused;
    let mut kinds = kinds;
    kinds.push(button(if paused { "继续追踪" } else { "暂停追踪" }).key("admin-log-pause").on_cx(move |_, _: &Activate, cx| {
        cx.dispatch_program(ShellMessage::Admin(AdminMessage::SetLogPaused(!paused)));
    }).into_any());
    kinds.push(button("重置筛选").key("admin-log-reset").on_cx(|_, _: &Activate, cx| {
        cx.dispatch_program(ShellMessage::Admin(AdminMessage::ResetLogFilters));
    }).into_any());
    rows.push(widget(Stack::row(8.0)).children(kinds).key("admin-log-kinds").into_any());
    for plugin_id in support::unique_sorted(model.admin.logs.iter().filter_map(|record| record.source.plugin_id.clone())) {
        rows.push(button(plugin_id.clone()).key(format!("admin-log-plugin-{plugin_id}")).on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program(ShellMessage::Admin(AdminMessage::SetLogPlugin(plugin_id.clone())));
        }).into_any());
    }
    for repo_id in support::unique_sorted(model.admin.logs.iter().filter_map(|record| record.source.repo_id.clone())) {
        rows.push(button(repo_id.clone()).key(format!("admin-log-repo-{repo_id}")).on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program(ShellMessage::Admin(AdminMessage::SetLogRepo(repo_id.clone())));
        }).into_any());
    }
    let search = model.admin.log_search.clone();
    rows.push(widget(TextInput::new(search).label("搜索日志").placeholder("搜索消息、动作、位置或上下文")).on_cx(|_, event: &TextChanged, cx| {
        cx.dispatch_program(ShellMessage::Admin(AdminMessage::SetLogSearch(event.value.to_string())));
    }).into_any());
    rows.push(text("统一查看宿主、插件与辅助进程的实时日志流。").key("admin-log-subline").into_any());
    if filtered.is_empty() {
        rows.push(text("还没有系统日志").key("admin-log-empty-title").into_any());
        rows.push(text("宿主、插件和辅助进程产生的关键操作会在这里持续汇总。").key("admin-log-empty-detail").into_any());
    }
    rows
}

fn plugin_rows(model: &ShellViewModel) -> Vec<AnyView> {
    let groups = model.admin.grouped_plugins();
    let count: usize = groups.iter().map(|(_, ids)| ids.len()).sum();
    let mut rows = vec![text(format!("{count} 个插件")).key("admin-plugin-count").into_any()];
    if !model.admin.action_error.is_empty() {
        rows.push(text(model.admin.action_error.clone()).key("admin-plugin-error").into_any());
    } else if !model.admin.action_message.is_empty() {
        rows.push(text(model.admin.action_message.clone()).key("admin-plugin-message").into_any());
    }
    let keyword = model.admin.keyword.clone();
    rows.push(widget(TextInput::new(keyword).label("筛选插件").placeholder("筛选导入器、脚本或元数据拓展")).on_cx(|_, event: &TextChanged, cx| {
        cx.dispatch_program(ShellMessage::Admin(AdminMessage::SetKeyword(event.value.to_string())));
    }).into_any());
    rows.push(
        widget(Stack::row(8.0))
            .children((button("从 .momoplug 安装").key("admin-plugin-install").on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program(ShellMessage::Admin(AdminMessage::ChooseArchive));
            }),))
            .into_any(),
    );
    for (category, ids) in groups {
        rows.push(text(format!("{} {}", support::category_label(&category), ids.len())).key(format!("admin-plugin-group-{category}")).into_any());
        for plugin_id in ids {
            let Some(plugin) = model.admin.plugins.iter().find(|plugin| plugin.plugin_id == plugin_id) else {
                continue;
            };
            let label = format!(
                "{} · {} · {} · {}",
                plugin.name,
                support::plugin_source_label(&plugin.source),
                support::plugin_status_label(plugin),
                support::dependency_label(plugin)
            );
            rows.push(text(label).key(format!("admin-plugin-{plugin_id}")).into_any());
            for (index, state) in plugin.dependency_status.required.iter().chain(plugin.dependency_status.optional.iter()).enumerate() {
                let name = state.name.clone().unwrap_or_else(|| state.plugin_id.clone());
                rows.push(
                    text(format!("{name} · {}", support::dependency_status_label(&state.status)))
                        .key(format!("admin-plugin-dep-{plugin_id}-{index}"))
                        .into_any(),
                );
            }
            for (index, line) in support::settings_upgrade_lines(plugin, model.admin.vue_settings.contains(&plugin_id)).into_iter().enumerate() {
                rows.push(text(line).key(format!("admin-plugin-upgrade-{plugin_id}-{index}")).into_any());
            }
            let toggle_id = plugin_id.clone();
            let enabled = plugin.enabled;
            rows.push(button(if enabled { "停用插件" } else { "启用插件" }).key(format!("admin-plugin-toggle-{plugin_id}")).on_cx(move |_, _: &Activate, cx| {
                cx.dispatch_program(ShellMessage::Admin(AdminMessage::SetEnabled { plugin_id: toggle_id.clone(), enabled: !enabled }));
            }).into_any());
            if support::can_delete_plugin(plugin) {
                let delete_id = plugin_id.clone();
                rows.push(button("删除插件").key(format!("admin-plugin-delete-{plugin_id}")).on_cx(move |_, _: &Activate, cx| {
                    cx.dispatch_program(ShellMessage::Admin(AdminMessage::RequestDelete(delete_id.clone())));
                }).into_any());
            }
            let settings_id = plugin_id.clone();
            rows.push(button("插件设置").key(format!("admin-plugin-settings-{plugin_id}")).on_cx(move |_, _: &Activate, cx| {
                cx.dispatch_program(ShellMessage::Admin(AdminMessage::ToggleSettings(settings_id.clone())));
            }).into_any());
        }
    }
    if let Some(plugin_id) = model.admin.pending_delete.clone() {
        rows.push(text(format!("确认删除 {plugin_id}")).key("admin-plugin-pending-delete").into_any());
        rows.push(button("确认删除").key("admin-plugin-confirm-delete").on_cx(|_, _: &Activate, cx| {
            cx.dispatch_program(ShellMessage::Admin(AdminMessage::ConfirmDelete));
        }).into_any());
        rows.push(button("取消删除").key("admin-plugin-cancel-delete").on_cx(|_, _: &Activate, cx| {
            cx.dispatch_program(ShellMessage::Admin(AdminMessage::CancelDelete));
        }).into_any());
    }
    rows
}

fn task_rows(model: &ShellViewModel) -> Vec<AnyView> {
    let rows_data = model.task_rows();
    let mut rows = vec![widget(Stack::bar(8.0))
        .children((
            text(format!("任务 {}", rows_data.len())).key("admin-task-count"),
            button("任务").key("admin-task-toggle").on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program(ShellMessage::Admin(AdminMessage::ToggleTaskPopover));
            }),
        ))
        .into_any()];
    if model.admin.popover_open {
        rows.push(button("关闭任务").key("admin-task-close").on_cx(|_, _: &Activate, cx| {
            cx.dispatch_program(ShellMessage::Admin(AdminMessage::CloseTaskPopover));
        }).into_any());
        if rows_data.is_empty() {
            rows.push(text("当前没有运行中的任务。").key("admin-task-empty").into_any());
        }
        for task in rows_data {
            rows.push(text(format!("{} · {} · {}", task.source, task.label, task.detail)).key(format!("admin-task-{}", task.id)).into_any());
        }
    }
    rows
}
