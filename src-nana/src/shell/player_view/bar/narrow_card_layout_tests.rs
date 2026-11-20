//! 窄窗口下播放条卡片排版的测试。

use crate::shell::ShellViewModel;
use crate::shell::view_harness::ShellHarness;

fn scene(name: &str) -> ShellViewModel {
    crate::shell::acceptance_gap_models()
        .into_iter()
        .find(|(scene, _)| *scene == name)
        .unwrap_or_else(|| panic!("没有场景 {name}"))
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
    let bounds = harness
        .document()
        .context()
        .world()
        .layout_box(id)
        .unwrap_or_else(|| panic!("{key} 没有布局盒"));
    (bounds.x, bounds.y, bounds.width, bounds.height)
}

fn centre_y((_, y, _, height): (f32, f32, f32, f32)) -> f32 {
    y + height / 2.0
}

/// 窗口宽于 1120、卡片窄于一行：仍是一行，音量越过卡片右内边距。
/// 只有窗口不宽于 1120px 才把三栏改成一列。
#[test]
fn a_wide_window_with_a_narrow_card_pushes_the_volume_out() {
    let mut harness = ShellHarness::mount_at(scene("files-multi-select"), 1180.0, 800.0);
    settle(&mut harness);
    assert!(harness.model.viewport_width > 1120.0);
    let width = harness.model.player.bar_width;
    assert!(
        width > 0.0 && width < 554.0,
        "卡片宽 {width}，这一档才该把音量挤出去"
    );
    let card = bounds(&harness, "file-player-slot");
    let media = bounds(&harness, "player-media");
    let play = bounds(&harness, "player-play");
    let volume = bounds(&harness, "player-volume");
    assert!(
        (centre_y(media) - centre_y(play)).abs() < 8.0,
        "传输应和媒体同一行：媒体 {media:?} 播放 {play:?}"
    );
    let volume_right = volume.0 + volume.2;
    let inner_right = card.0 + card.2 - 16.0;
    assert!(
        volume_right > inner_right,
        "音量应挤出卡片右内边距：音量 {volume:?} 卡片 {card:?}"
    );
}

/// 1200 宽的标准文件页卡片约 558：仍是一行，音量越过卡片右内边距。
/// 媒体栏下限 220 时，这一档的列下限之和已经超过卡片内容宽。
#[test]
fn the_standard_files_card_pushes_volume_past_the_padding() {
    let mut harness = ShellHarness::mount_at(scene("files-multi-select"), 1200.0, 800.0);
    settle(&mut harness);
    assert!(
        (harness.model.player.bar_width - 558.0).abs() < 1.0,
        "卡片宽 {}",
        harness.model.player.bar_width
    );
    let card = bounds(&harness, "file-player-slot");
    let media = bounds(&harness, "player-media");
    let play = bounds(&harness, "player-play");
    let volume = bounds(&harness, "player-volume");
    assert!(
        (centre_y(media) - centre_y(play)).abs() < 8.0,
        "标准文件页应留在一行：媒体 {media:?} 播放 {play:?}"
    );
    let volume_right = volume.0 + volume.2;
    let inner_right = card.0 + card.2 - 16.0;
    assert!(
        volume_right > inner_right,
        "音量应挤出卡片右内边距：音量 {volume:?} 卡片 {card:?}"
    );
}

/// `@media (max-width: 1120px)` 仍是三行，传输在媒体下面。
#[test]
fn the_viewport_media_query_still_stacks_three_rows() {
    let mut harness = ShellHarness::mount_at(scene("files-multi-select"), 1120.0, 800.0);
    settle(&mut harness);
    let media = bounds(&harness, "player-media");
    let play = bounds(&harness, "player-play");
    assert!(
        play.1 > media.1 + media.3 - 1.0,
        "1120 宽应是三行：媒体 {media:?} 播放 {play:?}"
    );
}
