//! 预览和元数据状态机测试。搜索与筛选的测试在 `search_tests.rs`。
//!
//! 这些分支来自 Vue 预览分派、元数据编辑和筛选栏。不启动仓库服务，也不写用户目录。

use serde_json::{json, Value};

use crate::backend::services::repository::{
    AssetDetail, AssetSummary, FileBrowserSnapshot, FilePreviewSourceResponse, MetadataEntry, RepositoryBackendSummary,
    RepositoryOverview, RepositorySnapshot, RepositoryStructureCacheState, RepositorySummary,
};
use crate::plugin_api::{NativeContributionKind, NativePluginContribution};

use super::super::workspace::{WorkspacePanel, WorkspaceRepository};
use super::super::{PreviewPixels, ShellMessage, ShellPage, ShellViewModel};
use super::{InspectEffect, InspectMessage, InspectState, PreviewBinding, PreviewBody, PreviewKind};

fn asset(path: &str, extension: &str, version: i64, virtual_asset: bool, metadata: Vec<MetadataEntry>) -> AssetDetail {
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
    MetadataEntry { key: key.into(), value_type: "string".into(), value, version: 1, updated_at: String::new() }
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
    });
    model.workspace.active_repo_id = Some("repo".into());
    model
}

fn preview_binding(kind: NativeContributionKind, extension: &str) -> PreviewBinding {
    PreviewBinding {
        extensions: vec![extension.into()],
        contribution: NativePluginContribution::new("plugin", kind, "官方预览", "view-pdf"),
    }
}

fn source(path: &str) -> FilePreviewSourceResponse {
    FilePreviewSourceResponse {
        repo_id: "repo".into(),
        path: path.into(),
        token: "token".into(),
        source_url: None,
        local_path: None,
        media_type: "image/png".into(),
        size_bytes: 4,
        modified_at: None,
    }
}

#[test]
fn extension_dispatch_prefers_markdown_and_native_preview() {
    assert_eq!(super::support::classify("PNG", &[]), PreviewKind::Image);
    assert_eq!(super::support::classify("md", &[]), PreviewKind::Markdown);
    assert_eq!(super::support::classify("txt", &[]), PreviewKind::Text);
    assert_eq!(super::support::classify("vue", &[]), PreviewKind::Text);
    assert_eq!(super::support::classify("mp3", &[]), PreviewKind::Media);
    assert_eq!(
        super::support::classify("pdf", &[]),
        PreviewKind::Native { view_id: "momobako.preview.pdf".into(), label: "PDF".into() }
    );
    assert_eq!(
        super::support::classify("glb", &[]),
        PreviewKind::Native { view_id: "momobako.preview.model".into(), label: "模型".into() }
    );
    assert_eq!(
        super::support::classify("zip", &[]),
        PreviewKind::Native { view_id: "momobako.preview.archive".into(), label: "压缩包".into() }
    );
    assert_eq!(super::support::classify("bin", &[]), PreviewKind::Unsupported);

    let native = preview_binding(NativeContributionKind::Preview, "pdf");
    assert_eq!(
        super::support::classify("pdf", &[native]),
        PreviewKind::Native { view_id: "momobako.preview.pdf".into(), label: "PDF".into() }
    );
    let playlist = preview_binding(NativeContributionKind::PlaylistPlayer, "pdf");
    assert_eq!(
        super::support::classify("pdf", &[playlist]),
        PreviewKind::Native { view_id: "momobako.preview.pdf".into(), label: "PDF".into() }
    );
}

#[test]
fn text_over_the_vue_limit_is_an_error_and_invalid_utf8_is_lossy() {
    let limit = 768 * 1024;
    assert_eq!(super::prepare_text(&vec![b'a'; limit]).unwrap().len(), limit);
    let error = super::prepare_text(&vec![b'a'; limit + 1]).unwrap_err();
    assert!(error.contains("文本超过"));
    assert_eq!(super::prepare_text(&[0xff]), Ok("\u{FFFD}".into()));
}

#[test]
fn preview_kind_starts_the_matching_body() {
    let mut state = InspectState::default();
    state.reduce(true, Some("repo"), InspectMessage::RegisterPreview(preview_binding(NativeContributionKind::PlaylistPlayer, "pdf")));
    state.note_detail(&asset("docs/a.pot", "pot", 1, false, Vec::new()));
    assert!(matches!(
        state.take_effects().pop(),
        Some(InspectEffect::LoadNative { view_id, path, .. }) if view_id == "momobako.preview.office" && path == "docs/a.pot"
    ));
    state.note_detail(&asset("docs/a.pdf", "pdf", 1, false, Vec::new()));
    assert!(matches!(
        state.take_effects().pop(),
        Some(InspectEffect::LoadNative { view_id, .. }) if view_id == "momobako.preview.pdf"
    ));

    state.reduce(true, Some("repo"), InspectMessage::RegisterPreview(preview_binding(NativeContributionKind::Preview, "pdf")));
    state.note_detail(&asset("docs/a.pdf", "pdf", 1, false, Vec::new()));
    assert!(matches!(
        state.take_effects().pop(),
        Some(InspectEffect::LoadNative { view_id, .. }) if view_id == "momobako.preview.pdf"
    ));

    state.note_detail(&asset("pics/a.png", "png", 1, false, Vec::new()));
    assert!(matches!(state.take_effects().pop(), Some(InspectEffect::LoadImage { path, .. }) if path == "pics/a.png"));

    state.note_detail(&asset("notes/a.md", "md", 1, false, Vec::new()));
    assert!(matches!(
        state.take_effects().pop(),
        Some(InspectEffect::LoadText { markdown: true, path, .. }) if path == "notes/a.md"
    ));
    state.note_detail(&asset("notes/a.txt", "txt", 1, false, Vec::new()));
    assert!(matches!(state.take_effects().pop(), Some(InspectEffect::LoadText { markdown: false, .. })));

    state.note_detail(&asset("models/a.glb", "glb", 1, false, Vec::new()));
    assert!(matches!(
        state.take_effects().pop(),
        Some(InspectEffect::LoadNative { view_id, path, .. }) if view_id == "momobako.preview.model" && path == "models/a.glb"
    ));
    state.note_detail(&asset("misc/a.bin", "bin", 1, false, Vec::new()));
    assert!(matches!(state.body, PreviewBody::Failed(ref message) if message == "无法预览此类型"));
}

#[test]
fn image_errors_stay_failed_and_stale_pixels_are_ignored() {
    let mut model = writable_shell();
    model.reduce(ShellMessage::SelectFile { path: "pics/a.png".into(), asset_id: None });
    model.preview_pixels = Some(PreviewPixels { width: 1, height: 1, rgba: vec![1, 2, 3, 4] });
    model.reduce(ShellMessage::PreviewPixelsLoaded {
        source: source("pics/other.png"),
        pixels: Ok(PreviewPixels { width: 2, height: 2, rgba: vec![9, 9, 9, 9] }),
    });
    assert_eq!(model.preview_pixels.as_ref().map(|pixels| pixels.width), Some(1));
    assert!(matches!(model.inspect.body, PreviewBody::Empty));

    model.reduce(ShellMessage::PreviewPixelsLoaded {
        source: source("pics/a.png"),
        pixels: Err("坏图".into()),
    });
    assert!(model.preview_pixels.is_none());
    assert!(matches!(model.inspect.body, PreviewBody::Failed(ref message) if message.contains("坏图")));
}

#[test]
fn media_without_a_decoder_never_reports_playing() {
    let mut state = InspectState::default();
    state.note_detail(&asset("audio/a.mp3", "mp3", 1, false, Vec::new()));
    let InspectEffect::LoadMedia { path, generation, .. } = state.take_effects().pop().unwrap() else {
        panic!("没有音视频请求");
    };
    state.reduce(true, Some("repo"), InspectMessage::MediaLoaded {
        path,
        generation,
        result: Err("没有原生解码器".into()),
        pcm: None,
        frames: None,
    });
    let PreviewBody::Media(session) = &state.body else { panic!("不是音视频") };
    assert_eq!(session.status, "failed");
    assert!(session.error.as_deref().unwrap_or_default().contains("没有原生解码器"));
    for message in [InspectMessage::PlayPause, InspectMessage::Seek(1_500), InspectMessage::SetVolume(0.4)] {
        state.reduce(true, Some("repo"), message);
        let PreviewBody::Media(session) = &state.body else { panic!("控制后丢了会话") };
        assert_eq!(session.status, "failed");
        assert_ne!(session.status, "playing");
    }
    assert!(super::support::preview_media_session("repo", b"ID3").is_err());
}

#[test]
fn wav_preview_plays_and_seeks_without_opening_a_device() {
    let mut state = InspectState::default();
    state.note_detail(&asset("audio/a.wav", "wav", 1, false, Vec::new()));
    let InspectEffect::LoadMedia { path, generation, .. } = state.take_effects().pop().unwrap() else {
        panic!("没有音视频请求");
    };
    let session = super::support::preview_media_session("repo", &tone_wav()).expect("wav");
    assert_eq!(session.status, "paused");
    assert_eq!(session.duration_ms, Some(2));
    state.reduce(true, Some("repo"), InspectMessage::MediaLoaded { path, generation, result: Ok(session), pcm: None, frames: None });
    state.reduce(true, Some("repo"), InspectMessage::PlayPause);
    state.reduce(true, Some("repo"), InspectMessage::Seek(1));
    let PreviewBody::Media(session) = &state.body else { panic!("丢了会话") };
    assert_eq!(session.status, "playing");
    assert_eq!(session.current_time_ms, 1);
    assert!(session.error.is_none());
}

fn tone_wav() -> Vec<u8> {
    let data = [0u8, 255, 0, 255, 0, 255, 0, 255, 0, 255, 0, 255, 0, 255, 0, 255];
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

#[test]
fn stale_text_is_ignored_and_the_current_generation_renders() {
    let mut state = InspectState::default();
    state.note_detail(&asset("notes/a.txt", "txt", 1, false, Vec::new()));
    let InspectEffect::LoadText { path, generation, markdown, .. } = state.take_effects().pop().unwrap() else {
        panic!("没有文本请求");
    };
    state.begin_selection("notes/b.txt");
    state.reduce(true, Some("repo"), InspectMessage::BodyLoaded {
        path: path.clone(),
        markdown,
        generation,
        result: Ok("旧文本".into()),
    });
    assert!(matches!(state.body, PreviewBody::Empty));
    let generation = state.generation;
    state.reduce(true, Some("repo"), InspectMessage::BodyLoaded {
        path: "notes/b.txt".into(),
        markdown: false,
        generation,
        result: Err("文本超过 786432 字节".into()),
    });
    assert!(matches!(state.body, PreviewBody::Failed(ref message) if message.contains("文本超过")));
}

#[test]
fn metadata_draft_tracks_edits_and_refuses_invalid_saves() {
    let mut state = InspectState::default();
    state.note_detail(&asset(
        "pics/a.png",
        "png",
        3,
        false,
        vec![meta("note", json!("from-note")), meta("tagGroups", json!(["kept"]))],
    ));
    assert_eq!(state.draft.comment, "from-note");
    assert_eq!(state.draft.tags, ["kept"]);
    state.reduce(true, Some("repo"), InspectMessage::SetComment("hi".into()));
    state.reduce(true, Some("repo"), InspectMessage::SetRating(4));
    state.reduce(true, Some("repo"), InspectMessage::SetRating(4));
    assert_eq!(state.draft.rating, 0);
    state.reduce(true, Some("repo"), InspectMessage::SetRating(4));
    state.reduce(true, Some("repo"), InspectMessage::AddTag("  ".into()));
    state.reduce(true, Some("repo"), InspectMessage::AddTag("a".into()));
    state.reduce(true, Some("repo"), InspectMessage::AddTag("a".into()));
    state.reduce(true, Some("repo"), InspectMessage::SetCustom { key: " ".into(), value: "x".into() });
    state.reduce(true, Some("repo"), InspectMessage::SetCustom { key: "rating".into(), value: "no".into() });
    state.reduce(true, Some("repo"), InspectMessage::SetCustom { key: "artist".into(), value: "momo".into() });
    assert!(state.dirty());
    state.reduce(true, Some("repo"), InspectMessage::SaveMetadata);
    state.reduce(true, Some("repo"), InspectMessage::SaveMetadata);
    let InspectEffect::SaveMetadata { repo_id, asset_id, expected_version, metadata } = state.take_effects().pop().unwrap() else {
        panic!("没有保存请求");
    };
    assert!(state.effects.is_empty());
    assert_eq!(repo_id, "repo");
    assert_eq!(asset_id, "asset");
    assert_eq!(expected_version, 3);
    assert_eq!(metadata.get("comment"), Some(&json!("hi")));
    assert_eq!(metadata.get("rating"), Some(&json!(4)));
    assert_eq!(metadata.get("tagGroups"), Some(&json!(["kept", "a"])));
    assert_eq!(metadata.get("artist"), Some(&json!("momo")));
    assert!(state.saving);

    state.reduce(true, Some("repo"), InspectMessage::Undo);
    assert!(state.effects.is_empty());
    state.reduce(true, Some("repo"), InspectMessage::MetadataSaved(Err("磁盘错误".into())));
    assert!(!state.saving);
    assert_eq!(state.draft.comment, "hi");
    assert!(state.error.contains("磁盘错误"));
}

#[test]
fn conflict_keeps_the_draft_until_the_server_version_is_adopted() {
    let mut state = InspectState::default();
    state.note_detail(&asset("pics/a.png", "png", 3, false, Vec::new()));
    state.reduce(true, Some("repo"), InspectMessage::SetComment("local".into()));
    state.reduce(true, Some("repo"), InspectMessage::SaveMetadata);
    state.take_effects();
    let server = asset("pics/a.png", "png", 9, false, vec![meta("comment", json!("server"))]);
    state.reduce(true, Some("repo"), InspectMessage::MetadataSaved(Ok(("conflict".into(), server))));
    assert_eq!(state.draft.comment, "local");
    assert_eq!(state.expected_version, 3);
    assert_eq!(state.conflict, "版本冲突，未写入");
    state.reduce(true, Some("repo"), InspectMessage::AdoptConflict);
    assert_eq!(state.draft.comment, "server");
    assert_eq!(state.expected_version, 9);
    assert!(!state.dirty());
    assert!(state.conflict.is_empty());
}

#[test]
fn successful_save_on_the_same_path_does_not_restart_preview() {
    let mut state = InspectState::default();
    state.note_detail(&asset("pics/a.png", "png", 3, false, Vec::new()));
    state.take_effects();
    state.reduce(true, Some("repo"), InspectMessage::SetLink("https://example.test".into()));
    state.reduce(true, Some("repo"), InspectMessage::SaveMetadata);
    state.take_effects();
    let saved = asset("pics/a.png", "png", 4, false, vec![meta("link", json!("https://example.test"))]);
    state.reduce(true, Some("repo"), InspectMessage::MetadataSaved(Ok(("success".into(), saved))));
    assert!(!state.dirty());
    assert_eq!(state.expected_version, 4);
    assert!(state.effects.is_empty());
    assert!(matches!(state.body, PreviewBody::Empty));
}

#[test]
fn virtual_clean_and_unrecognized_metadata_results_do_not_save() {
    let mut virtual_asset = InspectState::default();
    virtual_asset.note_detail(&asset("pics/a.png", "png", 1, true, Vec::new()));
    virtual_asset.take_effects();
    virtual_asset.reduce(true, Some("repo"), InspectMessage::SetComment("x".into()));
    virtual_asset.reduce(true, Some("repo"), InspectMessage::SaveMetadata);
    assert!(virtual_asset.effects.is_empty());
    assert!(!virtual_asset.saving);

    let mut clean = InspectState::default();
    clean.note_detail(&asset("pics/a.png", "png", 1, false, Vec::new()));
    clean.take_effects();
    clean.reduce(true, Some("repo"), InspectMessage::SaveMetadata);
    assert!(clean.effects.is_empty());
    assert!(!clean.saving);

    clean.reduce(true, Some("repo"), InspectMessage::SetComment("x".into()));
    clean.reduce(true, Some("repo"), InspectMessage::SaveMetadata);
    clean.take_effects();
    clean.reduce(true, Some("repo"), InspectMessage::MetadataSaved(Ok(("weird".into(), asset("pics/a.png", "png", 2, false, Vec::new())))));
    assert!(clean.error.contains("无法识别"));
    assert_eq!(clean.draft.comment, "x");
}

#[test]
fn primary_action_reopens_a_live_target_and_acceptance_keeps_the_old_detail() {
    let mut live = writable_shell();
    live.reduce(ShellMessage::AssetDetailLoaded(Ok(asset("pics/a.png", "png", 1, false, Vec::new()))));
    live.inspect.take_effects();
    live.detail = "保持".into();
    live.reduce(ShellMessage::PrimaryAction);
    assert_eq!(live.detail, "保持");
    assert!(matches!(live.inspect.effects.last(), Some(InspectEffect::LoadImage { .. })));

    let mut acceptance = ShellViewModel::for_page(ShellPage::SelectedFile);
    assert!(acceptance.inspect_surface_visible());
    acceptance.reduce(ShellMessage::PrimaryAction);
    assert_eq!(acceptance.detail, "当前没有可打开的预览");
    assert!(acceptance.inspect.effects.is_empty());

    live.reduce(ShellMessage::OpenDirectory("folder".into()));
    assert!(live.inspect.target_path.is_none());
}

#[test]
fn inspect_surface_appears_only_after_startup_with_a_repository() {
    let mut model = ShellViewModel::default();
    model.workspace.panel = WorkspacePanel::Search;
    assert!(!model.inspect_surface_visible());

    model.reduce(ShellMessage::RepositoriesLoaded(Ok(vec![RepositorySummary {
        repo_id: "repo".into(),
        name: "库".into(),
        path: "C:/repo".into(),
        backend: RepositoryBackendSummary {
            plugin_id: "filesystem".into(),
            kind: "local".into(),
            name: "本地".into(),
            capabilities: vec!["write".into()],
        },
        status: "ready".into(),
        asset_count: 1,
        updated_at: String::new(),
        local_cache: None,
        authentication: None,
    }])));
    model.reduce(ShellMessage::StartupSyncFinished { generation: 0, result: Ok(()) });
    model.reduce(ShellMessage::RepositorySnapshotLoaded(Ok(RepositorySnapshot {
        repository: RepositorySummary {
            repo_id: "repo".into(),
            name: "库".into(),
            path: "C:/repo".into(),
            backend: RepositoryBackendSummary {
                plugin_id: "filesystem".into(),
                kind: "local".into(),
                name: "本地".into(),
                capabilities: vec!["write".into()],
            },
            status: "ready".into(),
            asset_count: 1,
            updated_at: String::new(),
            local_cache: None,
            authentication: None,
        },
        folder_label: String::new(),
        folders: Vec::new(),
        assets: Vec::new(),
        playlists: Vec::new(),
        quick_access: Vec::new(),
        tag_groups: Vec::new(),
        metadata_fields: Vec::new(),
        recent_revision_count: 0,
        overview: RepositoryOverview {
            total_size_bytes: 0,
            total_size_label: "0 B".into(),
            file_count: 0,
            folder_count: 0,
            trash_count: 0,
            readme_content: None,
        },
    })));
    model.reduce(ShellMessage::FileBrowserLoaded(Ok(FileBrowserSnapshot {
        repo_id: "repo".into(),
        root_path: "C:/repo".into(),
        backend_plugin_id: "filesystem".into(),
        backend_kind: "local".into(),
        cache_state: RepositoryStructureCacheState::Ready,
        indexed_at: None,
        current_path: String::new(),
        total_entries: 0,
        loaded_count: 0,
        next_offset: None,
        has_more: false,
        special_location: None,
        tree: None,
        entries: Vec::new(),
    })));
    model.workspace.panel = WorkspacePanel::Search;
    assert!(model.inspect_surface_visible());
    model.workspace.panel = WorkspacePanel::Files;
    assert!(!model.inspect_surface_visible());
    model.reduce(ShellMessage::SelectFile { path: "pics/a.png".into(), asset_id: None });
    assert!(model.inspect_surface_visible());
    let selected = ShellViewModel::for_page(ShellPage::SelectedFile);
    assert!(selected.files_surface_visible());
    assert!(selected.inspect_surface_visible());
}

#[test]
fn metadata_autosave_waits_260ms_and_flushes_before_the_next_file() {
    let mut state = InspectState::default();
    state.note_detail(&asset("pics/a.png", "png", 1, false, Vec::new()));
    state.take_effects();
    state.reduce(true, Some("repo"), InspectMessage::SetComment("note".into()));
    assert!(state.effects.is_empty());
    assert!(!state.poll_own(259, true, Some("repo")));
    assert!(state.poll_own(1, true, Some("repo")));
    assert!(matches!(state.take_effects().as_slice(), [InspectEffect::SaveMetadata { .. }]));

    let mut next = InspectState::default();
    next.note_detail(&asset("pics/b.png", "png", 1, false, Vec::new()));
    next.take_effects();
    next.reduce(true, Some("repo"), InspectMessage::SetComment("later".into()));
    next.begin_selection("pics/c.png");
    assert!(matches!(next.take_effects().as_slice(), [InspectEffect::SaveMetadata { .. }]));
}

#[test]
fn avi_preview_plays_pauses_and_seeks_on_the_shared_session() {
    assert!(!crate::shell::player::sound_device_compiled_in());
    let parts = super::support::preview_media_parts("repo", &super::support::sample_uncompressed()).expect("avi");
    assert!(parts.frames.as_ref().is_some_and(|frames| frames.len() == 2));
    assert!(parts.pcm.is_some());
    let mut model = ShellViewModel::default();
    model.workspace.active_repo_id = Some("repo".into());
    model.reduce(ShellMessage::AssetDetailLoaded(Ok(asset("clips/a.avi", "avi", 1, false, Vec::new()))));
    let InspectEffect::LoadMedia { path, generation, .. } = model.inspect.take_effects().pop().unwrap() else {
        panic!("没有音视频请求");
    };
    model.reduce(ShellMessage::Inspect(InspectMessage::MediaLoaded {
        path,
        generation,
        result: Ok(parts.session),
        pcm: parts.pcm,
        frames: parts.frames,
    }));
    assert_eq!(model.player.session.status, "paused");
    assert!(model.preview_token.as_deref().unwrap_or_default().starts_with("video:"));
    assert_eq!(model.preview_pixels.as_ref().expect("画面").rgba[0], 255);
    model.reduce(ShellMessage::Inspect(InspectMessage::PlayPause));
    assert_eq!(model.player.session.status, "playing");
    assert_eq!(model.inspect.media_session().expect("会话").status, "playing");
    assert!(super::poll_timers(&mut model));
    assert_eq!(model.player.session.current_time_ms, 16);
    assert_eq!(model.inspect.media_session().expect("会话").current_time_ms, 16);
    model.reduce(ShellMessage::Inspect(InspectMessage::Seek(500)));
    assert_eq!(model.player.session.current_time_ms, 500);
    assert_eq!(model.inspect.media_session().expect("会话").current_time_ms, 500);
    assert_eq!(model.preview_pixels.as_ref().expect("第二帧").rgba[2], 255);
    model.reduce(ShellMessage::Inspect(InspectMessage::PlayPause));
    assert_eq!(model.player.session.status, "paused");
    assert_eq!(model.inspect.media_session().expect("会话").status, "paused");
}
