//! 来源下载的开始和收尾。
//!
//! 对应 Vue `sourceEntryActions.ts` 的 `runDownloadAction`：选好去处以后任务弹层出「{动作名} · 准备下载 …」，调用
//! 插件时「正在下载 …」（[`DownloadRun::begin`]）；插件交回以后（下载到其他资源库时还导入、同步了目标资源库）进度行
//! 写完「下载完成」收起，再照 Vue `context.refreshRepo()` 刷新当前资源库（[`finish`]）。
//!
//! 部分失败：网易云这类插件下载歌单时逐条下载，失败的条目放进结果的 `failed`，整个调用照样成功。Vue 只在进度行写
//! 「下载完成，N 项失败」，可紧接着的 `refreshRepo()` 开始「加载资源库」、占住进度行，这句话从来看不到，失败的条目
//! 只打在控制台。这里把失败的条数写进状态区（来源 [`FailureSource::Download`]），每一条的名称和原因由宿主按结果的
//! `failed` 写进系统日志（插件不另写，换成不写日志的来源插件也一样），提示里写明去「日志」看原因。
//! 收尾的刷新不清这条提示（`RefreshRun::keeps_status`），用户开始下一个操作时照常清掉；全部成功时不提示。
//!
//! 同名跳过：去处里已有的同名文件都保留原文件。下载到本地时插件不覆盖目录里已有的文件，结果的 `skippedEntries` 列出
//! 保留了哪些（插件原来直接覆盖，Vue 也没有提示）；下载到其他资源库时，下载回来的文件按 `skipExisting` 导入目标
//! 资源库（`admin_dispatch`），目标里已有同名文件的不导入、其余照常导入（Vue 的导入遇到一个同名就整批失败）。两种都
//! 把条数写进状态区（只有跳过时是提醒）、路径写进日志。
//!
//! 什么也没下到：插件报告的是一批（结果里有 `completed` 数组）却一项也没有、也没有交回文件时，状态区用警告色提醒
//! 「没有下载到任何条目」，不提醒的话界面上只有一闪而过的「下载完成」。Nana 下载歌单不列目录，由插件按歌单标识取
//! 来源里完整的歌单；网易云插件取不到歌单的曲目时整个调用报错（原因写进文件列），这条提醒留给真的没有曲目的歌单和
//! 别的来源插件。

use serde_json::{json, Value};

use crate::backend::services::repository::{SystemLogLocationInput, SystemLogWriteRequest};

use super::super::entry_actions::DownloadProgress;
use super::super::repo_operation::{OperationId, OperationPatch, OperationStart};
use super::super::status::FailureSource;
use super::super::ShellViewModel;
use super::source_target::TargetCandidate;
use super::AdminEffect;

/// 来源下载的结果：插件交回的载荷（失败的条目在 `failed` 里，下载到本地时保留的同名文件在 `skippedEntries` 里），
/// 以及导入目标资源库时因为目标里已有同名文件而跳过的条目（目标资源库里的相对路径；下载到本地时为空）。
#[derive(Clone, Debug, PartialEq)]
pub struct DownloadOutcome {
    pub payload: Value,
    pub skipped: Vec<String>,
}

impl DownloadOutcome {
    /// 只有插件交回的载荷：下载到本地，或者没有要导入的文件。
    pub(crate) fn plugin(payload: Value) -> Self {
        Self { payload, skipped: Vec::new() }
    }

    /// 插件交回的载荷，加上导入目标资源库的结果里跳过的同名条目（导入结果的 `skippedEntries`，没有这个键时为空）。
    pub(crate) fn imported(payload: Value, import: &Value) -> Self {
        let skipped = import
            .get("skippedEntries")
            .and_then(Value::as_array)
            .map(|items| items.iter().filter_map(Value::as_str).map(str::to_string).collect())
            .unwrap_or_default();
        Self { payload, skipped }
    }
}

/// 下载的去处。
#[derive(Clone, Debug, PartialEq)]
pub enum DownloadDestination {
    /// 用户在「选择下载目录」里选的目录。
    LocalFolder(String),
    /// 「下载到其他资源库」的目标资源库。
    Repository(TargetCandidate),
}

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
    pub destination: DownloadDestination,
}

impl DownloadRun {
    /// 选好去处，开始下载：任务弹层照 Vue「{动作名} · 准备下载 …」10%，调用插件时「正在下载 …」36% 不确定进度。
    pub(crate) fn begin(
        model: &mut ShellViewModel,
        progress: DownloadProgress,
        plugin_id: String,
        method: String,
        repository_id: Option<String>,
        destination: DownloadDestination,
    ) -> Self {
        let operation = model.operation.start(OperationStart::new(progress.label.clone(), progress.preparing(), 10.0));
        model.operation.update(operation, OperationPatch::step(progress.running(), 36.0).with_indeterminate(true));
        Self { operation, progress, plugin_id, method, repository_id, destination }
    }

    /// 下载到其他资源库时的目标资源库；下载到本地时没有。
    pub fn target(&self) -> Option<&TargetCandidate> {
        match &self.destination {
            DownloadDestination::Repository(target) => Some(target),
            DownloadDestination::LocalFolder(_) => None,
        }
    }
}

/// 下载结果回来了。成功时进度行照 Vue 写完「下载完成」（有失败的条目时「下载完成，N 项失败」）收起；有要告诉用户的
/// 结果（[`notice`]）时写进状态区和系统日志；再刷新当前资源库，有提示时这一轮刷新不清它。失败时进度行收起，文件列
/// 写「{动作名}「{名称}」失败：{原因}」（插件交回的原因原样写，例如登录失效、歌单不存在、网络超时；Vue 的失败没人接，
/// 界面上什么也没有）。
pub(crate) fn finish(model: &mut ShellViewModel, run: DownloadRun, result: Result<DownloadOutcome, String>) {
    let outcome = match result {
        Ok(outcome) => outcome,
        Err(error) => {
            eprintln!("Nana 来源下载失败：{}：{error}", run.method);
            model.operation.cancel(run.operation);
            let reason = if error.trim().is_empty() { "插件没有给出原因。".to_string() } else { error };
            model.files.error = format!("{}「{}」失败：{reason}", run.progress.label, run.progress.subject);
            return;
        }
    };
    model.files.error.clear();
    let failed = failed_items(&outcome.payload);
    let detail = if failed.is_empty() { "下载完成".to_string() } else { format!("下载完成，{} 项失败", failed.len()) };
    model.operation.update(run.operation, OperationPatch::step(detail, 100.0).with_indeterminate(false));
    model.operation.finish(run.operation);
    let Some(notice) = notice(&run, &outcome, &failed) else {
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

/// 下载结果里要告诉用户的，全部成功时没有：
/// - 有失败的条目：写几项失败、去「日志」看原因，每一条写一条日志；
/// - 去处里已有同名文件（[`kept_notice`]）：写已有几个同名文件、保留了原文件，保留的路径写一条日志；
/// - 插件报告的是一批、却一项也没有：提醒没有下载到任何条目。
///
/// 有失败的条目时是失败，只有跳过或空结果时是提醒。
fn notice(run: &DownloadRun, outcome: &DownloadOutcome, failed: &[FailedItem]) -> Option<Notice> {
    let (label, subject) = (&run.progress.label, &run.progress.subject);
    let mut parts = Vec::new();
    let mut logs = Vec::new();
    if !failed.is_empty() {
        parts.push(format!("{label}「{subject}」有 {} 项失败，原因见「日志」", failed.len()));
        logs.extend(failed.iter().map(|item| item.log(run)));
    }
    if let Some((kept, log)) = kept_notice(run, outcome) {
        logs.push(log);
        parts.push(if failed.is_empty() { format!("{label}「{subject}」：{kept}") } else { kept });
    }
    if !parts.is_empty() {
        return Some(Notice { message: format!("{}。", parts.join("；")), warning: failed.is_empty(), logs });
    }
    let payload = &outcome.payload;
    let empty_batch = payload.get("completed").and_then(Value::as_array).is_some_and(Vec::is_empty);
    if empty_batch && super::source_target::result_paths(payload).is_empty() {
        let message = format!("{label}「{subject}」没有下载到任何条目。");
        let context = json!({ "method": run.method, "subject": subject, "summary": payload.get("summary") });
        let log = host_log(run, "warn", "downloadEmpty", message.clone(), context);
        return Some(Notice { message, warning: true, logs: vec![log] });
    }
    None
}

/// 去处里已有、保留了原文件的同名文件：下载到本地时是插件没有覆盖的（结果的 `skippedEntries` 和
/// `completed[].skippedEntries`），下载到其他资源库时是导入时跳过的（导入结果的 `skippedEntries`）。交回状态区里的
/// 半句话和一条警告日志；没有时为空。
fn kept_notice(run: &DownloadRun, outcome: &DownloadOutcome) -> Option<(String, SystemLogWriteRequest)> {
    let (label, subject) = (&run.progress.label, &run.progress.subject);
    let (kept, action, paths, repo_id) = match &run.destination {
        DownloadDestination::LocalFolder(_) => {
            let paths = kept_paths(&outcome.payload);
            let kept = format!("下载目录里已有 {} 个同名文件，保留原文件、没有写入", paths.len());
            (kept, "downloadLocalSkipped", paths, run.repository_id.clone())
        }
        DownloadDestination::Repository(target) => {
            let kept = format!("目标资源库「{}」已有 {} 个同名文件，保留原文件、没有导入", target.name, outcome.skipped.len());
            (kept, "downloadImportSkipped", outcome.skipped.clone(), Some(target.repo_id.clone()))
        }
    };
    if paths.is_empty() {
        return None;
    }
    let mut context = json!({ "method": run.method, "subject": subject, "skippedPaths": paths });
    if let DownloadDestination::LocalFolder(folder) = &run.destination {
        context["folder"] = json!(folder);
    }
    let mut log = host_log(run, "warn", action, format!("{label}「{subject}」：{kept}。"), context);
    log.repo_id = repo_id;
    Some((kept, log))
}

/// 下载到本地时插件保留、没有写入的同名文件：`skippedEntries` 和 `completed[].skippedEntries`，去掉空白和重复。
fn kept_paths(payload: &Value) -> Vec<String> {
    super::source_target::listed_paths(payload, "skippedEntries")
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
