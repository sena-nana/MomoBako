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

/// 网格里的页图和旁边的类型图标用同一只预览盒。整页按这个盒子缩小，页边留在盒子里。
pub fn grid_page_box(width: u32, height: u32) -> ThumbBox {
    thumb_box(DisplayMode::Grid, width, height)
}

/// 把已有像素装进缩略图盒子。只缩小，不放大，也不补不存在的画面。
#[cfg_attr(not(test), allow(dead_code))]
pub fn fit_rgba(width: u32, height: u32, rgba: &[u8], max_w: u32, max_h: u32) -> Option<(u32, u32, Vec<u8>)> {
    fit_filtered(width, height, rgba, max_w, max_h, image::imageops::FilterType::Triangle, false)
}

fn fit_filtered(
    width: u32,
    height: u32,
    rgba: &[u8],
    max_w: u32,
    max_h: u32,
    filter: image::imageops::FilterType,
    allow_upscale: bool,
) -> Option<(u32, u32, Vec<u8>)> {
    if width == 0 || height == 0 || max_w == 0 || max_h == 0 {
        return None;
    }
    let pixels = (width as usize).checked_mul(height as usize)?.checked_mul(4)?;
    if rgba.len() < pixels {
        eprintln!("Nana 缩略图像素不够：{width}x{height}，字节 {}", rgba.len());
        return None;
    }
    let image = image::RgbaImage::from_raw(width, height, rgba[..pixels].to_vec())?;
    let scale = (max_w as f32 / width as f32).min(max_h as f32 / height as f32);
    if !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    let scale = if allow_upscale { scale.min(8.0) } else { scale.min(1.0) };
    let dst_w = ((width as f32 * scale).round() as u32).clamp(1, max_w);
    let dst_h = ((height as f32 * scale).round() as u32).clamp(1, max_h);
    let scaled = image::imageops::resize(&image, dst_w, dst_h, filter);
    Some((scaled.width(), scaled.height(), scaled.into_raw()))
}

/// 整页缩进预览盒。四周留出底色当页边，再描一圈边，避免白页贴在浅底上看不出来。
pub fn fit_page_sheet(width: u32, height: u32, rgba: &[u8], max_w: u32, max_h: u32) -> Option<(u32, u32, Vec<u8>)> {
    let pad = 10u32;
    if max_w <= pad * 2 || max_h <= pad * 2 {
        eprintln!("Nana 详情预览盒太小：{max_w}x{max_h}");
        return None;
    }
    let (page_w, page_h, page) = fit_filtered(
        width,
        height,
        rgba,
        max_w - pad * 2,
        max_h - pad * 2,
        image::imageops::FilterType::Lanczos3,
        false,
    )?;
    let mut canvas = vec![236u8, 238, 241, 255].repeat((max_w as usize) * (max_h as usize));
    let origin_x = (max_w - page_w) / 2;
    let origin_y = (max_h - page_h) / 2;
    for y in 0..page_h {
        let src = (y as usize) * (page_w as usize) * 4;
        let dst = (((origin_y + y) as usize) * (max_w as usize) + origin_x as usize) * 4;
        canvas[dst..dst + (page_w as usize) * 4].copy_from_slice(&page[src..src + (page_w as usize) * 4]);
    }
    let x1 = origin_x + page_w;
    let y1 = origin_y + page_h;
    for x in origin_x..x1 {
        stamp(&mut canvas, max_w, x, origin_y);
        stamp(&mut canvas, max_w, x, y1.saturating_sub(1));
    }
    for y in origin_y..y1 {
        stamp(&mut canvas, max_w, origin_x, y);
        stamp(&mut canvas, max_w, x1.saturating_sub(1), y);
    }
    Some((max_w, max_h, canvas))
}

fn stamp(canvas: &mut [u8], width: u32, x: u32, y: u32) {
    let index = ((y as usize) * (width as usize) + x as usize) * 4;
    if index + 3 < canvas.len() {
        canvas[index] = 120;
        canvas[index + 1] = 124;
        canvas[index + 2] = 130;
        canvas[index + 3] = 255;
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

        let page = grid_page_box(480, 640);
        assert_eq!((page.preview_width, page.preview_height), (132.0, 112.0));
        assert_eq!((page.item_width, page.item_height), (148.0, 190.0));
    }

    #[test]
    fn page_grid_keeps_the_whole_sheet() {
        let width = 480u32;
        let height = 640u32;
        let rgba = vec![255u8; width as usize * height as usize * 4];
        let box_px = grid_page_box(width, height);
        let max_w = box_px.preview_width.round() as u32;
        let max_h = box_px.preview_height.round() as u32;
        let (dst_w, dst_h, sheet) = fit_page_sheet(width, height, &rgba, max_w, max_h).expect("整页装进网格");
        assert_eq!((dst_w, dst_h), (max_w, max_h));
        assert!(sheet[0] < 250, "盒子左上角没有页边底色");
        let mid = (((dst_h / 2) * dst_w + dst_w / 2) * 4) as usize;
        assert_eq!(&sheet[mid..mid + 3], &[255, 255, 255], "页心不是整页白纸");
        let edge = (((dst_h / 2) * dst_w) * 4) as usize;
        assert!(sheet[edge] < 250, "页纸贴到了盒子左边，看不出页边");
    }

    #[test]
    fn page_sheet_keeps_the_whole_page_and_a_visible_edge() {
        let width = 80u32;
        let height = 120u32;
        let rgba = vec![255u8; width as usize * height as usize * 4];
        let (dst_w, dst_h, sheet) = fit_page_sheet(width, height, &rgba, 160, 100).expect("整页装进盒子");
        assert_eq!((dst_w, dst_h), (160, 100));
        let edged = sheet.chunks(4).any(|pixel| pixel[0] < 180 && pixel[1] < 180);
        assert!(edged, "页边没有描出来");
        let mid = (((dst_h / 2) * dst_w + dst_w / 2) * 4) as usize;
        assert_eq!(&sheet[mid..mid + 3], &[255, 255, 255]);
        let corner = 0usize;
        assert!(sheet[corner] < 250, "盒子四周没有留出页边底色");
    }

    #[test]
    fn fit_rgba_shrinks_existing_pixels_and_rejects_empty() {
        assert!(fit_rgba(0, 2, &[0, 0, 0, 255], 8, 8).is_none());
        assert!(fit_rgba(2, 2, &[255, 0, 0, 255], 8, 8).is_none());
        let rgba = vec![12u8, 40, 80, 255].repeat(8 * 8);
        let (width, height, scaled) = fit_rgba(8, 8, &rgba, 4, 2).expect("缩小");
        assert!(width <= 4 && height <= 2 && width > 0 && height > 0);
        assert_eq!(scaled.len(), width as usize * height as usize * 4);
        assert_eq!(&scaled[..4], &[12, 40, 80, 255]);
    }
}
