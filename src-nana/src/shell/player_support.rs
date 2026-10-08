//! 播放列表的纯函数：设置钳制、会话读写、播放器回退、队列和成员资格。
//!
//! 这些函数不碰窗口，也不把失败的解码说成正在播放。

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::backend::services::repository::{PlaylistDetail, PlaylistItem, PlaylistPlayerContribution, PlaylistSummary};
use crate::settings;

use super::{PlaybackMode, PlaybackSettings, PlayerCandidate, QueueItem};

pub const AUDIO_CAPABILITY: &str = "momobako.player.audio";
pub const OFFICIAL_AUDIO_PLUGIN: &str = "momobako.player.audio";
pub const AUDIO_SEQUENCE_TYPE: &str = "momobako.playlist.audio-sequence";
pub const SETTINGS_KEY: &str = "momobako.playbackSettings";
pub const SESSION_KEY_PREFIX: &str = "momobako.playbackSession";
pub const PREFERENCE_KEY: &str = "momobako.playlistPlayerPreferences.v1";

/// 播放器解析结果。音频没有官方实现时，不会把未选择的第三方实现当成默认。
#[derive(Clone, Debug, PartialEq)]
pub struct PlayerResolution {
    pub player: Option<PlayerCandidate>,
    pub capability_id: String,
    pub preferred_plugin_id: Option<String>,
    pub fallback_used: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StoredSession {
    pub repo_id: String,
    pub playlist_id: String,
    pub player_type_id: String,
    pub current_item_id: String,
    pub current_time_ms: u64,
    pub duration_ms: u64,
    pub mode: PlaybackMode,
    pub volume: f32,
    pub is_playing: bool,
}

/// 正式 Windows 构建才注册系统媒体控件。测试构建不注册，避免碰到真系统会话。
pub fn system_media_session_available() -> bool {
    super::super::system_media::session_compiled_in()
}

pub fn default_settings() -> PlaybackSettings {
    PlaybackSettings { image_duration_ms: 5000, object_fit_cover: false }
}

/// 图片停留钳在 2–30 秒。非有限数字回到 5 秒。
pub fn normalize_image_duration_ms(value: Option<f64>) -> u32 {
    let Some(value) = value.filter(|value| value.is_finite()) else {
        return 5000;
    };
    value.round().clamp(2000.0, 30000.0) as u32
}

pub fn normalize_object_fit(value: Option<&str>) -> bool {
    value == Some("cover")
}

pub fn normalize_mode(value: Option<&str>) -> PlaybackMode {
    match value {
        Some("shuffle") => PlaybackMode::Shuffle,
        Some("singleLoop") => PlaybackMode::SingleLoop,
        _ => PlaybackMode::ListLoop,
    }
}

pub fn normalize_volume(value: Option<f64>) -> f32 {
    value.filter(|value| value.is_finite()).map(|value| value.clamp(0.0, 1.0) as f32).unwrap_or(1.0)
}

pub fn playback_root() -> PathBuf {
    settings::default_path().parent().unwrap_or_else(|| Path::new(".")).to_path_buf()
}

pub fn settings_path() -> PathBuf {
    playback_root().join("playback-settings.json")
}

pub fn sessions_path() -> PathBuf {
    playback_root().join("playback-sessions.json")
}

pub fn preferences_path() -> PathBuf {
    playback_root().join("playlist-player-preferences.json")
}

pub fn read_settings_file(path: &Path) -> PlaybackSettings {
    match read_json(path, "播放设置", SETTINGS_KEY) {
        Some(value) => PlaybackSettings {
            image_duration_ms: normalize_image_duration_ms(value.get("imageDurationMs").and_then(|item| item.as_f64())),
            object_fit_cover: normalize_object_fit(value.get("objectFit").and_then(|item| item.as_str())),
        },
        None => default_settings(),
    }
}

pub fn settings_json(settings: &PlaybackSettings) -> serde_json::Value {
    serde_json::json!({
        "imageDurationMs": settings.image_duration_ms,
        "objectFit": if settings.object_fit_cover { "cover" } else { "contain" },
    })
}

pub fn read_sessions_file(path: &Path) -> BTreeMap<String, StoredSession> {
    let Some(value) = read_json(path, "播放会话", SESSION_KEY_PREFIX) else {
        return BTreeMap::new();
    };
    let Some(object) = value.as_object() else {
        eprintln!("Nana 播放会话文件不是对象");
        return BTreeMap::new();
    };
    object
        .iter()
        .filter_map(|(repo_id, value)| parse_session(repo_id, value).map(|session| (repo_id.clone(), session)))
        .collect()
}

pub fn sessions_json(sessions: &BTreeMap<String, StoredSession>) -> serde_json::Value {
    let mut object = serde_json::Map::new();
    for (repo_id, session) in sessions {
        object.insert(repo_id.clone(), serde_json::json!({
            "repoId": session.repo_id,
            "playlistId": session.playlist_id,
            "playerTypeId": session.player_type_id,
            "currentItemId": session.current_item_id,
            "currentTimeMs": session.current_time_ms,
            "durationMs": session.duration_ms,
            "mode": mode_name(session.mode),
            "volume": session.volume,
            "isPlaying": session.is_playing,
        }));
    }
    serde_json::Value::Object(object)
}

pub fn read_preferences_file(path: &Path) -> BTreeMap<String, String> {
    let Some(value) = read_json(path, "播放器偏好", PREFERENCE_KEY) else {
        return BTreeMap::new();
    };
    parse_preferences(&value)
}

pub fn preferences_json(preferences: &BTreeMap<String, String>) -> serde_json::Value {
    serde_json::Value::Object(preferences.iter().map(|(key, value)| (key.clone(), serde_json::Value::String(value.clone()))).collect())
}

/// 原子写入。失败时删除临时文件并记日志。
pub fn write_json(path: &Path, value: &serde_json::Value, label: &str) {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    if let Err(error) = fs::create_dir_all(parent) {
        eprintln!("Nana 创建{label}目录失败：{error}");
        return;
    }
    let raw = match serde_json::to_vec_pretty(value) {
        Ok(raw) => raw,
        Err(error) => {
            eprintln!("Nana 序列化{label}失败：{error}");
            return;
        }
    };
    let temporary = path.with_extension("json.new");
    if let Err(error) = fs::write(&temporary, raw) {
        eprintln!("Nana 写入{label}失败：{error}");
        return;
    }
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        eprintln!("Nana 提交{label}失败：{error}");
    }
}

pub fn capability_id(candidate: &PlayerCandidate) -> String {
    candidate
        .capability_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| default_capability(&candidate.player_type_id))
}

pub fn default_capability(player_type_id: &str) -> String {
    if player_type_id == AUDIO_SEQUENCE_TYPE {
        AUDIO_CAPABILITY.into()
    } else {
        format!("playlist-player:{player_type_id}")
    }
}

/// 按显式选择、官方默认的顺序解析。音频不会被未选择的第三方实现接管。
pub fn resolve_player(
    player_type_id: &str,
    candidates: &[PlayerCandidate],
    preferences: &BTreeMap<String, String>,
) -> PlayerResolution {
    let capability_id = resolve_capability(player_type_id, candidates);
    let preferred_plugin_id = preferences.get(&capability_id).cloned();
    if candidates.is_empty() {
        return PlayerResolution {
            player: None,
            capability_id,
            preferred_plugin_id: preferred_plugin_id.clone(),
            fallback_used: preferred_plugin_id.is_some(),
        };
    }
    let preferred = preferred_plugin_id.as_ref().and_then(|plugin_id| {
        candidates.iter().find(|candidate| &candidate.plugin_id == plugin_id).cloned()
    });
    let official = official_plugin(&capability_id).and_then(|plugin_id| {
        candidates.iter().find(|candidate| candidate.plugin_id == plugin_id).cloned()
    });
    let player = preferred.clone().or(official).or_else(|| {
        if capability_id == AUDIO_CAPABILITY {
            None
        } else {
            candidates.first().cloned()
        }
    });
    let fallback_used = preferred_plugin_id.as_ref().is_some_and(|plugin_id| player.as_ref().map(|item| &item.plugin_id) != Some(plugin_id));
    PlayerResolution { player, capability_id, preferred_plugin_id, fallback_used }
}

pub fn resolution_notice(resolution: &PlayerResolution) -> String {
    if resolution.fallback_used {
        let label = resolution.player.as_ref().map(|player| player.label.as_str()).unwrap_or("官方默认实现");
        return format!("所选播放器当前不可用，已回退到 {label}。");
    }
    if resolution.player.is_none() && resolution.capability_id == AUDIO_CAPABILITY {
        return "官方音频播放器未启用或缺失，音频播放暂不可用。".into();
    }
    if resolution.player.is_none() {
        return "缺少对应播放插件".into();
    }
    String::new()
}

/// 目录返回全部播放列表。没有扩展名的文件没有兼容列表。
pub fn compatible_playlist_ids(
    kind: &str,
    extension: &str,
    playlists: &[PlaylistSummary],
    contributions: &[PlaylistPlayerContribution],
    candidates: &[PlayerCandidate],
) -> Vec<String> {
    if kind == "directory" {
        return playlists.iter().map(|playlist| playlist.playlist_id.clone()).collect();
    }
    let extension = extension.trim().to_ascii_lowercase();
    if extension.is_empty() {
        return Vec::new();
    }
    playlists
        .iter()
        .filter(|playlist| type_supports_extension(&playlist.player_type_id, &extension, contributions, candidates))
        .map(|playlist| playlist.playlist_id.clone())
        .collect()
}

pub fn membership_can_toggle(kind: &str, asset_id: &str, is_virtual: bool) -> bool {
    kind == "file" && !asset_id.trim().is_empty() && !is_virtual
}

pub fn next_membership_ids(current: &[String], playlist_id: &str) -> Vec<String> {
    if current.iter().any(|item| item == playlist_id) {
        current.iter().filter(|item| item.as_str() != playlist_id).cloned().collect()
    } else {
        let mut next = current.to_vec();
        next.push(playlist_id.to_string());
        next
    }
}

/// `before` 为空时把来源移到末尾。来源不存在或顺序没变时返回空。
pub fn reorder_before(ids: &[String], source: &str, before: Option<&str>) -> Option<Vec<String>> {
    if !ids.iter().any(|id| id == source) {
        return None;
    }
    if before == Some(source) {
        return None;
    }
    let mut next: Vec<String> = ids.iter().filter(|id| id.as_str() != source).cloned().collect();
    match before {
        Some(target) => {
            let index = next.iter().position(|id| id == target)?;
            next.insert(index, source.to_string());
        }
        None => next.push(source.to_string()),
    }
    (next != ids).then_some(next)
}

/// 点到的播放条目。队列里有就用队列，否则从已打开的播放集拼一条，不开始播放。
pub fn listed_preview_item(queue: &[QueueItem], listed: Option<&PlaylistDetail>, id: &str) -> Option<QueueItem> {
    if let Some(item) = queue.iter().find(|item| item.id == id) {
        return Some(item.clone());
    }
    let detail = listed?;
    let item = detail.items.iter().find(|item| item.playlist_item_id == id)?;
    Some(queue_item_from_playlist(item, &detail.playlist))
}

pub fn queue_item_from_playlist(item: &PlaylistItem, playlist: &PlaylistSummary) -> QueueItem {
    QueueItem {
        id: item.playlist_item_id.clone(),
        playlist_id: item.playlist_id.clone(),
        asset_id: item.asset_id.clone(),
        path: item.path.clone(),
        filename: item.filename.clone(),
        extension: item.extension.to_ascii_lowercase(),
        status: item.status.clone(),
        status_reason: item.status_reason.clone(),
        transient: false,
        player_type_id: playlist.player_type_id.clone(),
        player_label: playlist.player_label.clone(),
        file_class: playlist.file_class.clone(),
        thumbnail_path: item.thumbnail_path.clone(),
    }
}

/// 只在音频或视频实现里按扩展名找播放器。和 Vue `findPlayerForEntry` 一样先找插件登记的
/// 播放器（条目上显示它的名字），都没有时才用内置的原生候选，保证没装插件也能播放。
pub fn find_player_for_extension<'a>(
    extension: &str,
    candidates: &'a [PlayerCandidate],
    contributions: &'a [PlaylistPlayerContribution],
) -> Option<PlayerMatch<'a>> {
    let extension = extension.trim().to_ascii_lowercase();
    if extension.is_empty() {
        return None;
    }
    if let Some(contribution) = contributions.iter().find(|contribution| {
        matches!(contribution.file_class.as_str(), "audio" | "video")
            && contribution.supported_extensions.iter().any(|item| item.eq_ignore_ascii_case(&extension))
    }) {
        return Some(PlayerMatch::Contribution(contribution));
    }
    candidates.iter().find(|candidate| {
        matches!(candidate.file_class.as_str(), "audio" | "video")
            && candidate.extensions.iter().any(|item| item.eq_ignore_ascii_case(&extension))
    }).map(PlayerMatch::Native)
}

pub enum PlayerMatch<'a> {
    Native(&'a PlayerCandidate),
    Contribution(&'a PlaylistPlayerContribution),
}

pub fn ready_ids(queue: &[QueueItem]) -> Vec<String> {
    queue.iter().filter(|item| item.status == "ready").map(|item| item.id.clone()).collect()
}

/// 当前项留在洗牌序列开头，其余用固定种子的 Fisher-Yates。
pub fn shuffle_order(current: Option<&str>, ready: &[String], seed: u64) -> Vec<String> {
    let mut rest: Vec<String> = ready.iter().filter(|id| Some(id.as_str()) != current).cloned().collect();
    let mut state = seed.max(1);
    for index in (1..rest.len()).rev() {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let next = ((state >> 16) as usize) % (index + 1);
        rest.swap(index, next);
    }
    match current {
        Some(id) if ready.iter().any(|item| item == id) => {
            let mut order = vec![id.to_string()];
            order.extend(rest);
            order
        }
        _ => rest,
    }
}

pub fn next_ready_id(
    mode: PlaybackMode,
    current: Option<&str>,
    ready: &[String],
    shuffle: &[String],
    natural_end: bool,
) -> NextStep {
    if ready.is_empty() {
        return NextStep::None;
    }
    if mode == PlaybackMode::SingleLoop && natural_end && current.is_some() {
        return NextStep::Item(current.unwrap_or_default().to_string());
    }
    if mode == PlaybackMode::Shuffle {
        if shuffle.is_empty() || !shuffle.iter().any(|id| Some(id.as_str()) == current) {
            return NextStep::ReshuffleMissing;
        }
        let index = shuffle.iter().position(|id| Some(id.as_str()) == current).unwrap_or(usize::MAX);
        if let Some(next) = shuffle.get(index.saturating_add(1)) {
            return NextStep::Item(next.clone());
        }
        return NextStep::ReshuffleWrap;
    }
    let index = ready.iter().position(|id| Some(id.as_str()) == current);
    let Some(index) = index else {
        return NextStep::Item(ready[0].clone());
    };
    if let Some(next) = ready.get(index + 1) {
        return NextStep::Item(next.clone());
    }
    if mode == PlaybackMode::ListLoop {
        NextStep::Item(ready[0].clone())
    } else {
        NextStep::None
    }
}

pub fn previous_ready_id(mode: PlaybackMode, current: Option<&str>, ready: &[String], history: &[String]) -> Option<String> {
    if mode == PlaybackMode::Shuffle && history.len() > 1 {
        return history.get(history.len() - 2).cloned();
    }
    let index = ready.iter().position(|id| Some(id.as_str()) == current);
    if let Some(index) = index.filter(|index| *index > 0) {
        return ready.get(index - 1).cloned();
    }
    ready.last().cloned()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NextStep {
    None,
    Item(String),
    /// 当前项不在洗牌序列里，重建后取下一项。
    ReshuffleMissing,
    /// 序列已经走到末尾，重建后从新序列的第一项开始。
    ReshuffleWrap,
}

pub fn mode_name(mode: PlaybackMode) -> &'static str {
    match mode {
        PlaybackMode::ListLoop => "listLoop",
        PlaybackMode::Shuffle => "shuffle",
        PlaybackMode::SingleLoop => "singleLoop",
    }
}

pub fn mode_label(mode: PlaybackMode) -> &'static str {
    match mode {
        PlaybackMode::Shuffle => "随机播放",
        PlaybackMode::SingleLoop => "单曲循环",
        _ => "列表循环",
    }
}

pub fn cycle_mode(mode: PlaybackMode) -> PlaybackMode {
    match mode {
        PlaybackMode::ListLoop => PlaybackMode::Shuffle,
        PlaybackMode::Shuffle => PlaybackMode::SingleLoop,
        PlaybackMode::SingleLoop => PlaybackMode::ListLoop,
    }
}

fn type_supports_extension(
    player_type_id: &str,
    extension: &str,
    contributions: &[PlaylistPlayerContribution],
    candidates: &[PlayerCandidate],
) -> bool {
    let from_candidate = candidates.iter().any(|candidate| {
        candidate.player_type_id == player_type_id
            && candidate.extensions.iter().any(|item| item.eq_ignore_ascii_case(extension))
    });
    from_candidate
        || contributions.iter().any(|contribution| {
            contribution.player_type_id == player_type_id
                && contribution.supported_extensions.iter().any(|item| item.eq_ignore_ascii_case(extension))
        })
}

fn resolve_capability(player_type_id: &str, candidates: &[PlayerCandidate]) -> String {
    let stable = default_capability(player_type_id);
    if candidates.iter().any(|candidate| capability_id(candidate) == stable) {
        return stable;
    }
    candidates.first().map(capability_id).unwrap_or(stable)
}

fn official_plugin(capability_id: &str) -> Option<&'static str> {
    (capability_id == AUDIO_CAPABILITY).then_some(OFFICIAL_AUDIO_PLUGIN)
}

fn parse_session(repo_id: &str, value: &serde_json::Value) -> Option<StoredSession> {
    let playlist_id = value.get("playlistId").and_then(|item| item.as_str()).unwrap_or("").trim().to_string();
    let current_item_id = value.get("currentItemId").and_then(|item| item.as_str()).unwrap_or("").trim().to_string();
    if playlist_id.is_empty() || current_item_id.is_empty() {
        eprintln!("Nana 忽略不完整的播放会话：{repo_id}");
        return None;
    }
    Some(StoredSession {
        repo_id: value.get("repoId").and_then(|item| item.as_str()).unwrap_or(repo_id).to_string(),
        playlist_id,
        player_type_id: value.get("playerTypeId").and_then(|item| item.as_str()).unwrap_or("").to_string(),
        current_item_id,
        current_time_ms: value.get("currentTimeMs").and_then(|item| item.as_u64()).unwrap_or(0),
        duration_ms: value.get("durationMs").and_then(|item| item.as_u64()).unwrap_or(0),
        mode: normalize_mode(value.get("mode").and_then(|item| item.as_str())),
        volume: normalize_volume(value.get("volume").and_then(|item| item.as_f64())),
        is_playing: value.get("isPlaying").and_then(|item| item.as_bool()).unwrap_or(false),
    })
}

fn parse_preferences(value: &serde_json::Value) -> BTreeMap<String, String> {
    let Some(object) = value.as_object() else {
        eprintln!("Nana 播放器偏好不是对象");
        return BTreeMap::new();
    };
    object
        .iter()
        .filter_map(|(key, value)| {
            let key = key.trim();
            let value = value.as_str().map(str::trim).unwrap_or("");
            if key.is_empty() || value.is_empty() {
                None
            } else {
                Some((key.to_string(), value.to_string()))
            }
        })
        .collect()
}

fn read_json(path: &Path, label: &str, key: &str) -> Option<serde_json::Value> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return None,
        Err(error) => {
            eprintln!("Nana 读取{label}失败（{key}）：{error}");
            return None;
        }
    };
    match serde_json::from_str(&raw) {
        Ok(value) => Some(value),
        Err(error) => {
            eprintln!("Nana 解析{label}失败（{key}）：{error}");
            None
        }
    }
}

pub fn format_time(ms: u64) -> String {
    let total = ms / 1000;
    format!("{}:{:02}", total / 60, total % 60)
}
