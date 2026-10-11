//! 缩略图和预览源的仓库测试：预览源按扩展名给的媒体类型（桌面端只把 `image/*` 当图片解码），
//! 以及生成、保存缩略图时按 EXIF 方向摆正照片。

use std::fs;
use std::path::Path;

use crate::services::repository::test_support::{create_repository_without_initial_sync, create_test_state};
use crate::services::repository::{
    install_local_filesystem_test_plugin_archive, FileReadRequest, RepositoryMutationRequest, RepositoryState,
    ThumbnailRequest,
};

/// 建一个本地资源库并做首次同步，素材表里有根目录的文件。
fn create_synced_repository(state: &RepositoryState, repo_root: &Path) -> String {
    state.ensure_initialized().expect("repository state should initialize");
    install_local_filesystem_test_plugin_archive(&state.root_path());
    state
        .create_repository(RepositoryMutationRequest {
            repo_id: None,
            name: "Thumbnail Repo".to_string(),
            path: repo_root.to_string_lossy().to_string(),
            backend_plugin_id: None,
            backend_config: None,
            skip_initial_sync: false,
        })
        .expect("repository should be created")
        .repository
        .repo_id
}

fn thumbnail_request(repo_id: &str, path: &str, action: &str, image_bytes: Option<Vec<u8>>) -> ThumbnailRequest {
    ThumbnailRequest {
        repo_id: repo_id.to_string(),
        path: path.to_string(),
        action: Some(action.to_string()),
        source_path: None,
        source_url: None,
        image_bytes,
        media_type: None,
    }
}

/// 64×32 的横图 JPEG，EXIF 写着方向 6（顺时针转 90°），摆正以后是高为宽两倍的竖图。
fn sideways_jpeg() -> Vec<u8> {
    use image::ImageEncoder;
    let exif = vec![
        0x49, 0x49, 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00, 0x01, 0x00, 0x12, 0x01, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00,
        0x06, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];
    let image = image::RgbImage::from_pixel(64, 32, image::Rgb([200, 120, 40]));
    let mut bytes = Vec::new();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 90);
    encoder.set_exif_metadata(exif).expect("JPEG should accept EXIF");
    encoder
        .write_image(image.as_raw(), 64, 32, image::ExtendedColorType::Rgb8)
        .expect("JPEG should encode");
    bytes
}

fn thumbnail_dimensions(path: Option<&str>) -> (u32, u32) {
    image::image_dimensions(path.expect("thumbnail path should be returned")).expect("thumbnail should decode")
}

/// 图片扩展名的预览源是 `image/*`，大写扩展名按小写认；预览服务的 Content-Type 用同一个类型。
/// 原来图片一律是 `application/octet-stream`，桌面端的图片预览全部报不支持。
#[test]
fn image_preview_sources_carry_an_image_media_type() {
    let (state, root, repo_root, _thumbnail_root) = create_test_state("preview-image-types");
    let repo_id = create_repository_without_initial_sync(&state, &repo_root);
    let expected = [
        ("cover.png", "image/png"),
        ("photo.JPG", "image/jpeg"),
        ("photo.jpeg", "image/jpeg"),
        ("still.webp", "image/webp"),
        ("loop.gif", "image/gif"),
        ("scan.bmp", "image/bmp"),
        ("scan.tif", "image/tiff"),
        ("scan.tiff", "image/tiff"),
        ("modern.avif", "image/avif"),
        ("icon.svg", "image/svg+xml"),
        ("model.glb", "model/gltf-binary"),
        ("blob.bin", "application/octet-stream"),
    ];
    for (name, media_type) in expected {
        fs::write(repo_root.join(name), b"bytes").expect("preview source should be written");
        let response = state
            .prepare_preview_file_source(FileReadRequest { repo_id: repo_id.clone(), path: name.to_string() })
            .expect("preview source should be prepared");
        assert_eq!(response.media_type, media_type, "{name}");
        let (_file, served) = state.open_preview_file_source(&response.token).expect("prepared source should open");
        assert_eq!(served, media_type, "{name} 的预览服务类型");
    }

    fs::remove_dir_all(root).expect("test temp root should be removed");
}

/// 生成的缩略图和存下的自定义缩略图都按 EXIF 方向摆正：横着拍、EXIF 写着转 90° 的照片成竖图。
#[test]
fn thumbnails_follow_the_photo_exif_orientation() {
    let (state, root, repo_root, _thumbnail_root) = create_test_state("thumb-exif-orientation");
    fs::write(repo_root.join("photo.jpg"), sideways_jpeg()).expect("photo should be written");
    fs::write(repo_root.join("note.txt"), "text").expect("note should be written");
    let repo_id = create_synced_repository(&state, &repo_root);

    let generated = state
        .ensure_thumbnail(thumbnail_request(&repo_id, "photo.jpg", "ensure", None))
        .expect("photo thumbnail should be generated");
    let (width, height) = thumbnail_dimensions(generated.thumbnail_path.as_deref());
    assert_eq!(height, width * 2, "生成的缩略图是摆正以后的竖图：{width}x{height}");

    let saved = state
        .ensure_thumbnail(thumbnail_request(&repo_id, "note.txt", "save", Some(sideways_jpeg())))
        .expect("custom thumbnail should be saved");
    assert!(saved.thumbnail_custom);
    let (width, height) = thumbnail_dimensions(saved.thumbnail_path.as_deref());
    assert_eq!(height, width * 2, "存下的自定义缩略图是摆正以后的竖图：{width}x{height}");

    fs::remove_dir_all(root).expect("test temp root should be removed");
}
