//! 内置 WAV 和压缩音频的播放会话测试。
//!
//! 只解内存里的 PCM，测试构建不打开声卡。辅助构造沿用上层 `tests` 模块。

use super::super::super::{InspectEffect, InspectMessage, ShellMessage};
use super::super::{PlayerMessage, QueueItem};
use super::{asset, queue_item, send, shell, temp_dir};

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

/// 和 Vue 一样，点播放条目后装载完成就开始播放；暂停、跳转、音量都落在同一游标上。
#[test]
fn wav_session_loads_and_plays_then_pauses_seeks_and_sets_volume() {
    let dir = temp_dir("wav");
    let path = dir.join("tone.wav");
    let samples = [128u8; 16];
    std::fs::write(&path, pcm_wav(8_000, 1, 8, &samples)).unwrap();
    let expected_ms = (samples.len() as u64) * 1000 / 8_000;

    let mut model = shell(true);
    assert_eq!(model.player.candidates.len(), 2);
    let builtin = &model.player.candidates[0];
    assert_eq!(builtin.plugin_id, "momobako.player.wav");
    assert_eq!(builtin.player_type_id, "momobako.playlist.wav");
    assert_eq!(builtin.label, "WAV");
    assert_eq!(builtin.file_class, "audio");
    assert_eq!(builtin.extensions, ["wav"]);
    assert!(builtin.supports_seek && builtin.supports_volume);

    model.player.repo_id = Some("repo".into());
    model.player.queue = vec![wav_queue_item("tone", &path)];
    model.reduce(ShellMessage::Player(PlayerMessage::PlayItem { item_id: "tone".into() }));
    assert_eq!(model.player.session.status, "loading", "条目字节经仓库服务读取前先停在读取中");
    assert!(!model.player.can_play);
    super::super::fulfill_loads(&mut model);
    assert_eq!(model.player.session.status, "playing");
    assert_eq!(model.player.session.duration_ms, Some(expected_ms));
    assert!(model.player.session.can_seek);
    assert!(model.player.session.can_volume);
    assert!(model.player.session.error.is_none());

    send(&mut model, PlayerMessage::SetPlaying(false));
    assert_eq!(model.player.session.status, "paused");
    let middle = expected_ms / 2;
    send(&mut model, PlayerMessage::Seek(middle));
    assert_eq!(model.player.session.current_time_ms, middle);
    assert_eq!(model.player.session.status, "paused");
    send(&mut model, PlayerMessage::SetPlaying(true));
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

#[test]
fn compressed_audio_playlist_loads_duration_and_plays_without_a_device() {
    let dir = temp_dir("compressed-audio");
    let mut model = shell(true);
    model.player.repo_id = Some("repo".into());
    let kind = model
        .player
        .candidates
        .iter()
        .find(|candidate| candidate.extensions.iter().any(|extension| extension == "mp3"))
        .expect("压缩音频候选")
        .player_type_id
        .clone();
    let cases = [
        ("tone.mp3", "mp3", crate::shell::audio_decode::fixture_mp3()),
        ("tone.flac", "flac", crate::shell::audio_decode::fixture_flac()),
        ("tone.ogg", "ogg", crate::shell::audio_decode::fixture_ogg()),
    ];
    for (name, extension, bytes) in cases {
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        let decoded = crate::shell::audio_decode::decode_compressed(bytes).expect(extension);
        assert!(decoded.duration_ms > 0, "{extension}");
        let mut item = wav_queue_item(extension, &path);
        item.filename = name.into();
        item.extension = extension.into();
        item.player_type_id = kind.clone();
        model.player.queue = vec![item];
        model.player.current_id = None;
        send(&mut model, PlayerMessage::PlayItem { item_id: extension.into() });
        assert_eq!(model.player.session.status, "playing", "{extension}");
        assert_eq!(model.player.session.duration_ms, Some(decoded.duration_ms), "{extension}");
        assert!(model.player.session.can_seek && model.player.session.can_volume, "{extension}");
        assert!(model.player.session.error.is_none(), "{extension}");
        send(&mut model, PlayerMessage::SetPlaying(false));
        assert_eq!(model.player.session.status, "paused", "{extension}");
    }
    let bad = dir.join("bad.mp3");
    std::fs::write(&bad, b"not-audio").unwrap();
    let mut item = wav_queue_item("bad", &bad);
    item.filename = "bad.mp3".into();
    item.extension = "mp3".into();
    item.player_type_id = kind;
    model.player.queue = vec![item];
    send(&mut model, PlayerMessage::PlayItem { item_id: "bad".into() });
    assert_eq!(model.player.session.status, "failed");
    assert!(model.player.session.error.is_some());
    assert_ne!(model.player.session.status, "playing");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn preview_play_drives_the_shared_cursor_without_opening_a_device() {
    assert!(!super::super::wav_player::sound_device_compiled_in());
    let bytes = pcm_wav(8_000, 1, 8, &[0, 255, 0, 255, 0, 255, 0, 255, 0, 255, 0, 255, 0, 255, 0, 255]);
    let parts = crate::shell::preview_media_parts("repo", &bytes).expect("预览");
    let (session, pcm) = (parts.session, parts.pcm);
    assert!(pcm.is_some());
    let mut model = shell(true);
    model.reduce(ShellMessage::AssetDetailLoaded(Ok(asset("audio/a.wav", "wav"))));
    let InspectEffect::LoadMedia { path, generation, .. } = model.inspect.take_effects().pop().unwrap() else {
        panic!("没有音视频请求");
    };
    model.reduce(ShellMessage::Inspect(InspectMessage::MediaLoaded { path, generation, result: Ok(session), pcm, frames: None }));
    assert!(model.player.preview_audio_armed());
    assert!(!model.player.preview_cursor_playing());
    model.reduce(ShellMessage::Inspect(InspectMessage::PlayPause));
    assert_eq!(model.inspect.media_session().map(|session| session.status.as_str()), Some("playing"));
    assert!(model.player.preview_cursor_playing());
    assert!(!super::super::wav_player::sound_device_compiled_in());
    send(&mut model, PlayerMessage::SetPlaying(false));
    assert_eq!(model.player.session.status, "paused");
    assert!(!model.player.preview_cursor_playing());
    assert_eq!(model.inspect.media_session().map(|session| session.status.as_str()), Some("paused"));
    send(&mut model, PlayerMessage::Seek(1));
    assert_eq!(model.inspect.media_session().map(|session| session.current_time_ms), Some(1));
    send(&mut model, PlayerMessage::SetPlaying(true));
    assert!(model.player.preview_cursor_playing());
}
