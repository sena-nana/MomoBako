//! Mutsuki 协议、硬链接和资源库导出副作用。

use nana_ui::RuntimeProgramContext;

use crate::backend::services::mutsuki_runner::PROTOCOL_REPOSITORY_EXPORT;
use crate::backend::services::repository::{
    FileTreeNode, HardlinkConfirmRequest, RepositoryArchiveExportOptions,
    RepositoryExportRequest, RepositoryGitExportOptions, TrashMutationRequest,
};
use crate::shell::{FilesMessage, HardlinkPrompt, ProtocolFolderTree, ShellMessage};
use crate::task_guard::spawn_service;
use crate::MomoBakoApplication;

use super::{files_message, snapshot_message};

/// 复制、移动、导入、压缩包和 Eagle 导入、删除：在任务里跑 Mutsuki 协议，结果回 `ProtocolFinished`（`hardlinks` 为真时
/// 成功后再读硬链接候选）。`sidebar_tree` 为真时从协议 JSON 取出文件夹树，没有树就不编。
pub(super) fn dispatch_protocol(
    app: &MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    what: &str,
    hardlinks: bool,
    sidebar_tree: bool,
    run: impl FnOnce(
            crate::backend::viewmodels::MutsukiTaskViewModel,
            std::sync::Arc<tokio::runtime::Runtime>,
        ) -> Result<serde_json::Value, String>
        + Send
        + 'static,
) {
    let failed = |error| protocol_message(Err(error), false, false);
    spawn_service(app.services.as_ref(), context, what, failed, |services| {
        let tasks = services.tasks.clone();
        let executor = services.executor.clone();
        move || protocol_message(run(tasks, executor), hardlinks, sidebar_tree)
    });
}

pub(super) fn dispatch_trash(
    app: &MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    repo_id: String,
    action: String,
    paths: Vec<String>,
) {
    let failed = |error| snapshot_message(Err(error), None);
    spawn_service(
        app.services.as_ref(),
        context,
        "回收站变更",
        failed,
        |services| {
            let browser = services.file_browser.clone();
            let executor = services.executor.clone();
            move || {
                let calls: Vec<Option<String>> = if paths.is_empty() {
                    vec![None]
                } else {
                    paths.into_iter().map(Some).collect()
                };
                let mut last = None;
                for path in calls {
                    let request = TrashMutationRequest {
                        repo_id: repo_id.clone(),
                        action: action.clone(),
                        path,
                    };
                    match executor.block_on(browser.mutate_trash(request)) {
                        Ok(snapshot) => last = Some(snapshot),
                        Err(error) => {
                            eprintln!("Nana 回收站变更失败：{error}");
                            return snapshot_message(Err(error), None);
                        }
                    }
                }
                match last {
                    Some(snapshot) => snapshot_message(Ok(snapshot), None),
                    None => snapshot_message(Err("回收站变更没有返回快照".into()), None),
                }
            }
        },
    );
}

/// 硬链接候选是为什么读的，结果按它交回不同的消息。
#[derive(Clone, Copy)]
pub(super) enum HardlinkRead {
    /// 复制以后：弹出确认，失败写进文件列表。
    Load,
    /// 结构更新的静默重读：只换列表。
    Refresh,
    /// 启动和换仓库后的后台读取：没有别的对话框时弹出确认，失败只记日志。
    Check,
}

/// 拉取硬链接候选，结果按 `read` 交回，带着请求时候选列表的版本 `epoch`。
pub(super) fn dispatch_hardlinks(
    app: &MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    repo_id: String,
    read: HardlinkRead,
    epoch: u64,
) {
    let failed = move |error| hardlinks_message(read, epoch, Err(error));
    spawn_service(
        app.services.as_ref(),
        context,
        "硬链接候选",
        failed,
        |services| {
            let interaction = services.repository_interaction.clone();
            let executor = services.executor.clone();
            move || {
                let result = executor
                    .block_on(interaction.list_hardlink_candidates(repo_id))
                    .map(|response| {
                        response
                            .candidates
                            .into_iter()
                            .map(|candidate| HardlinkPrompt {
                                id: candidate.candidate_id,
                                new_path: candidate.new_path,
                                existing_path: candidate.existing_path,
                                size_label: candidate.size_label,
                            })
                            .collect()
                    });
                hardlinks_message(read, epoch, result)
            }
        },
    );
}

fn hardlinks_message(
    read: HardlinkRead,
    epoch: u64,
    result: Result<Vec<HardlinkPrompt>, String>,
) -> ShellMessage {
    files_message(match read {
        HardlinkRead::Load => FilesMessage::HardlinksLoaded { epoch, result },
        HardlinkRead::Refresh => FilesMessage::HardlinksRefreshed { epoch, result },
        HardlinkRead::Check => FilesMessage::HardlinksChecked { epoch, result },
    })
}

pub(super) fn dispatch_confirm_hardlink(
    app: &MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    repo_id: String,
    candidate_id: String,
) {
    let failed_repo = repo_id.clone();
    let failed = move |error| {
        files_message(FilesMessage::HardlinkConfirmed {
            repo_id: failed_repo.clone(),
            result: Err(error),
        })
    };
    spawn_service(
        app.services.as_ref(),
        context,
        "确认硬链接",
        failed,
        |services| {
            let interaction = services.repository_interaction.clone();
            let executor = services.executor.clone();
            move || {
                let request = HardlinkConfirmRequest {
                    repo_id: repo_id.clone(),
                    candidate_id: candidate_id.clone(),
                };
                let result = executor
                    .block_on(interaction.confirm_hardlink_candidate(request))
                    .map(|_| candidate_id);
                files_message(FilesMessage::HardlinkConfirmed { repo_id, result })
            }
        },
    );
}

/// 选中文件后读素材详情。派发层自己的失败（服务没起来、任务排不进去、panic）写进文件列，不像服务交回的失败那样
/// 把整页换成错误页。
pub(super) fn dispatch_asset(
    app: &MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    repo_id: String,
    asset_id: String,
) {
    let failed = |error| files_message(FilesMessage::NoteError(error));
    spawn_service(
        app.services.as_ref(),
        context,
        "文件元数据",
        failed,
        |services| {
            let query = services.repository_query.clone();
            let executor = services.executor.clone();
            move || {
                ShellMessage::AssetDetailLoaded(
                    executor.block_on(query.get_asset_detail(repo_id, asset_id)),
                )
            }
        },
    );
}

fn protocol_message(
    result: Result<serde_json::Value, String>,
    hardlinks: bool,
    sidebar_tree: bool,
) -> ShellMessage {
    let folder_tree = match &result {
        Ok(value) if sidebar_tree => protocol_folder_tree(value),
        _ => None,
    };
    let result = result.map(|_| ());
    let reload = result.is_ok();
    files_message(FilesMessage::ProtocolFinished {
        result,
        reload,
        hardlinks: hardlinks && reload,
        folder_tree,
    })
}

/// 从复制、移动、删除的协议 JSON 取出文件夹树。
///
/// 单条结果就是目录快照；批量删除把快照放在 `snapshot` 里。没有 `tree`、`tree` 为 null 时不编。
/// 树解析不了、或没有仓库时记日志并丢掉。
pub(crate) fn protocol_folder_tree(value: &serde_json::Value) -> Option<ProtocolFolderTree> {
    let snapshot = value
        .get("snapshot")
        .filter(|item| item.is_object())
        .unwrap_or(value);
    let Some(tree) = snapshot.get("tree") else {
        return None;
    };
    if tree.is_null() {
        return None;
    }
    let Some(repo_id) = snapshot
        .get("repoId")
        .and_then(|item| item.as_str())
        .filter(|repo_id| !repo_id.is_empty())
    else {
        eprintln!("Nana 协议结果的文件夹树没有仓库，不写入侧栏");
        return None;
    };
    let current_path = snapshot
        .get("currentPath")
        .and_then(|item| item.as_str())
        .unwrap_or("")
        .to_string();
    let trash = snapshot
        .get("specialLocation")
        .and_then(|item| item.as_str())
        == Some("trash");
    match serde_json::from_value::<Vec<FileTreeNode>>(tree.clone()) {
        Ok(nodes) => Some(ProtocolFolderTree {
            repo_id: repo_id.to_string(),
            current_path,
            trash,
            nodes,
        }),
        Err(error) => {
            eprintln!("Nana 协议结果的文件夹树无法解析，不写入侧栏：{error}");
            None
        }
    }
}

/// 用对话框里的格式、路径和压缩选项组压缩包导出请求。
pub(super) fn archive_export(
    repo_id: String,
    format: String,
    output_path: String,
    compression: String,
    encrypt: bool,
    password: String,
) -> RepositoryExportRequest {
    let password = password.trim().to_string();
    RepositoryExportRequest {
        repo_id,
        target: "archive".into(),
        archive: Some(RepositoryArchiveExportOptions {
            format,
            output_path,
            compression,
            encrypt,
            password: (!password.is_empty()).then_some(password),
        }),
        git: None,
    }
}

/// 用对话框里的远端、分支和提交说明组 Git 导出请求。空白字段省略。
pub(super) fn git_export(
    repo_id: String,
    remote: String,
    branch: String,
    message: String,
) -> RepositoryExportRequest {
    RepositoryExportRequest {
        repo_id,
        target: "git".into(),
        archive: None,
        git: Some(RepositoryGitExportOptions {
            remote: filled(remote),
            branch: filled(branch),
            message: filled(message),
        }),
    }
}

fn filled(value: String) -> Option<String> {
    let value = value.trim().to_string();
    (!value.is_empty()).then_some(value)
}

/// 调用 `momobako.repository.export`。成功把宿主 message 写回对话框，失败写真实错误。
pub(super) fn dispatch_export(
    app: &MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    request: RepositoryExportRequest,
) {
    let failed = |error| files_message(FilesMessage::ExportFinished(Err(error)));
    spawn_service(
        app.services.as_ref(),
        context,
        "资源库导出",
        failed,
        |services| {
            let tasks = services.tasks.clone();
            let executor = services.executor.clone();
            move || {
                let result = executor
                    .block_on(tasks.execute(PROTOCOL_REPOSITORY_EXPORT, request))
                    .map(|(output, _)| export_notice(&output));
                files_message(FilesMessage::ExportFinished(result))
            }
        },
    );
}

/// 只采用宿主返回的 message。没有说明时记日志，不编造成功。
fn export_notice(value: &serde_json::Value) -> String {
    value
        .get("message")
        .and_then(|item| item.as_str())
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| {
            eprintln!("Nana 导出返回没有说明：{value}");
            "导出已返回，但没有结果说明。".into()
        })
}
