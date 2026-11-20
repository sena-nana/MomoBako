//! 拖动进度条的测试。

use nana_ui::InputModifiers;

use super::super::super::player::{PlayerMessage, QueueItem};
use crate::shell::ShellMessage;
use crate::shell::view_harness::ShellHarness;

fn scene() -> crate::shell::ShellViewModel {
    crate::shell::acceptance_gap_models()
        .into_iter()
        .find(|(name, _)| *name == "files-multi-select")
        .expect("没有 files-multi-select")
        .1
}

fn settle(harness: &mut ShellHarness) {
    for _ in 0..6 {
        harness.flush();
        for message in harness.take_messages() {
            harness.apply(message);
        }
    }
    harness.flush();
}

fn bounds(harness: &ShellHarness, key: &str) -> (f32, f32, f32, f32) {
    let id = harness.keyed(key).unwrap_or_else(|| panic!("没有 {key}"));
    let box_ = harness
        .document()
        .context()
        .world()
        .layout_box(id)
        .unwrap_or_else(|| panic!("{key} 没有布局盒"));
    (box_.x, box_.y, box_.width, box_.height)
}

fn seek_positions(messages: &[ShellMessage]) -> Vec<u64> {
    messages
        .iter()
        .filter_map(|message| match message {
            ShellMessage::Player(PlayerMessage::Seek(position_ms)) => Some(*position_ms),
            _ => None,
        })
        .collect()
}

/// 拖进度条还没松手就要跳转：拖动中每一格都 seek。
#[test]
fn dragging_the_seek_bar_seeks_before_release() {
    let mut harness = ShellHarness::mount(scene());
    settle(&mut harness);
    harness.model.player.queue = vec![QueueItem {
        id: "tone".into(),
        playlist_id: "list".into(),
        asset_id: "asset".into(),
        path: "music/tone.wav".into(),
        filename: "tone.wav".into(),
        extension: "wav".into(),
        status: "ready".into(),
        status_reason: None,
        transient: true,
        player_type_id: "momobako.playlist.wav".into(),
        player_label: "WAV".into(),
        file_class: "audio".into(),
        thumbnail_path: None,
        is_virtual: false,
        local_absolute_path: None,
    }];
    harness.model.player.current_id = Some("tone".into());
    harness.model.player.session.duration_ms = Some(10_000);
    harness.model.player.session.status = "paused".into();
    harness.sync();
    harness.flush();

    let seek = bounds(&harness, "player-seek");
    assert!(seek.2 > 40.0, "进度条宽 {}", seek.2);
    let y = seek.1 + seek.3 / 2.0;
    let plain = InputModifiers::default();
    harness.pointer(nana_ui::PointerPhase::Down, seek.0 + 4.0, y, plain);
    harness.pointer(nana_ui::PointerPhase::Move, seek.0 + seek.2 - 4.0, y, plain);
    let during = seek_positions(&harness.take_messages());
    assert!(
        during.iter().any(|position_ms| *position_ms > 0),
        "拖动中应跳转，实际 {during:?}"
    );
    harness.pointer(nana_ui::PointerPhase::Up, seek.0 + seek.2 - 4.0, y, plain);
}

fn volume_levels(messages: &[ShellMessage]) -> Vec<f32> {
    messages
        .iter()
        .filter_map(|message| match message {
            ShellMessage::Player(PlayerMessage::SetVolume(volume)) => Some(*volume),
            _ => None,
        })
        .collect()
}

/// 拖音量滑杆还没松手就要写入。松手不再把同一格写第二次。
#[test]
fn dragging_the_volume_slider_sets_volume_before_release() {
    // 1200 宽时音量滑杆伸出文件卡片，右端盖在详情上，点不到。放宽窗口让整条滑杆留在卡片里。
    let mut harness = ShellHarness::mount_at(scene(), 1800.0, 800.0);
    settle(&mut harness);
    harness.model.player.queue = vec![QueueItem {
        id: "tone".into(),
        playlist_id: "list".into(),
        asset_id: "asset".into(),
        path: "music/tone.wav".into(),
        filename: "tone.wav".into(),
        extension: "wav".into(),
        status: "ready".into(),
        status_reason: None,
        transient: true,
        player_type_id: "momobako.playlist.wav".into(),
        player_label: "WAV".into(),
        file_class: "audio".into(),
        thumbnail_path: None,
        is_virtual: false,
        local_absolute_path: None,
    }];
    harness.model.player.current_id = Some("tone".into());
    harness.model.player.session.volume = 1.0;
    harness.model.player.session.status = "paused".into();
    harness.sync();
    harness.flush();

    let slider = bounds(&harness, "player-volume");
    let card = bounds(&harness, "file-player-slot");
    assert!(slider.2 > 40.0, "音量滑杆宽 {}", slider.2);
    assert!(
        slider.0 >= card.0 && slider.0 + slider.2 <= card.0 + card.2 + 1.0,
        "音量应整段落在卡片里：滑杆 {slider:?} 卡片 {card:?}"
    );
    let y = slider.1 + slider.3 / 2.0;
    let plain = InputModifiers::default();
    harness.pointer(
        nana_ui::PointerPhase::Down,
        slider.0 + slider.2 - 6.0,
        y,
        plain,
    );
    harness.pointer(nana_ui::PointerPhase::Move, slider.0 + 6.0, y, plain);
    let during = volume_levels(&harness.take_messages());
    let last = *during
        .last()
        .unwrap_or_else(|| panic!("拖动中应写入音量，实际 {during:?}"));
    assert!(last < 0.99, "应从满音量往下调，实际 {during:?}");
    harness.pointer(nana_ui::PointerPhase::Up, slider.0 + 6.0, y, plain);
    let released = volume_levels(&harness.take_messages());
    assert!(
        released
            .iter()
            .all(|volume| (*volume - last).abs() > 0.0001),
        "松手不应再写一次 {last}，实际 {released:?}"
    );
}

fn duration_ms(messages: &[ShellMessage]) -> Vec<u32> {
    messages
        .iter()
        .filter_map(|message| match message {
            ShellMessage::Player(PlayerMessage::SetImageDuration(ms)) => Some(*ms),
            _ => None,
        })
        .collect()
}

/// 拖图片停留时长还没松手就要写入。松手不再把同一格写第二次。
#[test]
fn dragging_the_duration_slider_sets_duration_before_release() {
    let mut harness = ShellHarness::mount_at(scene(), 1800.0, 800.0);
    settle(&mut harness);
    harness.model.player.contributions.push(
        crate::backend::services::repository::PlaylistPlayerContribution {
            player_type_id: "momobako.playlist.image-slideshow".into(),
            label: "图片幻灯片".into(),
            file_class: "image".into(),
            supported_extensions: vec!["png".into()],
            supports_seek: false,
            supports_volume: false,
            supports_preview_navigation: true,
            description: None,
        },
    );
    harness.model.player.queue = vec![QueueItem {
        id: "still".into(),
        playlist_id: "list".into(),
        asset_id: "asset".into(),
        path: "media/cover.png".into(),
        filename: "cover.png".into(),
        extension: "png".into(),
        status: "ready".into(),
        status_reason: None,
        transient: true,
        player_type_id: "momobako.playlist.image-slideshow".into(),
        player_label: "图片幻灯片".into(),
        file_class: "image".into(),
        thumbnail_path: None,
        is_virtual: false,
        local_absolute_path: None,
    }];
    harness.model.player.current_id = Some("still".into());
    harness.model.player.settings.image_duration_ms = 2_000;
    harness.sync();
    harness.flush();

    let slider = bounds(&harness, "player-duration");
    let card = bounds(&harness, "file-player-slot");
    assert!(slider.2 > 40.0, "停留时长滑杆宽 {}", slider.2);
    assert!(
        slider.0 >= card.0 && slider.0 + slider.2 <= card.0 + card.2 + 1.0,
        "停留时长应落在卡片里：滑杆 {slider:?} 卡片 {card:?}"
    );
    let y = slider.1 + slider.3 / 2.0;
    let plain = InputModifiers::default();
    harness.pointer(
        nana_ui::PointerPhase::Down,
        slider.0 + slider.2 - 6.0,
        y,
        plain,
    );
    let during = duration_ms(&harness.take_messages());
    let last = *during
        .last()
        .unwrap_or_else(|| panic!("拖动中应写入停留时长，实际 {during:?}"));
    assert!(last > 2_000, "应从 2 秒往上调，实际 {during:?}");
    harness.pointer(
        nana_ui::PointerPhase::Up,
        slider.0 + slider.2 - 6.0,
        y,
        plain,
    );
    let released = duration_ms(&harness.take_messages());
    assert!(
        released.iter().all(|ms| *ms != last),
        "松手不应再写一次 {last}，实际 {released:?}"
    );
}
