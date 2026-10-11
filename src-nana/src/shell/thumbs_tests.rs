//! 缩略图宽高比和图片解码的测试：EXIF 方向、SVG、超过宿主纹理上限的大图。

use super::*;

#[test]
fn aspect_clamps_to_the_vue_range_and_unknown_stays_square() {
    assert_eq!(content_aspect(0, 100), 1.0);
    assert_eq!(content_aspect(100, 0), 1.0);
    assert_eq!(content_aspect(800, 600), 800.0 / 600.0);
    assert_eq!(content_aspect(4000, 1000), ASPECT_MAX);
    assert_eq!(content_aspect(100, 1000), ASPECT_MIN);
}

/// 只有一项「方向」的 EXIF（小端 TIFF 头，IFD0 一条 0x0112）。JPEG 编码器自己补 `Exif\0\0`。
fn exif_orientation(value: u8) -> Vec<u8> {
    vec![
        0x49, 0x49, 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00, // TIFF 头，IFD0 在偏移 8
        0x01, 0x00, // 一条
        0x12, 0x01, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, value, 0x00, 0x00, 0x00, // 方向，SHORT，1 个
        0x00, 0x00, 0x00, 0x00, // 没有下一个 IFD
    ]
}

/// 32×16 的 JPEG：左半红、右半蓝，EXIF 写着方向 `orientation`。
fn sideways_jpeg(orientation: u8) -> Vec<u8> {
    use image::ImageEncoder;
    let image = image::RgbImage::from_fn(32, 16, |x, _| if x < 16 { image::Rgb([230, 20, 20]) } else { image::Rgb([20, 20, 230]) });
    let mut bytes = Vec::new();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 95);
    encoder.set_exif_metadata(exif_orientation(orientation)).expect("JPEG 能写 EXIF");
    encoder.write_image(image.as_raw(), 32, 16, image::ExtendedColorType::Rgb8).expect("JPEG 编码");
    bytes
}

fn pixel(pixels: &crate::shell::PreviewPixels, x: u32, y: u32) -> [u8; 4] {
    let index = ((y * pixels.width + x) * 4) as usize;
    pixels.rgba[index..index + 4].try_into().expect("一个像素")
}

/// 手机照片 EXIF 写着「顺时针转 90°」：预览摆正成竖图，原来左边的红色转到上面。Vue 的 `<img>` 默认这样摆正。
#[test]
fn a_photo_is_turned_upright_by_its_exif_orientation() {
    let pixels = decode_preview_pixels(&sideways_jpeg(6)).expect("JPEG 能解");
    assert_eq!((pixels.width, pixels.height), (16, 32), "按 EXIF 转成竖图");
    let top = pixel(&pixels, 8, 4);
    let bottom = pixel(&pixels, 8, 28);
    assert!(top[0] > 150 && top[2] < 100, "上半是原来左边的红色：{top:?}");
    assert!(bottom[2] > 150 && bottom[0] < 100, "下半是原来右边的蓝色：{bottom:?}");

    let upright = decode_preview_pixels(&sideways_jpeg(1)).expect("JPEG 能解");
    assert_eq!((upright.width, upright.height), (32, 16), "方向 1 不转");
}

/// 缩略图文件也按 EXIF 方向摆正，宽高比跟着摆正以后的尺寸。
#[test]
fn a_thumbnail_file_is_turned_upright_too() {
    let path = std::env::temp_dir().join(format!("momobako-thumb-exif-{}.jpg", std::process::id()));
    std::fs::write(&path, sideways_jpeg(8)).expect("写缩略图");
    let frame = decode_thumbnail_file(&path.to_string_lossy()).expect("缩略图能解");
    std::fs::remove_file(&path).expect("删缩略图");
    assert_eq!((frame.natural_width, frame.natural_height), (16, 32), "方向 8 是逆时针 90°，也成竖图");
}

/// SVG 交给 resvg：24×24 的图标放大到最长边 2048 再画，不发糊；像素是直通 alpha，半透明的红还是红。
#[test]
fn an_svg_is_rasterized_crisply_with_straight_alpha() {
    let svg = br##"<?xml version="1.0" encoding="UTF-8"?>
<!-- icon -->
<svg xmlns="http://www.w3.org/2000/svg" width="24" height="12" viewBox="0 0 24 12">
  <rect x="0" y="0" width="12" height="12" fill="#ff0000"/>
  <rect x="12" y="0" width="12" height="12" fill="#ff0000" fill-opacity="0.5"/>
</svg>"##;
    let pixels = decode_preview_pixels(svg).expect("SVG 能画");
    assert_eq!((pixels.width, pixels.height), (2048, 1024), "按原比例放大到最长边 2048");
    assert_eq!(pixel(&pixels, 512, 512), [255, 0, 0, 255], "左半不透明的红");
    let half = pixel(&pixels, 1536, 512);
    assert!(half[0] >= 250 && half[1] == 0 && half[2] == 0, "右半半透明的还是红色（直通 alpha）：{half:?}");
    assert!((120..=135).contains(&half[3]), "右半一半透明：{half:?}");
}

/// 文档尺寸比 2048 大的 SVG 缩到最长边 2048 以内，不放大。
#[test]
fn a_large_svg_is_capped() {
    let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="6000" height="3000"><rect width="6000" height="3000" fill="#00ff00"/></svg>"##;
    let pixels = decode_preview_pixels(svg).expect("SVG 能画");
    assert_eq!((pixels.width, pixels.height), (2048, 1024));
}

/// 不是图片也不是 SVG 的字节照旧报解码失败。
#[test]
fn unknown_bytes_still_fail() {
    let error = decode_preview_pixels(b"<html><body>not an image</body></html>").expect_err("不是图片");
    assert!(error.contains("图片解码失败"), "{error}");
    assert!(decode_preview_pixels(b"<svg").is_err(), "坏的 SVG 报失败");
}

/// 一边超过宿主纹理上限（8192）的长图按比例缩到上限以内，不再整张拒绝。
#[test]
fn an_oversized_image_is_scaled_to_the_texture_limit() {
    let wide = image::RgbImage::from_pixel(9000, 20, image::Rgb([10, 200, 30]));
    let mut bytes = Vec::new();
    image::DynamicImage::ImageRgb8(wide).write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png).expect("PNG 编码");
    let pixels = decode_preview_pixels(&bytes).expect("长图能解");
    assert_eq!(pixels.width, 8192, "最长边缩到上限");
    assert!((17..=19).contains(&pixels.height), "高按比例缩：{}", pixels.height);
    assert_eq!(pixel(&pixels, 4000, 9), [10, 200, 30, 255]);
}
