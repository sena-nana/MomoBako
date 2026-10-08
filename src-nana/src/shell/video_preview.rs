//! 视频预览解码。
//!
//! 未压缩和 MJPEG 的 AVI 用纯 Rust 解出画面和 PCM。其余常见容器在 Windows 上交给系统媒体基础，
//! 只拉取样本，不打开窗口、不拉起播放器、也不把声音送进声卡。没有画面时仍解音轨，PCM 进现有播放会话。
//! 认不出的字节仍是「没有原生解码器」。解不开的具体文件返回「解码失败」。超过内存上限时失败。

#[path = "video_avi.rs"]
mod avi;
#[cfg(windows)]
#[path = "video_mf.rs"]
mod mf;

use crate::backend::services::repository::PlaybackSessionState;
use crate::shell::player::PreviewPcm;

/// 预览里同时保留的画面字节上限。超过就失败，避免把半段当成整段。
pub(super) const FRAME_BYTE_BUDGET: usize = 16 * 1024 * 1024;
/// 音轨 PCM 上限。超过时会话时长停在已经解出的部分。
pub(super) const PCM_BYTE_BUDGET: usize = 32 * 1024 * 1024;

/// 一帧预览画面。时间是相对片头的毫秒。
#[derive(Clone, Debug)]
pub struct VideoFrame {
    pub time_ms: u64,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// 解出来的画面和可选音轨。
#[derive(Clone, Debug)]
pub struct DecodedClip {
    pub duration_ms: u64,
    pub frames: Vec<VideoFrame>,
    pub pcm: Option<PreviewPcm>,
}

/// 解视频容器。成功时会话处于 paused，并能跳转。
pub(super) fn open_clip(repo_id: &str, bytes: &[u8]) -> Result<(PlaybackSessionState, Option<PreviewPcm>, Vec<VideoFrame>), String> {
    let clip = decode(bytes)?;
    if clip.frames.is_empty() {
        let Some(pcm) = clip.pcm else {
            eprintln!("Nana 媒体解码没有画面，也没有音轨");
            return Err("解码失败：没有可用画面".into());
        };
        if pcm.duration_ms == 0 || pcm.pcm.is_empty() {
            eprintln!("Nana 媒体音轨没有可播放的样本");
            return Err("解码失败：音轨没有样本".into());
        }
        let session = PlaybackSessionState {
            status: "paused".into(),
            duration_ms: Some(pcm.duration_ms.max(1)),
            can_seek: true,
            can_volume: true,
            error: None,
            ..fresh_session(repo_id)
        };
        return Ok((session, Some(pcm), Vec::new()));
    }
    if clip.duration_ms == 0 {
        eprintln!("Nana 视频解码没有可用画面");
        return Err("解码失败：没有可用画面".into());
    }
    let session = PlaybackSessionState {
        status: "paused".into(),
        duration_ms: Some(clip.duration_ms),
        can_seek: true,
        can_volume: clip.pcm.is_some(),
        error: None,
        ..fresh_session(repo_id)
    };
    Ok((session, clip.pcm, clip.frames))
}

fn decode(bytes: &[u8]) -> Result<DecodedClip, String> {
    let avi_error = match avi::try_decode(bytes) {
        Ok(Some(clip)) => return Ok(clip),
        Ok(None) => None,
        Err(error) => {
            eprintln!("Nana 内置 AVI 解码失败，改试系统解码：{error}");
            Some(error)
        }
    };
    let Some(kind) = container_kind(bytes) else {
        return Err(avi_error.unwrap_or_else(|| "没有原生解码器".into()));
    };
    #[cfg(windows)]
    {
        match mf::decode(bytes, kind) {
            Ok(clip) => Ok(clip),
            Err(error) => {
                eprintln!("Nana 系统视频解码失败：{error}");
                Err(avi_error.unwrap_or(error))
            }
        }
    }
    #[cfg(not(windows))]
    {
        Err(avi_error.unwrap_or_else(|| format!("{kind} 解码失败：当前构建没有系统媒体基础")))
    }
}

fn container_kind(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        return Some("MP4/MOV");
    }
    if bytes.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        return Some("MKV/WebM");
    }
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"AVI " {
        return Some("AVI");
    }
    if is_adts(bytes) {
        return Some("AAC");
    }
    if bytes.starts_with(b"OggS") || bytes.starts_with(b"OpusHead") {
        return Some("Opus");
    }
    None
}

/// ADTS 同步字。m4a 走 ftyp，这里只认裸 AAC，避免把普通字节送进媒体基础。
fn is_adts(bytes: &[u8]) -> bool {
    bytes.len() >= 7 && bytes[0] == 0xFF && (bytes[1] & 0xF6) == 0xF0
}

#[cfg(test)]
pub(super) fn sample_uncompressed() -> Vec<u8> {
    avi::sample_uncompressed()
}

fn fresh_session(repo_id: &str) -> PlaybackSessionState {
    PlaybackSessionState {
        session_id: format!("preview-{repo_id}"),
        repo_id: repo_id.to_string(),
        playlist_id: String::new(),
        playlist_item_id: None,
        status: "loading".into(),
        current_time_ms: 0,
        duration_ms: None,
        volume: 1.0,
        can_seek: false,
        can_volume: false,
        error: None,
        updated_at: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::decode;

    #[test]
    fn uncompressed_avi_yields_frames_and_pcm() {
        let clip = decode(&super::avi::sample_uncompressed()).expect("avi");
        assert_eq!(clip.frames.len(), 2);
        assert!(clip.duration_ms >= 500, "{}", clip.duration_ms);
        assert_eq!(clip.frames[0].rgba[0], 255);
        assert_eq!(clip.frames[1].rgba[2], 255);
        assert!(clip.pcm.is_some());
    }

    #[test]
    fn recognized_mp4_is_a_decode_failure_not_a_missing_decoder() {
        let mp4 = b"\x00\x00\x00\x18ftypmp42\x00\x00\x00\x00mp42isom";
        let message = decode(mp4).expect_err("残缺 mp4");
        assert!(message.contains("解码失败"), "{message}");
        assert!(message.contains("MP4/MOV"), "{message}");
        assert!(!message.contains("symphonia"), "{message}");
        assert_eq!(decode(b"not-video").expect_err("plain"), "没有原生解码器");
    }

    #[test]
    fn aac_and_opus_headers_fail_clearly_without_symphonia() {
        let aac = [0xFF, 0xF1, 0x50, 0x80, 0x00, 0x1F, 0xFC];
        let message = decode(&aac).expect_err("aac");
        assert!(message.contains("解码失败"), "{message}");
        assert!(message.contains("AAC"), "{message}");
        assert!(!message.contains("symphonia"), "{message}");
        let message = decode(b"OggSnot-a-real-opus").expect_err("opus");
        assert!(message.contains("解码失败"), "{message}");
        assert!(message.contains("Opus"), "{message}");
        let m4a = b"\x00\x00\x00\x18ftypM4A \x00\x00\x00\x00M4A mp42";
        let message = decode(m4a).expect_err("m4a");
        assert!(message.contains("解码失败"), "{message}");
        assert!(!message.contains("没有原生解码器"), "{message}");
    }
}
