#![allow(unsafe_op_in_unsafe_fn)]
//! Windows 媒体基础解码。
//!
//! 用源读取器把画面解成 RGB，把音轨解成 PCM。不创建播放会话，不打开窗口，也不把样本送进声卡。
//! 正式播放仍走已有的内存游标；测试构建不会调用 winmm。

use std::ptr;

use windows::Win32::Media::MediaFoundation::{
    IMFMediaBuffer, IMFSample, IMFSourceReader, MFAudioFormat_PCM, MFCreateMFByteStreamOnStream, MFCreateMediaType,
    MFCreateSourceReaderFromByteStream, MFMediaType_Audio, MFMediaType_Video, MFShutdown, MFStartup, MFVideoFormat_RGB32,
    MFSTARTUP_NOSOCKET, MF_MT_AUDIO_BITS_PER_SAMPLE, MF_MT_AUDIO_NUM_CHANNELS, MF_MT_AUDIO_SAMPLES_PER_SECOND,
    MF_MT_DEFAULT_STRIDE, MF_MT_FRAME_SIZE, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MF_SOURCE_READERF_ENDOFSTREAM,
    MF_SOURCE_READERF_ERROR, MF_SOURCE_READER_FIRST_AUDIO_STREAM, MF_SOURCE_READER_FIRST_VIDEO_STREAM, MF_VERSION,
};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};
use windows::Win32::UI::Shell::SHCreateMemStream;

use crate::shell::player::PreviewPcm;

use super::{DecodedClip, VideoFrame, FRAME_BYTE_BUDGET, PCM_BYTE_BUDGET};

/// 解一种已经认出的容器。失败文案带容器名和「解码失败」，不再把所有视频都说成没有解码器。
pub(super) fn decode(bytes: &[u8], kind: &str) -> Result<DecodedClip, String> {
    let mut apartment = Apartment::enter();
    let outcome = unsafe { pull(bytes, kind) };
    apartment.leave();
    outcome
}

struct Apartment {
    com: bool,
    media: bool,
}

impl Apartment {
    fn enter() -> Self {
        let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if com.is_err() {
            eprintln!("Nana 媒体基础 COM 初始化返回 {com:?}，仍继续尝试解码");
        }
        let media = unsafe { MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET) };
        if let Err(error) = &media {
            eprintln!("Nana 媒体基础启动失败：{error}");
        }
        Self { com: com.is_ok(), media: media.is_ok() }
    }

    fn leave(&mut self) {
        unsafe {
            if self.media {
                if let Err(error) = MFShutdown() {
                    eprintln!("Nana 媒体基础关闭失败：{error}");
                }
            }
            if self.com {
                CoUninitialize();
            }
        }
        self.media = false;
        self.com = false;
    }
}

unsafe fn pull(bytes: &[u8], kind: &str) -> Result<DecodedClip, String> {
    let stream = SHCreateMemStream(Some(bytes)).ok_or_else(|| failed(kind, "无法建立内存字节流"))?;
    let byte_stream = MFCreateMFByteStreamOnStream(&stream).map_err(|error| failed(kind, &error.to_string()))?;
    let reader = MFCreateSourceReaderFromByteStream(&byte_stream, None).map_err(|error| failed(kind, &error.to_string()))?;
    let video_index = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
    let audio_index = MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32;
    let video_ready = match configure_video(&reader, video_index) {
        Ok(()) => true,
        Err(error) => {
            eprintln!("Nana {kind} 没有可解的画面：{error}");
            false
        }
    };
    let audio_ready = configure_audio(&reader, audio_index);
    if !video_ready && !audio_ready {
        return Err(failed(kind, "没有可解的画面或音轨"));
    }
    let layout = if video_ready { video_layout(&reader, video_index).ok() } else { None };
    if video_ready && layout.is_none() {
        eprintln!("Nana {kind} 画面尺寸读取失败");
    }
    let mut frames = Vec::new();
    let mut stored = 0usize;
    let mut last_time = 0u64;
    let mut reached_end = false;
    if let Some((width, height, stride)) = layout {
        for _ in 0..50_000 {
            let sample = read_sample(&reader, video_index).map_err(|error| failed(kind, &error))?;
            match sample {
                Sample::End => {
                    reached_end = true;
                    break;
                }
                Sample::Empty => {}
                Sample::Data { time_ms, bytes } => {
                    last_time = time_ms;
                    if frames.last().is_some_and(|frame: &VideoFrame| time_ms.saturating_sub(frame.time_ms) < 80) {
                        continue;
                    }
                    let frame = rgba_frame(time_ms, width, height, stride, &bytes).map_err(|error| failed(kind, &error))?;
                    stored = stored.saturating_add(frame.rgba.len());
                    if stored > FRAME_BYTE_BUDGET {
                        eprintln!("Nana 视频画面在 {time_ms} ms 达到内存上限，会话时长按已解出的部分");
                        break;
                    }
                    frames.push(frame);
                }
            }
        }
    }
    if frames.is_empty() {
        if audio_ready {
            match read_audio(&reader, audio_index, 0) {
                Ok(pcm) if !pcm.pcm.is_empty() => {
                    eprintln!("Nana {kind} 没有画面，改用音轨");
                    return Ok(DecodedClip { duration_ms: pcm.duration_ms.max(1), frames, pcm: Some(pcm) });
                }
                Ok(_) => eprintln!("Nana {kind} 音轨没有样本"),
                Err(error) => eprintln!("Nana {kind} 音轨解码失败：{error}"),
            }
        }
        eprintln!("Nana {kind} 没有解出画面");
        return Err(failed(kind, "没有解出画面"));
    }
    let pcm = if audio_ready { read_audio(&reader, audio_index, last_time).ok() } else { None };
    let duration_ms = last_time.max(frames.last().map(|frame| frame.time_ms).unwrap_or(0)).max(pcm.as_ref().map(|audio| audio.duration_ms).unwrap_or(0));
    if !reached_end {
        eprintln!("Nana {kind} 解码停在 {duration_ms} ms，播放时长按已解出的画面");
    }
    let _ = reached_end;
    Ok(DecodedClip { duration_ms: duration_ms.max(1), frames, pcm })
}

fn failed(kind: &str, detail: &str) -> String {
    format!("{kind} 解码失败：{detail}")
}

unsafe fn configure_video(reader: &IMFSourceReader, index: u32) -> Result<(), String> {
    let media_type = MFCreateMediaType().map_err(|error| error.to_string())?;
    media_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video).map_err(|error| error.to_string())?;
    media_type.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32).map_err(|error| error.to_string())?;
    reader.SetCurrentMediaType(index, None, &media_type).map_err(|error| error.to_string())
}

unsafe fn configure_audio(reader: &IMFSourceReader, index: u32) -> bool {
    let media_type = match MFCreateMediaType() {
        Ok(media_type) => media_type,
        Err(error) => {
            eprintln!("Nana 视频音轨类型创建失败：{error}");
            return false;
        }
    };
    if media_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio).is_err()
        || media_type.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM).is_err()
        || media_type.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16).is_err()
    {
        eprintln!("Nana 视频音轨无法指定 PCM");
        return false;
    }
    if let Err(error) = reader.SetCurrentMediaType(index, None, &media_type) {
        eprintln!("Nana 视频没有可解的音轨：{error}");
        return false;
    }
    true
}

unsafe fn video_layout(reader: &IMFSourceReader, index: u32) -> Result<(u32, u32, i32), String> {
    let media_type = reader.GetCurrentMediaType(index).map_err(|error| error.to_string())?;
    let packed = media_type.GetUINT64(&MF_MT_FRAME_SIZE).map_err(|error| error.to_string())?;
    let width = (packed >> 32) as u32;
    let height = packed as u32;
    if width == 0 || height == 0 || width > 7680 || height > 4320 {
        return Err(format!("视频尺寸无效：{width}×{height}"));
    }
    let stride = media_type.GetUINT32(&MF_MT_DEFAULT_STRIDE).map(|value| value as i32).unwrap_or(width as i32 * 4);
    Ok((width, height, stride))
}

enum Sample {
    End,
    Empty,
    Data { time_ms: u64, bytes: Vec<u8> },
}

unsafe fn read_sample(reader: &IMFSourceReader, index: u32) -> Result<Sample, String> {
    let mut flags = 0u32;
    let mut time = 0i64;
    let mut sample = None;
    reader
        .ReadSample(index, 0, None, Some(&mut flags), Some(&mut time), Some(&mut sample))
        .map_err(|error| error.to_string())?;
    if flags & (MF_SOURCE_READERF_ENDOFSTREAM.0 as u32) != 0 {
        return Ok(Sample::End);
    }
    if flags & (MF_SOURCE_READERF_ERROR.0 as u32) != 0 {
        return Err("读取视频样本失败".into());
    }
    let Some(sample) = sample else {
        return Ok(Sample::Empty);
    };
    let bytes = copy_sample(&sample)?;
    let time_ms = u64::try_from(time / 10_000).unwrap_or(0);
    Ok(Sample::Data { time_ms, bytes })
}

unsafe fn copy_sample(sample: &IMFSample) -> Result<Vec<u8>, String> {
    let buffer: IMFMediaBuffer = sample.ConvertToContiguousBuffer().map_err(|error| error.to_string())?;
    let mut data = ptr::null_mut();
    let mut length = 0u32;
    buffer.Lock(&mut data, None, Some(&mut length)).map_err(|error| error.to_string())?;
    let copied = if data.is_null() || length == 0 {
        Vec::new()
    } else {
        std::slice::from_raw_parts(data, length as usize).to_vec()
    };
    if let Err(error) = buffer.Unlock() {
        eprintln!("Nana 视频缓冲解锁失败：{error}");
    }
    Ok(copied)
}

fn rgba_frame(time_ms: u64, width: u32, height: u32, stride: i32, bytes: &[u8]) -> Result<VideoFrame, String> {
    let row_stride = stride.unsigned_abs() as usize;
    let bottom_up = stride < 0;
    let needed = row_stride.saturating_mul(height as usize);
    if row_stride < width as usize * 4 || bytes.len() < needed {
        return Err("视频帧缓冲不足".into());
    }
    let (width, height, rgba) = scale_bgra(width, height, row_stride, bottom_up, bytes);
    Ok(VideoFrame { time_ms, width, height, rgba })
}

/// 媒体基础的 RGB32 是 BGRA。预览纹理要 RGBA，并把长边收到 320 以内。
fn scale_bgra(width: u32, height: u32, stride: usize, bottom_up: bool, bytes: &[u8]) -> (u32, u32, Vec<u8>) {
    let max_edge = width.max(height).max(1);
    let (out_w, out_h) = if max_edge <= 320 {
        (width, height)
    } else {
        let scale = 320.0 / max_edge as f32;
        ((width as f32 * scale).max(1.0) as u32, (height as f32 * scale).max(1.0) as u32)
    };
    let mut rgba = vec![0u8; out_w as usize * out_h as usize * 4];
    for y in 0..out_h {
        let src_y = (y as u64 * u64::from(height) / u64::from(out_h)) as u32;
        let src_y = if bottom_up { height - 1 - src_y } else { src_y };
        for x in 0..out_w {
            let src_x = (x as u64 * u64::from(width) / u64::from(out_w)) as usize;
            let src = src_y as usize * stride + src_x * 4;
            let dest = (y as usize * out_w as usize + x as usize) * 4;
            if src + 3 < bytes.len() {
                rgba[dest] = bytes[src + 2];
                rgba[dest + 1] = bytes[src + 1];
                rgba[dest + 2] = bytes[src];
                rgba[dest + 3] = 255;
            }
        }
    }
    (out_w, out_h, rgba)
}

unsafe fn read_audio(reader: &IMFSourceReader, index: u32, limit_ms: u64) -> Result<PreviewPcm, String> {
    let media_type = reader.GetCurrentMediaType(index).map_err(|error| error.to_string())?;
    let sample_rate = media_type.GetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND).unwrap_or(0);
    let channels = media_type.GetUINT32(&MF_MT_AUDIO_NUM_CHANNELS).unwrap_or(0) as u16;
    let bits = media_type.GetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE).unwrap_or(0) as u16;
    if sample_rate == 0 || (channels != 1 && channels != 2) || bits != 16 {
        return Err("音轨不是 16-bit 立体声或单声道".into());
    }
    let mut pcm = Vec::new();
    for _ in 0..200_000 {
        if pcm.len() > PCM_BYTE_BUDGET {
            eprintln!("Nana 视频音轨达到内存上限，播放时长按已解出的部分");
            break;
        }
        match read_sample(reader, index)? {
            Sample::End => break,
            Sample::Empty => {}
            Sample::Data { time_ms, bytes } => {
                pcm.extend_from_slice(&bytes);
                if limit_ms > 0 && time_ms > limit_ms.saturating_add(1_000) {
                    break;
                }
            }
        }
    }
    let frame_bytes = usize::from(channels) * 2;
    if frame_bytes == 0 || pcm.len() < frame_bytes {
        return Err("音轨没有样本".into());
    }
    let usable = pcm.len() - pcm.len() % frame_bytes;
    pcm.truncate(usable);
    let frame_count = pcm.len() as u64 / frame_bytes as u64;
    let duration_ms = frame_count.saturating_mul(1_000) / u64::from(sample_rate);
    Ok(PreviewPcm {
        sample_rate,
        channels,
        bits_per_sample: 16,
        frame_count,
        duration_ms,
        pcm,
    })
}
