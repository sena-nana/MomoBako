//! 打开目录清除预览、搜索路由出现时机、AVI 播放与音量，以及原生页面和预览纹理的测试。

use super::*;

/// 文件元数据读回以后有预览目标；打开目录时预览目标跟着清掉。
#[test]
fn opening_a_directory_drops_the_preview_target() {
    let mut live = writable_shell();
    live.reduce(ShellMessage::AssetDetailLoaded(Ok(asset(
        "pics/a.png",
        "png",
        1,
        false,
        Vec::new(),
    ))));
    assert!(matches!(
        live.inspect.effects.last(),
        Some(InspectEffect::LoadImage { .. })
    ));
    assert_eq!(live.inspect.target_path.as_deref(), Some("pics/a.png"));

    live.reduce(ShellMessage::OpenDirectory("folder".into()));
    assert!(live.inspect.target_path.is_none());
}

#[test]
fn search_route_appears_only_after_startup_with_a_repository() {
    let mut model = ShellViewModel::default();
    model.workspace.panel = WorkspacePanel::Search;
    assert_eq!(RouteKey::of(&model), RouteKey::Startup);

    model.reduce(ShellMessage::WorkspaceListLoaded {
        generation: 0,
        result: Ok(vec![RepositorySummary {
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
        }]),
    });
    model.reduce(ShellMessage::RepositorySynced {
        generation: 0,
        result: Ok(crate::shell::SyncCounts::default()),
    });
    model.reduce(ShellMessage::RepositorySnapshotLoaded {
        generation: 0,
        result: Ok(RepositorySnapshot {
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
    assert_eq!(
        RouteKey::of(&model),
        RouteKey::Startup,
        "首屏启动还在等设置包"
    );
    crate::shell::data_load_tests::answer_settings(&mut model);
    model.workspace.panel = WorkspacePanel::Search;
    assert_eq!(RouteKey::of(&model), RouteKey::Search);
    model.workspace.panel = WorkspacePanel::Files;
    assert_eq!(RouteKey::of(&model), RouteKey::Files);
    model.reduce(ShellMessage::SelectFile {
        path: "pics/a.png".into(),
        asset_id: None,
    });
    assert_eq!(RouteKey::of(&model), RouteKey::Files);
    let selected = ShellViewModel::for_page(ShellPage::SelectedFile);
    assert_eq!(RouteKey::of(&selected), RouteKey::Files);
}

#[test]
fn avi_preview_plays_pauses_and_seeks_on_the_shared_session() {
    assert!(!crate::shell::player::sound_device_compiled_in());
    let parts = super::super::support::preview_media_parts(
        "repo",
        &super::super::support::sample_uncompressed(),
    )
    .expect("avi");
    assert!(
        parts
            .frames
            .as_ref()
            .is_some_and(|frames| frames.len() == 2)
    );
    assert!(parts.pcm.is_some());
    let mut model = ShellViewModel::default();
    model.workspace.active_repo_id = Some("repo".into());
    model.player.contributions = vec![video_player()];
    model.reduce(ShellMessage::AssetDetailLoaded(Ok(asset(
        "clips/a.avi",
        "avi",
        1,
        false,
        Vec::new(),
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
        result: Ok(parts.session),
        pcm: parts.pcm,
        frames: parts.frames,
        prep: crate::shell::SourcePrep::default(),
    }));
    assert_eq!(model.player.session.status, "paused");
    assert!(
        model
            .preview_token
            .as_deref()
            .unwrap_or_default()
            .starts_with("video:")
    );
    assert_eq!(model.preview_pixels.as_ref().expect("画面").rgba[0], 255);
    assert!(
        crate::shell::wants_preview_texture(&model),
        "视频画面从纹理槽取，宿主要上传"
    );
    model.reduce(ShellMessage::Player(PlayerMessage::SetPlaying(true)));
    assert_eq!(model.player.session.status, "playing");
    assert_eq!(
        model.inspect.media_session().expect("会话").status,
        "playing"
    );
    assert!(super::super::poll_timers(
        &mut model,
        crate::frame_clock::FrameStep::millis(16)
    ));
    assert_eq!(model.player.session.current_time_ms, 16);
    assert_eq!(
        model.inspect.media_session().expect("会话").current_time_ms,
        16
    );
    model.reduce(ShellMessage::Player(PlayerMessage::Seek(500)));
    assert_eq!(model.player.session.current_time_ms, 500);
    assert_eq!(
        model.inspect.media_session().expect("会话").current_time_ms,
        500
    );
    assert_eq!(model.preview_pixels.as_ref().expect("第二帧").rgba[2], 255);
    model.reduce(ShellMessage::Player(PlayerMessage::SetPlaying(false)));
    assert_eq!(model.player.session.status, "paused");
    assert_eq!(
        model.inspect.media_session().expect("会话").status,
        "paused"
    );
}

/// 没有音轨的视频只走播放时钟：播放条上拖音量只记下音量，画面照样播放，不会因为「不支持音量控制」变成失败。
#[test]
fn volume_on_a_silent_video_is_remembered_without_failing_playback() {
    let parts = super::super::support::preview_media_parts(
        "repo",
        &super::super::support::sample_uncompressed(),
    )
    .expect("avi");
    let mut session = parts.session;
    session.can_volume = false;
    let mut model = ShellViewModel::default();
    model.workspace.active_repo_id = Some("repo".into());
    model.player.contributions = vec![video_player()];
    model.reduce(ShellMessage::AssetDetailLoaded(Ok(asset(
        "clips/silent.avi",
        "avi",
        1,
        false,
        Vec::new(),
    ))));
    let InspectEffect::LoadMedia {
        path, generation, ..
    } = model.inspect.take_effects().pop().unwrap()
    else {
        panic!("没有音视频请求");
    };
    let prep = crate::shell::SourcePrep::default();
    model.reduce(ShellMessage::Inspect(InspectMessage::MediaLoaded {
        path,
        generation,
        result: Ok(session),
        pcm: None,
        frames: parts.frames,
        prep,
    }));
    model.reduce(ShellMessage::Player(PlayerMessage::SetPlaying(true)));
    assert_eq!(model.player.session.status, "playing");
    model.reduce(ShellMessage::Player(PlayerMessage::SetVolume(0.3)));
    assert_eq!(
        model.player.session.status, "playing",
        "调音量不该让没有声音的视频失败"
    );
    assert_eq!(model.player.session.error, None);
    assert_eq!(
        model.player.session.volume, 0.3,
        "音量记下，下一项有声音时沿用"
    );
}

/// PDF 和模型的页图从 `deck` 编成内容图画，不复制进 `preview_pixels`，宿主没有东西可传到 `file-preview`。
/// 上一张图片留下的画面在原生预览读好时放掉；翻页和转模型也不再写回。
#[test]
fn native_pages_never_reach_the_preview_texture() {
    let mut model = writable_shell();
    model.reduce(ShellMessage::AssetDetailLoaded(Ok(asset(
        "pics/a.png",
        "png",
        1,
        false,
        Vec::new(),
    ))));
    model.reduce(ShellMessage::PreviewPixelsLoaded {
        source: source("pics/a.png"),
        pixels: Ok(PreviewPixels {
            width: 1,
            height: 1,
            rgba: vec![1, 2, 3, 4],
        }),
    });
    assert!(
        crate::shell::wants_preview_texture(&model),
        "图片预览从纹理槽取画面"
    );

    model.reduce(ShellMessage::AssetDetailLoaded(Ok(asset(
        "docs/a.pdf",
        "pdf",
        1,
        false,
        Vec::new(),
    ))));
    let generation = native_generation(&mut model);
    let page = |shade: u8| PageFrame {
        width: 2,
        height: 2,
        rgba: vec![shade; 16],
        text: String::new(),
        error: None,
    };
    model.reduce(ShellMessage::Inspect(InspectMessage::NativeLoaded {
        path: "docs/a.pdf".into(),
        generation,
        result: Ok(NativeLoad {
            text: "PDF".into(),
            frames: vec![page(10), page(20)],
            mesh: None,
            paged: true,
            archive: None,
        }),
    }));
    assert!(
        model.preview_pixels.is_none() && model.preview_token.is_none(),
        "上一张图片的画面放掉"
    );
    assert_eq!(
        model.inspect.raster_frame().map(|(_, _, rgba)| rgba[0]),
        Some(10),
        "页图从 deck 取"
    );
    assert!(!crate::shell::wants_preview_texture(&model));
    model.reduce(ShellMessage::Inspect(InspectMessage::TurnPage(1)));
    assert_eq!(
        model.inspect.raster_frame().map(|(_, _, rgba)| rgba[0]),
        Some(20)
    );
    assert!(
        model.preview_pixels.is_none() && model.preview_token.is_none(),
        "翻页不写回预览像素"
    );

    model.reduce(ShellMessage::AssetDetailLoaded(Ok(asset(
        "models/a.obj",
        "obj",
        1,
        false,
        Vec::new(),
    ))));
    let generation = native_generation(&mut model);
    let mesh = MeshData {
        vertices: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        triangles: vec![[0, 1, 2]],
    };
    model.reduce(ShellMessage::Inspect(InspectMessage::NativeLoaded {
        path: "models/a.obj".into(),
        generation,
        result: Ok(NativeLoad {
            text: String::new(),
            frames: Vec::new(),
            mesh: Some(mesh),
            paged: false,
            archive: None,
        }),
    }));
    model.reduce(ShellMessage::Inspect(InspectMessage::Orbit {
        yaw: 0.5,
        zoom: 1.0,
    }));
    assert!(model.inspect.raster_frame().is_some(), "转过以后画出网格");
    assert!(
        model.preview_pixels.is_none() && model.preview_token.is_none(),
        "转模型不写回预览像素"
    );
    assert!(!crate::shell::wants_preview_texture(&model));
}

/// 宿主只在预览框这一帧从 `file-preview` 槽取画面时上传：单击只选中时预览页藏着，不传；预览的正是
/// 幻灯片当前项时画幻灯片自己的帧，也不传。幻灯片本身不登记任何宿主纹理。
#[test]
fn the_preview_texture_is_wanted_only_while_the_frame_draws_it() {
    let mut model = writable_shell();
    model.reduce(ShellMessage::AssetDetailLoaded(Ok(asset(
        "pics/a.png",
        "png",
        1,
        false,
        Vec::new(),
    ))));
    model.reduce(ShellMessage::PreviewPixelsLoaded {
        source: source("pics/a.png"),
        pixels: Ok(PreviewPixels {
            width: 1,
            height: 1,
            rgba: vec![1, 2, 3, 4],
        }),
    });
    assert!(crate::shell::wants_preview_texture(&model));

    model.files.select_only = Some("pics/a.png".into());
    assert!(
        !crate::shell::wants_preview_texture(&model),
        "单击只选中：预览页藏着"
    );
    model.files.select_only = None;

    model.player.repo_id = Some("repo".into());
    model.player.still = Some(StillShow {
        path: "pics/a.png".into(),
        frame: Some(PreviewPixels {
            width: 1,
            height: 1,
            rgba: vec![9; 4],
        }),
    });
    assert!(
        !crate::shell::wants_preview_texture(&model),
        "幻灯片当前项画自己的帧"
    );
    model.player.still = None;
    assert!(crate::shell::wants_preview_texture(&model));
}
