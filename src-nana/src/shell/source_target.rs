//! 来源条目「下载到其他资源库」：选目标资源库，再带着目标调用插件。
//!
//! 对应 Vue `sourceEntryActions.ts` 的 `runDownloadAction`（`writable-repository`）和
//! `useEntryActionRepositoryDialog.ts`：候选是就绪、可写、不是当前这个的资源库（`requireReady`、
//! `requireWritable`）；只有一个候选时直接用它，有几个时弹「选择目标资源库」让用户挑，取消就什么也不做。
//! 选好以后任务弹层出「{动作名} · 准备下载 …」，载荷的 `destination` 写成 `{ kind: "repository", repoId, path }`
//! 交给宿主（`admin_dispatch`）：调用插件、把下载回来的文件导入目标资源库（下载目录时放进以歌单命名的文件夹）
//! 并同步它，最后照 Vue `context.refreshRepo()` 刷新当前资源库（收尾在 `source_download.rs`）。
//!
//! Vue 在没有候选时点了什么也不发生（`openDialog` 返回 null），这里在菜单上就禁用这一项并写出原因
//! （`entry_actions::NO_TARGET_REPOSITORY`）；点的那一刻候选没了（列表刚换过）写进文件列的错误。

use serde_json::Value;

use super::super::entry_actions::DownloadProgress;
use super::super::workspace::WorkspaceState;
use super::super::ShellViewModel;
use super::source_download::DownloadRun;
use super::AdminEffect;

/// 等着选目标资源库的下载：调用插件要的东西，加上导入到目标资源库的哪个文件夹。
#[derive(Clone, Debug, PartialEq)]
pub struct RepositoryDownload {
    pub plugin_id: String,
    pub method: String,
    pub payload: Value,
    /// 点下载时的当前资源库：插件按它取来源凭据。
    pub repository_id: Option<String>,
    pub progress: DownloadProgress,
    /// 下载回来的文件导入到目标资源库的哪个文件夹：下载目录时是歌单名，单曲放在根目录。
    pub parent_path: Option<String>,
}

/// 「选择目标资源库」对话框里的一项。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TargetCandidate {
    pub repo_id: String,
    pub name: String,
    pub path: String,
}

/// 开着的「选择目标资源库」：等着的下载和候选。
#[derive(Clone, Debug, PartialEq)]
pub struct TargetPick {
    pub download: RepositoryDownload,
    pub candidates: Vec<TargetCandidate>,
}

/// 能下载到的资源库：就绪、可写、不是当前这个。按资源库列表的顺序（名称排序）。
pub fn target_candidates(workspace: &WorkspaceState) -> Vec<TargetCandidate> {
    workspace
        .repositories
        .iter()
        .filter(|repository| repository.accepts_downloads() && workspace.active_repo_id.as_deref() != Some(repository.repo_id.as_str()))
        .map(|repository| TargetCandidate { repo_id: repository.repo_id.clone(), name: repository.name.clone(), path: repository.path.clone() })
        .collect()
}

/// 点了「下载到其他资源库」：一个候选直接开始，几个候选弹出选择，没有候选只报错。
pub(super) fn begin(model: &mut ShellViewModel, download: RepositoryDownload) {
    let mut candidates = target_candidates(&model.workspace);
    match candidates.len() {
        0 => {
            eprintln!("Nana 没有可下载到的其他资源库：{}", download.method);
            model.files.error = format!("{}。", super::super::entry_actions::NO_TARGET_REPOSITORY);
        }
        1 => {
            let target = candidates.remove(0);
            start(model, download, &target);
        }
        _ => model.admin.download_target = Some(TargetPick { download, candidates }),
    }
}

/// 在「选择目标资源库」里点了一个资源库：关掉对话框开始下载。候选里没有它（列表换过）时只记日志。
pub(super) fn choose(model: &mut ShellViewModel, repo_id: &str) {
    let Some(pick) = model.admin.download_target.take() else {
        eprintln!("Nana 没有等着选目标资源库的下载：{repo_id}");
        return;
    };
    let Some(target) = pick.candidates.iter().find(|candidate| candidate.repo_id == repo_id) else {
        eprintln!("Nana 目标资源库不在候选里：{repo_id}");
        model.admin.download_target = Some(pick);
        return;
    };
    let target = target.clone();
    start(model, pick.download, &target);
}

/// 「选择目标资源库」的取消、点外面和 Escape：不下载。
pub(crate) fn cancel(model: &mut ShellViewModel) {
    if model.admin.download_target.take().is_some() {
        eprintln!("Nana 取消选择下载的目标资源库");
    }
}

/// 带着目标资源库开始下载（任务弹层的进度行见 [`DownloadRun::begin`]）：载荷的 `destination` 写成目标资源库，
/// 交给宿主调用、导入和同步。
fn start(model: &mut ShellViewModel, download: RepositoryDownload, target: &TargetCandidate) {
    let RepositoryDownload { plugin_id, method, mut payload, repository_id, progress, parent_path } = download;
    let Some(object) = payload.as_object_mut() else {
        eprintln!("Nana 下载载荷不是对象，不能写目标资源库：{method}");
        model.files.error = format!("{method} 的载荷不是对象。");
        return;
    };
    object.insert("destination".into(), serde_json::json!({ "kind": "repository", "repoId": target.repo_id, "path": target.path }));
    let run = DownloadRun::begin(model, progress, plugin_id, method, repository_id, Some(target.clone()));
    model.admin.push_effect(AdminEffect::DownloadToRepository { run, payload, parent_path });
}

/// 插件下载结果里要导入的文件（Vue `resultPaths`）：`paths` 和 `completed[].paths`，去掉空白和重复，顺序不变。
pub(crate) fn result_paths(payload: &Value) -> Vec<String> {
    let direct = payload.get("paths").and_then(Value::as_array).into_iter().flatten();
    let completed = payload
        .get("completed")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("paths").and_then(Value::as_array))
        .flatten();
    let mut paths: Vec<String> = Vec::new();
    for path in direct.chain(completed) {
        let text = match path {
            Value::String(text) => text.trim().to_string(),
            Value::Null => String::new(),
            other => other.to_string(),
        };
        if !text.is_empty() && !paths.contains(&text) {
            paths.push(text);
        }
    }
    paths
}

/// 有没有能下载到的其他资源库：菜单据此决定「到其他资源库」能不能点。
pub(crate) fn has_target_repository(workspace: &WorkspaceState) -> bool {
    !target_candidates(workspace).is_empty()
}

#[cfg(test)]
#[path = "source_target_tests.rs"]
mod tests;
