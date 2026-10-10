//! 工作台状态机。
//!
//! 对应 Vue `AppShell.vue`、`workspace/lifecycle.ts`、`workspace/state.ts`、
//! `MissingRepositoryState.vue`、`EmptyRepositoryState.vue` 和缺失仓库动作。
//! 启动步骤、主区、侧栏和删除对话框都在这里归约，页面控件只消费结果。换仓库和同一个仓库的刷新在
//! `workspace_switch.rs`。

use std::{fs, path::Path};

use crate::backend::services::repository::RepositorySummary;
use crate::settings;
use crate::theme_map::{SIDEBAR_DEFAULT_PX, SIDEBAR_MAX_PX, SIDEBAR_MIN_PX};

use super::repo_operation::{OperationRequests, OperationStart};

/// 与 Vue `momobako.sidebarCollapsed` 相同的存储键。
pub const SIDEBAR_COLLAPSED_KEY: &str = "momobako.sidebarCollapsed";
/// 与 Vue `momobako.sidebarWidth` 相同的存储键。
pub const SIDEBAR_WIDTH_KEY: &str = "momobako.sidebarWidth";

#[path = "workspace_startup.rs"]
mod startup;
pub use startup::{StartupLog, StartupMode, StartupState, StartupStatus, StartupStepItem, StartupStepState};
#[path = "workspace_switch.rs"]
mod switch;
pub use switch::{RefreshRun, RefreshStage};
/// 工作台主面板，对应 `WorkspacePanelKey`。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum WorkspacePanel {
    #[default]
    Files,
    Trash,
    Search,
    SmartFolder,
    Playlist,
    Actions,
    Extensions,
    Logs,
}

/// 库分类，对应 `WorkspaceLibraryCategoryKey`。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LibraryCategory {
    #[default]
    All,
    Uncategorized,
    Untagged,
    Recent,
}

/// 启动完成后的四条互斥主区。加载过程中仍是启动步骤，不占这四条。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MainRegion {
    Startup,
    HasRepository,
    MissingRepository,
    EmptyRepository,
    LoadError,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum RepositoryPresence {
    #[default]
    Empty,
    Ready,
    Missing,
    LoadError,
}

/// 列表项里归约所需的仓库字段。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceRepository {
    pub repo_id: String,
    pub name: String,
    pub path: String,
    pub status: String,
    pub backend_plugin_id: String,
    pub capabilities: Vec<String>,
    pub cache_required: bool,
    pub cache_status: String,
}

impl WorkspaceRepository {
    pub fn from_summary(summary: &RepositorySummary) -> Self {
        Self {
            repo_id: summary.repo_id.clone(),
            name: summary.name.clone(),
            path: summary.path.clone(),
            status: summary.status.clone(),
            backend_plugin_id: summary.backend.plugin_id.clone(),
            capabilities: summary.backend.capabilities.clone(),
            cache_required: summary.local_cache.as_ref().is_some_and(|cache| cache.required),
            cache_status: summary
                .local_cache
                .as_ref()
                .map(|cache| cache.status.clone())
                .unwrap_or_default(),
        }
    }

    /// 来源缓存未就绪时，主操作是打开插件设置，而不是重定向本地文件夹。
    pub fn is_source_cache_issue(&self) -> bool {
        self.cache_required && self.cache_status != "ready"
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeleteMode {
    RecordOnly,
    DeleteMetadata,
    DeleteFolder,
}

impl DeleteMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::RecordOnly => "只删除记录",
            Self::DeleteMetadata => "删除 .momo 数据",
            Self::DeleteFolder => "删除整个文件夹",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeleteDialog {
    pub repo_id: String,
    pub error: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspaceDialog {
    Delete(DeleteDialog),
}

/// 宿主在归约后执行的副作用。列表、同步和摘要带着代次，换仓库、重试或新的刷新让旧结果作废。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspaceEffect {
    RefreshRepositories { generation: u64 },
    /// 结构更新后重拉列表。不带代次，不能改启动步骤或页面。
    RefreshRepositoriesSilent,
    SyncRepository { repo_id: String, generation: u64 },
    LoadSnapshot { repo_id: String, generation: u64 },
    /// 启动和换仓库的第 4 步：读根目录首屏。
    LoadFirstScreen { repo_id: String },
    /// 结构更新后重拉当前仓库摘要。失败不能把页面改成错误。
    LoadSnapshotSilent { repo_id: String },
    RelocateRepository { repo_id: String, path: String },
    DeleteRepository { repo_id: String, mode: DeleteMode },
    PersistSidebar,
    StopPlayback { previous_repo_id: String },
}

#[derive(Clone, Debug)]
pub struct WorkspaceState {
    pub panel: WorkspacePanel,
    pub library_category: LibraryCategory,
    pub repositories: Vec<WorkspaceRepository>,
    pub active_repo_id: Option<String>,
    pub last_active_repo_id: Option<String>,
    pub startup: StartupState,
    presence: RepositoryPresence,
    pub sidebar_collapsed: bool,
    pub sidebar_width: f32,
    pub path_prompt: bool,
    pub path_draft: String,
    pub relocating: bool,
    pub deleting_mode: Option<DeleteMode>,
    pub missing_error: String,
    pub dialogs: Vec<WorkspaceDialog>,
    pub stop_playback: bool,
    /// 列表和同步请求的代次。重试或新的刷新会使旧结果失效。
    pub list_generation: u64,
    /// 启动以后在刷新资源库（Vue `isLoadingRepositories`）：从重读列表到选完仓库。
    pub list_loading: bool,
    /// 在选仓库（Vue `isLoadingSnapshot`）：换仓库或同一个仓库的同步、摘要和重读目录，整轮结束时放下。
    pub snapshot_loading: bool,
    /// 进行中的同仓库刷新，见 `workspace_switch.rs`。
    pub refresh: Option<RefreshRun>,
    /// 对任务弹层「仓库操作」行的请求，归约结束时交给壳层的槽位。
    pub(crate) operation: OperationRequests,
    pub effects: Vec<WorkspaceEffect>,
}

impl Default for WorkspaceState {
    fn default() -> Self {
        Self {
            panel: WorkspacePanel::Files,
            library_category: LibraryCategory::All,
            repositories: Vec::new(),
            active_repo_id: None,
            last_active_repo_id: None,
            startup: StartupState::default(),
            presence: RepositoryPresence::Empty,
            sidebar_collapsed: false,
            sidebar_width: SIDEBAR_DEFAULT_PX,
            path_prompt: false,
            path_draft: String::new(),
            relocating: false,
            deleting_mode: None,
            missing_error: String::new(),
            dialogs: Vec::new(),
            stop_playback: false,
            list_generation: 0,
            list_loading: false,
            snapshot_loading: false,
            refresh: None,
            operation: OperationRequests::default(),
            effects: Vec::new(),
        }
    }
}

impl WorkspaceState {
    pub fn main_region(&self) -> MainRegion {
        match self.startup.status {
            StartupStatus::Error => MainRegion::LoadError,
            StartupStatus::Ready => match self.presence {
                RepositoryPresence::Ready => MainRegion::HasRepository,
                RepositoryPresence::Missing => MainRegion::MissingRepository,
                RepositoryPresence::Empty => MainRegion::EmptyRepository,
                RepositoryPresence::LoadError => MainRegion::LoadError,
            },
            StartupStatus::Idle | StartupStatus::Loading => MainRegion::Startup,
        }
    }

    pub fn active_repository(&self) -> Option<&WorkspaceRepository> {
        self.active_repo_id.as_ref().and_then(|repo_id| {
            self.repositories.iter().find(|repository| &repository.repo_id == repo_id)
        })
    }

    pub fn missing_busy(&self) -> bool {
        self.relocating || self.deleting_mode.is_some()
    }

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

    /// 开始一轮启动：记下「流程开始」和「开始读取资源库列表」，步骤条进入第 1 步。
    fn begin_startup(&mut self) {
        self.startup.begin_with(self.active_repo_id.as_deref(), self.last_active_repo_id.as_deref());
        self.startup.set_progress(1, "加载仓库列表", "读取已注册资源库，并匹配上次打开的工作区。");
        self.startup.log("info", "首屏启动开始读取资源库列表。", &[("step", Some("1"))]);
    }

    /// 启动失败：停在当前步骤，日志带上正在加载的目标资源库。
    /// Vue 在首屏加载完成前不写 `activeRepoId`，所以日志里只有 `targetRepoId`。
    /// 换仓库失败照 Vue 不写日志（`StartupState::log` 跳过），任务弹层这一行取消。
    fn fail_startup(&mut self, error: impl Into<String>) {
        let target = self.active_repo_id.clone();
        self.startup.fail_with(error, None, target.as_deref());
        self.presence = RepositoryPresence::LoadError;
        self.snapshot_loading = false;
        if self.startup.mode == StartupMode::Switch {
            self.operation.cancel();
        }
    }

    /// 刷新资源库，读完列表照 Vue 选当前仓库（不在列表里选第一项），见 [`Self::refresh_repositories`]。
    pub fn request_repository_refresh(&mut self) {
        self.refresh_repositories(None);
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
    pub fn note_sync_finished(&mut self, generation: u64, result: Result<(), String>) -> Option<String> {
        if self.startup.status != StartupStatus::Loading {
            return self.note_refresh_synced(generation, result);
        }
        if generation != self.list_generation {
            eprintln!("Nana 忽略过期的资源库同步结果：代次 {generation}");
            return None;
        }
        match result {
            Ok(()) => {
                if let Some(repo_id) = self.active_repo_id.clone() {
                    self.startup.log("info", "首屏启动文件变化同步完成。", &[("step", Some("2"))]);
                    self.last_active_repo_id = Some(repo_id.clone());
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

    /// 启动（含换仓库）第 3 步的摘要到了：成功进入第 4 步、排下首屏目录的读取，记住这个仓库；失败停在
    /// 第 3 步。调用方先用 [`Self::awaits_snapshot`] 认过代次和仓库。
    pub fn note_index_finished(&mut self, result: Result<(), String>) {
        if self.startup.status != StartupStatus::Loading || self.startup.current_step != 3 {
            return;
        }
        let Some(repo_id) = self.active_repo_id.clone() else {
            self.fail_startup("读完摘要时没有活动资源库");
            return;
        };
        match result {
            Ok(()) => {
                self.startup.log("info", "首屏启动资源库摘要读取完成。", &[("step", Some("3"))]);
                self.last_active_repo_id = Some(repo_id.clone());
                if self.startup.mode == StartupMode::Switch {
                    self.operation.update(super::repo_operation::OperationPatch::step("加载资源索引", 46.0));
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

    /// 首屏目录到了：启动（含换仓库）结束，任务弹层的「加载资源库」完成；失败停在第 4 步。
    pub fn note_first_screen_finished(&mut self, result: Result<(), String>) {
        if self.startup.status != StartupStatus::Loading || self.startup.current_step < 4 {
            return;
        }
        match result {
            Ok(()) => {
                self.startup.log("info", "首屏启动首屏目录与关联数据加载完成。", &[("step", Some("4"))]);
                self.startup.finish();
                self.presence = RepositoryPresence::Ready;
                self.snapshot_loading = false;
                if self.startup.mode == StartupMode::Switch {
                    self.operation.finish();
                }
            }
            Err(error) => {
                eprintln!("Nana 读取首屏目录失败：{error}");
                self.fail_startup(error);
            }
        }
    }

    pub fn toggle_sidebar(&mut self) {
        self.sidebar_collapsed = !self.sidebar_collapsed;
        self.effects.push(WorkspaceEffect::PersistSidebar);
    }

    pub fn set_sidebar_width(&mut self, width: f32) {
        self.sidebar_width = clamp_sidebar_width(width);
    }

    /// 对应 Vue 的 `@resize-end`：拖动过程中只夹取，松手后才写入。
    pub fn commit_sidebar_width(&mut self) {
        self.sidebar_width = clamp_sidebar_width(self.sidebar_width);
        self.effects.push(WorkspaceEffect::PersistSidebar);
    }

    pub fn sidebar_collapsed_storage(&self) -> &'static str {
        if self.sidebar_collapsed { "1" } else { "0" }
    }

    pub fn sidebar_width_storage(&self) -> String {
        format!("{}", self.sidebar_width)
    }

    pub fn apply_sidebar_storage(&mut self, collapsed: Option<&str>, width: Option<&str>) {
        self.sidebar_collapsed = collapsed == Some("1");
        self.sidebar_width = parse_sidebar_width(width);
    }

    pub fn load_prefs_file(&mut self, path: &Path) {
        match fs::read_to_string(path) {
            Ok(raw) => match serde_json::from_str::<serde_json::Value>(&raw) {
                Ok(value) => self.apply_sidebar_storage(
                    value.get(SIDEBAR_COLLAPSED_KEY).and_then(|item| item.as_str()),
                    value.get(SIDEBAR_WIDTH_KEY).and_then(|item| item.as_str()),
                ),
                Err(error) => eprintln!("Nana 侧栏偏好解析失败：{error}"),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => eprintln!("Nana 侧栏偏好读取失败：{error}"),
        }
    }

    pub fn save_prefs_file(&self, path: &Path) {
        let value = serde_json::json!({
            SIDEBAR_COLLAPSED_KEY: self.sidebar_collapsed_storage(),
            SIDEBAR_WIDTH_KEY: self.sidebar_width_storage(),
        });
        if let Some(parent) = path.parent()
            && let Err(error) = fs::create_dir_all(parent)
        {
            eprintln!("Nana 侧栏偏好目录创建失败：{error}");
            return;
        }
        if let Err(error) = fs::write(path, value.to_string()) {
            eprintln!("Nana 侧栏偏好写入失败：{error}");
        }
    }

    pub fn refresh_missing(&mut self) {
        if self.missing_busy() {
            return;
        }
        self.missing_error.clear();
        self.request_repository_refresh();
    }

    pub fn choose_missing_path(&mut self) {
        if self.active_repo_id.is_none() || self.missing_busy() {
            return;
        }
        self.missing_error.clear();
        self.path_prompt = true;
    }

    pub fn set_path_draft(&mut self, value: String) {
        self.path_draft = value;
    }

    /// 空选择不进入重定向，避免对话框取消被当成一次修复。
    pub fn submit_missing_path(&mut self) {
        if self.missing_busy() {
            return;
        }
        let Some(repo_id) = self.active_repo_id.clone() else {
            return;
        };
        let path = self.path_draft.trim().to_string();
        if path.is_empty() {
            return;
        }
        self.path_prompt = false;
        self.relocating = true;
        self.missing_error.clear();
        self.operation.start(OperationStart::new("重定向资源库", "校验资源库位置", 12.0));
        self.effects.push(WorkspaceEffect::RelocateRepository { repo_id, path });
    }

    /// 来源缓存没就绪时「打开来源设置」：返回要打开设置的来源插件。忙着或不是缓存问题时不打开。
    pub fn open_source_settings(&mut self) -> Option<String> {
        if self.missing_busy() {
            return None;
        }
        let plugin_id = self
            .active_repository()
            .filter(|repository| repository.is_source_cache_issue())
            .map(|repository| repository.backend_plugin_id.clone())?;
        self.missing_error.clear();
        Some(plugin_id)
    }

    pub fn delete_dialog_open(&self) -> bool {
        self.dialogs.iter().any(|dialog| matches!(dialog, WorkspaceDialog::Delete(_)))
    }

    pub fn open_delete_dialog(&mut self) {
        let Some(repo_id) = self.active_repo_id.clone() else {
            return;
        };
        if self.missing_busy() || !self.repositories.iter().any(|repository| repository.repo_id == repo_id) {
            return;
        }
        self.missing_error.clear();
        if let Some(WorkspaceDialog::Delete(dialog)) = self.dialogs.last_mut() {
            dialog.repo_id = repo_id;
            dialog.error.clear();
            return;
        }
        self.dialogs.push(WorkspaceDialog::Delete(DeleteDialog { repo_id, error: String::new() }));
    }

    pub fn close_delete_dialog(&mut self) {
        if self.deleting_mode.is_some() {
            return;
        }
        self.dialogs.pop();
    }

    pub fn confirm_delete(&mut self, mode: DeleteMode) -> bool {
        if self.deleting_mode.is_some() || !self.delete_mode_enabled(mode) {
            return false;
        }
        let Some(WorkspaceDialog::Delete(dialog)) = self.dialogs.last() else {
            return false;
        };
        let repo_id = dialog.repo_id.clone();
        self.deleting_mode = Some(mode);
        self.effects.push(WorkspaceEffect::DeleteRepository { repo_id, mode });
        true
    }

    pub fn note_delete_finished(&mut self, result: Result<(), String>) {
        let mode = self.deleting_mode.take();
        let Some(mode) = mode else {
            return;
        };
        match result {
            Ok(()) => {
                let repo_id = self.dialogs.last().and_then(|dialog| match dialog {
                    WorkspaceDialog::Delete(dialog) => Some(dialog.repo_id.clone()),
                });
                self.dialogs.clear();
                if let Some(repo_id) = repo_id {
                    self.remove_repository(&repo_id);
                }
                let _ = mode;
            }
            Err(error) => {
                eprintln!("Nana 删除资源库失败：{error}");
                if let Some(WorkspaceDialog::Delete(dialog)) = self.dialogs.last_mut() {
                    dialog.error = error;
                }
            }
        }
    }

    pub fn delete_mode_enabled(&self, mode: DeleteMode) -> bool {
        let Some(WorkspaceDialog::Delete(dialog)) = self.dialogs.last() else {
            return false;
        };
        let Some(repository) = self.repositories.iter().find(|item| item.repo_id == dialog.repo_id) else {
            return false;
        };
        match mode {
            DeleteMode::RecordOnly => true,
            DeleteMode::DeleteMetadata => can_delete_metadata(repository),
            DeleteMode::DeleteFolder => can_delete_folder(repository),
        }
    }

    pub fn delete_mode_detail(&self, mode: DeleteMode) -> String {
        let cache_required = self
            .dialogs
            .last()
            .and_then(|dialog| match dialog {
                WorkspaceDialog::Delete(dialog) => self.repositories.iter().find(|item| item.repo_id == dialog.repo_id),
            })
            .is_some_and(|repository| repository.cache_required);
        let enabled = self.delete_mode_enabled(mode);
        match mode {
            DeleteMode::RecordOnly => "只从 MomoBako 移除这条资源库记录，保留文件夹、.momo 和应用托管数据。".into(),
            DeleteMode::DeleteMetadata if enabled && cache_required => {
                "删除该资源库的 Momo 元数据目录与索引缓存，保留缓存文件夹中的其他用户内容。".into()
            }
            DeleteMode::DeleteMetadata if enabled => "删除该资源库的 .momo 数据目录，保留原文件夹与用户文件。".into(),
            DeleteMode::DeleteMetadata => "当前资源库的 .momo 目录不可直接访问，请先恢复路径后再删除元数据。".into(),
            DeleteMode::DeleteFolder if enabled => "递归删除当前资源库文件夹，目录内的 .momo 数据与用户文件会一起删除。".into(),
            DeleteMode::DeleteFolder => "当前资源库目录不可直接访问，暂时不能删除整个文件夹。".into(),
        }
    }

    pub fn missing_primary_label(&self) -> &'static str {
        if self.active_repository().is_some_and(|repository| repository.is_source_cache_issue()) {
            "打开来源设置"
        } else if self.relocating {
            "重定向中..."
        } else {
            "重定向"
        }
    }

    pub fn missing_delete_label(&self) -> &'static str {
        if self.deleting_mode.is_some() { "删除中..." } else { "删除资源库" }
    }

    pub fn take_effects(&mut self) -> Vec<WorkspaceEffect> {
        std::mem::take(&mut self.effects)
    }

    /// 测试直接进入已有仓库，不排队同步或快照，界面验收（`src-nana/tests`）也用它。产品路径上进仓库都要
    /// 先同步（启动和换仓库），验收场景走真实的启动消息（`acceptance_base.rs`）。
    #[doc(hidden)]
    pub fn present_repository(&mut self, repository: WorkspaceRepository) {
        self.active_repo_id = Some(repository.repo_id.clone());
        self.last_active_repo_id = self.active_repo_id.clone();
        self.repositories = vec![repository];
        self.presence = RepositoryPresence::Ready;
        self.startup.finish();
    }

    /// 启动时读回的列表。选择顺序：当前 id、上次记住的 id、列表第一项。空列表清空记住的仓库。
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
            self.last_active_repo_id = None;
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
            self.last_active_repo_id = Some(next_id);
            self.reset_library_category();
            self.presence = RepositoryPresence::Missing;
            self.finish_loading_run();
            return;
        }
        self.presence = RepositoryPresence::Ready;
        self.begin_sync(next_id);
    }

    /// 启动和换仓库进入第 2 步：记下「开始同步文件变化」并发出同步请求。
    fn begin_sync(&mut self, repo_id: String) {
        self.startup.set_progress(2, "扫描资源库文件", "同步文件变化，更新新增、移动和删除记录。");
        self.startup.log("info", "首屏启动开始同步文件变化。", &[("step", Some("2"))]);
        self.effects.push(WorkspaceEffect::SyncRepository { repo_id, generation: self.list_generation });
    }

    /// 读仓库摘要，带上这一代，过期的结果认得出来。
    fn load_snapshot(&mut self, repo_id: String) {
        self.snapshot_loading = true;
        self.effects.push(WorkspaceEffect::LoadSnapshot { repo_id, generation: self.list_generation });
    }

    fn assign_active(&mut self, next: Option<String>) {
        let previous = self.active_repo_id.clone();
        if previous != next {
            self.missing_error.clear();
            self.path_prompt = false;
            self.path_draft.clear();
        }
        if let Some(previous_repo_id) = previous.clone()
            && previous != next
        {
            self.stop_playback = true;
            self.effects.push(WorkspaceEffect::StopPlayback { previous_repo_id });
        }
        self.active_repo_id = next;
    }

    fn reset_library_category(&mut self) {
        self.library_category = LibraryCategory::All;
    }

}

pub fn sidebar_prefs_path() -> std::path::PathBuf {
    settings::default_path().with_file_name("sidebar.json")
}

/// 把侧栏宽度限制在 Vue 的 220–480，非法值回到 276。
pub fn clamp_sidebar_width(width: f32) -> f32 {
    if !width.is_finite() {
        SIDEBAR_DEFAULT_PX
    } else {
        width.clamp(SIDEBAR_MIN_PX, SIDEBAR_MAX_PX)
    }
}

pub fn parse_sidebar_width(raw: Option<&str>) -> f32 {
    let Some(raw) = raw else {
        return SIDEBAR_DEFAULT_PX;
    };
    raw.parse::<f32>().map(clamp_sidebar_width).unwrap_or(SIDEBAR_DEFAULT_PX)
}

fn repository_uses_local_metadata(repository: &WorkspaceRepository) -> bool {
    repository.capabilities.iter().any(|capability| capability == "localRootPath")
        || (repository.cache_required && repository.cache_status != "unconfigured")
}

fn repository_metadata_accessible(repository: &WorkspaceRepository) -> bool {
    if repository.cache_required {
        repository.cache_status == "ready"
    } else {
        repository.status == "ready"
    }
}

fn can_delete_metadata(repository: &WorkspaceRepository) -> bool {
    if repository_uses_local_metadata(repository) {
        repository_metadata_accessible(repository)
    } else {
        true
    }
}

fn can_delete_folder(repository: &WorkspaceRepository) -> bool {
    repository_uses_local_metadata(repository) && repository_metadata_accessible(repository)
}

#[cfg(test)]
#[path = "workspace_tests.rs"]
mod tests;
