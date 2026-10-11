//! 把 mp3、flac、ogg/Vorbis 字节解成交错的 16-bit 小端 PCM。三个以上声道下混成立体声。
//!
//! WAV 仍由 `wav_player` 解析，这里不注册 Wave，也不解视频。样本的位深换算和下混在 `audio_pcm.rs`，
//! WAV、AVI 音轨和媒体基础的音轨也用它。解码只动内存，不打开声卡。

#[path = "audio_pcm.rs"]
pub(crate) mod pcm;

use std::io::Cursor;

use symphonia::core::audio::Channels;
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as DecodeError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

/// 解码结果。`pcm` 是交错的 16-bit 小端样本，一帧包含全部声道各一个样本。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DecodedPcm {
    pub sample_rate: u32,
    pub channels: u16,
    pub pcm: Vec<u8>,
    pub duration_ms: u64,
}

/// 识别 mp3、flac 或 ogg/Vorbis。坏文件、空文件和视频返回错误。
pub(crate) fn decode_compressed(bytes: &[u8]) -> Result<DecodedPcm, String> {
    match decode_packets(bytes) {
        Ok(decoded) => Ok(decoded),
        Err(error) => {
            eprintln!("Nana 压缩音频解码失败：{error}");
            Err(error)
        }
    }
}

/// 按包解码并转成 16-bit。时长 = 帧数 × 1000 / 采样率，帧数是同一时刻全部声道的样本组数。
///
/// 输出声道数由第一个有样本的包定：单声道就是单声道，其余都是立体声。之后每个包按自己的声道布局换到这个声道数
/// （三个以上声道按扬声器位置下混，见 `audio_pcm.rs`），流里中途换了声道布局也接得上；采样率中途变了就报错。
fn decode_packets(bytes: &[u8]) -> Result<DecodedPcm, String> {
    if bytes.is_empty() {
        return Err("压缩音频是空的".into());
    }
    let stream = MediaSourceStream::new(Box::new(Cursor::new(bytes)), Default::default());
    let mut reader = symphonia::default::get_probe()
        .probe(&hint_for(bytes), stream, FormatOptions::default(), MetadataOptions::default())
        .map_err(|error| format!("无法识别压缩音频：{error}"))?;
    let (track_id, params) = {
        let track = reader.default_track(TrackType::Audio).ok_or("没有音频轨")?;
        let params = track
            .codec_params
            .as_ref()
            .and_then(|params| params.audio())
            .cloned()
            .ok_or("没有音频参数")?;
        (track.id, params)
    };
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&params, &AudioDecoderOptions::default())
        .map_err(|error| format!("无法创建解码器：{error}"))?;
    let mut pcm = Vec::new();
    let mut sample_rate = 0u32;
    let mut channels = 0u16;
    let mut frames = 0u64;
    let mut layout: Option<(Channels, pcm::Mixer)> = None;
    loop {
        let packet = match reader.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            Err(DecodeError::ResetRequired) if !pcm.is_empty() => break,
            Err(error) => return Err(format!("读取压缩音频失败：{error}")),
        };
        if packet.track_id != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            Err(DecodeError::DecodeError(_)) | Err(DecodeError::IoError(_)) => continue,
            Err(DecodeError::ResetRequired) if !pcm.is_empty() => break,
            Err(error) => return Err(format!("压缩音频包无法解码：{error}")),
        };
        if decoded.frames() == 0 {
            continue;
        }
        let spec = decoded.spec();
        let rate = spec.rate();
        let plane_count = u16::try_from(spec.channels().count()).map_err(|_| "声道数无效".to_string())?;
        if rate == 0 || plane_count == 0 {
            return Err("压缩音频采样率或声道无效".into());
        }
        if sample_rate == 0 {
            sample_rate = rate;
            channels = plane_count.min(2);
        } else if sample_rate != rate {
            return Err("压缩音频中途改变了采样率".into());
        }
        if layout.as_ref().is_none_or(|(known, _)| known != spec.channels()) {
            let positions = pcm::positions(plane_count, positioned_mask(spec.channels()));
            layout = Some((spec.channels().clone(), pcm::Mixer::new(&positions, channels)));
        }
        let mut samples = vec![0i16; decoded.samples_interleaved()];
        decoded.copy_to_slice_interleaved(&mut samples);
        if samples.len() % usize::from(plane_count) != 0 {
            return Err("压缩音频样本不是整帧".into());
        }
        frames = frames.saturating_add((samples.len() / usize::from(plane_count)) as u64);
        if let Some((_, mixer)) = &layout {
            for frame in samples.chunks_exact(usize::from(plane_count)) {
                mixer.push(frame, &mut pcm);
            }
        }
    }
    if pcm.is_empty() || sample_rate == 0 || channels == 0 {
        return Err("没有解出样本".into());
    }
    let duration_ms = frames.saturating_mul(1000) / u64::from(sample_rate);
    Ok(DecodedPcm { sample_rate, channels, pcm, duration_ms })
}

/// 按扬声器位置排的声道给出位置掩码（symphonia 的 `Position` 前 18 位和 WAVE 声道掩码一致，缓冲里的声道
/// 按位从低到高排）。别的表示没有位置，按声道数取常见布局。
fn positioned_mask(channels: &Channels) -> Option<u64> {
    match channels {
        Channels::Positioned(positions) => Some(positions.bits()),
        _ => None,
    }
}

/// 用文件头给探测一个起点。猜错也不会把别的格式认成这种。
fn hint_for(bytes: &[u8]) -> Hint {
    let mut hint = Hint::new();
    if bytes.starts_with(b"fLaC") {
        hint.with_extension("flac");
    } else if bytes.starts_with(b"OggS") {
        hint.with_extension("ogg");
    } else if bytes.starts_with(b"ID3") || bytes.first() == Some(&0xFF) {
        hint.with_extension("mp3");
    }
    hint
}

#[cfg(test)]
pub(crate) fn fixture_mp3() -> &'static [u8] {
    include_bytes!("audio_fixtures/tone.mp3")
}

#[cfg(test)]
pub(crate) fn fixture_flac() -> &'static [u8] {
    include_bytes!("audio_fixtures/tone.flac")
}

#[cfg(test)]
pub(crate) fn fixture_ogg() -> &'static [u8] {
    include_bytes!("audio_fixtures/tone.ogg")
}

#[cfg(test)]
mod tests {
    use super::{decode_compressed, fixture_flac, fixture_mp3, fixture_ogg};

    #[test]
    fn compressed_audio_decodes_flac_mp3_and_ogg_vorbis() {
        let flac = decode_compressed(fixture_flac()).expect("flac");
        assert_eq!(flac.sample_rate, 8_000);
        assert_eq!(flac.channels, 1);
        assert_eq!(flac.duration_ms, 2);
        let samples: Vec<i16> = flac.pcm.chunks_exact(2).map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]])).collect();
        assert_eq!(samples, vec![0, 1000, -1000, 500, 0, 1000, -1000, 500, 0, 1000, -1000, 500, 0, 1000, -1000, 500]);

        let mp3 = decode_compressed(fixture_mp3()).expect("mp3");
        assert!(mp3.sample_rate > 0);
        assert!(mp3.channels == 1 || mp3.channels == 2);
        assert!(mp3.duration_ms > 0);
        assert!(!mp3.pcm.is_empty());

        assert!(fixture_ogg().windows(6).any(|window| window == b"vorbis"));
        let ogg = decode_compressed(fixture_ogg()).expect("ogg");
        assert!(ogg.sample_rate > 0);
        assert!(ogg.channels == 1 || ogg.channels == 2);
        assert!(ogg.duration_ms > 0);
        assert!(!ogg.pcm.is_empty());
    }

    /// 5.1 声道的 FLAC 下混成立体声：前左右整份、中置和后方 -3 dB，低音不进，整体归一不削波。
    #[test]
    fn surround_flac_downmixes_to_stereo() {
        let frame = [1_000i16, -1_000, 0, 20_000, 0, 0];
        let flac = verbatim_flac(8_000, &vec![frame; 16]);
        let decoded = decode_compressed(&flac).expect("5.1 FLAC");
        assert_eq!((decoded.sample_rate, decoded.channels, decoded.duration_ms), (8_000, 2, 2));
        let samples: Vec<i16> = decoded.pcm.chunks_exact(2).map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]])).collect();
        let side = (1_000.0 / (1.0 + std::f32::consts::SQRT_2)).round() as i16;
        assert_eq!(samples, [side, -side].repeat(16));
    }

    /// 只有一个 VERBATIM 帧的 16 位 FLAC。`frames` 里每一项是一帧，各声道一个样本。测试用，不压缩。
    fn verbatim_flac(sample_rate: u32, frames: &[[i16; 6]]) -> Vec<u8> {
        let channels = 6u64;
        let block = frames.len();
        let mut bytes = b"fLaC".to_vec();
        // 最后一个元数据块，类型 STREAMINFO，34 字节。
        bytes.extend_from_slice(&[0x80, 0, 0, 34]);
        bytes.extend_from_slice(&(block as u16).to_be_bytes());
        bytes.extend_from_slice(&(block as u16).to_be_bytes());
        bytes.extend_from_slice(&[0; 6]);
        let packed = (u64::from(sample_rate) << 44) | ((channels - 1) << 41) | (15 << 36) | block as u64;
        bytes.extend_from_slice(&packed.to_be_bytes());
        bytes.extend_from_slice(&[0; 16]);
        // 帧头：同步码、定长块；块长在帧头末尾给 8 位，采样率取 STREAMINFO；6 个独立声道、16 位；帧号 0。
        let start = bytes.len();
        bytes.extend_from_slice(&[0xFF, 0xF8, 0x60, (((channels - 1) as u8) << 4) | (0b100 << 1), 0x00, (block - 1) as u8]);
        let header_crc = crc8(&bytes[start..]);
        bytes.push(header_crc);
        for channel in 0..channels as usize {
            bytes.push(0x02);
            for frame in frames {
                bytes.extend_from_slice(&frame[channel].to_be_bytes());
            }
        }
        let frame_crc = crc16(&bytes[start..]);
        bytes.extend_from_slice(&frame_crc.to_be_bytes());
        bytes
    }

    /// FLAC 帧头的 CRC-8：多项式 x^8 + x^2 + x + 1，初值 0。
    fn crc8(bytes: &[u8]) -> u8 {
        bytes.iter().fold(0u8, |mut crc, byte| {
            crc ^= byte;
            for _ in 0..8 {
                crc = if crc & 0x80 != 0 { (crc << 1) ^ 0x07 } else { crc << 1 };
            }
            crc
        })
    }

    /// FLAC 帧尾的 CRC-16：多项式 x^16 + x^15 + x^2 + 1，初值 0。
    fn crc16(bytes: &[u8]) -> u16 {
        bytes.iter().fold(0u16, |mut crc, byte| {
            crc ^= u16::from(*byte) << 8;
            for _ in 0..8 {
                crc = if crc & 0x8000 != 0 { (crc << 1) ^ 0x8005 } else { crc << 1 };
            }
            crc
        })
    }

    #[test]
    fn compressed_audio_rejects_garbage_video_and_wav() {
        assert!(decode_compressed(b"").is_err());
        assert!(decode_compressed(b"ID3").is_err());
        assert!(decode_compressed(b"not-audio").is_err());
        assert!(decode_compressed(b"\x00\x00\x00\x18ftypmp42\x00\x00\x00\x00mp42isom").is_err());
        let wav = tone_wav();
        assert!(decode_compressed(&wav).is_err());
        let session = super::super::preview_media_session("repo", &wav).expect("wav 仍走原解析");
        assert_eq!(session.status, "paused");
        assert_eq!(session.duration_ms, Some(2));
        assert!(session.can_seek && session.can_volume);
        for bytes in [fixture_mp3(), fixture_flac(), fixture_ogg()] {
            let session = super::super::preview_media_session("repo", bytes).expect("预览");
            assert_eq!(session.status, "paused");
            assert!(session.duration_ms.is_some_and(|duration| duration > 0));
            assert!(session.can_seek && session.can_volume);
            assert!(session.error.is_none());
        }
        assert!(super::super::preview_media_session("repo", b"ID3").is_err());
    }

    fn tone_wav() -> Vec<u8> {
        let data = [0u8, 255, 0, 255, 0, 255, 0, 255, 0, 255, 0, 255, 0, 255, 0, 255];
        let mut body = Vec::new();
        body.extend_from_slice(b"fmt ");
        body.extend_from_slice(&16u32.to_le_bytes());
        body.extend_from_slice(&1u16.to_le_bytes());
        body.extend_from_slice(&1u16.to_le_bytes());
        body.extend_from_slice(&8_000u32.to_le_bytes());
        body.extend_from_slice(&8_000u32.to_le_bytes());
        body.extend_from_slice(&1u16.to_le_bytes());
        body.extend_from_slice(&8u16.to_le_bytes());
        body.extend_from_slice(b"data");
        body.extend_from_slice(&(data.len() as u32).to_le_bytes());
        body.extend_from_slice(&data);
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&((body.len() + 4) as u32).to_le_bytes());
        bytes.extend_from_slice(b"WAVE");
        bytes.extend_from_slice(&body);
        bytes
    }
}
