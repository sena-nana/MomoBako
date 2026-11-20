//! 预览接管、切换仓库、恢复播放、共享媒体会话、播放器页面与侧栏播放的测试。

use super::*;

/// 预览里的音视频接管播放条：播放器正放着别的仓库时先停掉那边的
/// 运行时，但不删那个仓库存下的会话；预览的文件插成临时条目，临时条目不写会话文件。
#[test]
fn preview_takeover_keeps_the_other_repository_session() {
    let mut model = shell(true);
    model.player.repo_id = Some("repo-a".into());
    model.player.stored.insert(
        "repo-a".into(),
        super::super::support::StoredSession {
            repo_id: "repo-a".into(),
            playlist_id: "pl".into(),
            player_type_id: "audio".into(),
            current_item_id: "a".into(),
            current_time_ms: 10,
            duration_ms: 20,
            mode: PlaybackMode::ListLoop,
            volume: 1.0,
            is_playing: true,
        },
    );
    let mut session = super::super::fresh_session("repo-b");
    session.status = "paused".into();
    let entry = super::super::PreviewEntry {
        repo_id: "repo-b",
        path: "b.mp3",
        extension: "mp3",
        asset_id: "asset-b",
        is_virtual: false,
    };
    assert!(
        model
            .player
            .take_over_preview(entry, session, None, &mut model.inspect),
        "有能放 mp3 的内置候选时接管"
    );
    assert!(
        model.player.stored.contains_key("repo-a"),
        "不删别的仓库存下的会话"
    );
    assert_eq!(model.player.repo_id.as_deref(), Some("repo-b"));
    assert!(
        model
            .player
            .current_item()
            .is_some_and(|item| item.transient && item.path == "b.mp3")
    );
    assert!(
        !model
            .player
            .take_effects()
            .iter()
            .any(|effect| matches!(effect, PlayerEffect::PersistSessions)),
        "临时条目不写会话"
    );

    model.player.candidates.clear();
    let entry = super::super::PreviewEntry {
        repo_id: "repo-b",
        path: "c.mp3",
        extension: "mp3",
        asset_id: "asset-c",
        is_virtual: false,
    };
    assert!(
        !model.player.take_over_preview(
            entry,
            super::super::fresh_session("repo-b"),
            None,
            &mut model.inspect
        ),
        "没有播放器时不接管"
    );
}

#[test]
fn switching_repository_clears_only_the_previous_session() {
    let mut model = shell(true);
    model.player.candidates = vec![candidate("native.audio", "audio", "audio", &["mp3"])];
    load_playlist(&mut model, "repo-a", vec![item("a", "ready")]);
    send(&mut model, PlayerMessage::PlayListed { item_id: None });
    assert!(model.player.stored.contains_key("repo-a"));
    model.reduce(ShellMessage::PlaylistDetailLoaded(Ok(detail(
        "repo-b",
        "pl-b",
        "audio",
        "audio",
        vec![item("b", "ready")],
    ))));
    send(&mut model, PlayerMessage::PlayListed { item_id: None });
    assert!(!model.player.stored.contains_key("repo-a"));
    assert!(model.player.stored.contains_key("repo-b"));
    assert_eq!(model.player.current_id.as_deref(), Some("b"));

    model
        .player
        .stored
        .insert("repo-a".into(), model.player.stored["repo-b"].clone());
    send(
        &mut model,
        PlayerMessage::Stop {
            repo_id: Some("repo-a".into()),
            clear_stored: true,
        },
    );
    assert!(!model.player.stored.contains_key("repo-a"));
    assert_eq!(model.player.session.status, "failed");
    send(
        &mut model,
        PlayerMessage::Stop {
            repo_id: Some("repo-b".into()),
            clear_stored: true,
        },
    );
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
    model.player.load_files(
        &dir.join("missing-settings.json"),
        &sessions,
        &dir.join("missing-prefs.json"),
    );
    model.player.candidates = vec![candidate("native.audio", "audio", "audio", &["mp3"])];
    load_list(&mut model, vec![summary("repo", "pl", "audio", "audio")]);
    assert!(matches!(
        model.player.take_effects().iter().find(|effect| matches!(effect, PlayerEffect::RestoreDetail { .. })),
        Some(PlayerEffect::RestoreDetail { playlist_id, .. }) if playlist_id == "pl"
    ));
    send(
        &mut model,
        PlayerMessage::RestoreDetail(Ok(detail(
            "repo",
            "pl",
            "audio",
            "audio",
            vec![item("a", "ready")],
        ))),
    );
    assert_eq!(model.player.current_id.as_deref(), Some("a"));
    assert_eq!(model.player.mode, PlaybackMode::SingleLoop);
    assert_eq!(model.player.session.volume, 0.4);
    assert_eq!(model.player.session.current_time_ms, 1200);
    assert!(model.player.wants_playing);
    assert_eq!(model.player.session.status, "failed");
    assert!(
        model
            .player
            .session
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("没有原生解码器")
    );

    let mut rejected = shell(true);
    rejected
        .player
        .stored
        .insert("repo".into(), model.player.stored["repo"].clone());
    rejected.player.candidates = vec![candidate("native.audio", "audio", "audio", &["mp3"])];
    load_list(&mut rejected, vec![summary("repo", "pl", "audio", "audio")]);
    assert!(
        rejected
            .player
            .take_effects()
            .iter()
            .any(|effect| matches!(effect, PlayerEffect::RestoreDetail { .. }))
    );
    send(
        &mut rejected,
        PlayerMessage::RestoreDetail(Ok(detail(
            "repo",
            "pl",
            "audio",
            "audio",
            vec![item("a", "missing")],
        ))),
    );
    assert!(!rejected.player.stored.contains_key("repo"));
    assert!(rejected.player.current_id.is_none());

    let mut mismatch = shell(true);
    mismatch.player.stored.insert(
        "repo".into(),
        super::super::support::StoredSession {
            repo_id: "repo".into(),
            playlist_id: "other".into(),
            player_type_id: "audio".into(),
            current_item_id: "a".into(),
            current_time_ms: 0,
            duration_ms: 0,
            mode: PlaybackMode::ListLoop,
            volume: 1.0,
            is_playing: false,
        },
    );
    mismatch.player.restore_playlist_id = Some("pl".into());
    mismatch.player.candidates = vec![candidate("native.audio", "audio", "audio", &["mp3"])];
    send(
        &mut mismatch,
        PlayerMessage::RestoreDetail(Ok(detail(
            "repo",
            "pl",
            "audio",
            "audio",
            vec![item("a", "ready")],
        ))),
    );
    assert!(!mismatch.player.stored.contains_key("repo"));

    model.player.repo_id = Some("repo".into());
    load_list(&mut model, vec![summary("repo", "pl", "audio", "audio")]);
    assert!(
        !model
            .player
            .take_effects()
            .iter()
            .any(|effect| matches!(effect, PlayerEffect::RestoreDetail { .. }))
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// 插件登记的播放器类型要先读回才认得：读回之前不读详情也不丢会话；读回了认得就读详情，
/// 读回了还不认得就丢掉会话。顺序反过来也一样。
#[test]
fn restore_waits_for_plugin_player_types() {
    let stored = super::super::support::StoredSession {
        repo_id: "repo".into(),
        playlist_id: "pl".into(),
        player_type_id: "plugin.audio".into(),
        current_item_id: "a".into(),
        current_time_ms: 900,
        duration_ms: 4000,
        mode: PlaybackMode::ListLoop,
        volume: 0.5,
        is_playing: false,
    };
    let restores = |model: &mut ShellViewModel| {
        model
            .player
            .take_effects()
            .into_iter()
            .filter(|effect| matches!(effect, PlayerEffect::RestoreDetail { .. }))
            .count()
    };

    let mut model = shell(true);
    model.player.stored.insert("repo".into(), stored.clone());
    load_list(
        &mut model,
        vec![summary("repo", "pl", "plugin.audio", "audio")],
    );
    assert_eq!(restores(&mut model), 0, "播放器类型读回之前不读详情");
    assert!(model.player.stored.contains_key("repo"), "也不丢会话");
    model.reduce(ShellMessage::PlaylistPlayersLoaded(Ok(vec![contribution(
        "plugin.audio",
        "audio",
        &["mp3"],
    )])));
    assert_eq!(restores(&mut model), 1, "读回认得的类型以后读详情");
    model.reduce(ShellMessage::PlaylistPlayersLoaded(Ok(vec![contribution(
        "plugin.audio",
        "audio",
        &["mp3"],
    )])));
    assert_eq!(restores(&mut model), 0, "详情在读时不重复读");

    let mut players_first = shell(true);
    players_first
        .player
        .stored
        .insert("repo".into(), stored.clone());
    players_first.reduce(ShellMessage::PlaylistPlayersLoaded(Ok(vec![contribution(
        "plugin.audio",
        "audio",
        &["mp3"],
    )])));
    assert_eq!(restores(&mut players_first), 0, "还没有播放集列表");
    load_list(
        &mut players_first,
        vec![summary("repo", "pl", "plugin.audio", "audio")],
    );
    assert_eq!(restores(&mut players_first), 1);

    let mut gone = shell(true);
    gone.player.stored.insert("repo".into(), stored);
    load_list(
        &mut gone,
        vec![summary("repo", "pl", "plugin.audio", "audio")],
    );
    gone.reduce(ShellMessage::PlaylistPlayersLoaded(Ok(Vec::new())));
    assert_eq!(restores(&mut gone), 0);
    assert!(
        !gone.player.stored.contains_key("repo"),
        "读回了还不认得的类型就丢掉会话"
    );
}

#[test]
fn preview_and_player_share_one_media_session() {
    let mut model = shell(true);
    model.player.session.volume = 0.3;
    model.reduce(ShellMessage::AssetDetailLoaded(Ok(asset(
        "pics/a.png",
        "png",
    ))));
    assert_eq!(model.player.session.volume, 0.3);
    assert!(model.inspect.media_session().is_none());

    model.reduce(ShellMessage::AssetDetailLoaded(Ok(asset(
        "audio/a.mp3",
        "mp3",
    ))));
    let InspectEffect::LoadMedia {
        path, generation, ..
    } = model.inspect.take_effects().pop().unwrap()
    else {
        panic!("没有音视频请求");
    };
    model.reduce(ShellMessage::Inspect(InspectMessage::MediaLoaded {
        path,
        generation,
        result: Err("没有原生解码器".into()),
        pcm: None,
        frames: None,
        prep: crate::shell::SourcePrep::default(),
    }));
    // 读不出来的预览不接管播放条，播放器那份会话不受影响；预览页只显示自己的失败。
    assert_eq!(
        model
            .inspect
            .media_session()
            .map(|session| session.status.as_str()),
        Some("failed")
    );
    assert!(model.player.current_item().is_none());
    assert_ne!(model.player.session.status, "failed");
    send(&mut model, PlayerMessage::SetVolume(0.2));
    assert_eq!(model.player.session.volume, 0.2);
    assert_eq!(
        model.inspect.media_session().map(|session| session.volume),
        Some(1.0)
    );
    assert_ne!(model.player.session.status, "playing");
    // 播放器没在这个仓库里放东西，停止不改它；预览页的失败也留着。
    send(
        &mut model,
        PlayerMessage::Stop {
            repo_id: Some("repo".into()),
            clear_stored: true,
        },
    );
    assert_eq!(model.player.session.status, "idle");
    assert_eq!(
        model
            .inspect
            .media_session()
            .map(|session| session.status.as_str()),
        Some("failed")
    );

    model.player.candidates = vec![candidate("native.audio", "audio", "audio", &["mp3"])];
    load_playlist(&mut model, "repo", vec![item("a", "ready")]);
    send(&mut model, PlayerMessage::PlayListed { item_id: None });
    send(&mut model, PlayerMessage::OpenPreview { item_id: None });
    assert_eq!(model.workspace.panel, WorkspacePanel::Files);
    assert_eq!(model.workspace.library_category, LibraryCategory::All);
    assert_eq!(model.selected_path.as_deref(), Some("audio/a.mp3"));
    assert!(matches!(
        model.inspect.take_effects().pop(),
        Some(InspectEffect::LoadAsset { asset_id, .. }) if asset_id == "asset-a"
    ));
    assert!(!model.player.system_media_supported());
}

/// 播放条或播放集打开预览：预览页立刻换上。
///
/// 单击选中的同一条也要打开，不等素材详情回来；随后回来的详情留在预览页，不退回「只在右侧看」。
#[test]
fn opening_the_player_preview_shows_the_page_at_once() {
    let mut model = shell(true);
    model.player.candidates = vec![candidate("native.audio", "audio", "audio", &["mp3"])];
    load_playlist(&mut model, "repo", vec![item("a", "ready")]);
    send(&mut model, PlayerMessage::PlayListed { item_id: None });

    model.page = ShellPage::FileList;
    model.workspace.panel = WorkspacePanel::Playlist;
    model.workspace.library_category = LibraryCategory::Recent;
    model.files.select_only = Some("audio/a.mp3".into());
    model.files.click_load = Some("audio/a.mp3".into());
    model.files.selected = vec!["other.png".into()];
    model.files.primary = Some("other.png".into());
    assert!(
        !super::super::super::files_view::previewing(&model),
        "单击选中时还在文件列表"
    );

    send(&mut model, PlayerMessage::OpenPreview { item_id: None });

    assert_eq!(model.workspace.panel, WorkspacePanel::Files);
    assert_eq!(model.workspace.library_category, LibraryCategory::All);
    assert_eq!(model.page, ShellPage::SelectedFile);
    assert_eq!(model.selected_path.as_deref(), Some("audio/a.mp3"));
    assert!(
        model.files.select_only.is_none() && model.files.click_load.is_none(),
        "单击选中不再挡住预览"
    );
    assert_eq!(
        model.files.selected,
        vec!["audio/a.mp3".to_string()],
        "选择换成正在打开的这一条"
    );
    assert!(
        super::super::super::files_view::previewing(&model),
        "预览页立刻换上，不等详情"
    );

    model.reduce(ShellMessage::AssetDetailLoaded(Ok(asset(
        "audio/a.mp3",
        "mp3",
    ))));
    assert!(
        super::super::super::files_view::previewing(&model),
        "随后回来的详情仍留在预览页"
    );
    assert!(
        model.inspect.take_effects().iter().any(|effect| matches!(effect, InspectEffect::LoadMedia { path, .. } if path == "audio/a.mp3")),
        "详情回来以后才读预览内容"
    );

    let mut bare = shell(true);
    bare.page = ShellPage::FileList;
    bare.files.selected = vec!["keep.png".into()];
    send(&mut bare, PlayerMessage::OpenPreview { item_id: None });
    assert_eq!(bare.page, ShellPage::FileList, "没有正在放的条目时不换页");
    assert_eq!(bare.files.selected, vec!["keep.png".to_string()]);
}

/// 播放条量到的宽度只在真的变了时记下，半像素抖动和无效值不算。
#[test]
fn bar_width_feedback_ignores_half_pixel_jitter() {
    let mut model = shell(true);
    send(&mut model, PlayerMessage::BarResized(540.0));
    assert_eq!(model.player.bar_width, 540.0);
    send(&mut model, PlayerMessage::BarResized(540.4));
    assert_eq!(model.player.bar_width, 540.0);
    send(&mut model, PlayerMessage::BarResized(f32::NAN));
    assert_eq!(model.player.bar_width, 540.0);
    send(&mut model, PlayerMessage::BarResized(780.0));
    assert_eq!(model.player.bar_width, 780.0);
}

#[test]
fn acceptance_pages_use_the_player_surface() {
    assert!(ShellViewModel::for_page(ShellPage::PlaybackRunning).player_surface_visible());
    assert!(ShellViewModel::for_page(ShellPage::Playlists).player_surface_visible());
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
    model.reduce(ShellMessage::WorkspaceListLoaded {
        generation: 0,
        result: Ok(vec![repository.clone()]),
    });
    model.reduce(ShellMessage::RepositorySynced {
        generation: 0,
        result: Ok(crate::shell::SyncCounts::default()),
    });
    model.reduce(ShellMessage::RepositorySnapshotLoaded {
        generation: 0,
        result: Ok(RepositorySnapshot {
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
        }),
    });
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
    assert!(!model.player_surface_visible(), "首屏启动还在等设置包");
    crate::shell::data_load_tests::answer_settings(&mut model);
    assert!(model.player_surface_visible());
    model.workspace.panel = WorkspacePanel::Playlist;
    assert!(model.player_surface_visible());
    model.workspace.panel = WorkspacePanel::Trash;
    assert!(
        model.player_surface_visible(),
        "回收站仍是文件浏览面板，播放条留在列表下面"
    );

    let mut stale = shell(true);
    stale.sidebar.bind_repository(Some("repo-a"), false);
    stale.reduce(ShellMessage::PlaylistDetailLoaded(Ok(detail(
        "repo-b",
        "pl",
        "audio",
        "audio",
        vec![item("a", "ready")],
    ))));
    assert!(stale.player.listed.is_none());
}

/// 侧栏「播放」：从第一条就绪条目自动播放，不把正在看的播放集页换成这一张。
/// 没点开过播放集时也不打开页面。正在看的就是这一张时，页上的条目跟着读回的详情换。
/// 之后刷新正在看的那张，不把已经在放的另一张队列换掉。
#[test]
fn sidebar_play_starts_that_playlist_and_leaves_the_open_page() {
    fn row(playlist_id: &str, id: &str, status: &str) -> PlaylistItem {
        let mut row = item(id, status);
        row.playlist_id = playlist_id.into();
        row
    }

    let mut model = shell(true);
    model.workspace.panel = WorkspacePanel::Playlist;
    model.selected_playlist_id = Some("pl-a".into());
    model.playlist_item_ids = vec!["morning".into()];
    model.sidebar.active_playlist_id = Some("pl-a".into());
    model.player.listed = Some(detail(
        "repo",
        "pl-a",
        "audio",
        "audio",
        vec![row("pl-a", "morning", "ready")],
    ));
    model.sidebar.pending_play = Some("pl-b".into());
    model.reduce(ShellMessage::PlaylistDetailLoaded(Ok(detail(
        "repo",
        "pl-b",
        "audio",
        "audio",
        vec![
            row("pl-b", "broken", "missing"),
            row("pl-b", "night", "ready"),
        ],
    ))));
    assert_eq!(
        model.player.current_id.as_deref(),
        Some("night"),
        "跳过还没就绪的第一条"
    );
    assert!(model.player.wants_playing, "侧栏播放是自动播放");
    assert!(
        model
            .player
            .queue
            .iter()
            .all(|item| item.playlist_id == "pl-b")
    );
    assert_eq!(
        model
            .player
            .listed
            .as_ref()
            .map(|page| page.playlist.playlist_id.as_str()),
        Some("pl-a")
    );
    assert_eq!(model.selected_playlist_id.as_deref(), Some("pl-a"));
    assert_eq!(model.playlist_item_ids, ["morning"]);
    assert_eq!(model.sidebar.active_playlist_id.as_deref(), Some("pl-a"));
    assert_eq!(model.workspace.panel, WorkspacePanel::Playlist);
    assert!(model.sidebar.pending_play.is_none());

    model.reduce(ShellMessage::PlaylistDetailLoaded(Ok(detail(
        "repo",
        "pl-a",
        "audio",
        "audio",
        vec![
            row("pl-a", "morning", "ready"),
            row("pl-a", "extra", "ready"),
        ],
    ))));
    assert_eq!(
        model.player.current_id.as_deref(),
        Some("night"),
        "刷新正在看的播放集不换播放队列"
    );
    assert!(
        model
            .player
            .queue
            .iter()
            .all(|item| item.playlist_id == "pl-b")
    );
    let page = model.player.listed.as_ref().expect("正在看的播放集还在");
    assert_eq!(page.playlist.playlist_id, "pl-a");
    assert_eq!(page.items.len(), 2);
    assert_eq!(model.playlist_item_ids, ["morning", "extra"]);

    let mut bare = shell(true);
    bare.workspace.panel = WorkspacePanel::Files;
    bare.sidebar.pending_play = Some("pl-b".into());
    bare.reduce(ShellMessage::PlaylistDetailLoaded(Ok(detail(
        "repo",
        "pl-b",
        "audio",
        "audio",
        vec![row("pl-b", "night", "ready")],
    ))));
    assert!(
        bare.player.listed.is_none(),
        "没点开播放集时，播放不打开页面"
    );
    assert!(bare.selected_playlist_id.is_none());
    assert_eq!(bare.workspace.panel, WorkspacePanel::Files);
    assert_eq!(bare.player.current_id.as_deref(), Some("night"));

    let mut same = shell(true);
    same.workspace.panel = WorkspacePanel::Playlist;
    same.selected_playlist_id = Some("pl-a".into());
    same.player.listed = Some(detail(
        "repo",
        "pl-a",
        "audio",
        "audio",
        vec![row("pl-a", "morning", "ready")],
    ));
    same.sidebar.pending_play = Some("pl-a".into());
    same.reduce(ShellMessage::PlaylistDetailLoaded(Ok(detail(
        "repo",
        "pl-a",
        "audio",
        "audio",
        vec![
            row("pl-a", "broken", "missing"),
            row("pl-a", "morning", "ready"),
        ],
    ))));
    assert_eq!(same.player.current_id.as_deref(), Some("morning"));
    assert_eq!(
        same.player
            .listed
            .as_ref()
            .map(|page| page.playlist.playlist_id.as_str()),
        Some("pl-a")
    );
    assert_eq!(same.playlist_item_ids, ["broken", "morning"]);
    assert_eq!(same.workspace.panel, WorkspacePanel::Playlist);
}
