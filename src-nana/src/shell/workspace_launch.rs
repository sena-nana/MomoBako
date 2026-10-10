//! 首屏启动流程：读资源库列表、同步、读仓库摘要、读首屏目录，再等设置包和播放器类型。
//!
//! 对应 Vue `workspace/lifecycle.ts` 的 `ensureRepositoryWorkspace` / `loadInitialRepository` 和 `emitStartupLog`。
//! 步骤条和加载日志的数据在 `workspace_startup.rs`，换仓库和同一个仓库的刷新在 `workspace_switch.rs`；这里是
//! 把服务结果一步步推进成启动状态的那段状态机。换仓库借用其中的同步、读摘要和首屏目录三步。

use super::super::repo_operation::OperationPatch;
use super::startup::{IndexCounts, SettingsWait, StartupMode, StartupStatus, SyncCounts};
use super::{RepositoryPresence, WorkspaceEffect, WorkspaceRepository, WorkspaceState};

impl WorkspaceState {
    /// 首屏任务提交前把步骤条切到“正在读取仓库列表”。
    pub fn prepare_initial_list(&mut self) -> u64 {
        if self.startup.status == StartupStatus::Idle {
            self.list_generation = self.list_generation.saturating_add(1);
            self.begin_startup();
        }
        self.list_generation
    }

    /// 失败后重试。已就绪时不再重入，避免把当前工作区打回加载页。
    pub fn retry_startup(&mut self) -> bool {
        if self.startup.status == StartupStatus::Ready {
            return false;
        }
        self.list_generation = self.list_generation.saturating_add(1);
        self.begin_startup();
        self.effects.push(WorkspaceEffect::RefreshRepositories { generation: self.list_generation });
        true
    }

    /// 开始一轮启动：记下「流程开始」和「开始读取资源库列表」，步骤条进入第 1 步。「流程开始」的活动仓库是
    /// 上一轮留下的（[`Self::logged_active_repo_id`]），重试时和 Vue 一样带着失败那一轮已经换上的仓库。
    fn begin_startup(&mut self) {
        let active = self.logged_active_repo_id();
        self.startup.begin_with(active.as_deref(), self.last_active_repo_id.as_deref());
        self.startup.set_progress(1, "加载仓库列表", "读取已注册资源库，并匹配上次打开的工作区。");
        self.startup.log("info", "首屏启动开始读取资源库列表。", &[("step", Some("1"))]);
    }

    /// 加载日志里的活动仓库，照 Vue `activeRepoId` 的时机：首屏启动换上目标（读完摘要、选中缺失仓库、列表为空）
    /// 以前还是这一轮开始时的值，换仓库一开始就是目标。Nana 的 `active_repo_id` 在选中时就换成目标，日志不能直接用它。
    fn logged_active_repo_id(&self) -> Option<String> {
        if self.startup.active_applied || self.startup.mode == StartupMode::Switch {
            self.active_repo_id.clone()
        } else {
            self.startup.base_repo_id.clone()
        }
    }

    /// 启动失败：停在当前步骤，日志照 Vue 带上活动仓库（读完摘要以后才是这次的仓库）和正在加载的目标资源库。
    /// 换仓库失败照 Vue 不写日志（`StartupState::log` 跳过），任务弹层这一行取消。
    fn fail_startup(&mut self, error: impl Into<String>) {
        let (active, target) = (self.logged_active_repo_id(), self.active_repo_id.clone());
        self.startup.fail_with(error, active.as_deref(), target.as_deref());
        self.presence = RepositoryPresence::LoadError;
        self.snapshot_loading = false;
        if self.startup.mode == StartupMode::Switch {
            self.operation.cancel();
        }
    }

    /// 应用一轮仓库列表，返回是否应用了。`generation` 不匹配时保留当前步骤和已选仓库。
    /// 启动以后的列表接着选仓库（[`Self::apply_ready_list`]）；读失败时取消「加载资源库」这一行。
    pub fn apply_repository_list(&mut self, generation: Option<u64>, result: Result<Vec<WorkspaceRepository>, String>) -> bool {
        if let Some(generation) = generation
            && generation != self.list_generation
        {
            eprintln!("Nana 忽略过期的资源库列表结果：代次 {generation}，当前 {}", self.list_generation);
            return false;
        }
        let startup_open = self.startup.status != StartupStatus::Ready;
        match result {
            Err(error) => {
                eprintln!("Nana 读取资源库列表失败：{error}");
                self.list_loading = false;
                if startup_open {
                    if self.startup.status != StartupStatus::Loading {
                        self.begin_startup();
                    }
                    self.fail_startup(error);
                } else {
                    self.refresh = None;
                    self.operation.cancel();
                    self.missing_error = error;
                }
            }
            Ok(items) if startup_open => {
                self.list_loading = false;
                self.apply_repository_items(items);
            }
            Ok(items) => self.apply_ready_list(items),
        }
        true
    }

    /// 同步的结果：启动和换仓库接着读摘要；同一个仓库的刷新见 [`Self::note_refresh_synced`]。
    /// 返回要写进状态区的失败（只有同仓库刷新会有，启动的失败停在启动页上）。
    pub fn note_sync_finished(&mut self, generation: u64, result: Result<SyncCounts, String>) -> Option<String> {
        if self.startup.status != StartupStatus::Loading {
            return self.note_refresh_synced(generation, result.map(|_| ()));
        }
        if generation != self.list_generation {
            eprintln!("Nana 忽略过期的资源库同步结果：代次 {generation}");
            return None;
        }
        match result {
            Ok(counts) => {
                if let Some(repo_id) = self.active_repo_id.clone() {
                    let numbers = [
                        counts.scanned_files,
                        counts.created_assets,
                        counts.updated_assets,
                        counts.deleted_assets,
                        counts.created_events,
                        counts.hardlink_candidates,
                    ]
                    .map(|count| count.to_string());
                    self.startup.log(
                        "info",
                        "首屏启动文件变化同步完成。",
                        &[
                            ("step", Some("2")),
                            ("scannedFiles", Some(numbers[0].as_str())),
                            ("createdAssets", Some(numbers[1].as_str())),
                            ("updatedAssets", Some(numbers[2].as_str())),
                            ("deletedAssets", Some(numbers[3].as_str())),
                            ("createdEvents", Some(numbers[4].as_str())),
                            ("hardlinkCandidates", Some(numbers[5].as_str())),
                        ],
                    );
                    self.startup.set_progress(
                        3,
                        "读取仓库摘要",
                        "读取资源库摘要、素材索引和默认预览对象。",
                    );
                    self.startup.log("info", "首屏启动开始读取资源库摘要。", &[("step", Some("3"))]);
                    self.load_snapshot(repo_id);
                } else {
                    self.fail_startup("同步完成时没有活动资源库");
                }
            }
            Err(error) => {
                eprintln!("Nana 同步资源库失败：{error}");
                self.fail_startup(error);
            }
        }
        None
    }

    /// 启动（含换仓库）第 3 步的摘要到了：成功记下摘要计数、进入第 4 步、排下首屏目录的读取，记住这个仓库
    /// （Vue 读完摘要才 `rememberLastActiveRepository`）；失败停在第 3 步。调用方先用 [`Self::awaits_snapshot`]
    /// 认过代次和仓库。
    pub fn note_index_finished(&mut self, result: Result<IndexCounts, String>) {
        if self.startup.status != StartupStatus::Loading || self.startup.current_step != 3 {
            return;
        }
        let Some(repo_id) = self.active_repo_id.clone() else {
            self.fail_startup("读完摘要时没有活动资源库");
            return;
        };
        match result {
            Ok(counts) => {
                let (assets, playlists) = (counts.asset_count.to_string(), counts.playlist_count.to_string());
                self.startup.log(
                    "info",
                    "首屏启动资源库摘要读取完成。",
                    &[
                        ("step", Some("3")),
                        ("assetCount", Some(assets.as_str())),
                        ("playlistCount", Some(playlists.as_str())),
                        ("defaultAssetId", counts.default_asset_id.as_deref()),
                    ],
                );
                self.startup.index = Some(counts);
                self.startup.active_applied = true;
                self.remember_repository(Some(repo_id.clone()));
                if self.startup.mode == StartupMode::Switch {
                    self.operation.update(OperationPatch::step("加载资源索引", 46.0));
                }
                let detail = self.startup.first_screen_detail();
                self.startup.set_progress(4, "读取首屏目录", detail);
                self.startup.log("info", "首屏启动开始加载首屏目录与关联数据。", &[("step", Some("4"))]);
                self.effects.push(WorkspaceEffect::LoadFirstScreen { repo_id });
            }
            Err(error) => {
                eprintln!("Nana 读取资源库摘要失败：{error}");
                self.fail_startup(error);
            }
        }
    }

    /// 首屏目录到了：换仓库到这里结束、任务弹层的「加载资源库」完成；首屏启动接着等设置包（[`Self::finish_loading_run`]）。
    /// 失败停在第 4 步。已经在等设置包时再来的目录不算。
    pub fn note_first_screen_finished(&mut self, result: Result<(), String>) {
        if self.startup.status != StartupStatus::Loading
            || self.startup.current_step < 4
            || self.startup.settings != SettingsWait::None
        {
            return;
        }
        match result {
            Ok(()) => {
                let playlists = self.startup.index.as_ref().map_or(0, |counts| counts.playlist_count).to_string();
                self.startup.log(
                    "info",
                    "首屏启动首屏目录与关联数据加载完成。",
                    &[("step", Some("4")), ("playlistCount", Some(playlists.as_str()))],
                );
                self.presence = RepositoryPresence::Ready;
                self.finish_loading_run();
            }
            Err(error) => {
                eprintln!("Nana 读取首屏目录失败：{error}");
                self.fail_startup(error);
            }
        }
    }

    /// 首屏启动等的设置包回来了（失败也算）。`players_requested`：插件列表换了新的、排下了播放器类型的读取，接着等它；
    /// 否则首屏启动到这里结束。不是首屏启动在等的设置包（打开设置页、插件面板「刷新」）什么也不做。
    pub fn note_settings_loaded(&mut self, players_requested: bool) {
        if self.startup.settings != SettingsWait::Bundle {
            return;
        }
        if players_requested {
            self.startup.settings = SettingsWait::Players;
        } else {
            self.finish_launch();
        }
    }

    /// 首屏启动等的播放器类型回来了（失败也算）：首屏启动结束。别的时候读回的播放器类型什么也不做。
    pub fn note_players_loaded(&mut self) {
        if self.startup.settings == SettingsWait::Players {
            self.finish_launch();
        }
    }

    fn finish_launch(&mut self) {
        let repo_id = self.active_repo_id.clone();
        self.startup.finish_launch(repo_id.as_deref());
    }

    /// 工作区的内容已经读好：启动结束了，或者首屏启动读完首屏目录、只在等设置包。Vue 这时已经换上了
    /// `activeRepoId`，结构更新照常静默刷新，Nana 在等设置包的这段也一样，不丢掉外面的改动。
    pub fn content_loaded(&self) -> bool {
        self.startup.status == StartupStatus::Ready || self.startup.settings != SettingsWait::None
    }

    /// 启动时读回的列表。选择顺序：当前 id、上次记住的 id（还在列表里时）、列表第一项。空列表清空记住的仓库，
    /// 选中的仓库缺失时立即记住它。
    fn apply_repository_items(&mut self, items: Vec<WorkspaceRepository>) {
        self.repositories = items;
        let total = self.repositories.len().to_string();
        let ready = self.repositories.iter().filter(|item| item.status == "ready").count().to_string();
        let missing = self.repositories.iter().filter(|item| item.status == "missing").count().to_string();
        self.startup.log(
            "info",
            "首屏启动资源库列表读取完成。",
            &[("step", Some("1")), ("repositoryCount", Some(&total)), ("readyRepositoryCount", Some(&ready)), ("missingRepositoryCount", Some(&missing))],
        );
        if self.repositories.is_empty() {
            self.startup.log("warn", "首屏启动未找到可加载的资源库。", &[("repositoryCount", Some("0"))]);
            self.assign_active(None);
            self.startup.active_applied = true;
            self.remember_repository(None);
            self.reset_library_category();
            self.presence = RepositoryPresence::Empty;
            self.finish_loading_run();
            return;
        }
        let next_id = self
            .active_repo_id
            .clone()
            .filter(|repo_id| self.repositories.iter().any(|item| &item.repo_id == repo_id))
            .or_else(|| {
                self.last_active_repo_id.clone().filter(|repo_id| {
                    self.repositories.iter().any(|item| &item.repo_id == repo_id)
                })
            })
            .unwrap_or_else(|| self.repositories[0].repo_id.clone());
        let next = self.repositories.iter().find(|item| item.repo_id == next_id).cloned();
        let missing = next.as_ref().is_some_and(|item| item.status == "missing");
        let remembered = self.last_active_repo_id.clone();
        self.startup.log(
            "info",
            "首屏启动已选定资源库。",
            &[
                ("repositoryCount", Some(&total)),
                ("rememberedRepoId", remembered.as_deref()),
                ("repositoryStatus", next.as_ref().map(|item| item.status.as_str())),
                ("repositoryName", next.as_ref().map(|item| item.name.as_str())),
                ("repositoryPath", next.as_ref().map(|item| item.path.as_str())),
            ],
        );
        self.assign_active(Some(next_id.clone()));
        if missing {
            self.startup.log(
                "warn",
                "首屏启动遇到缺失资源库。",
                &[
                    ("repositoryName", next.as_ref().map(|item| item.name.as_str())),
                    ("repositoryPath", next.as_ref().map(|item| item.path.as_str())),
                ],
            );
            self.startup.active_applied = true;
            self.remember_repository(Some(next_id));
            self.reset_library_category();
            self.presence = RepositoryPresence::Missing;
            self.finish_loading_run();
            return;
        }
        self.presence = RepositoryPresence::Ready;
        self.begin_sync(next_id);
    }

    /// 启动和换仓库进入第 2 步：记下「开始同步文件变化」并发出同步请求。
    pub(super) fn begin_sync(&mut self, repo_id: String) {
        self.startup.set_progress(2, "扫描资源库文件", "同步文件变化，更新新增、移动和删除记录。");
        self.startup.log("info", "首屏启动开始同步文件变化。", &[("step", Some("2"))]);
        self.effects.push(WorkspaceEffect::SyncRepository { repo_id, generation: self.list_generation });
    }

    /// 读仓库摘要，带上这一代，过期的结果认得出来。
    pub(super) fn load_snapshot(&mut self, repo_id: String) {
        self.snapshot_loading = true;
        self.effects.push(WorkspaceEffect::LoadSnapshot { repo_id, generation: self.list_generation });
    }
}
