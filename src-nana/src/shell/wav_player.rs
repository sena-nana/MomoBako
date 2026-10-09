//! 内置 WAV 播放贡献，以及解成 PCM 后共用同一游标的 mp3、flac、ogg。
//!
//! WAV 只解析标准 PCM（RIFF/WAVE、`fmt `、`data`，8-bit 或 16-bit，单声道或双声道）。
//! 压缩格式先解成 16-bit 小端 PCM，再放进这份游标。播放头和音量留在内存里。
//! 正式 Windows 构建在播放成功后用 winmm 异步出声。测试构建不打开设备，单元测试不会出声。

use std::cell::RefCell;
use std::fs;
use std::path::Path;
use std::rc::Rc;

use crate::backend::services::repository::PlaybackSessionState;
use crate::host_api::{PlaybackMediaCapabilities, PlaybackMediaPlugin, PlaybackSessionController};

use super::PlayerCandidate;

pub(super) const PLUGIN_ID: &str = "momobako.player.wav";
pub(super) const PLAYER_TYPE_ID: &str = "momobako.playlist.wav";

const COMPRESSED_EXTENSIONS: &[&str] = &["mp3", "flac", "ogg"];
pub(super) const COMPRESSED_PLUGIN_ID: &str = "momobako.player.compressed-audio";
pub(super) const COMPRESSED_PLAYER_TYPE_ID: &str = "momobako.playlist.compressed-audio";

/// 生产列表里的 WAV 候选。
pub(super) fn builtin_candidate() -> PlayerCandidate {
    audio_candidate(PLUGIN_ID, PLAYER_TYPE_ID, "WAV", &["wav"])
}

/// mp3、flac、ogg 共用一个候选。装载时解进和 WAV 相同的内存游标。
pub(super) fn compressed_candidate() -> PlayerCandidate {
    audio_candidate(COMPRESSED_PLUGIN_ID, COMPRESSED_PLAYER_TYPE_ID, "MP3 / FLAC / Ogg", COMPRESSED_EXTENSIONS)
}

fn audio_candidate(plugin_id: &str, player_type_id: &str, label: &str, extensions: &[&str]) -> PlayerCandidate {
    PlayerCandidate {
        plugin_id: plugin_id.into(),
        player_type_id: player_type_id.into(),
        capability_id: None,
        label: label.into(),
        file_class: "audio".into(),
        extensions: extensions.iter().map(|ext| (*ext).to_string()).collect(),
        supports_seek: true,
        supports_volume: true,
    }
}

pub(super) fn builtin_candidates() -> Vec<PlayerCandidate> {
    vec![builtin_candidate(), compressed_candidate()]
}

/// 一次装载后的内存游标。克隆只复制句柄，播放头仍是同一份。
#[derive(Clone)]
pub(super) struct WavPlayer {
    inner: Rc<RefCell<Cursor>>,
}

struct Cursor {
    loaded: bool,
    sample_rate: u32,
    channels: u16,
    bits_per_sample: u16,
    frame_count: u64,
    /// 已解析的 PCM 负载。声卡只收由它拼出的完整 WAV，不收裸 PCM。
    pcm: Vec<u8>,
    /// PlaySound `SND_MEMORY` 正在读的完整 WAV。停声返回后才能丢掉。
    /// 测试构建不打开设备，因此不保留这块缓冲。
    #[cfg(all(windows, not(test)))]
    playback_wav: Vec<u8>,
    /// 播放头，单位是帧。
    frame: u64,
    volume: f32,
    playing: bool,
}

struct ParsedWav {
    sample_rate: u32,
    channels: u16,
    bits_per_sample: u16,
    frame_count: u64,
    duration_ms: u64,
    pcm: Vec<u8>,
}

/// 预览装进内存游标的 PCM。正式 Windows 构建播放时才交给 winmm，测试构建不打开设备。
#[derive(Clone, PartialEq)]
pub struct PreviewPcm {
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
    pub frame_count: u64,
    pub duration_ms: u64,
    pub pcm: Vec<u8>,
}

impl std::fmt::Debug for PreviewPcm {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PreviewPcm")
            .field("sample_rate", &self.sample_rate)
            .field("channels", &self.channels)
            .field("bits_per_sample", &self.bits_per_sample)
            .field("frame_count", &self.frame_count)
            .field("duration_ms", &self.duration_ms)
            .field("pcm_bytes", &self.pcm.len())
            .finish()
    }
}

struct FmtChunk {
    audio_format: u16,
    channels: u16,
    sample_rate: u32,
    block_align: u16,
    bits_per_sample: u16,
}

impl Default for WavPlayer {
    fn default() -> Self {
        Self {
            inner: Rc::new(RefCell::new(Cursor::empty())),
        }
    }
}

impl WavPlayer {
    /// 丢掉已装载的 PCM 和播放头，并停掉正在播放的声音。
    pub(super) fn clear(&mut self) {
        PlaybackMediaPlugin::dispose(self);
    }

    /// 预览字节已经在内存里，直接装进和播放列表相同的游标。这里不出声。
    pub(super) fn install_preview(&mut self, audio: PreviewPcm) {
        self.clear();
        *self.inner.borrow_mut() = Cursor::from_parsed(ParsedWav {
            sample_rate: audio.sample_rate,
            channels: audio.channels,
            bits_per_sample: audio.bits_per_sample,
            frame_count: audio.frame_count,
            duration_ms: audio.duration_ms,
            pcm: audio.pcm,
        });
    }

    #[cfg(test)]
    pub(super) fn is_playing(&self) -> bool {
        self.inner.borrow().playing
    }
}

impl std::fmt::Debug for WavPlayer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.inner.try_borrow() {
            Ok(cursor) => formatter
                .debug_struct("WavPlayer")
                .field("loaded", &cursor.loaded)
                .field("sample_rate", &cursor.sample_rate)
                .field("channels", &cursor.channels)
                .field("bits_per_sample", &cursor.bits_per_sample)
                .field("frame_count", &cursor.frame_count)
                .field("pcm_bytes", &cursor.pcm.len())
                .field("frame", &cursor.frame)
                .field("volume", &cursor.volume)
                .field("playing", &cursor.playing)
                .finish(),
            Err(_) => formatter.write_str("WavPlayer { <忙> }"),
        }
    }
}

impl PlaybackMediaPlugin for WavPlayer {
    fn load(&mut self, source: &str) -> Result<PlaybackMediaCapabilities, String> {
        self.clear();
        let compressed = is_compressed_path(source);
        match load_source(source) {
            Ok(parsed) => {
                let duration_ms = parsed.duration_ms;
                *self.inner.borrow_mut() = Cursor::from_parsed(parsed);
                Ok(PlaybackMediaCapabilities {
                    duration_ms: Some(duration_ms),
                    can_seek: true,
                    can_volume: true,
                })
            }
            Err(error) => {
                if compressed {
                    eprintln!("Nana 压缩音频装载失败：{error}");
                } else {
                    eprintln!("Nana WAV 装载失败：{error}");
                }
                Err(error)
            }
        }
    }

    fn play(&mut self) -> Result<(), String> {
        self.ensure_loaded("播放")?;
        let mut cursor = self.inner.borrow_mut();
        cursor.playing = true;
        // 设备失败只记日志。会话表示用户要播放，不能因此返回 Err 变成 failed。
        device::start(&mut cursor);
        Ok(())
    }

    fn pause(&mut self) -> Result<(), String> {
        self.ensure_loaded("暂停")?;
        let mut cursor = self.inner.borrow_mut();
        cursor.playing = false;
        device::stop(&mut cursor);
        Ok(())
    }

    fn seek(&mut self, position_ms: u64) -> Result<(), String> {
        self.ensure_loaded("跳转")?;
        let mut cursor = self.inner.borrow_mut();
        let rate = u64::from(cursor.sample_rate);
        let frame = if rate == 0 {
            0
        } else {
            position_ms.saturating_mul(rate) / 1000
        };
        cursor.frame = frame.min(cursor.frame_count);
        // 跳转先停掉当前声音。只有仍在播放时才从新位置再开，暂停时不自动重开。
        if cursor.playing {
            device::start(&mut cursor);
        } else {
            device::stop(&mut cursor);
        }
        Ok(())
    }

    fn set_volume(&mut self, volume: f32) -> Result<(), String> {
        self.ensure_loaded("音量")?;
        // 记在游标里，下次 play 或播放中跳转时再缩放。拖动音量不重开，避免把声音拉回播放头。
        self.inner.borrow_mut().volume = volume;
        Ok(())
    }

    fn dispose(&mut self) {
        let mut cursor = self.inner.borrow_mut();
        device::stop(&mut cursor);
        *cursor = Cursor::empty();
    }
}

impl WavPlayer {
    fn ensure_loaded(&self, action: &str) -> Result<(), String> {
        if self.inner.borrow().loaded {
            return Ok(());
        }
        let error = "WAV 尚未装载".to_string();
        eprintln!("Nana WAV {action}失败：{error}");
        Err(error)
    }
}

impl Cursor {
    fn empty() -> Self {
        Self {
            loaded: false,
            sample_rate: 0,
            channels: 0,
            bits_per_sample: 0,
            frame_count: 0,
            pcm: Vec::new(),
            #[cfg(all(windows, not(test)))]
            playback_wav: Vec::new(),
            frame: 0,
            volume: 1.0,
            playing: false,
        }
    }

    fn from_parsed(parsed: ParsedWav) -> Self {
        Self {
            loaded: true,
            sample_rate: parsed.sample_rate,
            channels: parsed.channels,
            bits_per_sample: parsed.bits_per_sample,
            frame_count: parsed.frame_count,
            pcm: parsed.pcm,
            #[cfg(all(windows, not(test)))]
            playback_wav: Vec::new(),
            frame: 0,
            volume: 1.0,
            playing: false,
        }
    }
}

pub(super) enum Action {
    Play,
    Pause,
    Seek(u64),
    Volume(f32),
}

/// 控制落到哪里：PCM 在游标里就出声；装好了但没有声音（只有画面的视频）只走时钟；
/// 都不是时按缺解码器报错。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Output {
    Cursor,
    Clock,
    Missing,
}

/// 按 [`Output`] 驱动播放、暂停、跳转和音量，返回新的会话和可能的错误。
pub(super) fn drive(
    output: Output,
    wav: &WavPlayer,
    session: PlaybackSessionState,
    action: Action,
) -> (PlaybackSessionState, Option<String>) {
    match output {
        Output::Cursor => control(wav.clone(), session, action),
        Output::Clock => control(ClockOnly, session, action),
        Output::Missing => control(MissingDecoder, session, action),
    }
}

fn control<P: PlaybackMediaPlugin>(
    plugin: P,
    session: PlaybackSessionState,
    action: Action,
) -> (PlaybackSessionState, Option<String>) {
    let mut controller = PlaybackSessionController::new(plugin, session);
    let error = match action {
        Action::Play => controller.play().err(),
        Action::Pause => controller.pause().err(),
        Action::Seek(position) => controller.seek(position).err(),
        Action::Volume(volume) => controller.set_volume(volume).err(),
    };
    (controller.state().clone(), error)
}

/// 只有画面、没有 PCM 的条目：播放、暂停、跳转和音量都只改会话，由播放时钟往前拨。
struct ClockOnly;

impl PlaybackMediaPlugin for ClockOnly {
    fn load(&mut self, _source: &str) -> Result<PlaybackMediaCapabilities, String> {
        Err("只有画面的条目不重复装载".into())
    }

    fn play(&mut self) -> Result<(), String> {
        Ok(())
    }

    fn pause(&mut self) -> Result<(), String> {
        Ok(())
    }

    fn seek(&mut self, _position_ms: u64) -> Result<(), String> {
        Ok(())
    }

    fn set_volume(&mut self, _volume: f32) -> Result<(), String> {
        Ok(())
    }

    fn dispose(&mut self) {}
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

/// WAV 优先。失败后再解 mp3、flac、ogg。两种都不是时返回错误，由调用方记日志。
pub(crate) fn pcm_from_bytes(bytes: &[u8]) -> Result<PreviewPcm, String> {
    let parsed = if let Ok(parsed) = parse_wav(bytes) {
        parsed
    } else {
        parsed_from_decoded(crate::shell::audio_decode::decode_compressed(bytes)?)?
    };
    Ok(PreviewPcm {
        sample_rate: parsed.sample_rate,
        channels: parsed.channels,
        bits_per_sample: parsed.bits_per_sample,
        frame_count: parsed.frame_count,
        duration_ms: parsed.duration_ms,
        pcm: parsed.pcm,
    })
}

/// 按扩展名说明解不开的原因：wav 报 WAV 头的问题，mp3、flac、ogg 报压缩音频解码的问题。
/// 其它扩展名不归这里管，返回 `None`。
pub(crate) fn pcm_error_for_extension(extension: &str, bytes: &[u8]) -> Option<String> {
    let extension = extension.trim().to_ascii_lowercase();
    if extension == "wav" {
        return parse_wav(bytes).err();
    }
    if COMPRESSED_EXTENSIONS.contains(&extension.as_str()) {
        return crate::shell::audio_decode::decode_compressed(bytes).err();
    }
    None
}

/// WAV 和已解码的压缩音频都走这份内存游标。其它候选仍是缺失解码器。
pub(super) fn is_memory_candidate(candidate: &PlayerCandidate) -> bool {
    candidate.plugin_id == PLUGIN_ID || candidate.plugin_id == COMPRESSED_PLUGIN_ID
}

/// 测试构建恒为 false：winmm 模块没有编进来。测试用它确认不会打开声卡。
#[cfg(test)]
pub(crate) fn sound_device_compiled_in() -> bool {
    cfg!(all(windows, not(test)))
}

/// WAV 走原解析。mp3、flac、ogg 读入后解成 16-bit PCM，再装进同一游标。
fn load_source(path: &str) -> Result<ParsedWav, String> {
    if is_compressed_path(path) {
        let bytes = fs::read(path).map_err(|error| format!("无法读取音频：{error}"))?;
        return parsed_from_decoded(crate::shell::audio_decode::decode_compressed(&bytes)?);
    }
    parse_wav_file(path)
}

fn is_compressed_path(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| COMPRESSED_EXTENSIONS.iter().any(|item| item.eq_ignore_ascii_case(ext)))
}

fn parsed_from_decoded(decoded: crate::shell::audio_decode::DecodedPcm) -> Result<ParsedWav, String> {
    let frame_bytes = usize::from(decoded.channels) * 2;
    if frame_bytes == 0 || decoded.pcm.len() % frame_bytes != 0 {
        return Err("压缩音频没有整帧样本".into());
    }
    Ok(ParsedWav {
        sample_rate: decoded.sample_rate,
        channels: decoded.channels,
        bits_per_sample: 16,
        frame_count: decoded.pcm.len() as u64 / frame_bytes as u64,
        duration_ms: decoded.duration_ms,
        pcm: decoded.pcm,
    })
}

/// 读取并解析一个标准 PCM WAV。坏文件返回错误，由调用方记录日志。
fn parse_wav_file(path: &str) -> Result<ParsedWav, String> {
    let bytes = fs::read(path).map_err(|error| format!("无法读取 WAV：{error}"))?;
    parse_wav(&bytes)
}

/// 时长按帧计算。一帧是同一时刻全部声道的样本，单声道时帧数就是样本数。
/// 秒 = 帧数 / 采样率，`duration_ms` = 帧数 × 1000 / 采样率。
fn parse_wav(bytes: &[u8]) -> Result<ParsedWav, String> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("不是 WAV 头".into());
    }
    let mut offset = 12usize;
    let mut fmt = None;
    let mut data = None;
    while offset + 8 <= bytes.len() {
        let id = [
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
        ];
        let size = u32::from_le_bytes([
            bytes[offset + 4],
            bytes[offset + 5],
            bytes[offset + 6],
            bytes[offset + 7],
        ]) as usize;
        let start = offset + 8;
        let end = start.checked_add(size).ok_or("WAV 块超出文件")?;
        if end > bytes.len() {
            return Err("WAV 块超出文件".into());
        }
        match id {
            id if id == *b"fmt " => fmt = Some(parse_fmt(&bytes[start..end])?),
            id if id == *b"data" && data.is_none() => data = Some(bytes[start..end].to_vec()),
            _ => {}
        }
        offset = end;
        if size % 2 == 1 {
            offset = offset.saturating_add(1);
        }
    }
    let fmt = fmt.ok_or("WAV 缺少 fmt 块")?;
    let pcm = data.ok_or("WAV 缺少 data 块")?;
    if fmt.audio_format != 1 {
        return Err("只支持 PCM WAV".into());
    }
    if fmt.channels != 1 && fmt.channels != 2 {
        return Err("只支持单声道或双声道 WAV".into());
    }
    if fmt.bits_per_sample != 8 && fmt.bits_per_sample != 16 {
        return Err("只支持 8-bit 或 16-bit WAV".into());
    }
    if fmt.sample_rate == 0 {
        return Err("WAV 采样率无效".into());
    }
    let frame_bytes = fmt.channels * (fmt.bits_per_sample / 8);
    if frame_bytes == 0 || fmt.block_align != frame_bytes {
        return Err("WAV 帧长无效".into());
    }
    let frame_bytes = u64::from(frame_bytes);
    if pcm.len() as u64 % frame_bytes != 0 {
        return Err("WAV 数据长度不是整帧".into());
    }
    let frame_count = pcm.len() as u64 / frame_bytes;
    let duration_ms = frame_count.saturating_mul(1000) / u64::from(fmt.sample_rate);
    Ok(ParsedWav {
        sample_rate: fmt.sample_rate,
        channels: fmt.channels,
        bits_per_sample: fmt.bits_per_sample,
        frame_count,
        duration_ms,
        pcm,
    })
}

fn parse_fmt(chunk: &[u8]) -> Result<FmtChunk, String> {
    if chunk.len() < 16 {
        return Err("WAV fmt 块不完整".into());
    }
    Ok(FmtChunk {
        audio_format: u16::from_le_bytes([chunk[0], chunk[1]]),
        channels: u16::from_le_bytes([chunk[2], chunk[3]]),
        sample_rate: u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]),
        block_align: u16::from_le_bytes([chunk[12], chunk[13]]),
        bits_per_sample: u16::from_le_bytes([chunk[14], chunk[15]]),
    })
}

/// 正式 Windows 构建用 winmm `PlaySoundW` 异步播放。测试构建不编译本模块，因此不打开设备。
#[cfg(all(windows, not(test)))]
mod device {
    use super::Cursor;

    /// 调用后立刻返回，不阻塞到播完。
    const SND_ASYNC: u32 = 0x0001;
    /// 找不到声音时不要播系统默认提示音。
    const SND_NODEFAULT: u32 = 0x0002;
    /// 参数是内存里的完整 WAV，不是文件路径。
    const SND_MEMORY: u32 = 0x0004;

    #[link(name = "winmm")]
    unsafe extern "system" {
        fn PlaySoundW(psz_sound: *const u16, module: *mut std::ffi::c_void, flags: u32) -> i32;
    }

    /// 从当前帧把剩余 PCM 打成带头 WAV 再异步播放。失败只记日志。
    pub(super) fn start(cursor: &mut Cursor) {
        let prepared = match headed_from(cursor) {
            Ok(bytes) => bytes,
            Err(error) => {
                eprintln!("Nana WAV 播放失败：{error}");
                stop(cursor);
                return;
            }
        };
        let Some(wav) = prepared else {
            stop(cursor);
            return;
        };
        // 先停掉上一声，返回后才能换掉它还在读的缓冲。
        stop(cursor);
        cursor.playback_wav = wav;
        let ok = unsafe {
            // SND_MEMORY 把指针当字节地址用。缓冲留在游标里，直到 stop 返回。
            PlaySoundW(
                cursor.playback_wav.as_ptr().cast(),
                std::ptr::null_mut(),
                SND_ASYNC | SND_NODEFAULT | SND_MEMORY,
            )
        };
        if ok == 0 {
            cursor.playback_wav.clear();
            eprintln!("Nana WAV 播放失败：PlaySoundW 未能播放，会话仍保持播放意图");
        }
    }

    /// 停声。`PlaySoundW` 的空指针会停掉当前波形；返回后才能释放 WAV 缓冲。
    pub(super) fn stop(cursor: &mut Cursor) {
        if cursor.playback_wav.is_empty() {
            return;
        }
        let ok = unsafe { PlaySoundW(std::ptr::null(), std::ptr::null_mut(), 0) };
        cursor.playback_wav.clear();
        if ok == 0 {
            eprintln!("Nana WAV 停止失败：PlaySoundW 返回失败");
        }
    }

    /// 没有剩余样本时返回 `Ok(None)`，这不是设备错误。
    fn headed_from(cursor: &Cursor) -> Result<Option<Vec<u8>>, String> {
        let frame_bytes = usize::from(cursor.channels) * usize::from(cursor.bits_per_sample / 8);
        if frame_bytes == 0 || cursor.sample_rate == 0 {
            return Err("WAV 帧长无效".into());
        }
        let frame = usize::try_from(cursor.frame).map_err(|_| "WAV 播放头超出地址空间")?;
        let start = frame
            .checked_mul(frame_bytes)
            .ok_or("WAV 播放头超出地址空间")?;
        if start >= cursor.pcm.len() {
            return Ok(None);
        }
        let pcm = scale_pcm(&cursor.pcm[start..], cursor.bits_per_sample, cursor.volume)?;
        Ok(Some(headed_wav(
            cursor.sample_rate,
            cursor.channels,
            cursor.bits_per_sample,
            &pcm,
        )?))
    }

    fn scale_pcm(src: &[u8], bits: u16, volume: f32) -> Result<Vec<u8>, String> {
        let volume = volume.clamp(0.0, 1.0);
        if volume >= 1.0 {
            return Ok(src.to_vec());
        }
        if bits == 8 {
            return Ok(src
                .iter()
                .map(|sample| {
                    let centered = f32::from(*sample) - 128.0;
                    (centered * volume + 128.0).round().clamp(0.0, 255.0) as u8
                })
                .collect());
        }
        if bits != 16 {
            return Err("只支持 8-bit 或 16-bit WAV".into());
        }
        let mut out = Vec::with_capacity(src.len());
        for chunk in src.chunks_exact(2) {
            let sample = i16::from_le_bytes([chunk[0], chunk[1]]);
            let mixed = (f32::from(sample) * volume)
                .round()
                .clamp(i16::MIN as f32, i16::MAX as f32) as i16;
            out.extend_from_slice(&mixed.to_le_bytes());
        }
        Ok(out)
    }

    fn headed_wav(
        sample_rate: u32,
        channels: u16,
        bits: u16,
        pcm: &[u8],
    ) -> Result<Vec<u8>, String> {
        let block_align = channels.saturating_mul(bits / 8);
        if block_align == 0 {
            return Err("WAV 帧长无效".into());
        }
        let data_len =
            u32::try_from(pcm.len()).map_err(|_| "WAV 超过 PlaySound 可承载长度".to_string())?;
        let byte_rate = sample_rate
            .checked_mul(u32::from(block_align))
            .ok_or("WAV 字节率溢出")?;
        let pad = data_len % 2;
        let riff_size = 36u32
            .checked_add(data_len)
            .and_then(|size| size.checked_add(pad))
            .ok_or("WAV 超过 PlaySound 可承载长度")?;
        let mut bytes = Vec::with_capacity(44 + pcm.len() + pad as usize);
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&riff_size.to_le_bytes());
        bytes.extend_from_slice(b"WAVE");
        bytes.extend_from_slice(b"fmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&channels.to_le_bytes());
        bytes.extend_from_slice(&sample_rate.to_le_bytes());
        bytes.extend_from_slice(&byte_rate.to_le_bytes());
        bytes.extend_from_slice(&block_align.to_le_bytes());
        bytes.extend_from_slice(&bits.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_len.to_le_bytes());
        bytes.extend_from_slice(pcm);
        if pad == 1 {
            bytes.push(0);
        }
        Ok(bytes)
    }
}

#[cfg(all(windows, not(test)))]
impl Drop for Cursor {
    fn drop(&mut self) {
        // 测试构建没有这个 Drop，不会去停一个没打开的设备。
        device::stop(self);
    }
}

/// 测试构建和非 Windows 保持静音状态机，不打开设备。
#[cfg(not(all(windows, not(test))))]
mod device {
    pub(super) fn start(_cursor: &mut super::Cursor) {}

    pub(super) fn stop(_cursor: &mut super::Cursor) {}
}
