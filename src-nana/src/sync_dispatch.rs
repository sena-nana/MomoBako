//! 把文件夹树「刷新」的服务请求交给领域服务：同步仓库走启动时同一条 `PROTOCOL_REPOSITORY_SYNC`，
//! 同步完以后在一个后台任务里重读仓库摘要、硬链接候选和目录树。结果作为 `ShellMessage::TreeSync`
//! 回到 `update` 归约。服务没起来或任务提交失败时当场按失败写回，刷新不会停在进行中。

use nana_ui::runtime::Task;
use nana_ui::RuntimeProgramContext;

use crate::backend::services::mutsuki_runner::PROTOCOL_REPOSITORY_SYNC;
use crate::backend::services::repository::SyncRequest;
use crate::shell::tree_sync::{TreeSyncEffect, TreeSyncMessage, TreeSyncRefresh};
use crate::shell::{HardlinkPrompt, ShellMessage, SidebarTree};
use crate::MomoBakoApplication;

/// 服务没起来时的原因。
const NO_SERVICES: &str = "领域服务未启动";

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
fn dispatch_sync(app: &mut MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>, token: u64, repo_id: String) {
    let Some(services) = app.services.as_ref() else {
        eprintln!("Nana 刷新文件夹树需要领域服务，当前服务未启动");
        app.shell.reduce(synced(token, Err(NO_SERVICES.into())));
        return;
    };
    let tasks = services.tasks.clone();
    let executor = services.executor.clone();
    if let Err(error) = context.run_task(Task::new(async move {
        let result = executor.block_on(tasks.execute(PROTOCOL_REPOSITORY_SYNC, SyncRequest { repo_id })).map(|(output, _)| {
            let scanned = output.get("scannedFiles").and_then(serde_json::Value::as_u64);
            if scanned.is_none() {
                eprintln!("Nana 仓库同步结果里没有扫描数：{output}");
            }
            scanned.unwrap_or_default()
        });
        synced(token, result)
    })) {
        eprintln!("Nana 仓库同步任务提交失败：{error}");
        app.shell.reduce(synced(token, Err(format!("仓库同步任务提交失败：{error}"))));
    }
}

/// 同步完以后重读仓库摘要、硬链接候选和目录树（`tree` 时）。三份各自记成败，交给归约照 Vue 收尾。
fn dispatch_refresh(app: &mut MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>, token: u64, repo_id: String, tree: bool) {
    let Some(services) = app.services.as_ref() else {
        eprintln!("Nana 刷新文件夹树需要领域服务，当前服务未启动");
        app.shell.reduce(refreshed(token, failed_refresh(NO_SERVICES, tree)));
        return;
    };
    let query = services.repository_query.clone();
    let interaction = services.repository_interaction.clone();
    let browser = services.file_browser.clone();
    let executor = services.executor.clone();
    if let Err(error) = context.run_task(Task::new(async move {
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
    })) {
        eprintln!("Nana 文件夹树重读任务提交失败：{error}");
        app.shell.reduce(refreshed(token, failed_refresh(&format!("文件夹树重读任务提交失败：{error}"), tree)));
    }
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
