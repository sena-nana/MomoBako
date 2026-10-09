//! 常驻播放集路由的回归：无关更新一个节点都不换、不重挂；换当前播放只改字段、播放条岛按它读到的值重建；
//! 详情读回时空框和面板原地互换，播放条岛这时才有内容；不可播放项目那一行按字段显隐；拖动排序和移除
//! （点击、回车）只动变了的行；动效帧不重挂；每一步都和同一 ViewModel 新挂的文档一样。

use nana_ui::runtime::StableNodeId;

use crate::backend::services::repository::{SystemLogLocation, SystemLogRecord, SystemLogSource};
use crate::shell::host_events::HostMessage;
use crate::shell::player::PlayerMessage;
use crate::shell::view_harness::ShellHarness;
use crate::shell::view_part_primary::RouteKey;
use crate::shell::{InspectMessage, ShellMessage, ShellPage, ShellViewModel, WorkspacePanel};

fn scene(name: &str) -> ShellViewModel {
    crate::shell::acceptance_gap_models()
        .into_iter()
        .find(|(scene, _)| *scene == name)
        .unwrap_or_else(|| panic!("没有场景 {name}"))
        .1
}

/// 一条实时日志：和播放集页无关的后台消息。
fn log_message() -> ShellMessage {
    ShellMessage::Host(HostMessage::LogRecorded(SystemLogRecord {
        id: "log-playlist".into(),
        timestamp: "2026-10-09T08:00:00Z".into(),
        level: "info".into(),
        category: "repository".into(),
        action: "sync".into(),
        message: "后台同步完成".into(),
        source: SystemLogSource { kind: "host".into(), label: Some("MomoBako".into()), plugin_id: None, repo_id: None },
        location: SystemLogLocation::default(),
        context: serde_json::json!({}),
    }))
}

/// 点开了演示播放列表、两首曲子、没有开始播放。
fn listed() -> ShellHarness {
    let harness = ShellHarness::mount(scene("playlist-open"));
    assert_eq!(RouteKey::of(&harness.model), RouteKey::Playlists);
    harness
}

/// 无关的后台消息和筛选栏开合：分支、条目列表、条目行和播放条一个都不换，也不重挂。
#[test]
fn unrelated_updates_keep_every_playlist_node() {
    let mut harness = listed();
    let keys = ["playlist-page", "playlist-reorder", "playlist-item-item-01", "playlist-remove-item-02", "player-title", "workspace-filter-bar"];
    let nodes = keys.map(|key| harness.keyed(key).unwrap_or_else(|| panic!("缺少 {key}")));
    let branch = harness.route_branch();
    let remounts = harness.view_stats().remounts;
    for message in [
        log_message(),
        ShellMessage::Inspect(InspectMessage::ToggleFilterBar),
        ShellMessage::Inspect(InspectMessage::ToggleFilterBar),
        log_message(),
    ] {
        harness.apply(message);
        harness.flush();
        assert_eq!(keys.map(|key| harness.keyed(key)), nodes.map(Some), "无关更新换掉了播放集页的节点");
        harness.assert_same_as_fresh_mount();
    }
    assert_eq!(harness.route_branch(), branch, "常驻路由的分支不该重挂");
    assert_eq!(harness.view_stats().remounts, remounts, "无关更新不该重挂任何内容");
}

/// 开始播放第二条：条目列表和行不换，列表里选中的条目原地换；播放条读到的当前项变了，岛重建一次。
#[test]
fn playing_an_item_keeps_the_list_and_rebuilds_only_the_player_island() {
    let mut harness = listed();
    let list = harness.keyed("playlist-reorder").expect("条目列表");
    let row = harness.keyed("playlist-item-item-02").expect("第二条");
    let card = harness.keyed("player-title").expect("播放条");
    let branch = harness.route_branch();
    let remounts = harness.view_stats().remounts;

    harness.apply(ShellMessage::Player(PlayerMessage::PlayListed { item_id: Some("item-02".into()) }));
    harness.flush();
    assert_eq!(harness.keyed("playlist-reorder"), Some(list), "换当前播放不该重建条目列表");
    assert_eq!(harness.keyed("playlist-item-item-02"), Some(row));
    assert_eq!(harness.route_branch(), branch);
    assert_ne!(harness.keyed("player-title"), Some(card), "播放条读到的当前项变了，岛要重建");
    assert_eq!(harness.view_stats().remounts, remounts + 1, "只重建播放条岛");
    harness.assert_same_as_fresh_mount();

    let rebuilt = harness.view_stats().remounts;
    for _ in 0..8 {
        harness.frame();
    }
    assert_eq!(harness.view_stats().remounts, rebuilt, "动效帧和播放推进不该重挂");
}

/// 详情还没读回时是「选择一个播放集」空框、岛里没有播放条；读回后空框和面板原地互换，播放条岛有了内容。
#[test]
fn listing_swaps_the_empty_frame_and_fills_the_player_island() {
    let open = scene("playlist-open");
    let detail = open.player.listed.clone().expect("演示播放列表的详情");
    let mut harness = ShellHarness::mount(ShellViewModel::for_page(ShellPage::Playlists));
    assert_eq!(RouteKey::of(&harness.model), RouteKey::Playlists);
    assert!(harness.find("选择一个播放集").is_some(), "没点开时显示空框");
    assert!(harness.keyed("player-title").is_none(), "没点开时岛里没有播放条");
    let empty = harness.keyed("playlist-page-empty-frame").expect("空框");
    let slot = harness.keyed("playlist-player-slot").expect("播放条的占位节点");
    let branch = harness.route_branch();

    harness.apply(ShellMessage::PlaylistDetailLoaded(Ok(detail)));
    harness.flush();
    assert_eq!(harness.route_branch(), branch, "读回详情不该重挂分支");
    assert_eq!(harness.keyed("playlist-page-empty-frame"), Some(empty));
    assert_eq!(harness.keyed("playlist-player-slot"), Some(slot));
    assert!(harness.find("选择一个播放集").is_none(), "读回后空框藏起来");
    assert!(harness.find("演示播放列表").is_some(), "读回后显示播放集标题");
    assert!(harness.keyed("player-title").is_some(), "读回后岛里有播放条");
    harness.assert_same_as_fresh_mount();
}

/// 有不能播放的条目时多一行「不可播放项目」，原因按字段改；都能播放时那一行藏起来。
#[test]
fn unplayable_items_toggle_their_status_line_in_place() {
    let mut harness = listed();
    let line = harness.keyed("playlist-item-status").expect("不可播放项目的行");
    assert!(harness.find("不可播放项目：track-01.mp3: 文件不存在").is_none());

    let mut detail = harness.model.player.listed.clone().expect("详情");
    detail.items[0].status = "missing".into();
    detail.items[0].status_reason = Some("文件不存在".into());
    harness.apply(ShellMessage::PlaylistDetailLoaded(Ok(detail.clone())));
    harness.flush();
    assert_eq!(harness.keyed("playlist-item-status"), Some(line), "状态行不该重建");
    assert!(harness.find("不可播放项目：track-01.mp3: 文件不存在").is_some(), "状态行没写出原因");
    harness.assert_same_as_fresh_mount();

    detail.items[0].status = "ready".into();
    detail.items[0].status_reason = None;
    harness.apply(ShellMessage::PlaylistDetailLoaded(Ok(detail)));
    harness.flush();
    assert_eq!(harness.keyed("playlist-item-status"), Some(line));
    assert!(harness.find("不可播放项目：track-01.mp3: 文件不存在").is_none(), "都能播放时状态行藏起来");
    harness.assert_same_as_fresh_mount();
}

/// 条目列表里各行的节点，按显示顺序。
fn item_rows(harness: &ShellHarness) -> Vec<StableNodeId> {
    let list = harness.keyed("playlist-reorder").expect("条目列表");
    harness.document().context().world().node(list).map(|node| node.children.to_vec()).unwrap_or_default()
}

/// 壳层存好以后读回的详情：照 `edit` 改过的条目。
fn reload(harness: &mut ShellHarness, edit: impl FnOnce(&mut Vec<crate::backend::services::repository::PlaylistItem>)) {
    let mut detail = harness.model.player.listed.clone().expect("详情");
    edit(&mut detail.items);
    harness.apply(ShellMessage::PlaylistDetailLoaded(Ok(detail)));
    harness.flush();
}

/// 拖动排序：按住第二条的拖动柄拖到第一条上面，列表发排序消息；读回新顺序以后两行都是原来的节点，
/// 只换了位置，列表也不重建。
#[test]
fn dragging_an_item_moves_its_row_without_rebuilding() {
    let mut harness = listed();
    let list = harness.keyed("playlist-reorder").expect("条目列表");
    let [first, second] = ["playlist-item-item-01", "playlist-item-item-02"].map(|key| harness.keyed(key).expect("条目行"));
    assert_eq!(item_rows(&harness), [first, second]);
    harness.take_messages();
    let grip = harness.center(harness.keyed("playlist-item-drag-item-02").expect("拖动柄"));
    let top = harness.document().context().world().layout_box(first).expect("第一条的盒子").y + 4.0;
    let messages = harness.drag_messages(grip, (grip.0, top));
    let reorder = messages.iter().any(|message| {
        matches!(message, ShellMessage::Player(PlayerMessage::Reorder { source, before })
            if source == "item-02" && before.as_deref() == Some("item-01"))
    });
    assert!(reorder, "拖到第一条上面应该发排序消息：{} 条消息", messages.len());

    reload(&mut harness, |items| items.reverse());
    assert_eq!(harness.keyed("playlist-reorder"), Some(list), "排序不该重建列表");
    assert_eq!(item_rows(&harness), [second, first], "两行只换位置，节点不换");
    harness.assert_same_as_fresh_mount();
}

/// 行里的按钮不被拖动手势抓走：点标题打开预览。移除：点第一条的「移除」、焦点在第二条的「移除」上
/// 按回车，都发移除消息；读回以后只拿掉那一行，留下的行和按钮还是原来的节点。
#[test]
fn removing_items_by_click_and_keyboard_keeps_the_other_rows() {
    let mut harness = listed();
    let second = harness.keyed("playlist-item-item-02").expect("第二条");
    harness.take_messages();
    let title = harness.center(harness.keyed("playlist-item-title-item-02").expect("第二条的标题"));
    let messages = harness.click_messages(title.0, title.1);
    assert!(
        matches!(messages.as_slice(), [ShellMessage::Player(PlayerMessage::OpenPreview { item_id: Some(id) })] if id == "item-02"),
        "点标题应该打开这一条的预览：{} 条消息",
        messages.len()
    );
    let remove_first = harness.center(harness.keyed("playlist-remove-item-01").expect("第一条的移除"));
    let messages = harness.click_messages(remove_first.0, remove_first.1);
    assert!(
        matches!(messages.as_slice(), [ShellMessage::RemovePlaylistItem { item_id, .. }] if item_id == "item-01"),
        "点「移除」应该只发一条移除消息：{} 条消息",
        messages.len()
    );
    reload(&mut harness, |items| items.retain(|item| item.playlist_item_id != "item-01"));
    assert_eq!(item_rows(&harness), [second], "只拿掉被移除的那一行");
    harness.assert_same_as_fresh_mount();

    let remove_second = harness.keyed("playlist-remove-item-02").expect("第二条的移除");
    harness.focus(remove_second);
    let messages = harness.key_messages("Enter");
    assert!(
        matches!(messages.as_slice(), [ShellMessage::RemovePlaylistItem { item_id, .. }] if item_id == "item-02"),
        "焦点在「移除」上按回车应该发移除消息：{} 条消息",
        messages.len()
    );
}

/// 换到文件面板再回来：分支重建，回来以后条目、播放条岛和新挂的一样；停在播放集页、面板换成别的时只剩外框。
#[test]
fn leaving_and_returning_rebuilds_a_matching_branch() {
    let mut harness = listed();
    harness.apply(ShellMessage::SetWorkspacePanel(WorkspacePanel::Files));
    harness.flush();
    assert_eq!(RouteKey::of(&harness.model), RouteKey::Files);
    harness.assert_same_as_fresh_mount();

    harness.apply(ShellMessage::SetWorkspacePanel(WorkspacePanel::Playlist));
    harness.flush();
    assert_eq!(RouteKey::of(&harness.model), RouteKey::Playlists);
    assert!(harness.keyed("player-title").is_some(), "回来以后播放条岛要放回去");
    harness.assert_same_as_fresh_mount();

    harness.apply(ShellMessage::SetWorkspacePanel(WorkspacePanel::Search));
    harness.apply(ShellMessage::Navigate(ShellPage::Playlists));
    harness.flush();
    assert_eq!(RouteKey::of(&harness.model), RouteKey::Playlists, "停在播放集页");
    assert!(harness.find("演示播放列表").is_none(), "不在播放集面板时不显示播放表面");
    assert!(harness.keyed("player-title").is_none(), "不在播放集面板时岛里没有播放条");
    harness.assert_same_as_fresh_mount();
}
