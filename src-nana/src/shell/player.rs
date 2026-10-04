//! 播放列表、播放条和共用播放会话。
//!
//! 成员资格、下载进度、播放器回退、会话持久化和仓库切换停止都在这里归约。
//! 播放条和预览页共用一份 `PlaybackSessionState`。没有原生解码器时停在失败态。

use std::collections::BTreeMap;
use std::path::Path;

use crate::backend::services::repository::{
    DownloaderPlaylistProgressEvent, DownloaderPlaylistRequest, PlaybackSessionState, PlaylistDetail,
    PlaylistItemsByPathsAddRequest, PlaylistItemsOrderRequest, PlaylistMembershipRequest, PlaylistMembershipSnapshot,
    PlaylistPlayerContribution, PlaylistSummary,
};
use crate::host_api::{PlaybackMediaCapabilities, PlaybackMediaPlugin, PlaybackSessionController};

use super::inspect::InspectState;
use super::workspace::{LibraryCategory, MainRegion, StartupStatus, WorkspacePanel};
use super::ShellViewModel;

#[path = "player_support.rs"]
mod support;
pub use support::{preferences_path, sessions_path, settings_path};
use support::{
    PlayerMatch, StoredSession, compatible_playlist_ids, cycle_mode, default_settings, find_player_for_extension,
    format_time, membership_can_toggle, mode_label, next_membership_ids, next_ready_id, preferences_json,
    previous_ready_id, queue_item_from_playlist, read_preferences_file, read_sessions_file, read_settings_file,
    ready_ids, reorder_before, resolution_notice, resolve_player, sessions_json, settings_json, shuffle_order,
    system_media_session_available, write_json, NextStep,
};

/// 列表循环、随机、单曲循环。和 Vue 的切换顺序一致。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PlaybackMode {
    #[default]
    ListLoop,
    Shuffle,
    SingleLoop,
}

/// 图片停留和画面适配。`object_fit_cover` 为假时是适应。
#[derive(Clone, Debug, PartialEq)]
pub struct PlaybackSettings {
    pub image_duration_ms: u32,
    pub object_fit_cover: bool,
}

/// 已登记的播放器实现。生产列表从空开始，登记前不能假装能播放。
#[derive(Clone, Debug, PartialEq)]
pub struct PlayerCandidate {
    pub plugin_id: String,
    pub player_type_id: String,
    pub capability_id: Option<String>,
    pub label: String,
    pub file_class: String,
    pub extensions: Vec<String>,
    pub supports_seek: bool,
    pub supports_volume: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct QueueItem {
    pub id: String,
    pub playlist_id: String,
    pub asset_id: String,
    pub path: String,
    pub filename: String,
    pub extension: String,
    pub status: String,
    pub status_reason: Option<String>,
    pub transient: bool,
    pub player_type_id: String,
    pub player_label: String,
    pub file_class: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DownloadProgress {
    pub phase: String,
    pub total: usize,
    pub completed: usize,
    pub failed: usize,
    pub current_song_name: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MembershipAction {
    pub playlist_id: String,
    pub label: String,
    pub checked: bool,
    pub toggle: bool,
}

pub enum PlayerMessage {
    SetCandidates(Vec<PlayerCandidate>),
    SetPreference { capability_id: String, plugin_id: Option<String> },
    PlayListed { item_id: Option<String> },
    PlayItem { item_id: String },
    PlayEntry { repo_id: String, kind: String, extension: String, asset_id: String, is_virtual: bool, path: String, filename: String },
    PlayNext { natural_end: bool },
    PlayPrevious,
    CycleMode,
    SetPlaying(bool),
    Seek(u64),
    SetVolume(f32),
    SetImageDuration(u32),
    SetObjectFit { cover: bool },
    ToggleQueue,
    /// `repo_id` 为空时停止当前会话。仓库切换要带上被换掉的仓库。
    Stop { repo_id: Option<String>, clear_stored: bool },
    ToggleMembership { playlist_id: String, kind: String, extension: String, asset_id: String, is_virtual: bool, path: String },
    MembershipsLoaded { repo_id: String, result: Result<BTreeMap<String, Vec<String>>, String> },
    MembershipSaved(Result<PlaylistMembershipSnapshot, String>),
    StartDownload(DownloaderPlaylistRequest),
    DownloadCompleted(Result<(serde_json::Value, Vec<serde_json::Value>), String>),
    CancelDownload,
    Reorder { source: String, before: Option<String> },
    OpenPreview,
    RestoreDetail(Result<PlaylistDetail, String>),
}

#[derive(Clone, Debug)]
pub enum PlayerEffect {
    PersistSettings,
    PersistSessions,
    PersistPreferences,
    LoadMemberships { repo_id: String },
    SetMembership(PlaylistMembershipRequest),
    AddByPaths(PlaylistItemsByPathsAddRequest),
    Reorder(PlaylistItemsOrderRequest),
    Download(DownloaderPlaylistRequest),
    RestoreDetail { repo_id: String, playlist_id: String },
}

#[derive(Clone, Debug)]
pub struct PlayerState {
    pub session: PlaybackSessionState,
    pub repo_id: Option<String>,
    pub queue: Vec<QueueItem>,
    pub current_id: Option<String>,
    pub mode: PlaybackMode,
    pub settings: PlaybackSettings,
    pub wants_playing: bool,
    pub can_play: bool,
    pub queue_open: bool,
    pub activity: String,
    pub notice: String,
    pub history: Vec<String>,
    pub shuffle_order: Vec<String>,
    pub shuffle_seed: u64,
    pub playlists: Vec<PlaylistSummary>,
    pub contributions: Vec<PlaylistPlayerContribution>,
    pub candidates: Vec<PlayerCandidate>,
    pub preferences: BTreeMap<String, String>,
    pub memberships: BTreeMap<String, Vec<String>>,
    pub listed: Option<PlaylistDetail>,
    pub download: DownloadProgress,
    download_playlist_id: Option<i64>,
    transient_seq: u64,
    restore_playlist_id: Option<String>,
    stored: BTreeMap<String, StoredSession>,
    effects: Vec<PlayerEffect>,
}

impl Default for PlayerState {
    fn default() -> Self {
        Self {
            session: fresh_session(""),
            repo_id: None,
            queue: Vec::new(),
            current_id: None,
            mode: PlaybackMode::ListLoop,
            settings: default_settings(),
            wants_playing: false,
            can_play: false,
            queue_open: false,
            activity: String::new(),
            notice: String::new(),
            history: Vec::new(),
            shuffle_order: Vec::new(),
            shuffle_seed: 1,
            playlists: Vec::new(),
            contributions: Vec::new(),
            candidates: Vec::new(),
            preferences: BTreeMap::new(),
            memberships: BTreeMap::new(),
            listed: None,
            download: DownloadProgress { phase: "idle".into(), ..DownloadProgress::default() },
            download_playlist_id: None,
            transient_seq: 0,
            restore_playlist_id: None,
            stored: BTreeMap::new(),
            effects: Vec::new(),
        }
    }
}

impl PlayerState {
    pub fn take_effects(&mut self) -> Vec<PlayerEffect> {
        std::mem::take(&mut self.effects)
    }

    pub fn load_default_files(&mut self) {
        self.load_files(&settings_path(), &sessions_path(), &preferences_path());
    }

    pub fn load_files(&mut self, settings: &Path, sessions: &Path, preferences: &Path) {
        self.settings = read_settings_file(settings);
        self.stored = read_sessions_file(sessions);
        self.preferences = read_preferences_file(preferences);
    }

    pub fn save_settings_file(&self, path: &Path) {
        write_json(path, &settings_json(&self.settings), "播放设置");
    }

    pub fn save_sessions_file(&self, path: &Path) {
        write_json(path, &sessions_json(&self.stored), "播放会话");
    }

    pub fn save_preferences_file(&self, path: &Path) {
        write_json(path, &preferences_json(&self.preferences), "播放器偏好");
    }

    /// 预览页装入音视频后，把那份会话交给播放条。
    pub fn adopt_session(&mut self, session: PlaybackSessionState) {
        self.session = session;
    }

    pub fn system_media_supported(&self) -> bool {
        let _ = self;
        system_media_session_available()
    }

    pub fn mode_text(&self) -> &'static str {
        mode_label(self.mode)
    }

    pub fn object_fit_text(&self) -> &'static str {
        if self.settings.object_fit_cover { "填充" } else { "适应" }
    }

    pub fn time_text(&self) -> String {
        format!("{} / {}", format_time(self.session.current_time_ms), format_time(self.session.duration_ms.unwrap_or(0)))
    }

    pub fn current_item(&self) -> Option<&QueueItem> {
        self.queue.iter().find(|item| Some(&item.id) == self.current_id.as_ref())
    }

    pub fn membership_actions(&self, kind: &str, extension: &str, asset_id: &str, is_virtual: bool) -> Vec<MembershipAction> {
        let toggle = membership_can_toggle(kind, asset_id, is_virtual);
        let current = self.memberships.get(asset_id).map(Vec::as_slice).unwrap_or(&[]);
        compatible_playlist_ids(kind, extension, &self.playlists, &self.contributions, &self.candidates)
            .into_iter()
            .filter_map(|playlist_id| {
                let playlist = self.playlists.iter().find(|playlist| playlist.playlist_id == playlist_id)?;
                let checked = toggle && current.iter().any(|id| id == &playlist_id);
                let label = if checked { format!("移出 {}", playlist.name) } else { format!("加入 {}", playlist.name) };
                Some(MembershipAction { playlist_id, label, checked, toggle })
            })
            .collect()
    }

    pub fn download_text(&self) -> String {
        match self.download.phase.as_str() {
            "idle" | "" => String::new(),
            "submitting" => "正在提交播放列表下载…".into(),
            "start" | "track" => format!(
                "正在下载 {} / {}，失败 {}",
                self.download.completed,
                self.download.total,
                self.download.failed
            ),
            "complete" => format!("下载完成，成功 {}，失败 {}", self.download.completed, self.download.failed),
            "error" => self.download.error.clone().unwrap_or_else(|| "播放列表下载失败".into()),
            other => format!("下载 {other}"),
        }
    }

    fn note_playlists(&mut self, repo_id: &str, playlists: &[PlaylistSummary]) {
        self.playlists = playlists.to_vec();
        if playlists.is_empty() {
            self.memberships.clear();
            return;
        }
        self.effects.push(PlayerEffect::LoadMemberships { repo_id: repo_id.to_string() });
        let Some(stored) = self.stored.get(repo_id).cloned() else {
            return;
        };
        if self.repo_id.as_deref() == Some(repo_id) {
            return;
        }
        if playlists.iter().any(|playlist| playlist.playlist_id == stored.playlist_id) {
            self.restore_playlist_id = Some(stored.playlist_id.clone());
            self.effects.push(PlayerEffect::RestoreDetail { repo_id: repo_id.to_string(), playlist_id: stored.playlist_id });
        }
    }

    fn note_players(&mut self, players: Vec<PlaylistPlayerContribution>) {
        self.contributions = players;
    }

    fn note_detail(&mut self, detail: &PlaylistDetail) {
        if self.restore_playlist_id.as_deref() == Some(detail.playlist.playlist_id.as_str()) {
            self.restore_playlist_id = None;
            self.restore(detail);
            return;
        }
        let same_playlist = self.listed.as_ref().is_some_and(|listed| listed.playlist.playlist_id == detail.playlist.playlist_id)
            || self.queue.iter().any(|item| item.playlist_id == detail.playlist.playlist_id);
        self.listed = Some(detail.clone());
        if same_playlist {
            self.sync_queue(detail);
        }
    }

    fn restore(&mut self, detail: &PlaylistDetail) {
        let repo_id = detail.playlist.repo_id.clone();
        let Some(stored) = self.stored.get(&repo_id).cloned() else {
            self.clear_stored(&repo_id);
            return;
        };
        if !self.session_can_restore(&stored, detail) {
            eprintln!("Nana 播放会话无法恢复：{}", stored.playlist_id);
            self.clear_stored(&repo_id);
            return;
        }
        self.listed = Some(detail.clone());
        self.repo_id = Some(repo_id);
        self.mode = stored.mode;
        self.wants_playing = stored.is_playing;
        self.session.volume = stored.volume;
        self.session.current_time_ms = stored.current_time_ms;
        self.session.duration_ms = Some(stored.duration_ms);
        self.queue = detail.items.iter().map(|item| queue_item_from_playlist(item, &detail.playlist)).collect();
        self.play_item(&stored.current_item_id, stored.is_playing);
    }

    fn session_can_restore(&self, stored: &StoredSession, detail: &PlaylistDetail) -> bool {
        if detail.playlist.playlist_id != stored.playlist_id {
            return false;
        }
        let known = self.candidates.iter().any(|candidate| candidate.player_type_id == stored.player_type_id)
            || self.contributions.iter().any(|contribution| contribution.player_type_id == stored.player_type_id);
        let ready = detail.items.iter().any(|item| item.playlist_item_id == stored.current_item_id && item.status == "ready");
        known && ready
    }

    fn play_listed(&mut self, item_id: Option<String>, inspect: &mut InspectState) {
        let Some(detail) = self.listed.clone() else {
            self.activity = "选择一个播放集".into();
            return;
        };
        if detail.items.is_empty() {
            self.activity = "播放集还是空的".into();
            return;
        }
        let repo_id = detail.playlist.repo_id.clone();
        if self.repo_id.as_ref().is_some_and(|current| current != &repo_id) {
            self.stop_runtime(true, inspect);
        }
        self.repo_id = Some(repo_id);
        self.queue = detail.items.iter().map(|item| queue_item_from_playlist(item, &detail.playlist)).collect();
        self.history.clear();
        self.shuffle_order.clear();
        let start = item_id
            .or_else(|| detail.items.iter().find(|item| item.status == "ready").map(|item| item.playlist_item_id.clone()))
            .or_else(|| detail.items.first().map(|item| item.playlist_item_id.clone()));
        let Some(start) = start else {
            self.activity = "播放集还是空的".into();
            return;
        };
        if self.mode == PlaybackMode::Shuffle {
            self.ensure_shuffle();
        }
        self.play_item(&start, true);
        self.publish(inspect);
    }

    /// 选中队列项并尝试装载。装载失败时会话保持 failed，不会变成 playing。
    fn play_item(&mut self, item_id: &str, auto_play: bool) {
        let Some(item) = self.queue.iter().find(|item| item.id == item_id).cloned() else {
            eprintln!("Nana 播放队列没有这个条目：{item_id}");
            self.activity = "当前没有可播放条目".into();
            return;
        };
        self.current_id = Some(item.id.clone());
        if self.history.last().map(String::as_str) != Some(item.id.as_str()) {
            self.history.push(item.id.clone());
        }
        self.wants_playing = auto_play;
        self.load_item(&item);
        self.persist_if_needed();
    }

    fn load_item(&mut self, item: &QueueItem) {
        self.session.repo_id = self.repo_id.clone().unwrap_or_default();
        self.session.playlist_id = item.playlist_id.clone();
        self.session.playlist_item_id = Some(item.id.clone());
        self.session.session_id = format!("playback-{}", self.session.repo_id);
        if item.status != "ready" {
            self.fail_session(item.status_reason.clone().unwrap_or_else(|| "当前条目不可播放".into()));
            return;
        }
        let resolution = self.resolve_type(&item.player_type_id);
        self.notice = resolution_notice(&resolution);
        if resolution.player.is_none() {
            let contributed = self.contributions.iter().any(|contribution| contribution.player_type_id == item.player_type_id);
            // 有 Vue 贡献但没有原生候选时直接失败，不能再去问解码器，否则文案会变成「没有原生解码器」。
            let message = if contributed {
                "播放运行时仍是 Vue 插件，需要升级为 Nana 原生播放贡献".to_string()
            } else if self.notice.is_empty() {
                "缺少对应播放插件".into()
            } else {
                self.notice.clone()
            };
            eprintln!("Nana 播放插件不可用：{message}");
            self.fail_session(message);
            return;
        }
        self.activity.clear();
        let mut controller = PlaybackSessionController::new(MissingDecoder, self.session.clone());
        if let Err(error) = controller.load(&item.path) {
            eprintln!("Nana 播放装载失败：{error}");
        }
        self.session = controller.state().clone();
        self.can_play = self.session.status != "failed";
        if item.file_class == "image" {
            self.apply_image_duration();
        }
        if self.session.status == "failed" {
            self.activity = self.session.error.clone().unwrap_or_else(|| "没有原生解码器".into());
        }
    }

    fn play_entry(&mut self, repo_id: &str, kind: &str, extension: &str, asset_id: &str, path: &str, filename: &str, inspect: &mut InspectState) {
        if kind == "directory" {
            eprintln!("Nana 目录不能作为临时播放项：{path}");
            self.activity = "没有可用于播放此媒体的插件".into();
            return;
        }
        let extension = if extension.trim().is_empty() {
            filename.rsplit('.').next().unwrap_or("").to_ascii_lowercase()
        } else {
            extension.to_ascii_lowercase()
        };
        let matched = find_player_for_extension(&extension, &self.candidates, &self.contributions);
        let Some((player_type_id, label, file_class, upgrade)) = (match matched {
            Some(PlayerMatch::Native(candidate)) => Some((candidate.player_type_id.clone(), candidate.label.clone(), candidate.file_class.clone(), false)),
            Some(PlayerMatch::Contribution(contribution)) => {
                Some((contribution.player_type_id.clone(), contribution.label.clone(), contribution.file_class.clone(), true))
            }
            None => None,
        }) else {
            eprintln!("Nana 没有可用于播放此媒体的插件：{path}");
            self.activity = "没有可用于播放此媒体的插件".into();
            return;
        };
        if self.repo_id.as_ref().is_some_and(|current| current != repo_id) {
            self.stop_runtime(false, inspect);
            self.listed = None;
            self.current_id = None;
            self.queue.clear();
            self.history.clear();
            self.shuffle_order.clear();
        }
        self.repo_id = Some(repo_id.to_string());
        let item = self.transient_item(path, filename, &extension, asset_id, &player_type_id, &label, &file_class);
        let current = self.current_id.clone();
        self.queue.retain(|queue_item| !queue_item.transient || queue_item.path != path);
        if let Some(index) = current.as_ref().and_then(|id| self.queue.iter().position(|queue_item| &queue_item.id == id)) {
            self.queue.insert(index + 1, item.clone());
            if self.mode == PlaybackMode::Shuffle {
                let mut order: Vec<String> = self.shuffle_order.iter().filter(|id| self.queue.iter().any(|queue_item| &queue_item.id == *id) || id.as_str() == item.id).cloned().collect();
                if let Some(order_index) = order.iter().position(|id| Some(id) == current.as_ref()) {
                    order.insert(order_index + 1, item.id.clone());
                    self.shuffle_order = order;
                }
            }
        } else {
            self.queue.push(item.clone());
        }
        self.play_item(&item.id, true);
        if upgrade {
            self.fail_session("播放运行时仍是 Vue 插件，需要升级为 Nana 原生播放贡献".into());
            eprintln!("Nana 播放运行时需要升级：{path}");
        }
        self.publish(inspect);
    }

    fn play_next(&mut self, natural_end: bool, inspect: &mut InspectState) {
        let previous_transient = self.current_item().is_some_and(|item| item.transient);
        let next = self.resolve_next(natural_end);
        let Some(next) = next else {
            self.wants_playing = false;
            if natural_end {
                self.prune_transients();
            }
            self.persist_if_needed();
            self.publish(inspect);
            return;
        };
        self.play_item(&next, true);
        if natural_end || previous_transient {
            self.prune_transients();
        }
        self.publish(inspect);
    }

    fn resolve_next(&mut self, natural_end: bool) -> Option<String> {
        let ready = ready_ids(&self.queue);
        match next_ready_id(self.mode, self.current_id.as_deref(), &ready, &self.shuffle_order, natural_end) {
            NextStep::None => None,
            NextStep::Item(id) => Some(id),
            NextStep::ReshuffleMissing => {
                self.ensure_shuffle();
                let ready = ready_ids(&self.queue);
                match next_ready_id(self.mode, self.current_id.as_deref(), &ready, &self.shuffle_order, natural_end) {
                    NextStep::Item(id) => Some(id),
                    NextStep::ReshuffleWrap => {
                        self.ensure_shuffle();
                        self.shuffle_order.first().cloned()
                    }
                    NextStep::None | NextStep::ReshuffleMissing => self.shuffle_order.first().cloned(),
                }
            }
            NextStep::ReshuffleWrap => {
                self.ensure_shuffle();
                self.shuffle_order.first().cloned()
            }
        }
    }

    fn play_previous(&mut self, inspect: &mut InspectState) {
        let ready = ready_ids(&self.queue);
        let Some(previous) = previous_ready_id(self.mode, self.current_id.as_deref(), &ready, &self.history) else {
            return;
        };
        self.history.pop();
        self.play_item(&previous, true);
        self.publish(inspect);
    }

    fn set_playing(&mut self, playing: bool, inspect: &mut InspectState) {
        self.wants_playing = playing;
        let mut controller = PlaybackSessionController::new(MissingDecoder, self.session.clone());
        let result = if playing { controller.play() } else { controller.pause() };
        if let Err(error) = result {
            eprintln!("Nana 播放控制失败：{error}");
        }
        self.session = controller.state().clone();
        self.can_play = self.session.status != "failed" && self.current_item().is_some();
        if self.session.status == "failed" {
            self.activity = self.session.error.clone().unwrap_or_default();
        }
        self.persist_if_needed();
        self.publish(inspect);
    }

    fn seek(&mut self, position_ms: u64, inspect: &mut InspectState) {
        let mut controller = PlaybackSessionController::new(MissingDecoder, self.session.clone());
        if let Err(error) = controller.seek(position_ms) {
            eprintln!("Nana 播放进度失败：{error}");
        }
        self.session = controller.state().clone();
        self.persist_if_needed();
        self.publish(inspect);
    }

    fn set_volume(&mut self, volume: f32, inspect: &mut InspectState) {
        let volume = volume.clamp(0.0, 1.0);
        let mut controller = PlaybackSessionController::new(MissingDecoder, self.session.clone());
        if let Err(error) = controller.set_volume(volume) {
            eprintln!("Nana 播放音量失败：{error}");
        }
        self.session = controller.state().clone();
        self.persist_if_needed();
        self.publish(inspect);
    }

    fn set_image_duration(&mut self, value: u32) {
        self.settings.image_duration_ms = support::normalize_image_duration_ms(Some(f64::from(value)));
        self.effects.push(PlayerEffect::PersistSettings);
        if self.current_item().is_some_and(|item| item.file_class == "image") {
            self.apply_image_duration();
            self.persist_if_needed();
        }
    }

    fn apply_image_duration(&mut self) {
        let duration = u64::from(self.settings.image_duration_ms);
        self.session.duration_ms = Some(duration);
        if self.session.current_time_ms > duration {
            self.session.current_time_ms = duration;
        }
    }

    fn cycle_mode(&mut self) {
        self.mode = cycle_mode(self.mode);
        if self.mode == PlaybackMode::Shuffle {
            self.ensure_shuffle();
        }
        self.persist_if_needed();
    }

    fn ensure_shuffle(&mut self) {
        self.shuffle_order = shuffle_order(self.current_id.as_deref(), &ready_ids(&self.queue), self.shuffle_seed);
        self.shuffle_seed = self.shuffle_seed.wrapping_add(1).max(1);
    }

    fn stop_for(&mut self, repo_id: Option<String>, clear_stored: bool, inspect: &mut InspectState) {
        let target = repo_id.clone().or_else(|| self.repo_id.clone());
        let applies = repo_id.as_ref().is_none_or(|repo_id| self.repo_id.as_ref() == Some(repo_id) || self.session.repo_id == *repo_id);
        if applies {
            self.stop_runtime(false, inspect);
        }
        if clear_stored {
            if let Some(repo_id) = target.filter(|repo_id| !repo_id.is_empty()) {
                self.clear_stored(&repo_id);
            }
        }
        self.publish(inspect);
    }

    /// 停掉运行时。`clear_stored_session` 为真时清掉当前仓库的持久会话，临时插播传假。
    fn stop_runtime(&mut self, clear_stored_session: bool, _inspect: &mut InspectState) {
        let previous_repo = self.repo_id.clone();
        self.wants_playing = false;
        self.can_play = false;
        self.queue_open = false;
        self.history.clear();
        self.shuffle_order.clear();
        self.activity.clear();
        self.notice.clear();
        if let Some(detail) = self.listed.clone() {
            self.queue = detail.items.iter().map(|item| queue_item_from_playlist(item, &detail.playlist)).collect();
        }
        self.session.status = "ended".into();
        self.session.current_time_ms = 0;
        self.session.duration_ms = None;
        self.session.error = None;
        self.session.can_seek = false;
        self.session.can_volume = false;
        if clear_stored_session {
            if let Some(repo_id) = previous_repo.filter(|repo_id| !repo_id.is_empty()) {
                self.clear_stored(&repo_id);
            }
        }
    }

    fn toggle_membership(
        &mut self,
        playlist_id: &str,
        kind: &str,
        extension: &str,
        asset_id: &str,
        is_virtual: bool,
        path: &str,
        writable: bool,
        repo_id: Option<&str>,
    ) {
        let compatible = compatible_playlist_ids(kind, extension, &self.playlists, &self.contributions, &self.candidates);
        if !compatible.iter().any(|id| id == playlist_id) {
            eprintln!("Nana 播放列表不兼容这个条目：{playlist_id}");
            return;
        }
        let Some(repo_id) = repo_id else {
            eprintln!("Nana 成员资格需要活动仓库");
            return;
        };
        if !writable {
            eprintln!("Nana 资源库不可写，已忽略播放列表成员变更");
            return;
        }
        if membership_can_toggle(kind, asset_id, is_virtual) {
            let current = self.memberships.get(asset_id).cloned().unwrap_or_default();
            let playlist_ids = next_membership_ids(&current, playlist_id);
            self.activity = "正在更新播放列表成员…".into();
            self.effects.push(PlayerEffect::SetMembership(PlaylistMembershipRequest {
                repo_id: repo_id.to_string(),
                asset_id: asset_id.to_string(),
                playlist_ids,
            }));
            return;
        }
        if path.trim().is_empty() {
            eprintln!("Nana 没有可加入播放列表的路径");
            return;
        }
        self.activity = "正在按路径加入播放列表…".into();
        self.effects.push(PlayerEffect::AddByPaths(PlaylistItemsByPathsAddRequest {
            repo_id: repo_id.to_string(),
            playlist_id: playlist_id.to_string(),
            paths: vec![path.to_string()],
        }));
    }

    fn note_memberships(&mut self, repo_id: &str, result: Result<BTreeMap<String, Vec<String>>, String>, active_repo: Option<&str>) {
        if active_repo != Some(repo_id) {
            eprintln!("Nana 忽略过期的播放列表成员：{repo_id}");
            return;
        }
        match result {
            Ok(memberships) => {
                self.memberships = memberships;
                self.activity.clear();
            }
            Err(error) => {
                eprintln!("Nana 读取播放列表成员失败：{error}");
                self.memberships.clear();
                self.activity = error;
            }
        }
    }

    fn note_membership_saved(&mut self, result: Result<PlaylistMembershipSnapshot, String>) {
        self.activity.clear();
        match result {
            Ok(snapshot) => {
                self.memberships.insert(snapshot.asset_id, snapshot.playlist_ids);
            }
            Err(error) => {
                eprintln!("Nana 更新播放列表成员失败：{error}");
                self.activity = error;
            }
        }
    }

    fn start_download(&mut self, request: DownloaderPlaylistRequest) {
        self.download_playlist_id = Some(request.playlist_id);
        self.download = DownloadProgress { phase: "submitting".into(), ..DownloadProgress::default() };
        self.activity = "正在提交播放列表下载…".into();
        self.effects.push(PlayerEffect::Download(request));
    }

    fn finish_download(&mut self, result: Result<(serde_json::Value, Vec<serde_json::Value>), String>) {
        match result {
            Ok((_output, events)) => {
                for event in events {
                    match serde_json::from_value::<DownloaderPlaylistProgressEvent>(event) {
                        Ok(event) => self.apply_download(event),
                        Err(error) => eprintln!("Nana 播放列表下载进度无法解析：{error}"),
                    }
                }
                if self.download.phase == "submitting" {
                    self.download.phase = "complete".into();
                    self.download_playlist_id = None;
                }
                self.activity = self.download_text();
            }
            Err(error) => {
                eprintln!("Nana 播放列表下载失败：{error}");
                self.download.phase = "error".into();
                self.download.error = Some(error.clone());
                self.download_playlist_id = None;
                self.activity = error;
            }
        }
    }

    fn apply_download(&mut self, event: DownloaderPlaylistProgressEvent) {
        if self.download_playlist_id != Some(event.playlist_id) {
            eprintln!("Nana 忽略过期的播放列表下载进度：{}", event.playlist_id);
            return;
        }
        self.download.phase = event.phase.clone();
        self.download.total = event.total;
        self.download.completed = event.completed;
        self.download.failed = event.failed;
        self.download.current_song_name = event.current_song_name;
        self.download.error = event.error;
        if event.phase == "complete" {
            self.download_playlist_id = None;
        }
    }

    fn reorder(&mut self, source: &str, before: Option<&str>, playlist_id: Option<&str>, ids: &[String], writable: bool, repo_id: Option<&str>) {
        let Some(playlist_id) = playlist_id else {
            eprintln!("Nana 排序需要先选中播放列表");
            return;
        };
        let Some(repo_id) = repo_id else {
            eprintln!("Nana 排序需要活动仓库");
            return;
        };
        if !writable {
            eprintln!("Nana 资源库不可写，已忽略播放列表排序");
            return;
        }
        let Some(item_ids) = reorder_before(ids, source, before) else {
            return;
        };
        self.activity = "正在保存播放列表顺序…".into();
        self.effects.push(PlayerEffect::Reorder(PlaylistItemsOrderRequest {
            repo_id: repo_id.to_string(),
            playlist_id: playlist_id.to_string(),
            item_ids,
        }));
    }

    fn sync_queue(&mut self, detail: &PlaylistDetail) {
        let mut queue = detail.items.iter().map(|item| queue_item_from_playlist(item, &detail.playlist)).collect::<Vec<_>>();
        let kept: Vec<_> = self
            .queue
            .iter()
            .filter(|item| item.transient && (self.current_id.as_ref() == Some(&item.id) || self.history.iter().any(|id| id == &item.id)))
            .cloned()
            .collect();
        queue.extend(kept);
        self.queue = queue;
        if self.mode == PlaybackMode::Shuffle {
            self.ensure_shuffle();
        }
    }

    fn prune_transients(&mut self) {
        let current = self.current_id.clone();
        let removed: Vec<String> = self
            .queue
            .iter()
            .filter(|item| item.transient && current.as_ref() != Some(&item.id))
            .map(|item| item.id.clone())
            .collect();
        if removed.is_empty() {
            return;
        }
        self.queue.retain(|item| !removed.iter().any(|id| id == &item.id));
        self.history.retain(|id| !removed.iter().any(|removed_id| removed_id == id));
        self.shuffle_order.retain(|id| !removed.iter().any(|removed_id| removed_id == id));
    }

    fn fail_session(&mut self, message: String) {
        self.can_play = false;
        self.session.status = "failed".into();
        self.session.error = Some(message.clone());
        self.activity = message;
    }

    fn set_preference(&mut self, capability_id: String, plugin_id: Option<String>) {
        let capability_id = capability_id.trim().to_string();
        if capability_id.is_empty() {
            eprintln!("Nana 播放器能力标识不能为空");
            self.activity = "播放器能力标识不能为空".into();
            return;
        }
        match plugin_id.map(|value| value.trim().to_string()).filter(|value| !value.is_empty()) {
            Some(plugin_id) => {
                self.preferences.insert(capability_id, plugin_id);
            }
            None => {
                self.preferences.remove(&capability_id);
            }
        }
        self.effects.push(PlayerEffect::PersistPreferences);
    }

    fn resolve_type(&self, player_type_id: &str) -> support::PlayerResolution {
        let candidates: Vec<_> = self.candidates.iter().filter(|candidate| candidate.player_type_id == player_type_id).cloned().collect();
        resolve_player(player_type_id, &candidates, &self.preferences)
    }

    fn transient_item(&mut self, path: &str, filename: &str, extension: &str, asset_id: &str, player_type_id: &str, label: &str, file_class: &str) -> QueueItem {
        self.transient_seq += 1;
        let asset = if asset_id.trim().is_empty() { path.to_string() } else { asset_id.to_string() };
        QueueItem {
            id: format!("transient:{asset}:{}", self.transient_seq),
            playlist_id: self.listed.as_ref().map(|detail| detail.playlist.playlist_id.clone()).unwrap_or_else(|| "__transient__".into()),
            asset_id: asset,
            path: path.to_string(),
            filename: filename.to_string(),
            extension: extension.to_string(),
            status: "ready".into(),
            status_reason: None,
            transient: true,
            player_type_id: player_type_id.to_string(),
            player_label: label.to_string(),
            file_class: file_class.to_string(),
        }
    }

    fn persist_if_needed(&mut self) {
        let Some(item) = self.current_item() else {
            return;
        };
        if item.transient {
            return;
        }
        let Some(repo_id) = self.repo_id.clone() else {
            return;
        };
        if item.playlist_id.is_empty() {
            return;
        }
        self.stored.insert(repo_id, StoredSession {
            repo_id: self.repo_id.clone().unwrap_or_default(),
            playlist_id: item.playlist_id.clone(),
            player_type_id: item.player_type_id.clone(),
            current_item_id: item.id.clone(),
            current_time_ms: self.session.current_time_ms,
            duration_ms: self.session.duration_ms.unwrap_or(0),
            mode: self.mode,
            volume: self.session.volume,
            is_playing: self.wants_playing,
        });
        self.effects.push(PlayerEffect::PersistSessions);
    }

    fn clear_stored(&mut self, repo_id: &str) {
        self.stored.remove(repo_id);
        self.effects.push(PlayerEffect::PersistSessions);
    }

    fn publish(&mut self, inspect: &mut InspectState) {
        inspect.replace_shared_media(self.session.clone());
    }

    fn open_preview(&mut self, model_panel: &mut WorkspacePanel, category: &mut LibraryCategory, selected: &mut Option<String>, inspect: &mut InspectState) {
        let Some(item) = self.current_item().cloned() else {
            eprintln!("Nana 没有正在播放的条目可以打开");
            return;
        };
        let repo_id = self.repo_id.clone().unwrap_or_default();
        *model_panel = WorkspacePanel::Files;
        *category = LibraryCategory::All;
        *selected = Some(item.path.clone());
        inspect.open_playlist_item(&item.path, &repo_id, &item.asset_id);
    }
}

struct MissingDecoder;

impl PlaybackMediaPlugin for MissingDecoder {
    fn load(&mut self, _source: &str) -> Result<PlaybackMediaCapabilities, String> {
        Err("没有原生解码器".into())
    }
    fn play(&mut self) -> Result<(), String> {
        Err("没有原生解码器".into())
    }
    fn pause(&mut self) -> Result<(), String> {
        Err("没有原生解码器".into())
    }
    fn seek(&mut self, _position_ms: u64) -> Result<(), String> {
        Err("没有原生解码器".into())
    }
    fn set_volume(&mut self, _volume: f32) -> Result<(), String> {
        Err("没有原生解码器".into())
    }
    fn dispose(&mut self) {}
}

fn fresh_session(repo_id: &str) -> PlaybackSessionState {
    PlaybackSessionState {
        session_id: format!("playback-{repo_id}"),
        repo_id: repo_id.to_string(),
        playlist_id: String::new(),
        playlist_item_id: None,
        status: "idle".into(),
        current_time_ms: 0,
        duration_ms: None,
        volume: 1.0,
        can_seek: false,
        can_volume: false,
        error: None,
        updated_at: String::new(),
    }
}

#[path = "player_reduce.rs"]
mod reduce;
pub(crate) use reduce::reduce_message;

impl ShellViewModel {
    pub(super) fn player_surface_visible(&self) -> bool {
        !self.acceptance_scene
            && self.workspace.startup.status == StartupStatus::Ready
            && self.workspace.main_region() == MainRegion::HasRepository
            && matches!(self.workspace.panel, WorkspacePanel::Files | WorkspacePanel::Playlist)
    }
}

#[cfg(test)]
#[path = "player_tests.rs"]
mod tests;
