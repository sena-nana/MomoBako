//! 压缩包内条目解码音频与 AVI 帧且不打开声音设备的测试。

use super::*;

/// 取出的 WAV、mp3、flac、ogg 用现有解码在内存里解开，预览是唱片舞台。不装进播放条，也不打开声卡。
/// 解不开的 wav 用现有失败浮层。视频和没有这条解码的格式仍停在名单上。
#[test]
fn archive_entry_previews_decoded_audio_without_opening_the_sound_device() {
    assert!(
        !crate::shell::player::sound_device_compiled_in(),
        "测试构建不应编进声卡"
    );
    let wav = voice_wav();
    let mp3 = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/tone.mp3"
    ));
    let flac = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/tone.flac"
    ));
    let ogg = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/tone.ogg"
    ));
    let zip = sample_zip(&[
        ("song/voice.wav", &wav),
        ("tone.mp3", mp3),
        ("tone.flac", flac),
        ("tone.ogg", ogg),
        ("broken.wav", b"RIFF"),
        ("clip.mp4", b"ftyp"),
        ("voice.wma", b"wma"),
    ]);
    let mut state = opened_archive(&zip, "pack.zip");
    let outer_list = native_text(&state);

    state.reduce(
        true,
        Some("repo"),
        InspectMessage::OpenArchiveFile("song/voice.wav".into()),
    );
    let preview = take(&mut state, &zip);
    assert_audio(&preview, &wav);
    finish(&mut state, "pack.zip", Ok(preview));
    assert_eq!(native_text(&state), outer_list, "外层名单还在");
    let harness = mount(state);
    assert!(harness.keyed("inspect-archive-entry").is_some());
    assert!(
        harness.keyed("inspect-audio-stage").is_some(),
        "用现有唱片舞台"
    );
    assert!(harness.keyed("inspect-player-mount").is_some());
    assert!(harness.find("voice").is_some(), "标题是文件名");
    assert!(harness.find("通用音频").is_some());
    assert!(harness.find("暂无歌词").is_some());
    assert!(
        harness.keyed("inspect-archive-list").is_some(),
        "打开后名单还在"
    );
    assert_eq!(
        harness.model.player.session.status, "idle",
        "没有装进播放条"
    );
    assert!(!harness.model.player.wants_playing, "没有开始播放");
    state = unmount(harness);

    for (name, bytes) in [
        ("tone.mp3", mp3.as_slice()),
        ("tone.flac", flac.as_slice()),
        ("tone.ogg", ogg.as_slice()),
    ] {
        state.reduce(
            true,
            Some("repo"),
            InspectMessage::EnterArchive(String::new()),
        );
        state.reduce(
            true,
            Some("repo"),
            InspectMessage::OpenArchiveFile(name.into()),
        );
        let preview = take(&mut state, &zip);
        assert_audio(&preview, bytes);
        finish(&mut state, "pack.zip", Ok(preview));
        assert!(state.take_effects().is_empty(), "{name} 不应再去读或播放");
        assert!(
            !matches!(
                state.body,
                super::super::super::super::PreviewBody::Media(_)
            ),
            "{name} 不应换成播放会话"
        );
    }

    state.reduce(
        true,
        Some("repo"),
        InspectMessage::EnterArchive(String::new()),
    );
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::OpenArchiveFile("broken.wav".into()),
    );
    let failed = take(&mut state, &zip);
    match &failed.kind {
        ArchiveEntryKind::Audio {
            error: Some(error),
            duration_ms,
            ..
        } => {
            assert_eq!(*duration_ms, 0);
            assert!(error.contains("不是 WAV 头"), "{error}");
        }
        other => panic!("失败状态丢了：{other:?}"),
    }
    finish(&mut state, "pack.zip", Ok(failed));
    let harness = mount(state);
    assert!(shows(&harness, "无法预览该音频"));
    assert!(shows(&harness, "不是 WAV 头"));
    assert!(
        harness.keyed("inspect-audio-stage").is_none(),
        "解不开就不画唱片"
    );
    assert_eq!(harness.model.player.session.status, "idle");
    assert!(!harness.model.player.wants_playing);
    state = unmount(harness);

    state.reduce(
        true,
        Some("repo"),
        InspectMessage::EnterArchive(String::new()),
    );
    for name in ["clip.mp4", "voice.wma"] {
        state.reduce(
            true,
            Some("repo"),
            InspectMessage::OpenArchiveFile(name.into()),
        );
        assert!(state.take_effects().is_empty(), "{name} 应停在名单上");
        assert!(state.archive_entry.is_none(), "{name}");
    }
    assert!(
        !crate::shell::player::sound_device_compiled_in(),
        "解完仍然没有声卡模块"
    );
    assert_eq!(native_text(&state), outer_list);
}

/// 取出的未压缩 AVI（夹具 `clip.avi`）用现有纯 Rust 解码画出真实帧。PCM 不进播放条，也不打开声卡。
/// 其它视频扩展名仍停在名单上。字节不是未压缩 AVI 的 `.avi` 取出后仍回到名单。
#[test]
fn archive_entry_draws_uncompressed_avi_frames_without_opening_the_sound_device() {
    assert!(
        !crate::shell::player::sound_device_compiled_in(),
        "测试构建不应编进声卡"
    );
    let clip = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/clip.avi"
    ));
    let zip = sample_zip(&[
        ("movie/clip.avi", clip),
        ("clip.mp4", b"ftyp"),
        ("clip.mkv", b"mkv"),
        ("clip.webm", b"webm"),
        ("clip.mov", b"mov"),
        ("clip.m4v", b"m4v"),
        ("fake.avi", b"RIFF"),
    ]);
    let mut state = opened_archive(&zip, "pack.zip");
    let outer_list = native_text(&state);

    state.reduce(
        true,
        Some("repo"),
        InspectMessage::OpenArchiveFile("movie/clip.avi".into()),
    );
    let preview = take(&mut state, &zip);
    let (first_png, later, second_png) = match &preview.kind {
        ArchiveEntryKind::Video {
            frames,
            position_ms,
        } => {
            assert_eq!(*position_ms, 0, "打开时播放头在片头");
            assert!(frames.len() >= 2, "{}", frames.len());
            let first = &frames[0];
            assert_eq!(
                (first.width, first.height, first.rgba.len()),
                (32, 18, 32 * 18 * 4)
            );
            assert!(
                first
                    .rgba
                    .chunks_exact(4)
                    .all(|pixel| pixel == [0x20, 0x22, 0x26, 0xFF]),
                "第一帧应是文件里的深灰"
            );
            let second = &frames[1];
            assert!(
                second.time_ms > first.time_ms,
                "第二帧应晚于片头，后半才落得上去"
            );
            assert!(
                second
                    .rgba
                    .chunks_exact(4)
                    .all(|pixel| pixel == [0x2A, 0x2D, 0x33, 0xFF]),
                "第二帧应是文件里的另一色"
            );
            (frame_png(first), second.time_ms, frame_png(second))
        }
        other => panic!("不是解开的画面：{other:?}"),
    };
    finish(&mut state, "pack.zip", Ok(preview));
    assert_eq!(native_text(&state), outer_list, "外层名单还在");
    assert!(state.take_effects().is_empty(), "解完不应再去读或播放");
    assert!(
        !matches!(
            state.body,
            super::super::super::super::PreviewBody::Media(_)
        ),
        "不应换成播放会话"
    );
    let mut harness = mount(state);
    assert!(harness.keyed("inspect-archive-entry").is_some());
    assert!(
        harness.keyed("inspect-archive-video").is_some(),
        "片头画已经解开的第一帧"
    );
    assert_eq!(
        painted(&harness, "inspect-archive-video"),
        first_png,
        "片头应是解码出的第一帧，不是手写色块"
    );
    assert!(
        harness.keyed("inspect-video").is_none(),
        "没有宿主纹理时不挂 file-preview"
    );
    assert!(
        !crate::shell::wants_preview_texture(&harness.model),
        "离屏不上传宿主纹理"
    );
    assert!(harness.model.preview_token.is_none(), "离屏不挂纹理记号");
    assert!(
        harness.keyed("inspect-archive-list").is_some(),
        "打开后名单还在"
    );
    assert!(
        harness.keyed("inspect-audio-stage").is_none(),
        "视频不画唱片"
    );
    assert!(harness.keyed("inspect-fallback-icon").is_none());
    assert_eq!(
        harness.model.player.session.status, "idle",
        "没有装进播放条"
    );
    assert!(!harness.model.player.wants_playing, "没有开始播放");
    assert!(
        !matches!(
            harness.model.inspect.body,
            super::super::super::super::PreviewBody::Media(_)
        ),
        "拨播放头也不换成播放会话"
    );

    // 夹具两帧平分时长，第二帧的时刻就是后半。用预览自己的位置，不拨播放条。
    super::super::super::archive::place_video(&mut harness.model, later);
    harness.sync();
    harness.flush();
    assert_eq!(
        painted(&harness, "inspect-archive-video"),
        second_png,
        "后半应是解码出的第二帧"
    );
    assert!(
        harness.keyed("inspect-video").is_none(),
        "播放头往前走也不挂 file-preview"
    );
    assert!(
        harness.model.preview_token.is_none(),
        "没有宿主纹理时不把记号挂回去"
    );
    assert_eq!(
        harness.model.player.session.status, "idle",
        "播放条仍是 idle"
    );
    assert!(!crate::shell::wants_preview_texture(&harness.model));

    harness.model.preview_token = Some("video:movie/clip.avi:0:32".into());
    harness.model.preview_pixels = Some(crate::shell::PreviewPixels {
        width: 2,
        height: 2,
        rgba: vec![255; 16],
    });
    harness.sync();
    harness.flush();
    assert!(
        harness.keyed("inspect-video").is_some(),
        "有宿主纹理时和文件预览一样画纹理槽"
    );
    assert!(
        harness.keyed("inspect-archive-video").is_none(),
        "有宿主纹理时不改画解码像素"
    );
    assert!(
        crate::shell::wants_preview_texture(&harness.model),
        "有宿主纹理时仍要上传"
    );
    super::super::super::archive::place_video(&mut harness.model, later);
    assert!(
        harness
            .model
            .preview_token
            .as_deref()
            .is_some_and(|token| token.starts_with("video:")),
        "已有记号时跟着换到这一帧"
    );
    harness.model.preview_token = None;
    super::super::super::archive::place_video(&mut harness.model, later);
    harness.sync();
    harness.flush();
    assert!(
        harness.model.preview_token.is_none(),
        "清掉记号后再拨，也不把未注册记号挂回去"
    );
    assert_eq!(
        painted(&harness, "inspect-archive-video"),
        second_png,
        "清掉记号后仍画解码出的第二帧"
    );
    assert!(harness.keyed("inspect-video").is_none());
    state = unmount(harness);

    state.reduce(
        true,
        Some("repo"),
        InspectMessage::EnterArchive(String::new()),
    );
    assert!(state.archive_entry.is_none(), "面包屑回到名单");
    for name in ["clip.mp4", "clip.mkv", "clip.webm", "clip.mov", "clip.m4v"] {
        state.reduce(
            true,
            Some("repo"),
            InspectMessage::OpenArchiveFile(name.into()),
        );
        assert!(state.take_effects().is_empty(), "{name} 应停在名单上");
        assert!(state.archive_entry.is_none(), "{name}");
    }

    state.reduce(
        true,
        Some("repo"),
        InspectMessage::OpenArchiveFile("fake.avi".into()),
    );
    let effect = state.take_effects().pop().expect("扩展名放行，要读字节");
    let InspectEffect::LoadArchiveEntry { entry, through, .. } = effect else {
        panic!("不是包内文件：{effect:?}")
    };
    let error = archive::preview_chain(&zip, &through, &entry).expect_err("不是未压缩 AVI");
    finish(&mut state, "pack.zip", Err(error));
    assert!(state.archive_entry.is_none(), "解不开仍停在名单上");
    assert_eq!(native_text(&state), outer_list);
    assert!(
        !crate::shell::player::sound_device_compiled_in(),
        "解完仍然没有声卡模块"
    );
}
