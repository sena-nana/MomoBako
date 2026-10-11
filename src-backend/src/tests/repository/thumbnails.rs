//! 缩略图和预览源的仓库测试：预览源按扩展名给的媒体类型（桌面端只把 `image/*` 当图片解码）。

use std::fs;

use crate::services::repository::test_support::{create_repository_without_initial_sync, create_test_state};
use crate::services::repository::FileReadRequest;

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
