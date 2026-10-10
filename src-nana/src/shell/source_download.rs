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
        let operation = model.operation.start(OperationStart::new(progress.label.clone(), progress.preparing.clone(), 10.0));
        if let Some(running) = &progress.running {
            model.operation.update(operation, OperationPatch::step(running.clone(), 36.0).with_indeterminate(true));
        }
        Self { operation, progress, plugin_id, method, repository_id, target }
    }
}

/// 下载结果回来了。成功时进度行照 Vue 写完「下载完成」（有失败的条目时「下载完成，N 项失败」）收起；有失败的条目
/// 时每一条写进系统日志、条数写进状态区；再刷新当前资源库，有提示时这一轮刷新不清它。失败时进度行收起，原因写进
/// 文件列。
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
    if failed.is_empty() {
        model.workspace.request_repository_refresh();
        return;
    }
    eprintln!("Nana 来源下载有 {} 项失败：{}「{}」", failed.len(), run.progress.label, run.progress.subject);
    model.admin.push_effect(AdminEffect::WriteLogs(failed.iter().map(|item| item.log(&run)).collect()));
    let notice = format!("{}「{}」有 {} 项失败，原因见「日志」。", run.progress.label, run.progress.subject, failed.len());
    model.status.fail(FailureSource::Download, notice);
    model.workspace.refresh_after_download();
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
        SystemLogWriteRequest {
            level: "error".into(),
            category: "source".into(),
            action: "downloadItemFailed".into(),
            message: format!("{}「{}」里的「{}」下载失败：{}", run.progress.label, run.progress.subject, self.name, self.reason),
            context: Some(json!({ "method": run.method, "subject": run.progress.subject, "item": self.detail })),
            repo_id: run.repository_id.clone(),
            plugin_id: Some(run.plugin_id.clone()),
            source_kind: Some("host".into()),
            source_label: None,
            location: Some(SystemLogLocationInput {
                module_path: Some(module_path!().into()),
                file: Some(file!().into()),
                line: Some(line!()),
            }),
        }
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
