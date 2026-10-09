//! 播放列表当前项的装载：音频、视频和图片幻灯片。
//!
//! 条目路径是仓库内相对路径，字节必须经仓库服务读取。归约只留下 `LoadItem` 请求并把会话
//! 置为 loading；`player_dispatch` 读出字节、解码后用 `ItemLoaded` 送回，这里再装进同一游标。
//! 代次或当前项变了的结果直接丢弃。测试构建的出声仍不打开声卡。

use super::super::inspect::{InspectState, PreviewBody};
use super::wav_player::{self, Action, Output};
use super::{PlayerEffect, PlayerState, QueueItem};

const CLIP_EXTENSIONS: &[&str] = &["mp4", "mov", "mkv", "webm", "avi", "m4v", "m4a", "aac", "opus"];
const SESSION_EXTENSIONS: &[&str] = &[
    "mp3", "wav", "ogg", "flac", "m4a", "aac", "opus", "mp4", "mov", "mkv", "webm", "avi", "m4v",
];
const STILL_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "gif", "bmp", "avif", "svg"];

/// 图片幻灯片的当前项。有帧才画；读不到文件时 `frame` 为空，不编造画面。
#[derive(Clone, Debug)]
pub(crate) struct StillShow {
    pub path: String,
    pub frame: Option<super::super::PreviewPixels>,
}

/// 读出并解码好的当前项。
#[derive(Clone, Debug)]
pub enum LoadedItem {
    /// 音频或视频：会话、PCM 和可选画面。
    Media(crate::shell::MediaParts),
    /// 图片幻灯片的一帧。
    Still(super::super::PreviewPixels),
}

/// 这些扩展名已经能进预览用的音视频会话，不再要求把 Vue 播放器升级一遍。
pub(super) fn uses_media_session(extension: &str) -> bool {
    SESSION_EXTENSIONS.iter().any(|item| item.eq_ignore_ascii_case(extension.trim()))
}

/// 按条目和播放器决定怎么装：音视频、图片、外部条目或缺少插件。
/// 需要字节的两类只排队读取，结果由 [`finish_load`] 装进会话。
pub(super) fn load_item(player: &mut PlayerState, item: &QueueItem) {
    player.still = None;
    player.outside_note = None;
    player.cursor_item = None;
    player.loaded_item = None;
    // 换到播放列表条目时，预览不再驱动当前项。
    player.preview_path.clear();
    player.session.repo_id = player.repo_id.clone().unwrap_or_default();
    player.session.playlist_id = item.playlist_id.clone();
    player.session.playlist_item_id = Some(item.id.clone());
    player.session.session_id = format!("playback-{}", player.session.repo_id);
    if item.status != "ready" {
        player.drop_clip_frames();
        player.wav.clear();
        player.fail_session(item.status_reason.clone().unwrap_or_else(|| "当前条目不可播放".into()));
        return;
    }
    if is_clip_extension(&item.extension) {
        player.notice.clear();
        begin_load(player, item, false);
        return;
    }
    let resolution = player.resolve_type(&item.player_type_id);
    player.notice = super::resolution_notice(&resolution);
    if let Some(candidate) = resolution.player.as_ref() {
        if wav_player::is_memory_candidate(candidate) {
            begin_load(player, item, false);
            return;
        }
        // 登记了原生实现但没有解码器：和 Vue 一样先按设置写图片停留时长，再报缺解码器。
        player.drop_clip_frames();
        player.wav.clear();
        if item.file_class == "image" {
            player.apply_image_duration();
        }
        eprintln!("Nana 播放器 {} 没有原生解码器：{}", candidate.plugin_id, item.path);
        player.fail_session("没有原生解码器".into());
        return;
    }
    let contributed = player.contributions.iter().any(|contribution| contribution.player_type_id == item.player_type_id);
    if contributed && uses_media_session(&item.extension) {
        eprintln!("Nana 播放贡献改走已有音视频会话：{}", item.path);
        player.notice.clear();
        begin_load(player, item, false);
        return;
    }
    if contributed && is_still_extension(&item.extension) {
        player.notice.clear();
        begin_load(player, item, true);
        return;
    }
    player.drop_clip_frames();
    player.wav.clear();
    let message = if contributed {
        outside_session_contract(&item.extension)
    } else if player.notice.is_empty() {
        "缺少对应播放插件".into()
    } else {
        player.notice.clone()
    };
    if contributed {
        player.notice.clear();
        player.session.duration_ms = None;
        player.session.current_time_ms = 0;
        player.session.can_seek = false;
        player.session.can_volume = false;
        player.outside_note = Some(message.clone());
    }
    eprintln!("Nana 播放插件不可用：{message}");
    player.fail_session(message);
}

/// 会话置为读取中，并留下读取请求。图片的停留时长先写进会话，和 Vue 运行时先配置时长一致。
fn begin_load(player: &mut PlayerState, item: &QueueItem, still: bool) {
    player.drop_clip_frames();
    player.wav.clear();
    player.activity.clear();
    player.load_generation = player.load_generation.wrapping_add(1);
    player.can_play = false;
    player.session.status = "loading".into();
    player.session.error = None;
    player.session.current_time_ms = 0;
    player.session.duration_ms = None;
    player.session.can_seek = false;
    player.session.can_volume = false;
    if still {
        player.apply_image_duration();
    }
    if item.path.trim().is_empty() {
        eprintln!("Nana 播放列表当前项没有路径：{}", item.id);
        player.fail_session("没有可播放的当前项".into());
        return;
    }
    player.effects.push(PlayerEffect::LoadItem {
        repo_id: player.session.repo_id.clone(),
        item_id: item.id.clone(),
        path: item.path.clone(),
        extension: item.extension.clone(),
        still,
        generation: player.load_generation,
    });
}

/// 读出的字节解成当前项。读文件由调用方经仓库服务完成，这里只解码。
pub(crate) fn decode_loaded(repo_id: &str, still: bool, extension: &str, bytes: &[u8]) -> Result<LoadedItem, String> {
    if still {
        return crate::shell::decode_preview_pixels(bytes).map(LoadedItem::Still);
    }
    decode_media(repo_id, extension, bytes).map(LoadedItem::Media)
}

/// 音视频字节解成会话、音轨和画面，播放条和预览页共用。wav、mp3、flac、ogg 解不开时报对应格式的原因
/// （WAV 头不对、压缩音频解码失败），不拿视频容器的「没有原生解码器」顶替。
pub(crate) fn decode_media(repo_id: &str, extension: &str, bytes: &[u8]) -> Result<crate::shell::MediaParts, String> {
    crate::shell::preview_media_parts(repo_id, bytes).map_err(|error| wav_player::pcm_error_for_extension(extension, bytes).unwrap_or(error))
}

/// 装进读取结果。过期的代次或已经换掉的条目不装。`still` 是请求时的类别，失败时据此写说明。
pub(super) fn finish_load(player: &mut PlayerState, item_id: &str, generation: u64, still: bool, result: Result<LoadedItem, String>) {
    if generation != player.load_generation || player.current_id.as_deref() != Some(item_id) {
        eprintln!("Nana 忽略过期的播放条目装载：{item_id}");
        return;
    }
    let Some(item) = player.current_item().cloned() else {
        eprintln!("Nana 播放队列已经没有这个条目：{item_id}");
        return;
    };
    match result {
        Ok(LoadedItem::Media(parts)) => install_clip(player, parts),
        Ok(LoadedItem::Still(pixels)) => install_still(player, &item, pixels),
        Err(error) if still => fail_still(player, &item, &error),
        Err(error) => {
            eprintln!("Nana 播放列表当前项装载失败：{}：{error}", item.path);
            player.clip_frames = None;
            player.clip_owned = true;
            player.wav.clear();
            player.fail_session(error);
        }
    }
    player.persist_if_needed();
}

fn install_clip(player: &mut PlayerState, parts: crate::shell::MediaParts) {
    player.clip_owned = true;
    player.clip_frames = parts.frames.filter(|frames| !frames.is_empty());
    player.session.duration_ms = parts.session.duration_ms;
    player.session.can_seek = parts.session.can_seek;
    player.session.can_volume = parts.session.can_volume;
    player.session.error = None;
    player.session.current_time_ms = 0;
    player.session.status = parts.session.status;
    player.loaded_item = player.current_id.clone();
    if let Some(pcm) = parts.pcm {
        player.wav.install_preview(pcm);
        player.cursor_item = player.current_id.clone();
        player.apply_cursor_volume();
        if player.wants_playing {
            let session = player.session.clone();
            let (session, error) = wav_player::drive(Output::Cursor, &player.wav, session, Action::Play);
            player.session = session;
            if let Some(error) = error {
                eprintln!("Nana 播放列表当前项出声失败：{error}");
                player.note_failure(format!("播放控制失败：{error}"));
            }
        }
    } else {
        player.wav.clear();
        if player.wants_playing {
            player.session.status = "playing".into();
        }
    }
    player.can_play = player.session.status != "failed";
}

/// 解得出就留下真实宽高。
fn install_still(player: &mut PlayerState, item: &QueueItem, pixels: super::super::PreviewPixels) {
    player.session.status = if player.wants_playing { "playing" } else { "paused" }.into();
    player.can_play = true;
    player.loaded_item = player.current_id.clone();
    player.still = Some(StillShow { path: item.path.clone(), frame: Some(pixels) });
}

/// 读不到或解不开：播放条和 Vue 一样只写「图片无法播放」，细节留在日志里。
fn fail_still(player: &mut PlayerState, item: &QueueItem, error: &str) {
    eprintln!("Nana 图片幻灯片没有可绘制的画面：{}：{error}", item.path);
    player.still = Some(StillShow { path: item.path.clone(), frame: None });
    player.fail_session("图片无法播放".into());
}

/// 预览页显示的正是当前项时，把会话和画面写回预览页；别的文件的预览不动。
pub(super) fn publish_clip(player: &mut PlayerState, inspect: &mut InspectState) {
    let shown = player.current_item().is_some_and(|item| inspect.target_path.as_deref() == Some(item.path.as_str()));
    if !shown {
        return;
    }
    inspect.replace_shared_media(player.session.clone());
    if !player.clip_owned {
        return;
    }
    if matches!(inspect.body, PreviewBody::Media(_)) {
        inspect.install_clip_frames(player.clip_frames.clone());
    }
    if player.clip_frames.is_none() {
        player.clip_owned = false;
    }
}

impl PlayerState {
    /// 离开视频或 m4a 当前项。已经交给预览的画面留到下一次真正发布再清。
    pub(super) fn drop_clip_frames(&mut self) {
        if self.clip_frames.is_some() {
            self.clip_owned = true;
        }
        self.clip_frames = None;
        self.still = None;
        self.outside_note = None;
    }
}

/// 图片幻灯片扩展名。和 `media-preview` 的 `imagePreviewExtensions` 一致。
pub(super) fn is_still_extension(extension: &str) -> bool {
    let extension = extension.trim();
    STILL_EXTENSIONS.iter().any(|item| item.eq_ignore_ascii_case(extension))
}

/// 不在音视频会话、也不是图片幻灯片时写进播放条的说明。和 Vue 插件报错一样只写一句事实。
pub(super) fn outside_session_contract(extension: &str) -> String {
    let extension = extension.trim();
    let shown = if extension.is_empty() { "（空）" } else { extension };
    format!("暂不支持播放 {shown} 文件")
}

fn is_clip_extension(extension: &str) -> bool {
    let extension = extension.trim();
    CLIP_EXTENSIONS.iter().any(|item| item.eq_ignore_ascii_case(extension))
}

/// 测试里代替 `player_dispatch`：按请求里的路径直接读文件、解码并送回。其它副作用原样放回。
#[cfg(test)]
pub(crate) fn fulfill_loads(model: &mut super::super::ShellViewModel) {
    let effects = model.player.take_effects();
    let mut kept = Vec::new();
    for effect in effects {
        let PlayerEffect::LoadItem { repo_id, item_id, path, extension, still, generation } = effect else {
            kept.push(effect);
            continue;
        };
        let result = std::fs::read(&path)
            .map_err(|error| format!("无法读取当前项：{error}"))
            .and_then(|bytes| decode_loaded(&repo_id, still, &extension, &bytes));
        model.reduce(super::super::ShellMessage::Player(super::PlayerMessage::ItemLoaded { item_id, generation, still, result }));
        kept.extend(model.player.take_effects());
    }
    model.player.requeue_effects(kept);
}
