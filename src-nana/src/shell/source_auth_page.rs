//! 有方法名的来源认证页。
//!
//! 对齐 `SourceAuthenticationSettings.vue`：账号与仓库、扫码、缓存目录、
//! 轮询和建仓说明。二维码用已有的 `QrCode` 画进这一页，不另留一行「已调用」。

use nana_ui::runtime::view::{text, widget, AnyView, IntoView};
use nana_ui::runtime::{
    Activate, BrowseRequested, Button, EmptyState, LabeledValue, List, ListItem, PathField, QrCode, SettingsCard, Stack, TextChanged,
    TextInput, ValidationIntent, ValidationMessage,
};

use super::super::workbench;
use super::super::{ShellMessage, ShellViewModel};
use super::{AdminMessage, SourceAuthCall};
use crate::backend::services::repository::PluginManifest;

/// 声明了登录方法时的账号页。没有方法名的仍走缺口卡片。
pub(super) fn source_auth_page(model: &ShellViewModel, plugin: &PluginManifest) -> AnyView {
    let plugin_id = plugin.plugin_id.as_str();
    let mut rows = vec![
        widget(workbench::meta("认证由 Source 插件处理，宿主只保存安全凭据引用。"))
            .key(format!("admin-source-page-note-{plugin_id}"))
            .into_any(),
        action_row(model, plugin),
    ];
    rows.push(repository_block(model, plugin));
    rows.push(session_flow(model, plugin));
    rows.push(provision_form(model, plugin));
    if let Some(notice) = notice(model) {
        rows.push(notice);
    }
    widget(SettingsCard::new("账号与仓库")).children(rows).key(format!("admin-source-page-{plugin_id}")).into_any()
}

/// 连接、取消、刷新和检查登录结果排成一排，对应 Vue 页头和会话操作。
fn action_row(model: &ShellViewModel, plugin: &PluginManifest) -> AnyView {
    let plugin_id = plugin.plugin_id.clone();
    widget(Stack::row(8.0).wrap(true))
        .children((
            connect_button(plugin),
            widget(Button::new("取消")).key(format!("admin-source-auth-cancel-{plugin_id}")).on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program(ShellMessage::Admin(AdminMessage::DismissSourceAuth));
            }),
            widget(Button::new("刷新二维码")).key(format!("admin-source-auth-refresh-{plugin_id}")).on_cx({
                let plugin_id = plugin_id.clone();
                move |_, _: &Activate, cx| cx.dispatch_program(auth_message(plugin_id.clone(), SourceAuthCall::CreateSession))
            }),
            poll_button(model, plugin),
        ))
        .key(format!("admin-source-actions-{plugin_id}"))
        .into_any()
}

fn connect_button(plugin: &PluginManifest) -> AnyView {
    let plugin_id = plugin.plugin_id.clone();
    let enabled = super::support::named_auth_method(plugin, "createSessionMethod").is_some();
    widget(Button::new("连接新账号").disabled(!enabled))
        .key(format!("admin-source-auth-create-{plugin_id}"))
        .on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program(auth_message(plugin_id.clone(), SourceAuthCall::CreateSession));
        })
        .into_any()
}

fn repository_block(model: &ShellViewModel, plugin: &PluginManifest) -> AnyView {
    let repositories = source_repositories(model, plugin);
    if repositories.is_empty() {
        return widget(EmptyState::new("还没有这个来源的仓库").message("连接新账号后，登录成功会按插件声明创建或更新仓库。").compact(true))
            .key(format!("admin-source-page-empty-{}", plugin.plugin_id))
            .into_any();
    }
    let plugin_id = plugin.plugin_id.clone();
    let items = repositories
        .iter()
        .enumerate()
        .map(|(index, repository)| repository_row(&plugin_id, index, repository))
        .collect::<Vec<_>>();
    widget(List::new()).children(items).key(format!("admin-source-page-list-{}", plugin.plugin_id)).into_any()
}

fn repository_row(plugin_id: &str, index: usize, repository: &super::super::workspace::WorkspaceRepository) -> AnyView {
    let path = repository.path.trim();
    let path = if path.is_empty() { "未返回路径" } else { path };
    let plugin_id = plugin_id.to_string();
    widget(Stack::column(6.0))
        .key(format!("admin-source-page-repo-{plugin_id}-{index}"))
        .children((
            widget(ListItem::new(repository.name.clone()).detail(format!("未检查 · {path}"))),
            widget(Stack::row(8.0).wrap(true)).children((
                auth_button("检查", &plugin_id, index, "status", SourceAuthCall::Status),
                auth_button("重新登录", &plugin_id, index, "relogin", SourceAuthCall::CreateSession),
                auth_button("退出", &plugin_id, index, "clear", SourceAuthCall::Clear),
            )),
        ))
        .into_any()
}

fn auth_button(label: &str, plugin_id: &str, index: usize, slot_name: &str, slot: SourceAuthCall) -> AnyView {
    let plugin_id = plugin_id.to_string();
    widget(Button::new(label))
        .key(format!("admin-source-auth-{slot_name}-{plugin_id}-{index}"))
        .on_cx(move |_, _: &Activate, cx| cx.dispatch_program(auth_message(plugin_id.clone(), slot)))
        .into_any()
}

fn poll_button(model: &ShellViewModel, plugin: &PluginManifest) -> AnyView {
    let plugin_id = plugin.plugin_id.clone();
    let needs_cache = requires_local_cache(plugin);
    let can_poll = super::support::named_auth_method(plugin, "pollSessionMethod").is_some()
        && (!needs_cache || !model.admin.source_cache_path.trim().is_empty());
    widget(Button::new("检查登录结果").disabled(!can_poll))
        .key(format!("admin-source-auth-poll-{plugin_id}"))
        .on_cx(move |_, _: &Activate, cx| cx.dispatch_program(auth_message(plugin_id.clone(), SourceAuthCall::Poll)))
        .into_any()
}

/// 扫码和缓存目录。有方法名时缓存目录用选目录，不只有文本框。
fn session_flow(model: &ShellViewModel, plugin: &PluginManifest) -> AnyView {
    let plugin_id = plugin.plugin_id.clone();
    let mut rows = vec![widget(workbench::section_title("扫码登录")).key(format!("admin-source-flow-title-{plugin_id}")).into_any()];
    if requires_local_cache(plugin) {
        rows.push(cache_picker(&plugin_id, &model.admin.source_cache_path));
    }
    rows.push(qr_row(&model.admin.source_auth));
    rows.extend(status_lines(&model.admin.source_auth));
    widget(Stack::column(8.0)).children(rows).key(format!("admin-source-flow-{plugin_id}")).into_any()
}

/// 路径框旁边是选目录。点浏览或按钮都排队文件夹对话框，取消不写路径。
fn cache_picker(plugin_id: &str, cache: &str) -> AnyView {
    let label = if cache.trim().is_empty() { "选择缓存目录" } else { "重新选择缓存目录" };
    widget(Stack::row(8.0).align(nana_ui::runtime::AlignSpec::Center).wrap(true))
        .children((
            widget(PathField::new(cache.to_string()).label("缓存目录").placeholder("选择缓存目录"))
                .key(format!("admin-source-cache-{plugin_id}"))
                .on_cx(|_, event: &TextChanged, cx| {
                    cx.dispatch_program(ShellMessage::Admin(AdminMessage::SetSourceCachePath(event.value.to_string())));
                })
                .on_cx(|_, _: &BrowseRequested, cx| {
                    cx.dispatch_program(ShellMessage::Admin(AdminMessage::ChooseSourceCache));
                }),
            widget(Button::new(label)).key(format!("admin-source-cache-browse-{plugin_id}")).on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program(ShellMessage::Admin(AdminMessage::ChooseSourceCache));
            }),
        ))
        .key(format!("admin-source-cache-row-{plugin_id}"))
        .into_any()
}

/// 建仓要有名称和路径。没有宿主登录结果、名称或路径时禁用提交，并在按钮旁说明缺什么。
fn provision_form(model: &ShellViewModel, plugin: &PluginManifest) -> AnyView {
    let plugin_id = plugin.plugin_id.as_str();
    let name = model.admin.source_repo_name.clone();
    let path = model.admin.source_repo_path.clone();
    let missing = provision_missing(model);
    let blocked = !missing.is_empty();
    widget(Stack::column(8.0))
        .children((
            widget(workbench::section_title("创建仓库")).key(format!("admin-source-provision-title-{plugin_id}")),
            widget(TextInput::new(name).label("名称").placeholder("仓库名称")).key(format!("admin-source-repo-name-{plugin_id}")).on_cx(
                |_, event: &TextChanged, cx| {
                    cx.dispatch_program(ShellMessage::Admin(AdminMessage::SetSourceRepoName(event.value.to_string())));
                },
            ),
            widget(TextInput::new(path).label("路径").placeholder("仓库路径")).key(format!("admin-source-repo-path-{plugin_id}")).on_cx(
                |_, event: &TextChanged, cx| {
                    cx.dispatch_program(ShellMessage::Admin(AdminMessage::SetSourceRepoPath(event.value.to_string())));
                },
            ),
            widget(Stack::row(8.0).align(nana_ui::runtime::AlignSpec::Center).wrap(true)).children((
                widget(Button::new("创建仓库").disabled(blocked)).key(format!("admin-source-provision-submit-{plugin_id}")).on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program(ShellMessage::Admin(AdminMessage::SubmitSourceRepository));
                }),
                blocked.then(|| {
                    widget(ValidationMessage::new(missing.join("，"), ValidationIntent::Danger)).key(format!("admin-source-provision-missing-{plugin_id}"))
                }),
            )),
        ))
        .key(format!("admin-source-provision-{plugin_id}"))
        .into_any()
}

/// 缺的条件写在提交按钮旁边。登录结果缺插件、账号配置、名称或路径时不能创建。
fn provision_missing(model: &ShellViewModel) -> Vec<&'static str> {
    super::source_provision::missing_labels(model)
}

fn qr_row(view: &super::support::SourceAuthView) -> AnyView {
    let Some(data) = view.qr_text.as_deref() else {
        return widget(EmptyState::new("还没有可绘制的二维码").message("创建会话返回 qrurl 或 qrUrl 后，用现有二维码控件画在这里。图片 data URL 不会再编一次。").compact(true))
            .key("admin-source-qr-empty")
            .into_any();
    };
    match QrCode::encode(data, 180.0) {
        Ok(code) => widget(code.label("扫码登录二维码")).key("admin-source-qr").into_any(),
        Err(error) => {
            eprintln!("Nana 登录二维码编码失败：{error}");
            text("登录二维码无法编码。").key("admin-source-qr-error").into_any()
        }
    }
}

fn status_lines(view: &super::support::SourceAuthView) -> Vec<AnyView> {
    view.lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            let (label, value) = labeled(line);
            widget(LabeledValue::new(label, value)).key(format!("admin-source-auth-line-{index}")).into_any()
        })
        .collect()
}

fn labeled(line: &str) -> (&'static str, String) {
    if line == "已登录" || line == "未登录" {
        return ("登录", line.to_string());
    }
    if line == "登录已过期" {
        return ("登录", "已过期".into());
    }
    if let Some(rest) = line.strip_prefix("账号 ") {
        return ("账号", rest.to_string());
    }
    if let Some(rest) = line.strip_prefix("资料 ") {
        return ("资料", rest.to_string());
    }
    ("状态", line.to_string())
}

/// 「已调用」只留在状态里给归约测试看，不铺到认证页上。
fn notice(model: &ShellViewModel) -> Option<AnyView> {
    if !model.admin.action_error.is_empty() {
        return Some(text(model.admin.action_error.clone()).key("admin-source-page-error").into_any());
    }
    let message = model.admin.action_message.trim();
    if message.is_empty() || message.starts_with("已调用") || message.starts_with("正在调用") {
        return None;
    }
    Some(text(message.to_string()).key("admin-source-page-message").into_any())
}

fn requires_local_cache(plugin: &PluginManifest) -> bool {
    plugin
        .contributes
        .get("source")
        .and_then(|item| item.get("authentication"))
        .and_then(|item| item.get("repositoryProvisioning"))
        .and_then(|item| item.get("requiresLocalCache"))
        .and_then(|item| item.as_bool())
        == Some(true)
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

fn auth_message(plugin_id: String, slot: SourceAuthCall) -> ShellMessage {
    ShellMessage::Admin(AdminMessage::CallSourceAuth { plugin_id, slot })
}
