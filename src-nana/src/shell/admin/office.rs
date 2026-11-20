//! Office 转换插件的自定义设置页。
//!
//! 照 office-convert 设置页：展开时调用 `officeConvert.getRuntimeStatus`，页头是
//! 「Office Convert / 运行状态与缓存」。状态对象回来以后才按真实字段画转换器、守护进程和最近自检；
//! 还没有结果时只写空说明，不编进程、端口、缓存条数或时间。自检、关闭守护进程和按资源库清缓存
//! 分别调用已有的插件方法，失败把插件交回的原因写在页上并记日志。转换器模式和自动下载仍由下方通用字段保存。

use std::sync::Arc;

use nana_ui::runtime::view::{AnyView, IntoView, Signal, fields, widget};
use nana_ui::runtime::{Activate, AlignSpec, LengthSpec, Select, SelectChanged, SelectOption};
use nana_ui_core::{RadiusTier, SemanticColorRole as Role};
use serde_json::Value;

use super::super::{ShellMessage, ShellViewModel};
use super::bind::ActionDisabled;
use super::style::{self, Tone, action, column, label, pad, row};
use super::{AdminEffect, AdminMessage, OfficeCall, PluginCallOrigin};
use crate::backend::services::repository::RepositorySummary;

/// Office 转换插件。只有这一张设置页挂运行状态和缓存清理。
pub(super) const PLUGIN_ID: &str = "momobako.service.office-convert";
/// 还没有状态时的说明，和 Vue 在 `status` 为空时写的那句一样。
pub(super) const WAITING: &str = "读取当前转换器、自带运行时与守护进程状态。";
/// 页头标题。
pub(super) const TITLE: &str = "运行状态与缓存";
/// 转换模式仍走下方通用字段，这句一直在。
const CONFIG_NOTE: &str = "转换模式与自动下载选项沿用下方插件通用配置字段保存。";
/// 缓存区标题。
const CACHE_TITLE: &str = "资源库缓存清理";
/// 缓存位置说明，一直在。不代表已经数过缓存文件。
const CACHE_NOTE: &str = "转换后的 PDF 缓存位于资源库 .momo/cache/office-preview 目录。";
/// 没选资源库时「当前资源库」写这个，不拿别的仓库顶上。
const NO_REPO: &str = "未选择";

/// 一次设置页会话。收起设置或换到别的插件时清空，对应 Vue 组件卸载。
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct OfficeRuntime {
    /// 正在等 `officeConvert.getRuntimeStatus`。
    loading: bool,
    /// 正在等 `officeConvert.runRuntimeSelfCheck`。
    self_checking: bool,
    /// 正在等 `officeConvert.shutdownDaemon`。
    stopping: bool,
    /// 正在等 `officeConvert.clearPreviewCache`。
    clearing: bool,
    /// 最近一次失败的原因。成功或重新操作时清掉。
    error: String,
    /// 自检、关闭或清理交回的那句话。刷新运行状态时清掉，操作后再读状态则保留。
    message: String,
    /// 插件交回的运行状态。没有结果时为 `None`，界面写 [`WAITING`]。
    status: Option<Value>,
    /// 缓存清理选中的资源库。空字符串是「选择资源库」。
    selected_repo_id: String,
}

/// 设置页要画的东西。没有状态时 `rows` 为空，画空说明，不补守护进程行。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct OfficeConvertView {
    pub loading: bool,
    pub self_checking: bool,
    pub stopping: bool,
    pub clearing: bool,
    /// 插件交回的 `daemon.running`。没有状态或字段不是 `true` 时为假，关闭按钮不可点。
    pub daemon_running: bool,
    pub error: String,
    pub message: String,
    pub rows: Vec<(String, String)>,
    /// 宿主已经拿到的资源库 `(repoId, name)`。没有列表时为空，不编仓库。
    pub repos: Vec<(String, String)>,
    pub selected_repo_id: String,
    pub selected_repo_name: String,
}

impl OfficeConvertView {
    /// 从 Office 转换状态和当前资源库列表投影。没有状态对象就不排行。
    pub(crate) fn project(model: &ShellViewModel) -> Self {
        let state = &model.admin.office;
        let repos = model
            .admin
            .repository_summaries
            .iter()
            .map(|repo| (repo.repo_id.clone(), repo.name.clone()))
            .collect::<Vec<_>>();
        let selected = repos.iter().find(|(id, _)| id == &state.selected_repo_id);
        Self {
            loading: state.loading,
            self_checking: state.self_checking,
            stopping: state.stopping,
            clearing: state.clearing,
            daemon_running: daemon_running(state),
            error: state.error.clone(),
            message: state.message.clone(),
            rows: state
                .status
                .as_ref()
                .filter(|status| status.is_object())
                .map(status_rows)
                .unwrap_or_default(),
            selected_repo_id: selected.map(|(id, _)| id.clone()).unwrap_or_default(),
            selected_repo_name: selected
                .map(|(_, name)| name.clone())
                .unwrap_or_else(|| NO_REPO.into()),
            repos,
        }
    }
}

/// 展开 Office 转换设置时读一次状态，并在还没选资源库时用宿主已有列表的第一项。别的插件清掉上次的结果。
pub(super) fn on_settings_opened(model: &mut ShellViewModel, plugin_id: &str) {
    if plugin_id != PLUGIN_ID {
        release(model);
        return;
    }
    if model.admin.office.selected_repo_id.is_empty() {
        if let Some(repo) = model.admin.repository_summaries.first() {
            model.admin.office.selected_repo_id = repo.repo_id.clone();
        }
    }
    request(model, false);
}

/// 收起设置时丢掉状态，晚到的结果不再写进来。
pub(super) fn release(model: &mut ShellViewModel) {
    model.admin.office = OfficeRuntime::default();
}

/// 排一次运行状态读取。`preserve_message` 留给自检、关闭和清理之后的那次刷新，成功提示留在页上。
pub(super) fn request(model: &mut ShellViewModel, preserve_message: bool) {
    if !settings_open(model) {
        eprintln!("Nana Office 转换设置没有展开，不读取运行状态");
        return;
    }
    if model.admin.office.loading {
        eprintln!("Nana Office 转换运行状态还在读取，不重复请求");
        return;
    }
    model.admin.office.loading = true;
    model.admin.office.error.clear();
    if !preserve_message {
        model.admin.office.message.clear();
    }
    push_call(model, OfficeCall::Status, Value::Object(Default::default()));
}

/// 下拉框换了资源库。只改选择，不调用插件。
pub(super) fn select_repository(model: &mut ShellViewModel, repo_id: String) {
    if !settings_open(model) {
        eprintln!("Nana Office 转换设置没有展开，不改缓存清理的资源库");
        return;
    }
    model.admin.office.selected_repo_id = repo_id;
}

/// 「运行自检」。调用 `officeConvert.runRuntimeSelfCheck`，不在结果回来前写通过或失败。
pub(super) fn self_check(model: &mut ShellViewModel) {
    if !begin_action(model, OfficeCall::SelfCheck) {
        return;
    }
    model.admin.office.self_checking = true;
    push_call(
        model,
        OfficeCall::SelfCheck,
        Value::Object(Default::default()),
    );
}

/// 「关闭守护进程」。只有状态里 `daemon.running` 为真才调用 `officeConvert.shutdownDaemon`。
pub(super) fn shutdown(model: &mut ShellViewModel) {
    if !settings_open(model) {
        eprintln!("Nana Office 转换设置没有展开，不关闭守护进程");
        return;
    }
    if model.admin.office.stopping {
        eprintln!("Nana Office 转换正在关闭守护进程，不重复请求");
        return;
    }
    if !daemon_running(&model.admin.office) {
        eprintln!("Nana Office 转换守护进程没有在运行，不调用关闭");
        return;
    }
    model.admin.office.stopping = true;
    model.admin.office.error.clear();
    model.admin.office.message.clear();
    push_call(
        model,
        OfficeCall::Shutdown,
        Value::Object(Default::default()),
    );
}

/// 「清理缓存」。选中的资源库必须还在宿主列表里，载荷只带它的 `repoId`。
pub(super) fn clear_cache(model: &mut ShellViewModel) {
    if !settings_open(model) {
        eprintln!("Nana Office 转换设置没有展开，不清理缓存");
        return;
    }
    let Some(repo_id) = selected_repo(model).map(|repo| repo.repo_id.clone()) else {
        eprintln!("Nana Office 转换没有选中资源库，不清理缓存");
        return;
    };
    if !begin_action(model, OfficeCall::ClearCache) {
        return;
    }
    model.admin.office.clearing = true;
    push_call(
        model,
        OfficeCall::ClearCache,
        serde_json::json!({ "repoId": repo_id }),
    );
}

/// 插件调用回来了。设置已经收起就丢掉；失败记下原因，不把没发生的操作写成成功。
pub(super) fn finish(model: &mut ShellViewModel, call: OfficeCall, result: Result<Value, String>) {
    if !settings_open(model) {
        eprintln!("Nana Office 转换设置已经收起，丢掉{}", call_label(call));
        return;
    }
    match call {
        OfficeCall::Status => finish_status(model, result),
        OfficeCall::SelfCheck | OfficeCall::Shutdown | OfficeCall::ClearCache => {
            finish_action(model, call, result)
        }
    }
}

/// 运行状态回来了。不是对象就不排行；失败保留上一份状态。
fn finish_status(model: &mut ShellViewModel, result: Result<Value, String>) {
    model.admin.office.loading = false;
    match result {
        Ok(value) if value.is_null() => {
            model.admin.office.error.clear();
            model.admin.office.status = None;
        }
        Ok(value) if !value.is_object() => {
            eprintln!("Nana Office 转换运行状态不是对象，不画状态行");
            model.admin.office.error.clear();
            model.admin.office.status = None;
        }
        Ok(value) => {
            model.admin.office.error.clear();
            model.admin.office.status = Some(value);
        }
        Err(error) => {
            eprintln!("Nana Office 转换运行状态读取失败：{error}");
            model.admin.office.error = error;
        }
    }
}

/// 自检、关闭或清理回来了。成功后按插件字段写提示，再读一次状态，提示留着。
fn finish_action(model: &mut ShellViewModel, call: OfficeCall, result: Result<Value, String>) {
    match call {
        OfficeCall::SelfCheck => model.admin.office.self_checking = false,
        OfficeCall::Shutdown => model.admin.office.stopping = false,
        OfficeCall::ClearCache => model.admin.office.clearing = false,
        OfficeCall::Status => {}
    }
    match result {
        Ok(value) => {
            model.admin.office.error.clear();
            model.admin.office.message = action_message(call, &value);
            request(model, true);
        }
        Err(error) => {
            eprintln!("Nana Office 转换{}失败：{error}", call_label(call));
            model.admin.office.error = error;
        }
    }
}

/// 操作开始前清掉上一条提示。设置没展开，或这一步还在进行，就不再发一次。
fn begin_action(model: &mut ShellViewModel, call: OfficeCall) -> bool {
    if !settings_open(model) {
        eprintln!("Nana Office 转换设置没有展开，不{}", call_label(call));
        return false;
    }
    let busy = match call {
        OfficeCall::SelfCheck => model.admin.office.self_checking,
        OfficeCall::ClearCache => model.admin.office.clearing,
        OfficeCall::Shutdown => model.admin.office.stopping,
        OfficeCall::Status => model.admin.office.loading,
    };
    if busy {
        eprintln!("Nana Office 转换{}还在进行，不重复请求", call_label(call));
        return false;
    }
    model.admin.office.error.clear();
    model.admin.office.message.clear();
    true
}

fn push_call(model: &mut ShellViewModel, call: OfficeCall, payload: Value) {
    model.admin.push_effect(AdminEffect::CallPlugin {
        plugin_id: PLUGIN_ID.to_string(),
        method: method_name(call).to_string(),
        payload,
        repository_id: None,
        origin: PluginCallOrigin::OfficeConvert(call),
    });
}

fn method_name(call: OfficeCall) -> &'static str {
    match call {
        OfficeCall::Status => "officeConvert.getRuntimeStatus",
        OfficeCall::SelfCheck => "officeConvert.runRuntimeSelfCheck",
        OfficeCall::Shutdown => "officeConvert.shutdownDaemon",
        OfficeCall::ClearCache => "officeConvert.clearPreviewCache",
    }
}

fn call_label(call: OfficeCall) -> &'static str {
    match call {
        OfficeCall::Status => "运行状态",
        OfficeCall::SelfCheck => "运行自检",
        OfficeCall::Shutdown => "关闭守护进程",
        OfficeCall::ClearCache => "清理缓存",
    }
}

fn settings_open(model: &ShellViewModel) -> bool {
    model.admin.active_settings_plugin_id.as_deref() == Some(PLUGIN_ID)
}

fn selected_repo(model: &ShellViewModel) -> Option<&RepositorySummary> {
    let id = model.admin.office.selected_repo_id.as_str();
    if id.is_empty() {
        return None;
    }
    model
        .admin
        .repository_summaries
        .iter()
        .find(|repo| repo.repo_id == id)
}

fn daemon_running(state: &OfficeRuntime) -> bool {
    state
        .status
        .as_ref()
        .and_then(|status| status.get("daemon"))
        .filter(|daemon| daemon.is_object())
        .and_then(|daemon| daemon.get("running"))
        .and_then(Value::as_bool)
        == Some(true)
}

/// 成功调用交回的那句话。清理条数只认插件字段，缺了才按 Vue 写成 0，调用失败时不会走到这里。
fn action_message(call: OfficeCall, value: &Value) -> String {
    match call {
        OfficeCall::SelfCheck => {
            if value.get("ok").and_then(Value::as_bool) == Some(true) {
                "运行时自检通过".into()
            } else {
                text_of(value.get("error")).unwrap_or_else(|| "运行时自检失败".into())
            }
        }
        OfficeCall::Shutdown => {
            if value.get("stopped").and_then(Value::as_bool) == Some(true) {
                "LibreOffice 守护进程已关闭".into()
            } else {
                "当前没有运行中的 LibreOffice 守护进程".into()
            }
        }
        OfficeCall::ClearCache => format!("已清理 {} 个缓存文件", removed_count(value)),
        OfficeCall::Status => String::new(),
    }
}

/// `removed` 缺席或不是数字时用 0，和 Vue `payload?.removed ?? 0` 一样。这只发生在调用已经成功之后。
fn removed_count(value: &Value) -> String {
    let Some(removed) = value.get("removed").filter(|item| !item.is_null()) else {
        return "0".into();
    };
    if let Some(count) = removed.as_i64() {
        return count.to_string();
    }
    if let Some(count) = removed.as_u64() {
        return count.to_string();
    }
    eprintln!("Nana Office 转换清理结果的 removed 不是数字，按 0 显示");
    "0".into()
}

/// 状态行。只在真有状态对象时调用；缺的字段用 Vue 的占位词，不另造进程或时间。
fn status_rows(status: &Value) -> Vec<(String, String)> {
    let daemon = status.get("daemon").filter(|item| item.is_object());
    let check = daemon
        .and_then(|item| item.get("lastSelfCheck"))
        .filter(|item| item.is_object());
    vec![
        (
            "模式".into(),
            missing_text(status.get("converterMode"), "auto"),
        ),
        (
            "自动下载".into(),
            if flag(status.get("autoDownloadLibreOffice")) {
                "开启".into()
            } else {
                "关闭".into()
            },
        ),
        (
            "Microsoft Office".into(),
            converter_text(status.get("microsoftOffice")),
        ),
        (
            "系统 LibreOffice".into(),
            converter_text(status.get("libreofficeSystem")),
        ),
        (
            "自带 LibreOffice".into(),
            converter_text(status.get("libreofficeBundle")),
        ),
        (
            "自带下载地址".into(),
            text_of(status.get("bundledDownloadUrl")).unwrap_or_else(|| "未配置".into()),
        ),
        ("守护进程".into(), daemon_text(daemon)),
        (
            "Helper 类型".into(),
            text_of(daemon.and_then(|item| item.get("helperType")))
                .unwrap_or_else(|| "未声明".into()),
        ),
        (
            "Helper 端口".into(),
            truthy_token(daemon.and_then(|item| item.get("port")))
                .unwrap_or_else(|| "未分配".into()),
        ),
        (
            "Helper 地址".into(),
            text_of(daemon.and_then(|item| item.get("baseUrl"))).unwrap_or_else(|| "未生成".into()),
        ),
        (
            "健康检查".into(),
            if flag(daemon.and_then(|item| item.get("healthy"))) {
                "通过".into()
            } else {
                "未通过".into()
            },
        ),
        (
            "Soffice 就绪".into(),
            tri_state(
                daemon.and_then(|item| item.get("sofficeReady")),
                "已就绪",
                "未就绪",
                "未上报",
            ),
        ),
        (
            "Soffice PID".into(),
            truthy_token(daemon.and_then(|item| item.get("sofficePid")))
                .unwrap_or_else(|| "未上报".into()),
        ),
        (
            "UNO 可用".into(),
            tri_state(
                daemon.and_then(|item| item.get("unoAvailable")),
                "是",
                "否",
                "未上报",
            ),
        ),
        (
            "Python 有效".into(),
            tri_state(
                daemon.and_then(|item| item.get("pythonValid")),
                "是",
                "否",
                "未上报",
            ),
        ),
        (
            "Python 路径".into(),
            text_of(daemon.and_then(|item| item.get("pythonPath")))
                .unwrap_or_else(|| "未上报".into()),
        ),
        ("控制方式".into(), daemon_control_text(daemon)),
        ("最近转换".into(), daemon_convert_text(daemon)),
        ("最近自检".into(), daemon_self_check_text(check)),
        (
            "自检样本".into(),
            text_of(check.and_then(|item| item.get("samplePath")))
                .unwrap_or_else(|| "未记录".into()),
        ),
        (
            "自检输出".into(),
            text_of(check.and_then(|item| item.get("pdfPath"))).unwrap_or_else(|| "未记录".into()),
        ),
        ("自检转换器".into(), self_check_converter_text(check)),
        (
            "自检错误".into(),
            text_of(check.and_then(|item| item.get("error"))).unwrap_or_else(|| "无".into()),
        ),
    ]
}

/// 转换器探测。没有对象写「未知」；可用时带上真实路径和版本，不可用写原因或「不可用」。
fn converter_text(status: Option<&Value>) -> String {
    let Some(status) = status.filter(|item| item.is_object()) else {
        return "未知".into();
    };
    if flag(status.get("available")) {
        let mut parts = vec![text_of(status.get("path")).unwrap_or_else(|| "可用".into())];
        if let Some(version) = text_of(status.get("version")) {
            parts.push(version);
        }
        return parts.join(" | ");
    }
    text_of(status.get("reason")).unwrap_or_else(|| "不可用".into())
}

/// 守护进程一行。没在跑时写错误或「未运行」，不补 PID 和更新时间。
fn daemon_text(daemon: Option<&Value>) -> String {
    let Some(daemon) = daemon else {
        return "未知".into();
    };
    if flag(daemon.get("running")) {
        let pid = truthy_token(daemon.get("pid"))
            .map(|pid| format!("PID {pid}"))
            .unwrap_or_else(|| "运行中".into());
        let soffice = if flag(daemon.get("sofficeReady")) {
            "Soffice 已就绪"
        } else {
            "Soffice 未就绪"
        };
        let mut parts = vec![pid, soffice.into()];
        if let Some(updated) = text_of(daemon.get("updatedAt")) {
            parts.push(format!("更新于 {updated}"));
        }
        return parts.join(" | ");
    }
    text_of(daemon.get("error")).unwrap_or_else(|| "未运行".into())
}

fn daemon_control_text(daemon: Option<&Value>) -> String {
    let Some(control) = daemon
        .and_then(|item| item.get("control"))
        .filter(|item| item.is_object())
    else {
        return "未声明".into();
    };
    let mut parts = Vec::new();
    if let Some(health) = text_of(control.get("health")) {
        parts.push(format!("health={health}"));
    }
    if let Some(shutdown) = text_of(control.get("shutdown")) {
        parts.push(format!("shutdown={shutdown}"));
    }
    if parts.is_empty() {
        "未声明".into()
    } else {
        parts.join(" | ")
    }
}

fn daemon_convert_text(daemon: Option<&Value>) -> String {
    let Some(last) = daemon
        .and_then(|item| item.get("lastConvert"))
        .filter(|item| item.is_object())
    else {
        return "暂无记录".into();
    };
    ["phase", "conversionMode", "sourcePath", "updatedAt"]
        .into_iter()
        .filter_map(|key| text_of(last.get(key)))
        .collect::<Vec<_>>()
        .join(" | ")
}

fn daemon_self_check_text(check: Option<&Value>) -> String {
    let Some(check) = check else {
        return "暂无记录".into();
    };
    let mut parts = vec![if flag(check.get("ok")) {
        "通过".into()
    } else {
        "失败".into()
    }];
    for key in ["converter", "converterVersion", "conversionMode"] {
        if let Some(text) = text_of(check.get(key)) {
            parts.push(text);
        }
    }
    if let Some(size) = number_text(check.get("pdfSizeBytes")) {
        parts.push(format!("{size} bytes"));
    }
    if let Some(duration) = number_text(check.get("durationMs")) {
        parts.push(format!("{duration} ms"));
    }
    if let Some(completed) = text_of(check.get("completedAt")) {
        parts.push(completed);
    }
    parts.join(" | ")
}

fn self_check_converter_text(check: Option<&Value>) -> String {
    let Some(check) = check else {
        return "未记录".into();
    };
    let parts = ["converter", "converterVersion", "converterPath"]
        .into_iter()
        .filter_map(|key| text_of(check.get(key)))
        .collect::<Vec<_>>();
    if parts.is_empty() {
        "未记录".into()
    } else {
        parts.join(" | ")
    }
}

/// `null` 和缺字段用占位词；空字符串留空，和 `??` 一样。
fn missing_text(value: Option<&Value>, fallback: &str) -> String {
    match value {
        None | Some(Value::Null) => fallback.into(),
        Some(value) => text_of(Some(value)).unwrap_or_default(),
    }
}

fn flag(value: Option<&Value>) -> bool {
    value.and_then(Value::as_bool) == Some(true)
}

fn tri_state(value: Option<&Value>, yes: &str, no: &str, unknown: &str) -> String {
    match value.and_then(Value::as_bool) {
        Some(true) => yes.into(),
        Some(false) => no.into(),
        None => unknown.into(),
    }
}

fn text_of(value: Option<&Value>) -> Option<String> {
    let text = value.and_then(Value::as_str).unwrap_or("").trim();
    (!text.is_empty()).then(|| text.to_string())
}

/// 数字 0 和空字符串在 Vue 里是假值，端口和 PID 因此写成「未分配 / 未上报」。
fn truthy_token(value: Option<&Value>) -> Option<String> {
    let value = value?;
    match value {
        Value::String(text) => {
            let text = text.trim();
            (!text.is_empty()).then(|| text.to_string())
        }
        Value::Number(number) if number.as_f64() != Some(0.0) => number_text(Some(value)),
        _ => None,
    }
}

fn number_text(value: Option<&Value>) -> Option<String> {
    let number = value?.as_number()?;
    if let Some(value) = number.as_i64() {
        return Some(value.to_string());
    }
    if let Some(value) = number.as_u64() {
        return Some(value.to_string());
    }
    number.as_f64().map(|value| value.to_string())
}

/// 设置区里的状态和缓存清理。不是 Office 转换时整块不占布局。
pub(super) fn settings_block(status: Signal<Option<OfficeConvertView>>) -> AnyView {
    let shown = status;
    nana_ui::runtime::view::dynamic(status, move |view: &Option<OfficeConvertView>| {
        view.as_ref()
            .map(panel)
            .unwrap_or_else(|| widget(column(0.0)).into_any())
    })
    .visible(move || shown.with(Option::is_some))
    .key("admin-office-status")
    .into_any()
}

/// 整张设置卡：运行状态在上，资源库缓存清理在下，通用配置字段仍在这块下面。
fn panel(view: &OfficeConvertView) -> AnyView {
    let mut body = vec![head(view.loading)];
    if !view.error.is_empty() {
        body.push(note(&view.error, "admin-office-error"));
    }
    if !view.message.is_empty() {
        body.push(note(&view.message, "admin-office-message"));
    }
    body.push(note(CONFIG_NOTE, "admin-office-config"));
    if view.rows.is_empty() {
        body.push(note(WAITING, "admin-office-waiting"));
    } else {
        body.push(rows(&view.rows, "status"));
    }
    body.push(actions(view));
    body.push(cache_section(view));
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
    .key("admin-office-card")
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
                widget(style::eyebrow_text("Office Convert")).key("admin-office-eyebrow"),
                widget(pad(column(0.0), 2.0, 0.0, 0.0, 0.0)).children((widget(style::label_lh(
                    TITLE,
                    14.0,
                    700,
                    Role::Text,
                    1.25,
                ))
                .key("admin-office-title"),)),
            )),
            widget(action(
                if loading { "刷新中" } else { "刷新" },
                None,
                Tone::Plain,
                loading,
            ))
            .prop::<bool, ActionDisabled>(loading)
            .key("admin-office-refresh")
            .on_cx(move |_, _: &Activate, cx| {
                cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::RefreshOfficeStatus));
            }),
        ))
        .key("admin-office-head")
        .into_any()
}

fn actions(view: &OfficeConvertView) -> AnyView {
    let shutdown_disabled = view.stopping || !view.daemon_running;
    widget(row(8.0).align(AlignSpec::Center).wrap(true))
        .children((
            widget(action(
                if view.self_checking {
                    "自检中"
                } else {
                    "运行自检"
                },
                None,
                Tone::Plain,
                view.self_checking,
            ))
            .prop::<bool, ActionDisabled>(view.self_checking)
            .key("admin-office-self-check")
            .on_cx(move |_, _: &Activate, cx| {
                cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::RunOfficeSelfCheck));
            }),
            widget(action(
                if view.stopping {
                    "关闭中"
                } else {
                    "关闭守护进程"
                },
                None,
                Tone::Plain,
                shutdown_disabled,
            ))
            .prop::<bool, ActionDisabled>(shutdown_disabled)
            .key("admin-office-shutdown")
            .on_cx(move |_, _: &Activate, cx| {
                cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::ShutdownOfficeDaemon));
            }),
        ))
        .key("admin-office-actions")
        .into_any()
}

/// 按资源库清理预览缓存。下拉框只列宿主已经拿到的资源库。
fn cache_section(view: &OfficeConvertView) -> AnyView {
    let clear_disabled = view.clearing || view.selected_repo_id.is_empty();
    widget(pad(column(8.0), 12.0, 0.0, 0.0, 0.0).width(LengthSpec::Fill))
        .children((
            widget(column(0.0)).children((
                widget(style::eyebrow_text("Preview Cache")).key("admin-office-cache-eyebrow"),
                widget(pad(column(0.0), 2.0, 0.0, 0.0, 0.0)).children((widget(style::label_lh(
                    CACHE_TITLE,
                    14.0,
                    700,
                    Role::Text,
                    1.25,
                ))
                .key("admin-office-cache-title"),)),
            )),
            rows(
                &[("当前资源库".into(), view.selected_repo_name.clone())],
                "repo",
            ),
            widget(row(8.0).align(AlignSpec::Center).width(LengthSpec::Fill)).children((
                repo_select(view),
                clear_button(view.clearing, clear_disabled),
            )),
            note(CACHE_NOTE, "admin-office-cache-note"),
        ))
        .key("admin-office-cache")
        .into_any()
}

fn clear_button(clearing: bool, disabled: bool) -> AnyView {
    widget(action(
        if clearing {
            "清理中"
        } else {
            "清理缓存"
        },
        None,
        Tone::Plain,
        disabled,
    ))
    .prop::<bool, ActionDisabled>(disabled)
    .key("admin-office-clear")
    .on_cx(move |_, _: &Activate, cx| {
        cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::ClearOfficeCache));
    })
    .into_any()
}

fn repo_select(view: &OfficeConvertView) -> AnyView {
    let options = repo_options(&view.repos);
    let selected = view.selected_repo_id.clone();
    let mut select = Select::new(Some(selected.clone())).options(options.clone());
    {
        let layout = Arc::make_mut(&mut select.style.layout);
        layout.width = Some(LengthSpec::Fill);
        layout.min_width = Some(LengthSpec::Px(0.0));
        layout.height = Some(LengthSpec::Px(32.0));
        layout.font_size = Some(14.0);
        layout.flex_grow = Some(1.0);
        layout.flex_shrink = Some(1.0);
    }
    let value = selected.clone();
    widget(select)
        .prop::<Vec<SelectOption>, fields::select::options>(move || options.clone())
        .prop::<Option<Arc<str>>, fields::select::value>(move || Some(Arc::from(value.as_str())))
        .key("admin-office-repo")
        .on_cx(move |_, event: &SelectChanged, cx| {
            cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::SelectOfficeRepository(
                event.value.to_string(),
            )));
        })
        .into_any()
}

fn repo_options(repos: &[(String, String)]) -> Vec<SelectOption> {
    let mut options = vec![SelectOption::new("", "选择资源库")];
    options.extend(
        repos
            .iter()
            .map(|(id, name)| SelectOption::new(id.clone(), name.clone())),
    );
    options
}

/// 警告色说明。错误、结果、空状态和页底说明都用 Vue `.repository-add-popover__note` 的浅底。
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
fn rows(items: &[(String, String)], prefix: &str) -> AnyView {
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
                    widget(field_label(name)).key(format!("admin-office-{prefix}-label-{index}")),
                    widget(value_text(value)).key(format!("admin-office-{prefix}-value-{index}")),
                ))
                .key(format!("admin-office-{prefix}-row-{index}"))
                .into_any()
        })
        .collect::<Vec<_>>();
    widget(column(8.0).width(LengthSpec::Fill))
        .children(lines)
        .key(format!("admin-office-{prefix}-rows"))
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
    use crate::backend::services::repository::RepositoryBackendSummary;
    use serde_json::json;

    fn open(model: &mut ShellViewModel) {
        model.reduce(ShellMessage::Admin(AdminMessage::ToggleSettings(
            PLUGIN_ID.into(),
        )));
    }

    fn repo(id: &str, name: &str) -> RepositorySummary {
        RepositorySummary {
            repo_id: id.into(),
            name: name.into(),
            path: format!("C:/{id}"),
            backend: RepositoryBackendSummary {
                plugin_id: "momobako.local-filesystem".into(),
                kind: "filesystem".into(),
                name: "本地文件系统".into(),
                capabilities: vec!["read".into()],
            },
            status: "ready".into(),
            asset_count: 0,
            updated_at: "2026-10-08T08:00:00Z".into(),
            local_cache: None,
            authentication: None,
        }
    }

    fn status_calls(model: &mut ShellViewModel) -> usize {
        model
            .admin
            .take_effects()
            .into_iter()
            .filter(|effect| {
                matches!(
                    effect,
                    AdminEffect::CallPlugin {
                        origin: PluginCallOrigin::OfficeConvert(OfficeCall::Status),
                        ..
                    }
                )
            })
            .count()
    }

    #[test]
    fn opening_requests_status_without_inventing_rows() {
        let mut model = ShellViewModel::default();
        open(&mut model);
        let effects = model.admin.take_effects();
        assert!(effects.iter().any(|effect| matches!(
            effect,
            AdminEffect::CallPlugin { plugin_id, method, payload, repository_id, origin }
                if plugin_id == PLUGIN_ID
                    && method == "officeConvert.getRuntimeStatus"
                    && payload == &json!({})
                    && repository_id.is_none()
                    && *origin == PluginCallOrigin::OfficeConvert(OfficeCall::Status)
        )));
        let view = OfficeConvertView::project(&model);
        assert!(view.loading);
        assert!(view.rows.is_empty(), "还没有状态，不画守护进程");
        assert_eq!(view.selected_repo_name, NO_REPO);
        assert!(!view.daemon_running);
        assert!(view.error.is_empty());
    }

    #[test]
    fn known_repositories_are_selectable_and_other_plugins_do_not_request() {
        let mut model = ShellViewModel::default();
        model.admin.note_backends(&[repo("repo-a", "默认资源库")]);
        open(&mut model);
        assert_eq!(status_calls(&mut model), 1);
        let view = OfficeConvertView::project(&model);
        assert_eq!(view.selected_repo_id, "repo-a");
        assert_eq!(view.selected_repo_name, "默认资源库");
        assert_eq!(view.repos, vec![("repo-a".into(), "默认资源库".into())]);

        let mut other = ShellViewModel::default();
        other.reduce(ShellMessage::Admin(AdminMessage::ToggleSettings(
            "momobako.local-filesystem".into(),
        )));
        assert!(other.admin.take_effects().iter().all(|effect| !matches!(
            effect,
            AdminEffect::CallPlugin {
                origin: PluginCallOrigin::OfficeConvert(_),
                ..
            }
        )));
    }

    #[test]
    fn status_rows_use_the_payload_and_missing_fields_use_vue_fallbacks() {
        let mut model = ShellViewModel::default();
        open(&mut model);
        model.admin.take_effects();
        model.reduce(ShellMessage::Admin(AdminMessage::OfficeCallFinished {
            call: OfficeCall::Status,
            result: Ok(json!({
                "converterMode": "libreoffice",
                "autoDownloadLibreOffice": true,
                "microsoftOffice": { "available": false, "reason": "未探测到系统 Microsoft Office 安装。" },
                "libreofficeSystem": { "available": true, "path": "C:/LibreOffice/program/soffice.exe", "version": "25.8.3" },
                "libreofficeBundle": { "available": false, "reason": "未发现已下载的 LibreOffice 运行时。" },
                "bundledDownloadUrl": "https://example.invalid/lo.msi",
                "daemon": {
                    "running": true,
                    "pid": 99,
                    "sofficeReady": true,
                    "updatedAt": "2026-10-13T00:00:00Z",
                    "helperType": "bundled",
                    "port": 18181,
                    "baseUrl": "http://127.0.0.1:18181",
                    "healthy": true,
                    "sofficePid": 100,
                    "unoAvailable": true,
                    "pythonValid": false,
                    "pythonPath": "C:/lo/python.exe",
                    "control": { "health": "http-local", "shutdown": "plugin-call" },
                    "lastConvert": { "phase": "completed", "conversionMode": "uno", "sourcePath": "a.docx", "updatedAt": "2026-10-13T00:01:00Z" },
                    "lastSelfCheck": {
                        "ok": true,
                        "converter": "LibreOffice",
                        "converterVersion": "25.8.3",
                        "conversionMode": "uno",
                        "pdfSizeBytes": 1200,
                        "durationMs": 40,
                        "completedAt": "2026-10-13T00:02:00Z",
                        "samplePath": "C:/tmp/self-check.docx",
                        "pdfPath": "C:/tmp/self-check.pdf",
                        "converterPath": "C:/lo/soffice.exe"
                    }
                }
            })),
        }));
        let rows = OfficeConvertView::project(&model).rows;
        assert_eq!(
            rows,
            vec![
                ("模式".into(), "libreoffice".into()),
                ("自动下载".into(), "开启".into()),
                (
                    "Microsoft Office".into(),
                    "未探测到系统 Microsoft Office 安装。".into()
                ),
                (
                    "系统 LibreOffice".into(),
                    "C:/LibreOffice/program/soffice.exe | 25.8.3".into()
                ),
                (
                    "自带 LibreOffice".into(),
                    "未发现已下载的 LibreOffice 运行时。".into()
                ),
                (
                    "自带下载地址".into(),
                    "https://example.invalid/lo.msi".into()
                ),
                (
                    "守护进程".into(),
                    "PID 99 | Soffice 已就绪 | 更新于 2026-10-13T00:00:00Z".into()
                ),
                ("Helper 类型".into(), "bundled".into()),
                ("Helper 端口".into(), "18181".into()),
                ("Helper 地址".into(), "http://127.0.0.1:18181".into()),
                ("健康检查".into(), "通过".into()),
                ("Soffice 就绪".into(), "已就绪".into()),
                ("Soffice PID".into(), "100".into()),
                ("UNO 可用".into(), "是".into()),
                ("Python 有效".into(), "否".into()),
                ("Python 路径".into(), "C:/lo/python.exe".into()),
                (
                    "控制方式".into(),
                    "health=http-local | shutdown=plugin-call".into()
                ),
                (
                    "最近转换".into(),
                    "completed | uno | a.docx | 2026-10-13T00:01:00Z".into()
                ),
                (
                    "最近自检".into(),
                    "通过 | LibreOffice | 25.8.3 | uno | 1200 bytes | 40 ms | 2026-10-13T00:02:00Z"
                        .into()
                ),
                ("自检样本".into(), "C:/tmp/self-check.docx".into()),
                ("自检输出".into(), "C:/tmp/self-check.pdf".into()),
                (
                    "自检转换器".into(),
                    "LibreOffice | 25.8.3 | C:/lo/soffice.exe".into()
                ),
                ("自检错误".into(), "无".into()),
            ]
        );
        assert!(OfficeConvertView::project(&model).daemon_running);

        model.reduce(ShellMessage::Admin(AdminMessage::OfficeCallFinished {
            call: OfficeCall::Status,
            result: Ok(json!({})),
        }));
        let sparse = OfficeConvertView::project(&model).rows;
        assert_eq!(
            sparse
                .iter()
                .find(|(name, _)| name == "守护进程")
                .map(|(_, value)| value.as_str()),
            Some("未知")
        );
        assert_eq!(
            sparse
                .iter()
                .find(|(name, _)| name == "Helper 端口")
                .map(|(_, value)| value.as_str()),
            Some("未分配")
        );
        assert_eq!(
            sparse
                .iter()
                .find(|(name, _)| name == "最近自检")
                .map(|(_, value)| value.as_str()),
            Some("暂无记录")
        );
        assert!(
            sparse.iter().all(|(_, value)| !value.contains("PID")),
            "空状态对象不编进程号"
        );
    }

    #[test]
    fn failure_keeps_the_previous_status_and_closing_drops_a_late_result() {
        let mut model = ShellViewModel::default();
        open(&mut model);
        model.admin.take_effects();
        model.reduce(ShellMessage::Admin(AdminMessage::OfficeCallFinished {
            call: OfficeCall::Status,
            result: Ok(json!({ "converterMode": "auto" })),
        }));
        model.reduce(ShellMessage::Admin(AdminMessage::OfficeCallFinished {
            call: OfficeCall::Status,
            result: Err("转换服务没有响应".into()),
        }));
        assert_eq!(model.admin.office.error, "转换服务没有响应");
        assert!(model.admin.office.status.is_some(), "失败保留上一次的状态");
        assert!(!model.admin.office.loading);

        open(&mut model);
        model.reduce(ShellMessage::Admin(AdminMessage::OfficeCallFinished {
            call: OfficeCall::Status,
            result: Ok(json!({ "converterMode": "libreoffice" })),
        }));
        assert!(model.admin.office.status.is_none());
        assert!(!model.admin.office.loading);
    }

    #[test]
    fn self_check_shutdown_and_clear_call_the_plugin_methods() {
        let mut model = ShellViewModel::default();
        model.admin.note_backends(&[repo("repo-a", "默认资源库")]);
        open(&mut model);
        model.admin.take_effects();
        model.reduce(ShellMessage::Admin(AdminMessage::OfficeCallFinished {
            call: OfficeCall::Status,
            result: Ok(json!({ "daemon": { "running": true, "pid": 7 } })),
        }));

        model.reduce(ShellMessage::Admin(AdminMessage::RunOfficeSelfCheck));
        assert!(model.admin.take_effects().iter().any(|effect| matches!(
            effect,
            AdminEffect::CallPlugin { method, payload, origin, .. }
                if method == "officeConvert.runRuntimeSelfCheck" && payload == &json!({}) && *origin == PluginCallOrigin::OfficeConvert(OfficeCall::SelfCheck)
        )));
        model.reduce(ShellMessage::Admin(AdminMessage::OfficeCallFinished {
            call: OfficeCall::SelfCheck,
            result: Ok(json!({ "ok": true })),
        }));
        assert_eq!(model.admin.office.message, "运行时自检通过");
        assert_eq!(status_calls(&mut model), 1, "自检成功后再读一次状态");
        assert_eq!(model.admin.office.message, "运行时自检通过");

        model.reduce(ShellMessage::Admin(AdminMessage::ShutdownOfficeDaemon));
        assert!(model.admin.take_effects().iter().any(|effect| matches!(
            effect,
            AdminEffect::CallPlugin { method, origin, .. }
                if method == "officeConvert.shutdownDaemon" && *origin == PluginCallOrigin::OfficeConvert(OfficeCall::Shutdown)
        )));

        model.reduce(ShellMessage::Admin(AdminMessage::ClearOfficeCache));
        assert!(model.admin.take_effects().iter().any(|effect| matches!(
            effect,
            AdminEffect::CallPlugin { method, payload, origin, .. }
                if method == "officeConvert.clearPreviewCache"
                    && payload == &json!({ "repoId": "repo-a" })
                    && *origin == PluginCallOrigin::OfficeConvert(OfficeCall::ClearCache)
        )));
        model.reduce(ShellMessage::Admin(AdminMessage::OfficeCallFinished {
            call: OfficeCall::ClearCache,
            result: Ok(json!({ "repoId": "repo-a", "removed": 3 })),
        }));
        assert_eq!(model.admin.office.message, "已清理 3 个缓存文件");
    }

    #[test]
    fn missing_daemon_missing_repo_and_call_errors_do_not_pretend_success() {
        let mut model = ShellViewModel::default();
        open(&mut model);
        model.admin.take_effects();
        model.reduce(ShellMessage::Admin(AdminMessage::ShutdownOfficeDaemon));
        assert!(
            model.admin.take_effects().is_empty(),
            "没有运行中的守护进程，不调用关闭"
        );
        assert!(model.admin.office.message.is_empty());

        model.reduce(ShellMessage::Admin(AdminMessage::ClearOfficeCache));
        assert!(
            model.admin.take_effects().is_empty(),
            "没有资源库，不调用清理"
        );
        assert!(model.admin.office.message.is_empty());

        model.reduce(ShellMessage::Admin(AdminMessage::RunOfficeSelfCheck));
        model.admin.take_effects();
        model.reduce(ShellMessage::Admin(AdminMessage::OfficeCallFinished {
            call: OfficeCall::SelfCheck,
            result: Err("soffice 没有响应".into()),
        }));
        assert_eq!(model.admin.office.error, "soffice 没有响应");
        assert!(model.admin.office.message.is_empty());
        assert!(!model.admin.office.self_checking);
        assert!(
            model.admin.take_effects().is_empty(),
            "自检失败不再读状态，也不写通过"
        );
    }

    #[test]
    fn refresh_while_loading_does_not_request_twice() {
        let mut model = ShellViewModel::default();
        open(&mut model);
        assert_eq!(status_calls(&mut model), 1);
        model.reduce(ShellMessage::Admin(AdminMessage::RefreshOfficeStatus));
        assert!(model.admin.take_effects().is_empty());
    }
}
