//! 内置 WAV、压缩音频和媒体基础音频候选的认领与播放会话测试。
//!
//! 只解内存里的 PCM，测试构建不打开声卡。辅助构造沿用上层 `tests` 模块。

use super::super::super::{InspectEffect, InspectMessage, ShellMessage};
use super::super::support::{builtin_audio_formats, find_player_for_extension};
use super::super::wav_player::{builtin_candidates, candidates_for, SYSTEM_PLAYER_TYPE_ID};
use super::super::support::StoredSession;
use super::super::{PlaybackMode, PlayerEffect, PlayerMessage, QueueItem};
use super::{asset, detail, item, load_list, queue_item, send, shell, summary, temp_dir};

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
    assert_eq!(model.player.candidates, candidates_for(cfg!(windows)), "生产列表是本平台的内置候选");
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
    assert!(model.player.preview_owns_bar());
    assert!(!model.player.cursor_playing());
    send(&mut model, PlayerMessage::SetPlaying(true));
    assert_eq!(model.inspect.media_session().map(|session| session.status.as_str()), Some("playing"));
    assert!(model.player.cursor_playing());
    assert!(!super::super::wav_player::sound_device_compiled_in());
    send(&mut model, PlayerMessage::SetPlaying(false));
    assert_eq!(model.player.session.status, "paused");
    assert!(!model.player.cursor_playing());
    assert_eq!(model.inspect.media_session().map(|session| session.status.as_str()), Some("paused"));
    send(&mut model, PlayerMessage::Seek(1));
    assert_eq!(model.inspect.media_session().map(|session| session.current_time_ms), Some(1));
    send(&mut model, PlayerMessage::SetPlaying(true));
    assert!(model.player.cursor_playing());

    // 离开预览页：临时条目留作当前项继续出声，和 Vue 离开预览页后继续播放一致。
    model.reduce(ShellMessage::OpenDirectory(String::new()));
    assert!(!model.player.preview_owns_bar());
    assert!(model.player.cursor_playing());
    assert_eq!(model.player.current_item().map(|item| item.path.as_str()), Some("audio/a.wav"));
}

/// 本平台 Nana 自己能解的音频格式都有内置候选认领；没有媒体基础的平台不认领 m4a、aac、opus，
/// 设置页的内置解码器也只写出真能解的格式。
#[test]
fn builtin_candidates_claim_what_the_platform_decodes() {
    let claims = |media_foundation: bool, extension: &str| {
        let candidates = candidates_for(media_foundation);
        find_player_for_extension(extension, &candidates, &[]).is_some()
    };
    for extension in ["wav", "mp3", "flac", "ogg"] {
        assert!(claims(true, extension) && claims(false, extension), "{extension} 各平台都由内置解码器认领");
    }
    for extension in ["m4a", "aac", "opus"] {
        assert!(claims(true, extension), "有媒体基础时认领 {extension}");
        assert!(!claims(false, extension), "没有媒体基础时不认领 {extension}");
    }
    assert_eq!(builtin_audio_formats(&candidates_for(false)).as_deref(), Some("WAV / MP3 / FLAC / Ogg"));
    assert_eq!(builtin_audio_formats(&candidates_for(true)).as_deref(), Some("WAV / MP3 / FLAC / Ogg / M4A / AAC / Opus"));
    assert_eq!(builtin_candidates(), candidates_for(cfg!(windows)), "媒体基础只在 Windows 上有");
}

/// 没装音频插件时，m4a 预览解出音轨后由内置候选接管播放条，播放落在同一游标上；
/// 没有媒体基础的平台没有候选认领它，预览不接管播放条，写明没有可用的播放器。
#[test]
fn m4a_preview_takes_over_the_bar_without_the_audio_plugin() {
    // 媒体基础解出的音轨同样是交错 PCM；这里用同形的 WAV 字节代替，测试不依赖系统解码器。
    let bytes = pcm_wav(8_000, 1, 8, &[0, 255, 0, 255, 0, 255, 0, 255]);
    let preview = |media_foundation: bool| {
        let parts = crate::shell::preview_media_parts("repo", &bytes).expect("预览");
        let mut model = shell(true);
        model.player.candidates = candidates_for(media_foundation);
        assert!(model.player.contributions.is_empty(), "没装音频插件");
        model.reduce(ShellMessage::AssetDetailLoaded(Ok(asset("audio/voice.m4a", "m4a"))));
        let InspectEffect::LoadMedia { path, generation, .. } = model.inspect.take_effects().pop().unwrap() else {
            panic!("没有音视频请求");
        };
        let loaded = InspectMessage::MediaLoaded { path, generation, result: Ok(parts.session), pcm: parts.pcm, frames: None };
        model.reduce(ShellMessage::Inspect(loaded));
        model
    };
    let mut model = preview(true);
    assert!(model.player.preview_owns_bar());
    assert_eq!(model.player.current_item().map(|item| item.player_type_id.as_str()), Some(SYSTEM_PLAYER_TYPE_ID));
    send(&mut model, PlayerMessage::SetPlaying(true));
    assert!(model.player.cursor_playing());
    assert!(!super::super::wav_player::sound_device_compiled_in());

    let bare = preview(false);
    assert!(!bare.player.preview_owns_bar());
    assert!(bare.player.current_item().is_none());
    assert_eq!(bare.player.activity, "没有可用于播放此媒体的插件");
}

/// 播放时钟只有一份：每帧往前拨，到头按自然结束切到下一项并接着放。
#[test]
fn the_clock_advances_and_a_natural_end_moves_to_the_next_item() {
    let dir = temp_dir("wav-clock");
    let first = dir.join("first.wav");
    let second = dir.join("second.wav");
    let samples = [128u8; 160];
    std::fs::write(&first, pcm_wav(8_000, 1, 8, &samples)).unwrap();
    std::fs::write(&second, pcm_wav(8_000, 1, 8, &samples)).unwrap();
    let mut model = shell(true);
    model.player.repo_id = Some("repo".into());
    model.player.queue = vec![wav_queue_item("first", &first), wav_queue_item("second", &second)];
    send(&mut model, PlayerMessage::PlayItem { item_id: "first".into() });
    assert_eq!(model.player.session.status, "playing");
    assert_eq!(model.player.session.duration_ms, Some(20));
    assert!(crate::shell::poll_timers(&mut model));
    assert_eq!(model.player.session.current_time_ms, 16);
    assert!(crate::shell::poll_timers(&mut model));
    assert_eq!(model.player.current_id.as_deref(), Some("second"), "到头切下一项");
    super::super::fulfill_loads(&mut model);
    assert_eq!(model.player.session.status, "playing");
    assert_eq!(model.player.session.current_time_ms, 0);
    send(&mut model, PlayerMessage::SetPlaying(false));
    assert!(!crate::shell::poll_timers(&mut model), "暂停时时钟不动");
    let _ = std::fs::remove_dir_all(dir);
}

/// 存下的播放会话照 Vue `setActivePlaylist(.., { restore: true })` 恢复：装载期间播放条显示存下的进度，
/// 会话文件不被装载清零；装好以后能跳转就跳到存下的位置（超过时长时夹到结尾），不在播放就停在那里。
#[test]
fn restoring_a_session_resumes_at_the_stored_position() {
    let dir = temp_dir("wav-restore");
    let path = dir.join("tone.wav");
    std::fs::write(&path, pcm_wav(8_000, 1, 8, &[128u8; 8_000])).unwrap();
    let mut tone = item("tone", "ready");
    tone.path = path.display().to_string();
    tone.filename = "tone.wav".into();
    tone.extension = "wav".into();
    let playlist = detail("repo", "pl", "momobako.playlist.wav", "audio", vec![tone]);
    let stored = |position: u64| StoredSession {
        repo_id: "repo".into(),
        playlist_id: "pl".into(),
        player_type_id: "momobako.playlist.wav".into(),
        current_item_id: "tone".into(),
        current_time_ms: position,
        duration_ms: 1_000,
        mode: PlaybackMode::ListLoop,
        volume: 0.4,
        is_playing: false,
    };

    let mut model = shell(true);
    model.player.stored.insert("repo".into(), stored(600));
    load_list(&mut model, vec![summary("repo", "pl", "momobako.playlist.wav", "audio")]);
    assert!(model.player.take_effects().iter().any(|effect| matches!(effect, PlayerEffect::RestoreDetail { .. })));
    model.reduce(ShellMessage::Player(PlayerMessage::RestoreDetail(Ok(playlist.clone()))));
    assert_eq!(model.player.session.status, "loading");
    assert_eq!(model.player.time_text(), "0:00 / 0:01", "装载期间显示存下的进度和时长");
    assert_eq!(model.player.session.current_time_ms, 600);
    assert_eq!(model.player.stored["repo"].current_time_ms, 600, "装载不能把存下的进度清零");
    super::super::fulfill_loads(&mut model);
    assert_eq!(model.player.session.status, "paused", "存下时没在播放");
    assert_eq!(model.player.session.current_time_ms, 600, "装好以后接着存下的位置");
    assert_eq!(model.player.session.volume, 0.4);
    send(&mut model, PlayerMessage::SetPlaying(true));
    assert_eq!(model.player.session.status, "playing");

    let mut beyond = shell(true);
    beyond.player.stored.insert("repo".into(), stored(5_000));
    load_list(&mut beyond, vec![summary("repo", "pl", "momobako.playlist.wav", "audio")]);
    beyond.player.take_effects();
    send(&mut beyond, PlayerMessage::RestoreDetail(Ok(playlist)));
    assert_eq!(beyond.player.session.current_time_ms, 1_000, "超过时长时夹到结尾");
    let _ = std::fs::remove_dir_all(dir);
}
