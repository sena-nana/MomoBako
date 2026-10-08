//! 来源账号与仓库设置区。
//!
//! 照 `SourceAuthenticationSettings.vue`：页头「账号与仓库」和「连接新账号」，下面每个来源仓库
//! 一行（名称、登录状态和路径，检查、重新登录、退出），扫码会话打开时多一块流程区（缓存目录、
//! 二维码、取消、刷新二维码、检查登录结果），最后是提示或错误。插件设置区展开时挂在设置头下面。

use nana_ui::runtime::view::{widget, AnyView, IntoView};
use nana_ui::runtime::{Activate, AlignSpec, LengthSpec, QrCode, Stack};
use nana_ui_core::{RadiusTier, SemanticColorRole as Role};

use super::super::{ShellMessage, ShellViewModel};
use super::icons;
use super::source_provision::{self as flow};
use super::style::{self, action, column, label, pad, row, wrapping, Tone};
use super::AdminMessage;
use crate::backend::services::repository::PluginManifest;

/// 整个账号与仓库区，竖排间距 12。
pub(super) fn source_auth_settings(model: &ShellViewModel, plugin: &PluginManifest) -> AnyView {
    let plugin_id = plugin.plugin_id.clone();
    let state = &model.admin.source_auth;
    let own = state.plugin_id == plugin_id;
    let busy = own && state.busy;
    let mut body = vec![head(&plugin_id, busy)];
    let repositories = flow::source_repositories(model, plugin);
    if !repositories.is_empty() {
        let rows = repositories.iter().map(|repository| repository_row(model, &plugin_id, repository, busy)).collect::<Vec<_>>();
        body.push(widget(column(12.0)).children(rows).key(format!("admin-source-repos-{}", style::key_part(&plugin_id))).into_any());
    }
    if own && state.session.is_some() {
        body.push(session_flow(model, plugin));
    }
    if own && !state.error.is_empty() {
        body.push(notice(state.error.clone(), true, &plugin_id));
    } else if own && !state.message.is_empty() {
        body.push(notice(state.message.clone(), false, &plugin_id));
    }
    widget(column(12.0)).children(body).key(format!("admin-source-auth-{}", style::key_part(&plugin_id))).into_any()
}

/// 页头：标题和说明在左，「连接新账号」在右。
fn head(plugin_id: &str, busy: bool) -> AnyView {
    let id = plugin_id.to_string();
    widget(style::spread(10.0, AlignSpec::Center))
        .children((
            widget(Stack::column(4.0).width(LengthSpec::Shrink).min_width(LengthSpec::Px(0.0)).grow(1.0).shrink(1.0)).children((
                widget(label("账号与仓库", 14.0, 700, Role::Text)).key(format!("admin-source-title-{}", style::key_part(&plugin_id))),
                widget(wrapping(label("认证由 Source 插件处理，宿主只保存安全凭据引用。", 12.0, 400, Role::Muted))),
            )),
            widget(action("连接新账号", Some(icons::KEY_ROUND), Tone::Primary, busy))
                .key(format!("admin-source-connect-{}", style::key_part(&plugin_id)))
                .on_cx(move |_, _: &Activate, cx| {
                    cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::BeginSourceAuth { plugin_id: id.clone(), repo_id: None }));
                }),
        ))
        .into_any()
}

/// `.source-auth-settings__repository`：内边距 12、`border-soft` 边线、md 圆角、`bg-subtle`。
fn repository_row(model: &ShellViewModel, plugin_id: &str, repository: &super::super::WorkspaceRepository, busy: bool) -> AnyView {
    let repo_id = repository.repo_id.clone();
    let status = flow::status_label(model, &repo_id);
    let path = flow::display_path(model, repository);
    let button = |text: &'static str, icon, tone, message: AdminMessage, key: String| {
        widget(action(text, icon, tone, busy))
            .key(key)
            .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(ShellMessage::Admin(message.clone())))
            .into_any()
    };
    let actions = vec![
        button(
            "检查",
            Some(icons::REFRESH_CW),
            Tone::Plain,
            AdminMessage::CheckSourceAuth { plugin_id: plugin_id.into(), repo_id: repo_id.clone() },
            format!("admin-source-check-{}", style::key_part(&repo_id)),
        ),
        button(
            "重新登录",
            None,
            Tone::Plain,
            AdminMessage::BeginSourceAuth { plugin_id: plugin_id.into(), repo_id: Some(repo_id.clone()) },
            format!("admin-source-relogin-{}", style::key_part(&repo_id)),
        ),
        button(
            "退出",
            Some(icons::LOG_OUT),
            Tone::Danger,
            AdminMessage::ClearSourceAuth { plugin_id: plugin_id.into(), repo_id: repo_id.clone() },
            format!("admin-source-clear-{}", style::key_part(&repo_id)),
        ),
    ];
    let frame = pad(style::spread(10.0, AlignSpec::Center), 12.0, 12.0, 12.0, 12.0)
        .surface(Role::Subtle)
        .outline(Role::BorderSoft, 1.0)
        .radius(RadiusTier::Md);
    widget(frame)
        .children((
            widget(Stack::column(2.0).width(LengthSpec::Shrink).min_width(LengthSpec::Px(0.0)).grow(1.0).shrink(1.0)).children((
                widget(label(repository.name.clone(), 14.0, 700, Role::Text)).key(format!("admin-source-repo-{}", style::key_part(&repo_id))),
                widget(wrapping(label(format!("{status} · {path}"), 12.0, 400, Role::Muted))).key(format!("admin-source-repo-status-{}", style::key_part(&repo_id))),
            )),
            widget(row(10.0).wrap(true).justify(nana_ui::runtime::JustifySpec::End)).children(actions),
        ))
        .key(format!("admin-source-repo-row-{}", style::key_part(&repo_id)))
        .into_any()
}

/// 扫码流程区：缓存目录、二维码和三个操作。
fn session_flow(model: &ShellViewModel, plugin: &PluginManifest) -> AnyView {
    let plugin_id = plugin.plugin_id.clone();
    let state = &model.admin.source_auth;
    let busy = state.busy;
    let mut rows = Vec::new();
    if flow::requires_local_cache(plugin) {
        let text = if state.cache_path.is_empty() { "选择缓存目录" } else { "重新选择缓存目录" };
        // 流程区是网格，Vue 的按钮被拉满整行，文字居中。
        let mut button = action(text, None, Tone::Plain, busy);
        std::sync::Arc::make_mut(&mut button.style.layout).width = Some(LengthSpec::Fill);
        rows.push(
            widget(button)
                .key(format!("admin-source-cache-{}", style::key_part(&plugin_id)))
                .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::ChooseSourceCache)))
                .into_any(),
        );
    }
    if !state.cache_path.is_empty() {
        rows.push(widget(wrapping(label(state.cache_path.clone(), 12.0, 400, Role::Muted))).key(format!("admin-source-cache-path-{}", style::key_part(&plugin_id))).into_any());
    }
    if let Some(code) = qr_code(state.session.as_ref()) {
        rows.push(widget(row(0.0).width(LengthSpec::Fill).justify(nana_ui::runtime::JustifySpec::Center)).children((code,)).into_any());
    }
    let poll_id = plugin_id.clone();
    let refresh_id = plugin_id.clone();
    let target = state.target_repo_id.clone();
    let can_poll = state.can_poll(flow::requires_local_cache(plugin));
    let poll_icon = if busy { Some(icons::LOADER_CIRCLE) } else { None };
    rows.push(
        widget(row(10.0).wrap(true).width(LengthSpec::Fill).justify(nana_ui::runtime::JustifySpec::End))
            .children((
                widget(action("取消", None, Tone::Plain, busy)).key(format!("admin-source-cancel-{}", style::key_part(&plugin_id))).on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::CancelSourceAuth));
                }),
                widget(action("刷新二维码", None, Tone::Plain, busy)).key(format!("admin-source-refresh-{}", style::key_part(&plugin_id))).on_cx(move |_, _: &Activate, cx| {
                    cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::BeginSourceAuth { plugin_id: refresh_id.clone(), repo_id: target.clone() }));
                }),
                widget(action("检查登录结果", poll_icon, Tone::Primary, !can_poll)).key(format!("admin-source-poll-{}", style::key_part(&plugin_id))).on_cx(move |_, _: &Activate, cx| {
                    cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::PollSourceAuth { plugin_id: poll_id.clone() }));
                }),
            ))
            .into_any(),
    );
    let frame = pad(column(12.0), 12.0, 12.0, 12.0, 12.0).surface(Role::Subtle).outline(Role::BorderSoft, 1.0).radius(RadiusTier::Md);
    widget(frame).children(rows).key(format!("admin-source-flow-{}", style::key_part(&plugin_id))).into_any()
}

/// 二维码。Vue 画会话里的二维码图片；Nana 用同一会话的扫码地址重新编码，内容一致，
/// 180 见方、白底。会话只有图片没有地址时不画，并记日志。
fn qr_code(session: Option<&serde_json::Value>) -> Option<AnyView> {
    let session = session?;
    let url = ["qrUrl", "qrurl"]
        .into_iter()
        .find_map(|key| session.get(key).and_then(|item| item.as_str()).map(str::trim).filter(|text| !text.is_empty() && !text.starts_with("data:")));
    let Some(url) = url else {
        if session.get("qrImage").or_else(|| session.get("qrimg")).is_some_and(|item| !item.is_null()) {
            eprintln!("Nana 扫码会话只有二维码图片，没有扫码地址，无法原生绘制");
        }
        return None;
    };
    match QrCode::encode(url, 180.0) {
        Ok(code) => Some(widget(code.label("扫码登录二维码")).key("admin-source-qr").into_any()),
        Err(error) => {
            eprintln!("Nana 登录二维码编码失败：{error}");
            None
        }
    }
}

/// 提示行：12 号，错误用危险色。
fn notice(text: String, error: bool, plugin_id: &str) -> AnyView {
    widget(pad(column(0.0), 4.0, 0.0, 0.0, 0.0))
        .children((widget(wrapping(label(text, 12.0, 400, if error { Role::Danger } else { Role::Muted }))).key(format!("admin-source-notice-{}", style::key_part(&plugin_id))),))
        .into_any()
}
