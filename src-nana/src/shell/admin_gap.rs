//! 下载服务、没有方法名的来源认证，以及非内置工具页。
//!
//! 版式对齐 Vue：下载服务是状态行，来源认证是「账号与仓库」，工具页是标题加说明。
//! 没有快照时只写缺的数据合约，不编造任务数、版本或登录状态。

use nana_ui::runtime::view::{text, widget, AnyView, IntoView};
use nana_ui::runtime::{Activate, AlignSpec, Button, EmptyState, LabeledValue, List, ListItem, SettingsCard, Stack};

use super::super::workbench;
use super::super::{ShellMessage, ShellViewModel};
use super::support::{self, DownloaderStatus};
use super::{AdminMessage, ToolPageEntry};
use crate::backend::services::repository::PluginManifest;

const DOWNLOADER_PLUGIN_ID: &str = "momobako.service.downloader";
const DOWNLOADER_METHOD: &str = "downloader.getRuntimeStatus";
/// 和 Vue 设置页同一组标签。没有快照时每行写「未加载」，不把字段名连成一段话。
const DOWNLOADER_LABELS: &[&str] = &["运行时", "aria2 状态", "aria2 版本", "RPC 地址", "任务数", "下载目录", "下载源"];

/// 当前打开的插件设置。下载服务和缺方法名的来源认证放到设置列最上面。
pub(crate) fn opened_plugin_pages(model: &ShellViewModel) -> Vec<AnyView> {
    let Some(plugin_id) = model.admin.active_settings_plugin_id.as_deref() else {
        return Vec::new();
    };
    let Some(plugin) = model.admin.plugins.iter().find(|plugin| plugin.plugin_id == plugin_id) else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    if support::is_downloader_settings(plugin) {
        rows.push(downloader_card(model));
    }
    if super::office::is_office_settings(plugin) {
        rows.push(super::office::office_card(model));
    }
    if let Some(card) = source_gap_card(model, plugin) {
        rows.push(card);
    } else if support::has_source_authentication(plugin) && !super::tool_native::source_auth_actions(plugin).is_empty() {
        rows.push(super::source_page::source_auth_page(model, plugin));
    }
    rows
}

/// 下载服务设置页。刷新只调用 Vue 写死的那个方法。
pub(super) fn downloader_card(model: &ShellViewModel) -> AnyView {
    let loading = model.admin.downloader_loading;
    let mut rows = vec![widget(Stack::bar(8.0).align(AlignSpec::Center))
        .children((
            widget(workbench::eyebrow("Downloader")).key("admin-downloader-eyebrow"),
            widget(Stack::spacer()),
            widget(Button::new(if loading { "刷新中" } else { "刷新" }).disabled(loading))
                .key("admin-downloader-refresh")
                .on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program(ShellMessage::Admin(AdminMessage::RefreshDownloader));
                }),
        ))
        .into_any()];
    if !model.admin.downloader_error.is_empty() {
        rows.push(text(model.admin.downloader_error.clone()).key("admin-downloader-error").into_any());
    }
    if let Some(status) = model.admin.downloader_status.as_ref() {
        rows.push(status_list(status));
    } else if model.admin.downloader_error.is_empty() {
        rows.push(unloaded_fields(DOWNLOADER_LABELS, "admin-downloader-missing"));
        rows.push(widget(workbench::wrapping_note("查看 aria2 运行时、任务队列和下载目录状态。")).key("admin-downloader-hint").into_any());
    }
    rows.push(
        widget(workbench::wrapping_note("音乐下载、歌词导出与 LibreOffice 运行时下载统一复用这套 aria2 任务层。"))
            .key("admin-downloader-note")
            .into_any(),
    );
    widget(SettingsCard::new("aria2 运行状态")).children(rows).key("admin-downloader-card").into_any()
}

/// 一行一个字段。值固定是「未加载」，调用方只有在没有快照时才挂这张网格。
pub(super) fn unloaded_fields(labels: &[&str], key: &'static str) -> AnyView {
    let rows = labels
        .iter()
        .enumerate()
        .map(|(index, label)| widget(LabeledValue::new(*label, "未加载")).key(format!("{key}-{index}")).into_any())
        .collect::<Vec<_>>();
    widget(List::new()).children(rows).key(key).into_any()
}

fn status_list(status: &DownloaderStatus) -> AnyView {
    let queue = status.queue_size.map(|count| count.to_string()).unwrap_or_else(|| "未返回".into());
    let rows = [
        ("运行时", status.runtime.as_str()),
        ("aria2 状态", status.aria2_state.as_str()),
        ("aria2 版本", status.version.as_str()),
        ("RPC 地址", status.rpc_url.as_str()),
        ("任务数", queue.as_str()),
        ("下载目录", status.downloads_dir.as_str()),
        ("下载源", status.download_url.as_str()),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (label, value))| widget(LabeledValue::new(label, value)).key(format!("admin-downloader-row-{index}")).into_any())
    .collect::<Vec<_>>();
    widget(List::new()).children(rows).key("admin-downloader-list").into_any()
}

/// 没有登录方法名时画出 Vue 的账号与仓库。有方法名的仍走原来的登录按钮。
pub(super) fn source_gap_card(model: &ShellViewModel, plugin: &PluginManifest) -> Option<AnyView> {
    let contract = support::source_auth_gap_contract(plugin)?;
    let plugin_id = plugin.plugin_id.as_str();
    let mut rows = vec![
        widget(workbench::meta("认证由 Source 插件处理，宿主只保存安全凭据引用。"))
            .key(format!("admin-source-gap-note-{plugin_id}"))
            .into_any(),
        widget(Button::new("连接新账号").disabled(true))
            .key(format!("admin-source-gap-connect-{plugin_id}"))
            .into_any(),
    ];
    let repositories = source_repositories(model, plugin);
    if repositories.is_empty() {
        rows.push(
            widget(EmptyState::new("还没有这个来源的仓库").message("连接新账号需要 createSessionMethod。当前声明里没有方法名。").compact(true))
                .key(format!("admin-source-gap-empty-{plugin_id}"))
                .into_any(),
        );
    } else {
        let items = repositories
            .iter()
            .enumerate()
            .map(|(index, repository)| repository_row(plugin_id, index, repository))
            .collect::<Vec<_>>();
        rows.push(widget(List::new()).children(items).key(format!("admin-source-gap-list-{plugin_id}")).into_any());
    }
    rows.push(widget(workbench::wrapping_note(contract)).key(format!("admin-source-gap-contract-{plugin_id}")).into_any());
    Some(widget(SettingsCard::new("账号与仓库")).children(rows).key(format!("admin-source-gap-{plugin_id}")).into_any())
}

fn repository_row(plugin_id: &str, index: usize, repository: &super::super::workspace::WorkspaceRepository) -> AnyView {
    let path = repository.path.trim();
    let path = if path.is_empty() { "未返回路径" } else { path };
    widget(Stack::column(6.0))
        .key(format!("admin-source-gap-repo-{plugin_id}-{index}"))
        .children((
            widget(ListItem::new(repository.name.clone()).detail(format!("未检查 · {path}"))),
            widget(Stack::row(8.0).wrap(true)).children((
                widget(Button::new("检查").disabled(true)).key(format!("admin-source-gap-status-{plugin_id}-{index}")),
                widget(Button::new("重新登录").disabled(true)).key(format!("admin-source-gap-relogin-{plugin_id}-{index}")),
                widget(Button::new("退出").disabled(true)).key(format!("admin-source-gap-clear-{plugin_id}-{index}")),
            )),
        ))
        .into_any()
}

fn source_repositories<'a>(model: &'a ShellViewModel, plugin: &PluginManifest) -> Vec<&'a super::super::workspace::WorkspaceRepository> {
    let mut accepted = vec![plugin.plugin_id.as_str()];
    accepted.extend(plugin.legacy_plugin_ids.iter().map(String::as_str));
    accepted.extend(plugin.compat.legacy_plugin_ids.iter().map(String::as_str));
    model
        .workspace
        .repositories
        .iter()
        .filter(|repository| accepted.iter().any(|id| repository.backend_plugin_id == *id))
        .collect()
}

/// 左侧工具导航：名称和说明。选中项走原来的 `SelectToolPage`。
pub(super) fn tool_nav(model: &ShellViewModel) -> AnyView {
    let mut rows = vec![widget(workbench::eyebrow("插件工具")).key("admin-tool-nav-eyebrow").into_any()];
    for page in &model.admin.tool_pages {
        let id = page.id.clone();
        let selected = model.admin.active_tool_page_id.as_deref() == Some(page.id.as_str());
        let description = page.description.trim();
        let description = if description.is_empty() { page.plugin_name.clone() } else { description.to_string() };
        rows.push(
            widget(Stack::column(2.0))
                .key(format!("admin-tool-nav-{}", page.id))
                .children((
                    widget(Button::new(if selected { format!("当前 {}", page.label) } else { page.label.clone() }))
                        .key(format!("admin-tool-{}", page.id))
                        .on_cx(move |_, _: &Activate, cx| {
                            cx.dispatch_program(ShellMessage::Admin(AdminMessage::SelectToolPage(id.clone())));
                        }),
                    widget(workbench::meta(description)).key(format!("admin-tool-desc-{}", page.id)),
                ))
                .into_any(),
        );
    }
    widget(Stack::column(8.0).width(nana_ui::runtime::LengthSpec::Px(220.0)).shrink(0.0))
        .children(rows)
        .key("admin-tool-nav")
        .into_any()
}

/// 右侧工具页。非内置页不再套一张和导航同名的卡片。
pub(super) fn tool_page_body(model: &ShellViewModel, page: &ToolPageEntry) -> AnyView {
    if support::is_builtin_tool_page(&page.id) {
        return super::tool_native::tool_surface(model, page);
    }
    let contract = support::foreign_tool_contract(page).unwrap_or_else(|| "工具页没有可绘制的快照。".into());
    widget(Stack::fill_column(8.0))
        .children((widget(EmptyState::new("没有可绘制的工具页快照").message(contract)).key(format!("admin-foreign-tool-empty-{}", page.id)),))
        .key(format!("admin-foreign-tool-{}", page.id))
        .into_any()
}

/// 左导航加右页面。插件列表不再塞一颗工具按钮。
pub(super) fn tool_workbench(model: &ShellViewModel) -> AnyView {
    let page = model
        .admin
        .tool_pages
        .iter()
        .find(|page| Some(page.id.as_str()) == model.admin.active_tool_page_id.as_deref())
        .or(model.admin.tool_pages.first());
    let body = page.map(|page| tool_page_body(model, page)).unwrap_or_else(|| {
        widget(EmptyState::new("没有工具页").message("插件还没有声明工具页。").compact(true)).key("admin-tool-missing").into_any()
    });
    widget(Stack::fill_row(16.0).align(AlignSpec::Start).min_height(nana_ui::runtime::LengthSpec::Px(280.0)))
        .children((tool_nav(model), body))
        .key("admin-tool-workbench")
        .into_any()
}

/// 和 Vue 一样，刷新固定打到下载服务插件，不使用当前设置页碰巧的 id。
pub(super) fn queue_downloader(model: &mut ShellViewModel) {
    if model.admin.downloader_loading {
        eprintln!("Nana 下载服务状态正在读取");
        return;
    }
    model.admin.downloader_loading = true;
    model.admin.downloader_error.clear();
    model.admin.action_message = "正在读取下载服务状态…".into();
    model.admin.effects.push(super::AdminEffect::CallPlugin {
        plugin_id: DOWNLOADER_PLUGIN_ID.into(),
        method: DOWNLOADER_METHOD.into(),
        payload: serde_json::json!({}),
        repository_id: None,
        origin: super::PluginCallOrigin::Downloader,
    });
}

/// 成功才替换快照。失败保留上一份，并记下错误。
pub(super) fn finish_downloader(model: &mut ShellViewModel, result: Result<serde_json::Value, String>) {
    model.admin.downloader_loading = false;
    match result {
        Ok(payload) => {
            model.admin.downloader_error.clear();
            model.admin.downloader_status = Some(support::parse_downloader_status(&payload));
            model.admin.action_message = "已读取下载服务状态。".into();
        }
        Err(error) => {
            eprintln!("Nana 下载服务状态失败：{error}");
            model.admin.downloader_error = error;
            model.admin.action_message.clear();
        }
    }
}
