//! Office 转换设置页。
//!
//! 结构对齐 `External/Plugins/office-convert` 的设置页：运行状态、自检、
//! 清缓存和停守护进程。没有 `officeConvert.getRuntimeStatus` 快照时只列出缺的字段。

use nana_ui::runtime::view::{text, widget, AnyView, IntoView};
use nana_ui::runtime::{Activate, AlignSpec, Button, LabeledValue, List, Stack};

use super::super::workbench;
use super::super::{ShellMessage, ShellViewModel};
use super::{AdminMessage, OfficeAction, PluginCallOrigin};
use crate::backend::services::repository::PluginManifest;

const PLUGIN_ID: &str = "momobako.service.office-convert";
const STATUS_METHOD: &str = "officeConvert.getRuntimeStatus";
const SELF_CHECK_METHOD: &str = "officeConvert.runRuntimeSelfCheck";
const CLEAR_METHOD: &str = "officeConvert.clearPreviewCache";
const STOP_METHOD: &str = "officeConvert.shutdownDaemon";
/// 和 Vue 设置页同一组标签。没有快照时每行写「未加载」。
const OFFICE_LABELS: &[&str] = &[
    "模式",
    "自动下载",
    "Microsoft Office",
    "系统 LibreOffice",
    "自带 LibreOffice",
    "自带下载地址",
    "守护进程",
    "Helper 类型",
    "Helper 端口",
    "Helper 地址",
    "健康检查",
    "Soffice 就绪",
    "Soffice PID",
    "UNO 可用",
    "Python 有效",
    "Python 路径",
    "控制方式",
    "最近转换",
    "最近自检",
    "自检样本",
    "自检输出",
    "自检转换器",
    "自检错误",
];

/// 官方 Office 转换插件，或设置页标题就是「Office 转换」。
pub fn is_office_settings(plugin: &PluginManifest) -> bool {
    plugin.plugin_id == PLUGIN_ID
        || plugin.legacy_plugin_ids.iter().any(|id| id == PLUGIN_ID)
        || super::support::settings_page_label(plugin) == Some("Office 转换")
}

/// 运行状态与缓存。按钮只打 Vue 写死的四个方法。
pub(super) fn office_card(model: &ShellViewModel) -> AnyView {
    let pending = model.admin.office_pending;
    let mut rows = vec![head(pending)];
    if !model.admin.office_error.is_empty() {
        rows.push(text(model.admin.office_error.clone()).key("admin-office-error").into_any());
    } else if !model.admin.office_message.is_empty() {
        rows.push(text(model.admin.office_message.clone()).key("admin-office-message").into_any());
    }
    rows.push(widget(workbench::wrapping_note("转换模式与自动下载选项沿用插件通用配置字段保存。")).key("admin-office-hint").into_any());
    if let Some(status) = model.admin.office_status.as_ref() {
        rows.push(status_list(status));
    } else if model.admin.office_error.is_empty() {
        rows.push(super::gap::unloaded_fields(OFFICE_LABELS, "admin-office-missing"));
        rows.push(widget(workbench::wrapping_note("读取当前转换器、自带运行时与守护进程状态。")).key("admin-office-read").into_any());
    }
    rows.push(action_row(model, pending));
    rows.push(cache_section(model, pending));
    widget(nana_ui::runtime::SettingsCard::new("运行状态与缓存")).children(rows).key("admin-office-card").into_any()
}

fn head(pending: Option<OfficeAction>) -> AnyView {
    let loading = pending == Some(OfficeAction::Status);
    widget(Stack::bar(8.0).align(AlignSpec::Center))
        .children((
            widget(workbench::eyebrow("Office Convert")).key("admin-office-eyebrow"),
            widget(Stack::spacer()),
            widget(Button::new(if loading { "刷新中" } else { "刷新" }).disabled(pending.is_some()))
                .key("admin-office-refresh")
                .on_cx(|_, _: &Activate, cx| cx.dispatch_program(office_message(OfficeAction::Status))),
        ))
        .into_any()
}

fn action_row(model: &ShellViewModel, pending: Option<OfficeAction>) -> AnyView {
    let running = daemon_running(model.admin.office_status.as_ref());
    let checking = pending == Some(OfficeAction::SelfCheck);
    let stopping = pending == Some(OfficeAction::StopDaemon);
    widget(Stack::row(8.0).wrap(true))
        .children((
            widget(Button::new(if checking { "自检中" } else { "运行自检" }).disabled(pending.is_some()))
                .key("admin-office-self-check")
                .on_cx(|_, _: &Activate, cx| cx.dispatch_program(office_message(OfficeAction::SelfCheck))),
            widget(Button::new(if stopping { "关闭中" } else { "关闭守护进程" }).disabled(pending.is_some() || !running))
                .key("admin-office-stop")
                .on_cx(|_, _: &Activate, cx| cx.dispatch_program(office_message(OfficeAction::StopDaemon))),
        ))
        .key("admin-office-actions")
        .into_any()
}

fn cache_section(model: &ShellViewModel, pending: Option<OfficeAction>) -> AnyView {
    let selected = model
        .workspace
        .repositories
        .iter()
        .find(|repository| repository.repo_id == model.admin.office_repo_id);
    let current = selected.map(|repository| repository.name.clone()).unwrap_or_else(|| "未选择".into());
    let clearing = pending == Some(OfficeAction::ClearCache);
    let mut choices = Vec::new();
    for repository in &model.workspace.repositories {
        let repo_id = repository.repo_id.clone();
        let chosen = model.admin.office_repo_id == repo_id;
        let label = if chosen { format!("已选 {}", repository.name) } else { repository.name.clone() };
        choices.push(
            widget(Button::new(label).disabled(pending.is_some()))
                .key(format!("admin-office-repo-{repo_id}"))
                .on_cx(move |_, _: &Activate, cx| {
                    cx.dispatch_program(ShellMessage::Admin(AdminMessage::SelectOfficeRepository(repo_id.clone())));
                })
                .into_any(),
        );
    }
    if choices.is_empty() {
        choices.push(widget(workbench::meta("还没有可选资源库")).key("admin-office-repo-empty").into_any());
    }
    widget(Stack::column(8.0))
        .children((
            widget(workbench::eyebrow("Preview Cache")).key("admin-office-cache-eyebrow"),
            widget(workbench::section_title("资源库缓存清理")).key("admin-office-cache-title"),
            widget(LabeledValue::new("当前资源库", current)).key("admin-office-repo"),
            widget(Stack::row(8.0).wrap(true)).children(choices).key("admin-office-repos"),
            widget(Button::new(if clearing { "清理中" } else { "清理缓存" }).disabled(pending.is_some() || selected.is_none()))
                .key("admin-office-clear")
                .on_cx(|_, _: &Activate, cx| cx.dispatch_program(office_message(OfficeAction::ClearCache))),
            widget(workbench::wrapping_note("转换后的 PDF 缓存位于资源库 .momo/cache/office-preview 目录。")).key("admin-office-cache-note"),
        ))
        .key("admin-office-cache")
        .into_any()
}

/// 快照里有的字段才画。缺的键写成「未返回」，不补一个可用状态。
fn status_list(status: &serde_json::Value) -> AnyView {
    let daemon = status.get("daemon");
    let rows = [
        ("模式", text_field(status, "converterMode")),
        ("自动下载", bool_text(status.get("autoDownloadLibreOffice"))),
        ("Microsoft Office", nested_text(status, "microsoftOffice")),
        ("系统 LibreOffice", nested_text(status, "libreofficeSystem")),
        ("自带 LibreOffice", nested_text(status, "libreofficeBundle")),
        ("自带下载地址", text_field(status, "bundledDownloadUrl")),
        ("守护进程", daemon_line(daemon)),
        ("Helper 类型", daemon_text(daemon, "helperType")),
        ("Helper 端口", daemon_text(daemon, "port")),
        ("Helper 地址", daemon_text(daemon, "baseUrl")),
        ("健康检查", bool_text(daemon.and_then(|item| item.get("healthy")))),
        ("Soffice 就绪", bool_text(daemon.and_then(|item| item.get("sofficeReady")))),
        ("Soffice PID", daemon_text(daemon, "sofficePid")),
        ("UNO 可用", bool_text(daemon.and_then(|item| item.get("unoAvailable")))),
        ("Python 有效", bool_text(daemon.and_then(|item| item.get("pythonValid")))),
        ("Python 路径", daemon_text(daemon, "pythonPath")),
    ];
    let items = rows
        .into_iter()
        .enumerate()
        .map(|(index, (label, value))| widget(LabeledValue::new(label, value)).key(format!("admin-office-row-{index}")).into_any())
        .collect::<Vec<_>>();
    widget(List::new()).children(items).key("admin-office-list").into_any()
}

fn text_field(value: &serde_json::Value, key: &str) -> String {
    value.get(key).and_then(scalar).unwrap_or_else(|| "未返回".into())
}

fn daemon_text(daemon: Option<&serde_json::Value>, key: &str) -> String {
    daemon.and_then(|item| item.get(key)).and_then(scalar).unwrap_or_else(|| "未返回".into())
}

fn nested_text(status: &serde_json::Value, key: &str) -> String {
    let Some(value) = status.get(key) else {
        return "未返回".into();
    };
    if let Some(text) = scalar(value) {
        return text;
    }
    let available = value.get("available").and_then(|item| item.as_bool());
    match available {
        Some(true) => value.get("path").and_then(scalar).unwrap_or_else(|| "可用".into()),
        Some(false) => value.get("reason").and_then(scalar).unwrap_or_else(|| "不可用".into()),
        None => "未返回".into(),
    }
}

fn bool_text(value: Option<&serde_json::Value>) -> String {
    match value.and_then(|item| item.as_bool()) {
        Some(true) => "是".into(),
        Some(false) => "否".into(),
        None => "未返回".into(),
    }
}

fn scalar(value: &serde_json::Value) -> Option<String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
        .or_else(|| value.as_i64().map(|item| item.to_string()))
        .or_else(|| value.as_u64().map(|item| item.to_string()))
        .or_else(|| value.as_bool().map(|item| if item { "是".into() } else { "否".into() }))
}

fn daemon_line(daemon: Option<&serde_json::Value>) -> String {
    let Some(daemon) = daemon else {
        return "未返回".into();
    };
    match daemon.get("running").and_then(|item| item.as_bool()) {
        Some(true) => daemon.get("pid").and_then(scalar).map(|pid| format!("PID {pid}")).unwrap_or_else(|| "运行中".into()),
        Some(false) => daemon.get("error").and_then(scalar).unwrap_or_else(|| "未运行".into()),
        None => "未返回".into(),
    }
}

fn daemon_running(status: Option<&serde_json::Value>) -> bool {
    status.and_then(|item| item.get("daemon")).and_then(|item| item.get("running")).and_then(|item| item.as_bool()) == Some(true)
}

fn office_message(action: OfficeAction) -> ShellMessage {
    ShellMessage::Admin(AdminMessage::RunOffice(action))
}

/// 按动作打到 Office 插件。清理缓存没有选中的仓库时不发请求。
pub(super) fn queue_office(model: &mut ShellViewModel, action: OfficeAction) {
    if model.admin.office_pending.is_some() {
        eprintln!("Nana Office 转换设置正在等待上一次调用");
        return;
    }
    let (method, payload) = match action {
        OfficeAction::Status => (STATUS_METHOD, serde_json::json!({})),
        OfficeAction::SelfCheck => (SELF_CHECK_METHOD, serde_json::json!({})),
        OfficeAction::StopDaemon => {
            if !daemon_running(model.admin.office_status.as_ref()) {
                eprintln!("Nana Office 守护进程没有运行，不发送关闭");
                return;
            }
            (STOP_METHOD, serde_json::json!({}))
        }
        OfficeAction::ClearCache => {
            let repo_id = model.admin.office_repo_id.trim();
            if repo_id.is_empty() {
                eprintln!("Nana Office 清理缓存需要先选择资源库");
                return;
            }
            (CLEAR_METHOD, serde_json::json!({ "repoId": repo_id }))
        }
    };
    model.admin.office_pending = Some(action);
    model.admin.office_error.clear();
    model.admin.effects.push(super::AdminEffect::CallPlugin {
        plugin_id: PLUGIN_ID.into(),
        method: method.into(),
        payload,
        repository_id: None,
        origin: PluginCallOrigin::Office,
    });
}

/// 刷新成功才替换快照。其它动作只记下返回里真实有的句子。
pub(super) fn finish_office(model: &mut ShellViewModel, method: &str, result: Result<serde_json::Value, String>) {
    model.admin.office_pending = None;
    match result {
        Ok(payload) => {
            model.admin.office_error.clear();
            if method == STATUS_METHOD {
                model.admin.office_status = Some(payload);
                model.admin.office_message.clear();
            } else {
                model.admin.office_message = office_notice(method, &payload);
            }
        }
        Err(error) => {
            eprintln!("Nana Office 转换调用失败：{method}：{error}");
            model.admin.office_message.clear();
            model.admin.office_error = if error.is_empty() { "Office 转换调用失败。".into() } else { error };
        }
    }
}

fn office_notice(method: &str, payload: &serde_json::Value) -> String {
    if method == CLEAR_METHOD {
        return match payload.get("removed").and_then(|item| item.as_u64()) {
            Some(count) => format!("已清理 {count} 个缓存文件"),
            None => "清理调用已返回，但没有 removed 字段。".into(),
        };
    }
    if method == STOP_METHOD {
        return match payload.get("stopped").and_then(|item| item.as_bool()) {
            Some(true) => "LibreOffice 守护进程已关闭".into(),
            Some(false) => "当前没有运行中的 LibreOffice 守护进程".into(),
            None => "关闭调用已返回，但没有 stopped 字段。".into(),
        };
    }
    if method == SELF_CHECK_METHOD {
        return match payload.get("ok").and_then(|item| item.as_bool()) {
            Some(true) => "运行时自检通过".into(),
            Some(false) => payload.get("error").and_then(scalar).unwrap_or_else(|| "运行时自检失败".into()),
            None => "自检调用已返回，但没有 ok 字段。".into(),
        };
    }
    "Office 转换调用已返回。".into()
}
