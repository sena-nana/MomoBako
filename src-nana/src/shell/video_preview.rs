//! 视频预览解码。
//!
//! 未压缩和 MJPEG 的 AVI 用纯 Rust 解出画面和 PCM。其余常见容器在 Windows 上交给系统媒体基础，
//! 只拉取样本，不打开窗口、不拉起播放器、也不把声音送进声卡。没有画面时仍解音轨，PCM 进现有播放会话。
//! 认不出的字节仍是「没有原生解码器」。解不开的具体文件返回「解码失败」。
//!
//! 画面和音轨都整段放在内存里，有上限。超过上限时只留前面一段：会话照样能放，时长是解出的这一段，
//! `error` 写明只解出了多少、全长多少（播放条第二行显示），不把半段当成整段。按播放时钟流式解码以后才能去掉上限。

#[path = "video_avi.rs"]
mod avi;
#[cfg(windows)]
#[path = "video_mf.rs"]
mod mf;

use crate::backend::services::repository::PlaybackSessionState;
use crate::shell::player::PreviewPcm;

/// 视频画面同时保留的字节上限。长边 320 的 16:9 画面一帧约 225 KiB，大约能留前 5–7 秒。
pub(super) const FRAME_BYTE_BUDGET: usize = 16 * 1024 * 1024;
/// 视频音轨的 PCM 上限。视频的音轨只读到最后一帧画面后 1 秒，平常碰不到。
pub(super) const PCM_BYTE_BUDGET: usize = 32 * 1024 * 1024;
/// 只有音轨（m4a、aac、opus）时的 PCM 上限：1 GiB，48 kHz 立体声 16 位约 93 分钟。
pub(super) const AUDIO_PCM_BYTE_BUDGET: usize = 1024 * 1024 * 1024;

/// 解码停在半路（内存上限、读样本的次数上限）。`total_ms` 是容器写的全长，读不到时为空。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Cut {
    pub total_ms: Option<u64>,
}

/// 一帧预览画面。时间是相对片头的毫秒。
#[derive(Clone, Debug)]
pub struct VideoFrame {
    pub time_ms: u64,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// 解出来的画面和可选音轨。`duration_ms` 是解出的这一段的长度；没解到结尾时 `cut` 有值。
#[derive(Clone, Debug)]
pub struct DecodedClip {
    pub duration_ms: u64,
    pub frames: Vec<VideoFrame>,
    pub pcm: Option<PreviewPcm>,
    pub cut: Option<Cut>,
}

/// 解视频容器。成功时会话处于 paused，并能跳转。
pub(super) fn open_clip(repo_id: &str, bytes: &[u8]) -> Result<(PlaybackSessionState, Option<PreviewPcm>, Vec<VideoFrame>), String> {
    session_for(repo_id, decode(bytes)?)
}

/// 解出的片段换成会话：没有画面时只用音轨。没解到结尾时会话照样能放，`error` 写明只解出了多少（[`cut_note`]）。
fn session_for(repo_id: &str, clip: DecodedClip) -> Result<(PlaybackSessionState, Option<PreviewPcm>, Vec<VideoFrame>), String> {
    let note = clip.cut.map(|cut| cut_note(clip.duration_ms, cut.total_ms));
    if let Some(note) = &note {
        eprintln!("Nana 媒体没有解到结尾：{note}");
    }
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
            error: note,
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
        error: note,
        ..fresh_session(repo_id)
    };
    Ok((session, clip.pcm, clip.frames))
}

/// 没解到结尾时写在会话里的说明：「只解出前 0:05（全长 2:31），之后的部分暂不能播放」。全长读不到时不写括号。
pub(super) fn cut_note(decoded_ms: u64, total_ms: Option<u64>) -> String {
    let total = total_ms.filter(|total| *total > decoded_ms).map(|total| format!("（全长 {}）", clock(total))).unwrap_or_default();
    format!("只解出前 {}{total}，之后的部分暂不能播放", clock(decoded_ms))
}

/// 和播放条一样写成「分:秒」。
fn clock(ms: u64) -> String {
    let seconds = ms / 1000;
    format!("{}:{:02}", seconds / 60, seconds % 60)
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

    /// 交错的 16 位小端样本。
    #[cfg(windows)]
    fn le16(samples: &[i16]) -> Vec<u8> {
        samples.iter().flat_map(|sample| sample.to_le_bytes()).collect()
    }

    /// 媒体基础解出的 5.1 音轨下混成立体声。拿 WAV 当容器在真实的源读取器上走一遍音轨读取；产品里 WAV 由自有
    /// 解析接手，不经媒体基础。
    #[cfg(windows)]
    #[test]
    fn media_foundation_downmixes_a_surround_track() {
        use crate::shell::audio_decode::pcm::{fmt_chunk, wav_file};
        let frames = le16(&[1_000, -1_000, 0, 20_000, 0, 0].repeat(8_000));
        let bytes = wav_file(&fmt_chunk(0, 6, 8_000, 16, Some((1, 0x3F))), &frames);
        let clip = super::mf::decode(&bytes, "WAV").expect("媒体基础解 5.1 WAV");
        assert!(clip.frames.is_empty());
        let pcm = clip.pcm.expect("音轨");
        assert_eq!((pcm.channels, pcm.bits_per_sample, pcm.duration_ms), (2, 16, 1_000));
        let side = (1_000.0 / (1.0 + std::f32::consts::SQRT_2)).round() as i16;
        assert_eq!(&pcm.pcm[..4], le16(&[side, -side]).as_slice());
    }

    /// 只有音轨的媒体（m4a、aac、opus 走这条路）整段解出来：48 kHz 立体声 200 秒约 37 MiB，不在 3 分钟左右截断。
    #[cfg(windows)]
    #[test]
    fn media_foundation_audio_is_not_cut_at_three_minutes() {
        use crate::shell::audio_decode::pcm::{fmt_chunk, wav_file};
        let bytes = wav_file(&fmt_chunk(1, 2, 48_000, 16, None), &vec![0; 48_000 * 4 * 200]);
        let clip = super::mf::decode(&bytes, "WAV").expect("媒体基础解 200 秒的音轨");
        assert_eq!(clip.pcm.as_ref().map(|pcm| pcm.duration_ms), Some(200_000));
        assert_eq!(clip.duration_ms, 200_000);
    }

    /// 只有一项上限很小的解码上限，其余照常。
    #[cfg(windows)]
    fn tight(frames: usize, audio_only: usize) -> super::mf::Budget {
        super::mf::Budget { frames, video_audio: super::PCM_BYTE_BUDGET, audio_only }
    }

    /// 音轨到了上限：只留前面一段，会话照样能放，时长是解出的这一段，`error` 写明只解出了多少、全长多少。
    #[cfg(windows)]
    #[test]
    fn a_cut_audio_track_says_how_much_was_decoded() {
        use crate::shell::audio_decode::pcm::{fmt_chunk, wav_file};
        let bytes = wav_file(&fmt_chunk(1, 1, 8_000, 16, None), &vec![0; 16_000 * 10]);
        let clip = super::mf::decode_within(&bytes, "WAV", &tight(super::FRAME_BYTE_BUDGET, 32_000)).expect("解出前一段");
        assert_eq!(clip.cut, Some(super::Cut { total_ms: Some(10_000) }));
        assert!(clip.duration_ms > 0 && clip.duration_ms <= 2_000, "{}", clip.duration_ms);
        let decoded = clip.duration_ms;
        let (session, pcm, frames) = super::session_for("repo", clip).expect("会话");
        assert!(frames.is_empty() && pcm.is_some());
        assert_eq!((session.status.as_str(), session.duration_ms, session.can_seek), ("paused", Some(decoded), true));
        assert_eq!(session.error, Some(super::cut_note(decoded, Some(10_000))));
    }

    /// 视频画面到了上限：只留前几帧，时长停在解出的这一段，同样写明只解出了多少、全长多少。
    #[cfg(windows)]
    #[test]
    fn a_cut_video_says_how_much_was_decoded() {
        let bytes = super::avi::sample_frames(30, 64, 36, 10);
        let whole = super::mf::decode(&bytes, "AVI").expect("媒体基础解 AVI");
        assert_eq!((whole.frames.len(), whole.cut), (30, None), "上限够时整段解完");
        let clip = super::mf::decode_within(&bytes, "AVI", &tight(40_000, super::AUDIO_PCM_BYTE_BUDGET)).expect("解出前几帧");
        assert_eq!(clip.frames.len(), 4);
        assert_eq!(clip.cut, Some(super::Cut { total_ms: Some(3_000) }));
        assert!(clip.duration_ms < 1_000, "{}", clip.duration_ms);
        let decoded = clip.duration_ms;
        let (session, _, _) = super::session_for("repo", clip).expect("会话");
        assert_eq!((session.status.as_str(), session.duration_ms), ("paused", Some(decoded)));
        assert_eq!(session.error, Some(super::cut_note(decoded, Some(3_000))));
    }

    /// 说明写成「分:秒」；全长读不到、或者不比解出的长时不写括号。
    #[test]
    fn the_cut_note_names_the_decoded_part_and_the_whole() {
        assert_eq!(super::cut_note(5_400, Some(151_000)), "只解出前 0:05（全长 2:31），之后的部分暂不能播放");
        assert_eq!(super::cut_note(5_400, None), "只解出前 0:05，之后的部分暂不能播放");
        assert_eq!(super::cut_note(5_400, Some(5_000)), "只解出前 0:05，之后的部分暂不能播放");
        let clip = super::DecodedClip { duration_ms: 5_400, frames: Vec::new(), pcm: None, cut: Some(super::Cut { total_ms: Some(151_000) }) };
        assert!(super::session_for("repo", clip).is_err(), "没有画面也没有音轨仍是解码失败");
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
