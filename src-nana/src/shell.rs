//! MomoBako 原生应用壳层和页面 ViewModel。
//!
//! 壳层只描述稳定的导航、状态和主内容层级；仓库、插件和任务服务通过
//! `ShellViewModel` 注入文本状态，避免把领域服务直接耦合到 Nana 控件树。

use crate::backend::services::repository::{
    AssetDetail, FileBrowserEntry, FileBrowserSnapshot, FilePreviewSourceResponse, RepositorySnapshot,
    PluginManifest, PlaylistSummary, RepositorySummary, SystemLogPage,
    PluginConfigSnapshot, PlaylistDetail, PlaylistPlayerContribution, TaskProgressSnapshot,
};
use crate::settings::ApplicationSettings;

mod acceptance;
mod workspace_dialogs;
mod files;
mod system_media;
pub(crate) use system_media::{poll as poll_media_keys, sync as sync_media_session};
mod inspect_shortcuts;
mod entry_actions;
mod palette;
mod admin_fields;
mod motion;
mod pointer_gesture;
pub use pointer_gesture::observe_live_pointer;
pub use motion::note_sidebar_resize;
mod thumbs;
mod files_view;
mod inspect;
mod inspect_library;
mod inspect_asmr;
mod inspect_view;
mod inspect_metadata_view;
mod inspect_search_view;
pub(crate) mod audio_decode;
pub(crate) mod player;
pub(crate) mod admin;
pub use admin::{AdminMessage, ToolPageEntry};
pub use acceptance::gap_models as acceptance_gap_models;
pub(crate) mod input;
pub mod host_events;
mod player_view;
mod sidebar;
mod sidebar_view;
mod workspace;
pub(crate) mod workspace_refresh;
pub use files::{display_mode_path, FileRow, FilesEffect, FilesMessage, HardlinkPrompt, VirtualQuery};
pub(crate) use thumbs::{decode_preview_pixels, decode_thumbnail_file, thumbnail_slot, ThumbnailFrame};
pub(crate) use inspect::poll_timers;
pub use inspect::{
    DateBound, InspectEffect, InspectMessage, NumberBound, SearchRequestDraft, SearchRow, prepare_text,
};
pub(crate) use inspect::native_preview::load as load_native_preview;
pub(crate) use inspect::support::{preview_media_parts, MediaParts};
#[cfg(test)]
pub(crate) use inspect::support::preview_media_session;
pub use player::PreviewPcm;
pub(crate) use inspect::NativeLoad;
pub use sidebar::{
    FolderMutation, GapMessage, SidebarEffect, SidebarFolder, SidebarMessage, SidebarPlaylist, SidebarSmartFolder,
    ShortcutId,
};
pub use workspace::{
    sidebar_prefs_path, DeleteMode, LibraryCategory, MainRegion, StartupStatus, WorkspaceEffect,
    WorkspacePanel, WorkspaceRepository, WorkspaceState,
};

/// Nana Runtime 传递给应用状态的壳层交互消息。
pub enum ShellMessage {
    Navigate(ShellPage),
    Refresh,
    PrimaryAction,
    EditAction,
    RepositoriesLoaded(Result<Vec<RepositorySummary>, String>),
    RepositorySnapshotLoaded(Result<RepositorySnapshot, String>),
    FileBrowserLoaded(Result<FileBrowserSnapshot, String>),
    /// 缩略图解码结果。像素由宿主上传，归约只留下原始宽高。
    ThumbnailPixels(Vec<ThumbnailFrame>),
    SelectFile { path: String, asset_id: Option<String> },
    OpenDirectory(String),
    AssetDetailLoaded(Result<AssetDetail, String>),
    PreviewSourceLoaded(Result<FilePreviewSourceResponse, String>),
    PreviewPixelsLoaded {
        source: FilePreviewSourceResponse,
        pixels: Result<PreviewPixels, String>,
    },
    PluginsLoaded(Result<Vec<PluginManifest>, String>),
    SelectPlugin(String),
    TogglePlugin { plugin_id: String, enabled: bool },
    DeletePlugin(String),
    PluginConfigLoaded(Result<PluginConfigSnapshot, String>),
    DeletePluginConfig { plugin_id: String, key: String },
    PluginConfigDraftChanged { key: String, value: String },
    SavePluginConfig { plugin_id: String, key: String },
    LogsLoaded(Result<SystemLogPage, String>),
    ClearLogs,
    PlaylistsLoaded(Result<Vec<PlaylistSummary>, String>),
    PlaylistPlayersLoaded(Result<Vec<PlaylistPlayerContribution>, String>),
    NewPlaylistNameChanged(String),
    SelectPlaylistPlayer(String),
    OpenPlaylistDialog,
    ClosePlaylistDialog,
    CreatePlaylist,
    SelectPlaylist(String),
    DeletePlaylist(String),
    PlaylistDetailLoaded(Result<PlaylistDetail, String>),
    RemovePlaylistItem { playlist_id: String, item_id: String },
    ReorderPlaylistItems { playlist_id: String, item_ids: Vec<String> },
    MovePlaylistItem { item_id: String, direction: i8 },
    AddPlaylistItemsByPaths { playlist_id: String, paths: Vec<String> },
    PlaylistNameDraftChanged(String),
    SavePlaylistName,
    SystemStatusLoaded(Result<crate::backend::services::runtime::ExternalApiConnectionStatus, String>),
    SettingsLoaded(Result<(ApplicationSettings, Option<String>), String>),
    SettingsThemeChanged(String),
    SettingsCacheLimitChanged(String),
    SettingsPlayerChanged(String),
    SettingsCloseBehaviorChanged(String),
    SaveSettings,
    SettingsSaved(Result<ApplicationSettings, String>),
    TaskSnapshotLoaded { active: usize, completed: usize },
    TaskProgressLoaded(Vec<TaskProgressSnapshot>),
    CancelTask(String),
    WindowAction(WindowAction),
    /// 带代次的仓库列表结果。代次不匹配时保留当前启动步骤。
    WorkspaceListLoaded {
        generation: u64,
        result: Result<Vec<RepositorySummary>, String>,
    },
    StartupSyncFinished { generation: u64, result: Result<(), String> },
    ToggleSidebar,
    SetSidebarWidth(f32),
    CommitSidebarWidth,
    StartupRetry,
    MissingRefresh,
    MissingChoosePath,
    MissingPathChanged(String),
    MissingSubmitPath,
    MissingRelocateFinished(Result<(), String>),
    MissingOpenDelete,
    MissingCloseDelete,
    MissingConfirmDelete(DeleteMode),
    MissingDeleteFinished(Result<(), String>),
    MissingOpenSourceSettings,
    SelectWorkspaceRepository(String),
    SetWorkspacePanel(WorkspacePanel),
    SetLibraryCategory(LibraryCategory),
    /// 侧栏导航。具体分支在 `sidebar::reduce_message` 里归约。
    Sidebar(sidebar::SidebarMessage),
    /// 文件浏览和变更。具体分支在 `files::reduce_message` 里归约。
    Files(files::FilesMessage),
    /// 预览、元数据和搜索。具体分支在 `inspect::reduce_message` 里归约。
    Inspect(inspect::InspectMessage),
    /// ASMR 作品队列、播放列表和补全候选。具体分支在 `inspect_asmr` 里归约。
    Asmr(inspect_asmr::AsmrMessage),
    /// 播放列表成员、下载、回退、会话和播放条。具体分支在 `player::reduce_message` 里归约。
    Player(player::PlayerMessage),
    /// 设置、插件、日志、任务和仓库动作。具体分支在 `admin::reduce_message` 里归约。
    Admin(admin::AdminMessage),
    /// 拖放、外部打开和关闭确认。具体分支在 `input::reduce_message` 里归约。
    Input(input::InputMessage),
    /// 日志广播和资源库结构更新。具体分支在 `host_events::reduce_message` 里归约。
    Host(host_events::HostMessage),
    /// 结构更新后的静默仓库列表和摘要。具体分支在 `workspace_refresh::reduce_message` 里归约。
    SilentWorkspace(workspace_refresh::SilentMessage),
}

/// 已解码的 RGBA 预览帧；解码在服务任务中完成，窗口线程只负责上传 GPU 纹理。
#[derive(Clone, Debug)]
pub struct PreviewPixels {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// 宿主无关的窗口生命周期动作。
#[derive(Clone, Copy, Debug)]
pub enum WindowAction {
    Minimize,
    ToggleMaximize,
    Close,
}

/// 主内容页面的可观察状态。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum ShellPage {
    /// 服务尚未返回仓库结构。
    #[default]
    Loading,
    /// 当前仓库没有可展示文件。
    EmptyRepository,
    /// 仓库服务返回可向用户解释的错误。
    Error,
    /// 展示文件列表。
    FileList,
    /// 展示当前选中的文件及预览入口。
    SelectedFile,
    /// 当前仓库的播放列表。
    Playlists,
    /// 插件原生设置贡献页。
    PluginSettings,
    /// 任务中心有正在运行的任务。
    TaskRunning,
    /// 播放会话正在使用统一宿主控制器。
    PlaybackRunning,
    /// 任务已请求取消但 worker 尚未退出。
    TaskCancelling,
    /// 当前资源存在同步冲突。
    Conflict,
    /// 编辑器存在尚未保存的内容。
    UnsavedEdit,
    /// 应用设置页。
    Settings,
    /// 设置校验失败，保留用户输入并阻止写入。
    SettingsError,
    /// 系统日志页。
    Logs,
}

impl ShellPage {
    fn title(&self) -> &'static str {
        match self {
            Self::Loading => "正在加载资源库",
            Self::EmptyRepository => "资源库为空",
            Self::Error => "资源库加载失败",
            Self::FileList => "文件列表",
            Self::SelectedFile => "文件预览",
            Self::Playlists => "播放列表",
            Self::PluginSettings => "插件设置",
            Self::TaskRunning => "任务进行中",
            Self::PlaybackRunning => "播放进行中",
            Self::TaskCancelling => "任务取消中",
            Self::Conflict => "同步冲突",
            Self::UnsavedEdit => "编辑未保存",
            Self::Settings => "应用设置",
            Self::SettingsError => "设置校验失败",
            Self::Logs => "系统日志",
        }
    }
}

/// 壳层所需的宿主无关页面状态。
#[derive(Clone, Debug)]
pub struct ShellViewModel {
    pub page: ShellPage,
    pub repository_name: String,
    pub repository_id: Option<String>,
    pub selected_path: Option<String>,
    pub detail: String,
    pub dirty: bool,
    pub file_entries: Vec<String>,
    pub browser_entries: Vec<FileBrowserEntry>,
    pub current_directory: String,
    pub preview_url: Option<String>,
    pub preview_token: Option<String>,
    pub preview_pixels: Option<PreviewPixels>,
    pub plugin_entries: Vec<String>,
    pub plugin_entry_ids: Vec<String>,
    pub plugin_enabled: Vec<bool>,
    pub log_entries: Vec<String>,
    pub playlist_entries: Vec<String>,
    pub playlist_entry_ids: Vec<String>,
    pub selected_playlist_id: Option<String>,
    pub selected_playlist_player_type_id: Option<String>,
    pub playlist_name_draft: String,
    pub new_playlist_name: String,
    pub playlist_players: Vec<PlaylistPlayerContribution>,
    pub selected_new_playlist_player_type_id: Option<String>,
    pub playlist_dialog_open: bool,
    pub playlist_item_entries: Vec<String>,
    pub playlist_item_ids: Vec<String>,
    pub playlist_item_status: String,
    pub active_tasks: usize,
    pub completed_tasks: usize,
    pub active_task_ids: Vec<String>,
    pub task_progress: Vec<TaskProgressSnapshot>,
    pub system_status: Option<String>,
    pub settings: ApplicationSettings,
    pub settings_cache_limit_draft: String,
    pub settings_error: Option<String>,
    pub selected_plugin_id: Option<String>,
    pub plugin_config_keys: Vec<String>,
    pub plugin_config_drafts: std::collections::BTreeMap<String, String>,
    pub plugin_config_string_values: std::collections::BTreeSet<String>,
    /// 标记 `for_page` 造出的验收模型。界面和产品窗口共用同一套表面。
    pub acceptance_scene: bool,
    pub workspace: WorkspaceState,
    pub sidebar: sidebar::SidebarState,
    pub files: files::FilesState,
    pub inspect: inspect::InspectState,
    pub asmr: inspect_asmr::AsmrUi,
    pub player: player::PlayerState,
    pub admin: admin::AdminState,
    pub input: input::InputState,
    pub motion: motion::MotionState,
    /// 窗口逻辑宽。Vue 唯一按窗口宽度切换的断点在播放条，见 [`NARROW_VIEWPORT_PX`]。
    pub viewport_width: f32,
    /// 对话框、弹层或打开文件夹之后，手势松开时要重建树。
    pub surface_dirty: bool,
    /// `prepare` 里产生的目录浏览。这一帧就要提交，不能等下一次 `update`。
    pub(crate) staged_browses: Vec<sidebar::SidebarEffect>,
}

impl Default for ShellViewModel {
    fn default() -> Self {
        Self {
            page: ShellPage::default(),
            repository_name: "默认资源库".into(),
            repository_id: None,
            selected_path: None,
            detail: "等待资源库服务响应".into(),
            dirty: false,
            file_entries: Vec::new(),
            browser_entries: Vec::new(),
            current_directory: String::new(),
            preview_url: None,
            preview_token: None,
            preview_pixels: None,
            plugin_entries: Vec::new(),
            plugin_entry_ids: Vec::new(),
            plugin_enabled: Vec::new(),
            log_entries: Vec::new(),
            playlist_entries: Vec::new(),
            playlist_entry_ids: Vec::new(),
            selected_playlist_id: None,
            selected_playlist_player_type_id: None,
            playlist_name_draft: String::new(),
            new_playlist_name: String::new(),
            playlist_players: Vec::new(),
            selected_new_playlist_player_type_id: None,
            playlist_dialog_open: false,
            playlist_item_entries: Vec::new(),
            playlist_item_ids: Vec::new(),
            playlist_item_status: String::new(),
            active_tasks: 0,
            completed_tasks: 0,
            active_task_ids: Vec::new(),
            task_progress: Vec::new(),
            system_status: None,
            settings: ApplicationSettings::default(),
            settings_cache_limit_draft: "1024".into(),
            settings_error: None,
            selected_plugin_id: None,
            plugin_config_keys: Vec::new(),
            plugin_config_drafts: std::collections::BTreeMap::new(),
            plugin_config_string_values: std::collections::BTreeSet::new(),
            acceptance_scene: false,
            workspace: WorkspaceState::default(),
            sidebar: sidebar::SidebarState::default(),
            files: files::FilesState::default(),
            inspect: inspect::InspectState::default(),
            asmr: inspect_asmr::AsmrUi::default(),
            player: player::PlayerState::default(),
            admin: admin::AdminState::default(),
            input: input::InputState::default(),
            motion: motion::MotionState::default(),
            viewport_width: DEFAULT_VIEWPORT_PX,
            surface_dirty: false,
            staged_browses: Vec::new(),
        }
    }
}

/// 主窗口默认逻辑宽，和窗口描述符的初始尺寸一致。
pub const DEFAULT_VIEWPORT_PX: f32 = 1200.0;
/// Vue `@media (max-width: 1120px)`：窗口不超过这个宽度时，播放条改成三行。
pub const NARROW_VIEWPORT_PX: f32 = 1120.0;

impl ShellViewModel {
    /// 创建某一页面的验收模型，并挂上和产品窗口相同的表面。
    pub fn for_page(page: ShellPage) -> Self {
        let mut model = Self {
            page,
            acceptance_scene: true,
            ..Self::default()
        };
        acceptance::seed(&mut model);
        model
    }

    /// 宿主报告窗口逻辑宽。跨过 Vue 的 1120px 断点时标记重建，返回是否跨过。
    pub fn set_viewport_width(&mut self, width: f32) -> bool {
        if !width.is_finite() || width <= 0.0 {
            eprintln!("Nana 窗口宽度无效，保持 {}：{width}", self.viewport_width);
            return false;
        }
        let crossed = self.narrow_viewport() != (width <= NARROW_VIEWPORT_PX);
        self.viewport_width = width;
        if crossed {
            self.surface_dirty = true;
        }
        crossed
    }

    /// 窗口是否落在 Vue 的窄屏断点内。
    pub fn narrow_viewport(&self) -> bool {
        self.viewport_width <= NARROW_VIEWPORT_PX
    }

    /// 在 ViewModel 边界集中处理导航和页面动作，避免控件闭包直接修改领域状态。
    pub fn reduce(&mut self, message: ShellMessage) {
        self.reduce_inner(message);
        self.flush_folder_mutations();
        self.follow_motion();
    }

    fn flush_folder_mutations(&mut self) {
        let Some(mutation) = self.sidebar.take_folder_mutation() else {
            return;
        };
        let effect = match mutation {
            sidebar::FolderMutation::Create { repo_id, parent, name } => files::FilesEffect::CreateDirectory {
                repo_id,
                parent: (!parent.is_empty()).then_some(parent),
                name,
            },
            sidebar::FolderMutation::Rename { repo_id, path, name } => {
                files::FilesEffect::Rename { repo_id, path, new_name: name }
            }
            sidebar::FolderMutation::Delete { repo_id, path } => files::FilesEffect::Delete {
                repo_id,
                paths: vec![path],
                mode: None,
            },
        };
        self.files.enqueue(effect);
    }

    fn follow_motion(&mut self) {
        let modal_open = self.files.dialog_open()
            || self.workspace.delete_dialog_open()
            || self.playlist_dialog_open
            || self.input.source_playlist.is_some()
            || self.sidebar.smart_draft.open
            || self.input.pending_close
            || self.admin.pending_delete.is_some();
        let panel_open = self.sidebar.popover != sidebar::PopoverMode::Closed || self.admin.popover_open;
        let startup = f32::from(self.workspace.startup.percent);
        let operation = self.files.operation_percent();
        let spinner = self.sidebar.tree_loading || self.sidebar.submitting || self.sidebar.smart_draft.busy || self.files.mutating;
        if self.motion.set_modal_open(modal_open) || self.motion.set_panel_open(panel_open) {
            self.surface_dirty = true;
        }
        self.motion.set_startup_percent(startup);
        self.motion.set_operation_percent(operation);
        self.motion.set_spinner(spinner);
        self.motion.set_pulse(self.files.operation_indeterminate());
        self.motion.set_sweep(self.player.download_indeterminate());
        self.motion.set_sidebar_collapsed(self.workspace.sidebar_collapsed, self.workspace.sidebar_width);
    }

    fn reduce_inner(&mut self, message: ShellMessage) {
        let Some(message) = input::reduce_message(self, message) else {
            return;
        };
        let Some(message) = player::reduce_message(self, message) else {
            return;
        };
        let Some(message) = sidebar::reduce_message(self, message) else {
            return;
        };
        let Some(message) = files::reduce_message(self, message) else {
            return;
        };
        let Some(message) = inspect::reduce_message(self, message) else {
            return;
        };
        let Some(message) = inspect_asmr::reduce_message(self, message) else {
            return;
        };
        let Some(message) = admin::reduce_message(self, message) else {
            return;
        };
        let Some(message) = host_events::reduce_message(self, message) else {
            return;
        };
        let Some(message) = workspace_refresh::reduce_message(self, message) else {
            return;
        };
        match message {
            ShellMessage::Navigate(page) => {
                self.page = page;
                self.detail = match self.page {
                    ShellPage::TaskRunning => format!(
                        "{} 个运行中任务 · {} 个近期完成任务",
                        self.active_tasks, self.completed_tasks
                    ),
                    _ => "正在读取页面数据…".into(),
                };
            }
            ShellMessage::RepositoriesLoaded(result) => self.apply_loaded_repositories(None, result),
            ShellMessage::WorkspaceListLoaded { generation, result } => {
                self.apply_loaded_repositories(Some(generation), result);
            }
            ShellMessage::RepositorySnapshotLoaded(Ok(snapshot)) => {
                if self.workspace.startup.status == StartupStatus::Loading
                    && self.workspace.active_repo_id.as_deref() != Some(snapshot.repository.repo_id.as_str())
                {
                    eprintln!("Nana 忽略过期的资源库摘要：{}", snapshot.repository.repo_id);
                    return;
                }
                self.workspace.note_index_finished(&snapshot.repository.repo_id, Ok(()));
                self.page = ShellPage::FileList;
                self.repository_name = snapshot.repository.name.clone();
                self.repository_id = Some(snapshot.repository.repo_id.clone());
                self.detail = format!(
                    "{} 个文件 · {} 个文件夹 · {}",
                    snapshot.overview.file_count,
                    snapshot.overview.folder_count,
                    snapshot.repository.status
                );
                self.apply_snapshot_sidebar(&snapshot);
            }
            ShellMessage::RepositorySnapshotLoaded(Err(error)) => {
                if let Some(repo_id) = self.workspace.active_repo_id.clone() {
                    self.workspace.note_index_finished(&repo_id, Err(error.clone()));
                }
                self.page = ShellPage::Error;
                self.detail = format!("无法读取资源库文件列表：{error}");
            }
            ShellMessage::FileBrowserLoaded(Ok(browser)) => {
                let virtual_view = files::FileContext::from_model(self).is_virtual();
                if !self.files.apply_browser(&browser, virtual_view) {
                    return;
                }
                self.workspace.note_first_screen_finished(Ok(()));
                self.page = ShellPage::FileList;
                self.file_entries = self.files.entry_names();
                self.browser_entries = browser.entries;
                self.current_directory = browser.current_path.clone();
                self.sidebar.note_directory(
                    &browser.current_path,
                    browser.special_location.as_deref() == Some("trash"),
                );
                self.detail = format!(
                    "{} 个条目 · 当前目录 {}",
                    browser.total_entries, browser.current_path
                );
            }
            ShellMessage::ThumbnailPixels(frames) => {
                self.files.note_thumbnail_sizes(&frames);
            }
            ShellMessage::FileBrowserLoaded(Err(error)) => {
                if self.files.note_load_failed(&error) {
                    self.detail = format!("无法读取文件列表：{error}");
                    return;
                }
                self.workspace.note_first_screen_finished(Err(error.clone()));
                self.page = ShellPage::Error;
                self.detail = format!("无法读取文件列表：{error}");
            }
            ShellMessage::SelectFile { path, .. } => {
                self.page = ShellPage::SelectedFile;
                self.player.disarm_preview_audio();
                self.inspect.begin_selection(&path);
                self.selected_path = Some(path);
                self.preview_url = None;
                self.preview_token = None;
                self.preview_pixels = None;
                self.detail = "正在读取文件元数据…".into();
            }
            ShellMessage::OpenDirectory(path) => {
                self.page = ShellPage::FileList;
                self.player.disarm_preview_audio();
                self.detail = format!("正在读取目录 {path}…");
                self.selected_path = None;
                self.preview_url = None;
                self.preview_token = None;
                self.preview_pixels = None;
                self.inspect.clear();
            }
            ShellMessage::AssetDetailLoaded(Ok(detail)) => {
                self.page = ShellPage::SelectedFile;
                self.selected_path = Some(detail.summary.path.clone());
                self.detail = format!(
                    "{} · {} 个元数据字段 · {} 个修订",
                    detail.summary.size_label,
                    detail.metadata.len(),
                    detail.revisions.len()
                );
                self.inspect.note_detail(&detail);
                if let Some(session) = self.inspect.media_session().cloned() {
                    self.player.adopt_session(session);
                }
            }
            ShellMessage::AssetDetailLoaded(Err(error)) => {
                self.page = ShellPage::Error;
                self.detail = format!("无法读取文件元数据：{error}");
                self.inspect.note_detail_error(&error);
            }
            ShellMessage::PreviewSourceLoaded(Ok(source)) => {
                self.page = ShellPage::SelectedFile;
                self.preview_url = source.source_url;
                self.preview_token = Some(source.token);
                self.detail = format!(
                    "{} · {} · {} 字节",
                    source.media_type, source.path, source.size_bytes
                );
            }
            ShellMessage::PreviewSourceLoaded(Err(error)) => {
                self.page = ShellPage::Error;
                self.detail = format!("无法打开预览源：{error}");
            }
            ShellMessage::PreviewPixelsLoaded { source, pixels } => {
                let pixels_ok = pixels.is_ok();
                let error_text = pixels.as_ref().err().cloned();
                if !self.inspect.note_pixels(&source.path, pixels_ok, error_text.as_deref()) {
                    return;
                }
                self.page = ShellPage::SelectedFile;
                self.preview_url = source.source_url;
                self.preview_token = Some(source.token);
                match pixels {
                    Ok(pixels) => {
                        self.preview_pixels = Some(pixels);
                        self.detail = format!("原生图片预览已加载 · {}", source.media_type);
                    }
                    Err(error) => {
                        self.preview_pixels = None;
                        self.detail = format!("预览源已准备，但图片解码失败：{error}");
                    }
                }
            }
            ShellMessage::PlaylistsLoaded(Ok(playlists)) => {
                self.page = ShellPage::Playlists;
                self.playlist_entries = playlists
                    .iter()
                    .map(|playlist| format!("{} · {} 项", playlist.name, playlist.item_count))
                    .collect();
                self.playlist_entry_ids = playlists.iter().map(|playlist| playlist.playlist_id.clone()).collect();
                self.detail = format!("{} 个播放列表", playlists.len());
            }
            ShellMessage::PlaylistsLoaded(Err(error)) => {
                self.page = ShellPage::Error;
                self.detail = format!("无法读取播放列表：{error}");
            }
            ShellMessage::PlaylistPlayersLoaded(Ok(players)) => {
                self.playlist_players = players;
                if self.selected_new_playlist_player_type_id.is_none() {
                    self.selected_new_playlist_player_type_id = self
                        .playlist_players
                        .first()
                        .map(|player| player.player_type_id.clone());
                }
                self.detail = format!("可用播放器 {} 个", self.playlist_players.len());
            }
            ShellMessage::PlaylistPlayersLoaded(Err(error)) => {
                self.detail = format!("无法读取播放器类型：{error}");
            }
            ShellMessage::NewPlaylistNameChanged(value) => {
                self.new_playlist_name = value;
            }
            ShellMessage::SelectPlaylistPlayer(player_type_id) => {
                self.selected_new_playlist_player_type_id = Some(player_type_id);
            }
            ShellMessage::OpenPlaylistDialog => {
                self.playlist_dialog_open = true;
            }
            ShellMessage::ClosePlaylistDialog => {
                self.playlist_dialog_open = false;
            }
            ShellMessage::CreatePlaylist => {
                if self.new_playlist_name.trim().is_empty() {
                    self.detail = "播放列表名称不能为空".into();
                } else if self.selected_new_playlist_player_type_id.is_none() {
                    self.detail = "请先选择播放器类型".into();
                } else {
                    self.detail = "正在创建播放列表…".into();
                    self.playlist_dialog_open = false;
                }
            }
            ShellMessage::SelectPlaylist(playlist_id) => {
                self.page = ShellPage::Playlists;
                self.selected_playlist_id = Some(playlist_id.clone());
                self.detail = format!("正在读取播放列表 {playlist_id}…");
            }
            ShellMessage::DeletePlaylist(playlist_id) => {
                self.detail = format!("正在删除播放列表 {playlist_id}…");
            }
            ShellMessage::PlaylistDetailLoaded(Ok(detail)) => {
                if self.sidebar.bound_repo_id().is_some_and(|repo_id| repo_id != detail.playlist.repo_id.as_str()) {
                    eprintln!("Nana 忽略过期的播放集详情：{}", detail.playlist.playlist_id);
                    return;
                }
                self.page = ShellPage::Playlists;
                self.selected_playlist_id = Some(detail.playlist.playlist_id.clone());
                self.selected_playlist_player_type_id = Some(detail.playlist.player_type_id.clone());
                self.playlist_name_draft = detail.playlist.name.clone();
                self.playlist_item_entries = detail
                    .items
                    .iter()
                    .map(|item| format!("{} · {}", item.filename, item.status))
                    .collect();
                self.playlist_item_ids = detail
                    .items
                    .iter()
                    .map(|item| item.playlist_item_id.clone())
                    .collect();
                self.playlist_item_status = detail
                    .items
                    .iter()
                    .filter(|item| item.status != "ready")
                    .map(|item| format!("{}: {}", item.filename, item.status_reason.clone().unwrap_or_else(|| item.status.clone())))
                    .collect::<Vec<_>>()
                    .join(" · ");
                self.detail = format!("{} · {} 个项目", detail.playlist.name, detail.items.len());
            }
            ShellMessage::PlaylistDetailLoaded(Err(error)) => {
                self.page = ShellPage::Error;
                self.detail = format!("无法读取播放列表详情：{error}");
            }
            ShellMessage::RemovePlaylistItem { item_id, .. } => {
                self.detail = format!("正在移除播放列表项目 {item_id}…");
            }
            ShellMessage::ReorderPlaylistItems { .. } => {
                self.detail = "正在保存播放列表顺序…".into();
            }
            ShellMessage::MovePlaylistItem { item_id, direction } => {
                if let Some(index) = self.playlist_item_ids.iter().position(|id| id == &item_id) {
                    let target = if direction < 0 { index.checked_sub(1) } else { (index + 1 < self.playlist_item_ids.len()).then_some(index + 1) };
                    if let Some(target) = target {
                        self.playlist_item_ids.swap(index, target);
                        self.playlist_item_entries.swap(index, target);
                        if let Some(playlist_id) = self.selected_playlist_id.clone() {
                            self.detail = format!("正在保存播放列表顺序：{}", playlist_id);
                        }
                    }
                }
            }
            ShellMessage::AddPlaylistItemsByPaths { paths, .. } => {
                self.detail = if paths.is_empty() {
                    "没有可添加的文件路径".into()
                } else {
                    format!("正在添加 {} 个播放列表项目…", paths.len())
                };
            }
            ShellMessage::PlaylistNameDraftChanged(value) => {
                self.playlist_name_draft = value;
            }
            ShellMessage::SavePlaylistName => {
                self.detail = if self.selected_playlist_id.is_none() {
                    "请先选择一个播放列表".into()
                } else if self.playlist_name_draft.trim().is_empty() {
                    "播放列表名称不能为空".into()
                } else {
                    "正在保存播放列表名称…".into()
                };
            }
            ShellMessage::PluginsLoaded(_)
            | ShellMessage::SelectPlugin(_)
            | ShellMessage::TogglePlugin { .. }
            | ShellMessage::DeletePlugin(_)
            | ShellMessage::PluginConfigLoaded(_)
            | ShellMessage::DeletePluginConfig { .. }
            | ShellMessage::PluginConfigDraftChanged { .. }
            | ShellMessage::SavePluginConfig { .. }
            | ShellMessage::LogsLoaded(_)
            | ShellMessage::ClearLogs
            | ShellMessage::SystemStatusLoaded(_)
            | ShellMessage::SettingsLoaded(_)
            | ShellMessage::SettingsThemeChanged(_)
            | ShellMessage::SettingsCacheLimitChanged(_)
            | ShellMessage::SettingsPlayerChanged(_)
            | ShellMessage::SettingsCloseBehaviorChanged(_)
            | ShellMessage::SaveSettings
            | ShellMessage::SettingsSaved(_)
            | ShellMessage::TaskSnapshotLoaded { .. }
            | ShellMessage::TaskProgressLoaded(_)
            | ShellMessage::CancelTask(_)
            | ShellMessage::Admin(_) => {}
            ShellMessage::WindowAction(_) => {}
            ShellMessage::Refresh => {
                self.detail = "正在刷新资源库…".into();
                self.workspace.request_repository_refresh();
            }
            ShellMessage::StartupSyncFinished { generation, result } => {
                self.workspace.note_sync_finished(generation, result);
                self.detail = self.workspace.startup.step_label.clone();
                if self.workspace.main_region() == MainRegion::LoadError {
                    self.page = ShellPage::Error;
                    if let Some(error) = &self.workspace.startup.error {
                        self.detail = format!("无法同步资源库：{error}");
                    }
                }
            }
            ShellMessage::ToggleSidebar => self.workspace.toggle_sidebar(),
            ShellMessage::SetSidebarWidth(width) => self.workspace.set_sidebar_width(width),
            ShellMessage::CommitSidebarWidth => self.workspace.commit_sidebar_width(),
            ShellMessage::StartupRetry => {
                if self.workspace.retry_startup() {
                    self.detail = self.workspace.startup.step_label.clone();
                }
            }
            ShellMessage::MissingRefresh => self.workspace.refresh_missing(),
            ShellMessage::MissingChoosePath => input::begin_relocate_dialog(self),
            ShellMessage::MissingPathChanged(value) => self.workspace.set_path_draft(value),
            ShellMessage::MissingSubmitPath => self.workspace.submit_missing_path(),
            ShellMessage::MissingRelocateFinished(result) => self.workspace.note_relocate_finished(result),
            ShellMessage::MissingOpenDelete => self.workspace.open_delete_dialog(),
            ShellMessage::MissingCloseDelete => self.workspace.close_delete_dialog(),
            ShellMessage::MissingConfirmDelete(mode) => {
                self.workspace.confirm_delete(mode);
            }
            ShellMessage::MissingDeleteFinished(result) => {
                self.workspace.note_delete_finished(result);
                self.repository_id = self.workspace.active_repo_id.clone();
                if let Some(repository) = self.workspace.active_repository() {
                    self.repository_name = repository.name.clone();
                } else if self.workspace.main_region() == MainRegion::EmptyRepository {
                    self.page = ShellPage::EmptyRepository;
                    self.repository_name = "默认资源库".into();
                    self.detail = "还没有可用资源库".into();
                }
            }
            ShellMessage::MissingOpenSourceSettings => {
                self.workspace.open_source_settings();
                if self.workspace.effects.iter().any(|effect| matches!(effect, WorkspaceEffect::OpenSourceSettings)) {
                    self.page = ShellPage::Settings;
                    self.detail = "正在打开来源设置…".into();
                }
            }
            ShellMessage::SelectWorkspaceRepository(repo_id) => {
                self.workspace.select_repository(&repo_id);
                self.repository_id = self.workspace.active_repo_id.clone();
                if let Some(repository) = self.workspace.active_repository() {
                    self.repository_name = repository.name.clone();
                }
                self.bind_sidebar_repository();
            }
            ShellMessage::SetWorkspacePanel(panel) => self.workspace.panel = panel,
            ShellMessage::SetLibraryCategory(category) => self.workspace.library_category = category,
            ShellMessage::PrimaryAction => {
                if !self.inspect.request_open() {
                    self.detail = "当前没有可打开的预览".into();
                }
            }
            ShellMessage::EditAction => {
                self.detail = if self.dirty || self.inspect.dirty() {
                    "未保存的修改留在当前草稿".into()
                } else {
                    "当前没有未保存的修改".into()
                };
            }
            ShellMessage::Sidebar(_) => {}
            ShellMessage::Files(_) => {}
            ShellMessage::Inspect(_) => {}
            ShellMessage::Player(_) => {}
            ShellMessage::Input(_) => {}
            ShellMessage::Host(_) => {}
            ShellMessage::SilentWorkspace(_) => {}
            ShellMessage::Asmr(_) => {}
        }
    }

    /// 把仓库列表结果写进工作台，并同步壳层上仍被旧页面读取的仓库名称。
    fn apply_loaded_repositories(&mut self, generation: Option<u64>, result: Result<Vec<RepositorySummary>, String>) {
        let mapped = match result {
            Ok(items) => Ok(items.iter().map(WorkspaceRepository::from_summary).collect()),
            Err(error) => Err(error),
        };
        self.workspace.apply_repository_list(generation, mapped);
        self.repository_id = self.workspace.active_repo_id.clone();
        if let Some(repository) = self.workspace.active_repository() {
            self.repository_name = repository.name.clone();
        }
        match self.workspace.main_region() {
            MainRegion::LoadError => {
                self.page = ShellPage::Error;
                self.detail = format!(
                    "无法读取资源库：{}",
                    self.workspace.startup.error.clone().unwrap_or_else(|| "未知错误".into())
                );
            }
            MainRegion::EmptyRepository => {
                self.page = ShellPage::EmptyRepository;
                self.repository_id = None;
                self.repository_name = "默认资源库".into();
                self.detail = "还没有可用资源库".into();
                self.file_entries.clear();
                self.browser_entries.clear();
            }
            MainRegion::MissingRepository => {
                self.detail = "资源库丢失".into();
                self.file_entries.clear();
                self.browser_entries.clear();
            }
            MainRegion::HasRepository => {
                self.page = ShellPage::FileList;
                self.detail = format!("{} 个资源库 · 已加载文件列表", self.workspace.repositories.len());
            }
            MainRegion::Startup => {
                self.detail = self.workspace.startup.step_label.clone();
            }
        }
        self.bind_sidebar_repository();
    }
}

mod interaction;
mod render;
mod workbench;
mod title_bar;
pub use interaction::commit_interaction;
pub(crate) use interaction::window_action_commands;
pub use render::mount_shell;

#[cfg(test)]
mod tests;
