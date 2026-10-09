//! 播放列表当前项。只解正在播的视频或 m4a，测试不打开声卡。

use crate::backend::services::repository::PlaylistPlayerContribution;

use super::super::{ShellMessage, ShellViewModel};
use super::{PlayerMessage, QueueItem};

/// 发一条播放消息。当前项的读取请求像 `player_dispatch` 一样当场读文件送回。
fn send(model: &mut ShellViewModel, message: PlayerMessage) {
    model.reduce(ShellMessage::Player(message));
    super::fulfill_loads(model);
}

fn item(id: &str, path: &str, extension: &str) -> QueueItem {
    QueueItem {
        id: id.into(),
        playlist_id: "pl".into(),
        asset_id: format!("asset-{id}"),
        path: path.into(),
        filename: format!("{id}.{extension}"),
        extension: extension.into(),
        status: "ready".into(),
        status_reason: None,
        transient: false,
        player_type_id: "momobako.playlist.video".into(),
        player_label: "视频".into(),
        file_class: "video".into(),
        thumbnail_path: None,
    }
}

fn pcm_wav() -> Vec<u8> {
    let data = [128u8; 8];
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
fn current_m4a_enters_the_session_without_a_sound_device() {
    assert!(!super::wav_player::sound_device_compiled_in());
    let dir = std::env::temp_dir().join(format!("nana-clip-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let current = dir.join("tone.m4a");
    std::fs::write(&current, pcm_wav()).unwrap();
    let other = "DO-NOT-DECODE-other-queue.mp4";
    let mut model = ShellViewModel::default();
    model.player.repo_id = Some("repo".into());
    model.player.queue = vec![item("tone", &current.display().to_string(), "m4a"), item("other", other, "mp4")];
    send(&mut model, PlayerMessage::PlayItem { item_id: "missing".into() });
    assert_eq!(model.player.session.status, "idle");
    assert!(model.player.session.error.is_none());
    assert_eq!(model.player.activity, "当前没有可播放条目");

    send(&mut model, PlayerMessage::PlayItem { item_id: "tone".into() });
    assert_eq!(model.player.session.status, "playing");
    assert!(model.player.session.duration_ms.unwrap_or(0) > 0);
    assert!(model.player.session.can_seek && model.player.session.can_volume);
    assert!(model.player.session.error.is_none());
    assert!(!model.player.activity.contains(other));

    let bad = dir.join("bad.m4a");
    std::fs::write(&bad, b"\x00\x00\x00\x18ftypM4A \x00\x00\x00\x00M4A mp42").unwrap();
    model.player.queue = vec![item("bad", &bad.display().to_string(), "m4a"), item("other", other, "mp4")];
    send(&mut model, PlayerMessage::PlayItem { item_id: "bad".into() });
    assert_eq!(model.player.session.status, "failed");
    let error = model.player.session.error.clone().unwrap_or_default();
    assert!(error.contains("解码失败"), "{error}");
    assert!(!error.contains("没有原生解码器"), "{error}");
    assert!(!error.contains(other), "{error}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn still_png_uses_real_pixels_and_a_missing_file_fails_without_a_frame() {
    assert!(!super::wav_player::sound_device_compiled_in());
    let dir = std::env::temp_dir().join(format!("nana-still-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("dot.png");
    let image = image::RgbaImage::from_pixel(2, 3, image::Rgba([8, 16, 32, 255]));
    image.save(&path).expect("写入测试图片");

    let mut model = ShellViewModel::default();
    model.player.candidates.clear();
    model.player.contributions = vec![slideshow()];
    model.player.repo_id = Some("repo".into());
    model.player.queue = vec![still_item("dot", &path.display().to_string())];
    send(&mut model, PlayerMessage::PlayItem { item_id: "dot".into() });
    assert_eq!(model.player.session.status, "playing");
    assert_eq!(model.player.session.duration_ms, Some(5000));
    assert!(!model.player.session.can_seek && !model.player.session.can_volume);
    assert!(model.player.session.error.is_none());
    let frame = model.player.still.as_ref().and_then(|item| item.frame.as_ref()).expect("画面");
    assert_eq!((frame.width, frame.height), (2, 3));
    assert!(!model.player.activity.contains("需要升级"));

    model.player.queue = vec![still_item("gone", "pics/missing.png")];
    send(&mut model, PlayerMessage::PlayItem { item_id: "gone".into() });
    assert_eq!(model.player.session.status, "failed");
    let error = model.player.session.error.clone().unwrap_or_default();
    assert!(error.contains("图片无法播放"), "{error}");
    assert!(!error.contains("需要升级"), "{error}");
    assert!(model.player.still.as_ref().is_some_and(|item| item.path == "pics/missing.png" && item.frame.is_none()));

    model.player.contributions[0].supported_extensions = vec!["wma".into()];
    let mut audio = still_item("side", "notes/side.wma");
    audio.extension = "wma".into();
    audio.file_class = "audio".into();
    audio.player_type_id = "momobako.playlist.image-slideshow".into();
    model.player.queue = vec![audio];
    send(&mut model, PlayerMessage::PlayItem { item_id: "side".into() });
    assert!(model.player.still.is_none());
    assert_eq!(model.player.session.duration_ms, None);
    let note = model.player.outside_note.clone().unwrap_or_default();
    assert!(note.contains("wma"), "{note}");
    assert!(!note.contains("需要升级"), "{note}");
    let _ = std::fs::remove_dir_all(dir);
}

fn slideshow() -> PlaylistPlayerContribution {
    PlaylistPlayerContribution {
        player_type_id: "momobako.playlist.image-slideshow".into(),
        label: "图片幻灯片".into(),
        file_class: "image".into(),
        supported_extensions: vec!["png".into()],
        supports_seek: false,
        supports_volume: false,
        supports_preview_navigation: true,
        description: None,
    }
}

fn still_item(id: &str, path: &str) -> QueueItem {
    let mut entry = item(id, path, "png");
    entry.player_type_id = "momobako.playlist.image-slideshow".into();
    entry.player_label = "图片幻灯片".into();
    entry.file_class = "image".into();
    entry
}

/// 条目路径是仓库内相对路径：播放只留下经仓库服务读取的请求，不直接读盘。
/// 读取结果按代次装进会话，过期的结果不装；装好后按播放意图开始播放。
#[test]
fn relative_item_paths_load_through_the_repository() {
    let mut model = ShellViewModel::default();
    model.player.candidates.clear();
    model.player.contributions = vec![PlaylistPlayerContribution {
        player_type_id: "momobako.playlist.audio-sequence".into(),
        label: "音频顺序播放".into(),
        file_class: "audio".into(),
        supported_extensions: vec!["mp3".into()],
        supports_seek: true,
        supports_volume: true,
        supports_preview_navigation: true,
        description: None,
    }];
    model.player.repo_id = Some("repo".into());
    let mut track = item("a", "music/a.mp3", "mp3");
    track.player_type_id = "momobako.playlist.audio-sequence".into();
    track.file_class = "audio".into();
    model.player.queue = vec![track];
    model.reduce(ShellMessage::Player(PlayerMessage::PlayItem { item_id: "a".into() }));
    assert_eq!(model.player.session.status, "loading");
    let effects = model.player.take_effects();
    let Some(super::PlayerEffect::LoadItem { repo_id, path, still, generation, .. }) =
        effects.iter().find(|effect| matches!(effect, super::PlayerEffect::LoadItem { .. })).cloned()
    else {
        panic!("播放没有留下读取请求");
    };
    assert_eq!((repo_id.as_str(), path.as_str(), still), ("repo", "music/a.mp3", false));

    model.reduce(ShellMessage::Player(PlayerMessage::ItemLoaded {
        item_id: "a".into(),
        generation: generation + 1,
        still: false,
        result: Err("过期".into()),
    }));
    assert_eq!(model.player.session.status, "loading", "过期的读取结果不装");

    let loaded = super::decode_loaded("repo", false, "mp3", crate::shell::audio_decode::fixture_mp3()).expect("解码");
    model.reduce(ShellMessage::Player(PlayerMessage::ItemLoaded { item_id: "a".into(), generation, still: false, result: Ok(loaded) }));
    assert_eq!(model.player.session.status, "playing");
    assert!(model.player.session.duration_ms.is_some_and(|duration| duration > 0));
    assert!(model.player.can_play);

    send(&mut model, PlayerMessage::SetPlaying(false));
    assert_eq!(model.player.session.status, "paused", "暂停落在同一游标上，不报缺解码器");
    assert!(model.player.session.error.is_none());
}
