//! 缩略图色板。
//!
//! 元数据里的 `thumbnailPalette` 或 `palette` 优先，最多五枚 `#RRGGBB`。都没有时，缩略图解码出像素以后
//! 按 Vue `extractPaletteFromImageElement` 从画面里取最多五种（`sample_rgba`），画在详情预览和预览页的
//! 色板槽上。元数据「调色板」一行仍只认元数据，不把取样结果写进去。
//! 画法：详情预览下面是 34×12 的药丸（`ThumbnailPalette.vue`），元数据一行是右对齐的 14×14 小方块。

use std::collections::BTreeMap;

use nana_ui::runtime::view::{AnyView, IntoView, widget};
use nana_ui::runtime::{AlignSpec, JustifySpec, LengthSpec, RadiusTier, SemanticColorRole, Stack};
use serde_json::Value;

use crate::backend::services::repository::MetadataEntry;

/// 从文件条目的元数据映射读出色板。
pub(super) fn from_metadata_map(metadata: &BTreeMap<String, Value>) -> Vec<String> {
    let value = metadata
        .get("thumbnailPalette")
        .or_else(|| metadata.get("palette"));
    hex_colors(value)
}

/// 从素材详情的元数据条目读出色板。
pub(super) fn from_metadata_entries(entries: &[MetadataEntry]) -> Vec<String> {
    let value = entries
        .iter()
        .find(|entry| entry.key == "thumbnailPalette")
        .or_else(|| entries.iter().find(|entry| entry.key == "palette"))
        .map(|entry| &entry.value);
    hex_colors(value)
}

/// 详情预览和预览页要画的色板。元数据里已经有合法颜色时用那一份，否则用缩略图像素取出的颜色。
pub(super) fn swatch_colors(
    metadata: &[String],
    thumbnail: Option<&str>,
    files: &super::files::FilesState,
) -> Vec<String> {
    if metadata.iter().any(|color| is_hex(color)) {
        return metadata
            .iter()
            .filter(|color| is_hex(color))
            .take(5)
            .cloned()
            .collect();
    }
    files.sampled_palette(thumbnail)
}

/// 从缩略图像素里取最多五种颜色，对应 Vue `extractPaletteFromImageElement`。
///
/// 算法：缩到宽 40、高按原比例且不超过 40 的采样格（最近邻，实色和 Vue 的 `drawImage` 一致），
/// 跳过 alpha 小于 24 的像素，按 16 级色桶累计，桶内平均后按出现次数从多到少取，
/// 和已经选中的颜色距离小于 30 的丢掉。尺寸和字节对不上时记日志并返回空，不编造颜色。
pub(super) fn sample_rgba(width: u32, height: u32, rgba: &[u8]) -> Vec<String> {
    let Some(bytes) = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
    else {
        eprintln!("Nana 缩略图色板采样尺寸溢出：{width}x{height}");
        return Vec::new();
    };
    if width == 0 || height == 0 || rgba.len() < bytes {
        eprintln!(
            "Nana 缩略图色板采样尺寸对不上：{width}x{height}，字节 {}",
            rgba.len()
        );
        return Vec::new();
    }
    let sample_w = 40u32;
    let sample_h =
        ((f64::from(height) / f64::from(width) * f64::from(sample_w)).round() as u32).clamp(1, 40);
    let mut buckets: Vec<Bucket> = Vec::new();
    for y in 0..sample_h {
        for x in 0..sample_w {
            let [r, g, b, a] = nearest(rgba, width, height, sample_w, sample_h, x, y);
            if a < 24 {
                continue;
            }
            let key = ((u16::from(r / 16)) << 8) | ((u16::from(g / 16)) << 4) | u16::from(b / 16);
            if let Some(bucket) = buckets.iter_mut().find(|bucket| bucket.key == key) {
                bucket.count += 1;
                bucket.r += u64::from(r);
                bucket.g += u64::from(g);
                bucket.b += u64::from(b);
            } else {
                buckets.push(Bucket {
                    key,
                    count: 1,
                    r: u64::from(r),
                    g: u64::from(g),
                    b: u64::from(b),
                    order: buckets.len() as u32,
                });
            }
        }
    }
    buckets.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then(left.order.cmp(&right.order))
    });
    let mut colors = Vec::new();
    for bucket in buckets {
        if bucket.count == 0 {
            continue;
        }
        let count = f64::from(bucket.count);
        let rgb = (
            (bucket.r as f64 / count).round() as u8,
            (bucket.g as f64 / count).round() as u8,
            (bucket.b as f64 / count).round() as u8,
        );
        if colors
            .iter()
            .any(|existing: &String| color_distance(existing, rgb) < 30.0)
        {
            continue;
        }
        colors.push(format!("#{:02X}{:02X}{:02X}", rgb.0, rgb.1, rgb.2));
        if colors.len() == 5 {
            break;
        }
    }
    colors
}

struct Bucket {
    key: u16,
    count: u32,
    r: u64,
    g: u64,
    b: u64,
    order: u32,
}

/// 采样格上的一像素。源图和采样格同尺寸时落在同一格，实色不会被插值改掉。
fn nearest(
    rgba: &[u8],
    src_w: u32,
    src_h: u32,
    sample_w: u32,
    sample_h: u32,
    x: u32,
    y: u32,
) -> [u8; 4] {
    let sx = ((f64::from(x) + 0.5) * f64::from(src_w) / f64::from(sample_w)).floor() as u32;
    let sy = ((f64::from(y) + 0.5) * f64::from(src_h) / f64::from(sample_h)).floor() as u32;
    let sx = sx.min(src_w - 1);
    let sy = sy.min(src_h - 1);
    let index = ((sy * src_w + sx) as usize) * 4;
    [
        rgba[index],
        rgba[index + 1],
        rgba[index + 2],
        rgba[index + 3],
    ]
}

/// Vue `colorDistance`：已选颜色和候选平均色的欧氏距离。
fn color_distance(hex: &str, rgb: (u8, u8, u8)) -> f64 {
    let channel = |index: usize| u8::from_str_radix(&hex[index..index + 2], 16).unwrap_or(0) as f64;
    let left = (channel(1), channel(3), channel(5));
    let dr = left.0 - f64::from(rgb.0);
    let dg = left.1 - f64::from(rgb.1);
    let db = left.2 - f64::from(rgb.2);
    (dr * dr + dg * dg + db * db).sqrt()
}

/// `ThumbnailPalette.vue`：一排 34×12 的药丸，间距 8，带 `--border-soft` 描边。没有合法颜色时不占一行。
pub(super) fn swatches(colors: &[String], prefix: &str) -> Option<AnyView> {
    let chips: Vec<AnyView> = colors
        .iter()
        .enumerate()
        .filter_map(|(index, color)| {
            let key = format!(
                "{prefix}-{index}-{}",
                color.trim_start_matches('#').to_ascii_uppercase()
            );
            swatch(color, 34.0, 12.0, None, &key)
        })
        .collect();
    if chips.is_empty() {
        return None;
    }
    Some(
        widget(
            Stack::row(8.0)
                .wrap(true)
                .align(AlignSpec::Center)
                .with_layout(|layout| layout.row_gap = Some(LengthSpec::Px(8.0))),
        )
        .children(chips)
        .key(format!("{prefix}-row"))
        .into_any(),
    )
}

/// 元数据「调色板」一行的值：右对齐的 14×14 小方块，间距 4，`Xs` 圆角。
pub(super) fn metadata_chips(colors: &[String], prefix: &str) -> Option<AnyView> {
    let chips: Vec<AnyView> = colors
        .iter()
        .enumerate()
        .filter_map(|(index, color)| {
            swatch(
                color,
                14.0,
                14.0,
                Some(RadiusTier::Xs),
                &format!("{prefix}-{index}"),
            )
        })
        .collect();
    if chips.is_empty() {
        return None;
    }
    Some(
        widget(
            Stack::bar(4.0)
                .justify(JustifySpec::End)
                .align(AlignSpec::Center),
        )
        .children(chips)
        .key(format!("{prefix}-row"))
        .into_any(),
    )
}

/// 一枚色块：底色就是元数据里的颜色，描一圈 1px `--border-soft`。`radius` 为空时画成药丸。
fn swatch(
    color: &str,
    width: f32,
    height: f32,
    radius: Option<RadiusTier>,
    key: &str,
) -> Option<AnyView> {
    let Some(rgba) = super::files_view::style::hex_rgba(color) else {
        eprintln!("Nana 色板颜色无效，跳过：{color}");
        return None;
    };
    let mut frame = super::files_view::style::fixed(Stack::row(0.0), width, height)
        .outline(SemanticColorRole::BorderSoft, 1.0);
    frame = match radius {
        Some(tier) => frame.radius(tier),
        None => frame.radius_px(999.0),
    };
    let frame = frame.with_layout(move |layout| layout.background = Some(rgba));
    Some(widget(frame).key(key.to_string()).into_any())
}

fn hex_colors(value: Option<&Value>) -> Vec<String> {
    let Some(Value::Array(items)) = value else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| item.as_str())
        .filter(|color| is_hex(color))
        .take(5)
        .map(ToString::to_string)
        .collect()
}

fn is_hex(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 7 && bytes[0] == b'#' && bytes[1..].iter().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
#[path = "palette_tests.rs"]
mod tests;
