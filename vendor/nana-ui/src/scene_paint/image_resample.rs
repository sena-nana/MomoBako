//! CPU preparation of decoded `url(...)` images for sampling. Runs on the
//! image worker, never on the frame path.
//!
//! Resampling is separable CatmullRom in premultiplied linear light: sRGB
//! bytes are linearized, colour is weighted by alpha so transparent texels do
//! not bleed their (meaningless) colour into the edge, and the kernel widens
//! with the reduction ratio so a large reduction still averages every source
//! texel under the destination one instead of skipping columns.

/// One prepared level: straight-alpha sRGB RGBA8, `width * height * 4` bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Level {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) rgba: Vec<u8>,
}

impl Level {
    pub(crate) fn new(width: u32, height: u32, rgba: Vec<u8>) -> Self {
        Self {
            width,
            height,
            rgba,
        }
    }
}

/// CatmullRom support radius in destination texels.
const SUPPORT: f32 = 2.0;

/// The level-0 size for drawing `source` at `demand` device pixels: each axis
/// the demanded size, never larger than the source and at least one texel.
/// Per axis, so a stretched image gets exactly the pixels it covers.
pub(crate) fn fit_to_demand(source: (u32, u32), demand: [u32; 2]) -> (u32, u32) {
    (
        source.0.min(demand[0]).max(1),
        source.1.min(demand[1]).max(1),
    )
}

/// `level` resampled to `width`×`height`. The same size comes back unchanged.
pub(crate) fn resample(level: &Level, width: u32, height: u32) -> Level {
    let (width, height) = (width.max(1), height.max(1));
    if (width, height) == (level.width, level.height) {
        return level.clone();
    }
    let lut = linear_lut();
    let x_taps = taps(level.width, width);
    let y_taps = taps(level.height, height);
    let src_row_len = level.width as usize * 4;
    let dst_row_len = width as usize * 4;
    // Horizontally filtered source rows, premultiplied linear, kept only while
    // a destination row still reaches them: the window start never moves back.
    let mut window: std::collections::VecDeque<(u32, Vec<f32>)> = Default::default();
    let mut scratch = vec![0.0_f32; src_row_len];
    let mut out = vec![0_u8; dst_row_len * height as usize];
    let mut acc = vec![0.0_f32; dst_row_len];
    for (dst_y, tap) in y_taps.iter().enumerate() {
        while window.front().is_some_and(|(row, _)| *row < tap.start) {
            window.pop_front();
        }
        let end = tap.start + tap.weights.len() as u32;
        let mut next = window.back().map_or(tap.start, |(row, _)| row + 1);
        while next < end {
            let src = &level.rgba[next as usize * src_row_len..][..src_row_len];
            premultiplied_linear(src, &lut, &mut scratch);
            window.push_back((next, filter_row(&scratch, &x_taps)));
            next += 1;
        }
        acc.fill(0.0);
        for (row, weight) in window
            .iter()
            .skip_while(|(row, _)| *row < tap.start)
            .zip(&tap.weights)
        {
            for (sum, value) in acc.iter_mut().zip(&row.1) {
                *sum += value * weight;
            }
        }
        encode_row(&acc, &mut out[dst_y * dst_row_len..][..dst_row_len]);
    }
    Level::new(width, height, out)
}

/// `base` followed by every half-size level down to 1×1. Each level is
/// resampled from the one above it, so the whole chain costs about a third
/// more than its first reduction.
pub(crate) fn mip_chain(base: Level) -> Vec<Level> {
    let mut levels = vec![base];
    loop {
        let last = levels.last().expect("chain starts with the base");
        if last.width == 1 && last.height == 1 {
            break;
        }
        let next = resample(last, (last.width / 2).max(1), (last.height / 2).max(1));
        levels.push(next);
    }
    levels
}

struct Tap {
    start: u32,
    weights: Vec<f32>,
}

fn taps(src_len: u32, dst_len: u32) -> Vec<Tap> {
    let src = src_len.max(1) as f32;
    let ratio = src / dst_len.max(1) as f32;
    let stretch = ratio.max(1.0);
    let support = SUPPORT * stretch;
    (0..dst_len)
        .map(|index| {
            let center = (index as f32 + 0.5) * ratio;
            let left = (center - support).floor().clamp(0.0, src - 1.0) as u32;
            let right = (center + support).ceil().clamp(left as f32 + 1.0, src) as u32;
            let mut weights: Vec<f32> = (left..right)
                .map(|src_index| catmull_rom((src_index as f32 + 0.5 - center) / stretch))
                .collect();
            let sum: f32 = weights.iter().sum();
            if sum.abs() > 1.0e-6 {
                for weight in &mut weights {
                    *weight /= sum;
                }
            }
            Tap {
                start: left,
                weights,
            }
        })
        .collect()
}

fn filter_row(src: &[f32], taps: &[Tap]) -> Vec<f32> {
    let mut out = vec![0.0_f32; taps.len() * 4];
    for (texel, tap) in out.as_chunks_mut::<4>().0.iter_mut().zip(taps) {
        let from = tap.start as usize * 4;
        for (offset, weight) in tap.weights.iter().enumerate() {
            let src = &src[from + offset * 4..][..4];
            for channel in 0..4 {
                texel[channel] += src[channel] * weight;
            }
        }
    }
    out
}

/// CatmullRom (B = 0, C = 0.5): keeps edges sharper than a box and rings less
/// than Lanczos.
fn catmull_rom(x: f32) -> f32 {
    let x = x.abs();
    if x < 1.0 {
        (1.5 * x - 2.5) * x * x + 1.0
    } else if x < 2.0 {
        ((-0.5 * x + 2.5) * x - 4.0) * x + 2.0
    } else {
        0.0
    }
}

fn linear_lut() -> [f32; 256] {
    std::array::from_fn(|value| {
        let encoded = value as f32 / 255.0;
        if encoded <= 0.04045 {
            encoded / 12.92
        } else {
            ((encoded + 0.055) / 1.055).powf(2.4)
        }
    })
}

fn premultiplied_linear(src: &[u8], lut: &[f32; 256], out: &mut [f32]) {
    for (texel, dst) in src
        .as_chunks::<4>()
        .0
        .iter()
        .zip(out.as_chunks_mut::<4>().0)
    {
        let alpha = f32::from(texel[3]) / 255.0;
        dst[0] = lut[texel[0] as usize] * alpha;
        dst[1] = lut[texel[1] as usize] * alpha;
        dst[2] = lut[texel[2] as usize] * alpha;
        dst[3] = alpha;
    }
}

fn encode_row(src: &[f32], out: &mut [u8]) {
    for (texel, dst) in src
        .as_chunks::<4>()
        .0
        .iter()
        .zip(out.as_chunks_mut::<4>().0)
    {
        let alpha = texel[3].clamp(0.0, 1.0);
        if alpha <= 1.0 / 510.0 {
            dst.fill(0);
            continue;
        }
        for channel in 0..3 {
            dst[channel] = encode_srgb(texel[channel] / alpha);
        }
        dst[3] = (alpha * 255.0).round() as u8;
    }
}

fn encode_srgb(linear: f32) -> u8 {
    let linear = linear.clamp(0.0, 1.0);
    let encoded = if linear <= 0.003_130_8 {
        linear * 12.92
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    };
    (encoded * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stripes(width: u32, height: u32) -> Level {
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for _ in 0..height {
            for x in 0..width {
                let value = if x % 2 == 0 { 0 } else { 255 };
                rgba.extend_from_slice(&[value, value, value, 255]);
            }
        }
        Level::new(width, height, rgba)
    }

    #[test]
    fn fit_follows_the_demand_per_axis_and_never_enlarges() {
        assert_eq!(fit_to_demand((1920, 1080), [320, 180]), (320, 180));
        assert_eq!(fit_to_demand((1000, 1000), [200, 50]), (200, 50));
        assert_eq!(fit_to_demand((64, 32), [400, 400]), (64, 32));
        assert_eq!(fit_to_demand((64, 32), [0, 10]), (1, 10));
    }

    #[test]
    fn a_large_reduction_averages_one_pixel_stripes_in_linear_light() {
        let shown = resample(&stripes(256, 4), 37, 1);
        assert_eq!((shown.width, shown.height), (37, 1));
        for texel in shown.rgba.as_chunks::<4>().0.iter() {
            // Linear-light mid grey encodes to ~188; a gamma-space average
            // would sit near 128, a skipped column at 0 or 255.
            assert!(
                (180..=196).contains(&texel[0]),
                "stripe survived as {texel:?}"
            );
            assert_eq!(texel[3], 255);
        }
    }

    #[test]
    fn transparent_texels_do_not_bleed_their_colour() {
        // Opaque red beside fully transparent green.
        let mut rgba = Vec::new();
        for x in 0..8 {
            rgba.extend_from_slice(if x < 4 {
                &[255, 0, 0, 255]
            } else {
                &[0, 255, 0, 0]
            });
        }
        let shown = resample(&Level::new(8, 1, rgba), 3, 1);
        for texel in shown
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|texel| texel[3] > 0)
        {
            assert_eq!(texel[1], 0, "transparent green leaked into {texel:?}");
        }
    }

    #[test]
    fn an_edge_stays_within_two_destination_texels() {
        let mut rgba = Vec::new();
        for x in 0..64 {
            let value = if x < 32 { 0 } else { 255 };
            rgba.extend_from_slice(&[value, value, value, 255]);
        }
        let shown = resample(&Level::new(64, 1, rgba), 16, 1);
        let ramp = shown
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|texel| (24..230).contains(&texel[0]))
            .count();
        assert!(ramp <= 2, "edge spread over {ramp} texels");
    }

    #[test]
    fn the_mip_chain_halves_down_to_one_texel() {
        let chain = mip_chain(stripes(8, 2));
        let sizes: Vec<_> = chain.iter().map(|l| (l.width, l.height)).collect();
        assert_eq!(sizes, [(8, 2), (4, 1), (2, 1), (1, 1)]);
        for level in &chain[1..] {
            assert_eq!(level.rgba.len(), (level.width * level.height * 4) as usize);
            for texel in level.rgba.as_chunks::<4>().0.iter() {
                assert!((170..=205).contains(&texel[0]), "{texel:?}");
            }
        }
    }
}
