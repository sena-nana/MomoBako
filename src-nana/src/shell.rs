//! MomoBako 原生应用壳层和页面 ViewModel。
//!
//! 壳层只描述稳定的导航、状态和主内容层级；仓库、插件和任务服务通过
//! `ShellViewModel` 注入文本状态，避免把领域服务直接耦合到 Nana 控件树。

use crate::backend::services::repository::{
    AssetDetail, FileBrowserEntry, FileBrowserSnapshot, FilePreviewSourceResponse, RepositorySnapshot,
    PlaylistSummary, RepositorySummary, SystemLogPage,
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
pub(crate) mod search_collate;
pub(crate) mod audio_decode;
pub(crate) mod player;
pub(crate) mod admin;
pub use admin::{AdminMessage, SourceStep, ToolPageEntry};
pub use acceptance::gap_models as acceptance_gap_models;
pub(crate) mod input;
pub mod host_events;
mod player_view;
mod sidebar;
mod sidebar_view;
mod workspace;
pub(crate) mod workspace_refresh;
pub(crate) mod tree_sync;
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
    SidebarTree, ShortcutId,
};
pub(crate) use sidebar::escape_layer;
pub use workspace::{
    sidebar_prefs_path, DeleteMode, LibraryCategory, MainRegion, StartupStatus, WorkspaceEffect,
    WorkspacePanel, WorkspaceRepository, WorkspaceState,
};

/// Nana Runtime 传递给应用状态的壳层交互消息。
pub enum ShellMessage {
    /// 打开设置页：读设置包和应用设置，和 Vue `Settings.vue` 挂载时的 `loadSettingsData` 一致。
    OpenSettings,
    Refresh,
    RepositoriesLoaded(Result<Vec<RepositorySummary>, String>),
    RepositorySnapshotLoaded(Result<RepositorySnapshot, String>),
    FileBrowserLoaded(Result<FileBrowserSnapshot, String>),
    /// 缩略图解码结果。像素由宿主上传，归约只留下原始宽高。
    ThumbnailPixels(Vec<ThumbnailFrame>),
    SelectFile { path: String, asset_id: Option<String> },
    OpenDirectory(String),
    AssetDetailLoaded(Result<AssetDetail, String>),
    PreviewPixelsLoaded {
        source: FilePreviewSourceResponse,
        pixels: Result<PreviewPixels, String>,
    },
    PluginConfigLoaded(Result<PluginConfigSnapshot, String>),
    LogsLoaded(Result<SystemLogPage, String>),
    ClearLogs,
    /// 新建或删除播放集以后这个仓库的整份播放集列表。`open` 是新建出来、要接着点开的播放集。
    PlaylistsLoaded { repo_id: String, result: Result<Vec<PlaylistSummary>, String>, open: Option<String> },
    PlaylistPlayersLoaded(Result<Vec<PlaylistPlayerContribution>, String>),
    NewPlaylistNameChanged(String),
    SelectPlaylistPlayer(String),
    OpenPlaylistDialog,
    ClosePlaylistDialog,
    CreatePlaylist,
    PlaylistDetailLoaded(Result<PlaylistDetail, String>),
    RemovePlaylistItem { playlist_id: String, item_id: String },
    SettingsLoaded(Result<(ApplicationSettings, Option<String>), String>),
    SettingsThemeChanged(String),
    SettingsSaved(Result<ApplicationSettings, String>),
    /// 宿主观察到的运行中任务（排队、运行、取消中），整份换上。
    TaskProgressLoaded(Vec<TaskProgressSnapshot>),
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
    /// 刷新文件夹树时的同步和重读结果。具体分支在 `tree_sync::reduce_message` 里归约。
    TreeSync(tree_sync::TreeSyncMessage),
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

/// 壳层所需的宿主无关页面状态。
#[derive(Clone, Debug)]
pub struct ShellViewModel {
    pub page: ShellPage,
    pub repository_name: String,
    pub repository_id: Option<String>,
    pub selected_path: Option<String>,
    pub dirty: bool,
    pub browser_entries: Vec<FileBrowserEntry>,
    pub current_directory: String,
    pub preview_token: Option<String>,
    pub preview_pixels: Option<PreviewPixels>,
    pub selected_playlist_id: Option<String>,
    pub new_playlist_name: String,
    pub playlist_players: Vec<PlaylistPlayerContribution>,
    pub selected_new_playlist_player_type_id: Option<String>,
    pub playlist_dialog_open: bool,
    pub playlist_item_ids: Vec<String>,
    pub playlist_item_status: String,
    /// 运行中的任务，任务弹层和侧栏「任务」的计数读它。
    pub task_progress: Vec<TaskProgressSnapshot>,
    pub settings: ApplicationSettings,
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
    /// 全局状态区：最近一次失败和它的来源，见 `status.rs`。
    pub status: status::StatusState,
    /// 文件夹树的「刷新」：同步仓库再刷新工作区，见 `tree_sync.rs`。
    pub tree_sync: tree_sync::TreeSyncState,
    /// 窗口逻辑宽。Vue 唯一按窗口宽度切换的断点在播放条，见 [`NARROW_VIEWPORT_PX`]。
    pub viewport_width: f32,
    /// 对话框、弹层或打开文件夹之后，手势松开时要重建树。
    pub surface_dirty: bool,
    /// 状态版本：每次归约加一，归约之外改了界面要读的状态、标脏时也加一。视图据此判断上次整体
    /// 同步之后状态有没有变过，整块重挂的内容按它决定要不要重挂。
    pub(crate) revision: u64,
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
            dirty: false,
            browser_entries: Vec::new(),
            current_directory: String::new(),
            preview_token: None,
            preview_pixels: None,
            selected_playlist_id: None,
            new_playlist_name: String::new(),
            playlist_players: Vec::new(),
            selected_new_playlist_player_type_id: None,
            playlist_dialog_open: false,
            playlist_item_ids: Vec::new(),
            playlist_item_status: String::new(),
            task_progress: Vec::new(),
            settings: ApplicationSettings::default(),
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
            status: status::StatusState::default(),
            tree_sync: tree_sync::TreeSyncState::default(),
            viewport_width: DEFAULT_VIEWPORT_PX,
            surface_dirty: false,
            revision: 0,
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
            self.mark_surface_dirty();
        }
        crossed
    }

    /// 归约之外改了界面要读的状态：标脏，并让状态版本加一，整块重挂的内容随后按新状态重挂。
    pub(crate) fn mark_surface_dirty(&mut self) {
        self.surface_dirty = true;
        self.revision = self.revision.wrapping_add(1);
    }

    /// 窗口是否落在 Vue 的窄屏断点内。
    pub fn narrow_viewport(&self) -> bool {
        self.viewport_width <= NARROW_VIEWPORT_PX
    }

    /// 在 ViewModel 边界集中处理导航和页面动作，避免控件闭包直接修改领域状态。
    pub fn reduce(&mut self, message: ShellMessage) {
        self.revision = self.revision.wrapping_add(1);
        let (before, seq, started) = (status::Activity::of(self), self.status.seq(), status::starts_operation(&message));
        let startup_open = self.workspace.startup.status != StartupStatus::Ready;
        self.reduce_inner(message);
        if startup_open && self.workspace.startup.status == StartupStatus::Ready {
            // Vue 启动流程在结束前 `loadSettingsData`：插件、钩子记录、缓存、API 设计和外部连接。
            // 没有仓库、仓库丢失的启动也读，添加资源库的来源列表要用插件清单。
            self.admin.begin_settings_load();
        }
        self.flush_folder_mutations();
        self.settle_sidebar_dialogs();
        tree_sync::settle(self);
        self.settle_status(&before, seq, started);
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
            sidebar::FolderMutation::Delete { repo_id, path, mode } => files::FilesEffect::Delete {
                repo_id,
                paths: vec![path],
                mode: Some(mode.as_str().into()),
            },
        };
        self.files.enqueue(effect);
    }

    fn follow_motion(&mut self) {
        let panel_open = self.sidebar.popover != sidebar::PopoverMode::Closed || self.admin.popover_open;
        let startup = f32::from(self.workspace.startup.percent);
        let operation = self.files.operation_percent();
        let spinner = self.sidebar.tree_loading
            || self.sidebar.submitting
            || self.sidebar.smart_draft.busy
            || self.files.mutating
            || self.tree_sync.running()
            || status::busy(self);
        if self.motion.set_panel_open(panel_open) {
            self.surface_dirty = true;
        }
        self.motion.set_startup_percent(startup);
        self.motion.set_operation_percent(operation);
        self.motion.set_spinner(spinner);
        self.motion.set_pulse(self.files.operation_indeterminate());
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
        let Some(message) = tree_sync::reduce_message(self, message) else {
            return;
        };
        match message {
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
                self.apply_snapshot_sidebar(&snapshot);
            }
            ShellMessage::RepositorySnapshotLoaded(Err(error)) => {
                eprintln!("Nana 读取资源库摘要失败：{error}");
                if self.workspace.startup.status == StartupStatus::Ready {
                    self.status.fail(status::FailureSource::Repository, format!("无法读取资源库摘要：{error}"));
                }
                if let Some(repo_id) = self.workspace.active_repo_id.clone() {
                    self.workspace.note_index_finished(&repo_id, Err(error));
                }
                self.page = ShellPage::Error;
            }
            ShellMessage::FileBrowserLoaded(Ok(browser)) => {
                let virtual_view = files::FileContext::from_model(self).is_virtual();
                if !self.files.apply_browser(&browser, virtual_view) {
                    return;
                }
                self.workspace.note_first_screen_finished(Ok(()));
                self.page = ShellPage::FileList;
                self.browser_entries = browser.entries;
                self.current_directory = browser.current_path.clone();
                self.sidebar.note_directory(
                    &browser.current_path,
                    browser.special_location.as_deref() == Some("trash"),
                );
            }
            ShellMessage::ThumbnailPixels(frames) => {
                self.files.note_thumbnail_sizes(&frames);
            }
            ShellMessage::FileBrowserLoaded(Err(error)) => {
                if self.files.note_load_failed(&error) {
                    return;
                }
                eprintln!("Nana 读取首屏目录失败：{error}");
                if self.workspace.startup.status == StartupStatus::Ready {
                    self.status.fail(status::FailureSource::Directory, format!("无法读取文件列表：{error}"));
                }
                self.workspace.note_first_screen_finished(Err(error));
                self.page = ShellPage::Error;
            }
            ShellMessage::SelectFile { path, .. } => {
                self.page = ShellPage::SelectedFile;
                self.player.disarm_preview_audio();
                self.inspect.begin_selection(&path);
                self.selected_path = Some(path);
                self.preview_token = None;
                self.preview_pixels = None;
            }
            ShellMessage::OpenDirectory(_) => {
                self.page = ShellPage::FileList;
                self.player.disarm_preview_audio();
                self.selected_path = None;
                self.preview_token = None;
                self.preview_pixels = None;
                self.inspect.clear();
            }
            ShellMessage::AssetDetailLoaded(Ok(detail)) => {
                self.page = ShellPage::SelectedFile;
                self.selected_path = Some(detail.summary.path.clone());
                self.inspect.note_detail(&detail);
                if let Some(session) = self.inspect.media_session().cloned() {
                    self.player.adopt_session(session);
                }
            }
            ShellMessage::AssetDetailLoaded(Err(error)) => {
                self.status.fail(status::FailureSource::Asset, format!("无法读取文件元数据：{error}"));
                self.page = ShellPage::Error;
                self.inspect.note_detail_error(&error);
            }
            ShellMessage::PreviewPixelsLoaded { source, pixels } => {
                let pixels_ok = pixels.is_ok();
                let error_text = pixels.as_ref().err().cloned();
                if !self.inspect.note_pixels(&source.path, pixels_ok, error_text.as_deref()) {
                    return;
                }
                self.page = ShellPage::SelectedFile;
                self.preview_token = Some(source.token);
                self.preview_pixels = pixels.ok();
            }
            ShellMessage::PlaylistsLoaded { repo_id, result: Ok(playlists), open } => {
                self.apply_playlist_list(&repo_id, &playlists, open);
            }
            ShellMessage::PlaylistsLoaded { result: Err(error), .. } => {
                eprintln!("Nana 播放集新建或删除失败：{error}");
                self.status.fail(status::FailureSource::Playlist, format!("播放集操作失败：{error}"));
            }
            ShellMessage::PlaylistPlayersLoaded(Ok(mut players)) => {
                // 照 Vue `listRegisteredPlaylistPlayers` 按名称的 zh-CN 顺序排，新建播放集默认选第一项。
                players.sort_by(|left, right| {
                    search_collate::compare_zh(&left.label, &right.label).then_with(|| left.player_type_id.cmp(&right.player_type_id))
                });
                self.playlist_players = players;
                if self.selected_new_playlist_player_type_id.is_none() {
                    self.selected_new_playlist_player_type_id = self
                        .playlist_players
                        .first()
                        .map(|player| player.player_type_id.clone());
                }
            }
            ShellMessage::PlaylistPlayersLoaded(Err(error)) => {
                self.status.fail(status::FailureSource::Playlist, format!("无法读取播放器类型：{error}"));
            }
            ShellMessage::NewPlaylistNameChanged(value) => {
                self.new_playlist_name = value;
            }
            ShellMessage::SelectPlaylistPlayer(player_type_id) => {
                self.selected_new_playlist_player_type_id = Some(player_type_id);
            }
            ShellMessage::OpenPlaylistDialog => self.open_playlist_dialog(),
            ShellMessage::ClosePlaylistDialog => {
                self.playlist_dialog_open = false;
            }
            // 名称为空或没有选类型时「创建」禁用，回车提交也不建（Vue `playlistDialogDisabled`）；
            // 提交由 `app_dispatch::dispatch_playlists` 按同样的条件发出。
            ShellMessage::CreatePlaylist => {
                if self.new_playlist_name.trim().is_empty() || self.selected_new_playlist_player_type_id.is_none() {
                    eprintln!("Nana 新建播放集缺少名称或播放类型，不提交");
                } else {
                    self.playlist_dialog_open = false;
                }
            }
            ShellMessage::PlaylistDetailLoaded(Ok(detail)) => {
                if self.sidebar.bound_repo_id().is_some_and(|repo_id| repo_id != detail.playlist.repo_id.as_str()) {
                    eprintln!("Nana 忽略过期的播放集详情：{}", detail.playlist.playlist_id);
                    return;
                }
                self.selected_playlist_id = Some(detail.playlist.playlist_id.clone());
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
            }
            ShellMessage::PlaylistDetailLoaded(Err(error)) => {
                eprintln!("Nana 播放集详情读取或条目修改失败：{error}");
                self.status.fail(status::FailureSource::Playlist, format!("播放集操作失败：{error}"));
            }
            ShellMessage::OpenSettings
            | ShellMessage::RemovePlaylistItem { .. }
            | ShellMessage::PluginConfigLoaded(_)
            | ShellMessage::LogsLoaded(_)
            | ShellMessage::ClearLogs
            | ShellMessage::SettingsLoaded(_)
            | ShellMessage::SettingsThemeChanged(_)
            | ShellMessage::SettingsSaved(_)
            | ShellMessage::TaskProgressLoaded(_)
            | ShellMessage::Admin(_) => {}
            ShellMessage::WindowAction(_) => {}
            ShellMessage::Refresh => self.workspace.request_repository_refresh(),
            ShellMessage::StartupSyncFinished { generation, result } => {
                self.workspace.note_sync_finished(generation, result);
                if self.workspace.main_region() == MainRegion::LoadError {
                    self.page = ShellPage::Error;
                }
            }
            ShellMessage::ToggleSidebar => self.workspace.toggle_sidebar(),
            ShellMessage::SetSidebarWidth(width) => self.workspace.set_sidebar_width(width),
            ShellMessage::CommitSidebarWidth => self.workspace.commit_sidebar_width(),
            ShellMessage::StartupRetry => {
                self.workspace.retry_startup();
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
                }
            }
            ShellMessage::MissingOpenSourceSettings => {
                if let Some(plugin_id) = self.workspace.open_source_settings() {
                    admin::open_settings_page(self, Some(&plugin_id));
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
            ShellMessage::Sidebar(_) => {}
            ShellMessage::Files(_) => {}
            ShellMessage::Inspect(_) => {}
            ShellMessage::Player(_) => {}
            ShellMessage::Input(_) => {}
            ShellMessage::Host(_) => {}
            ShellMessage::SilentWorkspace(_) => {}
            ShellMessage::TreeSync(_) => {}
            ShellMessage::Asmr(_) => {}
        }
    }

    /// 把仓库列表结果写进工作台，并同步壳层上仍被旧页面读取的仓库名称。
    fn apply_loaded_repositories(&mut self, generation: Option<u64>, result: Result<Vec<RepositorySummary>, String>) {
        let mapped = match result {
            Ok(items) => Ok(items.iter().map(WorkspaceRepository::from_summary).collect()),
            Err(error) => Err(error),
        };
        // 启动以后重读列表失败：缺失仓库页会就近显示，别的区域进状态区。
        let failed = mapped.as_ref().err().filter(|_| self.workspace.startup.status == StartupStatus::Ready).cloned();
        let applied = self.workspace.apply_repository_list(generation, mapped);
        if let Some(error) = failed.filter(|_| applied && self.workspace.main_region() != MainRegion::MissingRepository) {
            self.status.fail(status::FailureSource::Repository, format!("无法读取资源库列表：{error}"));
        }
        self.repository_id = self.workspace.active_repo_id.clone();
        if let Some(repository) = self.workspace.active_repository() {
            self.repository_name = repository.name.clone();
        }
        match self.workspace.main_region() {
            MainRegion::LoadError => self.page = ShellPage::Error,
            MainRegion::EmptyRepository => {
                self.page = ShellPage::EmptyRepository;
                self.repository_id = None;
                self.repository_name = "默认资源库".into();
                self.browser_entries.clear();
            }
            MainRegion::MissingRepository => self.browser_entries.clear(),
            MainRegion::HasRepository => self.page = ShellPage::FileList,
            MainRegion::Startup => {}
        }
        self.bind_sidebar_repository();
    }
}

mod interaction;
mod render;
mod remount_state;
mod row_sync;
pub(crate) mod status;
mod shell_tint;
mod startup_view;
mod workbench;
mod title_bar;
mod hot;
mod view_host;
mod view_part;
mod view_part_sidebar;
mod view_part_primary;
mod view_part_overlay;
mod route_home;
mod route_startup;
mod route_settings;
mod route_files;
mod route_search;
mod route_playlists;
mod route_admin;
mod route_missing;
mod route_empty;
pub use interaction::commit_interaction;
pub(crate) use interaction::window_action_commands;
pub use view_host::mount_shell;
pub(crate) use view_host::ShellView;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod escape_tests;
#[cfg(test)]
mod data_load_tests;
#[cfg(test)]
mod operation_row_tests;
#[cfg(test)]
pub(crate) mod view_harness;
