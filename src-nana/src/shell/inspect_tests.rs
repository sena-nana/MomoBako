//! 预览、元数据和搜索状态机测试。
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
use super::{
    AdvancedField, FilterList, InspectEffect, InspectMessage, InspectState, MatchMode, PreviewBinding, PreviewBody,
    PreviewKind, SearchRow, SortDirection,
};

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

fn send(model: &mut ShellViewModel, message: InspectMessage) {
    model.reduce(ShellMessage::Inspect(message));
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

fn row(asset_id: &str, path: &str) -> SearchRow {
    SearchRow {
        repo_id: "repo-hit".into(),
        asset_id: asset_id.into(),
        path: path.into(),
        filename: "cover.png".into(),
        repo_name: "图库".into(),
    }
}

#[test]
fn extension_dispatch_prefers_markdown_and_native_preview() {
    assert_eq!(super::support::classify("PNG", &[]), PreviewKind::Image);
    assert_eq!(super::support::classify("md", &[]), PreviewKind::Markdown);
    assert_eq!(super::support::classify("txt", &[]), PreviewKind::Text);
    assert_eq!(super::support::classify("vue", &[]), PreviewKind::Text);
    assert_eq!(super::support::classify("mp3", &[]), PreviewKind::Media);
    assert_eq!(super::support::classify("pdf", &[]), PreviewKind::Upgrade);
    assert_eq!(super::support::classify("glb", &[]), PreviewKind::Upgrade);
    assert_eq!(super::support::classify("zip", &[]), PreviewKind::Upgrade);
    assert_eq!(super::support::classify("bin", &[]), PreviewKind::Unsupported);

    let native = preview_binding(NativeContributionKind::Preview, "pdf");
    assert_eq!(
        super::support::classify("pdf", &[native]),
        PreviewKind::Native { view_id: "view-pdf".into(), label: "官方预览".into() }
    );
    let playlist = preview_binding(NativeContributionKind::PlaylistPlayer, "pdf");
    assert_eq!(super::support::classify("pdf", &[playlist]), PreviewKind::Upgrade);
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
    assert!(matches!(state.body, PreviewBody::Upgrade(ref message) if message.contains("升级")));
    assert!(state.effects.is_empty());
    state.note_detail(&asset("docs/a.pdf", "pdf", 1, false, Vec::new()));
    assert!(matches!(
        state.take_effects().pop(),
        Some(InspectEffect::LoadNative { view_id, .. }) if view_id == "momobako.preview.pdf"
    ));

    state.reduce(true, Some("repo"), InspectMessage::RegisterPreview(preview_binding(NativeContributionKind::Preview, "pdf")));
    state.note_detail(&asset("docs/a.pdf", "pdf", 1, false, Vec::new()));
    assert!(matches!(state.body, PreviewBody::Native { ref view_id, .. } if view_id == "view-pdf"));

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
    state.reduce(true, Some("repo"), InspectMessage::MediaLoaded { path, generation, result: Ok(session) });
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
fn search_criteria_follow_the_vue_filter_rules() {
    let mut state = InspectState::default();
    state.results = vec![row("old", "old.png")];
    state.reduce(true, Some("repo"), InspectMessage::RunSearch);
    assert!(state.results.is_empty());
    assert!(state.effects.is_empty());

    state.reduce(true, Some("repo"), InspectMessage::SetQuery("peach".into()));
    state.reduce(true, Some("repo"), InspectMessage::RunSearch);
    let InspectEffect::Search { request, .. } = state.take_effects().pop().unwrap() else { panic!("没有搜索") };
    assert_eq!(request.query, "peach");
    assert!(request.repo_id.is_none());
    assert!(request.match_mode.is_none());

    state.reduce(true, Some("repo"), InspectMessage::ToggleFilter { key: FilterList::Colors, value: " red ".into() });
    state.reduce(true, Some("repo"), InspectMessage::ToggleFilter { key: FilterList::Shapes, value: "square".into() });
    state.reduce(true, Some("repo"), InspectMessage::SetMatchMode(MatchMode::Or));
    let InspectEffect::Search { request, .. } = state.take_effects().pop().unwrap() else { panic!("没有筛选搜索") };
    assert!(request.metadata_filters.iter().any(|(key, value)| key == "color" && value == "red"));
    assert!(request.metadata_filters.iter().any(|(key, value)| key == "shape" && value == "square"));
    assert_eq!(request.match_mode.as_deref(), Some("or"));
    assert_eq!(request.repo_id.as_deref(), Some("repo"));

    state.results = vec![row("kept", "kept.png")];
    state.search_generation = 4;
    state.reduce(true, None, InspectMessage::SearchFinished { generation: 3, result: Ok(Vec::new()) });
    assert_eq!(state.results.len(), 1);
    state.reduce(true, None, InspectMessage::SearchFinished { generation: 4, result: Err("搜索失败".into()) });
    assert_eq!(state.results.len(), 1);
    assert_eq!(state.search_error, "搜索失败");

    let mut missing = InspectState::default();
    missing.results = vec![row("kept", "kept.png")];
    missing.reduce(true, None, InspectMessage::ToggleFilter { key: FilterList::Tags, value: "peach".into() });
    assert!(missing.results.is_empty());
    assert!(missing.effects.is_empty());

    let mut locked = InspectState::default();
    locked.reduce(false, Some("repo"), InspectMessage::ToggleFilter { key: FilterList::Tags, value: "peach".into() });
    assert!(locked.filters.tags.is_empty());
    assert!(locked.effects.is_empty());
}

#[test]
fn filter_bar_clear_and_parsers_keep_query_and_drop_empty_bounds() {
    let mut state = InspectState::default();
    state.reduce(true, Some("repo"), InspectMessage::SetQuery("peach".into()));
    state.reduce(true, Some("repo"), InspectMessage::ToggleFilterBar);
    state.reduce(true, Some("repo"), InspectMessage::SetFilterInput { key: FilterList::Tags, value: "draft".into() });
    assert!(state.filters.tags.is_empty());
    state.reduce(true, Some("repo"), InspectMessage::SubmitFilterInput);
    assert_eq!(state.filters.tags, ["draft"]);
    state.take_effects();
    state.reduce(true, Some("repo"), InspectMessage::ToggleFilterBar);
    assert!(!state.filter_bar_open);
    assert_eq!(state.filters.tags, ["draft"]);
    state.reduce(true, Some("repo"), InspectMessage::ToggleFilterBar);
    state.reduce(true, Some("repo"), InspectMessage::ClearFilters);
    assert_eq!(state.query, "peach");
    assert!(state.filter_bar_open);
    assert!(state.filters.tags.is_empty());
    let InspectEffect::Search { request, .. } = state.take_effects().pop().unwrap() else { panic!("清空后应重跑查询") };
    assert_eq!(request.query, "peach");
    assert!(request.repo_id.is_none());

    state.reduce(true, Some("repo"), InspectMessage::SetAdvanced { field: AdvancedField::ExcludeQuery, value: "nope".into() });
    state.reduce(true, Some("repo"), InspectMessage::SetAdvanced { field: AdvancedField::ExcludePaths, value: r"a\b,/c/".into() });
    state.reduce(true, Some("repo"), InspectMessage::ApplyAdvanced);
    let InspectEffect::Search { request, .. } = state.take_effects().pop().unwrap() else { panic!("排除条件也应搜索") };
    assert_eq!(request.exclude_query.as_deref(), Some("nope"));
    assert_eq!(request.exclude_path_prefixes, ["a/b", "c"]);
    assert!(request.repo_id.is_none(), "排除关键词和排除路径不把搜索收进当前仓库");

    state.reduce(true, Some("repo"), InspectMessage::SetAdvanced { field: AdvancedField::Metadata, value: "artist=momo, =x, key=".into() });
    state.reduce(true, Some("repo"), InspectMessage::SetAdvanced { field: AdvancedField::Number, value: "rating=1..5,nan=foo..bar,width=..10".into() });
    state.reduce(true, Some("repo"), InspectMessage::SetAdvanced { field: AdvancedField::Date, value: "added=2020-01-01..2021-01-01".into() });
    state.reduce(true, Some("repo"), InspectMessage::SetAdvanced { field: AdvancedField::SortField, value: "name".into() });
    state.reduce(true, Some("repo"), InspectMessage::SetSortDirection(SortDirection::Desc));
    state.reduce(true, Some("repo"), InspectMessage::SetAdvanced { field: AdvancedField::Limit, value: "0".into() });
    state.reduce(true, Some("repo"), InspectMessage::ApplyAdvanced);
    let InspectEffect::Search { request, .. } = state.take_effects().pop().unwrap() else { panic!("没有高级筛选") };
    assert_eq!(request.metadata_filters, [("artist".into(), "momo".into())]);
    assert_eq!(request.number_filters.len(), 2);
    assert_eq!(request.number_filters[0].min.as_deref(), Some("1"));
    assert_eq!(request.number_filters[0].max.as_deref(), Some("5"));
    assert_eq!(request.date_filters[0].from.as_deref(), Some("2020-01-01"));
    assert_eq!(request.sort_field.as_deref(), Some("name"));
    assert_eq!(request.sort_direction.as_deref(), Some("desc"));
    assert!(request.limit.is_none());
    assert_eq!(request.repo_id.as_deref(), Some("repo"));

    state.reduce(true, Some("repo"), InspectMessage::SetMinimumRating(Some(0.0)));
    state.take_effects();
    state.reduce(true, Some("repo"), InspectMessage::SetMinimumRating(Some(1.0)));
    let InspectEffect::Search { request, .. } = state.take_effects().pop().unwrap() else { panic!("没有评分筛选") };
    assert_eq!(request.min_rating, Some(1.0));
    assert_eq!(request.repo_id.as_deref(), Some("repo"));
    state.reduce(false, Some("repo"), InspectMessage::ApplyShortcut {
        metadata: "kind=book".into(),
        sort_field: "added".into(),
        sort_direction: SortDirection::Desc,
    });
    assert_eq!(state.filters.metadata_filters, "artist=momo, =x, key=");
}

#[test]
fn opening_a_hit_loads_that_asset_and_empty_ids_stay_on_search() {
    let mut model = writable_shell();
    model.workspace.panel = WorkspacePanel::Search;
    model.inspect.results = vec![row("asset-1", "pics/cover.png"), row("", "pics/empty.png")];
    send(&mut model, InspectMessage::OpenHit("".into()));
    assert_eq!(model.workspace.panel, WorkspacePanel::Search);
    assert_eq!(model.inspect.search_error, "搜索结果没有素材 id");
    assert!(model.inspect.effects.is_empty());

    send(&mut model, InspectMessage::OpenHit("asset-1".into()));
    assert_eq!(model.workspace.panel, WorkspacePanel::Files);
    assert_eq!(model.selected_path.as_deref(), Some("pics/cover.png"));
    assert_eq!(model.inspect.repo_id.as_deref(), Some("repo-hit"));
    assert!(matches!(
        model.inspect.effects.last(),
        Some(InspectEffect::LoadAsset { repo_id, asset_id }) if repo_id == "repo-hit" && asset_id == "asset-1"
    ));
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
    acceptance.reduce(ShellMessage::PrimaryAction);
    assert_eq!(acceptance.detail, "该操作的领域服务尚未接通，数据未写入");
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
    assert!(ShellViewModel::for_page(ShellPage::SelectedFile).acceptance_scene);
}
