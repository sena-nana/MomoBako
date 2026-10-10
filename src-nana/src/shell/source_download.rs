//! 来源下载的开始和收尾。
//!
//! 对应 Vue `sourceEntryActions.ts` 的 `runDownloadAction`：选好去处以后任务弹层出「{动作名} · 准备下载 …」，调用
//! 插件时「正在下载 …」（[`DownloadRun::begin`]）；插件交回以后（下载到其他资源库时还导入、同步了目标资源库）进度行
//! 写完「下载完成」收起，再照 Vue `context.refreshRepo()` 刷新当前资源库（[`finish`]）。
//!
//! 部分失败：网易云这类插件下载歌单时逐条下载，失败的条目放进结果的 `failed`，整个调用照样成功。Vue 只在进度行写
//! 「下载完成，N 项失败」，可紧接着的 `refreshRepo()` 开始「加载资源库」、占住进度行，这句话从来看不到，失败的条目
//! 只打在控制台；插件自己写的失败日志宿主也不收（`mutsuki_host::call_session` 只取调用的输出）。这里把失败的条数
//! 写进状态区（来源 [`FailureSource::Download`]），每一条的名称和原因写进系统日志，提示里写明去「日志」看原因。
//! 收尾的刷新不清这条提示（`RefreshRun::keeps_status`），用户开始下一个操作时照常清掉；全部成功时不提示。
//!
//! 什么也没下到：插件报告的是一批（结果里有 `completed` 数组）却一项也没有、也没有交回文件时，状态区用警告色提醒
//! 「没有下载到任何条目」。Nana 下载歌单不列目录，由插件按歌单标识取来源里完整的歌单；网易云插件取不到歌单详情时
//! 什么也不下、照样交回空结果，不提醒的话界面上只有一闪而过的「下载完成」。

use serde_json::{json, Value};

use crate::backend::services::repository::{SystemLogLocationInput, SystemLogWriteRequest};

use super::super::entry_actions::DownloadProgress;
use super::super::repo_operation::{OperationId, OperationPatch, OperationStart};
use super::super::status::FailureSource;
use super::super::ShellViewModel;
use super::source_target::TargetCandidate;
use super::AdminEffect;

/// 一次开始了的来源下载：任务弹层里的进度行，收尾的提示和日志要用的名字。下载结果带着它回来。
#[derive(Clone, Debug, PartialEq)]
pub struct DownloadRun {
    /// 任务弹层里这次下载的进度行。
    pub operation: OperationId,
    /// 动作名和下载的条目名。
    pub progress: DownloadProgress,
    pub plugin_id: String,
    pub method: String,
    /// 点下载时的当前资源库：插件按它取来源凭据，失败的日志也记在它名下。
    pub repository_id: Option<String>,
    /// 下载到其他资源库时的目标资源库；下载到本地时为空。
    pub target: Option<TargetCandidate>,
}

impl DownloadRun {
    /// 选好去处，开始下载：任务弹层照 Vue「{动作名} · 准备下载 …」10%，调用插件时「正在下载 …」36% 不确定进度。
    pub(crate) fn begin(
        model: &mut ShellViewModel,
        progress: DownloadProgress,
        plugin_id: String,
        method: String,
        repository_id: Option<String>,
        target: Option<TargetCandidate>,
    ) -> Self {
        let operation = model.operation.start(OperationStart::new(progress.label.clone(), progress.preparing(), 10.0));
        model.operation.update(operation, OperationPatch::step(progress.running(), 36.0).with_indeterminate(true));
        Self { operation, progress, plugin_id, method, repository_id, target }
    }
}

/// 下载结果回来了。成功时进度行照 Vue 写完「下载完成」（有失败的条目时「下载完成，N 项失败」）收起；有要告诉用户的
/// 结果（[`notice`]）时写进状态区和系统日志；再刷新当前资源库，有提示时这一轮刷新不清它。失败时进度行收起，原因
/// 写进文件列。
pub(crate) fn finish(model: &mut ShellViewModel, run: DownloadRun, result: Result<Value, String>) {
    let payload = match result {
        Ok(payload) => payload,
        Err(error) => {
            eprintln!("Nana 来源下载失败：{}：{error}", run.method);
            model.operation.cancel(run.operation);
            model.files.error = if error.is_empty() { format!("{} 调用失败。", run.method) } else { error };
            return;
        }
    };
    model.files.error.clear();
    let failed = failed_items(&payload);
    let detail = if failed.is_empty() { "下载完成".to_string() } else { format!("下载完成，{} 项失败", failed.len()) };
    model.operation.update(run.operation, OperationPatch::step(detail, 100.0).with_indeterminate(false));
    model.operation.finish(run.operation);
    let Some(notice) = notice(&run, &payload, &failed) else {
        model.workspace.request_repository_refresh();
        return;
    };
    eprintln!("Nana 来源下载的结果：{}", notice.message);
    model.admin.push_effect(AdminEffect::WriteLogs(notice.logs));
    if notice.warning {
        model.status.warn(FailureSource::Download, notice.message);
    } else {
        model.status.fail(FailureSource::Download, notice.message);
    }
    model.workspace.refresh_after_download();
}

/// 收尾时要告诉用户的结果：状态区的一句话（失败还是提醒）和写进系统日志的记录。
struct Notice {
    message: String,
    /// 提醒而不是失败：状态区用警告色。
    warning: bool,
    logs: Vec<SystemLogWriteRequest>,
}

/// 下载结果里要告诉用户的：有失败的条目时写几项失败、去「日志」看原因，每一条写一条日志；插件报告的是一批、却一项
/// 也没有（网易云下载歌单时取不到歌单详情就什么也不下，照样交回空结果）时提醒没有下载到任何条目。全部成功时没有。
fn notice(run: &DownloadRun, payload: &Value, failed: &[FailedItem]) -> Option<Notice> {
    let (label, subject) = (&run.progress.label, &run.progress.subject);
    if !failed.is_empty() {
        return Some(Notice {
            message: format!("{label}「{subject}」有 {} 项失败，原因见「日志」。", failed.len()),
            warning: false,
            logs: failed.iter().map(|item| item.log(run)).collect(),
        });
    }
    let empty_batch = payload.get("completed").and_then(Value::as_array).is_some_and(Vec::is_empty);
    if empty_batch && super::source_target::result_paths(payload).is_empty() {
        let message = format!("{label}「{subject}」没有下载到任何条目。");
        let context = json!({ "method": run.method, "subject": subject, "summary": payload.get("summary") });
        let log = host_log(run, "warn", "downloadEmpty", message.clone(), context);
        return Some(Notice { message, warning: true, logs: vec![log] });
    }
    None
}

/// 插件报告失败的一条：结果里 `failed[]` 的一项，网易云是 `{ songId, songName, error }`。
#[derive(Clone, Debug, PartialEq)]
struct FailedItem {
    /// 失败的是哪一项：歌曲名、名称或标题，都没有时是「歌曲 {标识}」或「第 N 项」。
    name: String,
    reason: String,
    /// 插件交回的原样内容，写进日志的上下文。
    detail: Value,
}

impl FailedItem {
    /// 写进系统日志的一条：哪次下载里的哪一项、为什么失败，上下文带插件交回的原样内容。
    fn log(&self, run: &DownloadRun) -> SystemLogWriteRequest {
        let message = format!("{}「{}」里的「{}」下载失败：{}", run.progress.label, run.progress.subject, self.name, self.reason);
        let context = json!({ "method": run.method, "subject": run.progress.subject, "item": self.detail });
        host_log(run, "error", "downloadItemFailed", message, context)
    }
}

/// 宿主写的一条来源下载日志：分类 `source`，记在来源资源库和插件名下。
fn host_log(run: &DownloadRun, level: &str, action: &str, message: String, context: Value) -> SystemLogWriteRequest {
    SystemLogWriteRequest {
        level: level.into(),
        category: "source".into(),
        action: action.into(),
        message,
        context: Some(context),
        repo_id: run.repository_id.clone(),
        plugin_id: Some(run.plugin_id.clone()),
        source_kind: Some("host".into()),
        source_label: None,
        location: Some(SystemLogLocationInput { module_path: Some(module_path!().into()), file: Some(file!().into()), line: Some(line!()) }),
    }
}

/// 结果里失败的条目（Vue `response.payload.failed`）。不是数组时当作没有失败。
fn failed_items(payload: &Value) -> Vec<FailedItem> {
    let Some(items) = payload.get("failed").and_then(Value::as_array) else {
        return Vec::new();
    };
    items
        .iter()
        .enumerate()
        .map(|(index, item)| match item {
            Value::String(reason) => FailedItem { name: format!("第 {} 项", index + 1), reason: reason.clone(), detail: item.clone() },
            _ => FailedItem {
                name: text_of(item, &["songName", "name", "title"])
                    .or_else(|| identifier(item.get("songId")).map(|id| format!("歌曲 {id}")))
                    .unwrap_or_else(|| format!("第 {} 项", index + 1)),
                reason: text_of(item, &["error", "message", "reason"]).unwrap_or_else(|| "插件没有给出原因".into()),
                detail: item.clone(),
            },
        })
        .collect()
}

/// `keys` 里第一个非空的文字。
fn text_of(item: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .filter_map(|key| item.get(*key).and_then(Value::as_str))
        .map(str::trim)
        .find(|text| !text.is_empty())
        .map(str::to_string)
}

/// 数字或非空文字的标识。
fn identifier(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::Number(number) => Some(number.to_string()),
        Value::String(text) => Some(text.trim().to_string()).filter(|text| !text.is_empty()),
        _ => None,
    }
}

#[cfg(test)]
#[path = "source_download_tests.rs"]
mod tests;
