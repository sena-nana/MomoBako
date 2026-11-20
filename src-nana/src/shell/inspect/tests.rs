//! 预览和元数据状态机测试。搜索与筛选的测试在 `shell/inspect/search/tests.rs`。
//!
//! 覆盖预览分派、元数据编辑和筛选栏的分支。不启动仓库服务，也不写用户目录。

use serde_json::{Value, json};

use super::super::player::PlayerMessage;
use super::super::player::StillShow;
use super::super::view_part_primary::RouteKey;
use super::super::workspace::{WorkspacePanel, WorkspaceRepository};
use super::super::{PreviewPixels, ShellMessage, ShellPage, ShellViewModel};
use super::bridge::{MeshData, NativeLoad, PageFrame};
use super::{InspectEffect, InspectMessage, InspectState, PreviewBody, PreviewKind};
use crate::backend::services::repository::{
    AssetDetail, AssetSummary, FileBrowserSnapshot, FilePreviewSourceResponse, MetadataEntry,
    RepositoryBackendSummary, RepositoryOverview, RepositorySnapshot,
    RepositoryStructureCacheState, RepositorySummary,
};

mod bodies;
mod media;
mod metadata;

fn asset(
    path: &str,
    extension: &str,
    version: i64,
    virtual_asset: bool,
    metadata: Vec<MetadataEntry>,
) -> AssetDetail {
    let filename = path.rsplit(['/', '\\']).next().unwrap_or(path).to_string();
    AssetDetail {
        summary: AssetSummary {
            asset_id: "asset".into(),
            repo_id: "repo".into(),
            path: path.into(),
            filename,
            extension: extension.into(),
            size_bytes: 12,
            size_label: "12 B".into(),
            status: "ready".into(),
            modified_at: String::new(),
            last_accessed_at: None,
            version,
            tags: vec!["summary-tag".into()],
            thumbnail_path: None,
            hardlink_group_id: None,
            hardlink_state: None,
            is_virtual: virtual_asset,
            provider_id: None,
            provider_item_id: None,
            source_payload: None,
            local_absolute_path: None,
        },
        metadata,
        revisions: Vec::new(),
    }
}

fn meta(key: &str, value: Value) -> MetadataEntry {
    MetadataEntry {
        key: key.into(),
        value_type: "string".into(),
        value,
        version: 1,
        updated_at: String::new(),
    }
}

fn writable_shell() -> ShellViewModel {
    let mut model = ShellViewModel::default();
    model.workspace.repositories.push(WorkspaceRepository {
        repo_id: "repo".into(),
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
    model.workspace.active_repo_id = Some("repo".into());
    model
}

fn source(path: &str) -> FilePreviewSourceResponse {
    FilePreviewSourceResponse {
        repo_id: "repo".into(),
        path: path.into(),
        token: "token".into(),
        local_path: None,
        media_type: "image/png".into(),
        size_bytes: 4,
        modified_at: None,
    }
}

fn tone_wav() -> Vec<u8> {
    let data = [
        0u8, 255, 0, 255, 0, 255, 0, 255, 0, 255, 0, 255, 0, 255, 0, 255,
    ];
    let mut body = Vec::new();
    body.extend_from_slice(b"fmt ");
    body.extend_from_slice(&16u32.to_le_bytes());
    body.extend_from_slice(&1u16.to_le_bytes());
    body.extend_from_slice(&1u16.to_le_bytes());
    body.extend_from_slice(&8_000u32.to_le_bytes());
    body.extend_from_slice(&8_000u32.to_le_bytes());
    body.extend_from_slice(&1u16.to_le_bytes());
    body.extend_from_slice(&8u16.to_le_bytes());
    body.extend_from_slice(b"data");
    body.extend_from_slice(&(data.len() as u32).to_le_bytes());
    body.extend_from_slice(&data);
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&((body.len() + 4) as u32).to_le_bytes());
    bytes.extend_from_slice(b"WAVE");
    bytes.extend_from_slice(&body);
    bytes
}

fn video_player() -> crate::backend::services::repository::PlaylistPlayerContribution {
    crate::backend::services::repository::PlaylistPlayerContribution {
        player_type_id: "momobako.playlist.video-sequence".into(),
        label: "视频顺序播放".into(),
        file_class: "video".into(),
        supported_extensions: vec!["avi".into(), "mp4".into()],
        supports_seek: true,
        supports_volume: true,
        supports_preview_navigation: true,
        description: None,
    }
}

/// 原生预览读好以后从 `LoadNative` 请求里取代数。
fn native_generation(model: &mut ShellViewModel) -> u64 {
    match model.inspect.take_effects().pop() {
        Some(InspectEffect::LoadNative { generation, .. }) => generation,
        _ => panic!("没有原生预览请求"),
    }
}
