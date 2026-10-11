//! 预览和元数据状态机测试。搜索与筛选的测试在 `search_tests.rs`。
//!
//! 这些分支来自 Vue 预览分派、元数据编辑和筛选栏。不启动仓库服务，也不写用户目录。

use serde_json::{json, Value};

use crate::backend::services::repository::{
    AssetDetail, AssetSummary, FileBrowserSnapshot, FilePreviewSourceResponse, MetadataEntry, RepositoryBackendSummary,
    RepositoryOverview, RepositorySnapshot, RepositoryStructureCacheState, RepositorySummary,
};
use super::super::workspace::{WorkspacePanel, WorkspaceRepository};
use super::super::player::PlayerMessage;
use super::super::view_part_primary::RouteKey;
use super::super::{PreviewPixels, ShellMessage, ShellPage, ShellViewModel};
use super::super::player::StillShow;
use super::bridge::{MeshData, NativeLoad, PageFrame};
use super::{InspectEffect, InspectMessage, InspectState, PreviewBody, PreviewKind};

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
        source_url: None,
        local_path: None,
        media_type: "image/png".into(),
        size_bytes: 4,
        modified_at: None,
    }
}

/// 分派照 Vue `filePreviewExtensions.ts`：图片、Markdown（先于文本）、文本、音视频，再是内置的 PDF、
/// Office、模型和压缩包（原来由 `office-preview`、`three-model-preview`、`preview-archive` 插件的前端绘制），
/// 认不出的扩展名（例如 psd）照 Vue 没有预览插件时一样不预览。
#[test]
fn extension_dispatch_prefers_markdown_and_native_preview() {
    assert_eq!(super::support::classify("PNG"), PreviewKind::Image);
    assert_eq!(super::support::classify("md"), PreviewKind::Markdown);
    assert_eq!(super::support::classify("txt"), PreviewKind::Text);
    assert_eq!(super::support::classify("vue"), PreviewKind::Text);
    assert_eq!(super::support::classify("mp3"), PreviewKind::Media);
    assert_eq!(
        super::support::classify("pdf"),
        PreviewKind::Native { view_id: "momobako.preview.pdf".into(), label: "PDF".into() }
    );
    assert_eq!(
        super::support::classify("glb"),
        PreviewKind::Native { view_id: "momobako.preview.model".into(), label: "模型".into() }
    );
    assert_eq!(
        super::support::classify("zip"),
        PreviewKind::Native { view_id: "momobako.preview.archive".into(), label: "压缩包".into() }
    );
    assert_eq!(super::support::classify("bin"), PreviewKind::Unsupported);
    assert_eq!(super::support::classify("psd"), PreviewKind::Unsupported);
}

#[test]
fn text_over_the_vue_limit_shows_the_head_and_invalid_utf8_is_lossy() {
    let limit = 768 * 1024;
    let whole = super::prepare_text(&vec![b'a'; limit]);
    assert_eq!((whole.text.len(), whole.truncated_at), (limit, None));
    let head = super::prepare_text(&vec![b'a'; limit + 1]);
    assert_eq!((head.text.len(), head.truncated_at), (limit, Some(limit as u64)));
    assert_eq!(super::prepare_text(&[0xff]).text, "\u{FFFD}");
}

#[test]
fn text_decodes_boms_and_drops_a_character_cut_by_the_limit() {
    assert_eq!(super::prepare_text(&[0xef, 0xbb, 0xbf, b'h', b'i']).text, "hi");
    assert_eq!(super::prepare_text(&[0xff, 0xfe, b'h', 0, b'i', 0]).text, "hi");
    assert_eq!(super::prepare_text(&[0xfe, 0xff, 0, b'h', 0, b'i']).text, "hi");
    let mut bytes = vec![b'a'; 768 * 1024 - 1];
    bytes.extend_from_slice("好".as_bytes());
    let head = super::prepare_text(&bytes);
    assert!(head.text.chars().all(|c| c == 'a'));
    assert_eq!(head.text.len(), 768 * 1024 - 1);
}

#[test]
fn preview_kind_starts_the_matching_body() {
    let mut state = InspectState::default();
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

/// 读不出来的预览不接管播放条：播放条上的播放、跳转和音量都不会把它变成 playing。
#[test]
fn media_without_a_decoder_never_reports_playing() {
    let mut model = ShellViewModel::default();
    model.workspace.active_repo_id = Some("repo".into());
    model.reduce(ShellMessage::AssetDetailLoaded(Ok(asset("audio/a.mp3", "mp3", 1, false, Vec::new()))));
    let InspectEffect::LoadMedia { path, generation, .. } = model.inspect.take_effects().pop().unwrap() else {
        panic!("没有音视频请求");
    };
    model.reduce(ShellMessage::Inspect(InspectMessage::MediaLoaded {
        path,
        generation,
        result: Err("没有原生解码器".into()),
        pcm: None,
        frames: None,
    }));
    let PreviewBody::Media(session) = &model.inspect.body else { panic!("不是音视频") };
    assert_eq!(session.status, "failed");
    assert!(session.error.as_deref().unwrap_or_default().contains("没有原生解码器"));
    assert!(model.player.current_item().is_none());
    assert!(model.inspect.take_effects().is_empty(), "失败的预览不该自动播放");
    for message in [PlayerMessage::SetPlaying(true), PlayerMessage::Seek(1_500), PlayerMessage::SetVolume(0.4)] {
        model.reduce(ShellMessage::Player(message));
        let PreviewBody::Media(session) = &model.inspect.body else { panic!("控制后丢了会话") };
        assert_eq!(session.status, "failed");
    }
    assert!(super::support::preview_media_session("repo", b"ID3").is_err());
}

/// 和 Vue `playEntry` 一样：预览的音频插成临时条目接管播放条，宿主派发自动播放后开始走时钟。
#[test]
fn wav_preview_takes_over_the_bar_and_autoplays_once_dispatched() {
    let mut model = ShellViewModel::default();
    model.workspace.active_repo_id = Some("repo".into());
    model.reduce(ShellMessage::AssetDetailLoaded(Ok(asset("audio/a.wav", "wav", 1, false, Vec::new()))));
    let InspectEffect::LoadMedia { path, generation, .. } = model.inspect.take_effects().pop().unwrap() else {
        panic!("没有音视频请求");
    };
    let session = super::support::preview_media_session("repo", &tone_wav()).expect("wav");
    assert_eq!(session.status, "paused");
    assert_eq!(session.duration_ms, Some(2));
    model.reduce(ShellMessage::Inspect(InspectMessage::MediaLoaded { path, generation, result: Ok(session), pcm: None, frames: None }));
    let item = model.player.current_item().cloned().expect("临时条目");
    assert!(item.transient);
    assert_eq!(item.path, "audio/a.wav");
    assert_eq!(item.player_label, "WAV");
    assert!(model.player.preview_owns_bar());
    assert_eq!(model.player.session.status, "paused", "归约本身不出声");
    let bar = super::super::player_view::bar::props_for_test(&model);
    assert_eq!(bar.title(), "正在播放 a");
    assert_eq!(bar.subtitle(), "WAV · audio/a.wav");
    assert!(bar.can_play && !bar.playing);
    let effects = model.inspect.take_effects();
    let [InspectEffect::Autoplay { path, generation }] = effects.as_slice() else {
        panic!("接管后应排一次自动播放");
    };
    model.reduce(ShellMessage::Inspect(InspectMessage::Autoplay { path: path.clone(), generation: *generation }));
    assert_eq!(model.player.session.status, "playing");
    assert_eq!(model.inspect.media_session().map(|session| session.status.as_str()), Some("playing"));
    assert!(super::super::player_view::bar::props_for_test(&model).playing);
    model.reduce(ShellMessage::Player(PlayerMessage::SetPlaying(false)));
    model.reduce(ShellMessage::Player(PlayerMessage::Seek(1)));
    let PreviewBody::Media(session) = &model.inspect.body else { panic!("丢了会话") };
    assert_eq!(session.status, "paused");
    assert_eq!(session.current_time_ms, 1);
    assert!(session.error.is_none());
    model.reduce(ShellMessage::Inspect(InspectMessage::Autoplay { path: "audio/a.wav".into(), generation: model.inspect.generation + 1 }));
    assert_eq!(model.player.session.status, "paused", "过期的自动播放不动");
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
        result: Ok(super::prepare_text("旧文本".as_bytes())),
    });
    assert!(matches!(state.body, PreviewBody::Empty));
    let generation = state.generation;
    state.reduce(true, Some("repo"), InspectMessage::BodyLoaded {
        path: "notes/b.txt".into(),
        markdown: false,
        generation,
        result: Err("无法读取文件".into()),
    });
    assert!(matches!(state.body, PreviewBody::Failed(ref message) if message.contains("无法读取")));
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
    assert!(state.dirty());
    state.take_effects();
    // 补丁不收空键和编辑器自己的四个输入，别的键按原样写，写完立刻保存（编辑器还没存的改动一起写）。
    let patch = [(" ", json!("x")), ("rating", json!("no")), ("artist", json!("momo"))].map(|(key, value)| (key.to_string(), value));
    state.apply_metadata_patch("asset", patch.into_iter().collect()).expect("补丁写进当前素材");
    state.reduce(true, Some("repo"), InspectMessage::SaveMetadata);
    let InspectEffect::SaveMetadata { repo_id, asset_id, expected_version, metadata } = state.take_effects().pop().unwrap() else {
        panic!("没有保存请求");
    };
    assert!(state.effects.is_empty(), "保存中再要求保存不会再发一次");
    assert_eq!(repo_id, "repo");
    assert_eq!(asset_id, "asset");
    assert_eq!(expected_version, 3);
    assert_eq!(metadata.get("comment"), Some(&json!("hi")));
    assert_eq!(metadata.get("rating"), Some(&json!(4)));
    assert_eq!(metadata.get("tagGroups"), Some(&json!(["kept", "a"])));
    assert_eq!(metadata.get("artist"), Some(&json!("momo")));
    assert!(!metadata.contains_key(" ") && metadata.len() == 4, "{metadata:?}");
    assert!(state.saving);
    assert!(state.apply_metadata_patch("other", Default::default()).is_err(), "补丁只写正在看的素材");

    state.reduce(true, Some("repo"), InspectMessage::MetadataSaved(Err("磁盘错误".into())));
    assert!(!state.saving);
    assert_eq!(state.draft.comment, "hi");
    assert!(state.error.contains("磁盘错误"));
}

/// 只改注释时，保存只写注释和补丁写进来的字段：没动过的评分、标签组和元数据里原有的布尔、数字、数组
/// 都不回写，免得被改存成字符串，或者把旧格式的标签组改写掉。补丁里的数组、数字按原样写。
#[test]
fn saving_leaves_untouched_fields_alone() {
    let mut state = InspectState::default();
    state.note_detail(&asset(
        "voice/a.mp3",
        "mp3",
        2,
        false,
        vec![
            meta("favorite", json!(false)),
            meta("trackDurationMs", json!(125000)),
            meta("voiceActors", json!(["甲", "乙"])),
            meta("circle", json!("社团")),
        ],
    ));
    state.take_effects();
    state.reduce(true, Some("repo"), InspectMessage::SetComment("夜里听".into()));
    let patch = [("circle", json!("新社团")), ("price", json!(1320)), ("scenarioTags", json!(["治愈", "耳语"]))];
    state.apply_metadata_patch("asset", patch.map(|(key, value)| (key.to_string(), value)).into_iter().collect()).expect("补丁");
    let InspectEffect::SaveMetadata { metadata, .. } = state.take_effects().pop().unwrap() else {
        panic!("没有保存请求");
    };
    assert_eq!(metadata.get("comment"), Some(&json!("夜里听")));
    assert_eq!(metadata.get("circle"), Some(&json!("新社团")), "改过的字段照常保存");
    assert_eq!(metadata.get("price"), Some(&json!(1320)), "数字还是数字");
    assert_eq!(metadata.get("scenarioTags"), Some(&json!(["治愈", "耳语"])), "数组还是数组");
    for key in ["rating", "tagGroups", "link", "favorite", "trackDurationMs", "voiceActors"] {
        assert!(!metadata.contains_key(key), "没改的 {key} 不回写：{metadata:?}");
    }
}

/// 旧格式的标签组 `{ label, tags }` 照 Vue `metadataTagGroups` 展平进编辑器（有 tags 取 tags，没有取 label）；
/// 只改评分时标签组不回写，旧数据原样留着；改了标签才写展平后的字符串数组。
#[test]
fn legacy_tag_groups_show_their_tags_and_survive_unrelated_saves() {
    let legacy = json!([{ "label": "角色", "tags": ["主角", " 配角 "] }, { "label": "场景" }, "单独", { "label": "空", "tags": [] }, 7]);
    let mut state = InspectState::default();
    state.note_detail(&asset("pics/a.png", "png", 1, false, vec![meta("tagGroups", legacy)]));
    state.take_effects();
    assert_eq!(state.draft.tags, ["主角", "配角", "场景", "单独", "空"]);
    state.reduce(true, Some("repo"), InspectMessage::SetRating(3));
    state.reduce(true, Some("repo"), InspectMessage::SaveMetadata);
    let InspectEffect::SaveMetadata { metadata, .. } = state.take_effects().pop().unwrap() else {
        panic!("没有保存请求");
    };
    assert_eq!(metadata, [("rating".to_string(), json!(3))].into_iter().collect(), "只写改了的评分");

    let saved = asset("pics/a.png", "png", 2, false, vec![meta("rating", json!(3)), meta("tagGroups", json!([{ "label": "角色", "tags": ["主角"] }]))]);
    state.reduce(true, Some("repo"), InspectMessage::MetadataSaved(Ok(("success".into(), saved))));
    state.reduce(true, Some("repo"), InspectMessage::RemoveTag("主角".into()));
    state.reduce(true, Some("repo"), InspectMessage::AddTag("新".into()));
    state.reduce(true, Some("repo"), InspectMessage::SaveMetadata);
    let InspectEffect::SaveMetadata { metadata, .. } = state.take_effects().pop().unwrap() else {
        panic!("没有保存请求");
    };
    assert_eq!(metadata.get("tagGroups"), Some(&json!(["新"])));
}

/// 换选前写出去的上一个素材的保存结果回来时，不动当前素材：不换回上一个文件的预览和草稿，冲突也不落到
/// 当前素材上（那样「采用服务器版本」会把上一个文件的元数据写进当前文件），只交给状态区报出来。
#[test]
fn save_results_of_the_previous_file_stay_off_the_current_file() {
    let mut model = writable_shell();
    model.reduce(ShellMessage::AssetDetailLoaded(Ok(asset("pics/a.png", "png", 1, false, Vec::new()))));
    model.reduce(ShellMessage::Inspect(InspectMessage::SetComment("给 a".into())));
    model.inspect.take_effects();
    model.reduce(ShellMessage::SelectFile { path: "pics/b.png".into(), asset_id: Some("b".into()) });
    assert!(matches!(model.inspect.take_effects().as_slice(), [InspectEffect::SaveMetadata { asset_id, .. }] if asset_id == "asset"), "换选前写出上一个文件");
    let mut other = asset("pics/b.png", "png", 5, false, vec![meta("comment", json!("b 的注释"))]);
    other.summary.asset_id = "b".into();
    model.reduce(ShellMessage::AssetDetailLoaded(Ok(other)));
    model.inspect.take_effects();
    assert!(!model.inspect.can_edit(), "上一个文件的保存还在路上，同一时刻只发一次");

    let stale = asset("pics/a.png", "png", 9, false, vec![meta("comment", json!("别处写的"))]);
    model.reduce(ShellMessage::Inspect(InspectMessage::MetadataSaved(Ok(("conflict".into(), stale)))));
    assert_eq!(model.inspect.target_path.as_deref(), Some("pics/b.png"), "预览不回到上一个文件");
    assert_eq!(model.inspect.draft_comment(), "b 的注释");
    assert!(model.inspect.conflict.is_empty(), "冲突不落到当前文件");
    assert!(model.inspect.can_edit());
    let failure = model.status.failure().map(|failure| failure.message.clone());
    assert_eq!(failure.as_deref(), Some("保存元数据失败：「a.png」版本冲突，修改没有写入"));
}

/// 保存还在路上时补进草稿的字段，结果回来以后留着并接着保存，不会被这次结果冲掉。
#[test]
fn a_patch_made_while_saving_survives_the_result_and_saves_next() {
    let mut state = InspectState::default();
    state.note_detail(&asset("voice/a.mp3", "mp3", 1, false, Vec::new()));
    state.take_effects();
    state.reduce(true, Some("repo"), InspectMessage::SetComment("先存".into()));
    state.reduce(true, Some("repo"), InspectMessage::SaveMetadata);
    state.take_effects();
    state.apply_metadata_patch("asset", [("circle".to_string(), json!("社团"))].into_iter().collect()).expect("保存中也收补丁");
    assert!(state.take_effects().is_empty(), "保存中不再发第二次");
    let saved = asset("voice/a.mp3", "mp3", 2, false, vec![meta("comment", json!("先存"))]);
    state.reduce(true, Some("repo"), InspectMessage::MetadataSaved(Ok(("success".into(), saved))));
    assert_eq!(state.draft.custom.get("circle"), Some(&json!("社团")), "补丁还在草稿里");
    assert_eq!(state.draft.comment, "先存");
    let InspectEffect::SaveMetadata { expected_version, metadata, .. } = state.take_effects().pop().expect("接着保存补丁") else {
        panic!("没有保存请求");
    };
    assert_eq!(expected_version, 2);
    assert_eq!(metadata, [("circle".to_string(), json!("社团"))].into_iter().collect());
}

/// 回到目录（不再看文件）之前，等着自动保存的改动先写出去，不随预览一起丢掉。
#[test]
fn leaving_the_preview_flushes_the_pending_autosave() {
    let mut state = InspectState::default();
    state.note_detail(&asset("pics/a.png", "png", 1, false, Vec::new()));
    state.take_effects();
    state.reduce(true, Some("repo"), InspectMessage::SetComment("刚打的字".into()));
    assert!(state.timers_pending());
    state.clear();
    assert!(matches!(state.take_effects().as_slice(), [InspectEffect::SaveMetadata { metadata, .. }] if metadata.get("comment") == Some(&json!("刚打的字"))));
    assert!(!state.dirty(), "草稿跟着清掉，不留到下一个文件");
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

/// 文件元数据读回以后有预览目标；打开目录时预览目标跟着清掉。
#[test]
fn opening_a_directory_drops_the_preview_target() {
    let mut live = writable_shell();
    live.reduce(ShellMessage::AssetDetailLoaded(Ok(asset("pics/a.png", "png", 1, false, Vec::new()))));
    assert!(matches!(live.inspect.effects.last(), Some(InspectEffect::LoadImage { .. })));
    assert_eq!(live.inspect.target_path.as_deref(), Some("pics/a.png"));

    live.reduce(ShellMessage::OpenDirectory("folder".into()));
    assert!(live.inspect.target_path.is_none());
}

#[test]
fn search_route_appears_only_after_startup_with_a_repository() {
    let mut model = ShellViewModel::default();
    model.workspace.panel = WorkspacePanel::Search;
    assert_eq!(RouteKey::of(&model), RouteKey::Startup);

    model.reduce(ShellMessage::WorkspaceListLoaded { generation: 0, result: Ok(vec![RepositorySummary {
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
    }]) });
    model.reduce(ShellMessage::RepositorySynced { generation: 0, result: Ok(crate::shell::SyncCounts::default()) });
    model.reduce(ShellMessage::RepositorySnapshotLoaded { generation: 0, result: Ok(RepositorySnapshot {
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
    }) });
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
    assert_eq!(RouteKey::of(&model), RouteKey::Startup, "首屏启动还在等设置包");
    crate::shell::data_load_tests::answer_settings(&mut model);
    model.workspace.panel = WorkspacePanel::Search;
    assert_eq!(RouteKey::of(&model), RouteKey::Search);
    model.workspace.panel = WorkspacePanel::Files;
    assert_eq!(RouteKey::of(&model), RouteKey::Files);
    model.reduce(ShellMessage::SelectFile { path: "pics/a.png".into(), asset_id: None });
    assert_eq!(RouteKey::of(&model), RouteKey::Files);
    let selected = ShellViewModel::for_page(ShellPage::SelectedFile);
    assert_eq!(RouteKey::of(&selected), RouteKey::Files);
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
    model.player.contributions = vec![video_player()];
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
    assert!(crate::shell::wants_preview_texture(&model), "视频画面从纹理槽取，宿主要上传");
    model.reduce(ShellMessage::Player(PlayerMessage::SetPlaying(true)));
    assert_eq!(model.player.session.status, "playing");
    assert_eq!(model.inspect.media_session().expect("会话").status, "playing");
    assert!(super::poll_timers(&mut model, crate::frame_clock::FrameStep::millis(16)));
    assert_eq!(model.player.session.current_time_ms, 16);
    assert_eq!(model.inspect.media_session().expect("会话").current_time_ms, 16);
    model.reduce(ShellMessage::Player(PlayerMessage::Seek(500)));
    assert_eq!(model.player.session.current_time_ms, 500);
    assert_eq!(model.inspect.media_session().expect("会话").current_time_ms, 500);
    assert_eq!(model.preview_pixels.as_ref().expect("第二帧").rgba[2], 255);
    model.reduce(ShellMessage::Player(PlayerMessage::SetPlaying(false)));
    assert_eq!(model.player.session.status, "paused");
    assert_eq!(model.inspect.media_session().expect("会话").status, "paused");
}

/// 没有音轨的视频只走播放时钟：播放条上拖音量只记下音量，画面照样播放，不会因为「不支持音量控制」变成失败。
#[test]
fn volume_on_a_silent_video_is_remembered_without_failing_playback() {
    let parts = super::support::preview_media_parts("repo", &super::support::sample_uncompressed()).expect("avi");
    let mut session = parts.session;
    session.can_volume = false;
    let mut model = ShellViewModel::default();
    model.workspace.active_repo_id = Some("repo".into());
    model.player.contributions = vec![video_player()];
    model.reduce(ShellMessage::AssetDetailLoaded(Ok(asset("clips/silent.avi", "avi", 1, false, Vec::new()))));
    let InspectEffect::LoadMedia { path, generation, .. } = model.inspect.take_effects().pop().unwrap() else {
        panic!("没有音视频请求");
    };
    model.reduce(ShellMessage::Inspect(InspectMessage::MediaLoaded { path, generation, result: Ok(session), pcm: None, frames: parts.frames }));
    model.reduce(ShellMessage::Player(PlayerMessage::SetPlaying(true)));
    assert_eq!(model.player.session.status, "playing");
    model.reduce(ShellMessage::Player(PlayerMessage::SetVolume(0.3)));
    assert_eq!(model.player.session.status, "playing", "调音量不该让没有声音的视频失败");
    assert_eq!(model.player.session.error, None);
    assert_eq!(model.player.session.volume, 0.3, "音量记下，下一项有声音时沿用");
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

/// PDF 和模型的页图从 `deck` 编成内容图画，不复制进 `preview_pixels`，宿主没有东西可传到 `file-preview`。
/// 上一张图片留下的画面在原生预览读好时放掉；翻页和转模型也不再写回。
#[test]
fn native_pages_never_reach_the_preview_texture() {
    let mut model = writable_shell();
    model.reduce(ShellMessage::AssetDetailLoaded(Ok(asset("pics/a.png", "png", 1, false, Vec::new()))));
    model.reduce(ShellMessage::PreviewPixelsLoaded {
        source: source("pics/a.png"),
        pixels: Ok(PreviewPixels { width: 1, height: 1, rgba: vec![1, 2, 3, 4] }),
    });
    assert!(crate::shell::wants_preview_texture(&model), "图片预览从纹理槽取画面");

    model.reduce(ShellMessage::AssetDetailLoaded(Ok(asset("docs/a.pdf", "pdf", 1, false, Vec::new()))));
    let generation = native_generation(&mut model);
    let page = |shade: u8| PageFrame { width: 2, height: 2, rgba: vec![shade; 16], text: String::new(), error: None };
    model.reduce(ShellMessage::Inspect(InspectMessage::NativeLoaded {
        path: "docs/a.pdf".into(),
        generation,
        result: Ok(NativeLoad { text: "PDF".into(), frames: vec![page(10), page(20)], mesh: None, paged: true }),
    }));
    assert!(model.preview_pixels.is_none() && model.preview_token.is_none(), "上一张图片的画面放掉");
    assert_eq!(model.inspect.raster_frame().map(|(_, _, rgba)| rgba[0]), Some(10), "页图从 deck 取");
    assert!(!crate::shell::wants_preview_texture(&model));
    model.reduce(ShellMessage::Inspect(InspectMessage::TurnPage(1)));
    assert_eq!(model.inspect.raster_frame().map(|(_, _, rgba)| rgba[0]), Some(20));
    assert!(model.preview_pixels.is_none() && model.preview_token.is_none(), "翻页不写回预览像素");

    model.reduce(ShellMessage::AssetDetailLoaded(Ok(asset("models/a.obj", "obj", 1, false, Vec::new()))));
    let generation = native_generation(&mut model);
    let mesh = MeshData { vertices: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]], triangles: vec![[0, 1, 2]] };
    model.reduce(ShellMessage::Inspect(InspectMessage::NativeLoaded {
        path: "models/a.obj".into(),
        generation,
        result: Ok(NativeLoad { text: String::new(), frames: Vec::new(), mesh: Some(mesh), paged: false }),
    }));
    model.reduce(ShellMessage::Inspect(InspectMessage::Orbit { yaw: 0.5, zoom: 1.0 }));
    assert!(model.inspect.raster_frame().is_some(), "转过以后画出网格");
    assert!(model.preview_pixels.is_none() && model.preview_token.is_none(), "转模型不写回预览像素");
    assert!(!crate::shell::wants_preview_texture(&model));
}

/// 宿主只在预览框这一帧从 `file-preview` 槽取画面时上传：单击只选中时预览页藏着，不传；预览的正是
/// 幻灯片当前项时画幻灯片自己的帧，也不传。幻灯片本身不登记任何宿主纹理。
#[test]
fn the_preview_texture_is_wanted_only_while_the_frame_draws_it() {
    let mut model = writable_shell();
    model.reduce(ShellMessage::AssetDetailLoaded(Ok(asset("pics/a.png", "png", 1, false, Vec::new()))));
    model.reduce(ShellMessage::PreviewPixelsLoaded {
        source: source("pics/a.png"),
        pixels: Ok(PreviewPixels { width: 1, height: 1, rgba: vec![1, 2, 3, 4] }),
    });
    assert!(crate::shell::wants_preview_texture(&model));

    model.files.select_only = Some("pics/a.png".into());
    assert!(!crate::shell::wants_preview_texture(&model), "单击只选中：预览页藏着");
    model.files.select_only = None;

    model.player.repo_id = Some("repo".into());
    model.player.still = Some(StillShow {
        path: "pics/a.png".into(),
        frame: Some(PreviewPixels { width: 1, height: 1, rgba: vec![9; 4] }),
    });
    assert!(!crate::shell::wants_preview_texture(&model), "幻灯片当前项画自己的帧");
    model.player.still = None;
    assert!(crate::shell::wants_preview_texture(&model));
}
