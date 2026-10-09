//! 文件夹树的「刷新」：同步整个仓库再刷新工作区，对应 Vue `refreshFileBrowserTree`
//! （`composables/workspace/sync.ts`）。
//!
//! 一轮刷新分三段，状态都在归约里推进，服务请求交给宿主（`sync_dispatch.rs`）：
//! 1. 同步：开操作进度「刷新文件树 / 同步并读取目录结构」，同步进度停在「扫描文件夹结构」1/3，
//!    宿主走启动时同一条 `PROTOCOL_REPOSITORY_SYNC`。刷新开始时忙碌标志由假变真，状态区清掉上一次失败；
//! 2. 刷新：同步完成后进度经「写入索引结果」2/3 到「刷新文件夹树」3/3，宿主一起重读仓库摘要、硬链接
//!    候选和目录树（回收站面板不读树）；
//! 3. 读目录：照 Vue 非静默地重读当前目录（回收站读回收站），读完进度是「刷新完成」，操作进度收起。
//!
//! 任何一段失败：失败写进全局状态区（来源 [`FailureSource::Sync`]），操作进度取消，同步进度标成失败。
//! 同步进度的扫描、写入、刷新三档在侧栏状态区显示（失败和忙碌优先）。刷新期间文件夹分组的刷新按钮
//! 转圈并禁用，条件和 Vue 一样看「正在读目录」（[`loading_file_browser`]）。

use std::time::{SystemTime, UNIX_EPOCH};

use crate::backend::services::repository::RepositorySnapshot;

use super::admin::OperationProgress;
use super::files::{FileContext, HardlinkPrompt};
use super::sidebar::SidebarTree;
use super::status::FailureSource;
use super::{ShellMessage, ShellViewModel, StartupStatus, WorkspacePanel};

/// Vue `SYNC_TOTAL_STEPS`。
const SYNC_TOTAL_STEPS: u8 = 3;

/// 同步进度的阶段，Vue `RepositorySyncProgress.phase`。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SyncPhase {
    #[default]
    Idle,
    Scanning,
    Writing,
    Refreshing,
    Complete,
    Error,
}

/// Vue `syncProgress`（`RepositorySyncProgress`）：阶段、文案、第几步、一共几步和四舍五入的百分比。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncProgress {
    pub phase: SyncPhase,
    pub label: String,
    pub current: u8,
    pub total: u8,
    pub percent: u8,
}

impl Default for SyncProgress {
    /// Vue `createInitialSyncProgress`：空闲，0 / 3 步。
    fn default() -> Self {
        Self { phase: SyncPhase::Idle, label: String::new(), current: 0, total: SYNC_TOTAL_STEPS, percent: 0 }
    }
}

impl SyncProgress {
    /// Vue `setSyncProgress`：一共 [`SYNC_TOTAL_STEPS`] 步，百分比是 `round(current / total * 100)`。
    fn set(&mut self, phase: SyncPhase, label: impl Into<String>, current: u8) {
        let total = SYNC_TOTAL_STEPS;
        let percent = (f32::from(current) / f32::from(total) * 100.0).round() as u8;
        *self = Self { phase, label: label.into(), current, total, percent };
    }

    /// 扫描、写入、刷新三档在状态区显示（Vue `isShowingSyncProgress`），返回文案和百分比。
    pub fn shown(&self) -> Option<(&str, u8)> {
        matches!(self.phase, SyncPhase::Scanning | SyncPhase::Writing | SyncPhase::Refreshing)
            .then_some((self.label.as_str(), self.percent))
    }
}

/// 一轮刷新走到哪一段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    /// 等同步结果。
    Syncing,
    /// 等摘要、硬链接候选和目录树。
    Refreshing,
    /// 等当前目录读完。
    Directory,
}

/// 进行中的一轮刷新。`token` 认出这一轮的结果，`repo_id` 是发起时的仓库。
#[derive(Clone, Debug)]
struct Run {
    token: u64,
    repo_id: String,
    stage: Stage,
}

/// 文件夹树刷新的状态。
#[derive(Clone, Debug, Default)]
pub struct TreeSyncState {
    run: Option<Run>,
    /// 同步进度，状态区的第三档读它。
    pub progress: SyncProgress,
    issued: u64,
    effects: Vec<TreeSyncEffect>,
}

impl TreeSyncState {
    /// 正在刷新：从点下刷新到读完目录（Vue 这段时间 `isLoadingFileBrowser` 为真）。
    pub fn running(&self) -> bool {
        self.run.is_some()
    }

    pub fn take_effects(&mut self) -> Vec<TreeSyncEffect> {
        std::mem::take(&mut self.effects)
    }

    /// 这一轮还在等 `stage` 这一段的结果。
    fn awaiting(&self, token: u64, stage: Stage) -> bool {
        self.run.as_ref().is_some_and(|run| run.token == token && run.stage == stage)
    }
}

/// 归约后交给宿主的服务请求。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TreeSyncEffect {
    /// 同步仓库，和启动时的同步同一条协议。
    Sync { token: u64, repo_id: String },
    /// 重读仓库摘要和硬链接候选；`tree` 时再读目录树。
    Refresh { token: u64, repo_id: String, tree: bool },
}

/// 宿主送回的结果。
#[derive(Clone, Debug)]
pub enum TreeSyncMessage {
    /// 同步完成，成功时带扫描的文件数。
    Synced { token: u64, result: Result<u64, String> },
    /// 摘要、硬链接候选和目录树的结果。
    Refreshed { token: u64, result: TreeSyncRefresh },
}

/// 同步后重读的三份数据，各自成败。没有要求读树时 `tree` 为空。
#[derive(Clone, Debug)]
pub struct TreeSyncRefresh {
    pub snapshot: Result<RepositorySnapshot, String>,
    pub hardlinks: Result<Vec<HardlinkPrompt>, String>,
    pub tree: Option<Result<SidebarTree, String>>,
}

/// Vue `isLoadingFileBrowser`：非静默地读目录，或者正在刷新文件夹树。
pub(crate) fn loading_file_browser(model: &ShellViewModel) -> bool {
    model.files.loading || model.tree_sync.running()
}

/// 「刷新文件夹树」不可用：照 Vue 没有仓库（启动完成前 Vue 不写活动仓库）、仓库缺失或正在读目录时
/// 禁用；目录树自己在读时按钮转圈，也一起禁用。按钮的禁用和归约的拦截都读它，点得动就一定会刷新。
pub(crate) fn refresh_blocked(model: &ShellViewModel) -> bool {
    model.workspace.active_repo_id.is_none()
        || model.workspace.startup.status != StartupStatus::Ready
        || model.navigation_locked()
        || model.sidebar.tree_loading
        || loading_file_browser(model)
}

/// 点下「刷新文件夹树」：开一轮同步。按钮禁用时同样拦下。
pub(super) fn begin(model: &mut ShellViewModel) {
    if refresh_blocked(model) {
        eprintln!("Nana 现在不能刷新文件夹树");
        return;
    }
    let Some(repo_id) = model.workspace.active_repo_id.clone() else {
        return;
    };
    let state = &mut model.tree_sync;
    state.issued += 1;
    let token = state.issued;
    state.run = Some(Run { token, repo_id: repo_id.clone(), stage: Stage::Syncing });
    state.progress.set(SyncPhase::Scanning, "扫描文件夹结构", 1);
    state.effects.push(TreeSyncEffect::Sync { token, repo_id });
    model.admin.operation = Some(OperationProgress {
        label: "刷新文件树".into(),
        detail: "同步并读取目录结构".into(),
        value: 12.0,
        indeterminate: false,
        updated_at_ms: now_ms(),
    });
}

/// 归约宿主送回的结果。不是这类消息时交回主归约。
pub(super) fn reduce_message(model: &mut ShellViewModel, message: ShellMessage) -> Option<ShellMessage> {
    let ShellMessage::TreeSync(message) = message else {
        return Some(message);
    };
    match message {
        TreeSyncMessage::Synced { token, result } => synced(model, token, result),
        TreeSyncMessage::Refreshed { token, result } => refreshed(model, token, result),
    }
    None
}

/// 等目录读完的这一轮，目录不再在读时收尾。每次归约结束时调。
pub(super) fn settle(model: &mut ShellViewModel) {
    let waiting = model.tree_sync.run.as_ref().is_some_and(|run| run.stage == Stage::Directory);
    if waiting && !model.files.loading {
        complete(model);
    }
}

/// 同步结果：成功后进度走到「刷新文件夹树」，排下重读；回收站面板不读树。
fn synced(model: &mut ShellViewModel, token: u64, result: Result<u64, String>) {
    if !model.tree_sync.awaiting(token, Stage::Syncing) {
        eprintln!("Nana 忽略过期的文件夹树同步结果：第 {token} 轮");
        return;
    }
    let Some(repo_id) = current_repo(model) else {
        return;
    };
    let scanned = match result {
        Ok(scanned) => scanned,
        Err(error) => return fail(model, error),
    };
    if let Some(operation) = model.admin.operation.as_mut() {
        operation.detail = format!("已扫描 {scanned} 个文件");
        operation.value = 58.0;
    }
    let tree = model.workspace.panel != WorkspacePanel::Trash;
    let state = &mut model.tree_sync;
    state.progress.set(SyncPhase::Writing, "写入索引结果", 2);
    state.progress.set(SyncPhase::Refreshing, "刷新文件夹树", 3);
    if let Some(run) = state.run.as_mut() {
        run.stage = Stage::Refreshing;
    }
    state.effects.push(TreeSyncEffect::Refresh { token, repo_id, tree });
}

/// 重读结果：照 Vue `Promise.all`，成功的几份先写进去，任何一份失败整轮按失败收尾；都成功时读当前目录。
fn refreshed(model: &mut ShellViewModel, token: u64, result: TreeSyncRefresh) {
    if !model.tree_sync.awaiting(token, Stage::Refreshing) {
        eprintln!("Nana 忽略过期的文件夹树刷新结果：第 {token} 轮");
        return;
    }
    let Some(repo_id) = current_repo(model) else {
        return;
    };
    let mut first_error: Option<String> = None;
    match result.snapshot {
        Ok(snapshot) => super::workspace_refresh::apply_snapshot(model, Ok(snapshot)),
        Err(error) => _ = first_error.get_or_insert(error),
    }
    match result.hardlinks {
        Ok(prompts) => model.files.note_hardlinks_synced(prompts),
        Err(error) => _ = first_error.get_or_insert(error),
    }
    match result.tree {
        Some(Ok(tree)) => model.sidebar.apply_tree(&repo_id, Ok(tree)),
        Some(Err(error)) => _ = first_error.get_or_insert(error),
        None => {}
    }
    if let Some(error) = first_error {
        return fail(model, error);
    }
    let context = FileContext::from_model(model);
    if context.is_virtual() || !model.files.reload_current(&context) {
        eprintln!("Nana 刷新文件夹树不重读目录：当前是虚拟视图或文件正在变更");
        return complete(model);
    }
    if let Some(run) = model.tree_sync.run.as_mut() {
        run.stage = Stage::Directory;
    }
}

/// 发起这一轮的仓库仍是当前仓库时返回它；换了仓库就放弃这一轮，进度回到空闲。
fn current_repo(model: &mut ShellViewModel) -> Option<String> {
    let repo_id = model.tree_sync.run.as_ref().map(|run| run.repo_id.clone())?;
    if model.workspace.active_repo_id.as_deref() == Some(repo_id.as_str()) {
        return Some(repo_id);
    }
    eprintln!("Nana 仓库已经换走，放弃这一轮文件夹树刷新：{repo_id}");
    model.tree_sync.run = None;
    model.tree_sync.progress = SyncProgress::default();
    model.admin.operation = None;
    None
}

/// 收尾：进度「刷新完成」，操作进度收起。
fn complete(model: &mut ShellViewModel) {
    model.tree_sync.run = None;
    model.tree_sync.progress.set(SyncPhase::Complete, "刷新完成", 3);
    model.admin.operation = None;
}

/// 失败：写进全局状态区，取消操作进度，同步进度标成失败。
fn fail(model: &mut ShellViewModel, error: String) {
    eprintln!("Nana 刷新文件夹树失败：{error}");
    model.status.fail(FailureSource::Sync, format!("刷新文件夹树失败：{error}"));
    model.tree_sync.run = None;
    model.tree_sync.progress.set(SyncPhase::Error, error, 3);
    model.admin.operation = None;
}

/// 操作进度的更新时间：墙钟毫秒。任务中心的条目按它排先后。
fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|elapsed| elapsed.as_millis() as i64).unwrap_or_default()
}

#[cfg(test)]
#[path = "tree_sync_tests.rs"]
mod tests;
