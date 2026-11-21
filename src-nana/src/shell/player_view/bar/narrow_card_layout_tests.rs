//! 播放条卡片排版的测试：宽窗是媒体行加一行传输条，窗口不宽于 1120px 时传输条换成 `Stacked`，
//! 整张卡片是媒体、进度、按钮三行。

use nana_ui::runtime::{Entity, MediaTransportBar, MediaTransportSlots, StableNodeId};

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

type Bounds = (f32, f32, f32, f32);

fn bounds_of(harness: &ShellHarness, id: StableNodeId) -> Bounds {
    let bounds = harness
        .document()
        .context()
        .world()
        .layout_box(id)
        .unwrap_or_else(|| panic!("{id:?} 没有布局盒"));
    (bounds.x, bounds.y, bounds.width, bounds.height)
}

fn bounds(harness: &ShellHarness, key: &str) -> Bounds {
    let id = harness.keyed(key).unwrap_or_else(|| panic!("没有 {key}"));
    bounds_of(harness, id)
}

/// 传输条自己装配的节点：进度、音量弹层的触发器。
pub(super) fn transport_slots(harness: &ShellHarness) -> MediaTransportSlots {
    let id = harness
        .keyed("player-transport-bar")
        .expect("没有 player-transport-bar");
    harness
        .document()
        .context()
        .read(Entity::<MediaTransportBar>::from_stable_id(id), |bar| {
            bar.slots().clone()
        })
        .expect("读得到传输条")
}

fn centre_y((_, y, _, height): Bounds) -> f32 {
    y + height / 2.0
}

/// 宽窗：媒体行在上，传输按钮、进度和音量在下面同一行，都留在卡片里，不再从右边挤出去。
#[test]
fn a_wide_window_keeps_transport_seek_and_volume_on_one_row_inside_the_card() {
    for width in [1180.0, 1200.0] {
        let mut harness = ShellHarness::mount_at(scene("files-multi-select"), width, 800.0);
        settle(&mut harness);
        assert!(harness.model.viewport_width > 1120.0);
        let card = bounds(&harness, "file-player-slot");
        let media = bounds(&harness, "player-media");
        let play = bounds(&harness, "player-play");
        let slots = transport_slots(&harness);
        let seek = bounds_of(&harness, slots.seek.expect("进度"));
        let volume = bounds_of(&harness, slots.volume_menu.expect("音量弹层"));
        assert!(
            play.1 >= media.1 + media.3 - 1.0,
            "{width}：传输在媒体行下面：媒体 {media:?} 播放 {play:?}"
        );
        assert!(
            (centre_y(play) - centre_y(volume)).abs() < 8.0,
            "{width}：播放和音量同一行：播放 {play:?} 音量 {volume:?}"
        );
        assert!(
            seek.0 > play.0 + play.2 && seek.0 + seek.2 < volume.0 && seek.2 > 40.0,
            "{width}：进度夹在按钮和音量中间：播放 {play:?} 进度 {seek:?} 音量 {volume:?}"
        );
        assert!(
            volume.0 + volume.2 <= card.0 + card.2 + 0.5,
            "{width}：音量留在卡片里：音量 {volume:?} 卡片 {card:?}"
        );
    }
}

/// `@media (max-width: 1120px)`：媒体、进度、按钮三行。
#[test]
fn the_viewport_media_query_still_stacks_three_rows() {
    let mut harness = ShellHarness::mount_at(scene("files-multi-select"), 1120.0, 800.0);
    settle(&mut harness);
    let media = bounds(&harness, "player-media");
    let play = bounds(&harness, "player-play");
    let seek = bounds_of(&harness, transport_slots(&harness).seek.expect("进度"));
    assert!(
        seek.1 > media.1 + media.3 - 1.0,
        "1120 宽进度在媒体下面：媒体 {media:?} 进度 {seek:?}"
    );
    assert!(
        play.1 > seek.1 + seek.3 - 1.0,
        "1120 宽按钮在进度下面：进度 {seek:?} 播放 {play:?}"
    );
}
