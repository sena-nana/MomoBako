//! 点开播放集时先画详情缓存，再拉新的。对齐 Vue `selectPlaylist`。
//!
//! 不打开声卡，也不建窗口。页面画什么以 `PlaylistPageView::project` 为准。

use super::super::super::player_view::playlist::PlaylistPageView;
use super::super::super::workspace::WorkspacePanel;
use super::super::super::{ShellMessage, SidebarEffect, SidebarMessage};
use super::{detail, item, load_list, queue_item, shell, summary};

/// 点开播放集：有缓存先画缓存，没有缓存不编一份；新详情回来再换；失败留着缓存；当前这一张也重拉。
/// 侧栏「播放」仍只改播放条。
#[test]
fn opening_a_playlist_paints_the_cached_detail_then_reloads() {
    let mut model = shell(true);
    load_list(
        &mut model,
        vec![
            summary("repo", "pl-a", "audio", "audio"),
            summary("repo", "pl-b", "audio", "audio"),
        ],
    );
    model.sidebar.take_effects();

    let mut cached = detail(
        "repo",
        "pl-a",
        "audio",
        "audio",
        vec![row("pl-a", "one", "ready")],
    );
    cached.playlist.name = "缓存甲".into();
    let mut fresh = detail(
        "repo",
        "pl-a",
        "audio",
        "audio",
        vec![row("pl-a", "two", "ready")],
    );
    fresh.playlist.name = "新的甲".into();
    let mut other = detail(
        "repo",
        "pl-b",
        "audio",
        "audio",
        vec![row("pl-b", "bee", "ready")],
    );
    other.playlist.name = "乙".into();

    assert!(
        !PlaylistPageView::project(&model).head.listed,
        "还没点开是空框"
    );

    open(&mut model, "pl-a");
    assert_eq!(
        loads(&mut model),
        vec![String::from("pl-a")],
        "没有缓存也要拉这一张"
    );
    assert!(
        !PlaylistPageView::project(&model).head.listed,
        "没有缓存不编一份播放集"
    );
    assert_eq!(model.selected_playlist_id.as_deref(), Some("pl-a"));
    assert_eq!(model.workspace.panel, WorkspacePanel::Playlist);

    model.reduce(ShellMessage::PlaylistDetailLoaded(Ok(cached.clone())));
    assert_eq!(shown(&model), ("缓存甲".into(), vec!["one.mp3".into()]));

    open(&mut model, "pl-b");
    assert_eq!(loads(&mut model), vec![String::from("pl-b")]);
    assert_eq!(
        shown(&model),
        ("缓存甲".into(), vec!["one.mp3".into()]),
        "没缓存时留着上一张，不编乙"
    );
    assert_eq!(
        model
            .player
            .listed
            .as_ref()
            .map(|page| page.playlist.playlist_id.as_str()),
        Some("pl-a")
    );
    assert_eq!(model.selected_playlist_id.as_deref(), Some("pl-b"));

    model.reduce(ShellMessage::PlaylistDetailLoaded(Ok(other.clone())));
    assert_eq!(shown(&model), ("乙".into(), vec!["bee.mp3".into()]));

    let mut night = queue_item("night", "mp3");
    night.playlist_id = "pl-b".into();
    model.player.queue = vec![night];
    model.player.current_id = Some("night".into());
    model.player.wants_playing = true;

    open(&mut model, "pl-a");
    assert_eq!(
        loads(&mut model),
        vec![String::from("pl-a")],
        "有缓存仍要重拉"
    );
    assert_eq!(
        shown(&model),
        ("缓存甲".into(), vec!["one.mp3".into()]),
        "先画缓存里的甲"
    );
    assert_eq!(model.playlist_item_ids, ["one"]);
    assert!(playing_night(&model), "先画缓存不改播放队列");

    model.reduce(ShellMessage::PlaylistDetailLoaded(Ok(fresh.clone())));
    assert_eq!(
        shown(&model),
        ("新的甲".into(), vec!["two.mp3".into()]),
        "新详情回来后换成这一份"
    );
    assert_eq!(model.playlist_item_ids, ["two"]);
    assert!(
        playing_night(&model),
        "正在放的是另一张时，换页上的详情不换队列"
    );

    open(&mut model, "pl-a");
    assert_eq!(
        loads(&mut model),
        vec![String::from("pl-a")],
        "点的就是当前这一张也重拉"
    );
    assert_eq!(shown(&model), ("新的甲".into(), vec!["two.mp3".into()]));

    model.reduce(ShellMessage::PlaylistDetailLoaded(Err(
        "读不到播放集".into()
    )));
    assert_eq!(
        shown(&model),
        ("新的甲".into(), vec!["two.mp3".into()]),
        "失败留着缓存，不假装读到了新的"
    );
    assert_eq!(
        model
            .status
            .failure()
            .map(|failure| failure.message.as_str()),
        Some("播放集操作失败：读不到播放集")
    );
    assert!(playing_night(&model));

    model.sidebar.pending_play = Some("pl-b".into());
    model.reduce(ShellMessage::PlaylistDetailLoaded(Ok(other)));
    assert_eq!(
        shown(&model),
        ("新的甲".into(), vec!["two.mp3".into()]),
        "侧栏播放不换正在看的页"
    );
    assert_eq!(model.selected_playlist_id.as_deref(), Some("pl-a"));
    assert_eq!(model.workspace.panel, WorkspacePanel::Playlist);
    assert_eq!(model.player.current_id.as_deref(), Some("bee"));
    assert!(
        model
            .player
            .queue
            .iter()
            .all(|item| item.playlist_id == "pl-b")
    );
}

fn row(
    playlist_id: &str,
    id: &str,
    status: &str,
) -> crate::backend::services::repository::PlaylistItem {
    let mut row = item(id, status);
    row.playlist_id = playlist_id.into();
    row
}

fn open(model: &mut super::super::super::ShellViewModel, playlist_id: &str) {
    model.reduce(ShellMessage::Sidebar(SidebarMessage::OpenSidebarPlaylist(
        playlist_id.into(),
    )));
}

fn loads(model: &mut super::super::super::ShellViewModel) -> Vec<String> {
    model
        .sidebar
        .take_effects()
        .into_iter()
        .filter_map(|effect| match effect {
            SidebarEffect::LoadPlaylistDetail { playlist_id, .. } => Some(playlist_id),
            _ => None,
        })
        .collect()
}

/// 页面上画出来的标题和条目文件名。
fn shown(model: &super::super::super::ShellViewModel) -> (String, Vec<String>) {
    let page = PlaylistPageView::project(model);
    assert!(page.head.listed, "播放集页应画着一份详情");
    let files = page
        .items
        .iter()
        .map(|entry| entry.filename.clone())
        .collect();
    (page.head.name, files)
}

fn playing_night(model: &super::super::super::ShellViewModel) -> bool {
    model.player.wants_playing
        && model.player.current_id.as_deref() == Some("night")
        && model
            .player
            .queue
            .iter()
            .all(|item| item.playlist_id == "pl-b")
}

/// 侧栏播放集列表刷新。照 Vue `refreshPlaylists` / `syncPlaylistMemberships`：
/// 当前这张还在时只换页眉名称，条目和播放队列留着；不在了就清掉已画出的详情，再点没有缓存的另一张停在空框；
/// 列表空了丢掉这个仓库的详情缓存，下次点开不先画出旧的那一份。
#[test]
fn refreshing_the_playlist_list_drops_a_missing_page_and_retitles_the_open_one() {
    let mut model = shell(true);
    load_list(
        &mut model,
        vec![
            summary("repo", "pl-a", "audio", "audio"),
            summary("repo", "pl-b", "audio", "audio"),
        ],
    );
    model.sidebar.take_effects();
    open(&mut model, "pl-a");
    let _ = loads(&mut model);
    let mut cached = detail(
        "repo",
        "pl-a",
        "audio",
        "audio",
        vec![row("pl-a", "one", "ready")],
    );
    cached.playlist.name = "缓存甲".into();
    model.reduce(ShellMessage::PlaylistDetailLoaded(Ok(cached)));
    assert_eq!(shown(&model), ("缓存甲".into(), vec!["one.mp3".into()]));

    let mut night = queue_item("night", "mp3");
    night.playlist_id = "pl-b".into();
    model.player.queue = vec![night];
    model.player.current_id = Some("night".into());
    model.player.wants_playing = true;

    let mut renamed = summary("repo", "pl-a", "audio", "audio");
    renamed.name = "新的甲".into();
    renamed.item_count = 9;
    load_list(
        &mut model,
        vec![renamed, summary("repo", "pl-b", "audio", "audio")],
    );
    assert_eq!(
        shown(&model),
        ("新的甲".into(), vec!["one.mp3".into()]),
        "还在列表里时只换页眉名称，条目不动"
    );
    assert!(
        PlaylistPageView::project(&model)
            .head
            .subline
            .starts_with("音频 · 1 项"),
        "页眉项数按画着的条目算，不用列表上的 9"
    );
    assert_eq!(model.workspace.panel, WorkspacePanel::Playlist);
    assert!(playing_night(&model), "换页眉名称不改播放队列");
    assert!(
        model
            .player
            .cached_playlist_detail("repo", "pl-a")
            .is_some_and(|detail| detail.playlist.name == "缓存甲"),
        "页眉换名不改写详情缓存"
    );

    load_list(&mut model, vec![summary("repo", "pl-b", "audio", "audio")]);
    assert_eq!(
        model.workspace.panel,
        WorkspacePanel::Files,
        "当前这张不在了，回到文件页"
    );
    assert!(model.sidebar.active_playlist_id.is_none());
    assert!(
        !PlaylistPageView::project(&model).head.listed,
        "已画出的详情清掉，停在空框"
    );
    assert!(model.playlist_item_ids.is_empty());
    assert!(
        model
            .player
            .cached_playlist_detail("repo", "pl-a")
            .is_some(),
        "列表里还有别的播放集时，不丢掉这一张的缓存"
    );
    assert!(playing_night(&model), "收起页面不改正在放的另一张");

    open(&mut model, "pl-b");
    assert_eq!(loads(&mut model), vec![String::from("pl-b")]);
    assert!(
        !PlaylistPageView::project(&model).head.listed,
        "没有缓存，上一张已经收起，不把甲再画出来"
    );
    assert_eq!(model.selected_playlist_id.as_deref(), Some("pl-b"));

    load_list(&mut model, Vec::new());
    assert!(
        model
            .player
            .cached_playlist_detail("repo", "pl-a")
            .is_none(),
        "列表空了丢掉这个仓库的详情缓存"
    );
    assert!(
        model
            .player
            .cached_playlist_detail("repo", "pl-b")
            .is_none()
    );
    assert_eq!(model.workspace.panel, WorkspacePanel::Files);
    assert!(playing_night(&model));

    let mut returned = summary("repo", "pl-a", "audio", "audio");
    returned.name = "又回来的甲".into();
    load_list(&mut model, vec![returned]);
    open(&mut model, "pl-a");
    assert_eq!(loads(&mut model), vec![String::from("pl-a")]);
    assert!(
        !PlaylistPageView::project(&model).head.listed,
        "缓存已丢，点开不先画出旧的甲"
    );
}
