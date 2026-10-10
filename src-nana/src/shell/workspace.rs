//! 工作台状态机。
//!
//! 对应 Vue `AppShell.vue`、`workspace/lifecycle.ts`、`workspace/state.ts`、
//! `MissingRepositoryState.vue`、`EmptyRepositoryState.vue` 和缺失仓库动作。
//! 这里是工作台的状态、主区和侧栏偏好，页面控件只消费结果。按流程分在子模块里：首屏启动在
//! `workspace_launch.rs`（步骤条和加载日志的数据在 `workspace_startup.rs`），换仓库和同一个仓库的刷新在
//! `workspace_switch.rs`，缺失仓库的动作和删除对话框在 `workspace_missing.rs`，记住上次打开的资源库在
//! `workspace_remember.rs`。

use std::{fs, path::Path};

use crate::backend::services::repository::RepositorySummary;
use crate::settings;
use crate::theme_map::{SIDEBAR_DEFAULT_PX, SIDEBAR_MAX_PX, SIDEBAR_MIN_PX};

use super::repo_operation::OperationRequests;

/// 与 Vue `momobako.sidebarCollapsed` 相同的存储键。
pub const SIDEBAR_COLLAPSED_KEY: &str = "momobako.sidebarCollapsed";
/// 与 Vue `momobako.sidebarWidth` 相同的存储键。
pub const SIDEBAR_WIDTH_KEY: &str = "momobako.sidebarWidth";

#[path = "workspace_startup.rs"]
mod startup;
pub use startup::{
    IndexCounts, SettingsWait, StartupLog, StartupMode, StartupState, StartupStatus, StartupStepItem, StartupStepState, SyncCounts,
};
#[path = "workspace_switch.rs"]
mod switch;
pub use switch::{RefreshRun, RefreshStage};
#[path = "workspace_remember.rs"]
mod remember;
pub use remember::last_repository_path;
#[path = "workspace_launch.rs"]
mod launch;
#[path = "workspace_missing.rs"]
mod missing;

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
    /// 来源本地缓存目录（`localCache.path`），来源条目动作的 `managedCacheRoot`。
    pub cache_path: Option<String>,
    /// 来源登录过期（`authentication.loginExpired`）。
    pub login_expired: bool,
    /// 来源没有登录（`authentication.loggedIn` 为假）。
    pub logged_out: bool,
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
            cache_path: summary.local_cache.as_ref().and_then(|cache| cache.path.clone()),
            login_expired: summary.authentication.as_ref().is_some_and(|auth| auth.login_expired),
            logged_out: summary.authentication.as_ref().is_some_and(|auth| !auth.logged_in),
        }
    }

    /// 来源缓存未就绪时，主操作是打开插件设置，而不是重定向本地文件夹。
    pub fn is_source_cache_issue(&self) -> bool {
        self.cache_required && self.cache_status != "ready"
    }

    /// 来源认证或本地缓存不可用（Vue `actionDisabled` 的仓库部分）：登录过期、没有登录，或者要本地缓存但缓存
    /// 没就绪。这时来源条目动作禁用。
    pub fn source_unavailable(&self) -> bool {
        self.login_expired || self.logged_out || self.is_source_cache_issue()
    }

    /// 能当下载目标：就绪，而且可写（Vue 目标资源库对话框的 `requireReady`、`requireWritable`）。
    pub fn accepts_downloads(&self) -> bool {
        self.status == "ready" && self.capabilities.iter().any(|capability| capability == "write")
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
    /// 记住的资源库变了，写进 `last-repository.json`。
    PersistLastRepository,
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

    /// 刷新资源库，读完列表照 Vue 选当前仓库（不在列表里选第一项），见 [`Self::refresh_repositories`]。
    pub fn request_repository_refresh(&mut self) {
        self.refresh_repositories(None);
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

#[cfg(test)]
#[path = "workspace_tests.rs"]
mod tests;
