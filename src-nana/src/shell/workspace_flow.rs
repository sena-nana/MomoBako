//! 工作台流程落到壳层上的那一层：资源库列表、同步、摘要和目录的结果写进壳层，换了活动仓库时清掉上一个
//! 仓库的内容，同一个仓库刷新时重读当前目录。状态机本身在 `workspace.rs` 和 `workspace_switch.rs`。

use crate::backend::services::repository::{FileBrowserSnapshot, RepositorySnapshot, RepositorySummary};

use super::files::FileContext;
use super::status::FailureSource;
use super::workspace::{RefreshStage, StartupMode};
use super::{MainRegion, ShellPage, ShellViewModel, StartupStatus, WorkspaceEffect, WorkspaceRepository};

impl ShellViewModel {
    /// 变更以后刷新工作区（Vue `refreshWorkspaceAfterMutation`，仓库动作执行完、确认硬链接以后）：静默重读
    /// 仓库摘要；当前不在回收站、也不是智能文件夹时非静默地重读当前目录，选择留着，任务弹层出「读取目录」。
    /// 已经换了仓库就不读。
    pub(super) fn reload_after_mutation(&mut self, repo_id: &str) {
        if self.workspace.active_repo_id.as_deref() != Some(repo_id) {
            eprintln!("Nana 变更完成时已经换了仓库，不重读：{repo_id}");
            return;
        }
        let context = FileContext::from_model(self);
        if !context.trash && !context.smart_folder {
            let path = self.files.current_path.clone();
            if !self.files.request_refresh_read(&context, &path) {
                eprintln!("Nana 变更完成后没能重读目录：{path}");
            }
        }
        self.workspace.effects.push(WorkspaceEffect::LoadSnapshotSilent { repo_id: repo_id.to_string() });
    }

    /// 把仓库列表结果写进工作台，再同步壳层上的仓库名称和页面。启动以后读失败时进状态区（缺失页自己显示）。
    pub(super) fn apply_loaded_repositories(&mut self, generation: Option<u64>, result: Result<Vec<RepositorySummary>, String>) {
        let mapped = match result {
            Ok(items) => Ok(items.iter().map(WorkspaceRepository::from_summary).collect()),
            Err(error) => Err(error),
        };
        let failed = mapped.as_ref().err().filter(|_| self.workspace.startup.status == StartupStatus::Ready).cloned();
        let applied = self.workspace.apply_repository_list(generation, mapped);
        if let Some(error) = failed.filter(|_| applied && self.workspace.main_region() != MainRegion::MissingRepository) {
            self.status.fail(FailureSource::Repository, format!("无法读取资源库列表：{error}"));
        }
        if !applied {
            return;
        }
        self.sync_repository_header();
        match self.workspace.main_region() {
            MainRegion::LoadError => self.page = ShellPage::Error,
            MainRegion::MissingRepository => self.browser_entries.clear(),
            MainRegion::EmptyRepository | MainRegion::HasRepository | MainRegion::Startup => {}
        }
        // 启动以后列表让当前仓库变成缺失或可用（不在刷新里，例如缺失仓库刷新后仍缺失），侧栏跟着换。
        // 还在同步、读摘要的刷新等摘要到了再绑定，免得目录树读到同步以前的结构。
        if self.workspace.startup.status == StartupStatus::Ready && self.workspace.refresh.is_none() {
            self.bind_sidebar_repository();
        }
    }

    /// 选一个资源库（切换列表、搜索命中、来源登录、附加成功……），见 `WorkspaceState::select_repository`。
    pub(super) fn select_workspace_repository(&mut self, repo_id: &str) {
        if self.workspace.select_repository(repo_id) {
            self.sync_repository_header();
        }
    }

    /// 同步的结果。同仓库刷新失败写进状态区；启动和换仓库失败停在启动页。
    pub(super) fn note_repository_synced(&mut self, generation: u64, result: Result<(), String>) {
        if let Some(failure) = self.workspace.note_sync_finished(generation, result) {
            self.status.fail(FailureSource::Repository, failure);
        }
        if self.workspace.main_region() == MainRegion::LoadError {
            self.page = ShellPage::Error;
        }
    }

    /// 仓库摘要的结果：写进仓库名称和侧栏（同步以后才绑定侧栏），启动和换仓库接着读首屏目录，
    /// 同一个仓库的刷新接着重读当前目录和侧栏。过期的结果不算。
    pub(super) fn apply_repository_snapshot(&mut self, generation: u64, result: Result<RepositorySnapshot, String>) {
        if !self.workspace.awaits_snapshot(generation) {
            eprintln!("Nana 忽略过期的资源库摘要：代次 {generation}");
            return;
        }
        let refreshing = self.workspace.refresh_stage() == Some(RefreshStage::Snapshot);
        match result {
            Ok(snapshot) => {
                let repo_id = snapshot.repository.repo_id.clone();
                if self.workspace.active_repo_id.as_deref() != Some(repo_id.as_str()) {
                    eprintln!("Nana 忽略别的仓库的资源库摘要：{repo_id}");
                    return;
                }
                self.repository_name = snapshot.repository.name.clone();
                self.repository_id = Some(repo_id.clone());
                self.apply_snapshot_sidebar(&snapshot);
                if refreshing {
                    self.reload_repository_side_data(&repo_id);
                    self.begin_refresh_directory();
                } else {
                    self.workspace.note_index_finished(Ok(()));
                }
            }
            Err(error) => {
                eprintln!("Nana 读取资源库摘要失败：{error}");
                if refreshing {
                    self.workspace.finish_refresh();
                    self.status.fail(FailureSource::Repository, format!("无法读取资源库摘要：{error}"));
                } else {
                    self.workspace.note_index_finished(Err(error));
                    self.page = ShellPage::Error;
                }
            }
        }
    }

    /// 目录快照到了：别的仓库的结果丢掉；文件表面认下以后，启动和换仓库的首屏目录到这里结束。
    pub(super) fn apply_file_browser(&mut self, browser: FileBrowserSnapshot) {
        if self.workspace.active_repo_id.as_deref() != Some(browser.repo_id.as_str()) {
            eprintln!("Nana 忽略别的仓库的目录：{}", browser.repo_id);
            return;
        }
        let virtual_view = FileContext::from_model(self).is_virtual();
        if !self.files.apply_browser(&browser, virtual_view) {
            return;
        }
        self.workspace.note_first_screen_finished(Ok(()));
        if !matches!(self.page, ShellPage::Settings | ShellPage::SettingsError) {
            self.page = ShellPage::FileList;
        }
        self.browser_entries = browser.entries;
        self.current_directory = browser.current_path.clone();
        self.sidebar.note_directory(&browser.current_path, browser.special_location.as_deref() == Some("trash"));
    }

    /// 读目录失败：文件表面自己发起的读取写在文件列；启动和换仓库的首屏失败停在启动页。
    pub(super) fn note_file_browser_failed(&mut self, error: String) {
        if self.files.note_load_failed(&error) {
            return;
        }
        eprintln!("Nana 读取首屏目录失败：{error}");
        if self.workspace.startup.status == StartupStatus::Ready {
            self.status.fail(FailureSource::Directory, format!("无法读取文件列表：{error}"));
        }
        self.workspace.note_first_screen_finished(Err(error));
        self.page = ShellPage::Error;
    }

    /// 归约结束时：活动仓库换了，清掉上一个仓库的内容；换仓库的启动页走完（或失败）时接着做等着的事；
    /// 同仓库刷新记下当前目录、等目录读完收尾。
    pub(super) fn settle_workspace(&mut self, active_before: &Option<String>, startup_before: StartupStatus) {
        if &self.workspace.active_repo_id != active_before {
            self.reset_repository_content();
        }
        let startup = &self.workspace.startup;
        if startup_before == StartupStatus::Loading && startup.mode == StartupMode::Switch {
            match startup.status {
                StartupStatus::Ready => super::inspect::search::resume_after_switch(self),
                StartupStatus::Error => super::inspect::search::drop_after_switch(self),
                StartupStatus::Idle | StartupStatus::Loading => {}
            }
        }
        let current = self.files.current_path.clone();
        self.workspace.note_refresh_directory(&current);
        self.settle_repository_refresh();
    }

    /// Vue `resetActiveRepositoryContent`：清掉上一个仓库的目录、选择、预览、搜索和侧栏内容。
    /// 换仓库还在走时侧栏先清空，读完摘要（同步以后）再绑定新仓库；缺失、空库和启动以后的直接绑定。
    fn reset_repository_content(&mut self) {
        self.files.reset_for_repository();
        self.inspect.clear();
        self.inspect.reset_search();
        self.player.disarm_preview_audio();
        self.selected_path = None;
        self.preview_token = None;
        self.preview_pixels = None;
        self.browser_entries.clear();
        self.current_directory.clear();
        self.selected_playlist_id = None;
        self.playlist_item_ids.clear();
        self.playlist_item_status.clear();
        if self.page == ShellPage::SelectedFile {
            self.page = ShellPage::FileList;
        }
        self.sync_repository_header();
        if self.workspace.startup.status == StartupStatus::Loading {
            self.sidebar.bind_repository(None, false);
        } else {
            self.bind_sidebar_repository();
        }
    }

    /// 壳层上的仓库名称和 id 跟着活动仓库；列表空了回到空库页。
    fn sync_repository_header(&mut self) {
        self.repository_id = self.workspace.active_repo_id.clone();
        if let Some(repository) = self.workspace.active_repository() {
            self.repository_name = repository.name.clone();
        } else if self.workspace.main_region() == MainRegion::EmptyRepository {
            self.page = ShellPage::EmptyRepository;
            self.repository_id = None;
            self.repository_name = "默认资源库".into();
            self.browser_entries.clear();
        }
    }

    /// 同一个仓库刷新时照 Vue `queueRepositoryBackgroundLoads` 在后台重读侧栏（目录树、智能文件夹、播放集）、
    /// 仓库动作和硬链接候选。
    fn reload_repository_side_data(&mut self, repo_id: &str) {
        let panel = self.workspace.panel;
        self.sidebar.refresh_after_structure(repo_id, panel);
        self.admin.queue_actions(Some(repo_id.to_string()));
        self.files.check_hardlinks(repo_id);
    }

    /// 同仓库刷新读完摘要：非静默地重读当前目录（任务弹层是「读取目录」）。智能文件夹视图没有目录可读、
    /// 文件正在变更时读不了，直接收尾。
    fn begin_refresh_directory(&mut self) {
        let Some(directory) = self.workspace.begin_refresh_directory() else {
            return;
        };
        let context = FileContext::from_model(self);
        if context.smart_folder || !self.files.request_refresh_read(&context, &directory) {
            eprintln!("Nana 刷新资源库不重读目录：智能文件夹视图或文件正在变更");
            self.workspace.finish_refresh();
        }
    }

    /// 同仓库刷新在等目录：目录不再在读时收尾。读当前目录失败、又不是根目录时，照 Vue 先退回根目录再读一次。
    fn settle_repository_refresh(&mut self) {
        if self.workspace.refresh_stage() != Some(RefreshStage::Directory) || self.files.loading {
            return;
        }
        if !self.files.error.is_empty() && self.workspace.fall_back_to_root() {
            let context = FileContext::from_model(self);
            if self.files.request_refresh_read(&context, "") {
                return;
            }
        }
        self.workspace.finish_refresh();
    }
}
