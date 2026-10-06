//! 播放列表成员、下载、回退、会话和共用播放条的状态机测试。
//!
//! 这些分支来自 Vue 播放列表和播放条。不启动仓库服务，也不写用户目录。

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::backend::services::repository::{
    AssetDetail, AssetSummary, DownloaderDestinationRequest, DownloaderPlaylistProgressEvent, DownloaderPlaylistRequest,
    FileBrowserSnapshot, PlaylistDetail, PlaylistItem, PlaylistMembershipSnapshot, PlaylistPlayerContribution, PlaylistSummary,
    RepositoryBackendSummary, RepositoryOverview, RepositorySnapshot, RepositoryStructureCacheState, RepositorySummary,
};

use super::support::{
    normalize_image_duration_ms, normalize_object_fit, normalize_volume, read_preferences_file, read_sessions_file,
    read_settings_file, resolution_notice, resolve_player, shuffle_order, system_media_session_available, NextStep,
    AUDIO_CAPABILITY, AUDIO_SEQUENCE_TYPE, OFFICIAL_AUDIO_PLUGIN,
};
use super::super::workspace::{LibraryCategory, WorkspacePanel, WorkspaceRepository};
use super::super::{InspectEffect, ShellMessage, ShellPage, ShellViewModel};
use super::{PlaybackMode, PlayerCandidate, PlayerEffect, PlayerMessage, QueueItem};

fn shell(writable: bool) -> ShellViewModel {
    let mut model = ShellViewModel::default();
    model.repository_id = Some("repo".into());
    model.workspace.repositories.push(WorkspaceRepository {
        repo_id: "repo".into(),
        name: "库".into(),
        path: "C:/repo".into(),
        status: "ready".into(),
        backend_plugin_id: "filesystem".into(),
        capabilities: if writable { vec!["write".into()] } else { Vec::new() },
        cache_required: false,
        cache_status: String::new(),
    });
    model.workspace.active_repo_id = Some("repo".into());
    model
}

fn send(model: &mut ShellViewModel, message: PlayerMessage) {
    model.reduce(ShellMessage::Player(message));
}

fn summary(repo_id: &str, playlist_id: &str, player_type_id: &str, file_class: &str) -> PlaylistSummary {
    PlaylistSummary {
        playlist_id: playlist_id.into(),
        repo_id: repo_id.into(),
        name: "早晨".into(),
        player_type_id: player_type_id.into(),
        player_plugin_id: player_type_id.into(),
        player_label: "音频".into(),
        file_class: file_class.into(),
        item_count: 1,
        sort_order: 0,
        created_at: String::new(),
        updated_at: String::new(),
    }
}

fn item(id: &str, status: &str) -> PlaylistItem {
    PlaylistItem {
        playlist_item_id: id.into(),
        playlist_id: "pl".into(),
        asset_id: format!("asset-{id}"),
        path: format!("audio/{id}.mp3"),
        filename: format!("{id}.mp3"),
        extension: "mp3".into(),
        thumbnail_path: None,
        status: status.into(),
        status_reason: None,
        sort_order: 0,
        added_at: String::new(),
        is_virtual: false,
        provider_id: None,
        provider_item_id: None,
        source_payload: None,
        metadata: None,
        local_absolute_path: None,
    }
}

fn detail(repo_id: &str, playlist_id: &str, player_type_id: &str, file_class: &str, items: Vec<PlaylistItem>) -> PlaylistDetail {
    let mut playlist = summary(repo_id, playlist_id, player_type_id, file_class);
    playlist.item_count = items.len() as i64;
    PlaylistDetail { playlist, items }
}

fn candidate(plugin_id: &str, player_type_id: &str, class: &str, extensions: &[&str]) -> PlayerCandidate {
    PlayerCandidate {
        plugin_id: plugin_id.into(),
        player_type_id: player_type_id.into(),
        capability_id: (player_type_id == AUDIO_SEQUENCE_TYPE).then(|| AUDIO_CAPABILITY.to_string()),
        label: plugin_id.into(),
        file_class: class.into(),
        extensions: extensions.iter().map(|item| (*item).to_string()).collect(),
        supports_seek: true,
        supports_volume: true,
    }
}

fn contribution(player_type_id: &str, class: &str, extensions: &[&str]) -> PlaylistPlayerContribution {
    PlaylistPlayerContribution {
        player_type_id: player_type_id.into(),
        label: "Vue 播放器".into(),
        file_class: class.into(),
        supported_extensions: extensions.iter().map(|item| (*item).to_string()).collect(),
        supports_seek: true,
        supports_volume: true,
        supports_preview_navigation: false,
        description: None,
    }
}

fn queue_item(id: &str, class: &str) -> QueueItem {
    QueueItem {
        id: id.into(),
        playlist_id: "pl".into(),
        asset_id: format!("asset-{id}"),
        path: format!("media/{id}.{class}"),
        filename: format!("{id}.{class}"),
        extension: class.into(),
        status: "ready".into(),
        status_reason: None,
        transient: false,
        player_type_id: "audio".into(),
        player_label: "音频".into(),
        file_class: class.into(),
    }
}

fn load_playlist(model: &mut ShellViewModel, repo_id: &str, items: Vec<PlaylistItem>) {
    model.reduce(ShellMessage::PlaylistDetailLoaded(Ok(detail(repo_id, "pl", "audio", "audio", items))));
}

fn temp_dir(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("momobako-nana-player-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("创建临时目录");
    path
}

fn asset(path: &str, extension: &str) -> AssetDetail {
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

fn download_request(playlist_id: i64) -> DownloaderPlaylistRequest {
    DownloaderPlaylistRequest {
        source_repository_id: Some("repo".into()),
        playlist_id,
        playlist_name: Some("早晨".into()),
        tracks: Vec::new(),
        destination: DownloaderDestinationRequest {
            kind: "library".into(),
            path: None,
            repo_id: Some("repo".into()),
            parent_path: None,
        },
        managed_cache_root: None,
        source_payload: None,
        level: None,
    }
}

fn progress(playlist_id: i64, phase: &str, total: usize, completed: usize) -> serde_json::Value {
    serde_json::to_value(DownloaderPlaylistProgressEvent {
        phase: phase.into(),
        playlist_id,
        playlist_name: None,
        total,
        completed,
        failed: 0,
        current_song_id: None,
        current_song_name: Some("歌曲".into()),
        error: None,
    })
    .expect("进度事件")
}

#[test]
fn duration_object_fit_and_corrupt_files_fall_back_to_defaults() {
    assert_eq!(normalize_image_duration_ms(Some(f64::NAN)), 5000);
    assert_eq!(normalize_image_duration_ms(Some(1000.0)), 2000);
    assert_eq!(normalize_image_duration_ms(Some(2500.4)), 2500);
    assert_eq!(normalize_image_duration_ms(Some(40000.0)), 30000);
    assert!(!normalize_object_fit(Some("contain")));
    assert!(normalize_object_fit(Some("cover")));
    assert_eq!(normalize_volume(Some(f64::NAN)), 1.0);
    assert_eq!(normalize_volume(Some(2.0)), 1.0);
    assert_eq!(normalize_volume(Some(-1.0)), 0.0);
    assert!(!system_media_session_available());

    let dir = temp_dir("files");
    let settings = dir.join("settings.json");
    let sessions = dir.join("sessions.json");
    let preferences = dir.join("preferences.json");
    std::fs::write(&settings, "{").unwrap();
    std::fs::write(&sessions, "[1]").unwrap();
    std::fs::write(&preferences, "[]").unwrap();
    let mut player = super::PlayerState::default();
    player.load_files(&settings, &sessions, &preferences);
    assert_eq!(player.settings.image_duration_ms, 5000);
    assert!(!player.settings.object_fit_cover);
    assert!(player.stored.is_empty());
    assert!(player.preferences.is_empty());

    std::fs::write(&sessions, r#"{"repo":{"playlistId":"","currentItemId":"a"},"kept":{"playlistId":"pl","currentItemId":"item","playerTypeId":"audio","currentTimeMs":12,"durationMs":34,"mode":"shuffle","volume":0.25,"isPlaying":true}}"#).unwrap();
    player.load_files(&dir.join("missing.json"), &sessions, &dir.join("missing-prefs.json"));
    assert_eq!(player.stored.len(), 1);
    assert_eq!(player.stored["kept"].mode, PlaybackMode::Shuffle);
    assert_eq!(player.stored["kept"].volume, 0.25);
    assert!(player.stored["kept"].is_playing);

    player.settings.image_duration_ms = 8000;
    player.settings.object_fit_cover = true;
    player.preferences.insert(AUDIO_CAPABILITY.into(), OFFICIAL_AUDIO_PLUGIN.into());
    player.save_settings_file(&settings);
    player.save_sessions_file(&sessions);
    player.save_preferences_file(&preferences);
    assert_eq!(read_settings_file(&settings).image_duration_ms, 8000);
    assert!(read_settings_file(&settings).object_fit_cover);
    assert_eq!(read_sessions_file(&sessions)["kept"].current_item_id, "item");
    assert_eq!(read_preferences_file(&preferences).get(AUDIO_CAPABILITY).map(String::as_str), Some(OFFICIAL_AUDIO_PLUGIN));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn player_resolution_keeps_audio_off_unselected_third_parties() {
    let official = candidate(OFFICIAL_AUDIO_PLUGIN, AUDIO_SEQUENCE_TYPE, "audio", &["mp3"]);
    let third = candidate("other.player", AUDIO_SEQUENCE_TYPE, "audio", &["mp3"]);
    let mut preferences = BTreeMap::new();
    let missing = resolve_player(AUDIO_SEQUENCE_TYPE, &[third.clone()], &preferences);
    assert!(missing.player.is_none());
    assert!(!missing.fallback_used);
    assert_eq!(resolution_notice(&missing), "官方音频播放器未启用或缺失，音频播放暂不可用。");

    preferences.insert(AUDIO_CAPABILITY.into(), "missing.plugin".into());
    let fallback = resolve_player(AUDIO_SEQUENCE_TYPE, &[official.clone(), third.clone()], &preferences);
    assert_eq!(fallback.player.as_ref().map(|player| player.plugin_id.as_str()), Some(OFFICIAL_AUDIO_PLUGIN));
    assert!(fallback.fallback_used);
    assert!(resolution_notice(&fallback).contains("已回退到"));

    preferences.insert(AUDIO_CAPABILITY.into(), "other.player".into());
    let preferred = resolve_player(AUDIO_SEQUENCE_TYPE, &[official, third], &preferences);
    assert_eq!(preferred.player.as_ref().map(|player| player.plugin_id.as_str()), Some("other.player"));
    assert!(!preferred.fallback_used);

    let slides = candidate("slides.player", "slides", "image", &["png"]);
    let chosen = resolve_player("slides", &[slides], &BTreeMap::new());
    assert_eq!(chosen.player.as_ref().map(|player| player.plugin_id.as_str()), Some("slides.player"));

    let mut model = shell(true);
    send(&mut model, PlayerMessage::SetPreference { capability_id: "  ".into(), plugin_id: Some("x".into()) });
    assert!(model.player.preferences.is_empty());
    assert!(model.player.take_effects().is_empty());
    send(&mut model, PlayerMessage::SetPreference { capability_id: AUDIO_CAPABILITY.into(), plugin_id: Some(" other.player ".into()) });
    assert_eq!(model.player.preferences.get(AUDIO_CAPABILITY).map(String::as_str), Some("other.player"));
    send(&mut model, PlayerMessage::SetPreference { capability_id: AUDIO_CAPABILITY.into(), plugin_id: None });
    assert!(model.player.preferences.is_empty());
    assert!(matches!(model.player.take_effects().last(), Some(PlayerEffect::PersistPreferences)));
}

#[test]
fn membership_follows_kind_extension_and_write_permission() {
    let mut model = shell(true);
    model.reduce(ShellMessage::PlaylistPlayersLoaded(Ok(vec![contribution("audio", "audio", &["mp3"])])));
    model.reduce(ShellMessage::PlaylistsLoaded(Ok(vec![summary("repo", "pl", "audio", "audio")])));
    assert!(matches!(model.player.take_effects().last(), Some(PlayerEffect::LoadMemberships { repo_id }) if repo_id == "repo"));

    send(&mut model, PlayerMessage::ToggleMembership {
        playlist_id: "pl".into(),
        kind: "file".into(),
        extension: "mp3".into(),
        asset_id: "asset-1".into(),
        is_virtual: false,
        path: "a.mp3".into(),
    });
    match model.player.take_effects().pop() {
        Some(PlayerEffect::SetMembership(request)) => assert_eq!(request.playlist_ids, ["pl"]),
        other => panic!("期望成员更新，得到 {other:?}"),
    }
    assert!(model.player.memberships.is_empty());
    send(&mut model, PlayerMessage::MembershipSaved(Ok(PlaylistMembershipSnapshot { asset_id: "asset-1".into(), playlist_ids: vec!["pl".into()] })));
    assert_eq!(model.player.memberships.get("asset-1").cloned().unwrap_or_default(), vec!["pl".to_string()]);
    send(&mut model, PlayerMessage::ToggleMembership {
        playlist_id: "pl".into(),
        kind: "file".into(),
        extension: "MP3".into(),
        asset_id: "asset-1".into(),
        is_virtual: false,
        path: "a.mp3".into(),
    });
    match model.player.take_effects().pop() {
        Some(PlayerEffect::SetMembership(request)) => assert!(request.playlist_ids.is_empty()),
        other => panic!("期望移出成员，得到 {other:?}"),
    }

    send(&mut model, PlayerMessage::ToggleMembership {
        playlist_id: "pl".into(),
        kind: "file".into(),
        extension: "mp3".into(),
        asset_id: "virtual".into(),
        is_virtual: true,
        path: "virtual.mp3".into(),
    });
    assert!(matches!(model.player.take_effects().pop(), Some(PlayerEffect::AddByPaths(_))));
    send(&mut model, PlayerMessage::ToggleMembership {
        playlist_id: "pl".into(),
        kind: "directory".into(),
        extension: String::new(),
        asset_id: String::new(),
        is_virtual: false,
        path: "folder".into(),
    });
    assert!(matches!(model.player.take_effects().pop(), Some(PlayerEffect::AddByPaths(_))));

    send(&mut model, PlayerMessage::ToggleMembership {
        playlist_id: "pl".into(),
        kind: "file".into(),
        extension: String::new(),
        asset_id: "asset-1".into(),
        is_virtual: false,
        path: "bare".into(),
    });
    assert!(model.player.take_effects().is_empty());

    let saved = model.player.memberships.clone();
    send(&mut model, PlayerMessage::MembershipSaved(Err("写入失败".into())));
    assert_eq!(model.player.memberships, saved);
    assert_eq!(model.player.activity, "写入失败");

    send(&mut model, PlayerMessage::MembershipsLoaded {
        repo_id: "other".into(),
        result: Ok(BTreeMap::from([("asset-1".into(), vec!["gone".into()])])),
    });
    assert_eq!(model.player.memberships, saved);
    send(&mut model, PlayerMessage::MembershipsLoaded { repo_id: "repo".into(), result: Err("读取失败".into()) });
    assert!(model.player.memberships.is_empty());

    model.player.memberships.insert("asset-1".into(), vec!["pl".into()]);
    model.reduce(ShellMessage::PlaylistsLoaded(Ok(Vec::new())));
    assert!(model.player.memberships.is_empty());
    assert!(model.player.take_effects().is_empty());

    let mut readonly = shell(false);
    readonly.reduce(ShellMessage::PlaylistsLoaded(Ok(vec![summary("repo", "pl", "audio", "audio")])));
    readonly.reduce(ShellMessage::PlaylistPlayersLoaded(Ok(vec![contribution("audio", "audio", &["mp3"])])));
    readonly.player.take_effects();
    send(&mut readonly, PlayerMessage::ToggleMembership {
        playlist_id: "pl".into(),
        kind: "file".into(),
        extension: "mp3".into(),
        asset_id: "asset-1".into(),
        is_virtual: false,
        path: "a.mp3".into(),
    });
    assert!(readonly.player.take_effects().is_empty());
}

#[test]
fn reorder_uses_the_page_order_and_skips_noops() {
    let mut model = shell(true);
    model.selected_playlist_id = Some("pl".into());
    model.playlist_item_ids = vec!["a".into(), "b".into(), "c".into()];
    send(&mut model, PlayerMessage::Reorder { source: "c".into(), before: Some("a".into()) });
    match model.player.take_effects().pop() {
        Some(PlayerEffect::Reorder(request)) => assert_eq!(request.item_ids, ["c", "a", "b"]),
        other => panic!("期望排序请求，得到 {other:?}"),
    }
    assert_eq!(model.playlist_item_ids, ["a", "b", "c"]);
    send(&mut model, PlayerMessage::Reorder { source: "a".into(), before: None });
    match model.player.take_effects().pop() {
        Some(PlayerEffect::Reorder(request)) => assert_eq!(request.item_ids, ["b", "c", "a"]),
        other => panic!("期望移到末尾，得到 {other:?}"),
    }
    send(&mut model, PlayerMessage::Reorder { source: "a".into(), before: Some("b".into()) });
    send(&mut model, PlayerMessage::Reorder { source: "missing".into(), before: None });
    assert!(model.player.take_effects().is_empty());

    model.workspace.repositories[0].capabilities.clear();
    send(&mut model, PlayerMessage::Reorder { source: "c".into(), before: Some("a".into()) });
    assert!(model.player.take_effects().is_empty());
}

#[test]
fn playback_modes_shuffle_and_transients_follow_the_queue() {
    let ready = vec!["a".into(), "b".into(), "c".into()];
    let shuffled = shuffle_order(Some("a"), &ready, 1);
    assert_eq!(shuffled, shuffle_order(Some("a"), &ready, 1));
    assert_eq!(shuffled[0], "a");
    assert_eq!(
        super::support::next_ready_id(PlaybackMode::ListLoop, Some("c"), &ready, &[], false),
        NextStep::Item("a".into())
    );
    assert_eq!(
        super::support::next_ready_id(PlaybackMode::SingleLoop, Some("c"), &ready, &[], true),
        NextStep::Item("c".into())
    );
    assert_eq!(super::support::next_ready_id(PlaybackMode::SingleLoop, Some("c"), &ready, &[], false), NextStep::None);
    assert_eq!(
        super::support::next_ready_id(PlaybackMode::Shuffle, Some("a"), &ready, &[], false),
        NextStep::ReshuffleMissing
    );
    assert_eq!(
        super::support::next_ready_id(PlaybackMode::Shuffle, Some("c"), &ready, &ready, false),
        NextStep::ReshuffleWrap
    );

    let mut model = shell(true);
    model.player.candidates = vec![candidate("native.audio", "audio", "audio", &["mp3"])];
    model.player.queue = vec![queue_item("a", "mp3"), queue_item("b", "mp3"), queue_item("c", "mp3")];
    model.player.current_id = Some("a".into());
    model.player.repo_id = Some("repo".into());
    send(&mut model, PlayerMessage::PlayNext { natural_end: false });
    assert_eq!(model.player.current_id.as_deref(), Some("b"));
    assert_eq!(model.player.session.status, "failed");
    assert_ne!(model.player.session.status, "playing");
    assert!(model.player.wants_playing);
    send(&mut model, PlayerMessage::PlayPrevious);
    assert_eq!(model.player.current_id.as_deref(), Some("a"));

    model.player.current_id = Some("c".into());
    model.player.mode = PlaybackMode::SingleLoop;
    send(&mut model, PlayerMessage::PlayNext { natural_end: true });
    assert_eq!(model.player.current_id.as_deref(), Some("c"));
    send(&mut model, PlayerMessage::PlayNext { natural_end: false });
    assert!(!model.player.wants_playing);
    assert_eq!(model.player.current_id.as_deref(), Some("c"));

    model.player.mode = PlaybackMode::Shuffle;
    model.player.current_id = Some("a".into());
    model.player.shuffle_order.clear();
    model.player.wants_playing = true;
    send(&mut model, PlayerMessage::PlayNext { natural_end: false });
    assert_ne!(model.player.current_id.as_deref(), Some("a"));
    model.player.current_id = Some("a".into());
    model.player.shuffle_order = vec!["a".into()];
    send(&mut model, PlayerMessage::PlayNext { natural_end: false });
    assert_eq!(model.player.current_id.as_deref(), Some("a"));

    model.player.mode = PlaybackMode::Shuffle;
    model.player.history = vec!["a".into(), "b".into(), "c".into()];
    model.player.current_id = Some("c".into());
    send(&mut model, PlayerMessage::PlayPrevious);
    assert_eq!(model.player.current_id.as_deref(), Some("b"));

    let mut transient = queue_item("t", "mp3");
    transient.transient = true;
    model.player.mode = PlaybackMode::ListLoop;
    model.player.queue = vec![queue_item("a", "mp3"), transient];
    model.player.current_id = Some("t".into());
    model.player.history = vec!["a".into(), "t".into()];
    send(&mut model, PlayerMessage::PlayNext { natural_end: true });
    assert_eq!(model.player.current_id.as_deref(), Some("a"));
    assert!(model.player.queue.iter().all(|item| item.id != "t"));
    assert!(model.player.history.iter().all(|id| id != "t"));

    model.player.listed = Some(detail("repo", "pl", "audio", "audio", vec![item("a", "ready")]));
    let mut kept = queue_item("keep", "mp3");
    kept.transient = true;
    let mut dropped = queue_item("drop", "mp3");
    dropped.transient = true;
    model.player.queue = vec![queue_item("a", "mp3"), kept, dropped];
    model.player.current_id = Some("keep".into());
    model.player.history = vec!["keep".into()];
    model.reduce(ShellMessage::PlaylistDetailLoaded(Ok(detail("repo", "pl", "audio", "audio", vec![item("a", "ready")]))));
    let ids: Vec<_> = model.player.queue.iter().map(|item| item.id.as_str()).collect();
    assert_eq!(ids, ["a", "keep"]);
}

#[test]
fn listed_playback_distinguishes_missing_plugin_upgrade_and_decoder_failure() {
    let mut missing = shell(true);
    load_playlist(&mut missing, "repo", vec![item("a", "ready")]);
    send(&mut missing, PlayerMessage::PlayListed { item_id: None });
    assert_eq!(missing.player.session.status, "failed");
    assert!(missing.player.session.error.as_deref().unwrap_or_default().contains("缺少对应播放插件"));
    assert!(!missing.player.can_play);
    assert!(missing.player.stored.contains_key("repo"));

    let mut upgrade = shell(true);
    upgrade.player.candidates.clear();
    upgrade.player.contributions = vec![contribution("audio", "audio", &["mp3"])];
    load_playlist(&mut upgrade, "repo", vec![item("a", "ready")]);
    send(&mut upgrade, PlayerMessage::PlayListed { item_id: None });
    assert!(upgrade.player.session.error.as_deref().unwrap_or_default().contains("需要升级为 Nana 原生播放贡献"));
    assert_ne!(upgrade.player.session.status, "playing");

    let mut decoded = shell(true);
    decoded.player.candidates = vec![candidate("native.audio", "audio", "audio", &["mp3"])];
    load_playlist(&mut decoded, "repo", vec![item("a", "ready")]);
    send(&mut decoded, PlayerMessage::PlayListed { item_id: None });
    assert_eq!(decoded.player.session.status, "failed");
    assert!(decoded.player.session.error.as_deref().unwrap_or_default().contains("没有原生解码器"));
    let volume = decoded.player.session.volume;
    let time = decoded.player.session.current_time_ms;
    send(&mut decoded, PlayerMessage::SetVolume(0.2));
    send(&mut decoded, PlayerMessage::Seek(1_500));
    assert_eq!(decoded.player.session.volume, volume);
    assert_eq!(decoded.player.session.current_time_ms, time);
    assert_ne!(decoded.player.session.status, "playing");

    let mut image = shell(true);
    image.player.candidates = vec![candidate("native.image", "audio", "image", &["png"])];
    let mut picture = item("pic", "ready");
    picture.extension = "png".into();
    picture.filename = "pic.png".into();
    picture.path = "pics/pic.png".into();
    image.reduce(ShellMessage::PlaylistDetailLoaded(Ok(detail("repo", "pl", "audio", "image", vec![picture]))));
    send(&mut image, PlayerMessage::PlayListed { item_id: None });
    assert_eq!(image.player.session.duration_ms, Some(5000));
    image.player.session.current_time_ms = 9_000;
    send(&mut image, PlayerMessage::SetImageDuration(1_000));
    assert_eq!(image.player.settings.image_duration_ms, 2000);
    assert_eq!(image.player.session.duration_ms, Some(2000));
    assert_eq!(image.player.session.current_time_ms, 2000);
    send(&mut image, PlayerMessage::SetObjectFit { cover: true });
    assert_eq!(image.player.object_fit_text(), "填充");
    assert!(matches!(image.player.take_effects().last(), Some(PlayerEffect::PersistSettings)));

    send(&mut decoded, PlayerMessage::CycleMode);
    assert_eq!(decoded.player.mode, PlaybackMode::Shuffle);
    send(&mut decoded, PlayerMessage::CycleMode);
    assert_eq!(decoded.player.mode, PlaybackMode::SingleLoop);
    send(&mut decoded, PlayerMessage::CycleMode);
    assert_eq!(decoded.player.mode, PlaybackMode::ListLoop);
    assert_eq!(decoded.player.mode_text(), "列表循环");
}

#[test]
fn entry_playback_keeps_the_other_repository_session() {
    let mut model = shell(true);
    model.player.candidates = vec![candidate("native.audio", "audio", "audio", &["mp3"])];
    model.player.repo_id = Some("repo-a".into());
    model.player.stored.insert("repo-a".into(), super::support::StoredSession {
        repo_id: "repo-a".into(),
        playlist_id: "pl".into(),
        player_type_id: "audio".into(),
        current_item_id: "a".into(),
        current_time_ms: 10,
        duration_ms: 20,
        mode: PlaybackMode::ListLoop,
        volume: 1.0,
        is_playing: true,
    });
    send(&mut model, PlayerMessage::PlayEntry {
        repo_id: "repo-b".into(),
        kind: "file".into(),
        extension: "mp3".into(),
        asset_id: "asset-b".into(),
        is_virtual: false,
        path: "b.mp3".into(),
        filename: "b.mp3".into(),
    });
    assert!(model.player.stored.contains_key("repo-a"));
    assert!(model.player.current_item().is_some_and(|item| item.transient));
    assert!(model.player.session.error.as_deref().unwrap_or_default().contains("没有原生解码器"));
    assert!(!model.player.take_effects().iter().any(|effect| matches!(effect, PlayerEffect::PersistSessions)));

    let before = model.player.queue.len();
    send(&mut model, PlayerMessage::PlayEntry {
        repo_id: "repo-b".into(),
        kind: "directory".into(),
        extension: String::new(),
        asset_id: String::new(),
        is_virtual: false,
        path: "folder".into(),
        filename: "folder".into(),
    });
    assert_eq!(model.player.queue.len(), before);
    assert_eq!(model.player.activity, "没有可用于播放此媒体的插件");

    model.player.candidates.clear();
    model.player.contributions = vec![contribution("vue-audio", "audio", &["mp3"])];
    send(&mut model, PlayerMessage::PlayEntry {
        repo_id: "repo-b".into(),
        kind: "file".into(),
        extension: String::new(),
        asset_id: "asset-b".into(),
        is_virtual: false,
        path: "nested/song.mp3".into(),
        filename: "song.mp3".into(),
    });
    assert!(model.player.session.error.as_deref().unwrap_or_default().contains("需要升级为 Nana 原生播放贡献"));
}

#[test]
fn switching_repository_clears_only_the_previous_session() {
    let mut model = shell(true);
    model.player.candidates = vec![candidate("native.audio", "audio", "audio", &["mp3"])];
    load_playlist(&mut model, "repo-a", vec![item("a", "ready")]);
    send(&mut model, PlayerMessage::PlayListed { item_id: None });
    assert!(model.player.stored.contains_key("repo-a"));
    model.reduce(ShellMessage::PlaylistDetailLoaded(Ok(detail("repo-b", "pl-b", "audio", "audio", vec![item("b", "ready")]))));
    send(&mut model, PlayerMessage::PlayListed { item_id: None });
    assert!(!model.player.stored.contains_key("repo-a"));
    assert!(model.player.stored.contains_key("repo-b"));
    assert_eq!(model.player.current_id.as_deref(), Some("b"));

    model.player.stored.insert("repo-a".into(), model.player.stored["repo-b"].clone());
    send(&mut model, PlayerMessage::Stop { repo_id: Some("repo-a".into()), clear_stored: true });
    assert!(!model.player.stored.contains_key("repo-a"));
    assert_eq!(model.player.session.status, "failed");
    send(&mut model, PlayerMessage::Stop { repo_id: Some("repo-b".into()), clear_stored: true });
    assert_eq!(model.player.session.status, "ended");
    assert_eq!(model.player.session.current_time_ms, 0);
    assert!(model.player.session.duration_ms.is_none());
    assert!(model.player.session.error.is_none());
    assert!(!model.player.wants_playing);
    assert_eq!(model.player.current_id.as_deref(), Some("b"));
    assert!(!model.player.stored.contains_key("repo-b"));
}

#[test]
fn restore_accepts_a_ready_known_item_and_rejects_the_rest() {
    let dir = temp_dir("restore");
    let sessions = dir.join("sessions.json");
    std::fs::write(
        &sessions,
        r#"{"repo":{"repoId":"repo","playlistId":"pl","playerTypeId":"audio","currentItemId":"a","currentTimeMs":1200,"durationMs":4000,"mode":"singleLoop","volume":0.4,"isPlaying":true}}"#,
    )
    .unwrap();
    let mut model = shell(true);
    model.player.load_files(&dir.join("missing-settings.json"), &sessions, &dir.join("missing-prefs.json"));
    model.player.candidates = vec![candidate("native.audio", "audio", "audio", &["mp3"])];
    model.reduce(ShellMessage::PlaylistsLoaded(Ok(vec![summary("repo", "pl", "audio", "audio")])));
    assert!(matches!(
        model.player.take_effects().iter().find(|effect| matches!(effect, PlayerEffect::RestoreDetail { .. })),
        Some(PlayerEffect::RestoreDetail { playlist_id, .. }) if playlist_id == "pl"
    ));
    send(&mut model, PlayerMessage::RestoreDetail(Ok(detail("repo", "pl", "audio", "audio", vec![item("a", "ready")]))));
    assert_eq!(model.player.current_id.as_deref(), Some("a"));
    assert_eq!(model.player.mode, PlaybackMode::SingleLoop);
    assert_eq!(model.player.session.volume, 0.4);
    assert_eq!(model.player.session.current_time_ms, 1200);
    assert!(model.player.wants_playing);
    assert_eq!(model.player.session.status, "failed");
    assert!(model.player.session.error.as_deref().unwrap_or_default().contains("没有原生解码器"));

    let mut rejected = shell(true);
    rejected.player.stored.insert("repo".into(), model.player.stored["repo"].clone());
    rejected.reduce(ShellMessage::PlaylistsLoaded(Ok(vec![summary("repo", "pl", "audio", "audio")])));
    rejected.player.take_effects();
    send(&mut rejected, PlayerMessage::RestoreDetail(Ok(detail("repo", "pl", "audio", "audio", vec![item("a", "missing")]))));
    assert!(!rejected.player.stored.contains_key("repo"));
    assert!(rejected.player.current_id.is_none());

    let mut mismatch = shell(true);
    mismatch.player.stored.insert("repo".into(), super::support::StoredSession {
        repo_id: "repo".into(),
        playlist_id: "other".into(),
        player_type_id: "audio".into(),
        current_item_id: "a".into(),
        current_time_ms: 0,
        duration_ms: 0,
        mode: PlaybackMode::ListLoop,
        volume: 1.0,
        is_playing: false,
    });
    mismatch.player.restore_playlist_id = Some("pl".into());
    mismatch.player.candidates = vec![candidate("native.audio", "audio", "audio", &["mp3"])];
    send(&mut mismatch, PlayerMessage::RestoreDetail(Ok(detail("repo", "pl", "audio", "audio", vec![item("a", "ready")]))));
    assert!(!mismatch.player.stored.contains_key("repo"));

    model.player.repo_id = Some("repo".into());
    model.reduce(ShellMessage::PlaylistsLoaded(Ok(vec![summary("repo", "pl", "audio", "audio")])));
    assert!(!model.player.take_effects().iter().any(|effect| matches!(effect, PlayerEffect::RestoreDetail { .. })));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn download_applies_returned_events_and_ignores_other_playlists() {
    let mut model = shell(true);
    send(&mut model, PlayerMessage::StartDownload(download_request(7)));
    assert_eq!(model.player.download.phase, "submitting");
    assert!(matches!(model.player.take_effects().pop(), Some(PlayerEffect::Download(_))));
    send(&mut model, PlayerMessage::DownloadCompleted(Ok((serde_json::Value::Null, vec![
        progress(7, "start", 3, 0),
        progress(9, "track", 9, 9),
        progress(7, "track", 3, 1),
    ]))));
    assert_eq!(model.player.download.phase, "track");
    assert_eq!(model.player.download.total, 3);
    assert_eq!(model.player.download.completed, 1);
    assert!(model.player.download_text().contains("正在下载"));

    send(&mut model, PlayerMessage::StartDownload(download_request(7)));
    send(&mut model, PlayerMessage::DownloadCompleted(Ok((serde_json::Value::Null, Vec::new()))));
    assert_eq!(model.player.download.phase, "complete");
    send(&mut model, PlayerMessage::DownloadCompleted(Err("下载失败".into())));
    assert_eq!(model.player.download.phase, "error");
    assert_eq!(model.player.activity, "下载失败");
    send(&mut model, PlayerMessage::CancelDownload);
    assert!(model.player.activity.contains("还没有可取消的句柄"));
    send(&mut model, PlayerMessage::StartDownload(download_request(7)));
    send(&mut model, PlayerMessage::DownloadProgress(serde_json::from_value(progress(7, "track", 4, 1)).expect("进度")));
    assert!(model.player.activity.contains("正在下载 1 / 4，失败"));
    send(&mut model, PlayerMessage::NoteDownloadTask("task-9".into()));
    send(&mut model, PlayerMessage::CancelDownload);
    assert_eq!(model.player.activity, "正在取消下载…");
    assert!(matches!(model.player.take_effects().pop(), Some(PlayerEffect::CancelDownload { task_id }) if task_id == "task-9"));
}

#[test]
fn preview_and_player_share_one_media_session() {
    let mut model = shell(true);
    model.player.session.volume = 0.3;
    model.reduce(ShellMessage::AssetDetailLoaded(Ok(asset("pics/a.png", "png"))));
    assert_eq!(model.player.session.volume, 0.3);
    assert!(model.inspect.media_session().is_none());

    model.reduce(ShellMessage::AssetDetailLoaded(Ok(asset("audio/a.mp3", "mp3"))));
    assert_eq!(model.player.session.status, "failed");
    assert!(model.player.session.error.as_deref().unwrap_or_default().contains("没有原生解码器"));
    assert_eq!(model.inspect.media_session().map(|session| session.status.as_str()), Some("failed"));
    send(&mut model, PlayerMessage::SetVolume(0.2));
    assert_eq!(model.player.session.volume, 1.0);
    assert_eq!(model.inspect.media_session().map(|session| session.volume), Some(1.0));
    assert_ne!(model.player.session.status, "playing");
    send(&mut model, PlayerMessage::Stop { repo_id: Some("repo".into()), clear_stored: true });
    assert_eq!(model.player.session.status, "ended");
    assert_eq!(model.inspect.media_session().map(|session| session.status.as_str()), Some("ended"));

    model.player.candidates = vec![candidate("native.audio", "audio", "audio", &["mp3"])];
    load_playlist(&mut model, "repo", vec![item("a", "ready")]);
    send(&mut model, PlayerMessage::PlayListed { item_id: None });
    send(&mut model, PlayerMessage::OpenPreview);
    assert_eq!(model.workspace.panel, WorkspacePanel::Files);
    assert_eq!(model.workspace.library_category, LibraryCategory::All);
    assert_eq!(model.selected_path.as_deref(), Some("audio/a.mp3"));
    assert!(matches!(
        model.inspect.take_effects().pop(),
        Some(InspectEffect::LoadAsset { asset_id, .. }) if asset_id == "asset-a"
    ));
    assert!(!model.player.system_media_supported());
}

#[test]
fn live_player_surface_stays_off_acceptance_scenes() {
    assert!(!ShellViewModel::for_page(ShellPage::PlaybackRunning).player_surface_visible());
    assert!(!ShellViewModel::for_page(ShellPage::Playlists).player_surface_visible());
    assert!(!shell(true).player_surface_visible());

    let mut model = ShellViewModel::default();
    let repository = RepositorySummary {
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
    };
    model.reduce(ShellMessage::RepositoriesLoaded(Ok(vec![repository.clone()])));
    model.reduce(ShellMessage::StartupSyncFinished { generation: 0, result: Ok(()) });
    model.reduce(ShellMessage::RepositorySnapshotLoaded(Ok(RepositorySnapshot {
        repository,
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
    assert!(model.player_surface_visible());
    model.workspace.panel = WorkspacePanel::Playlist;
    assert!(model.player_surface_visible());
    model.workspace.panel = WorkspacePanel::Trash;
    assert!(!model.player_surface_visible());

    let mut stale = shell(true);
    stale.sidebar.bind_repository(Some("repo-a"), false);
    stale.reduce(ShellMessage::PlaylistDetailLoaded(Ok(detail("repo-b", "pl", "audio", "audio", vec![item("a", "ready")]))));
    assert!(stale.player.listed.is_none());
}

/// 8-bit 单声道 PCM。`data` 里每个字节是一个样本。
fn pcm_wav(sample_rate: u32, channels: u16, bits: u16, data: &[u8]) -> Vec<u8> {
    let block_align = channels * (bits / 8);
    let byte_rate = sample_rate * u32::from(block_align);
    let mut body = Vec::new();
    body.extend_from_slice(b"fmt ");
    body.extend_from_slice(&16u32.to_le_bytes());
    body.extend_from_slice(&1u16.to_le_bytes());
    body.extend_from_slice(&channels.to_le_bytes());
    body.extend_from_slice(&sample_rate.to_le_bytes());
    body.extend_from_slice(&byte_rate.to_le_bytes());
    body.extend_from_slice(&block_align.to_le_bytes());
    body.extend_from_slice(&bits.to_le_bytes());
    body.extend_from_slice(b"data");
    body.extend_from_slice(&(data.len() as u32).to_le_bytes());
    body.extend_from_slice(data);
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&((body.len() + 4) as u32).to_le_bytes());
    bytes.extend_from_slice(b"WAVE");
    bytes.extend_from_slice(&body);
    bytes
}

fn wav_queue_item(id: &str, path: &std::path::Path) -> QueueItem {
    let mut item = queue_item(id, "wav");
    item.path = path.display().to_string();
    item.filename = format!("{id}.wav");
    item.extension = "wav".into();
    item.player_type_id = "momobako.playlist.wav".into();
    item.file_class = "audio".into();
    item
}

#[test]
fn wav_session_loads_paused_then_plays_seeks_and_pauses() {
    let dir = temp_dir("wav");
    let path = dir.join("tone.wav");
    let samples = [128u8; 16];
    std::fs::write(&path, pcm_wav(8_000, 1, 8, &samples)).unwrap();
    let expected_ms = (samples.len() as u64) * 1000 / 8_000;

    let mut model = shell(true);
    assert_eq!(model.player.candidates.len(), 1);
    let builtin = &model.player.candidates[0];
    assert_eq!(builtin.plugin_id, "momobako.player.wav");
    assert_eq!(builtin.player_type_id, "momobako.playlist.wav");
    assert_eq!(builtin.label, "WAV");
    assert_eq!(builtin.file_class, "audio");
    assert_eq!(builtin.extensions, ["wav"]);
    assert!(builtin.supports_seek && builtin.supports_volume);

    model.player.repo_id = Some("repo".into());
    model.player.queue = vec![wav_queue_item("tone", &path)];
    send(&mut model, PlayerMessage::PlayItem { item_id: "tone".into() });
    assert_eq!(model.player.session.status, "paused");
    assert_eq!(model.player.session.duration_ms, Some(expected_ms));
    assert!(model.player.session.can_seek);
    assert!(model.player.session.can_volume);
    assert!(model.player.session.error.is_none());
    assert_ne!(model.player.session.status, "playing");

    send(&mut model, PlayerMessage::SetPlaying(true));
    assert_eq!(model.player.session.status, "playing");
    let middle = expected_ms / 2;
    send(&mut model, PlayerMessage::Seek(middle));
    assert_eq!(model.player.session.current_time_ms, middle);
    assert_eq!(model.player.session.status, "playing");
    send(&mut model, PlayerMessage::SetPlaying(false));
    assert_eq!(model.player.session.status, "paused");
    assert_eq!(model.player.session.current_time_ms, middle);
    send(&mut model, PlayerMessage::SetVolume(0.25));
    assert_eq!(model.player.session.volume, 0.25);
    assert_eq!(model.player.session.status, "paused");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn wav_player_rejects_a_non_wav_header() {
    let dir = temp_dir("wav-bad");
    let path = dir.join("notes.bin");
    std::fs::write(&path, b"not-a-wav-header").unwrap();
    let mut model = shell(true);
    model.player.queue = vec![wav_queue_item("bad", &path)];
    send(&mut model, PlayerMessage::PlayItem { item_id: "bad".into() });
    assert_eq!(model.player.session.status, "failed");
    assert!(model.player.session.error.as_deref().unwrap_or_default().contains("不是 WAV 头"));
    assert_ne!(model.player.session.status, "playing");
    let _ = std::fs::remove_dir_all(dir);
}
