//! 播放列表当前项的视频和 m4a/aac/opus。
//!
//! 只解正在播的那一条，走预览用的 `preview_media_parts` 和同一套画面、PCM 内存上限。
//! 没有当前项时 `play_item` 直接返回，不会进到这里。测试构建的出声仍不打开声卡。

use super::super::inspect::{InspectState, PreviewBody};
use super::wav_player::{self, Action};
use super::{QueueItem, PlayerState};

const CLIP_EXTENSIONS: &[&str] = &["mp4", "mov", "mkv", "webm", "avi", "m4v", "m4a", "aac", "opus"];
const SESSION_EXTENSIONS: &[&str] = &[
    "mp3", "wav", "ogg", "flac", "m4a", "aac", "opus", "mp4", "mov", "mkv", "webm", "avi", "m4v",
];
const STILL_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "gif", "bmp", "avif", "svg"];

/// 图片幻灯片。有帧才画；读不到文件就留下缺的合约，不编造画面。
#[derive(Clone, Debug)]
pub(crate) struct StillShow {
    pub extension: String,
    pub path: String,
    pub width: u32,
    pub height: u32,
    pub frame: Option<super::super::PreviewPixels>,
    pub missing: Option<String>,
}

/// 这些扩展名已经能进预览用的音视频会话，不再要求把 Vue 播放器升级一遍。
pub(super) fn uses_media_session(extension: &str) -> bool {
    SESSION_EXTENSIONS.iter().any(|item| item.eq_ignore_ascii_case(extension.trim()))
}

pub(super) fn load_item(player: &mut PlayerState, item: &QueueItem) {
    player.still = None;
    player.outside_note = None;
    player.session.repo_id = player.repo_id.clone().unwrap_or_default();
    player.session.playlist_id = item.playlist_id.clone();
    player.session.playlist_item_id = Some(item.id.clone());
    player.session.session_id = format!("playback-{}", player.session.repo_id);
    if item.status != "ready" {
        player.drop_clip_frames();
        player.fail_session(item.status_reason.clone().unwrap_or_else(|| "当前条目不可播放".into()));
        return;
    }
    if is_clip_extension(&item.extension) {
        load_clip(player, item);
        return;
    }
    player.drop_clip_frames();
    let resolution = player.resolve_type(&item.player_type_id);
    player.notice = super::resolution_notice(&resolution);
    if resolution.player.is_none() {
        let contributed = player.contributions.iter().any(|contribution| contribution.player_type_id == item.player_type_id);
        if contributed && uses_media_session(&item.extension) {
            eprintln!("Nana 播放贡献改走已有音视频会话：{}", item.path);
            load_clip(player, item);
            return;
        }
        if contributed && is_still_extension(&item.extension) {
            load_still(player, item);
            return;
        }
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
        return;
    }
    player.activity.clear();
    player.preview_armed = false;
    let memory = resolution.player.as_ref().is_some_and(wav_player::is_memory_candidate);
    if !memory {
        player.wav.clear();
    }
    let session = player.session.clone();
    let (session, error) = wav_player::drive(memory, &player.wav, session, Action::Load(item.path.clone()));
    player.session = session;
    player.can_play = player.session.status != "failed";
    if item.file_class == "image" {
        player.apply_image_duration();
    }
    if let Some(error) = error {
        eprintln!("Nana 播放装载失败：{error}");
        player.activity = error;
    }
}

/// 只读取当前路径。队列里的其它条目不打开。
fn load_clip(player: &mut PlayerState, item: &QueueItem) {
    player.activity.clear();
    player.notice.clear();
    player.preview_armed = false;
    let path = item.path.trim();
    if path.is_empty() {
        eprintln!("Nana 播放列表当前项没有路径");
        player.clip_frames = None;
        player.clip_owned = true;
        player.fail_session("解码失败：没有可播放的当前项".into());
        return;
    }
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("Nana 播放列表当前项读取失败：{path}：{error}");
            player.clip_frames = None;
            player.clip_owned = true;
            player.wav.clear();
            player.fail_session(format!("解码失败：无法读取当前项：{error}"));
            return;
        }
    };
    let repo_id = player.session.repo_id.clone();
    match super::super::preview_media_parts(&repo_id, &bytes) {
        Ok(parts) => install_clip(player, parts),
        Err(error) => {
            eprintln!("Nana 播放列表当前项解码失败：{path}：{error}");
            player.clip_frames = None;
            player.clip_owned = true;
            player.wav.clear();
            player.fail_session(error);
        }
    }
}

fn install_clip(player: &mut PlayerState, parts: super::super::MediaParts) {
    player.clip_owned = true;
    player.clip_frames = parts.frames.filter(|frames| !frames.is_empty());
    player.session.duration_ms = parts.session.duration_ms;
    player.session.can_seek = parts.session.can_seek;
    player.session.can_volume = parts.session.can_volume;
    player.session.error = None;
    player.session.current_time_ms = 0;
    player.session.status = parts.session.status;
    if let Some(pcm) = parts.pcm {
        player.wav.install_preview(pcm);
        if player.wants_playing {
            let session = player.session.clone();
            let (session, error) = wav_player::drive(true, &player.wav, session, Action::Play);
            player.session = session;
            if let Some(error) = error {
                eprintln!("Nana 播放列表当前项出声失败：{error}");
                player.activity = error;
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

pub(super) fn publish_clip(player: &mut PlayerState, inspect: &mut InspectState) {
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

/// 只读当前图片。解得出就留下真实宽高；读失败不进入播放。
/// 停留时长先按设置写进会话，和 Vue 运行时先配置时长、再读播放源的顺序一致。
fn load_still(player: &mut PlayerState, item: &QueueItem) {
    player.drop_clip_frames();
    player.wav.clear();
    player.activity.clear();
    player.notice.clear();
    player.session.error = None;
    player.session.can_seek = false;
    player.session.can_volume = false;
    player.session.current_time_ms = 0;
    player.apply_image_duration();
    let path = item.path.trim();
    let decoded = if path.is_empty() {
        Err("没有路径".to_string())
    } else {
        match std::fs::read(path) {
            Ok(bytes) => super::super::decode_preview_pixels(&bytes),
            Err(error) => {
                eprintln!("Nana 图片幻灯片读取失败：{path}：{error}");
                Err(format!("无法读取当前项：{error}"))
            }
        }
    };
    match decoded {
        Ok(pixels) => {
            let width = pixels.width;
            let height = pixels.height;
            player.session.status = if player.wants_playing { "playing" } else { "paused" }.into();
            player.can_play = true;
            player.still = Some(StillShow {
                extension: item.extension.clone(),
                path: path.to_string(),
                width,
                height,
                frame: Some(pixels),
                missing: None,
            });
        }
        Err(error) => {
            eprintln!("Nana 图片幻灯片没有可绘制的画面：{path}：{error}");
            let missing = format!("图片幻灯片需要路径上的 RGBA 帧。当前读不到 {path}：{error}。不显示假图。");
            player.still = Some(StillShow {
                extension: item.extension.clone(),
                path: path.to_string(),
                width: 0,
                height: 0,
                frame: None,
                missing: Some(missing),
            });
            // 播放条和 Vue 一样只写「图片无法播放」，读失败的细节留在日志和缺帧说明里。
            player.fail_session("图片无法播放".into());
        }
    }
}

fn is_clip_extension(extension: &str) -> bool {
    let extension = extension.trim();
    CLIP_EXTENSIONS.iter().any(|item| item.eq_ignore_ascii_case(extension))
}
