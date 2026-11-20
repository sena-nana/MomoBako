//! 播放列表、播放条和共用播放会话。
//!
//! 成员资格、下载进度、播放器回退、会话持久化和仓库切换停止都在这里归约。
//! 播放条和预览页共用一份 `PlaybackSessionState`。没有原生解码器时停在失败态。

use std::collections::BTreeMap;
use std::path::Path;

use super::ShellViewModel;
use super::inspect::InspectState;
use super::workspace::{LibraryCategory, MainRegion, StartupStatus, WorkspacePanel};
use crate::backend::services::repository::{
    PlaybackSessionState, PlaylistDetail, PlaylistItemsByPathsAddRequest,
    PlaylistItemsOrderRequest, PlaylistMembershipRequest, PlaylistMembershipSnapshot,
    PlaylistPlayerContribution, PlaylistSummary,
};

mod clip;
mod support;
#[cfg(test)]
pub(crate) use clip::fulfill_loads;
pub(crate) use clip::{LoadedItem, StillShow, decode_loaded, decode_media};
mod wav_player;
pub(crate) use support::{
    AUDIO_CAPABILITY, AUDIO_SEQUENCE_TYPE, builtin_audio_formats, capability_id, resolution_notice,
    resolve_player,
};
use support::{
    NextStep, PlayerMatch, StoredSession, cycle_mode, default_settings, find_player_for_extension,
    format_time, mode_label, next_ready_id, preferences_json, previous_ready_id,
    queue_item_from_playlist, read_preferences_file, read_sessions_file, read_settings_file,
    ready_ids, sessions_json, settings_json, shuffle_order, system_media_session_available,
    write_json,
};
pub use support::{preferences_path, sessions_path, settings_path};
pub use wav_player::PreviewPcm;
pub(crate) use wav_player::pcm_from_bytes;
#[cfg(test)]
pub(crate) use wav_player::sound_device_compiled_in;

mod transport;

/// 列表循环、随机、单曲循环，循环按钮按这个顺序切换。
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

/// 已登记的播放器实现。内置 WAV，以及解成 PCM 后共用游标的 mp3/flac/ogg 和 Windows 上的 m4a/aac/opus。
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
    pub thumbnail_path: Option<String>,
    /// 在线条目（网易云这类来源的单曲）：本地没有文件，装载前先准备播放源（`playback_prep.rs`）。
    pub is_virtual: bool,
    /// 已经落在本机的文件。预览页打开和定位用它，不拼仓库根。
    pub local_absolute_path: Option<String>,
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
    SetPreference {
        capability_id: String,
        plugin_id: Option<String>,
    },
    PlayListed {
        item_id: Option<String>,
    },
    PlayItem {
        item_id: String,
    },
    PlayNext {
        natural_end: bool,
    },
    PlayPrevious,
    CycleMode,
    SetPlaying(bool),
    Seek(u64),
    SetVolume(f32),
    SetImageDuration(u32),
    SetObjectFit {
        cover: bool,
    },
    ToggleQueue,
    /// 播放条卡片排版后的宽度。只记下来；三行排版只看窗口是不是不宽于 1120。
    BarResized(f32),
    /// `repo_id` 为空时停止当前会话。仓库切换要带上被换掉的仓库。
    Stop {
        repo_id: Option<String>,
        clear_stored: bool,
    },
    ToggleMembership {
        playlist_id: String,
        kind: String,
        extension: String,
        asset_id: String,
        is_virtual: bool,
        path: String,
    },
    MembershipsLoaded {
        repo_id: String,
        result: Result<BTreeMap<String, Vec<String>>, String>,
    },
    MembershipSaved(Result<PlaylistMembershipSnapshot, String>),
    /// 按路径加入（目录、虚拟条目）以后读回的那张播放集。不一定是页面上点开的那张，不能当成「点开了它」。
    PathsAdded(Result<PlaylistDetail, String>),
    /// 播放集页移除或排序交回的那张。先换上这份详情，再重读播放集列表和成员索引。
    ItemsChanged(Result<PlaylistDetail, String>),
    Reorder {
        source: String,
        before: Option<String>,
    },
    OpenPreview {
        item_id: Option<String>,
    },
    RestoreDetail(Result<PlaylistDetail, String>),
    /// 当前项经仓库服务读出并解码后的结果。`still` 回带请求时的类别；`prep` 是在线条目准备播放源的结果
    /// （后端写下的访问、失败能不能重试），本地文件是默认值。
    ItemLoaded {
        item_id: String,
        generation: u64,
        still: bool,
        result: Result<LoadedItem, String>,
        prep: super::SourcePrep,
    },
}

#[derive(Clone, Debug)]
pub enum PlayerEffect {
    PersistSettings,
    PersistSessions,
    PersistPreferences,
    LoadMemberships {
        repo_id: String,
    },
    SetMembership(PlaylistMembershipRequest),
    AddByPaths(PlaylistItemsByPathsAddRequest),
    Reorder(PlaylistItemsOrderRequest),
    RestoreDetail {
        repo_id: String,
        playlist_id: String,
    },
    /// 经仓库服务读出当前项的字节。`still` 为真时按图片解码，否则按音视频。`virtual_entry` 是在线条目：
    /// 先经 `momobako.playback.prepare` 准备播放源，再读准备好的本地文件。
    LoadItem {
        repo_id: String,
        item_id: String,
        path: String,
        extension: String,
        still: bool,
        generation: u64,
        virtual_entry: bool,
    },
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
    /// 正在放的播放集的播放器类型。没有当前项时，播放条封面的类型字仍按它认。
    pub(crate) active_player_type_id: Option<String>,
    /// 播放条卡片的实际宽度，排版回报后才有；0 表示还没量到。三行排版不看它。
    pub(crate) bar_width: f32,
    pub notice: String,
    pub history: Vec<String>,
    pub shuffle_order: Vec<String>,
    pub shuffle_seed: u64,
    pub playlists: Vec<PlaylistSummary>,
    /// `playlists` 属于哪个仓库。恢复会话按它找存下的会话。
    playlists_repo_id: Option<String>,
    pub contributions: Vec<PlaylistPlayerContribution>,
    /// 插件登记的播放器类型读回过一次。读回之前认不出插件类型，不能据此丢掉存下的会话。
    players_loaded: bool,
    pub candidates: Vec<PlayerCandidate>,
    pub preferences: BTreeMap<String, String>,
    pub memberships: BTreeMap<String, Vec<String>>,
    pub listed: Option<PlaylistDetail>,
    /// 已读回的播放集详情，键是 `仓库:播放集`。点开时先画这一份，再拉新的。
    detail_cache: BTreeMap<String, PlaylistDetail>,
    transient_seq: u64,
    restore_playlist_id: Option<String>,
    stored: BTreeMap<String, StoredSession>,
    effects: Vec<PlayerEffect>,
    /// 没有就近显示的失败（播放控制、播放集成员），归约结束时交给状态区。
    failures: Vec<String>,
    wav: wav_player::WavPlayer,
    /// 预览页刚接管当前项时是它的路径；离开预览页后清空，临时条目留作当前项。
    preview_path: String,
    /// 当前项解出的画面。没有当前项或不是视频时为空，不保留整队画面。
    clip_frames: Option<Vec<super::inspect::support::VideoFrame>>,
    /// 为真时，下一次发布才改预览画面。预览自己的帧不会被空的播放列表清掉。
    clip_owned: bool,
    /// 图片幻灯片的当前帧。读不到时没有帧，不编造画面。
    pub(crate) still: Option<StillShow>,
    /// 扩展名不在音视频会话里、也不是图片时的缺数据说明。
    pub(crate) outside_note: Option<String>,
    /// 当前项读取请求的代次。结果代次对不上就丢弃。
    load_generation: u64,
    /// PCM 正装在共用游标里的条目。播放、暂停、跳转和音量只在它是当前项时驱动游标。
    cursor_item: Option<String>,
    /// 已经读好的条目（音视频或图片，有没有 PCM 都算）。是当前项时控制直接生效。
    loaded_item: Option<String>,
    /// 恢复会话时要接着放的条目和位置。这一项装好、又能跳转时跳过去，换了条目就作废。
    resume: Option<(String, u64)>,
    /// 播放源准备失败、可以重试的条目（在线条目登录失效、网络不通……）。它还是当前项时播放条的「播放」可点，
    /// 点了重新准备；换了条目、停止或装好了就作废。
    retry_item: Option<String>,
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
            active_player_type_id: None,
            bar_width: 0.0,
            notice: String::new(),
            history: Vec::new(),
            shuffle_order: Vec::new(),
            shuffle_seed: 1,
            playlists: Vec::new(),
            playlists_repo_id: None,
            contributions: Vec::new(),
            players_loaded: false,
            candidates: wav_player::builtin_candidates(),
            preferences: BTreeMap::new(),
            memberships: BTreeMap::new(),
            listed: None,
            detail_cache: BTreeMap::new(),
            transient_seq: 0,
            restore_playlist_id: None,
            stored: BTreeMap::new(),
            effects: Vec::new(),
            failures: Vec::new(),
            wav: wav_player::WavPlayer::default(),
            preview_path: String::new(),
            clip_frames: None,
            clip_owned: false,
            still: None,
            outside_note: None,
            load_generation: 0,
            cursor_item: None,
            loaded_item: None,
            resume: None,
            retry_item: None,
        }
    }
}

impl PlayerState {
    pub fn take_effects(&mut self) -> Vec<PlayerEffect> {
        std::mem::take(&mut self.effects)
    }

    /// 记下一次没有就近显示的失败，排着等状态区取走。
    pub(super) fn note_failure(&mut self, message: String) {
        self.failures.push(message);
    }

    /// 取走排着的失败。
    pub(crate) fn take_failures(&mut self) -> Vec<String> {
        std::mem::take(&mut self.failures)
    }

    /// 取走后没有处理的副作用放回队首，顺序不变。
    pub(crate) fn requeue_effects(&mut self, effects: Vec<PlayerEffect>) {
        self.effects.splice(0..0, effects);
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
        if self.settings.object_fit_cover {
            "填充"
        } else {
            "适应"
        }
    }

    pub fn time_text(&self) -> String {
        format!(
            "{} / {}",
            format_time(self.session.current_time_ms),
            format_time(self.session.duration_ms.unwrap_or(0))
        )
    }

    /// 播放条认播放器类别用的类型。
    ///
    /// 有当前项时用这一项的类型。没有当前项时退回正在放的播放集的类型。
    /// 空字符串认不出播放器，封面写「媒体」。
    pub(crate) fn bar_player_type_id(&self) -> Option<&str> {
        if self.current_item().is_some() {
            return self
                .current_item()
                .map(|item| item.player_type_id.as_str())
                .filter(|id| !id.is_empty());
        }
        self.active_player_type_id
            .as_deref()
            .filter(|id| !id.is_empty())
    }

    pub fn current_item(&self) -> Option<&QueueItem> {
        self.queue
            .iter()
            .find(|item| Some(&item.id) == self.current_id.as_ref())
    }

    /// 换上这个仓库的播放集列表：读成员索引，再看存下的会话能不能恢复。
    pub(super) fn note_playlists(&mut self, repo_id: &str, playlists: &[PlaylistSummary]) {
        self.playlists = playlists.to_vec();
        self.playlists_repo_id = Some(repo_id.to_string());
        if playlists.is_empty() {
            self.memberships.clear();
            return;
        }
        self.effects.push(PlayerEffect::LoadMemberships {
            repo_id: repo_id.to_string(),
        });
        self.queue_restore();
    }

    /// 插件登记的播放器类型读回：换上贡献，再看存下的会话能不能恢复。
    fn note_players(&mut self, players: Vec<PlaylistPlayerContribution>) {
        self.contributions = players;
        self.players_loaded = true;
        self.queue_restore();
    }

    /// 活动仓库或播放集列表变了以后恢复会话：存下的会话所在的播放集在列表里、
    /// 播放器还没在放这个仓库时读它的详情，详情回来再由 [`Self::restore`] 决定恢复还是丢弃。
    /// 播放器类型要先认得：内置候选当场认得，插件类型等播放器类型读回；读回了还不认得就丢掉会话。
    /// 已经在读的详情不重复读。
    fn queue_restore(&mut self) {
        let Some(repo_id) = self.playlists_repo_id.clone() else {
            return;
        };
        if self.restore_playlist_id.is_some() || self.repo_id.as_deref() == Some(repo_id.as_str()) {
            return;
        }
        let Some(stored) = self.stored.get(&repo_id).cloned() else {
            return;
        };
        if !self
            .playlists
            .iter()
            .any(|playlist| playlist.playlist_id == stored.playlist_id)
        {
            return;
        }
        if !self.player_type_known(&stored.player_type_id) {
            if self.players_loaded {
                eprintln!(
                    "Nana 存下的播放会话用的播放器已经不在：{}",
                    stored.player_type_id
                );
                self.clear_stored(&repo_id);
            }
            return;
        }
        self.restore_playlist_id = Some(stored.playlist_id.clone());
        self.effects.push(PlayerEffect::RestoreDetail {
            repo_id,
            playlist_id: stored.playlist_id,
        });
    }

    /// 内置候选或插件登记的播放器里有这个类型。
    fn player_type_known(&self, player_type_id: &str) -> bool {
        self.candidates
            .iter()
            .any(|candidate| candidate.player_type_id == player_type_id)
            || self
                .contributions
                .iter()
                .any(|contribution| contribution.player_type_id == player_type_id)
    }

    /// 记下读回的播放集。返回 `true` 表示这次走了会话恢复，调用方不要再从头播放。
    ///
    /// 播放队列只在它本来就是这一张时跟着换。正在放另一张时，刷新页面上的这一张不把播放队列换掉：
    /// 播放队列跟播放器走，不跟点开的页面走。
    fn note_detail(&mut self, detail: &PlaylistDetail) -> bool {
        // 读回的是真详情才进缓存。失败不会走到这里，不会把失败写成一份播放集。
        self.remember_playlist_detail(detail);
        if self.restore_playlist_id.as_deref() == Some(detail.playlist.playlist_id.as_str()) {
            self.restore_playlist_id = None;
            self.restore(detail);
            return true;
        }
        let playback_is_this = self
            .queue
            .iter()
            .any(|item| !item.transient && item.playlist_id == detail.playlist.playlist_id);
        self.listed = Some(detail.clone());
        if playback_is_this {
            self.sync_queue(detail);
        }
        false
    }

    fn cycle_mode(&mut self) {
        self.mode = cycle_mode(self.mode);
        if self.mode == PlaybackMode::Shuffle {
            self.ensure_shuffle();
        }
        self.persist_if_needed();
    }

    fn ensure_shuffle(&mut self) {
        self.shuffle_order = shuffle_order(
            self.current_id.as_deref(),
            &ready_ids(&self.queue),
            self.shuffle_seed,
        );
        self.shuffle_seed = self.shuffle_seed.wrapping_add(1).max(1);
    }

    fn stop_for(
        &mut self,
        repo_id: Option<String>,
        clear_stored: bool,
        inspect: &mut InspectState,
    ) {
        let target = repo_id.clone().or_else(|| self.repo_id.clone());
        let applies = repo_id.as_ref().is_none_or(|repo_id| {
            self.repo_id.as_ref() == Some(repo_id) || self.session.repo_id == *repo_id
        });
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
        self.wav.clear();
        self.cursor_item = None;
        self.loaded_item = None;
        self.retry_item = None;
        self.preview_path.clear();
        self.load_generation = self.load_generation.wrapping_add(1);
        self.drop_clip_frames();
        let previous_repo = self.repo_id.clone();
        self.wants_playing = false;
        self.can_play = false;
        self.queue_open = false;
        self.history.clear();
        self.shuffle_order.clear();
        self.notice.clear();
        if let Some(detail) = self.listed.clone() {
            self.queue = detail
                .items
                .iter()
                .map(|item| queue_item_from_playlist(item, &detail.playlist))
                .collect();
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

    fn sync_queue(&mut self, detail: &PlaylistDetail) {
        let mut queue = detail
            .items
            .iter()
            .map(|item| queue_item_from_playlist(item, &detail.playlist))
            .collect::<Vec<_>>();
        let kept: Vec<_> = self
            .queue
            .iter()
            .filter(|item| {
                item.transient
                    && (self.current_id.as_ref() == Some(&item.id)
                        || self.history.iter().any(|id| id == &item.id))
            })
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
        self.queue
            .retain(|item| !removed.iter().any(|id| id == &item.id));
        self.history
            .retain(|id| !removed.iter().any(|removed_id| removed_id == id));
        self.shuffle_order
            .retain(|id| !removed.iter().any(|removed_id| removed_id == id));
    }

    fn fail_session(&mut self, message: String) {
        self.can_play = false;
        self.session.status = "failed".into();
        self.session.error = Some(message);
    }

    fn set_preference(&mut self, capability_id: String, plugin_id: Option<String>) {
        let capability_id = capability_id.trim().to_string();
        if capability_id.is_empty() {
            eprintln!("Nana 播放器能力标识不能为空");
            return;
        }
        match plugin_id
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
        {
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
        let candidates: Vec<_> = self
            .candidates
            .iter()
            .filter(|candidate| candidate.player_type_id == player_type_id)
            .cloned()
            .collect();
        resolve_player(player_type_id, &candidates, &self.preferences)
    }

    fn transient_item(
        &mut self,
        entry: &PreviewEntry<'_>,
        filename: &str,
        extension: &str,
        player_type_id: &str,
        label: &str,
        file_class: &str,
    ) -> QueueItem {
        self.transient_seq += 1;
        let (path, asset_id) = (entry.path, entry.asset_id);
        let asset = if asset_id.trim().is_empty() {
            path.to_string()
        } else {
            asset_id.to_string()
        };
        QueueItem {
            id: format!("transient:{asset}:{}", self.transient_seq),
            playlist_id: self
                .listed
                .as_ref()
                .map(|detail| detail.playlist.playlist_id.clone())
                .unwrap_or_else(|| "__transient__".into()),
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
            thumbnail_path: None,
            is_virtual: entry.is_virtual,
            local_absolute_path: None,
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
        self.stored.insert(
            repo_id,
            StoredSession {
                repo_id: self.repo_id.clone().unwrap_or_default(),
                playlist_id: item.playlist_id.clone(),
                player_type_id: item.player_type_id.clone(),
                current_item_id: item.id.clone(),
                current_time_ms: self.session.current_time_ms,
                duration_ms: self.session.duration_ms.unwrap_or(0),
                mode: self.mode,
                volume: self.session.volume,
                is_playing: self.wants_playing,
            },
        );
        self.effects.push(PlayerEffect::PersistSessions);
    }

    fn clear_stored(&mut self, repo_id: &str) {
        self.stored.remove(repo_id);
        self.effects.push(PlayerEffect::PersistSessions);
    }

    fn publish(&mut self, inspect: &mut InspectState) {
        clip::publish_clip(self, inspect);
    }

    /// 打开这一条的预览。返回是否找到了条目。
    ///
    /// 页和「立刻进入预览」由归约补上：这里只切到文件面板、全部分类，并记下路径、发起详情读取。
    fn open_preview(
        &mut self,
        item_id: Option<&str>,
        model_panel: &mut WorkspacePanel,
        category: &mut LibraryCategory,
        selected: &mut Option<String>,
        inspect: &mut InspectState,
    ) -> bool {
        let item = match item_id {
            Some(id) => support::listed_preview_item(&self.queue, self.listed.as_ref(), id),
            None => self.current_item().cloned(),
        };
        let Some(item) = item else {
            eprintln!("Nana 没有可打开预览的播放条目");
            return false;
        };
        let repo_id = self.repo_id.clone().unwrap_or_default();
        *model_panel = WorkspacePanel::Files;
        *category = LibraryCategory::All;
        *selected = Some(item.path.clone());
        inspect.open_playlist_item(&item.path, &repo_id, &item.asset_id);
        true
    }
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

mod preview;
mod reduce;
pub(crate) use preview::{ClockStep, PreviewEntry};
mod library;
pub(crate) use reduce::reduce_message;

impl ShellViewModel {
    /// 文件浏览面板和播放集页才有底栏播放条。回收站和智能文件夹也算文件浏览面板，
    /// 播放条留在列表下面，不因为换了面板藏起来。
    pub(super) fn player_surface_visible(&self) -> bool {
        self.workspace.startup.status == StartupStatus::Ready
            && self.workspace.main_region() == MainRegion::HasRepository
            && matches!(
                self.workspace.panel,
                WorkspacePanel::Files
                    | WorkspacePanel::Trash
                    | WorkspacePanel::SmartFolder
                    | WorkspacePanel::Playlist
            )
    }
}

#[cfg(test)]
mod clip_tests;
#[cfg(test)]
mod tests;
