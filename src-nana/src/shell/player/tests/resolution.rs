//! 默认值回退、播放器解析、成员资格、排序、播放模式与列表播放失败区分的测试。

use super::*;

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
    let mut player = super::super::PlayerState::default();
    player.load_files(&settings, &sessions, &preferences);
    assert_eq!(player.settings.image_duration_ms, 5000);
    assert!(!player.settings.object_fit_cover);
    assert!(player.stored.is_empty());
    assert!(player.preferences.is_empty());

    std::fs::write(&sessions, r#"{"repo":{"playlistId":"","currentItemId":"a"},"kept":{"playlistId":"pl","currentItemId":"item","playerTypeId":"audio","currentTimeMs":12,"durationMs":34,"mode":"shuffle","volume":0.25,"isPlaying":true}}"#).unwrap();
    player.load_files(
        &dir.join("missing.json"),
        &sessions,
        &dir.join("missing-prefs.json"),
    );
    assert_eq!(player.stored.len(), 1);
    assert_eq!(player.stored["kept"].mode, PlaybackMode::Shuffle);
    assert_eq!(player.stored["kept"].volume, 0.25);
    assert!(player.stored["kept"].is_playing);

    player.settings.image_duration_ms = 8000;
    player.settings.object_fit_cover = true;
    player
        .preferences
        .insert(AUDIO_CAPABILITY.into(), OFFICIAL_AUDIO_PLUGIN.into());
    player.save_settings_file(&settings);
    player.save_sessions_file(&sessions);
    player.save_preferences_file(&preferences);
    assert_eq!(read_settings_file(&settings).image_duration_ms, 8000);
    assert!(read_settings_file(&settings).object_fit_cover);
    assert_eq!(
        read_sessions_file(&sessions)["kept"].current_item_id,
        "item"
    );
    assert_eq!(
        read_preferences_file(&preferences)
            .get(AUDIO_CAPABILITY)
            .map(String::as_str),
        Some(OFFICIAL_AUDIO_PLUGIN)
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn player_resolution_keeps_audio_off_unselected_third_parties() {
    let official = candidate(
        OFFICIAL_AUDIO_PLUGIN,
        AUDIO_SEQUENCE_TYPE,
        "audio",
        &["mp3"],
    );
    let third = candidate("other.player", AUDIO_SEQUENCE_TYPE, "audio", &["mp3"]);
    let mut preferences = BTreeMap::new();
    let missing = resolve_player(AUDIO_SEQUENCE_TYPE, &[third.clone()], &preferences);
    assert!(missing.player.is_none());
    assert!(!missing.fallback_used);
    assert_eq!(
        resolution_notice(&missing),
        "官方音频播放器未启用或缺失，音频播放暂不可用。"
    );

    preferences.insert(AUDIO_CAPABILITY.into(), "missing.plugin".into());
    let fallback = resolve_player(
        AUDIO_SEQUENCE_TYPE,
        &[official.clone(), third.clone()],
        &preferences,
    );
    assert_eq!(
        fallback
            .player
            .as_ref()
            .map(|player| player.plugin_id.as_str()),
        Some(OFFICIAL_AUDIO_PLUGIN)
    );
    assert!(fallback.fallback_used);
    assert!(resolution_notice(&fallback).contains("已回退到"));

    preferences.insert(AUDIO_CAPABILITY.into(), "other.player".into());
    let preferred = resolve_player(AUDIO_SEQUENCE_TYPE, &[official, third], &preferences);
    assert_eq!(
        preferred
            .player
            .as_ref()
            .map(|player| player.plugin_id.as_str()),
        Some("other.player")
    );
    assert!(!preferred.fallback_used);

    let slides = candidate("slides.player", "slides", "image", &["png"]);
    let chosen = resolve_player("slides", &[slides], &BTreeMap::new());
    assert_eq!(
        chosen
            .player
            .as_ref()
            .map(|player| player.plugin_id.as_str()),
        Some("slides.player")
    );

    let mut model = shell(true);
    send(
        &mut model,
        PlayerMessage::SetPreference {
            capability_id: "  ".into(),
            plugin_id: Some("x".into()),
        },
    );
    assert!(model.player.preferences.is_empty());
    assert!(model.player.take_effects().is_empty());
    send(
        &mut model,
        PlayerMessage::SetPreference {
            capability_id: AUDIO_CAPABILITY.into(),
            plugin_id: Some(" other.player ".into()),
        },
    );
    assert_eq!(
        model
            .player
            .preferences
            .get(AUDIO_CAPABILITY)
            .map(String::as_str),
        Some("other.player")
    );
    send(
        &mut model,
        PlayerMessage::SetPreference {
            capability_id: AUDIO_CAPABILITY.into(),
            plugin_id: None,
        },
    );
    assert!(model.player.preferences.is_empty());
    assert!(matches!(
        model.player.take_effects().last(),
        Some(PlayerEffect::PersistPreferences)
    ));
}

#[test]
fn membership_follows_kind_extension_and_write_permission() {
    let mut model = shell(true);
    model.reduce(ShellMessage::PlaylistPlayersLoaded(Ok(vec![contribution(
        "audio",
        "audio",
        &["mp3"],
    )])));
    load_list(&mut model, vec![summary("repo", "pl", "audio", "audio")]);
    assert!(
        matches!(model.player.take_effects().last(), Some(PlayerEffect::LoadMemberships { repo_id }) if repo_id == "repo")
    );

    send(
        &mut model,
        PlayerMessage::ToggleMembership {
            playlist_id: "pl".into(),
            kind: "file".into(),
            extension: "mp3".into(),
            asset_id: "asset-1".into(),
            is_virtual: false,
            path: "a.mp3".into(),
        },
    );
    match model.player.take_effects().pop() {
        Some(PlayerEffect::SetMembership(request)) => assert_eq!(request.playlist_ids, ["pl"]),
        other => panic!("期望成员更新，得到 {other:?}"),
    }
    assert!(model.player.memberships.is_empty());
    send(
        &mut model,
        PlayerMessage::MembershipSaved(Ok(PlaylistMembershipSnapshot {
            asset_id: "asset-1".into(),
            playlist_ids: vec!["pl".into()],
        })),
    );
    assert_eq!(
        model
            .player
            .memberships
            .get("asset-1")
            .cloned()
            .unwrap_or_default(),
        vec!["pl".to_string()]
    );
    send(
        &mut model,
        PlayerMessage::ToggleMembership {
            playlist_id: "pl".into(),
            kind: "file".into(),
            extension: "MP3".into(),
            asset_id: "asset-1".into(),
            is_virtual: false,
            path: "a.mp3".into(),
        },
    );
    match model.player.take_effects().pop() {
        Some(PlayerEffect::SetMembership(request)) => assert!(request.playlist_ids.is_empty()),
        other => panic!("期望移出成员，得到 {other:?}"),
    }

    send(
        &mut model,
        PlayerMessage::ToggleMembership {
            playlist_id: "pl".into(),
            kind: "file".into(),
            extension: "mp3".into(),
            asset_id: "virtual".into(),
            is_virtual: true,
            path: "virtual.mp3".into(),
        },
    );
    assert!(matches!(
        model.player.take_effects().pop(),
        Some(PlayerEffect::AddByPaths(_))
    ));
    send(
        &mut model,
        PlayerMessage::ToggleMembership {
            playlist_id: "pl".into(),
            kind: "directory".into(),
            extension: String::new(),
            asset_id: String::new(),
            is_virtual: false,
            path: "folder".into(),
        },
    );
    assert!(matches!(
        model.player.take_effects().pop(),
        Some(PlayerEffect::AddByPaths(_))
    ));

    send(
        &mut model,
        PlayerMessage::ToggleMembership {
            playlist_id: "pl".into(),
            kind: "file".into(),
            extension: String::new(),
            asset_id: "asset-1".into(),
            is_virtual: false,
            path: "bare".into(),
        },
    );
    assert!(model.player.take_effects().is_empty());

    let saved = model.player.memberships.clone();
    send(
        &mut model,
        PlayerMessage::MembershipSaved(Err("写入失败".into())),
    );
    assert_eq!(model.player.memberships, saved);
    let failure = model.status.failure().expect("成员写入失败要进状态区");
    assert_eq!(failure.message, "更新播放集成员失败：写入失败");

    send(
        &mut model,
        PlayerMessage::MembershipsLoaded {
            repo_id: "other".into(),
            result: Ok(BTreeMap::from([("asset-1".into(), vec!["gone".into()])])),
        },
    );
    assert_eq!(model.player.memberships, saved);
    send(
        &mut model,
        PlayerMessage::MembershipsLoaded {
            repo_id: "repo".into(),
            result: Err("读取失败".into()),
        },
    );
    assert!(model.player.memberships.is_empty());

    model
        .player
        .memberships
        .insert("asset-1".into(), vec!["pl".into()]);
    load_list(&mut model, Vec::new());
    assert!(model.player.memberships.is_empty());
    assert!(model.player.take_effects().is_empty());

    let mut readonly = shell(false);
    load_list(&mut readonly, vec![summary("repo", "pl", "audio", "audio")]);
    readonly.reduce(ShellMessage::PlaylistPlayersLoaded(Ok(vec![contribution(
        "audio",
        "audio",
        &["mp3"],
    )])));
    readonly.player.take_effects();
    send(
        &mut readonly,
        PlayerMessage::ToggleMembership {
            playlist_id: "pl".into(),
            kind: "file".into(),
            extension: "mp3".into(),
            asset_id: "asset-1".into(),
            is_virtual: false,
            path: "a.mp3".into(),
        },
    );
    assert!(readonly.player.take_effects().is_empty());
}

#[test]
fn reorder_uses_the_page_order_and_skips_noops() {
    let mut model = shell(true);
    model.selected_playlist_id = Some("pl".into());
    model.playlist_item_ids = vec!["a".into(), "b".into(), "c".into()];
    send(
        &mut model,
        PlayerMessage::Reorder {
            source: "c".into(),
            before: Some("a".into()),
        },
    );
    match model.player.take_effects().pop() {
        Some(PlayerEffect::Reorder(request)) => assert_eq!(request.item_ids, ["c", "a", "b"]),
        other => panic!("期望排序请求，得到 {other:?}"),
    }
    assert_eq!(model.playlist_item_ids, ["a", "b", "c"]);
    send(
        &mut model,
        PlayerMessage::Reorder {
            source: "a".into(),
            before: None,
        },
    );
    match model.player.take_effects().pop() {
        Some(PlayerEffect::Reorder(request)) => assert_eq!(request.item_ids, ["b", "c", "a"]),
        other => panic!("期望移到末尾，得到 {other:?}"),
    }
    send(
        &mut model,
        PlayerMessage::Reorder {
            source: "a".into(),
            before: Some("b".into()),
        },
    );
    send(
        &mut model,
        PlayerMessage::Reorder {
            source: "missing".into(),
            before: None,
        },
    );
    assert!(model.player.take_effects().is_empty());

    model.workspace.repositories[0].capabilities.clear();
    send(
        &mut model,
        PlayerMessage::Reorder {
            source: "c".into(),
            before: Some("a".into()),
        },
    );
    assert!(model.player.take_effects().is_empty());
}

#[test]
fn playback_modes_shuffle_and_transients_follow_the_queue() {
    let ready = vec!["a".into(), "b".into(), "c".into()];
    let shuffled = shuffle_order(Some("a"), &ready, 1);
    assert_eq!(shuffled, shuffle_order(Some("a"), &ready, 1));
    assert_eq!(shuffled[0], "a");
    assert_eq!(
        super::super::support::next_ready_id(PlaybackMode::ListLoop, Some("c"), &ready, &[], false),
        NextStep::Item("a".into())
    );
    assert_eq!(
        super::super::support::next_ready_id(
            PlaybackMode::SingleLoop,
            Some("c"),
            &ready,
            &[],
            true
        ),
        NextStep::Item("c".into())
    );
    assert_eq!(
        super::super::support::next_ready_id(
            PlaybackMode::SingleLoop,
            Some("c"),
            &ready,
            &[],
            false
        ),
        NextStep::None
    );
    assert_eq!(
        super::super::support::next_ready_id(PlaybackMode::Shuffle, Some("a"), &ready, &[], false),
        NextStep::ReshuffleMissing
    );
    assert_eq!(
        super::super::support::next_ready_id(
            PlaybackMode::Shuffle,
            Some("c"),
            &ready,
            &ready,
            false
        ),
        NextStep::ReshuffleWrap
    );

    let mut model = shell(true);
    model.player.candidates = vec![candidate("native.audio", "audio", "audio", &["mp3"])];
    model.player.queue = vec![
        queue_item("a", "mp3"),
        queue_item("b", "mp3"),
        queue_item("c", "mp3"),
    ];
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

    model.player.listed = Some(detail(
        "repo",
        "pl",
        "audio",
        "audio",
        vec![item("a", "ready")],
    ));
    let mut kept = queue_item("keep", "mp3");
    kept.transient = true;
    let mut dropped = queue_item("drop", "mp3");
    dropped.transient = true;
    model.player.queue = vec![queue_item("a", "mp3"), kept, dropped];
    model.player.current_id = Some("keep".into());
    model.player.history = vec!["keep".into()];
    model.reduce(ShellMessage::PlaylistDetailLoaded(Ok(detail(
        "repo",
        "pl",
        "audio",
        "audio",
        vec![item("a", "ready")],
    ))));
    let ids: Vec<_> = model
        .player
        .queue
        .iter()
        .map(|item| item.id.as_str())
        .collect();
    assert_eq!(ids, ["a", "keep"]);
}

#[test]
fn listed_playback_distinguishes_missing_plugin_upgrade_and_decoder_failure() {
    let mut missing = shell(true);
    load_playlist(&mut missing, "repo", vec![item("a", "ready")]);
    send(&mut missing, PlayerMessage::PlayListed { item_id: None });
    assert_eq!(missing.player.session.status, "failed");
    assert!(
        missing
            .player
            .session
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("缺少对应播放插件")
    );
    assert!(!missing.player.can_play);
    assert!(missing.player.stored.contains_key("repo"));

    let mut upgrade = shell(true);
    upgrade.player.candidates.clear();
    upgrade.player.contributions = vec![contribution("audio", "audio", &["mp3"])];
    load_playlist(&mut upgrade, "repo", vec![item("a", "ready")]);
    send(&mut upgrade, PlayerMessage::PlayListed { item_id: None });
    // 条目路径是仓库内相对路径，经仓库服务读取；这里读不到就写明读取失败。
    assert!(
        upgrade
            .player
            .session
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("无法读取当前项")
    );
    assert!(
        !upgrade
            .player
            .session
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("需要升级")
    );
    assert_ne!(upgrade.player.session.status, "playing");

    let mut decoded = shell(true);
    decoded.player.candidates = vec![candidate("native.audio", "audio", "audio", &["mp3"])];
    load_playlist(&mut decoded, "repo", vec![item("a", "ready")]);
    send(&mut decoded, PlayerMessage::PlayListed { item_id: None });
    assert_eq!(decoded.player.session.status, "failed");
    assert!(
        decoded
            .player
            .session
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("没有原生解码器")
    );
    let time = decoded.player.session.current_time_ms;
    // 音量先记下来留给下一项；跳转在没装好的条目上不动，也不改掉原来的错误。
    send(&mut decoded, PlayerMessage::SetVolume(0.2));
    send(&mut decoded, PlayerMessage::Seek(1_500));
    assert_eq!(decoded.player.session.volume, 0.2);
    assert_eq!(decoded.player.session.current_time_ms, time);
    assert!(
        decoded
            .player
            .session
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("没有原生解码器")
    );
    assert_ne!(decoded.player.session.status, "playing");
    // 没装好的条目没有控制落点：暂停不动，播放重新装载、照样报没有原生解码器，不读文件。
    decoded.player.take_effects();
    send(&mut decoded, PlayerMessage::SetPlaying(false));
    send(&mut decoded, PlayerMessage::SetPlaying(true));
    assert_eq!(decoded.player.session.status, "failed");
    assert!(
        decoded
            .player
            .session
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("没有原生解码器")
    );
    assert!(
        !decoded
            .player
            .take_effects()
            .iter()
            .any(|effect| matches!(effect, PlayerEffect::LoadItem { .. })),
        "不读文件"
    );

    let mut image = shell(true);
    image.player.candidates = vec![candidate("native.image", "audio", "image", &["png"])];
    let mut picture = item("pic", "ready");
    picture.extension = "png".into();
    picture.filename = "pic.png".into();
    picture.path = "pics/pic.png".into();
    image.reduce(ShellMessage::PlaylistDetailLoaded(Ok(detail(
        "repo",
        "pl",
        "audio",
        "image",
        vec![picture],
    ))));
    send(&mut image, PlayerMessage::PlayListed { item_id: None });
    assert_eq!(image.player.session.duration_ms, Some(5000));
    image.player.session.current_time_ms = 9_000;
    send(&mut image, PlayerMessage::SetImageDuration(1_000));
    assert_eq!(image.player.settings.image_duration_ms, 2000);
    assert_eq!(image.player.session.duration_ms, Some(2000));
    assert_eq!(image.player.session.current_time_ms, 2000);
    send(&mut image, PlayerMessage::SetObjectFit { cover: true });
    assert_eq!(image.player.object_fit_text(), "填充");
    assert!(matches!(
        image.player.take_effects().last(),
        Some(PlayerEffect::PersistSettings)
    ));

    send(&mut decoded, PlayerMessage::CycleMode);
    assert_eq!(decoded.player.mode, PlaybackMode::Shuffle);
    send(&mut decoded, PlayerMessage::CycleMode);
    assert_eq!(decoded.player.mode, PlaybackMode::SingleLoop);
    send(&mut decoded, PlayerMessage::CycleMode);
    assert_eq!(decoded.player.mode, PlaybackMode::ListLoop);
    assert_eq!(decoded.player.mode_text(), "列表循环");
}
