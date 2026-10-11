//! 把文件夹树「刷新」的服务请求交给领域服务：同步仓库走启动时同一条 `PROTOCOL_REPOSITORY_SYNC`，
//! 同步完以后在一个后台任务里重读仓库摘要、硬链接候选和目录树。结果作为 `ShellMessage::TreeSync`
//! 回到 `update` 归约。服务没起来、任务排不进去或任务 panic 时按失败写回（`task_guard.rs`），刷新不会停在进行中。

use nana_ui::RuntimeProgramContext;

use crate::backend::services::mutsuki_runner::PROTOCOL_REPOSITORY_SYNC;
use crate::backend::services::repository::SyncRequest;
use crate::shell::tree_sync::{TreeSyncEffect, TreeSyncMessage, TreeSyncRefresh};
use crate::shell::{HardlinkPrompt, ShellMessage, SidebarTree};
use crate::task_guard::spawn_service;
use crate::MomoBakoApplication;

/// 执行刷新文件夹树归约留下的请求。
pub(crate) fn dispatch_tree_sync_effects(app: &mut MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>) {
    for effect in app.shell.tree_sync.take_effects() {
        match effect {
            TreeSyncEffect::Sync { token, repo_id } => dispatch_sync(app, context, token, repo_id),
            TreeSyncEffect::Refresh { token, repo_id, tree } => dispatch_refresh(app, context, token, repo_id, tree),
        }
    }
}

/// 同步仓库。成功时取出扫描的文件数（`SyncResult.scannedFiles`）。
fn dispatch_sync(app: &MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>, token: u64, repo_id: String) {
    spawn_service(app.services.as_ref(), context, "仓库同步", move |error| synced(token, Err(error)), |services| {
        let tasks = services.tasks.clone();
        let executor = services.executor.clone();
        move || {
            let result = executor.block_on(tasks.execute(PROTOCOL_REPOSITORY_SYNC, SyncRequest { repo_id })).map(|(output, _)| {
                let scanned = output.get("scannedFiles").and_then(serde_json::Value::as_u64);
                if scanned.is_none() {
                    eprintln!("Nana 仓库同步结果里没有扫描数：{output}");
                }
                scanned.unwrap_or_default()
            });
            synced(token, result)
        }
    });
}

/// 同步完以后重读仓库摘要、硬链接候选和目录树（`tree` 时）。三份各自记成败，交给归约照 Vue 收尾；整个任务失败时
/// 三份都按同一个原因失败。
fn dispatch_refresh(app: &MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>, token: u64, repo_id: String, tree: bool) {
    let failed = move |error: String| refreshed(token, failed_refresh(&error, tree));
    spawn_service(app.services.as_ref(), context, "文件夹树重读", failed, |services| {
        let query = services.repository_query.clone();
        let interaction = services.repository_interaction.clone();
        let browser = services.file_browser.clone();
        let executor = services.executor.clone();
        move || {
            let snapshot = executor.block_on(query.get_repository_snapshot(repo_id.clone()));
            let hardlinks = executor.block_on(interaction.list_hardlink_candidates(repo_id.clone())).map(|response| {
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
            let tree = tree.then(|| executor.block_on(browser.get_repository_tree(repo_id)).map(|snapshot| SidebarTree::from_nodes(&snapshot.tree)));
            refreshed(token, TreeSyncRefresh { snapshot, hardlinks, tree })
        }
    });
}

/// 三份都按同一个原因失败。
fn failed_refresh(error: &str, tree: bool) -> TreeSyncRefresh {
    TreeSyncRefresh { snapshot: Err(error.to_string()), hardlinks: Err(error.to_string()), tree: tree.then(|| Err(error.to_string())) }
}

fn synced(token: u64, result: Result<u64, String>) -> ShellMessage {
    ShellMessage::TreeSync(TreeSyncMessage::Synced { token, result })
}

fn refreshed(token: u64, result: TreeSyncRefresh) -> ShellMessage {
    ShellMessage::TreeSync(TreeSyncMessage::Refreshed { token, result })
}
