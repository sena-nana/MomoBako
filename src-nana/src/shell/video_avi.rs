//! 纯 Rust 的 AVI 画面和音轨。
//!
//! 只解未压缩 BI_RGB 和 MJPEG。其它压缩交给系统媒体基础。
//! 这里不出声，也不打开窗口。

use crate::shell::player::PreviewPcm;

use super::{DecodedClip, VideoFrame};

/// 能解就返回画面。认不出或不是这两种编码时返回 `Ok(None)`，让调用方继续试系统解码。
pub(super) fn try_decode(bytes: &[u8]) -> Result<Option<DecodedClip>, String> {
    let Some(avi) = parse(bytes) else {
        return Ok(None);
    };
    let Some(video) = avi.streams.iter().enumerate().find(|(_, stream)| stream.video) else {
        return Ok(None);
    };
    if !supported_video(video.1) {
        return Ok(None);
    }
    let (index, stream) = video;
    let mut frames = Vec::new();
    let mut stored = 0usize;
    for (ordinal, payload) in avi.frames.iter().filter(|(stream_index, _)| *stream_index == index) {
        let _ = ordinal;
        let frame = decode_frame(stream, payload)?;
        stored = stored.saturating_add(frame.rgba.len());
        if stored > super::FRAME_BYTE_BUDGET {
            eprintln!("Nana AVI 画面超过预览内存上限，不把截断结果当成完整视频");
            return Err("AVI 画面超过预览内存上限".into());
        }
        frames.push(frame);
    }
    if frames.is_empty() {
        eprintln!("Nana AVI 没有解出画面");
        return Err("AVI 没有解出画面".into());
    }
    let rate = u64::from(stream.rate.max(1));
    let scale = u64::from(stream.scale.max(1));
    for (ordinal, frame) in frames.iter_mut().enumerate() {
        frame.time_ms = (ordinal as u64).saturating_mul(scale).saturating_mul(1_000) / rate;
    }
    let duration_ms = (frames.len() as u64).saturating_mul(scale).saturating_mul(1_000) / rate;
    let pcm = avi.streams.iter().enumerate().find(|(_, stream)| stream.audio).and_then(|(audio_index, stream)| {
        let mut pcm = Vec::new();
        for (stream_index, payload) in &avi.audio {
            if *stream_index == audio_index {
                pcm.extend_from_slice(payload);
            }
        }
        pcm_of(stream, pcm)
    });
    let duration_ms = duration_ms.max(pcm.as_ref().map(|audio| audio.duration_ms).unwrap_or(0));
    Ok(Some(DecodedClip { duration_ms, frames, pcm }))
}

fn supported_video(stream: &Stream) -> bool {
    stream.compression == 0 || stream.compression == u32::from_le_bytes(*b"MJPG")
}

fn decode_frame(stream: &Stream, payload: &[u8]) -> Result<VideoFrame, String> {
    if stream.compression == u32::from_le_bytes(*b"MJPG") {
        return decode_jpeg(payload);
    }
    decode_dib(stream, payload)
}

/// JPEG 帧走仓库里的 `image`。解不开就返回错误，不拿空图像冒充成功。
fn decode_jpeg(payload: &[u8]) -> Result<VideoFrame, String> {
    let image = image::load_from_memory(payload).map_err(|error| {
        eprintln!("Nana MJPEG 帧解码失败：{error}");
        format!("MJPEG 帧解码失败：{error}")
    })?;
    let rgba = image.to_rgba8();
    Ok(VideoFrame {
        time_ms: 0,
        width: rgba.width(),
        height: rgba.height(),
        rgba: rgba.into_raw(),
    })
}

fn decode_dib(stream: &Stream, payload: &[u8]) -> Result<VideoFrame, String> {
    let width = stream.width;
    let height = stream.height;
    if width == 0 || height == 0 || (stream.bit_count != 24 && stream.bit_count != 32) {
        return Err("AVI 图像格式不受支持".into());
    }
    let channels = usize::from(stream.bit_count / 8);
    let stride = ((width * channels + 3) / 4) * 4;
    let rows = height;
    let needed = stride.saturating_mul(rows);
    if payload.len() < needed {
        eprintln!("Nana AVI 帧长度不足：{} < {needed}", payload.len());
        return Err("AVI 帧长度不足".into());
    }
    let mut rgba = vec![0u8; width * height * 4];
    for y in 0..height {
        let src_y = if stream.bottom_up { height - 1 - y } else { y };
        let row = &payload[src_y * stride..src_y * stride + width * channels];
        for x in 0..width {
            let pixel = &row[x * channels..];
            let dest = (y * width + x) * 4;
            rgba[dest] = pixel[2];
            rgba[dest + 1] = pixel[1];
            rgba[dest + 2] = pixel[0];
            rgba[dest + 3] = 255;
        }
    }
    Ok(VideoFrame { time_ms: 0, width: width as u32, height: height as u32, rgba })
}

fn pcm_of(stream: &Stream, pcm: Vec<u8>) -> Option<PreviewPcm> {
    if stream.audio_format != 1 || stream.sample_rate == 0 {
        return None;
    }
    if stream.channels != 1 && stream.channels != 2 {
        return None;
    }
    if stream.bits != 8 && stream.bits != 16 {
        return None;
    }
    let frame_bytes = usize::from(stream.channels) * usize::from(stream.bits / 8);
    if frame_bytes == 0 || pcm.len() % frame_bytes != 0 || pcm.is_empty() {
        return None;
    }
    let frame_count = pcm.len() as u64 / frame_bytes as u64;
    let duration_ms = frame_count.saturating_mul(1_000) / u64::from(stream.sample_rate);
    Some(PreviewPcm {
        sample_rate: stream.sample_rate,
        channels: stream.channels,
        bits_per_sample: stream.bits,
        frame_count,
        duration_ms,
        pcm,
    })
}

struct Stream {
    video: bool,
    audio: bool,
    width: usize,
    height: usize,
    bottom_up: bool,
    bit_count: u16,
    compression: u32,
    scale: u32,
    rate: u32,
    audio_format: u16,
    channels: u16,
    sample_rate: u32,
    bits: u16,
}

struct Avi {
    streams: Vec<Stream>,
    frames: Vec<(usize, Vec<u8>)>,
    audio: Vec<(usize, Vec<u8>)>,
}

fn parse(bytes: &[u8]) -> Option<Avi> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"AVI " {
        return None;
    }
    let mut avi = Avi { streams: Vec::new(), frames: Vec::new(), audio: Vec::new() };
    walk(&bytes[12..], &mut avi);
    (!avi.streams.is_empty()).then_some(avi)
}

fn walk(data: &[u8], avi: &mut Avi) {
    for_each_chunk(data, |id, payload| {
        if id != *b"LIST" || payload.len() < 4 {
            return;
        }
        let kind = [payload[0], payload[1], payload[2], payload[3]];
        let body = &payload[4..];
        match &kind {
            b"hdrl" => collect_streams(body, avi),
            b"movi" => collect_media(body, avi),
            _ => walk(body, avi),
        }
    });
}

fn collect_streams(data: &[u8], avi: &mut Avi) {
    for_each_chunk(data, |id, payload| {
        if id == *b"LIST" && payload.len() >= 4 && &payload[0..4] == b"strl" {
            if let Some(stream) = parse_strl(&payload[4..]) {
                avi.streams.push(stream);
            }
        }
    });
}

fn parse_strl(data: &[u8]) -> Option<Stream> {
    let mut header = None;
    let mut format = None;
    for_each_chunk(data, |id, payload| {
        if id == *b"strh" {
            header = Some(payload.to_vec());
        } else if id == *b"strf" {
            format = Some(payload.to_vec());
        }
    });
    let header = header?;
    let format = format?;
    if header.len() < 48 {
        return None;
    }
    let fcc = [header[0], header[1], header[2], header[3]];
    let scale = u32::from_le_bytes(header[20..24].try_into().ok()?);
    let rate = u32::from_le_bytes(header[24..28].try_into().ok()?);
    let mut stream = Stream {
        video: &fcc == b"vids",
        audio: &fcc == b"auds",
        width: 0,
        height: 0,
        bottom_up: true,
        bit_count: 0,
        compression: 0,
        scale: scale.max(1),
        rate: rate.max(1),
        audio_format: 0,
        channels: 0,
        sample_rate: 0,
        bits: 0,
    };
    if stream.video && format.len() >= 20 {
        let height = i32::from_le_bytes(format[8..12].try_into().ok()?);
        stream.bottom_up = height > 0;
        stream.width = i32::from_le_bytes(format[4..8].try_into().ok()?).unsigned_abs() as usize;
        stream.height = height.unsigned_abs() as usize;
        stream.bit_count = u16::from_le_bytes(format[14..16].try_into().ok()?);
        stream.compression = u32::from_le_bytes(format[16..20].try_into().ok()?);
    }
    if stream.audio && format.len() >= 16 {
        stream.audio_format = u16::from_le_bytes(format[0..2].try_into().ok()?);
        stream.channels = u16::from_le_bytes(format[2..4].try_into().ok()?);
        stream.sample_rate = u32::from_le_bytes(format[4..8].try_into().ok()?);
        stream.bits = u16::from_le_bytes(format[14..16].try_into().ok()?);
    }
    Some(stream)
}

fn collect_media(data: &[u8], avi: &mut Avi) {
    for_each_chunk(data, |id, payload| {
        let Some(index) = stream_index(&id) else {
            return;
        };
        match &id[2..4] {
            b"db" | b"dc" => avi.frames.push((index, payload.to_vec())),
            b"wb" => avi.audio.push((index, payload.to_vec())),
            _ => {}
        }
    });
}

fn stream_index(id: &[u8; 4]) -> Option<usize> {
    let tens = id[0];
    let ones = id[1];
    if !tens.is_ascii_digit() || !ones.is_ascii_digit() {
        return None;
    }
    Some(usize::from(tens - b'0') * 10 + usize::from(ones - b'0'))
}

fn for_each_chunk(data: &[u8], mut visit: impl FnMut([u8; 4], &[u8])) {
    let mut at = 0usize;
    while at + 8 <= data.len() {
        let id = [data[at], data[at + 1], data[at + 2], data[at + 3]];
        let size = u32::from_le_bytes([data[at + 4], data[at + 5], data[at + 6], data[at + 7]]) as usize;
        let start = at + 8;
        let Some(end) = start.checked_add(size) else {
            break;
        };
        if end > data.len() {
            break;
        }
        visit(id, &data[start..end]);
        at = end + usize::from(size % 2 == 1);
    }
}

/// 两帧 2×2 未压缩画面加一小段 PCM。测试用，不代表外部文件。
#[cfg(test)]
pub(crate) fn sample_uncompressed() -> Vec<u8> {
    let video = stream_list(b"vids", 1, 2, &dib_format(2, 2, 32));
    let audio = stream_list(b"auds", 1, 8_000, &wav_format());
    let mut hdrl = Vec::new();
    push_chunk(&mut hdrl, b"avih", &[0u8; 56]);
    hdrl.extend_from_slice(&video);
    hdrl.extend_from_slice(&audio);
    let mut movi = Vec::new();
    push_chunk(&mut movi, b"00db", &solid(2, 2, [255, 0, 0, 255]));
    push_chunk(&mut movi, b"00db", &solid(2, 2, [0, 0, 255, 255]));
    push_chunk(&mut movi, b"01wb", &vec![0u8, 0, 255, 127, 0, 0, 255, 127, 0, 0, 1, 0, 2, 0, 3, 0]);
    let mut body = Vec::new();
    push_list(&mut body, b"hdrl", &hdrl);
    push_list(&mut body, b"movi", &movi);
    let mut file = Vec::new();
    file.extend_from_slice(b"RIFF");
    file.extend_from_slice(&(body.len() as u32 + 4).to_le_bytes());
    file.extend_from_slice(b"AVI ");
    file.extend_from_slice(&body);
    file
}

#[cfg(test)]
fn stream_list(fcc: &[u8; 4], scale: u32, rate: u32, format: &[u8]) -> Vec<u8> {
    let mut header = vec![0u8; 56];
    header[0..4].copy_from_slice(fcc);
    header[20..24].copy_from_slice(&scale.to_le_bytes());
    header[24..28].copy_from_slice(&rate.to_le_bytes());
    let mut children = Vec::new();
    push_chunk(&mut children, b"strh", &header);
    push_chunk(&mut children, b"strf", format);
    let mut list = Vec::new();
    push_list(&mut list, b"strl", &children);
    list
}

#[cfg(test)]
fn dib_format(width: i32, height: i32, bits: u16) -> Vec<u8> {
    let mut format = vec![0u8; 40];
    format[0..4].copy_from_slice(&40u32.to_le_bytes());
    format[4..8].copy_from_slice(&width.to_le_bytes());
    format[8..12].copy_from_slice(&height.to_le_bytes());
    format[12..14].copy_from_slice(&1u16.to_le_bytes());
    format[14..16].copy_from_slice(&bits.to_le_bytes());
    format
}

#[cfg(test)]
fn wav_format() -> Vec<u8> {
    let mut format = vec![0u8; 16];
    format[0..2].copy_from_slice(&1u16.to_le_bytes());
    format[2..4].copy_from_slice(&1u16.to_le_bytes());
    format[4..8].copy_from_slice(&8_000u32.to_le_bytes());
    format[8..12].copy_from_slice(&16_000u32.to_le_bytes());
    format[12..14].copy_from_slice(&2u16.to_le_bytes());
    format[14..16].copy_from_slice(&16u16.to_le_bytes());
    format
}

#[cfg(test)]
fn solid(width: usize, height: usize, rgba: [u8; 4]) -> Vec<u8> {
    let mut bgra = Vec::new();
    for _ in 0..height {
        for _ in 0..width {
            bgra.extend_from_slice(&[rgba[2], rgba[1], rgba[0], 255]);
        }
    }
    bgra.reverse_rows(width);
    bgra
}

#[cfg(test)]
trait ReverseRows {
    fn reverse_rows(&mut self, width: usize);
}

#[cfg(test)]
impl ReverseRows for Vec<u8> {
    fn reverse_rows(&mut self, width: usize) {
        let stride = width * 4;
        let height = self.len() / stride;
        let copy = self.clone();
        for y in 0..height {
            let src = (height - 1 - y) * stride;
            let dest = y * stride;
            self[dest..dest + stride].copy_from_slice(&copy[src..src + stride]);
        }
    }
}

#[cfg(test)]
fn push_chunk(buf: &mut Vec<u8>, id: &[u8; 4], data: &[u8]) {
    buf.extend_from_slice(id);
    buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
    buf.extend_from_slice(data);
    if data.len() % 2 == 1 {
        buf.push(0);
    }
}

#[cfg(test)]
fn push_list(buf: &mut Vec<u8>, kind: &[u8; 4], children: &[u8]) {
    buf.extend_from_slice(b"LIST");
    buf.extend_from_slice(&((4 + children.len()) as u32).to_le_bytes());
    buf.extend_from_slice(kind);
    buf.extend_from_slice(children);
    if children.len() % 2 == 1 {
        buf.push(0);
    }
}
