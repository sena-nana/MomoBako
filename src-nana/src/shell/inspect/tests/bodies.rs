//! 按扩展名分派预览、文本截断与解码、图片失败、无解码器媒体、WAV 接管播放条与过期文本的测试。

use super::*;

/// 分派顺序：图片、Markdown（先于文本）、文本、音视频，再是内置的 PDF、
/// Office、模型和压缩包（由 Nana 自己绘制），
/// 认不出的扩展名（例如 psd）不预览。
#[test]
fn extension_dispatch_prefers_markdown_and_native_preview() {
    assert_eq!(super::super::support::classify("PNG"), PreviewKind::Image);
    assert_eq!(super::super::support::classify("md"), PreviewKind::Markdown);
    assert_eq!(super::super::support::classify("txt"), PreviewKind::Text);
    assert_eq!(super::super::support::classify("vue"), PreviewKind::Text);
    assert_eq!(super::super::support::classify("mp3"), PreviewKind::Media);
    assert_eq!(
        super::super::support::classify("pdf"),
        PreviewKind::Native {
            view_id: "momobako.preview.pdf".into(),
            label: "PDF".into()
        }
    );
    assert_eq!(
        super::super::support::classify("glb"),
        PreviewKind::Native {
            view_id: "momobako.preview.model".into(),
            label: "模型".into()
        }
    );
    assert_eq!(
        super::super::support::classify("zip"),
        PreviewKind::Native {
            view_id: "momobako.preview.archive".into(),
            label: "压缩包".into()
        }
    );
    assert_eq!(
        super::super::support::classify("bin"),
        PreviewKind::Unsupported
    );
    assert_eq!(
        super::super::support::classify("psd"),
        PreviewKind::Unsupported
    );
}

#[test]
fn text_over_the_limit_shows_the_head_and_invalid_utf8_is_lossy() {
    let limit = 768 * 1024;
    let whole = super::super::prepare_text(&vec![b'a'; limit]);
    assert_eq!((whole.text.len(), whole.truncated_at), (limit, None));
    let head = super::super::prepare_text(&vec![b'a'; limit + 1]);
    assert_eq!(
        (head.text.len(), head.truncated_at),
        (limit, Some(limit as u64))
    );
    assert_eq!(super::super::prepare_text(&[0xff]).text, "\u{FFFD}");
}

#[test]
fn text_decodes_boms_and_drops_a_character_cut_by_the_limit() {
    assert_eq!(
        super::super::prepare_text(&[0xef, 0xbb, 0xbf, b'h', b'i']).text,
        "hi"
    );
    assert_eq!(
        super::super::prepare_text(&[0xff, 0xfe, b'h', 0, b'i', 0]).text,
        "hi"
    );
    assert_eq!(
        super::super::prepare_text(&[0xfe, 0xff, 0, b'h', 0, b'i']).text,
        "hi"
    );
    let mut bytes = vec![b'a'; 768 * 1024 - 1];
    bytes.extend_from_slice("好".as_bytes());
    let head = super::super::prepare_text(&bytes);
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
    assert!(
        matches!(state.take_effects().pop(), Some(InspectEffect::LoadImage { path, .. }) if path == "pics/a.png")
    );

    state.note_detail(&asset("notes/a.md", "md", 1, false, Vec::new()));
    assert!(matches!(
        state.take_effects().pop(),
        Some(InspectEffect::LoadText { markdown: true, path, .. }) if path == "notes/a.md"
    ));
    state.note_detail(&asset("notes/a.txt", "txt", 1, false, Vec::new()));
    assert!(matches!(
        state.take_effects().pop(),
        Some(InspectEffect::LoadText {
            markdown: false,
            ..
        })
    ));

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
    model.reduce(ShellMessage::SelectFile {
        path: "pics/a.png".into(),
        asset_id: None,
    });
    model.preview_pixels = Some(PreviewPixels {
        width: 1,
        height: 1,
        rgba: vec![1, 2, 3, 4],
    });
    model.reduce(ShellMessage::PreviewPixelsLoaded {
        source: source("pics/other.png"),
        pixels: Ok(PreviewPixels {
            width: 2,
            height: 2,
            rgba: vec![9, 9, 9, 9],
        }),
    });
    assert_eq!(
        model.preview_pixels.as_ref().map(|pixels| pixels.width),
        Some(1)
    );
    assert!(matches!(model.inspect.body, PreviewBody::Empty));

    model.reduce(ShellMessage::PreviewPixelsLoaded {
        source: source("pics/a.png"),
        pixels: Err("坏图".into()),
    });
    assert!(model.preview_pixels.is_none());
    assert!(
        matches!(model.inspect.body, PreviewBody::Failed(ref message) if message.contains("坏图"))
    );
}

/// 读不出来的预览不接管播放条：播放条上的播放、跳转和音量都不会把它变成 playing。
#[test]
fn media_without_a_decoder_never_reports_playing() {
    let mut model = ShellViewModel::default();
    model.workspace.active_repo_id = Some("repo".into());
    model.reduce(ShellMessage::AssetDetailLoaded(Ok(asset(
        "audio/a.mp3",
        "mp3",
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
        result: Err("没有原生解码器".into()),
        pcm: None,
        frames: None,
        prep: crate::shell::SourcePrep::default(),
    }));
    let PreviewBody::Media(session) = &model.inspect.body else {
        panic!("不是音视频")
    };
    assert_eq!(session.status, "failed");
    assert!(
        session
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("没有原生解码器")
    );
    assert!(model.player.current_item().is_none());
    assert!(
        model.inspect.take_effects().is_empty(),
        "失败的预览不该自动播放"
    );
    for message in [
        PlayerMessage::SetPlaying(true),
        PlayerMessage::Seek(1_500),
        PlayerMessage::SetVolume(0.4),
    ] {
        model.reduce(ShellMessage::Player(message));
        let PreviewBody::Media(session) = &model.inspect.body else {
            panic!("控制后丢了会话")
        };
        assert_eq!(session.status, "failed");
    }
    assert!(super::super::support::preview_media_session("repo", b"ID3").is_err());
}

/// 预览的音频插成临时条目接管播放条，宿主派发自动播放后开始走时钟。
#[test]
fn wav_preview_takes_over_the_bar_and_autoplays_once_dispatched() {
    let mut model = ShellViewModel::default();
    model.workspace.active_repo_id = Some("repo".into());
    model.reduce(ShellMessage::AssetDetailLoaded(Ok(asset(
        "audio/a.wav",
        "wav",
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
    let session = super::super::support::preview_media_session("repo", &tone_wav()).expect("wav");
    assert_eq!(session.status, "paused");
    assert_eq!(session.duration_ms, Some(2));
    let prep = crate::shell::SourcePrep::default();
    model.reduce(ShellMessage::Inspect(InspectMessage::MediaLoaded {
        path,
        generation,
        result: Ok(session),
        pcm: None,
        frames: None,
        prep,
    }));
    let item = model.player.current_item().cloned().expect("临时条目");
    assert!(item.transient);
    assert_eq!(item.path, "audio/a.wav");
    assert_eq!(item.player_label, "WAV");
    assert!(model.player.preview_owns_bar());
    assert_eq!(model.player.session.status, "paused", "归约本身不出声");
    let bar = super::super::super::player_view::bar::props_for_test(&model);
    assert_eq!(bar.title(), "正在播放 a");
    assert_eq!(bar.subtitle(), "WAV · audio/a.wav");
    assert!(bar.can_play && !bar.playing);
    let effects = model.inspect.take_effects();
    let [InspectEffect::Autoplay { path, generation }] = effects.as_slice() else {
        panic!("接管后应排一次自动播放");
    };
    model.reduce(ShellMessage::Inspect(InspectMessage::Autoplay {
        path: path.clone(),
        generation: *generation,
    }));
    assert_eq!(model.player.session.status, "playing");
    assert_eq!(
        model
            .inspect
            .media_session()
            .map(|session| session.status.as_str()),
        Some("playing")
    );
    assert!(super::super::super::player_view::bar::props_for_test(&model).playing);
    model.reduce(ShellMessage::Player(PlayerMessage::SetPlaying(false)));
    model.reduce(ShellMessage::Player(PlayerMessage::Seek(1)));
    let PreviewBody::Media(session) = &model.inspect.body else {
        panic!("丢了会话")
    };
    assert_eq!(session.status, "paused");
    assert_eq!(session.current_time_ms, 1);
    assert!(session.error.is_none());
    model.reduce(ShellMessage::Inspect(InspectMessage::Autoplay {
        path: "audio/a.wav".into(),
        generation: model.inspect.generation + 1,
    }));
    assert_eq!(model.player.session.status, "paused", "过期的自动播放不动");
}

#[test]
fn stale_text_is_ignored_and_the_current_generation_renders() {
    let mut state = InspectState::default();
    state.note_detail(&asset("notes/a.txt", "txt", 1, false, Vec::new()));
    let InspectEffect::LoadText {
        path,
        generation,
        markdown,
        ..
    } = state.take_effects().pop().unwrap()
    else {
        panic!("没有文本请求");
    };
    state.begin_selection("notes/b.txt");
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::BodyLoaded {
            path: path.clone(),
            markdown,
            generation,
            result: Ok(super::super::prepare_text("旧文本".as_bytes())),
        },
    );
    assert!(matches!(state.body, PreviewBody::Empty));
    let generation = state.generation;
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::BodyLoaded {
            path: "notes/b.txt".into(),
            markdown: false,
            generation,
            result: Err("无法读取文件".into()),
        },
    );
    assert!(matches!(state.body, PreviewBody::Failed(ref message) if message.contains("无法读取")));
}
