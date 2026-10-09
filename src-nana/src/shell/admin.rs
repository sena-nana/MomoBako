//! 设置、插件、日志、任务和仓库动作的状态。
//!
//! 结构对应 Vue 的 `Settings.vue`、`PluginManagerPanel.vue`、`SourceAuthenticationSettings.vue`、
//! `WorkspaceLogsPanel.vue`、`TaskPopover.vue`、`ExtensionsPanel.vue` 和 `RepositoryActionsPanel.vue`。
//! 消息在 `admin_reduce` 里归约，副作用由 `admin_dispatch` 交给领域服务。

use std::collections::BTreeMap;

use crate::backend::services::repository::{
    ApiDesignSnapshot, CacheSnapshot, PluginConfigSnapshot, PluginHookExecutionRecord, PluginManifest, RepositoryAction,
    RepositorySummary, SystemLogRecord,
};
use crate::backend::services::runtime::ExternalApiConnectionStatus;

use super::player::PlayerMessage;
use super::{ShellMessage, ShellViewModel, WorkspacePanel};

#[path = "admin_support.rs"]
pub(crate) mod support;
#[path = "admin_time.rs"]
mod time;
#[path = "admin_style.rs"]
pub(crate) mod style;
#[path = "admin_icons.rs"]
pub(crate) mod icons;
#[path = "admin_bind.rs"]
pub(crate) mod bind;
#[path = "admin_reduce.rs"]
mod reduce;
#[path = "admin_view.rs"]
mod view;
#[path = "admin_settings_state.rs"]
mod settings_state;
#[path = "admin_settings_view.rs"]
mod settings_view;
#[path = "admin_plugins_state.rs"]
mod plugins_state;
#[path = "admin_plugins_view.rs"]
mod plugins_view;
#[path = "admin_plugin_card.rs"]
mod plugin_card;
#[path = "admin_logs_state.rs"]
mod logs_state;
#[path = "admin_logs_view.rs"]
mod logs_view;
#[path = "admin_actions_view.rs"]
mod actions_view;
#[path = "admin_gap.rs"]
mod tools;
#[path = "tool_native.rs"]
mod tool_native;
#[path = "api_playground.rs"]
pub(crate) mod api;
#[path = "source_auth_page.rs"]
mod source_page;
#[path = "source_provision.rs"]
pub(crate) mod source_provision;

pub(crate) use actions_view::{actions_panel, ActionsSignals, ActionsView};
pub(crate) use logs_state::{LogsSignals, LogsView};
pub(crate) use logs_view::logs_panel;
pub(crate) use plugins_state::{PluginPanelSignals, PluginPanelView};
pub(crate) use plugins_view::delete_dialog as plugin_delete_dialog;
pub(crate) use reduce::{open_settings_page, reduce_message};
pub(crate) use settings_state::{SettingsSignals, SettingsView};
pub(crate) use settings_view::settings_page;
pub(crate) use source_provision::SourceAuthState;
pub use source_provision::SourceStep;
pub(crate) use tools::{extensions_page, ToolsSignals};
pub(crate) use view::task_popover;

#[cfg(test)]
#[path = "admin_tests.rs"]
mod tests;

/// 插件配置、安装、日志筛选、任务弹层、来源登录和仓库动作消息。
#[derive(Clone, Debug)]
pub enum AdminMessage {
    SetKeyword(String),
    SetEnabled { plugin_id: String, enabled: bool },
    RequestDelete(String),
    CancelDelete,
    ConfirmDelete,
    ToggleSettings(String),
    RoutePlugin(String),
    /// 选项、勾选或回车提交的字段值。
    ConfigInput { plugin_id: String, key: String, text: String, checked: Option<bool> },
    /// 单行字段的草稿，回车才提交。
    FieldDraft { plugin_id: String, key: String, value: String },
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
    /// 设置页一次读完的五份数据。任何一份失败，整批都不写入，和 Vue 的 `Promise.all` 一致。
    SettingsBundleLoaded {
        plugins: Result<Vec<PluginManifest>, String>,
        hooks: Result<Vec<PluginHookExecutionRecord>, String>,
        cache: Result<CacheSnapshot, String>,
        api: Result<ApiDesignSnapshot, String>,
        external: Result<ExternalApiConnectionStatus, String>,
    },
    SetCornerStyle(String),
    SetCornerRadius(String),
    CopyExternal { label: String, value: String },
    ExportExternal,
    CompleteExport(Option<String>),
    WriteFinished(Result<(), String>),
    SetAudioPlayer(Option<String>),
    ToggleLogLevel(String),
    ToggleLogKind(String),
    SetLogPlugin(String),
    SetLogRepo(String),
    SetLogSearch(String),
    ResetLogFilters,
    SetLogPaused(bool),
    /// 展开或收起一条日志的上下文。
    ToggleLogContext(String),
    ToggleTaskPopover,
    /// 弹层头部的关闭按钮和弹层外的点击层。Escape 走全局的 [`crate::shell::escape_layer`]。
    CloseTaskPopover,
    SetOperation(Option<OperationProgress>),
    SelectAction(String),
    RunAction(Option<String>),
    ActionsLoaded { repo_id: String, result: Result<Vec<RepositoryAction>, String> },
    ActionRunFinished { result: Result<RepositoryAction, String> },
    SetToolPages(Vec<ToolPageEntry>),
    SelectToolPage(String),
    /// API Playground 上的输入和请求。
    Api(api::ApiMessage),
    /// 文件右键里能直接调用的插件动作。
    CallFilePlugin {
        plugin_id: String,
        method: String,
        payload: serde_json::Value,
        repository_id: Option<String>,
    },
    /// 文件插件动作结束。成功写到文件活动，失败写到文件错误。
    FilePluginFinished { method: String, result: Result<serde_json::Value, String> },
    /// 来源登录：连接新账号或重新登录。`repo_id` 为空是连接新账号。
    BeginSourceAuth { plugin_id: String, repo_id: Option<String> },
    /// 来源登录：查询仓库登录状态。
    CheckSourceAuth { plugin_id: String, repo_id: String },
    /// 来源登录：退出仓库账号，仓库和缓存保留。
    ClearSourceAuth { plugin_id: String, repo_id: String },
    /// 来源登录：取消扫码会话。
    CancelSourceAuth,
    /// 来源登录：检查扫码结果，成功后创建或更新仓库。
    PollSourceAuth { plugin_id: String },
    /// 来源登录：排队选择缓存目录。取消时不改。
    ChooseSourceCache,
    /// 来源缓存目录对话框的结果。
    SetSourceCachePath(String),
    /// 来源登录流程里一步结束。
    SourceStepFinished { step: SourceStep, result: Result<serde_json::Value, String> },
}

/// 插件调用从哪来。登录结果和文件菜单结果不能混在一起。
#[derive(Clone, Debug, PartialEq)]
pub enum PluginCallOrigin {
    /// 来源登录流程里的一步。
    SourceAuth(SourceStep),
    FileMenu,
    /// API Playground 的插件调用。
    Playground,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OperationProgress {
    pub label: String,
    pub detail: String,
    pub value: f64,
    pub indeterminate: bool,
    pub updated_at_ms: i64,
}

/// 拓展页左侧的一个工具页。Nana 只能画内置的三个；其它插件的工具页没有原生控件。
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
    /// 读最近 200 条系统日志。每次切到日志面板都读，和 Vue `setActivePanel('logs')` 一致。
    LoadLogs,
    /// 读设置目录里的应用设置（主题、缓存上限、关闭行为）。打开设置页时读一次。
    LoadAppSettings,
    /// 读插件登记的播放器类型。插件列表换新以后读，和 Vue 同步前端插件注册表的时机一致。
    LoadPlaylistPlayers,
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
    /// 主题改动后立即写设置文件。
    SaveSettings,
    CopyText(String),
    RequestOpenDialog,
    RequestSaveDialog { content: String },
    WriteFile { path: String, bytes: Vec<u8> },
    /// 调用插件方法。来源登录可带仓库；文件动作带上当前仓库。
    CallPlugin {
        plugin_id: String,
        method: String,
        payload: serde_json::Value,
        repository_id: Option<String>,
        origin: PluginCallOrigin,
    },
    /// 来源登录成功后更新已有仓库的后端配置。
    UpdateBackendConfig { repo_id: String, backend_config: serde_json::Value, step: SourceStep },
    /// 需要本地缓存的来源仓库写入缓存目录，并迁移旧缓存。
    ConfigureSourceCache { repo_id: String, path: String, step: SourceStep },
    /// 跳过首次同步创建来源仓库。
    CreateSourceRepository {
        repo_id: String,
        name: String,
        path: String,
        backend_plugin_id: String,
        backend_config: serde_json::Value,
    },
    /// 后台同步来源仓库，完成后刷新仓库列表。
    SyncRepository { repo_id: String },
    /// API Playground 发出的外部 HTTP 请求。
    HttpRequest(api::HttpRequest),
}

#[derive(Clone, Debug)]
pub struct AdminState {
    pub plugins: Vec<PluginManifest>,
    pub keyword: String,
    pub pending_delete: Option<String>,
    pub active_settings_plugin_id: Option<String>,
    pub config_snapshots: BTreeMap<String, PluginConfigSnapshot>,
    pub json_drafts: BTreeMap<String, BTreeMap<String, String>>,
    /// 单行字段的未提交草稿。快照更新后清掉。
    pub field_drafts: BTreeMap<String, BTreeMap<String, String>>,
    pub action_message: String,
    pub action_error: String,
    /// 设置页数据读取失败的原因，对应 Vue `useWorkspaceProgress().error`。
    pub load_error: String,
    pub hook_executions: Vec<PluginHookExecutionRecord>,
    pub managing: bool,
    pub loading_settings: bool,
    pending_notice: Option<String>,
    pending_failure: Option<String>,
    /// 来源认证页的会话、缓存目录和提示。换插件或收起设置时重置。
    pub source_auth: SourceAuthState,
    /// 最近一次仓库列表的完整摘要。来源认证页要读登录状态和本地缓存路径。
    pub repository_summaries: Vec<RepositorySummary>,
    pub logs: Vec<SystemLogRecord>,
    /// 在读历史日志（Vue `isLoadingLogs`）。
    pub logs_loading: bool,
    pub log_levels: Vec<String>,
    pub log_kinds: Vec<String>,
    pub log_plugin_id: String,
    pub log_repo_id: String,
    pub log_search: String,
    pub log_paused: bool,
    pub log_would_scroll: bool,
    log_signature: String,
    /// 展开了上下文的日志。
    pub log_context_open: std::collections::BTreeSet<String>,
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
    pub api: api::ApiState,
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
            field_drafts: BTreeMap::new(),
            action_message: String::new(),
            action_error: String::new(),
            load_error: String::new(),
            hook_executions: Vec::new(),
            managing: false,
            loading_settings: false,
            pending_notice: None,
            pending_failure: None,
            source_auth: SourceAuthState::default(),
            repository_summaries: Vec::new(),
            logs: Vec::new(),
            logs_loading: false,
            log_levels: Vec::new(),
            log_kinds: Vec::new(),
            log_plugin_id: String::new(),
            log_repo_id: String::new(),
            log_search: String::new(),
            log_paused: false,
            log_would_scroll: false,
            log_signature: String::new(),
            log_context_open: std::collections::BTreeSet::new(),
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
            api: api::ApiState::default(),
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

    /// 排一个副作用。只给本模块和来源认证流程用。
    pub(crate) fn push_effect(&mut self, effect: AdminEffect) {
        self.effects.push(effect);
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

    /// 写失败文案。Vue 先用全局错误，空了才用这一步的兜底文案。
    fn apply_failure(&mut self, error: &str) {
        self.pending_notice = None;
        let fallback = self.pending_failure.take().unwrap_or_default();
        self.action_error = if error.is_empty() { fallback } else { error.to_string() };
    }

    pub fn note_backends(&mut self, repositories: &[RepositorySummary]) {
        self.backends = support::backend_counts(repositories);
        self.repository_summaries = repositories.to_vec();
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

    /// 换上新的插件列表，并按清单里的工具页声明重排拓展页工具。
    fn store_plugins(&mut self, plugins: Vec<PluginManifest>) {
        self.plugins = plugins;
        let pages = support::tool_pages_from_plugins(&self.plugins);
        self.active_tool_page_id = support::apply_tool_page_selection(&pages, self.active_tool_page_id.as_deref());
        self.tool_pages = pages;
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
        self.field_drafts.remove(plugin_id);
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
        sort_logs(&mut self.logs);
        self.logs.truncate(500);
        self.note_log_scroll();
    }

    /// 切到日志面板：读最近 200 条历史日志（Vue `loadSystemLogsInWorkspace`）。
    pub(super) fn begin_logs_load(&mut self) {
        self.logs_loading = true;
        self.effects.push(AdminEffect::LoadLogs);
    }

    /// 历史日志读回：整份换掉，照 Vue `sortSystemLogs` 按时间再 id 降序。
    pub(super) fn replace_logs(&mut self, mut records: Vec<SystemLogRecord>) {
        sort_logs(&mut records);
        self.logs = records;
        self.note_log_scroll();
    }

    pub fn filtered_logs(&self) -> Vec<SystemLogRecord> {
        support::filtered_logs(&self.logs, &self.log_levels, &self.log_kinds, &self.log_plugin_id, &self.log_repo_id, &self.log_search)
    }

    pub fn grouped_plugins(&self) -> Vec<(String, Vec<String>)> {
        support::grouped_plugin_ids(&self.plugins, &self.keyword, &self.hook_executions)
    }
}

/// 日志按时间再 id 降序，和 Vue `sortSystemLogs` 一致。
fn sort_logs(records: &mut [SystemLogRecord]) {
    records.sort_by(|left, right| right.timestamp.cmp(&left.timestamp).then_with(|| right.id.cmp(&left.id)));
}

impl ShellViewModel {
    /// 日志面板在追踪模式下让日志列表跟随末尾：新记录进来滚到底，暂停后停在原位。
    /// Vue 的跟随滚的是不定高的日志列表本身，列表不会出现滚动，实际没有效果；这里给列表定高再跟随。
    pub(super) fn admin_logs_follow_end(&self) -> bool {
        self.admin_workspace_visible(WorkspacePanel::Logs) && self.admin.log_would_scroll && !self.admin.log_paused
    }

    /// 工作台面板要求启动完成且主区有仓库。
    pub(super) fn admin_workspace_visible(&self, panel: WorkspacePanel) -> bool {
        self.workspace.startup.status == super::StartupStatus::Ready
            && self.workspace.main_region() == super::MainRegion::HasRepository
            && self.workspace.panel == panel
    }

    /// 任务弹层的行：仓库操作加运行中的任务，按更新时间降序。侧栏「任务」的计数也是这些行的个数，
    /// 和 Vue `TaskPopover.vue` 的 `activeTaskCount` 一样。
    pub(crate) fn task_rows(&self) -> Vec<support::PopoverRow> {
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
