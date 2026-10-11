//! 未压缩样本的统一：各种位深、浮点和多声道的交错样本，换成播放游标能放的交错 PCM。
//!
//! 播放游标只收 8-bit 或 16-bit、单声道或双声道的小端 PCM（`wav_player.rs`）。WAV 的 `fmt ` 块（含
//! WAVE_FORMAT_EXTENSIBLE）、AVI 音轨、媒体基础和 symphonia 解出的样本都经这里：8/16-bit 单双声道原样留下，
//! 其余样本取成 16-bit，三个以上声道按扬声器位置下混成立体声。只动内存，不开声卡。

use crate::shell::player::PreviewPcm;

/// `fmt ` 块的编码标签。
const FORMAT_PCM: u16 = 0x0001;
const FORMAT_IEEE_FLOAT: u16 = 0x0003;
const FORMAT_EXTENSIBLE: u16 = 0xFFFE;
/// KSDATAFORMAT_SUBTYPE_PCM / _IEEE_FLOAT 这类子格式 GUID 的后 12 字节；前 4 字节里低 2 字节是编码标签。
const SUBTYPE_TAIL: [u8; 12] = [0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71];

/// 扬声器位置，和 WAVE 声道掩码的位一致（symphonia 的 `Position` 前 18 位也是这一套）。
const FRONT_LEFT: u64 = 1 << 0;
const FRONT_RIGHT: u64 = 1 << 1;
const FRONT_CENTER: u64 = 1 << 2;
const LOW_FREQUENCY: u64 = 1 << 3;
const BACK_LEFT: u64 = 1 << 4;
const BACK_RIGHT: u64 = 1 << 5;
const FRONT_LEFT_OF_CENTER: u64 = 1 << 6;
const FRONT_RIGHT_OF_CENTER: u64 = 1 << 7;
const BACK_CENTER: u64 = 1 << 8;
const SIDE_LEFT: u64 = 1 << 9;
const SIDE_RIGHT: u64 = 1 << 10;
const TOP_FRONT_LEFT: u64 = 1 << 12;
const TOP_FRONT_RIGHT: u64 = 1 << 14;
const TOP_BACK_LEFT: u64 = 1 << 15;
const TOP_BACK_RIGHT: u64 = 1 << 17;
/// symphonia 在 WAVE 的 18 位之后自己定义的位置。
const LOW_FREQUENCY_2: u64 = 1 << 18;
const TOP_SIDE_LEFT: u64 = 1 << 19;
const TOP_SIDE_RIGHT: u64 = 1 << 20;
const BOTTOM_FRONT_LEFT: u64 = 1 << 22;
const BOTTOM_FRONT_RIGHT: u64 = 1 << 23;
const FRONT_LEFT_WIDE: u64 = 1 << 24;
const FRONT_RIGHT_WIDE: u64 = 1 << 25;
/// 没有位置的声道。
const UNPLACED: u64 = 0;

/// -3 dB。
const HALF_POWER: f32 = std::f32::consts::FRAC_1_SQRT_2;

/// 样本编码。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Encoding {
    /// 整数 PCM。8-bit 是无符号，其余是有符号小端。
    Integer,
    /// IEEE 浮点，32 或 64 位小端。
    Float,
}

/// 播放要用的格式字段。`bits_per_sample` 是 `fmt ` 块写的位数；每个样本占的字节从块对齐算，有效位数比
/// 容器少时样本靠高位放，所以按容器取高 16 位就对。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct WaveFormat {
    pub(crate) encoding: Encoding,
    pub(crate) channels: u16,
    pub(crate) sample_rate: u32,
    pub(crate) block_align: u16,
    pub(crate) bits_per_sample: u16,
    /// WAVE_FORMAT_EXTENSIBLE 的声道掩码；没有或是 0 时按声道数取常见布局。
    pub(crate) channel_mask: Option<u64>,
}

impl WaveFormat {
    /// 解析 `fmt ` 块（WAVEFORMATEX 或 WAVEFORMATEXTENSIBLE）。整数 PCM 和浮点以外的编码（ADPCM、A-law 等）
    /// 返回带编码标签的原因。
    pub(crate) fn parse(chunk: &[u8]) -> Result<Self, String> {
        if chunk.len() < 16 {
            return Err("WAV fmt 块不完整".into());
        }
        let tag = u16_at(chunk, 0);
        let (encoding, channel_mask) = match tag {
            FORMAT_PCM => (Encoding::Integer, None),
            FORMAT_IEEE_FLOAT => (Encoding::Float, None),
            FORMAT_EXTENSIBLE => {
                if chunk.len() < 40 {
                    return Err("WAV 扩展格式块不完整".into());
                }
                let sub_format = &chunk[24..40];
                if sub_format[2..4] != [0, 0] || sub_format[4..] != SUBTYPE_TAIL {
                    return Err("WAV 扩展格式的子格式认不出".into());
                }
                let encoding = match u16_at(sub_format, 0) {
                    FORMAT_PCM => Encoding::Integer,
                    FORMAT_IEEE_FLOAT => Encoding::Float,
                    other => return Err(format!("不支持的 WAV 编码（扩展格式 0x{other:04X}）")),
                };
                let mask = u32_at(chunk, 20);
                (encoding, (mask != 0).then_some(u64::from(mask)))
            }
            other => return Err(format!("不支持的 WAV 编码（0x{other:04X}）")),
        };
        let format = Self {
            encoding,
            channels: u16_at(chunk, 2),
            sample_rate: u32_at(chunk, 4),
            block_align: u16_at(chunk, 12),
            bits_per_sample: u16_at(chunk, 14),
            channel_mask,
        };
        format.check()?;
        Ok(format)
    }

    /// 解码器直接给出声道、采样率和位深的交错样本（媒体基础的音轨）。块对齐 = 声道数 × 每个样本的字节。
    pub(crate) fn interleaved(
        encoding: Encoding,
        channels: u16,
        sample_rate: u32,
        bits_per_sample: u16,
        channel_mask: Option<u64>,
    ) -> Result<Self, String> {
        let block_align = channels.checked_mul(bits_per_sample.div_ceil(8)).ok_or("WAV 帧长无效")?;
        let format = Self {
            encoding,
            channels,
            sample_rate,
            block_align,
            bits_per_sample,
            channel_mask: channel_mask.filter(|mask| *mask != 0),
        };
        format.check()?;
        Ok(format)
    }

    /// 每个样本占的字节：整帧字节 / 声道数。
    fn sample_bytes(&self) -> usize {
        usize::from(self.block_align) / usize::from(self.channels.max(1))
    }

    fn check(&self) -> Result<(), String> {
        if self.channels == 0 {
            return Err("WAV 声道数无效".into());
        }
        if self.sample_rate == 0 {
            return Err("WAV 采样率无效".into());
        }
        let bytes = self.sample_bytes();
        let whole = self.block_align != 0 && usize::from(self.block_align) % usize::from(self.channels) == 0;
        if !whole || usize::from(self.bits_per_sample).div_ceil(8) != bytes {
            return Err("WAV 帧长无效".into());
        }
        let supported = match self.encoding {
            Encoding::Integer => (1..=4).contains(&bytes),
            Encoding::Float => bytes == 4 || bytes == 8,
        };
        if !supported {
            return Err(format!("不支持 {} 位的 WAV 样本", self.bits_per_sample));
        }
        Ok(())
    }

    /// 游标能直接放：8/16-bit 整数、单声道或双声道。
    fn passthrough(&self) -> bool {
        self.encoding == Encoding::Integer && self.sample_bytes() <= 2 && self.channels <= 2
    }

    /// 换出来的声道数和位深。
    pub(crate) fn output(&self) -> (u16, u16) {
        if self.passthrough() {
            (self.channels, (self.sample_bytes() * 8) as u16)
        } else {
            (self.channels.min(2), 16)
        }
    }

    /// 把整帧的样本换好以后追加到 `out`。不是整帧时报错，不追加。
    pub(crate) fn convert_into(&self, data: &[u8], out: &mut Vec<u8>) -> Result<(), String> {
        let frame_bytes = usize::from(self.block_align);
        if frame_bytes == 0 || data.len() % frame_bytes != 0 {
            return Err("WAV 数据长度不是整帧".into());
        }
        if self.passthrough() {
            out.extend_from_slice(data);
            return Ok(());
        }
        let (channels, _) = self.output();
        let mixer = Mixer::new(&positions(self.channels, self.channel_mask), channels);
        let width = self.sample_bytes();
        let mut frame = Vec::with_capacity(usize::from(self.channels));
        out.reserve(data.len() / frame_bytes * usize::from(channels) * 2);
        for bytes in data.chunks_exact(frame_bytes) {
            frame.clear();
            frame.extend(bytes.chunks_exact(width).map(|sample| self.sample_at(sample)));
            mixer.push(&frame, out);
        }
        Ok(())
    }

    /// 整段样本换成预览和播放共用的 PCM。时长按帧算：帧数 × 1000 / 采样率。
    pub(crate) fn preview_pcm(&self, data: &[u8]) -> Result<PreviewPcm, String> {
        let mut pcm = Vec::new();
        self.convert_into(data, &mut pcm)?;
        let (channels, bits_per_sample) = self.output();
        let frame_count = (data.len() / usize::from(self.block_align)) as u64;
        Ok(PreviewPcm {
            sample_rate: self.sample_rate,
            channels,
            bits_per_sample,
            frame_count,
            duration_ms: frame_count.saturating_mul(1000) / u64::from(self.sample_rate),
            pcm,
        })
    }

    /// 一个样本取成 16-bit：整数取高 16 位（8-bit 先去掉 128 的偏移），浮点按 [-1, 1] 映射，超出的夹住。
    fn sample_at(&self, bytes: &[u8]) -> i16 {
        match (self.encoding, bytes) {
            (Encoding::Integer, [byte]) => (i16::from(*byte) - 128) << 8,
            (Encoding::Integer, [low, high]) => i16::from_le_bytes([*low, *high]),
            (Encoding::Integer, [_, low, high]) => i16::from_le_bytes([*low, *high]),
            (Encoding::Integer, [_, _, low, high]) => i16::from_le_bytes([*low, *high]),
            (Encoding::Float, [a, b, c, d]) => float_sample(f64::from(f32::from_le_bytes([*a, *b, *c, *d]))),
            (Encoding::Float, [a, b, c, d, e, f, g, h]) => float_sample(f64::from_le_bytes([*a, *b, *c, *d, *e, *f, *g, *h])),
            _ => 0,
        }
    }
}

/// 浮点样本换成 16-bit。NaN 换成 0。
fn float_sample(value: f64) -> i16 {
    (value.clamp(-1.0, 1.0) * f64::from(i16::MAX)).round() as i16
}

/// 每个声道的扬声器位置，按声道顺序。掩码里置位的位从低到高依次对应各声道（WAVE 的约定）；掩码的位比声道少时，
/// 多出来的声道没有位置。没有掩码时按声道数取常见布局（Windows 的 KSAUDIO_SPEAKER_*），认不出的声道数都没有位置。
pub(crate) fn positions(channels: u16, mask: Option<u64>) -> Vec<u64> {
    let count = usize::from(channels);
    let Some(mask) = mask.filter(|mask| *mask != 0) else {
        return default_layout(count);
    };
    let mut placed: Vec<u64> = (0..64).map(|bit| 1u64 << bit).filter(|bit| mask & bit != 0).take(count).collect();
    placed.resize(count, UNPLACED);
    placed
}

fn default_layout(count: usize) -> Vec<u64> {
    match count {
        1 => vec![FRONT_CENTER],
        2 => vec![FRONT_LEFT, FRONT_RIGHT],
        3 => vec![FRONT_LEFT, FRONT_RIGHT, FRONT_CENTER],
        4 => vec![FRONT_LEFT, FRONT_RIGHT, BACK_LEFT, BACK_RIGHT],
        5 => vec![FRONT_LEFT, FRONT_RIGHT, FRONT_CENTER, BACK_LEFT, BACK_RIGHT],
        6 => vec![FRONT_LEFT, FRONT_RIGHT, FRONT_CENTER, LOW_FREQUENCY, BACK_LEFT, BACK_RIGHT],
        7 => vec![FRONT_LEFT, FRONT_RIGHT, FRONT_CENTER, LOW_FREQUENCY, BACK_CENTER, SIDE_LEFT, SIDE_RIGHT],
        8 => vec![FRONT_LEFT, FRONT_RIGHT, FRONT_CENTER, LOW_FREQUENCY, BACK_LEFT, BACK_RIGHT, SIDE_LEFT, SIDE_RIGHT],
        _ => vec![UNPLACED; count],
    }
}

/// 一个扬声器位置在左、右两侧的分量：前方左右整份进各自一侧；后、侧、顶部和底部的左右按 -3 dB 进各自一侧；
/// 低音声道不进；中置和没有位置的声道按 -3 dB 进两侧。
fn gains(position: u64) -> (f32, f32) {
    match position {
        FRONT_LEFT | FRONT_LEFT_OF_CENTER | FRONT_LEFT_WIDE => (1.0, 0.0),
        FRONT_RIGHT | FRONT_RIGHT_OF_CENTER | FRONT_RIGHT_WIDE => (0.0, 1.0),
        LOW_FREQUENCY | LOW_FREQUENCY_2 => (0.0, 0.0),
        BACK_LEFT | SIDE_LEFT | TOP_FRONT_LEFT | TOP_BACK_LEFT | TOP_SIDE_LEFT | BOTTOM_FRONT_LEFT => (HALF_POWER, 0.0),
        BACK_RIGHT | SIDE_RIGHT | TOP_FRONT_RIGHT | TOP_BACK_RIGHT | TOP_SIDE_RIGHT | BOTTOM_FRONT_RIGHT => (0.0, HALF_POWER),
        _ => (HALF_POWER, HALF_POWER),
    }
}

/// 一帧 N 个声道换成 1 或 2 个声道的系数，建一次、每帧复用。
pub(crate) struct Mixer {
    source: usize,
    target: u16,
    left: Vec<f32>,
    right: Vec<f32>,
}

impl Mixer {
    /// 算法：声道数已经是目标时原样留下；单声道升双声道把样本抄到两侧；双声道降单声道取两侧平均。三个以上声道
    /// 先按 [`gains`] 下混成立体声：每一侧 = Σ 分量 × 样本，再整体乘 1 / max(左侧分量和, 右侧分量和, 1)，满幅的
    /// 输入不会削波（和 ffmpeg 默认的下混一样归一）；目标是单声道时再取两侧平均。
    pub(crate) fn new(positions: &[u64], target: u16) -> Self {
        let source = positions.len();
        let (mut left, mut right): (Vec<f32>, Vec<f32>) = positions.iter().map(|position| gains(*position)).unzip();
        let peak = left.iter().sum::<f32>().max(right.iter().sum::<f32>()).max(1.0);
        for gain in left.iter_mut().chain(right.iter_mut()) {
            *gain /= peak;
        }
        Self { source, target: target.clamp(1, 2), left, right }
    }

    /// 一帧样本换算后按小端 16-bit 追加到 `out`。样本数和建它时的声道数对不上时不追加。
    pub(crate) fn push(&self, frame: &[i16], out: &mut Vec<u8>) {
        if frame.len() != self.source || self.source == 0 {
            return;
        }
        let stereo = match self.source {
            1 => (frame[0], frame[0]),
            2 => (frame[0], frame[1]),
            _ => (mix(frame, &self.left), mix(frame, &self.right)),
        };
        if self.target == 1 {
            let mono = if self.source == 1 { frame[0] } else { ((i32::from(stereo.0) + i32::from(stereo.1)) / 2) as i16 };
            out.extend_from_slice(&mono.to_le_bytes());
            return;
        }
        out.extend_from_slice(&stereo.0.to_le_bytes());
        out.extend_from_slice(&stereo.1.to_le_bytes());
    }
}

fn mix(frame: &[i16], gains: &[f32]) -> i16 {
    let sum: f32 = frame.iter().zip(gains).map(|(sample, gain)| f32::from(*sample) * gain).sum();
    sum.round().clamp(f32::from(i16::MIN), f32::from(i16::MAX)) as i16
}

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

/// 拼一个 `fmt ` 块。`extensible` 有值时写 WAVE_FORMAT_EXTENSIBLE：（子格式编码标签，声道掩码）。
#[cfg(test)]
pub(crate) fn fmt_chunk(tag: u16, channels: u16, sample_rate: u32, bits: u16, extensible: Option<(u16, u32)>) -> Vec<u8> {
    let block_align = channels * bits.div_ceil(8);
    let mut chunk = Vec::new();
    chunk.extend_from_slice(&(if extensible.is_some() { FORMAT_EXTENSIBLE } else { tag }).to_le_bytes());
    chunk.extend_from_slice(&channels.to_le_bytes());
    chunk.extend_from_slice(&sample_rate.to_le_bytes());
    chunk.extend_from_slice(&(sample_rate * u32::from(block_align)).to_le_bytes());
    chunk.extend_from_slice(&block_align.to_le_bytes());
    chunk.extend_from_slice(&bits.to_le_bytes());
    if let Some((sub_tag, mask)) = extensible {
        chunk.extend_from_slice(&22u16.to_le_bytes());
        chunk.extend_from_slice(&bits.to_le_bytes());
        chunk.extend_from_slice(&mask.to_le_bytes());
        chunk.extend_from_slice(&u32::from(sub_tag).to_le_bytes());
        chunk.extend_from_slice(&SUBTYPE_TAIL);
    }
    chunk
}

/// 用 `fmt ` 块和样本拼一个 RIFF/WAVE 文件。
#[cfg(test)]
pub(crate) fn wav_file(fmt: &[u8], data: &[u8]) -> Vec<u8> {
    let mut body = b"WAVE".to_vec();
    for (id, chunk) in [(b"fmt ", fmt), (b"data", data)] {
        body.extend_from_slice(id);
        body.extend_from_slice(&(chunk.len() as u32).to_le_bytes());
        body.extend_from_slice(chunk);
        if chunk.len() % 2 == 1 {
            body.push(0);
        }
    }
    let mut file = b"RIFF".to_vec();
    file.extend_from_slice(&(body.len() as u32).to_le_bytes());
    file.extend_from_slice(&body);
    file
}

#[cfg(test)]
mod tests {
    use super::*;

    fn le16(samples: &[i16]) -> Vec<u8> {
        samples.iter().flat_map(|sample| sample.to_le_bytes()).collect()
    }

    fn i16s(bytes: &[u8]) -> Vec<i16> {
        bytes.chunks_exact(2).map(|pair| i16::from_le_bytes([pair[0], pair[1]])).collect()
    }

    /// PCM、浮点和扩展格式都认得；ADPCM 这类压缩编码、认不出的子格式和对不上的帧长报出原因。
    #[test]
    fn parses_pcm_float_and_extensible_and_names_what_it_cannot_play() {
        let pcm24 = WaveFormat::parse(&fmt_chunk(FORMAT_PCM, 2, 48_000, 24, None)).expect("24 位 PCM");
        assert_eq!((pcm24.encoding, pcm24.block_align, pcm24.channel_mask), (Encoding::Integer, 6, None));
        let float = WaveFormat::parse(&fmt_chunk(FORMAT_IEEE_FLOAT, 1, 44_100, 32, None)).expect("32 位浮点");
        assert_eq!(float.encoding, Encoding::Float);
        let extensible = WaveFormat::parse(&fmt_chunk(0, 6, 48_000, 32, Some((FORMAT_IEEE_FLOAT, 0x3F)))).expect("扩展格式浮点");
        assert_eq!((extensible.encoding, extensible.channels, extensible.channel_mask), (Encoding::Float, 6, Some(0x3F)));
        let no_mask = WaveFormat::parse(&fmt_chunk(0, 2, 48_000, 16, Some((FORMAT_PCM, 0)))).expect("扩展格式 PCM");
        assert_eq!(no_mask.channel_mask, None, "掩码是 0 时按声道数取布局");

        let adpcm = WaveFormat::parse(&fmt_chunk(0x0002, 2, 44_100, 4, None)).expect_err("ADPCM");
        assert!(adpcm.contains("0x0002"), "{adpcm}");
        let mut unknown = fmt_chunk(0, 2, 44_100, 16, Some((FORMAT_PCM, 3)));
        unknown[30] = 0x11;
        assert!(WaveFormat::parse(&unknown).expect_err("子格式").contains("子格式"));
        let mut misaligned = fmt_chunk(FORMAT_PCM, 2, 44_100, 16, None);
        misaligned[12] = 2;
        assert_eq!(WaveFormat::parse(&misaligned).expect_err("帧长"), "WAV 帧长无效");
        assert!(WaveFormat::parse(&fmt_chunk(FORMAT_IEEE_FLOAT, 1, 44_100, 16, None)).is_err(), "16 位浮点不是合法格式");
    }

    /// 8/16-bit 单双声道原样留下；24、32 位整数取高 16 位，浮点按满幅映射并夹住。
    #[test]
    fn samples_become_sixteen_bit_and_small_formats_pass_through() {
        let byte = WaveFormat::parse(&fmt_chunk(FORMAT_PCM, 2, 8_000, 8, None)).unwrap();
        let pcm = byte.preview_pcm(&[0, 255, 128, 1]).unwrap();
        assert_eq!((pcm.channels, pcm.bits_per_sample, pcm.frame_count, pcm.pcm.as_slice()), (2, 8, 2, [0u8, 255, 128, 1].as_slice()));

        let deep = WaveFormat::parse(&fmt_chunk(FORMAT_PCM, 1, 8_000, 24, None)).unwrap();
        let pcm = deep.preview_pcm(&[0xAA, 0x34, 0x12, 0x00, 0x00, 0x80]).unwrap();
        assert_eq!((pcm.channels, pcm.bits_per_sample, pcm.frame_count), (1, 16, 2));
        assert_eq!(i16s(&pcm.pcm), [0x1234, i16::MIN]);

        let wide = WaveFormat::parse(&fmt_chunk(FORMAT_PCM, 1, 8_000, 32, None)).unwrap();
        let samples: Vec<u8> = [0x7FFF_FFFFi32, -0x4000_0000].iter().flat_map(|sample| sample.to_le_bytes()).collect();
        assert_eq!(i16s(&wide.preview_pcm(&samples).unwrap().pcm), [i16::MAX, -0x4000]);

        let float = WaveFormat::parse(&fmt_chunk(FORMAT_IEEE_FLOAT, 1, 8_000, 32, None)).unwrap();
        let samples: Vec<u8> = [0.5f32, -1.5, f32::NAN].iter().flat_map(|sample| sample.to_le_bytes()).collect();
        assert_eq!(i16s(&float.preview_pcm(&samples).unwrap().pcm), [16_384, -i16::MAX, 0]);
        let double = WaveFormat::parse(&fmt_chunk(FORMAT_IEEE_FLOAT, 1, 8_000, 64, None)).unwrap();
        assert_eq!(i16s(&double.preview_pcm(&(-0.25f64).to_le_bytes()).unwrap().pcm), [-8_192]);
        assert_eq!(double.preview_pcm(&[0; 12]).expect_err("半帧"), "WAV 数据长度不是整帧");
    }

    /// 5.1 下混成立体声：前左右整份、中置和后方 -3 dB，低音不进；分量和最大的一侧归一到 1，满幅不削波。
    #[test]
    fn surround_downmixes_to_stereo_without_clipping() {
        let format = WaveFormat::parse(&fmt_chunk(0, 6, 48_000, 16, Some((FORMAT_PCM, 0x3F)))).unwrap();
        let scale = 1.0 / (1.0 + 2.0 * HALF_POWER);
        let frames = le16(&[1_000, 0, 0, 30_000, 0, 0, 0, 0, 1_000, 0, 0, 0, i16::MAX, i16::MAX, i16::MAX, 0, i16::MAX, i16::MAX]);
        let pcm = format.preview_pcm(&frames).unwrap();
        assert_eq!((pcm.channels, pcm.bits_per_sample, pcm.frame_count), (2, 16, 3));
        let center = (1_000.0 * HALF_POWER * scale).round() as i16;
        assert_eq!(i16s(&pcm.pcm), [(1_000.0 * scale).round() as i16, 0, center, center, i16::MAX, i16::MAX]);
    }

    /// 掩码里的位按顺序对应声道，位不够时多出的声道没有位置；没有掩码时按声道数取常见布局。
    #[test]
    fn positions_follow_the_mask_or_the_usual_layout() {
        assert_eq!(positions(3, Some(0b1_0000_0011)), [FRONT_LEFT, FRONT_RIGHT, BACK_CENTER]);
        assert_eq!(positions(3, Some(0b11)), [FRONT_LEFT, FRONT_RIGHT, UNPLACED]);
        assert_eq!(positions(2, Some(0b111)), [FRONT_LEFT, FRONT_RIGHT]);
        assert_eq!(positions(6, None), [FRONT_LEFT, FRONT_RIGHT, FRONT_CENTER, LOW_FREQUENCY, BACK_LEFT, BACK_RIGHT]);
        assert_eq!(positions(4, Some(0)), [FRONT_LEFT, FRONT_RIGHT, BACK_LEFT, BACK_RIGHT]);
        assert_eq!(positions(9, None), vec![UNPLACED; 9]);
    }

    /// 单声道升双声道抄到两侧，双声道降单声道取平均，三个以上声道降单声道是下混后的平均。
    #[test]
    fn mixer_moves_between_mono_and_stereo() {
        let mut out = Vec::new();
        Mixer::new(&positions(1, None), 2).push(&[1_200], &mut out);
        Mixer::new(&positions(2, None), 1).push(&[1_000, -3_000], &mut out);
        Mixer::new(&positions(3, None), 1).push(&[1_000, 1_000, 0], &mut out);
        let side = (1_000.0 / (1.0 + HALF_POWER)).round() as i16;
        assert_eq!(i16s(&out), [1_200, 1_200, -1_000, side]);
        let mut ignored = Vec::new();
        Mixer::new(&positions(2, None), 2).push(&[1, 2, 3], &mut ignored);
        assert!(ignored.is_empty(), "样本数和声道数对不上的帧不追加");
    }
}
