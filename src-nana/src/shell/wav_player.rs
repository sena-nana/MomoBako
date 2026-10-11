//! 内置 WAV 播放贡献，以及解成 PCM 后共用同一游标的 mp3、flac、ogg；Windows 上还有媒体基础解出的 m4a、aac、opus。
//!
//! WAV 读 RIFF/WAVE 的 `fmt ` 和 `data`：整数 PCM（8/16/24/32 位）、浮点（32/64 位）和 WAVE_FORMAT_EXTENSIBLE，
//! 8/16-bit 单双声道原样进游标，其余换成 16-bit，三个以上声道下混成立体声（`audio_pcm.rs`）。
//! 压缩格式先解成 16-bit 小端 PCM，再放进这份游标。播放头和音量留在内存里。
//! 正式 Windows 构建在播放成功后用 winmm 异步出声。测试构建不打开设备，单元测试不会出声。

use std::cell::RefCell;
use std::rc::Rc;

use crate::backend::services::repository::PlaybackSessionState;
use crate::host_api::{PlaybackMediaPlugin, PlaybackSessionController};
use crate::shell::audio_decode::pcm::WaveFormat;

use super::PlayerCandidate;

pub(super) const PLUGIN_ID: &str = "momobako.player.wav";
pub(super) const PLAYER_TYPE_ID: &str = "momobako.playlist.wav";

const COMPRESSED_EXTENSIONS: &[&str] = &["mp3", "flac", "ogg"];
pub(super) const COMPRESSED_PLUGIN_ID: &str = "momobako.player.compressed-audio";
pub(super) const COMPRESSED_PLAYER_TYPE_ID: &str = "momobako.playlist.compressed-audio";

/// 交给系统媒体基础解音轨的格式。媒体基础只在 Windows 上有。
const SYSTEM_EXTENSIONS: &[&str] = &["m4a", "aac", "opus"];
pub(super) const SYSTEM_PLUGIN_ID: &str = "momobako.player.system-audio";
pub(super) const SYSTEM_PLAYER_TYPE_ID: &str = "momobako.playlist.system-audio";

/// 生产列表里的 WAV 候选。
pub(super) fn builtin_candidate() -> PlayerCandidate {
    audio_candidate(PLUGIN_ID, PLAYER_TYPE_ID, "WAV", &["wav"])
}

/// mp3、flac、ogg 共用一个候选。装载时解进和 WAV 相同的内存游标。
pub(super) fn compressed_candidate() -> PlayerCandidate {
    audio_candidate(COMPRESSED_PLUGIN_ID, COMPRESSED_PLAYER_TYPE_ID, "MP3 / FLAC / Ogg", COMPRESSED_EXTENSIONS)
}

/// m4a、aac、opus 共用一个候选。媒体基础把音轨解成 PCM 后，同样装进 WAV 的内存游标。
pub(super) fn system_candidate() -> PlayerCandidate {
    audio_candidate(SYSTEM_PLUGIN_ID, SYSTEM_PLAYER_TYPE_ID, "M4A / AAC / Opus", SYSTEM_EXTENSIONS)
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

/// 本平台 Nana 自己能解的音频格式各有一个候选认领。没有媒体基础的平台不认领 m4a、aac、opus。
pub(super) fn builtin_candidates() -> Vec<PlayerCandidate> {
    candidates_for(cfg!(windows))
}

/// 内置候选。`media_foundation` 为真时再登记交给媒体基础的格式。
pub(super) fn candidates_for(media_foundation: bool) -> Vec<PlayerCandidate> {
    let mut candidates = vec![builtin_candidate(), compressed_candidate()];
    if media_foundation {
        candidates.push(system_candidate());
    }
    candidates
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
    /// 播放头，单位是帧。播放中不随声卡前进：播放时钟只推进会话进度，重新出声前由 [`drive`] 对齐到会话进度。
    frame: u64,
    volume: f32,
    playing: bool,
    /// 测试构建不开声卡，记下最近一次出声的起点帧和音量，停声后为空。单元测试据此核对出声位置。
    #[cfg(test)]
    sounding: Option<(u64, f32)>,
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
        let mut cursor = self.inner.borrow_mut();
        device::stop(&mut cursor);
        *cursor = Cursor::empty();
    }

    /// 预览字节已经在内存里，直接装进和播放列表相同的游标。这里不出声。
    pub(super) fn install_preview(&mut self, audio: PreviewPcm) {
        self.clear();
        *self.inner.borrow_mut() = Cursor::from_pcm(audio);
    }

    #[cfg(test)]
    pub(super) fn is_playing(&self) -> bool {
        self.inner.borrow().playing
    }

    /// 测试用：声卡这时会从哪一帧、按什么音量出声。没在出声时为空。
    #[cfg(test)]
    pub(super) fn sounding(&self) -> Option<(u64, f32)> {
        self.inner.borrow().sounding
    }

    /// 把播放头放到 `position_ms`，不动声卡。只在重新出声之前调用，见 [`drive`]。
    fn locate(&self, position_ms: u64) {
        let mut cursor = self.inner.borrow_mut();
        cursor.frame = cursor.frame_at(position_ms);
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
        cursor.frame = cursor.frame_at(position_ms);
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
        let mut cursor = self.inner.borrow_mut();
        cursor.volume = volume;
        // PlaySound 出声以后改不了音量，正在出声时按新音量从播放头重开；暂停时只记下，下次出声时用。
        if cursor.playing {
            device::start(&mut cursor);
        }
        Ok(())
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
            #[cfg(test)]
            sounding: None,
        }
    }

    fn from_pcm(audio: PreviewPcm) -> Self {
        Self {
            loaded: true,
            sample_rate: audio.sample_rate,
            channels: audio.channels,
            bits_per_sample: audio.bits_per_sample,
            frame_count: audio.frame_count,
            pcm: audio.pcm,
            #[cfg(all(windows, not(test)))]
            playback_wav: Vec::new(),
            frame: 0,
            volume: 1.0,
            playing: false,
            #[cfg(test)]
            sounding: None,
        }
    }

    /// 毫秒换成帧：帧 = 毫秒 × 采样率 / 1000，夹在总帧数以内。
    fn frame_at(&self, position_ms: u64) -> u64 {
        let rate = u64::from(self.sample_rate);
        let frame = if rate == 0 { 0 } else { position_ms.saturating_mul(rate) / 1000 };
        frame.min(self.frame_count)
    }
}

pub(super) enum Action {
    Play,
    Pause,
    Seek(u64),
    Volume(f32),
}

/// 控制落到哪里：PCM 在游标里就出声；装好了但没有声音（只有画面的视频）只走时钟。当前项还没装好时没有落点，
/// 调用方不驱动控制器（见 `PlayerState::output`）。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Output {
    Cursor,
    Clock,
}

/// 按 [`Output`] 驱动播放、暂停、跳转和音量，返回新的会话和可能的错误。
///
/// 播放时钟只有一份，是会话进度（`player_preview.rs` 的 `advance_clock`）；游标的播放头不随声卡前进。
/// 所以要重新出声时（继续播放、播放中换音量），先把播放头对到会话进度，声音才从界面上的进度接着走，
/// 而不是从上次装载或跳转的位置重来。
pub(super) fn drive(
    output: Output,
    wav: &WavPlayer,
    session: PlaybackSessionState,
    action: Action,
) -> (PlaybackSessionState, Option<String>) {
    match output {
        Output::Cursor => {
            if matches!(action, Action::Play | Action::Volume(_)) {
                wav.locate(session.current_time_ms);
            }
            control(wav.clone(), session, action)
        }
        Output::Clock => control(ClockOnly, session, action),
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
}

/// WAV 优先。失败后再解 mp3、flac、ogg。两种都不是时返回错误，由调用方记日志。
pub(crate) fn pcm_from_bytes(bytes: &[u8]) -> Result<PreviewPcm, String> {
    match parse_wav(bytes) {
        Ok(audio) => Ok(audio),
        Err(_) => pcm_from_decoded(crate::shell::audio_decode::decode_compressed(bytes)?),
    }
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

/// WAV、已解码的压缩音频和媒体基础解出的音轨都走这份内存游标。其它候选仍是缺失解码器。
pub(super) fn is_memory_candidate(candidate: &PlayerCandidate) -> bool {
    [PLUGIN_ID, COMPRESSED_PLUGIN_ID, SYSTEM_PLUGIN_ID].contains(&candidate.plugin_id.as_str())
}

/// 测试构建恒为 false：winmm 模块没有编进来。测试用它确认不会打开声卡。
#[cfg(test)]
pub(crate) fn sound_device_compiled_in() -> bool {
    cfg!(all(windows, not(test)))
}

fn pcm_from_decoded(decoded: crate::shell::audio_decode::DecodedPcm) -> Result<PreviewPcm, String> {
    let frame_bytes = usize::from(decoded.channels) * 2;
    if frame_bytes == 0 || decoded.pcm.len() % frame_bytes != 0 {
        return Err("压缩音频没有整帧样本".into());
    }
    Ok(PreviewPcm {
        sample_rate: decoded.sample_rate,
        channels: decoded.channels,
        bits_per_sample: 16,
        frame_count: decoded.pcm.len() as u64 / frame_bytes as u64,
        duration_ms: decoded.duration_ms,
        pcm: decoded.pcm,
    })
}

/// 按块读出 `fmt ` 和第一个 `data`，样本经 `audio_pcm.rs` 换成游标能放的 PCM。
/// 时长按帧计算：一帧是同一时刻全部声道的样本，`duration_ms` = 帧数 × 1000 / 采样率。
fn parse_wav(bytes: &[u8]) -> Result<PreviewPcm, String> {
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
            id if id == *b"fmt " => fmt = Some(WaveFormat::parse(&bytes[start..end])?),
            id if id == *b"data" && data.is_none() => data = Some(&bytes[start..end]),
            _ => {}
        }
        offset = end;
        if size % 2 == 1 {
            offset = offset.saturating_add(1);
        }
    }
    let fmt = fmt.ok_or("WAV 缺少 fmt 块")?;
    let data = data.ok_or("WAV 缺少 data 块")?;
    fmt.preview_pcm(data)
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

/// 测试构建和非 Windows 保持静音状态机，不打开设备。测试构建记下这次会从哪一帧、按什么音量出声；
/// 没有剩余样本时和真设备一样不出声。
#[cfg(not(all(windows, not(test))))]
mod device {
    pub(super) fn start(cursor: &mut super::Cursor) {
        #[cfg(test)]
        {
            cursor.sounding = (cursor.frame < cursor.frame_count).then_some((cursor.frame, cursor.volume));
        }
        #[cfg(not(test))]
        let _ = cursor;
    }

    pub(super) fn stop(cursor: &mut super::Cursor) {
        #[cfg(test)]
        {
            cursor.sounding = None;
        }
        #[cfg(not(test))]
        let _ = cursor;
    }
}
