//! 文件缩略图的宽高比和解码，以及预览图片的解码。
//!
//! Vue 在图片加载后用原始宽高比，并夹到 0.55–2.4；各展示方式的卡片尺寸在 `files_cards.rs`。
//! 解码按 EXIF 方向摆正（照片和 Vue 的 `<img>` 一样不横躺、不倒置）。预览里的 SVG 交给 NanaUI 带着的 resvg
//! 光栅（`nana-svg-raster`）；预览图任一边超过宿主纹理上限时按比例缩小，不拒绝长条漫画和全景图。

use std::io::Cursor;

use image::{DynamicImage, GenericImageView, ImageDecoder, ImageReader};

/// 与 `usePreviewUi` 的 `Math.min(Math.max(aspect, 0.55), 2.4)` 相同。
pub const ASPECT_MIN: f32 = 0.55;
pub const ASPECT_MAX: f32 = 2.4;

/// 宽高未知或无效时按正方形。否则夹到 Vue 的范围。
pub fn content_aspect(width: u32, height: u32) -> f32 {
    if width == 0 || height == 0 {
        return 1.0;
    }
    let ratio = width as f32 / height as f32;
    if !ratio.is_finite() || ratio <= 0.0 {
        1.0
    } else {
        ratio.clamp(ASPECT_MIN, ASPECT_MAX)
    }
}

/// 宿主纹理槽的前缀。
const SLOT_PREFIX: &str = "thumb:";

/// 宿主纹理槽。同一路径始终对应同一槽，方便重复挂载。
pub fn thumbnail_slot(path: &str) -> String {
    format!("{SLOT_PREFIX}{path}")
}

/// 槽对应的缩略图文件；不是缩略图槽或文件为空时为 `None`。
pub fn slot_file(slot: &str) -> Option<&str> {
    slot.strip_prefix(SLOT_PREFIX).filter(|path| !path.is_empty())
}

/// 解码后的缩略图。布局用原始宽高，纹理用缩小后的像素。
#[derive(Clone, Debug)]
pub struct ThumbnailFrame {
    pub path: String,
    pub natural_width: u32,
    pub natural_height: u32,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// 预览图的最长边上限，和宿主纹理的上限一样。超过时按比例缩到这以内。
const PREVIEW_MAX_EDGE: u32 = 8192;
/// SVG 光栅的最长边：小图标放大到预览框里不发糊，大画布缩下来，一张最多 16 MiB。
const SVG_EDGE: u32 = 2048;

/// 把预览图解码成 RGBA（直通 alpha）。按 EXIF 方向摆正；认不出格式、内容是 SVG 时交给 resvg 光栅；
/// 任一边超过 [`PREVIEW_MAX_EDGE`] 时按比例缩小。
pub fn decode_preview_pixels(bytes: &[u8]) -> Result<super::PreviewPixels, String> {
    if image::guess_format(bytes).is_err() && looks_like_svg(bytes) {
        return rasterize_svg(bytes);
    }
    let mut image = decode_oriented(bytes).map_err(|error| format!("图片解码失败：{error}"))?;
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 {
        return Err(format!("图片尺寸不受支持：{width}x{height}"));
    }
    if width > PREVIEW_MAX_EDGE || height > PREVIEW_MAX_EDGE {
        eprintln!("Nana 预览图 {width}x{height} 超过宿主纹理上限，缩到 {PREVIEW_MAX_EDGE} 以内");
        image = image.thumbnail(PREVIEW_MAX_EDGE, PREVIEW_MAX_EDGE);
    }
    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();
    Ok(super::PreviewPixels { width, height, rgba: rgba.into_raw() })
}

/// 解码图片并按 EXIF 方向摆正。格式按内容认；EXIF 读不出时照原样，不让整张图失败。
fn decode_oriented(bytes: &[u8]) -> image::ImageResult<DynamicImage> {
    let mut decoder = ImageReader::new(Cursor::new(bytes)).with_guessed_format()?.into_decoder()?;
    let orientation = decoder.orientation().unwrap_or_else(|error| {
        eprintln!("Nana 图片的 EXIF 方向读不出，按原样显示：{error}");
        image::metadata::Orientation::NoTransforms
    });
    // 和 `load_from_memory` 一样先按默认上限预留解码缓冲，过大的图报上限错误，不硬分配。
    image::Limits::default().reserve(decoder.total_bytes())?;
    let mut image = DynamicImage::from_decoder(decoder)?;
    image.apply_orientation(orientation);
    Ok(image)
}

/// 内容像 SVG：去掉 UTF-8 BOM 和前导空白后以 `<` 开头，开头 4 KiB 里有 `<svg`（允许 XML 声明、注释和 DOCTYPE 在前）。
fn looks_like_svg(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(4096)];
    let head = head.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(head);
    let text = String::from_utf8_lossy(head);
    let text = text.trim_start();
    text.starts_with('<') && text.contains("<svg")
}

/// 用 resvg 把 SVG 画成直通 alpha 的 RGBA，文字用界面自带的 Noto Sans SC。
///
/// 算法：先按文档尺寸画（最长边超过 [`SVG_EDGE`] 时缩到它以内）；文档比它小时按原比例拉到最长边正好是
/// [`SVG_EDGE`] 重画一次。拉伸的画法交回预乘的像素，还原成直通 alpha，和别的图片一样由上传那一步再预乘。
fn rasterize_svg(bytes: &[u8]) -> Result<super::PreviewPixels, String> {
    let font = Some(nana_svg_raster::SvgFont { bytes: nana_ui_core::fonts::UI_FONT_REGULAR, family: nana_ui_core::fonts::UI_FONT_FAMILY });
    let Some(document) = nana_svg_raster::rasterize_document_capped_with_font(bytes, SVG_EDGE, font) else {
        eprintln!("Nana SVG 解析或绘制失败");
        return Err("SVG 解析或绘制失败".into());
    };
    let longest = document.width.max(document.height).max(1);
    let enlarged = (longest < SVG_EDGE)
        .then(|| std::str::from_utf8(bytes).ok())
        .flatten()
        .and_then(|markup| {
            let scale = SVG_EDGE as f32 / longest as f32;
            let width = ((document.width as f32 * scale).round() as u32).clamp(1, SVG_EDGE);
            let height = ((document.height as f32 * scale).round() as u32).clamp(1, SVG_EDGE);
            nana_svg_raster::rasterize_stretch(markup, width, height, font, SVG_EDGE)
        });
    let pixels = match enlarged {
        Some(raster) => {
            let mut rgba = raster.rgba.to_vec();
            unpremultiply(&mut rgba);
            super::PreviewPixels { width: raster.width, height: raster.height, rgba }
        }
        None => super::PreviewPixels { width: document.width, height: document.height, rgba: document.rgba.to_vec() },
    };
    Ok(pixels)
}

/// 预乘的 RGBA 还原成直通 alpha。全透明的像素留成 0。
fn unpremultiply(rgba: &mut [u8]) {
    for pixel in rgba.chunks_exact_mut(4) {
        let alpha = u16::from(pixel[3]);
        if alpha == 0 || alpha == 255 {
            continue;
        }
        for channel in &mut pixel[..3] {
            *channel = ((u16::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8;
        }
    }
}

/// 读取缩略图文件。原始尺寸（摆正以后）用于宽高比，上传前缩到 480 边以内。
pub fn decode_thumbnail_file(path: &str) -> Result<ThumbnailFrame, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("读取缩略图失败：{error}"))?;
    if bytes.len() > 20 * 1024 * 1024 {
        return Err(format!("缩略图过大：{} 字节", bytes.len()));
    }
    let image = decode_oriented(&bytes).map_err(|error| format!("缩略图解码失败：{error}"))?;
    let (natural_width, natural_height) = image.dimensions();
    if natural_width == 0 || natural_height == 0 {
        return Err("缩略图尺寸为空".into());
    }
    let scaled = image.thumbnail(480, 480);
    let rgba = scaled.to_rgba8();
    let (width, height) = rgba.dimensions();
    if width == 0 || height == 0 {
        return Err("缩略图缩放后尺寸为空".into());
    }
    Ok(ThumbnailFrame {
        path: path.to_string(),
        natural_width,
        natural_height,
        width,
        height,
        rgba: rgba.into_raw(),
    })
}

#[cfg(test)]
#[path = "thumbs_tests.rs"]
mod tests;
