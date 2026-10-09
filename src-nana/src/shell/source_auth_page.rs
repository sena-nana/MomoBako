//! 来源账号与仓库设置区。
//!
//! 照 `SourceAuthenticationSettings.vue`：页头「账号与仓库」和「连接新账号」，下面每个来源仓库
//! 一行（名称、登录状态和路径，检查、重新登录、退出），扫码会话打开时多一块流程区（缓存目录、
//! 二维码、取消、刷新二维码、检查登录结果），最后是提示或错误。插件设置区展开时挂在设置头下面。
//!
//! 视图只建一次，只读插件管理面板的信号：仓库行按仓库 id 做键，按钮的禁用、文案和二维码都是绑定，
//! 处理器只带插件和仓库 id，会变的目标仓库在点下去时现读信号。

use nana_ui::runtime::view::{fields, widget, AnyView, FieldWrite, IntoView, Item, Signal, Store, StoreList, StorePath};
use nana_ui::runtime::{Activate, AlignSpec, LengthSpec, QrCode, Stack};
use nana_ui_core::{RadiusTier, SemanticColorRole as Role};

use super::super::{ShellMessage, ShellViewModel};
use super::bind::{row_text, ActionDisabled};
use super::icons;
use super::source_provision::{self as flow};
use super::style::{self, action, column, label, pad, row, wrapping, Tone};
use super::AdminMessage;
use crate::backend::services::repository::PluginManifest;

/// 二维码边长。
const QR_SIZE: f32 = 180.0;

/// 账号与仓库区要显示的东西（仓库行在 [`SourceRepoRow`]）。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SourceAuthView {
    /// 这个插件的登录流程有调用在途。
    pub busy: bool,
    pub session: Option<SessionView>,
    /// 错误（真）或提示。
    pub notice: Option<(String, bool)>,
}

/// 扫码流程区。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SessionView {
    pub requires_cache: bool,
    pub cache_button: &'static str,
    pub cache_path: String,
    /// 二维码的扫码地址；会话只有图片时为 `None`，不画。
    pub qr_url: Option<String>,
    pub can_poll: bool,
    /// 「刷新二维码」重新登录的仓库。
    pub target_repo_id: Option<String>,
}

/// 一个来源仓库行。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SourceRepoRow {
    pub repo_id: String,
    pub name: String,
    /// 「登录状态 · 路径」。
    pub status: String,
}

impl SourceRepoRow {
    /// 仓库行的键。
    pub(crate) fn key(row: &SourceRepoRow) -> String {
        row.repo_id.clone()
    }
}

impl SourceAuthView {
    /// 从 ViewModel 取 `plugin` 的账号与仓库区。登录状态只属于正在流程里的那个插件。
    pub(crate) fn project(model: &ShellViewModel, plugin: &PluginManifest) -> (Self, Vec<SourceRepoRow>) {
        let state = &model.admin.source_auth;
        let own = state.plugin_id == plugin.plugin_id;
        let repositories = flow::source_repositories(model, plugin)
            .into_iter()
            .map(|repository| SourceRepoRow {
                repo_id: repository.repo_id.clone(),
                name: repository.name.clone(),
                status: format!("{} · {}", flow::status_label(model, &repository.repo_id), flow::display_path(model, repository)),
            })
            .collect();
        let requires_cache = flow::requires_local_cache(plugin);
        let session = state.session.as_ref().filter(|_| own).map(|session| SessionView {
            requires_cache,
            cache_button: if state.cache_path.is_empty() { "选择缓存目录" } else { "重新选择缓存目录" },
            cache_path: state.cache_path.clone(),
            qr_url: qr_url(session),
            can_poll: state.can_poll(requires_cache),
            target_repo_id: state.target_repo_id.clone(),
        });
        let notice = if own && !state.error.is_empty() {
            Some((state.error.clone(), true))
        } else if own && !state.message.is_empty() {
            Some((state.message.clone(), false))
        } else {
            None
        };
        (Self { busy: own && state.busy, session, notice }, repositories)
    }
}

/// 会话里的扫码地址。Vue 画会话里的二维码图片；Nana 用同一会话的扫码地址重新编码，内容一致。
/// 会话只有图片没有地址时不画，并记日志。
fn qr_url(session: &serde_json::Value) -> Option<String> {
    let url = ["qrUrl", "qrurl"]
        .into_iter()
        .find_map(|key| session.get(key).and_then(|item| item.as_str()).map(str::trim).filter(|text| !text.is_empty() && !text.starts_with("data:")));
    if url.is_none() && session.get("qrImage").or_else(|| session.get("qrimg")).is_some_and(|item| !item.is_null()) {
        eprintln!("Nana 扫码会话只有二维码图片，没有扫码地址，无法原生绘制");
    }
    url.map(str::to_string)
}

/// 整个账号与仓库区，竖排间距 12。`plugin_id` 是展开了设置的这张卡片的插件。
pub(super) fn source_auth_settings(plugin_id: &str, source: Signal<Option<SourceAuthView>>, repos: Store<Vec<SourceRepoRow>>) -> AnyView {
    let pid = style::key_part(plugin_id);
    let busy = move || source.with(|view| view.as_ref().is_some_and(|view| view.busy));
    let repo_plugin = plugin_id.to_string();
    let rows = repos
        .keyed(SourceRepoRow::key)
        .each(move |item| repository_row(&repo_plugin, item, source))
        .gap(12.0)
        .visible(move || !repos.is_empty())
        .key(format!("admin-source-repos-{pid}"));
    let notice = source_notice(source, &pid);
    // 不是来源插件时整块不占布局。
    widget(column(12.0))
        .visible(move || source.with(Option::is_some))
        .children((head(plugin_id, busy), rows, session_flow(plugin_id, source), notice))
        .key(format!("admin-source-auth-{pid}"))
        .into_any()
}

/// 页头：标题和说明在左，「连接新账号」在右。
fn head(plugin_id: &str, busy: impl Fn() -> bool + Send + 'static) -> AnyView {
    let id = plugin_id.to_string();
    widget(style::spread(10.0, AlignSpec::Center))
        .children((
            widget(Stack::column(4.0).width(LengthSpec::Shrink).min_width(LengthSpec::Px(0.0)).grow(1.0).shrink(1.0)).children((
                widget(label("账号与仓库", 14.0, 700, Role::Text)).key(format!("admin-source-title-{}", style::key_part(plugin_id))),
                widget(wrapping(label("认证由 Source 插件处理，宿主只保存安全凭据引用。", 12.0, 400, Role::Muted))),
            )),
            widget(action("连接新账号", Some(icons::KEY_ROUND), Tone::Primary, false))
                .prop::<bool, ActionDisabled>(busy)
                .key(format!("admin-source-connect-{}", style::key_part(plugin_id)))
                .on_cx(move |_, _: &Activate, cx| {
                    cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::BeginSourceAuth { plugin_id: id.clone(), repo_id: None }));
                }),
        ))
        .into_any()
}

/// 仓库行在 Store 里的句柄。
type RepoItem = Item<Store<Vec<SourceRepoRow>>, String, SourceRepoRow>;

/// `.source-auth-settings__repository`：内边距 12、`border-soft` 边线、md 圆角、`bg-subtle`。
/// 行的身份是仓库 id（建行时取，之后不变），按钮带着它和插件 id。
fn repository_row(plugin_id: &str, item: RepoItem, source: Signal<Option<SourceAuthView>>) -> AnyView {
    let repo_id = item.get_untracked().repo_id;
    let busy = move || source.with(|view| view.as_ref().is_some_and(|view| view.busy));
    let button = |text: &'static str, icon, tone, message: AdminMessage, key: String| {
        widget(action(text, icon, tone, false))
            .prop::<bool, ActionDisabled>(busy)
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
                style::bound(label(String::new(), 14.0, 700, Role::Text), row_text(item, |row| &row.name))
                    .key(format!("admin-source-repo-{}", style::key_part(&repo_id))),
                style::bound(wrapping(label(String::new(), 12.0, 400, Role::Muted)), row_text(item, |row| &row.status))
                    .key(format!("admin-source-repo-status-{}", style::key_part(&repo_id))),
            )),
            widget(row(10.0).wrap(true).justify(nana_ui::runtime::JustifySpec::End)).children(actions),
        ))
        .key(format!("admin-source-repo-row-{}", style::key_part(&repo_id)))
        .into_any()
}

/// 会话里的一个值。没有会话时读默认值。
fn session_value<R: Default + 'static>(source: Signal<Option<SourceAuthView>>, pick: fn(&SessionView) -> R) -> impl Fn() -> R + Send + 'static {
    move || source.with(|view| view.as_ref().and_then(|view| view.session.as_ref()).map(pick).unwrap_or_default())
}

/// 扫码流程区：缓存目录、二维码和三个操作。没有会话时不占布局。
fn session_flow(plugin_id: &str, source: Signal<Option<SourceAuthView>>) -> AnyView {
    let pid = style::key_part(plugin_id);
    let busy = move || source.with(|view| view.as_ref().is_some_and(|view| view.busy));
    // 流程区是网格，Vue 的按钮被拉满整行，文字居中。
    let mut cache = action("选择缓存目录", None, Tone::Plain, false);
    std::sync::Arc::make_mut(&mut cache.style.layout).width = Some(LengthSpec::Fill);
    let cache = widget(cache)
        .prop::<String, fields::button::label>(session_value(source, |session| session.cache_button.to_string()))
        .prop::<bool, ActionDisabled>(busy)
        .visible(session_value(source, |session| session.requires_cache))
        .key(format!("admin-source-cache-{pid}"))
        .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::ChooseSourceCache)));
    let cache_path = style::bound(wrapping(label(String::new(), 12.0, 400, Role::Muted)), session_value(source, |session| session.cache_path.clone()))
        .visible(session_value(source, |session| !session.cache_path.is_empty()))
        .key(format!("admin-source-cache-path-{pid}"));
    let placeholder = QrCode::from_modules(vec![false], 1, QR_SIZE).map(|code| code.label("扫码登录二维码"));
    let qr = match placeholder {
        Ok(code) => widget(code).prop::<Option<String>, QrPayload>(session_value(source, |session| session.qr_url.clone())).key("admin-source-qr").into_any(),
        Err(error) => {
            eprintln!("Nana 建不出二维码占位：{error}");
            ().into_any()
        }
    };
    let qr_row = widget(row(0.0).width(LengthSpec::Fill).justify(nana_ui::runtime::JustifySpec::Center))
        .visible(session_value(source, |session| session.qr_url.is_some()))
        .children((qr,));
    let refresh_id = plugin_id.to_string();
    let poll_id = plugin_id.to_string();
    let poll_icon = move || busy().then_some(icons::LOADER_CIRCLE);
    let actions = widget(row(10.0).wrap(true).width(LengthSpec::Fill).justify(nana_ui::runtime::JustifySpec::End)).children((
        widget(action("取消", None, Tone::Plain, false))
            .prop::<bool, ActionDisabled>(busy)
            .key(format!("admin-source-cancel-{pid}"))
            .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::CancelSourceAuth))),
        widget(action("刷新二维码", None, Tone::Plain, false))
            .prop::<bool, ActionDisabled>(busy)
            .key(format!("admin-source-refresh-{pid}"))
            .on_cx(move |_, _: &Activate, cx| {
                // 目标仓库随会话变，点下去时现读。
                let repo_id = source.with_untracked(|view| view.as_ref().and_then(|view| view.session.as_ref()).and_then(|session| session.target_repo_id.clone()));
                cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::BeginSourceAuth { plugin_id: refresh_id.clone(), repo_id }));
            }),
        widget(action("检查登录结果", None, Tone::Primary, false))
            .prop::<Option<nana_ui_core::Icon>, super::bind::ButtonIcon>(poll_icon)
            .prop::<bool, ActionDisabled>(session_value(source, |session| !session.can_poll))
            .key(format!("admin-source-poll-{pid}"))
            .on_cx(move |_, _: &Activate, cx| {
                cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::PollSourceAuth { plugin_id: poll_id.clone() }));
            }),
    ));
    let frame = pad(column(12.0), 12.0, 12.0, 12.0, 12.0).surface(Role::Subtle).outline(Role::BorderSoft, 1.0).radius(RadiusTier::Md);
    widget(frame)
        .visible(move || source.with(|view| view.as_ref().is_some_and(|view| view.session.is_some())))
        .children((cache, cache_path, qr_row, actions))
        .key(format!("admin-source-flow-{pid}"))
        .into_any()
}

/// 提示行：12 号，错误用危险色。没有提示时不占布局。
fn source_notice(source: Signal<Option<SourceAuthView>>, pid: &str) -> AnyView {
    let notice = move || source.with(|view| view.as_ref().and_then(|view| view.notice.clone()));
    widget(pad(column(0.0), 4.0, 0.0, 0.0, 0.0))
        .visible(move || notice().is_some())
        .children((style::bound(wrapping(label(String::new(), 12.0, 400, Role::Muted)), move || notice().map(|(text, _)| text).unwrap_or_default())
            .foreground(move || Some(if notice().is_some_and(|(_, error)| error) { Role::Danger } else { Role::Muted }))
            .key(format!("admin-source-notice-{pid}")),))
        .into_any()
}

/// 二维码的扫码地址：换了地址重新编码，模块换掉，边长和读屏名称不变。没有地址或编码失败时留着原样，
/// 外面那一行按有没有地址显隐。
pub(crate) struct QrPayload;

impl FieldWrite<QrCode, Option<String>> for QrPayload {
    const FIELD: &'static str = "QrCode.modules";

    fn write(target: &mut QrCode, url: Option<String>) {
        let Some(url) = url else {
            return;
        };
        match QrCode::encode(&url, target.size) {
            Ok(code) => {
                target.modules = code.modules;
                target.width = code.width;
            }
            Err(error) => eprintln!("Nana 登录二维码编码失败：{error}"),
        }
    }

    fn differs(target: &QrCode, url: &Option<String>) -> bool {
        let Some(url) = url else {
            return false;
        };
        QrCode::encode(url, target.size).map(|code| code.modules != target.modules || code.width != target.width).unwrap_or(false)
    }
}
