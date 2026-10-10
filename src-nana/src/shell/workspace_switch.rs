//! 换仓库和同一个仓库的刷新。
//!
//! 对应 Vue `navigation.ts` 的 `selectRepository` 和 `lifecycle.ts` 的 `loadRepositories`：
//! - **换到别的仓库**：回到启动页（[`StartupMode::Switch`]），走「扫描资源库文件 → 读取仓库摘要 →
//!   读取首屏目录」，失败停在启动页给「重试」；目标缺失时直接结束，主区是缺失页。任务弹层是
//!   「加载资源库 · 读取资源库快照」，扫描时 28%，读完摘要 46%，首屏读完以后完成。上一个仓库的内容由
//!   壳层在活动仓库变了的那次归约里清掉（`workspace_flow.rs`）。
//! - **同一个仓库**：先同步，再读摘要、重读当前目录（读失败退回根目录）。从「刷新」进来（Vue
//!   `loadRepositories`）时先重读资源库列表，任务弹层是「加载资源库 · 读取已注册资源库」，列表到了走到
//!   「加载资源库摘要」38%，之后由重读目录接手。整轮期间状态区写「正在同步仓库状态」，失败写进状态区。
//!
//! 换仓库的入口：切换列表、附加或新建资源库成功、删除当前仓库以后选下一个、搜索命中在别的仓库，
//! 以及刷新以后当前仓库已经不在列表里。

use crate::backend::services::repository::RepositorySummary;

use super::super::repo_operation::{OperationPatch, OperationStart};
use super::super::search_collate::compare_zh;
use super::startup::{StartupMode, StartupStatus};
use super::{RepositoryPresence, WorkspaceEffect, WorkspacePanel, WorkspaceRepository, WorkspaceState};

/// 同一个仓库的刷新走到哪一步。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefreshStage {
    /// 读资源库列表，只有从「刷新」进来时有这一步。
    Listing,
    Syncing,
    Snapshot,
    /// 重读当前目录，读完收尾。
    Directory,
}

/// 进行中的一轮同仓库刷新。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefreshRun {
    /// 这一轮的列表、同步和摘要请求带的代次。
    pub generation: u64,
    pub stage: RefreshStage,
    /// 读完列表以后要选的仓库（附加、来源登录以后切过去的那个）。没有时照 Vue 选当前仓库，
    /// 当前仓库不在列表里时选第一项。
    pub target: Option<String>,
    /// 要重读的目录：Vue 在 `selectRepository` 开始时记下的 `previousDirectoryPath`，壳层随后填上。
    pub directory: Option<String>,
    /// 读当前目录失败，已经退回根目录读过一次。
    pub fell_back: bool,
    /// 这一轮是来源下载的收尾（Vue `context.refreshRepo()`），不是用户开始的新操作：状态区里下载刚记下的失败
    /// 不因为这一轮读列表、同步和重读目录被清掉（`status.rs` 的 `Activity`）。
    pub keeps_status: bool,
}

impl WorkspaceState {
    /// 刷新资源库（Vue `loadRepositories`）：重读资源库列表再选仓库，`target` 是读完以后要切过去的仓库。
    /// 启动还没完成时只重读列表，结果接着走启动。
    pub fn refresh_repositories(&mut self, target: Option<String>) {
        self.list_generation = self.list_generation.saturating_add(1);
        self.list_loading = true;
        if self.startup.status == StartupStatus::Ready {
            self.operation.start(OperationStart::new("加载资源库", "读取已注册资源库", 12.0).indeterminate());
            self.refresh = Some(RefreshRun {
                generation: self.list_generation,
                stage: RefreshStage::Listing,
                target,
                directory: None,
                fell_back: false,
                keeps_status: false,
            });
        } else if let Some(target) = target {
            eprintln!("Nana 启动还没完成，刷新资源库列表以后不另选仓库：{target}");
        }
        self.effects.push(WorkspaceEffect::RefreshRepositories { generation: self.list_generation });
    }

    /// 来源下载收尾的刷新：和 [`Self::request_repository_refresh`] 一样重读列表、同步、读摘要和当前目录，只是这一轮
    /// 属于刚结束的下载（[`RefreshRun::keeps_status`]），下载记在状态区里的失败留到用户开始下一个操作。
    pub fn refresh_after_download(&mut self) {
        self.refresh_repositories(None);
        if let Some(run) = self.refresh.as_mut() {
            run.keeps_status = true;
        }
    }

    /// 启动以后读回的资源库列表（Vue `loadRepositories` 拿到列表）：换上列表，空列表回到空库；否则选目标
    /// 仓库、当前仓库或第一项。选中别的仓库就是换仓库，选中同一个仓库接着同步。
    pub(super) fn apply_ready_list(&mut self, items: Vec<WorkspaceRepository>) {
        self.repositories = items;
        self.operation.update(OperationPatch::step("加载资源库摘要", 38.0));
        if self.repositories.is_empty() {
            self.assign_active(None);
            self.remember_repository(None);
            self.reset_library_category();
            self.presence = RepositoryPresence::Empty;
            self.finish_refresh();
            return;
        }
        let target = self.refresh.as_ref().and_then(|run| run.target.clone());
        let next = [target, self.active_repo_id.clone()]
            .into_iter()
            .flatten()
            .find(|repo_id| self.repositories.iter().any(|item| &item.repo_id == repo_id))
            .unwrap_or_else(|| self.repositories[0].repo_id.clone());
        self.select_repository(&next);
    }

    /// 选一个资源库（Vue `selectRepository`）：换到别的仓库回到启动页走三步；同一个仓库先同步再重读。
    /// 启动或换仓库还在走时，让进行中的结果作废，从同步重新开始。返回有没有选上。
    pub fn select_repository(&mut self, repo_id: &str) -> bool {
        let Some(repository) = self.repositories.iter().find(|item| item.repo_id == repo_id).cloned() else {
            eprintln!("Nana 资源库不在列表里，不能选择：{repo_id}");
            return false;
        };
        match self.startup.status {
            StartupStatus::Loading => self.restart_loading(repository),
            StartupStatus::Ready if self.active_repo_id.as_deref() == Some(repo_id) => self.select_same(repository),
            StartupStatus::Ready => self.begin_switch(repository),
            StartupStatus::Idle | StartupStatus::Error => {
                eprintln!("Nana 启动还没完成，不能选择资源库：{repo_id}");
                return false;
            }
        }
        true
    }

    /// 启动或换仓库途中又选了仓库：旧的同步和摘要结果作废，从这个仓库的同步重新走。和 Vue `selectRepository`
    /// 换仓库一样立即换上活动仓库。
    fn restart_loading(&mut self, repository: WorkspaceRepository) {
        self.list_generation = self.list_generation.saturating_add(1);
        self.assign_active(Some(repository.repo_id.clone()));
        self.startup.active_applied = true;
        if repository.status == "missing" {
            self.presence = RepositoryPresence::Missing;
            self.reset_library_category();
            self.remember_repository(Some(repository.repo_id));
            self.finish_loading_run();
            return;
        }
        self.presence = RepositoryPresence::Ready;
        self.begin_sync(repository.repo_id);
    }

    /// 换到别的仓库（Vue `selectRepository` 的 `isSwitchingRepository`）。
    fn begin_switch(&mut self, repository: WorkspaceRepository) {
        self.list_generation = self.list_generation.saturating_add(1);
        self.refresh = None;
        self.list_loading = false;
        self.startup.begin_switch();
        self.assign_active(Some(repository.repo_id.clone()));
        self.reset_library_category();
        if matches!(self.panel, WorkspacePanel::Trash | WorkspacePanel::SmartFolder | WorkspacePanel::Playlist) {
            // 回收站、智能文件夹和播放集属于上一个仓库。Vue 换仓库不改面板，回收站面板会接着显示新仓库的
            // 根目录（不是回收站），这里回到文件面板。
            self.panel = WorkspacePanel::Files;
        }
        self.snapshot_loading = true;
        self.operation.start(OperationStart::new("加载资源库", "读取资源库快照", 10.0).indeterminate());
        if repository.status == "missing" {
            self.presence = RepositoryPresence::Missing;
            self.remember_repository(Some(repository.repo_id));
            self.finish_loading_run();
            return;
        }
        self.presence = RepositoryPresence::Ready;
        self.operation.update(OperationPatch::step("扫描资源库文件", 28.0));
        self.begin_sync(repository.repo_id);
    }

    /// 同一个仓库（Vue `selectRepository` 不换仓库）：缺失时回到缺失页；否则先同步，再读摘要、重读当前目录。
    fn select_same(&mut self, repository: WorkspaceRepository) {
        let target_listing = self.refresh.as_ref().is_some_and(|run| run.stage == RefreshStage::Listing);
        self.list_generation = self.list_generation.saturating_add(1);
        if repository.status == "missing" {
            self.presence = RepositoryPresence::Missing;
            self.reset_library_category();
            self.remember_repository(Some(repository.repo_id));
            self.finish_refresh();
            return;
        }
        if !target_listing {
            self.refresh = None;
        }
        self.presence = RepositoryPresence::Ready;
        self.snapshot_loading = true;
        let run = self.refresh.get_or_insert_with(|| RefreshRun {
            generation: 0,
            stage: RefreshStage::Syncing,
            target: None,
            directory: None,
            fell_back: false,
            keeps_status: false,
        });
        run.generation = self.list_generation;
        run.stage = RefreshStage::Syncing;
        self.effects.push(WorkspaceEffect::SyncRepository { repo_id: repository.repo_id, generation: self.list_generation });
    }

    /// 同仓库刷新的同步结果。成功接着读摘要；失败收尾，返回写进状态区的文案。过期的结果不算。
    pub(super) fn note_refresh_synced(&mut self, generation: u64, result: Result<(), String>) -> Option<String> {
        let run = self.refresh.as_mut().filter(|run| run.stage == RefreshStage::Syncing && run.generation == generation);
        if run.is_none() {
            eprintln!("Nana 忽略过期的资源库同步结果：代次 {generation}");
            return None;
        }
        match result {
            Ok(()) => {
                let Some(repo_id) = self.active_repo_id.clone() else {
                    self.finish_refresh();
                    return Some("同步完成时没有活动资源库".into());
                };
                if let Some(run) = self.refresh.as_mut() {
                    run.stage = RefreshStage::Snapshot;
                }
                self.load_snapshot(repo_id);
                None
            }
            Err(error) => {
                eprintln!("Nana 刷新时同步资源库失败：{error}");
                self.finish_refresh();
                Some(format!("同步资源库失败：{error}"))
            }
        }
    }

    /// 这一代的摘要结果有人在等：启动（含换仓库）的第 3 步，或者同仓库刷新的读摘要。
    pub(crate) fn awaits_snapshot(&self, generation: u64) -> bool {
        if generation != self.list_generation {
            return false;
        }
        let startup = self.startup.status == StartupStatus::Loading && self.startup.current_step == 3;
        startup || self.refresh_stage() == Some(RefreshStage::Snapshot)
    }

    /// 同仓库刷新现在在哪一步。
    pub(crate) fn refresh_stage(&self) -> Option<RefreshStage> {
        self.refresh.as_ref().map(|run| run.stage)
    }

    /// 同仓库刷新读完摘要：进入重读目录，返回要读的目录。
    pub(crate) fn begin_refresh_directory(&mut self) -> Option<String> {
        let run = self.refresh.as_mut().filter(|run| run.stage == RefreshStage::Snapshot)?;
        run.stage = RefreshStage::Directory;
        Some(run.directory.clone().unwrap_or_default())
    }

    /// 壳层记下刷新开始时的当前目录（Vue `previousDirectoryPath`）。已经记下的不改。
    pub(crate) fn note_refresh_directory(&mut self, directory: &str) {
        if let Some(run) = self.refresh.as_mut().filter(|run| run.stage != RefreshStage::Listing)
            && run.directory.is_none()
        {
            run.directory = Some(directory.to_string());
        }
    }

    /// 读当前目录失败、还没退回根目录过：记下退回，返回 true，壳层接着读根目录。
    pub(crate) fn fall_back_to_root(&mut self) -> bool {
        let Some(run) = self.refresh.as_mut().filter(|run| run.stage == RefreshStage::Directory) else {
            return false;
        };
        if run.fell_back || run.directory.as_deref().unwrap_or_default().is_empty() {
            return false;
        }
        run.fell_back = true;
        run.directory = Some(String::new());
        true
    }

    /// 同仓库刷新收尾（成功或失败）：放下忙碌，任务弹层的这一行走到 100% 再收起。行已经被读目录接手时
    /// 完成不碰它（Vue `finishOperationProgress` 对过期编号什么也不做）。
    pub(crate) fn finish_refresh(&mut self) {
        self.refresh = None;
        self.list_loading = false;
        self.snapshot_loading = false;
        self.operation.finish();
    }

    /// 启动或换仓库的加载走完了（空列表、目标缺失时在选定以后，可用的仓库在读完首屏目录以后）：换仓库到这里结束，
    /// 任务弹层的「加载资源库」完成；首屏启动照 Vue 先在第 4 步等设置包和播放器类型（`StartupState::await_settings`），
    /// 都回来再结束。
    pub(super) fn finish_loading_run(&mut self) {
        self.snapshot_loading = false;
        if self.startup.mode == StartupMode::Launch {
            let repo_id = self.active_repo_id.clone();
            self.startup.await_settings(repo_id.as_deref());
            return;
        }
        self.startup.finish();
        self.operation.finish();
    }

    /// 把附加、新建或重定向返回的仓库摘要放进列表（Vue `upsertRepositorySummary`，按名称排序）。
    pub fn upsert_repository(&mut self, summary: &RepositorySummary) {
        let repository = WorkspaceRepository::from_summary(summary);
        self.repositories.retain(|item| item.repo_id != repository.repo_id);
        self.repositories.push(repository);
        self.repositories.sort_by(|left, right| compare_zh(&left.name, &right.name));
    }

    /// 删除当前资源库以后选下一个（Vue `selectNextRepositoryAfterRemoval`）：列表空了回到空库，
    /// 否则换到第一项。
    pub(super) fn remove_repository(&mut self, repo_id: &str) {
        self.repositories.retain(|repository| repository.repo_id != repo_id);
        if self.active_repo_id.as_deref() != Some(repo_id) {
            return;
        }
        let Some(next) = self.repositories.first().cloned() else {
            self.assign_active(None);
            self.presence = RepositoryPresence::Empty;
            self.remember_repository(None);
            self.reset_library_category();
            return;
        };
        self.begin_switch(next);
    }

    /// 重定向的结果（Vue `relocateMissingRepository`）：成功时任务弹层走到「更新资源库状态」64%，换上返回的
    /// 仓库摘要，再按同一个仓库同步、读摘要、读目录；失败写在缺失页上。
    pub fn note_relocate_finished(&mut self, result: Result<RepositorySummary, String>) {
        self.relocating = false;
        match result {
            Ok(summary) => {
                self.missing_error.clear();
                self.path_draft.clear();
                self.operation.update(OperationPatch::step("更新资源库状态", 64.0));
                self.upsert_repository(&summary);
                if !self.select_repository(&summary.repo_id) {
                    self.operation.finish();
                }
            }
            Err(error) => {
                eprintln!("Nana 重定向资源库失败：{error}");
                self.operation.cancel();
                self.missing_error = error;
            }
        }
    }
}
