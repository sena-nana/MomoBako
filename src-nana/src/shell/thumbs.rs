//! 文件缩略图的宽高。
//!
//! Vue 在图片加载后用原始宽高比，并夹到 0.55–2.4。
//! 自适应、瀑布流、网格和列表的盒子按 `workspace.css` 的像素还原。

use image::GenericImageView;

use super::files::DisplayMode;

/// 与 `usePreviewUi` 的 `Math.min(Math.max(aspect, 0.55), 2.4)` 相同。
pub const ASPECT_MIN: f32 = 0.55;
pub const ASPECT_MAX: f32 = 2.4;

/// 一种展示方式下，预览图和条目卡片的逻辑像素。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThumbBox {
    pub preview_width: f32,
    pub preview_height: f32,
    pub item_width: f32,
    pub item_height: f32,
}

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

/// 按展示方式算出预览图和卡片。网格预览宽是 148 减去左右 8px 内边距。
pub fn thumb_box(mode: DisplayMode, width: u32, height: u32) -> ThumbBox {
    let aspect = content_aspect(width, height);
    match mode {
        DisplayMode::List => ThumbBox {
            preview_width: 56.0,
            preview_height: 56.0,
            item_width: 0.0,
            item_height: 72.0,
        },
        DisplayMode::Grid => ThumbBox {
            preview_width: 132.0,
            preview_height: 112.0,
            item_width: 148.0,
            item_height: 190.0,
        },
        DisplayMode::Adaptive => {
            let preview_height = 120.0;
            let preview_width = preview_height * aspect;
            ThumbBox {
                preview_width,
                preview_height,
                item_width: (preview_width + 16.0).clamp(118.0, 300.0),
                item_height: preview_height + 62.0,
            }
        }
        DisplayMode::Masonry => {
            let preview_width = 148.0;
            let preview_height = preview_width / aspect;
            ThumbBox {
                preview_width,
                preview_height,
                item_width: 164.0,
                item_height: preview_height + 62.0,
            }
        }
    }
}

/// 宿主纹理槽。同一路径始终对应同一槽，方便重复挂载。
pub fn thumbnail_slot(path: &str) -> String {
    format!("thumb:{path}")
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

    #[test]
    fn boxes_follow_the_vue_pixel_rules() {
        let square = thumb_box(DisplayMode::Adaptive, 1000, 1000);
        assert_eq!(square.preview_height, 120.0);
        assert_eq!(square.preview_width, 120.0);
        assert_eq!(square.item_width, 136.0);

        let wide = thumb_box(DisplayMode::Adaptive, 4000, 1000);
        assert_eq!(wide.preview_width, 120.0 * ASPECT_MAX);
        assert_eq!(wide.item_width, 300.0);

        let tall = thumb_box(DisplayMode::Adaptive, 100, 1000);
        assert_eq!(tall.item_width, 118.0);

        let grid = thumb_box(DisplayMode::Grid, 800, 600);
        assert_eq!((grid.preview_width, grid.preview_height), (132.0, 112.0));
        assert_eq!((grid.item_width, grid.item_height), (148.0, 190.0));

        let list = thumb_box(DisplayMode::List, 1920, 1080);
        assert_eq!((list.preview_width, list.preview_height), (56.0, 56.0));
        assert_eq!(list.item_height, 72.0);

        let masonry = thumb_box(DisplayMode::Masonry, 2000, 1000);
        assert_eq!(masonry.item_width, 164.0);
        assert_eq!(masonry.preview_width, 148.0);
        assert_eq!(masonry.preview_height, 74.0);
    }
}
