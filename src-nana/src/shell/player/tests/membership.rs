//! 播放集成员变了以后的刷新：右键加入、移出，以及按路径加入目录和虚拟条目。
//!
//! 照 Vue `refreshPlaylists` 重读播放集列表（侧栏项数）和成员索引；点开的那张受影响时页面跟着刷新，
//! 不受影响时不换掉页面上点开的那张。辅助构造沿用上层 `tests` 模块，不启动仓库服务。

use crate::backend::services::repository::PlaylistMembershipSnapshot;
use crate::shell::SidebarEffect;
use crate::shell::view_part_sidebar::project::SidebarView;

use super::super::super::{ShellMessage, ShellViewModel, SidebarMessage};
use super::super::{PlayerEffect, PlayerMessage};
use super::{contribution, detail, item, load_list, send, shell, summary};

/// 「pl」点开着、有一条「a」；「pl2」没点开、有一条。播放器类型认得 mp3。
fn opened() -> ShellViewModel {
    let mut model = shell(true);
    model.reduce(ShellMessage::PlaylistPlayersLoaded(Ok(vec![contribution(
        "audio",
        "audio",
        &["mp3"],
    )])));
    load_list(
        &mut model,
        vec![
            summary("repo", "pl", "audio", "audio"),
            summary("repo", "pl2", "audio", "audio"),
        ],
    );
    model.reduce(ShellMessage::PlaylistDetailLoaded(Ok(detail(
        "repo",
        "pl",
        "audio",
        "audio",
        vec![item("a", "ready")],
    ))));
    model.player.take_effects();
    model.sidebar.take_effects();
    model
}

/// 侧栏这一轮要重读的播放集列表和详情。
fn playlist_loads(model: &mut ShellViewModel) -> Vec<SidebarEffect> {
    model
        .sidebar
        .take_effects()
        .into_iter()
        .filter(|effect| {
            matches!(
                effect,
                SidebarEffect::LoadPlaylists { .. } | SidebarEffect::LoadPlaylistDetail { .. }
            )
        })
        .collect()
}

/// 侧栏这一行的「播放器 · N 项」。
fn meta(model: &ShellViewModel, playlist_id: &str) -> String {
    SidebarView::project(model)
        .playlist_rows
        .into_iter()
        .find(|row| row.playlist_id == playlist_id)
        .map(|row| row.meta)
        .unwrap_or_default()
}

fn reload_list() -> SidebarEffect {
    SidebarEffect::LoadPlaylists {
        repo_id: "repo".into(),
    }
}

/// 右键加入点开的那张：重读播放集列表和它的详情，页面多一条、侧栏项数跟着变。移出没点开的那张：只重读列表，
/// 读回以后侧栏项数和成员索引都换新。
#[test]
fn toggling_membership_refreshes_counts_and_the_open_playlist() {
    let mut model = opened();
    let saved = PlaylistMembershipSnapshot {
        asset_id: "asset-b".into(),
        playlist_ids: vec!["pl".into()],
    };
    send(&mut model, PlayerMessage::MembershipSaved(Ok(saved)));
    assert_eq!(
        playlist_loads(&mut model),
        [
            reload_list(),
            SidebarEffect::LoadPlaylistDetail {
                repo_id: "repo".into(),
                playlist_id: "pl".into()
            }
        ]
    );
    model.reduce(ShellMessage::PlaylistDetailLoaded(Ok(detail(
        "repo",
        "pl",
        "audio",
        "audio",
        vec![item("a", "ready"), item("b", "ready")],
    ))));
    assert_eq!(model.playlist_item_ids, ["a", "b"], "页面上多了加入的一条");
    assert_eq!(meta(&model, "pl"), "音频 · 2 项");

    model
        .player
        .memberships
        .insert("asset-c".into(), vec!["pl2".into()]);
    let removed = PlaylistMembershipSnapshot {
        asset_id: "asset-c".into(),
        playlist_ids: Vec::new(),
    };
    send(&mut model, PlayerMessage::MembershipSaved(Ok(removed)));
    assert_eq!(
        playlist_loads(&mut model),
        [reload_list()],
        "没点开的那张只重读列表"
    );
    let mut emptied = summary("repo", "pl2", "audio", "audio");
    emptied.item_count = 0;
    let mut open = summary("repo", "pl", "audio", "audio");
    open.item_count = 2;
    model.reduce(ShellMessage::Sidebar(
        SidebarMessage::SidebarPlaylistsLoaded {
            repo_id: "repo".into(),
            result: Ok(vec![open, emptied]),
        },
    ));
    assert_eq!(meta(&model, "pl2"), "音频 · 0 项");
    assert!(
        matches!(
            model.player.take_effects().last(),
            Some(PlayerEffect::LoadMemberships { .. })
        ),
        "成员索引也重读"
    );

    send(
        &mut model,
        PlayerMessage::MembershipSaved(Err("写入失败".into())),
    );
    assert!(playlist_loads(&mut model).is_empty(), "写入失败不重读");
}

/// 按路径加入（目录、虚拟条目）没点开的那张：页面上点开的还是原来那张，只重读播放集列表和成员索引。加到点开的这张：
/// 换上读回的详情。失败进状态区。
#[test]
fn adding_by_path_refreshes_without_switching_the_open_playlist() {
    let mut model = opened();
    send(
        &mut model,
        PlayerMessage::ToggleMembership {
            playlist_id: "pl2".into(),
            kind: "directory".into(),
            extension: String::new(),
            asset_id: String::new(),
            is_virtual: false,
            path: "albums".into(),
        },
    );
    assert!(matches!(
        model.player.take_effects().pop(),
        Some(PlayerEffect::AddByPaths(_))
    ));
    let other = detail(
        "repo",
        "pl2",
        "audio",
        "audio",
        vec![item("x", "ready"), item("y", "ready")],
    );
    model.reduce(ShellMessage::Player(PlayerMessage::PathsAdded(Ok(other))));
    assert_eq!(
        model
            .player
            .listed
            .as_ref()
            .map(|listed| listed.playlist.playlist_id.as_str()),
        Some("pl"),
        "页面上点开的还是 pl"
    );
    assert_eq!(model.selected_playlist_id.as_deref(), Some("pl"));
    assert_eq!(model.playlist_item_ids, ["a"]);
    assert_eq!(playlist_loads(&mut model), [reload_list()]);

    let grown = detail(
        "repo",
        "pl",
        "audio",
        "audio",
        vec![item("a", "ready"), item("z", "ready")],
    );
    model.reduce(ShellMessage::Player(PlayerMessage::PathsAdded(Ok(grown))));
    assert_eq!(
        model.playlist_item_ids,
        ["a", "z"],
        "加到点开的这张：换上读回的详情"
    );
    assert_eq!(meta(&model, "pl"), "音频 · 2 项");
    assert_eq!(playlist_loads(&mut model), [reload_list()]);

    model.reduce(ShellMessage::Player(PlayerMessage::PathsAdded(Err(
        "磁盘满".into(),
    ))));
    assert_eq!(
        model
            .status
            .failure()
            .map(|failure| failure.message.as_str()),
        Some("加入播放集失败：磁盘满")
    );
    assert!(playlist_loads(&mut model).is_empty());
}

/// 播放集页移除或排序交回详情以后，照 Vue `refreshPlaylists` 重读播放集列表和成员索引。
/// 页面条目用这次交回的，不另读详情。点开别的播放集以后，侧栏项数仍是重读回来的数。
/// 失败和过期的结果不重读。直接读详情（点开播放集）也不重读列表。
#[test]
fn page_remove_and_reorder_refresh_the_list_without_rereading_the_detail() {
    let mut model = opened();
    let emptied = detail("repo", "pl", "audio", "audio", vec![]);
    model.reduce(ShellMessage::Player(PlayerMessage::ItemsChanged(Ok(
        emptied,
    ))));
    assert!(
        model
            .player
            .listed
            .as_ref()
            .is_some_and(|listed| listed.items.is_empty()),
        "页面换成交回的空列表"
    );
    assert!(model.playlist_item_ids.is_empty());
    assert_eq!(meta(&model, "pl"), "音频 · 0 项");
    assert_eq!(
        playlist_loads(&mut model),
        [reload_list()],
        "只重读列表，不另读详情"
    );

    let mut emptied_summary = summary("repo", "pl", "audio", "audio");
    emptied_summary.item_count = 0;
    let other = summary("repo", "pl2", "audio", "audio");
    model.reduce(ShellMessage::Sidebar(
        SidebarMessage::SidebarPlaylistsLoaded {
            repo_id: "repo".into(),
            result: Ok(vec![emptied_summary, other]),
        },
    ));
    assert!(
        matches!(
            model.player.take_effects().last(),
            Some(PlayerEffect::LoadMemberships { .. })
        ),
        "列表读回以后重读成员索引"
    );

    model.reduce(ShellMessage::PlaylistDetailLoaded(Ok(detail(
        "repo",
        "pl2",
        "audio",
        "audio",
        vec![item("b", "ready")],
    ))));
    assert_eq!(
        meta(&model, "pl"),
        "音频 · 0 项",
        "离开这一张以后项数仍是重读回来的 0"
    );
    assert!(
        playlist_loads(&mut model).is_empty(),
        "点开另一张不重读列表"
    );

    let reordered = detail("repo", "pl2", "audio", "audio", vec![item("b", "ready")]);
    model.reduce(ShellMessage::Player(PlayerMessage::ItemsChanged(Ok(
        reordered,
    ))));
    assert_eq!(model.playlist_item_ids, ["b"]);
    assert_eq!(playlist_loads(&mut model), [reload_list()]);

    model.reduce(ShellMessage::Player(PlayerMessage::ItemsChanged(Err(
        "移除失败".into(),
    ))));
    assert_eq!(
        model
            .status
            .failure()
            .map(|failure| failure.message.as_str()),
        Some("播放集操作失败：移除失败")
    );
    assert_eq!(model.playlist_item_ids, ["b"], "失败不换掉页面上的条目");
    assert!(playlist_loads(&mut model).is_empty());

    let foreign = detail("repo-b", "pl", "audio", "audio", vec![]);
    model.reduce(ShellMessage::Player(PlayerMessage::ItemsChanged(Ok(
        foreign,
    ))));
    assert_eq!(
        model
            .player
            .listed
            .as_ref()
            .map(|listed| listed.playlist.repo_id.as_str()),
        Some("repo")
    );
    assert!(
        playlist_loads(&mut model).is_empty(),
        "别的仓库的结果不重读"
    );
}
