//! 文件缩略图的宽高比和解码。
//!
//! Vue 在图片加载后用原始宽高比，并夹到 0.55–2.4；各展示方式的卡片尺寸在 `files_cards.rs`。

use image::GenericImageView;

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

/// 把预览图解码成 RGBA。过大的图拒绝上传。
pub fn decode_preview_pixels(bytes: &[u8]) -> Result<super::PreviewPixels, String> {
    let image = image::load_from_memory(bytes).map_err(|error| format!("图片解码失败：{error}"))?;
    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();
    if width == 0 || height == 0 || width > 8192 || height > 8192 {
        return Err(format!("图片尺寸不受支持：{width}x{height}"));
    }
    Ok(super::PreviewPixels {
        width,
        height,
        rgba: rgba.into_raw(),
    })
}

/// 读取缩略图文件。原始尺寸用于宽高比，上传前缩到 480 边以内。
pub fn decode_thumbnail_file(path: &str) -> Result<ThumbnailFrame, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("读取缩略图失败：{error}"))?;
    if bytes.len() > 20 * 1024 * 1024 {
        return Err(format!("缩略图过大：{} 字节", bytes.len()));
    }
    let image = image::load_from_memory(&bytes).map_err(|error| format!("缩略图解码失败：{error}"))?;
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
mod tests {
    use super::*;

    #[test]
    fn aspect_clamps_to_the_vue_range_and_unknown_stays_square() {
        assert_eq!(content_aspect(0, 100), 1.0);
        assert_eq!(content_aspect(100, 0), 1.0);
        assert_eq!(content_aspect(800, 600), 800.0 / 600.0);
        assert_eq!(content_aspect(4000, 1000), ASPECT_MAX);
        assert_eq!(content_aspect(100, 1000), ASPECT_MIN);
    }
}
