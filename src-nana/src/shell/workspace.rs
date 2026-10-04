//! 工作台状态机。
//!
//! 对应 Vue `AppShell.vue`、`workspace/lifecycle.ts`、`workspace/state.ts`、
//! `MissingRepositoryState.vue`、`EmptyRepositoryState.vue` 和缺失仓库动作。
//! 启动步骤、主区、侧栏和删除对话框都在这里归约，页面控件只消费结果。

use std::{fs, path::Path};

use crate::backend::services::repository::RepositorySummary;
use crate::settings;
use crate::theme_map::{SIDEBAR_DEFAULT_PX, SIDEBAR_MAX_PX, SIDEBAR_MIN_PX};

/// 与 Vue `momobako.sidebarCollapsed` 相同的存储键。
pub const SIDEBAR_COLLAPSED_KEY: &str = "momobako.sidebarCollapsed";
/// 与 Vue `momobako.sidebarWidth` 相同的存储键。
pub const SIDEBAR_WIDTH_KEY: &str = "momobako.sidebarWidth";
const STARTUP_TOTAL_STEPS: u8 = 4;
const STARTUP_LOG_LIMIT: usize = 32;
const STARTUP_VISIBLE_LOGS: usize = 8;

const STARTUP_STEP_HINTS: [(&str, &str); 4] = [
    ("准备资源库", "读取仓库列表或切换目标资源库。"),
    ("同步文件变化", "扫描新增、移动、删除和缓存状态。"),
    ("读取资源索引", "整理摘要、素材索引和默认预览对象。"),
    ("加载首屏内容", "准备目录、播放列表和首屏辅助数据。"),
];

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

/// 启动流程状态。失败时保留当前步骤，已完成步骤不退回。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum StartupStatus {
    #[default]
    Idle,
    Loading,
    Ready,
    Error,
}

/// 单个启动步骤在步骤条上的状态。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StartupStepState {
    Pending,
    Current,
    Done,
    Error,
}

/// 启动步骤的可见文案。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StartupStepItem {
    pub number: u8,
    pub label: &'static str,
    pub detail: &'static str,
    pub state: StartupStepState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StartupLog {
    pub level: &'static str,
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StartupState {
    pub status: StartupStatus,
    pub step_label: String,
    pub step_detail: String,
    pub current_step: u8,
    pub total_steps: u8,
    pub percent: u8,
    pub error: Option<String>,
    pub logs: Vec<StartupLog>,
}

impl Default for StartupState {
    fn default() -> Self {
        Self {
            status: StartupStatus::Idle,
            step_label: "准备加载仓库".into(),
            step_detail: "准备资源库状态，恢复上次打开的工作区。".into(),
            current_step: 0,
            total_steps: STARTUP_TOTAL_STEPS,
            percent: 0,
            error: None,
            logs: Vec::new(),
        }
    }
}

impl StartupState {
    /// 重新开始一轮启动。已有日志和错误随这轮一起清空。
    pub fn begin(&mut self) {
        *self = Self {
            status: StartupStatus::Loading,
            ..Self::default()
        };
    }

    /// 进入指定步骤。百分比按当前步除以总步数四舍五入，和 Vue 一致。
    pub fn set_progress(&mut self, step: u8, label: impl Into<String>, detail: impl Into<String>) {
        let step = step.clamp(1, STARTUP_TOTAL_STEPS);
        let label = label.into();
        self.status = StartupStatus::Loading;
        self.step_label = label.clone();
        self.step_detail = detail.into();
        self.current_step = step;
        self.total_steps = STARTUP_TOTAL_STEPS;
        self.percent = u8::try_from((u16::from(step) * 100) / u16::from(STARTUP_TOTAL_STEPS)).unwrap_or(100);
        self.error = None;
        self.push_log("info", label);
    }

    /// 停在当前步骤。更早的步骤保持完成，当前步骤变为失败。
    pub fn fail(&mut self, message: impl Into<String>) {
        let message = message.into();
        self.status = StartupStatus::Error;
        self.step_label = "加载失败".into();
        self.step_detail = "资源库加载流程已停止，保留当前错误供重试。".into();
        self.error = Some(message.clone());
        self.push_log("error", message);
    }

    pub fn finish(&mut self) {
        self.status = StartupStatus::Ready;
        self.step_label = "加载完成".into();
        self.step_detail = "工作区首屏已经准备完成。".into();
        self.current_step = STARTUP_TOTAL_STEPS;
        self.total_steps = STARTUP_TOTAL_STEPS;
        self.percent = 100;
        self.error = None;
    }

    /// 步骤条规则与 `AppShell.vue` 的 `startupStepItems` 相同。
    pub fn step_items(&self) -> [StartupStepItem; 4] {
        std::array::from_fn(|index| {
            let number = u8::try_from(index + 1).unwrap_or(1);
            let (label, detail) = STARTUP_STEP_HINTS[index];
            StartupStepItem {
                number,
                label,
                detail,
                state: self.step_state(number),
            }
        })
    }

    pub fn step_state(&self, step_number: u8) -> StartupStepState {
        let is_current = self.current_step == step_number;
        let is_done = self.current_step > step_number || self.status == StartupStatus::Ready;
        let is_error = is_current && self.status == StartupStatus::Error;
        if is_error {
            StartupStepState::Error
        } else if is_done {
            StartupStepState::Done
        } else if is_current {
            StartupStepState::Current
        } else {
            StartupStepState::Pending
        }
    }

    pub fn visible_logs(&self) -> Vec<&StartupLog> {
        self.logs.iter().rev().take(STARTUP_VISIBLE_LOGS).collect()
    }

    fn push_log(&mut self, level: &'static str, message: impl Into<String>) {
        self.logs.push(StartupLog { level, message: message.into() });
        if self.logs.len() > STARTUP_LOG_LIMIT {
            let extra = self.logs.len() - STARTUP_LOG_LIMIT;
            self.logs.drain(0..extra);
        }
    }
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

/// 宿主在归约后执行的副作用。过期的列表和同步带有启动代次。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspaceEffect {
    RefreshRepositories { generation: u64 },
    SyncRepository { repo_id: String, generation: u64 },
    LoadSnapshot { repo_id: String },
    RelocateRepository { repo_id: String, path: String },
    DeleteRepository { repo_id: String, mode: DeleteMode },
    OpenSourceSettings,
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
            self.startup.begin();
            self.startup.set_progress(
                1,
                "加载仓库列表",
                "读取已注册资源库，并匹配上次打开的工作区。",
            );
        }
        self.list_generation
    }

    /// 失败后重试。已就绪时不再重入，避免把当前工作区打回加载页。
    pub fn retry_startup(&mut self) -> bool {
        if self.startup.status == StartupStatus::Ready {
            return false;
        }
        self.list_generation = self.list_generation.saturating_add(1);
        self.startup.begin();
        self.startup.set_progress(
            1,
            "加载仓库列表",
            "读取已注册资源库，并匹配上次打开的工作区。",
        );
        self.effects.push(WorkspaceEffect::RefreshRepositories { generation: self.list_generation });
        true
    }

    pub fn request_repository_refresh(&mut self) {
        self.list_generation = self.list_generation.saturating_add(1);
        self.effects.push(WorkspaceEffect::RefreshRepositories { generation: self.list_generation });
    }

    /// 应用一轮仓库列表。`generation` 不匹配时保留当前步骤和已选仓库。
    pub fn apply_repository_list(&mut self, generation: Option<u64>, result: Result<Vec<WorkspaceRepository>, String>) {
        if let Some(generation) = generation
            && generation != self.list_generation
        {
            eprintln!("Nana 忽略过期的资源库列表结果：代次 {generation}，当前 {}", self.list_generation);
            return;
        }
        let startup_open = self.startup.status != StartupStatus::Ready;
        match result {
            Err(error) => {
                eprintln!("Nana 读取资源库列表失败：{error}");
                if startup_open {
                    if self.startup.status != StartupStatus::Loading {
                        self.startup.begin();
                    }
                    self.startup.fail(error);
                    self.presence = RepositoryPresence::LoadError;
                } else {
                    self.missing_error = error;
                }
            }
            Ok(items) => self.apply_repository_items(items, startup_open),
        }
    }

    pub fn note_sync_finished(&mut self, generation: u64, result: Result<(), String>) {
        if generation != self.list_generation || self.startup.status != StartupStatus::Loading {
            eprintln!("Nana 忽略过期的资源库同步结果：代次 {generation}");
            return;
        }
        match result {
            Ok(()) => {
                if let Some(repo_id) = self.active_repo_id.clone() {
                    self.last_active_repo_id = Some(repo_id.clone());
                    self.startup.set_progress(
                        3,
                        "读取仓库摘要",
                        "读取资源库摘要、素材索引和默认预览对象。",
                    );
                    self.effects.push(WorkspaceEffect::LoadSnapshot { repo_id });
                } else {
                    self.startup.fail("同步完成时没有活动资源库");
                    self.presence = RepositoryPresence::LoadError;
                }
            }
            Err(error) => {
                eprintln!("Nana 同步资源库失败：{error}");
                self.startup.fail(error);
                self.presence = RepositoryPresence::LoadError;
            }
        }
    }

    pub fn note_index_finished(&mut self, repo_id: &str, result: Result<(), String>) {
        if self.startup.status != StartupStatus::Loading || self.startup.current_step < 3 {
            return;
        }
        if self.active_repo_id.as_deref() != Some(repo_id) {
            eprintln!("Nana 忽略过期的资源库摘要：{repo_id}");
            return;
        }
        match result {
            Ok(()) => self.startup.set_progress(
                4,
                "读取首屏目录",
                "加载根目录、播放列表和首屏关联数据。",
            ),
            Err(error) => {
                eprintln!("Nana 读取资源库摘要失败：{error}");
                self.startup.fail(error);
                self.presence = RepositoryPresence::LoadError;
            }
        }
    }

    pub fn note_first_screen_finished(&mut self, result: Result<(), String>) {
        if self.startup.status != StartupStatus::Loading || self.startup.current_step < 4 {
            return;
        }
        match result {
            Ok(()) => {
                self.startup.finish();
                self.presence = RepositoryPresence::Ready;
            }
            Err(error) => {
                eprintln!("Nana 读取首屏目录失败：{error}");
                self.startup.fail(error);
                self.presence = RepositoryPresence::LoadError;
            }
        }
    }

    /// 切换仓库。只有上一个 id 存在且不同时才停止播放，和 `watch(activeRepoId)` 一致。
    pub fn select_repository(&mut self, repo_id: &str) {
        let Some(repository) = self.repositories.iter().find(|item| item.repo_id == repo_id).cloned() else {
            return;
        };
        if self.startup.status == StartupStatus::Loading {
            // 让已经发出的同步或列表结果失效，避免写到新的活动仓库上。
            self.list_generation = self.list_generation.saturating_add(1);
        }
        self.assign_active(Some(repository.repo_id.clone()));
        if repository.status == "missing" {
            self.presence = RepositoryPresence::Missing;
            self.reset_library_category();
            self.last_active_repo_id = Some(repository.repo_id);
            if self.startup.status == StartupStatus::Loading {
                self.startup.finish();
            }
            return;
        }
        self.presence = RepositoryPresence::Ready;
        if self.startup.status == StartupStatus::Loading {
            self.startup.set_progress(2, "扫描资源库文件", "同步文件变化，更新新增、移动和删除记录。");
            self.effects.push(WorkspaceEffect::SyncRepository {
                repo_id: repository.repo_id,
                generation: self.list_generation,
            });
        } else if self.startup.status == StartupStatus::Ready {
            self.last_active_repo_id = Some(repository.repo_id.clone());
            self.effects.push(WorkspaceEffect::LoadSnapshot { repo_id: repository.repo_id });
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
        self.effects.push(WorkspaceEffect::RelocateRepository { repo_id, path });
    }

    pub fn note_relocate_finished(&mut self, result: Result<(), String>) {
        self.relocating = false;
        match result {
            Ok(()) => {
                self.missing_error.clear();
                self.path_draft.clear();
                self.request_repository_refresh();
            }
            Err(error) => {
                eprintln!("Nana 重定向资源库失败：{error}");
                self.missing_error = error;
            }
        }
    }

    pub fn open_source_settings(&mut self) {
        if self.missing_busy() {
            return;
        }
        if self.active_repository().is_some_and(|repository| repository.is_source_cache_issue()) {
            self.missing_error.clear();
            self.effects.push(WorkspaceEffect::OpenSourceSettings);
        }
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

    /// 选择顺序：当前 id、上次记住的 id、列表第一项。空列表清空记住的仓库。
    fn apply_repository_items(&mut self, items: Vec<WorkspaceRepository>, startup_open: bool) {
        self.repositories = items;
        if self.repositories.is_empty() {
            self.assign_active(None);
            self.last_active_repo_id = None;
            self.reset_library_category();
            self.presence = RepositoryPresence::Empty;
            if startup_open {
                self.startup.finish();
            }
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
        let missing = self
            .repositories
            .iter()
            .find(|item| item.repo_id == next_id)
            .is_some_and(|item| item.status == "missing");
        self.assign_active(Some(next_id.clone()));
        if missing {
            self.last_active_repo_id = Some(next_id);
            self.reset_library_category();
            self.presence = RepositoryPresence::Missing;
            if startup_open {
                self.startup.finish();
            }
            return;
        }
        self.presence = RepositoryPresence::Ready;
        if startup_open {
            self.startup.set_progress(2, "扫描资源库文件", "同步文件变化，更新新增、移动和删除记录。");
            self.effects.push(WorkspaceEffect::SyncRepository {
                repo_id: next_id,
                generation: self.list_generation,
            });
        } else {
            self.last_active_repo_id = Some(next_id.clone());
            self.effects.push(WorkspaceEffect::LoadSnapshot { repo_id: next_id });
        }
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

    fn remove_repository(&mut self, repo_id: &str) {
        self.repositories.retain(|repository| repository.repo_id != repo_id);
        if self.active_repo_id.as_deref() != Some(repo_id) {
            return;
        }
        let next = self.repositories.first().map(|repository| repository.repo_id.clone());
        self.assign_active(next.clone());
        match next {
            Some(next_id) => {
                let missing = self.repositories.iter().any(|repository| repository.repo_id == next_id && repository.status == "missing");
                self.presence = if missing { RepositoryPresence::Missing } else { RepositoryPresence::Ready };
                self.last_active_repo_id = Some(next_id.clone());
                if !missing {
                    self.effects.push(WorkspaceEffect::LoadSnapshot { repo_id: next_id });
                }
            }
            None => {
                self.presence = RepositoryPresence::Empty;
                self.last_active_repo_id = None;
                self.reset_library_category();
            }
        }
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
