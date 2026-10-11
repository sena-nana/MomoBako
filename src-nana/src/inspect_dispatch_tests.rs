//! 图片预览的真实派发路径：归约排下 `LoadImage`，用产品同一个函数（[`super::load_image_preview`]）准备预览源、
//! 读字节、解码，结果再交回归约。预览源的媒体类型用后端真实的扩展名映射
//! （`preview_media_type_for_extension`）：后端给图片的类型不对时，这里的图片预览会失败。离屏场景直接往归约里
//! 注入像素，拦不住这类问题。

use std::path::Path;

use crate::backend::services::repository::{
    preview_media_type_for_extension, AssetDetail, AssetSummary, FilePreviewSourceResponse, FileReadRequest,
};
use crate::shell::{InspectEffect, ShellMessage, ShellViewModel, WorkspaceRepository};

use super::{load_image_preview, ImagePreviewSource};

const REPO_ID: &str = "repo";
const COVER_PNG: &[u8] = include_bytes!("shell/preview_fixtures/cover.png");

/// 后端预览源的替身：媒体类型照后端按小写扩展名给，字节是夹具。
struct Backend {
    bytes: Vec<u8>,
}

impl ImagePreviewSource for Backend {
    fn prepare(&self, request: FileReadRequest) -> Result<FilePreviewSourceResponse, String> {
        let extension = Path::new(&request.path)
            .extension()
            .map(|extension| extension.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        Ok(FilePreviewSourceResponse {
            repo_id: request.repo_id,
            token: format!("token:{}", request.path),
            path: request.path,
            source_url: None,
            local_path: None,
            media_type: preview_media_type_for_extension(&extension).to_string(),
            size_bytes: self.bytes.len() as i64,
            modified_at: None,
        })
    }

    fn read(&self, _request: FileReadRequest) -> Result<Vec<u8>, String> {
        Ok(self.bytes.clone())
    }
}

fn shell() -> ShellViewModel {
    let mut model = ShellViewModel::default();
    model.workspace.repositories.push(WorkspaceRepository {
        repo_id: REPO_ID.into(),
        name: "库".into(),
        path: "C:/repo".into(),
        status: "ready".into(),
        backend_plugin_id: "filesystem".into(),
        capabilities: vec!["write".into()],
        cache_required: false,
        cache_status: String::new(),
        cache_path: None,
        login_expired: false,
        logged_out: false,
    });
    model.workspace.active_repo_id = Some(REPO_ID.into());
    model
}

fn detail(path: &str) -> AssetDetail {
    let filename = path.rsplit('/').next().unwrap_or(path).to_string();
    let extension = filename.rsplit_once('.').map(|(_, extension)| extension.to_string()).unwrap_or_default();
    AssetDetail {
        summary: AssetSummary {
            asset_id: format!("asset:{path}"),
            repo_id: REPO_ID.into(),
            path: path.into(),
            filename,
            extension,
            size_bytes: 12,
            size_label: "12 B".into(),
            status: "ready".into(),
            modified_at: String::new(),
            last_accessed_at: None,
            version: 1,
            tags: Vec::new(),
            thumbnail_path: None,
            hardlink_group_id: None,
            hardlink_state: None,
            is_virtual: false,
            provider_id: None,
            provider_item_id: None,
            source_payload: None,
            local_absolute_path: None,
        },
        metadata: Vec::new(),
        revisions: Vec::new(),
    }
}

/// 打开这张图片的预览，把归约排下的 `LoadImage` 按产品的派发函数跑完，结果交回归约。
fn preview(model: &mut ShellViewModel, path: &str, bytes: &[u8]) {
    model.reduce(ShellMessage::AssetDetailLoaded(Ok(detail(path))));
    let request = model
        .inspect
        .take_effects()
        .into_iter()
        .find_map(|effect| match effect {
            InspectEffect::LoadImage { repo_id, path } => Some((repo_id, path)),
            _ => None,
        });
    let (repo_id, path) = request.expect("图片预览排下读取");
    let message = load_image_preview(&Backend { bytes: bytes.to_vec() }, repo_id, path);
    model.reduce(message);
}

/// 本地 PNG 经真实派发路径解成像素：后端给的是 `image/png`，预览拿到原图尺寸和令牌。
#[test]
fn a_local_png_previews_through_the_real_dispatch_path() {
    let mut model = shell();
    preview(&mut model, "pics/cover.png", COVER_PNG);
    let pixels = model.preview_pixels.as_ref().expect("图片预览有像素");
    assert_eq!((pixels.width, pixels.height), (480, 300));
    assert_eq!(model.preview_token.as_deref(), Some("token:pics/cover.png"));
}

/// 扩展名大写也一样：后端按小写扩展名给类型。
#[test]
fn an_upper_case_extension_still_previews() {
    let mut model = shell();
    preview(&mut model, "pics/COVER.PNG", COVER_PNG);
    assert!(model.preview_pixels.is_some(), "大写扩展名的图片也要能预览");
}

/// SVG 经同一条路径预览：后端给 `image/svg+xml`，解码交给 resvg，不再报「图片解码失败」。
#[test]
fn an_svg_previews_through_the_real_dispatch_path() {
    let mut model = shell();
    let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><rect width="40" height="20" fill="#3fa796"/></svg>"##;
    preview(&mut model, "icons/badge.svg", svg);
    let pixels = model.preview_pixels.as_ref().expect("SVG 预览有像素");
    assert_eq!((pixels.width, pixels.height), (2048, 1024), "按原比例画到最长边 2048");
}
