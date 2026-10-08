//! 设置、插件、日志、任务和仓库动作的状态。
//!
//! 插件、日志和设置消息在这里归约。验收页和产品窗口共用同一套表面。

use std::collections::{BTreeMap, BTreeSet};

use crate::backend::services::repository::{
    ApiDesignSnapshot, CacheSnapshot, PluginConfigSnapshot, PluginHookExecutionRecord, PluginManifest, RepositoryAction,
    RepositorySummary, SystemLogRecord,
};
use crate::backend::services::runtime::ExternalApiConnectionStatus;

use super::player::PlayerMessage;
use super::{ShellMessage, ShellPage, ShellViewModel, WorkspacePanel};

#[path = "admin_support.rs"]
pub(super) mod support;
#[path = "admin_reduce.rs"]
mod reduce;
#[path = "tool_native.rs"]
mod tool_native;
#[path = "admin_view.rs"]
mod view;
#[path = "admin_gap.rs"]
mod gap;
#[path = "office_settings.rs"]
mod office;
#[path = "source_auth_page.rs"]
mod source_page;
#[path = "source_provision.rs"]
mod source_provision;

pub(crate) use gap::opened_plugin_pages;
pub(crate) use reduce::reduce_message;
pub(crate) use view::{admin_surface, task_popover};

#[cfg(test)]
#[path = "admin_tests.rs"]
mod tests;

/// 来源认证上的一次调用。方法名从认证声明里读，按钮不写死方法。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceAuthCall {
    CreateSession,
    Status,
    Clear,
    /// 轮询扫码结果。方法名仍从 `pollSessionMethod` 读取。
    Poll,
}

/// Office 转换设置页上的一次调用。没有快照时不编造运行状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OfficeAction {
    Status,
    SelfCheck,
    ClearCache,
    StopDaemon,
}

/// 插件配置、安装、日志筛选、任务弹层和仓库动作消息。
#[derive(Clone, Debug)]
pub enum AdminMessage {
    SetKeyword(String),
    SetEnabled { plugin_id: String, enabled: bool },
    RequestDelete(String),
    CancelDelete,
    ConfirmDelete,
    ToggleSettings(String),
    RoutePlugin(String),
    ConfigInput { plugin_id: String, key: String, text: String, checked: Option<bool> },
    JsonDraft { plugin_id: String, key: String, value: String },
    SaveJson { plugin_id: String, key: String },
    ResetConfig { plugin_id: String, key: String },
    ChooseArchive,
    RefreshPlugins,
    InstallArchive(Option<String>),
    PluginsReplaced(Result<Vec<PluginManifest>, String>),
    OpenDataDirectory(String),
    DataDirectoryFinished { name: String, result: Result<String, String> },
    HooksLoaded(Result<Vec<PluginHookExecutionRecord>, String>),
    CacheLoaded(Result<CacheSnapshot, String>),
    ApiDesignLoaded(Result<ApiDesignSnapshot, String>),
    SettingsBundleLoaded {
        plugins: Result<Vec<PluginManifest>, String>,
        hooks: Result<Vec<PluginHookExecutionRecord>, String>,
        cache: Result<CacheSnapshot, String>,
        api: Result<ApiDesignSnapshot, String>,
    },
    SetCornerStyle(String),
    SetCornerRadius(String),
    CopyExternal { label: String, value: String },
    ExportExternal,
    CompleteExport(Option<String>),
    WriteFinished(Result<(), String>),
    SelectRepository(String),
    SetAudioPlayer(Option<String>),
    MarkVueSettings(String),
    ToggleLogLevel(String),
    ToggleLogKind(String),
    SetLogPlugin(String),
    SetLogRepo(String),
    SetLogSearch(String),
    ResetLogFilters,
    SetLogPaused(bool),
    ToggleTaskPopover,
    CloseTaskPopover,
    TaskEscape,
    TaskOutside { inside: bool },
    TaskUnmount,
    SetOperation(Option<OperationProgress>),
    SelectAction(String),
    RunAction(Option<String>),
    ActionsLoaded { repo_id: String, result: Result<Vec<RepositoryAction>, String> },
    ActionRunFinished { result: Result<RepositoryAction, String> },
    SetToolPages(Vec<ToolPageEntry>),
    SelectToolPage(String),
    /// 按认证声明调用登录方法。没有插件或方法名时不派发。
    CallSourceAuth { plugin_id: String, slot: SourceAuthCall },
    /// 插件调用结束。成功文案只附加字符串 message 和 status。
    SourceAuthFinished { method: String, result: Result<serde_json::Value, String> },
    /// 文件右键里能直接调用的插件动作。
    CallFilePlugin {
        plugin_id: String,
        method: String,
        payload: serde_json::Value,
        repository_id: Option<String>,
    },
    /// 文件插件动作结束。成功写到文件活动，失败写到文件错误。
    FilePluginFinished { method: String, result: Result<serde_json::Value, String> },
    /// 刷新下载服务状态。没有快照时页面只说明缺的字段。
    RefreshDownloader,
    /// 下载服务状态返回。失败不编造任务数。
    DownloaderStatusFinished { result: Result<serde_json::Value, String> },
    /// 清掉本页的扫码会话，不调用插件的退出方法。
    DismissSourceAuth,
    /// 来源认证页上的缓存目录草稿。
    SetSourceCachePath(String),
    /// 排队选择来源缓存目录。取消时不改草稿。
    ChooseSourceCache,
    /// 建仓名称草稿。
    SetSourceRepoName(String),
    /// 建仓路径草稿。
    SetSourceRepoPath(String),
    /// 登录结果里有插件、账号配置、名称和路径才创建仓库。
    SubmitSourceRepository,
    /// 来源建仓协议返回。成功文案来自宿主，失败是真实错误。
    SourceRepositoryFinished { result: Result<String, String> },
    /// Office 转换设置页的刷新、自检、清缓存或停守护进程。
    RunOffice(OfficeAction),
    /// Office 插件调用结束。状态快照只在刷新成功时替换。
    OfficeFinished { method: String, result: Result<serde_json::Value, String> },
    /// 选择要清理预览缓存的资源库。
    SelectOfficeRepository(String),
}

/// 插件调用从哪来。登录结果和文件菜单结果不能混在一起。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PluginCallOrigin {
    SourceAuth,
    FileMenu,
    /// 下载服务设置页读取 `downloader.getRuntimeStatus`。
    Downloader,
    /// Office 转换设置页。
    Office,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OperationProgress {
    pub label: String,
    pub detail: String,
    pub value: f64,
    pub indeterminate: bool,
    pub updated_at_ms: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToolPageEntry {
    pub id: String,
    pub label: String,
    pub plugin_name: String,
    pub description: String,
    pub native: bool,
}

#[derive(Clone, Debug)]
pub enum AdminEffect {
    LoadSettingsBundle,
    Install(String),
    DeletePlugin(String),
    SetEnabled { plugin_id: String, enabled: bool },
    SetConfig { plugin_id: String, key: String, value: serde_json::Value },
    DeleteConfig { plugin_id: String, key: String },
    LoadConfig(String),
    OpenDataDirectory { plugin_id: String, name: String },
    LoadActions { repo_id: String },
    RunAction { repo_id: String, action_id: String, paths: Vec<String> },
    ReloadBrowser { repo_id: String },
    PersistCorners,
    CopyText(String),
    RequestOpenDialog,
    RequestSaveDialog { content: String },
    WriteFile { path: String, bytes: Vec<u8> },
    /// 用来源登录结果创建仓库。配置必须来自登录返回，不能现编。
    CreateSourceRepository {
        repo_id: String,
        name: String,
        path: String,
        backend_plugin_id: String,
        backend_config: serde_json::Value,
    },
    /// 调用插件方法。来源登录不带仓库；文件动作带上当前仓库。
    CallPlugin {
        plugin_id: String,
        method: String,
        payload: serde_json::Value,
        repository_id: Option<String>,
        origin: PluginCallOrigin,
    },
}

#[derive(Clone, Debug)]
pub struct AdminState {
    pub plugins: Vec<PluginManifest>,
    pub keyword: String,
    pub pending_delete: Option<String>,
    pub active_settings_plugin_id: Option<String>,
    pub config_snapshots: BTreeMap<String, PluginConfigSnapshot>,
    pub json_drafts: BTreeMap<String, BTreeMap<String, String>>,
    pub action_message: String,
    pub action_error: String,
    pub source_auth: support::SourceAuthView,
    /// 只有 `downloader.getRuntimeStatus` 成功后才有值。
    pub downloader_status: Option<support::DownloaderStatus>,
    pub downloader_error: String,
    pub downloader_loading: bool,
    /// 来源认证页的缓存目录。空字符串表示还没选。
    pub source_cache_path: String,
    pub source_repo_name: String,
    pub source_repo_path: String,
    /// 最近一次来源登录调用的插件。结果返回前先记在这里。
    pub source_auth_plugin_id: String,
    /// 来源建仓协议还没返回。避免重复提交。
    pub source_creating: bool,
    /// 只有 `officeConvert.getRuntimeStatus` 成功后才有值。
    pub office_status: Option<serde_json::Value>,
    pub office_error: String,
    pub office_message: String,
    pub office_pending: Option<OfficeAction>,
    pub office_repo_id: String,
    pub hook_executions: Vec<PluginHookExecutionRecord>,
    pub vue_settings: BTreeSet<String>,
    pub managing: bool,
    pub loading_settings: bool,
    pending_notice: Option<String>,
    pending_failure: Option<String>,
    pub logs: Vec<SystemLogRecord>,
    pub log_levels: Vec<String>,
    pub log_kinds: Vec<String>,
    pub log_plugin_id: String,
    pub log_repo_id: String,
    pub log_search: String,
    pub log_paused: bool,
    pub log_would_scroll: bool,
    log_signature: String,
    pub popover_open: bool,
    pub operation: Option<OperationProgress>,
    pub actions: Vec<RepositoryAction>,
    pub active_action_id: Option<String>,
    pub actions_loading: bool,
    pub actions_running: bool,
    pub actions_error: String,
    actions_repo_id: Option<String>,
    pub tool_pages: Vec<ToolPageEntry>,
    pub active_tool_page_id: Option<String>,
    pub corner_style: String,
    pub corner_radius: f64,
    pub external: Option<ExternalApiConnectionStatus>,
    pub external_message: String,
    pub external_error: String,
    pub cache: Option<CacheSnapshot>,
    pub api_design: Option<ApiDesignSnapshot>,
    pub backends: Vec<support::BackendCount>,
    effects: Vec<AdminEffect>,
}

impl Default for AdminState {
    fn default() -> Self {
        Self {
            plugins: Vec::new(),
            keyword: String::new(),
            pending_delete: None,
            active_settings_plugin_id: None,
            config_snapshots: BTreeMap::new(),
            json_drafts: BTreeMap::new(),
            action_message: String::new(),
            action_error: String::new(),
            source_auth: support::SourceAuthView::default(),
            downloader_status: None,
            downloader_error: String::new(),
            downloader_loading: false,
            source_cache_path: String::new(),
            source_repo_name: String::new(),
            source_repo_path: String::new(),
            source_auth_plugin_id: String::new(),
            source_creating: false,
            office_status: None,
            office_error: String::new(),
            office_message: String::new(),
            office_pending: None,
            office_repo_id: String::new(),
            hook_executions: Vec::new(),
            vue_settings: BTreeSet::new(),
            managing: false,
            loading_settings: false,
            pending_notice: None,
            pending_failure: None,
            logs: Vec::new(),
            log_levels: Vec::new(),
            log_kinds: Vec::new(),
            log_plugin_id: String::new(),
            log_repo_id: String::new(),
            log_search: String::new(),
            log_paused: false,
            log_would_scroll: false,
            log_signature: String::new(),
            popover_open: false,
            operation: None,
            actions: Vec::new(),
            active_action_id: None,
            actions_loading: false,
            actions_running: false,
            actions_error: String::new(),
            actions_repo_id: None,
            tool_pages: Vec::new(),
            active_tool_page_id: None,
            corner_style: support::default_corner_style().into(),
            corner_radius: support::DEFAULT_CORNER_RADIUS,
            external: None,
            external_message: String::new(),
            external_error: String::new(),
            cache: None,
            api_design: None,
            backends: Vec::new(),
            effects: Vec::new(),
        }
    }
}

impl AdminState {
    pub fn take_effects(&mut self) -> Vec<AdminEffect> {
        std::mem::take(&mut self.effects)
    }

    /// 缺文件时用平台默认圆角，不立刻写回。
    pub fn load_default_file(&mut self) {
        let (style, radius) = support::read_corners(&support::corners_path());
        self.corner_style = style;
        self.corner_radius = radius;
    }

    pub fn load_corners_file(&mut self, path: &std::path::Path) {
        let (style, radius) = support::read_corners(path);
        self.corner_style = style;
        self.corner_radius = radius;
    }

    pub(crate) fn save_corners_file(&self) {
        support::write_corners(&support::corners_path(), &self.corner_style, self.corner_radius);
    }

    fn reset_action(&mut self) {
        self.action_message.clear();
        self.action_error.clear();
    }

    fn remember_outcome(&mut self, success: &str, failure: &str) {
        self.pending_notice = Some(success.into());
        self.pending_failure = Some(failure.into());
    }

    fn apply_success(&mut self) {
        if let Some(notice) = self.pending_notice.take() {
            self.action_message = notice;
        }
        self.pending_failure = None;
    }

    fn apply_failure(&mut self, error: &str) {
        self.pending_notice = None;
        let fallback = self.pending_failure.take().unwrap_or_default();
        self.action_error = if error.is_empty() { fallback } else { error.to_string() };
    }

    pub fn note_backends(&mut self, repositories: &[RepositorySummary]) {
        self.backends = support::backend_counts(repositories);
    }

    pub fn queue_actions(&mut self, repo_id: Option<String>) {
        let Some(repo_id) = repo_id.filter(|id| !id.trim().is_empty()) else {
            self.actions.clear();
            self.active_action_id = None;
            self.actions_loading = false;
            self.actions_repo_id = None;
            return;
        };
        self.actions_loading = true;
        self.actions_error.clear();
        self.actions_repo_id = Some(repo_id.clone());
        self.effects.push(AdminEffect::LoadActions { repo_id });
    }

    pub fn begin_settings_load(&mut self) {
        self.loading_settings = true;
        self.effects.push(AdminEffect::LoadSettingsBundle);
    }

    fn store_plugins(&mut self, plugins: Vec<PluginManifest>) {
        self.plugins = plugins;
    }

    fn sync_json_drafts(&mut self, plugin_id: &str) {
        let Some(plugin) = self.plugins.iter().find(|plugin| plugin.plugin_id == plugin_id).cloned() else {
            return;
        };
        let Some(snapshot) = self.config_snapshots.get(plugin_id) else {
            return;
        };
        let mut drafts = self.json_drafts.remove(plugin_id).unwrap_or_default();
        for field in support::settings_fields(&plugin) {
            if field.field_type != "json" {
                continue;
            }
            let value = snapshot.values.get(&field.key).cloned().or(field.default_value);
            drafts.insert(field.key, value.map(|item| support::json_draft_text(&item)).unwrap_or_default());
        }
        self.json_drafts.insert(plugin_id.to_string(), drafts);
    }

    fn note_log_scroll(&mut self) {
        let signature = self
            .filtered_logs()
            .iter()
            .map(|record| record.id.clone())
            .collect::<Vec<_>>()
            .join("|");
        if !signature.is_empty() && signature != self.log_signature {
            self.log_would_scroll = !self.log_paused;
        }
        self.log_signature = signature;
    }

    /// 按 id 合并一条实时日志，时间再 id 降序，最多保留 500 条。
    pub(super) fn merge_log(&mut self, record: SystemLogRecord) {
        if let Some(slot) = self.logs.iter_mut().find(|item| item.id == record.id) {
            *slot = record;
        } else {
            self.logs.push(record);
        }
        self.logs.sort_by(|left, right| {
            right.timestamp.cmp(&left.timestamp).then_with(|| right.id.cmp(&left.id))
        });
        self.logs.truncate(500);
        self.note_log_scroll();
    }

    pub fn filtered_logs(&self) -> Vec<SystemLogRecord> {
        support::filtered_logs(&self.logs, &self.log_levels, &self.log_kinds, &self.log_plugin_id, &self.log_repo_id, &self.log_search)
    }

    pub fn grouped_plugins(&self) -> Vec<(String, Vec<String>)> {
        support::grouped_plugin_ids(&self.plugins, &self.keyword, &self.hook_executions)
    }
}

impl ShellViewModel {
    /// 设置页不要求当前仓库。工作台面板仍要求启动完成且主区有仓库。
    pub(super) fn admin_settings_visible(&self) -> bool {
        matches!(self.page, ShellPage::Settings | ShellPage::SettingsError)
    }

    pub(super) fn admin_workspace_visible(&self, panel: WorkspacePanel) -> bool {
        self.workspace.startup.status == super::StartupStatus::Ready
            && self.workspace.main_region() == super::MainRegion::HasRepository
            && self.workspace.panel == panel
    }

    fn task_rows(&self) -> Vec<support::PopoverRow> {
        support::popover_rows(&self.task_progress, self.admin.operation.as_ref())
    }

    pub(crate) fn set_audio_preference(&mut self, plugin_id: Option<String>) {
        let plugin_id = plugin_id.and_then(|value| {
            let value = value.trim().to_string();
            (!value.is_empty()).then_some(value)
        });
        self.reduce(ShellMessage::Player(PlayerMessage::SetPreference {
            capability_id: super::player::AUDIO_CAPABILITY.into(),
            plugin_id,
        }));
    }
}
