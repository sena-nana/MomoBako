//! 下载服务插件的自定义设置页。
//!
//! 照 `service-downloader` 的 `register.js`：展开「下载服务」时调用 `downloader.getRuntimeStatus`，
//! 页头是「Downloader / aria2 运行状态」和「刷新」，有状态才画运行时、aria2、版本、RPC、任务数、
//! 下载目录和下载源。还没有结果时只写 Vue 的空说明，不编任务数、目录或下载源。aria2 状态里的
//! `secret` 丢掉，不进状态也不进界面。

use std::sync::Arc;

use nana_ui::runtime::view::{AnyView, IntoView, Signal, widget};
use nana_ui::runtime::{Activate, AlignSpec, LengthSpec};
use nana_ui_core::{RadiusTier, SemanticColorRole as Role};
use serde_json::Value;

use super::super::{ShellMessage, ShellViewModel};
use super::bind::ActionDisabled;
use super::style::{self, Tone, action, column, label, pad};
use super::{AdminEffect, AdminMessage, PluginCallOrigin};

/// 下载服务插件。只有这一张设置页挂运行状态。
pub(super) const PLUGIN_ID: &str = "momobako.service.downloader";
/// 读取运行状态的插件方法，和清单 `apiTests` 里的方法一致。
const METHOD: &str = "downloader.getRuntimeStatus";
/// 还没有状态时的说明，和 Vue 在 `status` 为空时写的那句一样。
pub(super) const WAITING: &str = "查看 aria2 运行时、任务队列和下载目录状态。";
/// 页底说明，一直在。
const FOOTER: &str = "音乐下载、歌词导出与 LibreOffice 运行时下载统一复用这套 aria2 任务层。";
/// 状态卡标题。
pub(super) const TITLE: &str = "aria2 运行状态";

/// 一次运行状态读取。收起设置或换到别的插件时清空，对应 Vue 组件卸载。
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct DownloaderRuntime {
    /// 正在等 `downloader.getRuntimeStatus`。
    pub loading: bool,
    /// 最近一次失败的原因。成功或重新读取时清掉。
    pub error: String,
    /// 插件交回的状态。没有结果时为 `None`，界面写 [`WAITING`]。
    pub status: Option<Value>,
}

/// 状态卡要画的东西。没有状态时 `rows` 为空，画空说明。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DownloaderStatusView {
    pub loading: bool,
    pub error: String,
    pub rows: Vec<(String, String)>,
}

impl DownloaderStatusView {
    /// 从下载服务状态投影。没有状态就不排行，避免把缺字段画成 0 个任务。
    pub(crate) fn project(state: &DownloaderRuntime) -> Self {
        Self {
            loading: state.loading,
            error: state.error.clone(),
            rows: state.status.as_ref().map(status_rows).unwrap_or_default(),
        }
    }
}

/// 展开下载服务设置时读一次状态；别的插件清掉上次的结果。
pub(super) fn on_settings_opened(model: &mut ShellViewModel, plugin_id: &str) {
    if plugin_id == PLUGIN_ID {
        request(model);
    } else {
        release(model);
    }
}

/// 收起设置时丢掉状态，晚到的结果不再写进来。
pub(super) fn release(model: &mut ShellViewModel) {
    model.admin.downloader = DownloaderRuntime::default();
}

/// 排一次运行状态读取。设置没展开、或上一次还没回来时不重复请求。
pub(super) fn request(model: &mut ShellViewModel) {
    if model.admin.active_settings_plugin_id.as_deref() != Some(PLUGIN_ID) {
        eprintln!("Nana 下载服务设置没有展开，不读取运行状态");
        return;
    }
    if model.admin.downloader.loading {
        eprintln!("Nana 下载服务运行状态还在读取，不重复请求");
        return;
    }
    model.admin.downloader.loading = true;
    model.admin.downloader.error.clear();
    model.admin.push_effect(AdminEffect::CallPlugin {
        plugin_id: PLUGIN_ID.to_string(),
        method: METHOD.to_string(),
        payload: Value::Object(Default::default()),
        repository_id: None,
        origin: PluginCallOrigin::DownloaderStatus,
    });
}

/// 运行状态回来了。设置已经收起或换了插件就丢掉；`secret` 不留在状态里。
pub(super) fn finish(model: &mut ShellViewModel, result: Result<Value, String>) {
    if model.admin.active_settings_plugin_id.as_deref() != Some(PLUGIN_ID) {
        eprintln!("Nana 下载服务设置已经收起，丢掉运行状态");
        return;
    }
    model.admin.downloader.loading = false;
    match result {
        Ok(value) if value.is_null() => {
            model.admin.downloader.error.clear();
            model.admin.downloader.status = None;
        }
        Ok(value) => {
            model.admin.downloader.error.clear();
            model.admin.downloader.status = Some(without_secret(value));
        }
        Err(error) => {
            eprintln!("Nana 下载服务运行状态读取失败：{error}");
            model.admin.downloader.error = error;
        }
    }
}

/// 插件交回的状态行。只在真有状态对象时调用；缺的字段用 Vue 的占位词，不另造一条来源。
fn status_rows(status: &Value) -> Vec<(String, String)> {
    let aria2 = status.get("aria2").filter(|item| !item.is_null());
    vec![
        (
            "运行时".into(),
            nonempty(status.get("runtime")).unwrap_or_else(|| "aria2".into()),
        ),
        ("aria2 状态".into(), aria2_running(aria2)),
        (
            "aria2 版本".into(),
            nonempty(aria2.and_then(|item| item.get("version"))).unwrap_or_else(|| "未知".into()),
        ),
        (
            "RPC 地址".into(),
            nonempty(aria2.and_then(|item| item.get("rpcUrl"))).unwrap_or_else(|| "未启动".into()),
        ),
        ("任务数".into(), queue_size(status.get("queueSize"))),
        (
            "下载目录".into(),
            nonempty(status.get("downloadsDir")).unwrap_or_else(|| "未初始化".into()),
        ),
        (
            "下载源".into(),
            nonempty(status.get("downloadUrl")).unwrap_or_else(|| "未配置".into()),
        ),
    ]
}

/// aria2 是否在跑。没有对象写「未知」；在跑时带上真实的 PID 和来源，没在跑写错误或「未运行」。
fn aria2_running(aria2: Option<&Value>) -> String {
    let Some(aria2) = aria2 else {
        return "未知".into();
    };
    if aria2.get("running").and_then(Value::as_bool) == Some(true) {
        let mut parts = vec!["运行中".to_string()];
        if let Some(pid) = aria2.get("pid").and_then(Value::as_u64) {
            parts.push(format!("PID {pid}"));
        }
        if let Some(source) = nonempty(aria2.get("source")) {
            parts.push(source);
        }
        return parts.join(" | ");
    }
    nonempty(aria2.get("error")).unwrap_or_else(|| "未运行".into())
}

/// 任务数只认插件交回的数字。字段不在时照 Vue `queueSize ?? 0` 写 0，不在没有状态对象时调用。
fn queue_size(value: Option<&Value>) -> String {
    match value.and_then(Value::as_u64) {
        Some(count) => count.to_string(),
        None => "0".into(),
    }
}

fn nonempty(value: Option<&Value>) -> Option<String> {
    let text = value.and_then(Value::as_str).unwrap_or("").trim();
    (!text.is_empty()).then(|| text.to_string())
}

/// 丢掉 aria2 的 `secret`。不把密钥写进日志。
fn without_secret(mut value: Value) -> Value {
    let dropped = value
        .get_mut("aria2")
        .and_then(Value::as_object_mut)
        .is_some_and(|aria2| aria2.remove("secret").is_some());
    if dropped {
        eprintln!("Nana 下载服务运行状态里的 secret 已丢掉，不进入界面");
    }
    value
}

/// 设置区里的状态卡。不是下载服务、或设置没收起时整块不占布局。
pub(super) fn settings_block(status: Signal<Option<DownloaderStatusView>>) -> AnyView {
    let shown = status;
    nana_ui::runtime::view::dynamic(status, move |view: &Option<DownloaderStatusView>| {
        view.as_ref()
            .map(panel)
            .unwrap_or_else(|| widget(column(0.0)).into_any())
    })
    .visible(move || shown.with(Option::is_some))
    .key("admin-downloader-status")
    .into_any()
}

/// 整张状态卡：浅强调底、细边、页头、提示和状态行。
fn panel(view: &DownloaderStatusView) -> AnyView {
    let mut body = vec![head(view.loading)];
    if !view.error.is_empty() {
        body.push(note(&view.error, "admin-downloader-error"));
    }
    if view.rows.is_empty() {
        body.push(note(WAITING, "admin-downloader-waiting"));
    } else {
        body.push(rows(&view.rows));
    }
    body.push(note(FOOTER, "admin-downloader-footer"));
    let fill = style::SoftFill::new(
        style::Soft::RoleMix {
            role: Role::AccentSoft,
            ratio: 0.42,
            base: Role::Subtle,
        },
        Some(RadiusTier::Md),
    )
    .bordered(Role::BorderSoft, 1.0);
    widget(
        pad(column(10.0), 10.0, 10.0, 10.0, 10.0)
            .painter(fill)
            .with_layout(|layout| {
                layout.width = Some(LengthSpec::Fill);
            }),
    )
    .children(body)
    .key("admin-downloader-card")
    .into_any()
}

/// 页头：眉题、标题和刷新。读取中按钮写「刷新中」并禁用。
fn head(loading: bool) -> AnyView {
    widget(style::spread(10.0, AlignSpec::Start))
        .children((
            widget(
                column(0.0)
                    .width(LengthSpec::Shrink)
                    .min_width(LengthSpec::Px(0.0))
                    .grow(1.0)
                    .shrink(1.0),
            )
            .children((
                widget(style::eyebrow_text("Downloader")).key("admin-downloader-eyebrow"),
                widget(pad(column(0.0), 2.0, 0.0, 0.0, 0.0)).children((widget(style::label_lh(
                    TITLE,
                    14.0,
                    700,
                    Role::Text,
                    1.25,
                ))
                .key("admin-downloader-title"),)),
            )),
            widget(action(
                if loading { "刷新中" } else { "刷新" },
                None,
                Tone::Plain,
                loading,
            ))
            .prop::<bool, ActionDisabled>(loading)
            .key("admin-downloader-refresh")
            .on_cx(move |_, _: &Activate, cx| {
                cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::RefreshDownloaderStatus));
            }),
        ))
        .key("admin-downloader-head")
        .into_any()
}

/// 警告色说明。错误、空状态和页底说明都用 Vue `.repository-add-popover__note` 的浅底。
fn note(text: &str, key: &'static str) -> AnyView {
    let fill = style::SoftFill::new(
        style::Soft::Alpha {
            role: Role::Warning,
            alpha: 0.12,
        },
        Some(RadiusTier::Sm),
    );
    widget(
        pad(column(0.0), 8.0, 10.0, 8.0, 10.0)
            .painter(fill)
            .with_layout(|layout| {
                layout.width = Some(LengthSpec::Fill);
            }),
    )
    .children((widget(style::wrapping(style::label_lh(
        text,
        12.0,
        400,
        Role::Warning,
        1.45,
    )))
    .key(key),))
    .into_any()
}

/// 状态行：上为 11 号大写弱标签，下为右对齐的值。第一行没有上边线。
fn rows(items: &[(String, String)]) -> AnyView {
    let lines = items
        .iter()
        .enumerate()
        .map(|(index, (name, value))| {
            let mut line = column(6.0).width(LengthSpec::Fill);
            if index > 0 {
                line = style::top_rule(line)
                    .with_layout(|layout| layout.padding_top = Some(LengthSpec::Px(12.0)));
            }
            widget(line)
                .children((
                    widget(field_label(name)).key(format!("admin-downloader-label-{index}")),
                    widget(value_text(value)).key(format!("admin-downloader-value-{index}")),
                ))
                .key(format!("admin-downloader-row-{index}"))
                .into_any()
        })
        .collect::<Vec<_>>();
    widget(column(8.0).width(LengthSpec::Fill))
        .children(lines)
        .key("admin-downloader-rows")
        .into_any()
}

fn field_label(text: &str) -> nana_ui::runtime::Text {
    let mut text = label(text.to_uppercase(), 11.0, 700, Role::Faint);
    Arc::make_mut(&mut text.style.layout).letter_spacing = Some(0.4);
    text
}

fn value_text(text: &str) -> nana_ui::runtime::Text {
    style::align_end(style::wrapping(label(text, 13.0, 400, Role::Text)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn open(model: &mut ShellViewModel, plugin_id: &str) {
        model.reduce(ShellMessage::Admin(AdminMessage::ToggleSettings(
            plugin_id.into(),
        )));
    }

    #[test]
    fn opening_downloader_settings_requests_status_without_inventing_rows() {
        let mut model = ShellViewModel::default();
        open(&mut model, PLUGIN_ID);
        let effects = model.admin.take_effects();
        assert!(effects.iter().any(|effect| matches!(
            effect,
            AdminEffect::CallPlugin { plugin_id, method, payload, repository_id, origin }
                if plugin_id == PLUGIN_ID
                    && method == METHOD
                    && payload == &json!({})
                    && repository_id.is_none()
                    && *origin == PluginCallOrigin::DownloaderStatus
        )));
        assert!(model.admin.downloader.loading);
        let view = DownloaderStatusView::project(&model.admin.downloader);
        assert!(view.rows.is_empty(), "还没有状态，不画任务数");
        assert!(view.error.is_empty());
    }

    #[test]
    fn other_plugins_do_not_request_downloader_status() {
        let mut model = ShellViewModel::default();
        open(&mut model, "momobako.local-filesystem");
        assert!(model.admin.take_effects().iter().all(|effect| !matches!(
            effect,
            AdminEffect::CallPlugin {
                origin: PluginCallOrigin::DownloaderStatus,
                ..
            }
        )));
        assert!(!model.admin.downloader.loading);
    }

    #[test]
    fn status_rows_use_the_payload_and_drop_the_secret() {
        let mut model = ShellViewModel::default();
        open(&mut model, PLUGIN_ID);
        model.admin.take_effects();
        model.reduce(ShellMessage::Admin(AdminMessage::DownloaderStatusLoaded(Ok(json!({
            "runtime": "aria2",
            "aria2": { "running": true, "pid": 42, "source": "bundled", "version": "1.37.0", "rpcUrl": "http://127.0.0.1:16831/jsonrpc", "secret": "do-not-show" },
            "queueSize": 2,
            "downloadsDir": "D:/downloads",
            "downloadUrl": "https://example.invalid/aria2.zip"
        })))));
        let status = model.admin.downloader.status.clone().expect("状态写进来了");
        assert!(
            status["aria2"].get("secret").is_none(),
            "secret 不留在状态里"
        );
        let rows = DownloaderStatusView::project(&model.admin.downloader).rows;
        assert_eq!(
            rows,
            vec![
                ("运行时".into(), "aria2".into()),
                ("aria2 状态".into(), "运行中 | PID 42 | bundled".into()),
                ("aria2 版本".into(), "1.37.0".into()),
                ("RPC 地址".into(), "http://127.0.0.1:16831/jsonrpc".into()),
                ("任务数".into(), "2".into()),
                ("下载目录".into(), "D:/downloads".into()),
                ("下载源".into(), "https://example.invalid/aria2.zip".into()),
            ]
        );
        assert!(rows.iter().all(|(_, value)| value != "do-not-show"));
        assert!(!model.admin.downloader.loading);
    }

    #[test]
    fn missing_fields_use_vue_fallbacks_and_failure_keeps_the_previous_status() {
        let mut model = ShellViewModel::default();
        open(&mut model, PLUGIN_ID);
        model.admin.take_effects();
        model.reduce(ShellMessage::Admin(AdminMessage::DownloaderStatusLoaded(
            Ok(json!({ "aria2": { "running": false } })),
        )));
        let rows = DownloaderStatusView::project(&model.admin.downloader).rows;
        assert_eq!(
            rows.iter()
                .find(|(name, _)| name == "任务数")
                .map(|(_, value)| value.as_str()),
            Some("0")
        );
        assert_eq!(
            rows.iter()
                .find(|(name, _)| name == "aria2 状态")
                .map(|(_, value)| value.as_str()),
            Some("未运行")
        );
        model.reduce(ShellMessage::Admin(AdminMessage::DownloaderStatusLoaded(
            Err("aria2 没有响应".into()),
        )));
        assert_eq!(model.admin.downloader.error, "aria2 没有响应");
        assert!(
            model.admin.downloader.status.is_some(),
            "失败保留上一次的状态"
        );
        assert!(!model.admin.downloader.loading);
    }

    #[test]
    fn closing_settings_drops_a_late_status() {
        let mut model = ShellViewModel::default();
        open(&mut model, PLUGIN_ID);
        model.admin.take_effects();
        open(&mut model, PLUGIN_ID);
        model.reduce(ShellMessage::Admin(AdminMessage::DownloaderStatusLoaded(
            Ok(json!({ "queueSize": 9 })),
        )));
        assert!(model.admin.downloader.status.is_none());
        assert!(!model.admin.downloader.loading);
    }

    #[test]
    fn refresh_while_loading_does_not_request_twice() {
        let mut model = ShellViewModel::default();
        open(&mut model, PLUGIN_ID);
        assert_eq!(
            model
                .admin
                .take_effects()
                .iter()
                .filter(|effect| matches!(effect, AdminEffect::CallPlugin { .. }))
                .count(),
            1
        );
        model.reduce(ShellMessage::Admin(AdminMessage::RefreshDownloaderStatus));
        assert!(model.admin.take_effects().is_empty());
    }
}
