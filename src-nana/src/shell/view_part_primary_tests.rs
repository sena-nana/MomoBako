//! 主区块的回归：路由键怎么取、路由容器和整列填充一样排、换路由只换主区当前分支，
//! 侧栏、浮层和主区外框都不动。

use nana_ui::runtime::Stack;

use super::RouteKey;
use crate::shell::view_harness::ShellHarness;
use crate::shell::{ShellMessage, ShellPage, ShellViewModel, WorkspacePanel};

fn scene(name: &str) -> ShellViewModel {
    crate::shell::acceptance_gap_models()
        .into_iter()
        .find(|(scene, _)| *scene == name)
        .unwrap_or_else(|| panic!("没有场景 {name}"))
        .1
}

#[test]
fn route_keys_follow_startup_page_region_and_panel() {
    assert_eq!(RouteKey::of(&ShellViewModel::for_page(ShellPage::Loading)), RouteKey::Startup);
    assert_eq!(RouteKey::of(&ShellViewModel::for_page(ShellPage::Error)), RouteKey::Startup);
    assert_eq!(RouteKey::of(&ShellViewModel::for_page(ShellPage::Settings)), RouteKey::Settings);
    assert_eq!(RouteKey::of(&ShellViewModel::for_page(ShellPage::FileList)), RouteKey::Files);
    assert_eq!(RouteKey::of(&ShellViewModel::for_page(ShellPage::Playlists)), RouteKey::Playlists);
    assert_eq!(RouteKey::of(&ShellViewModel::for_page(ShellPage::EmptyRepository)), RouteKey::Empty);
    assert_eq!(RouteKey::of(&scene("missing")), RouteKey::Missing);
    assert_eq!(RouteKey::of(&scene("search-results")), RouteKey::Search);

    let mut model = scene("live-files-plain");
    for (panel, route) in [
        (WorkspacePanel::Logs, RouteKey::Logs),
        (WorkspacePanel::Extensions, RouteKey::Extensions),
        (WorkspacePanel::Actions, RouteKey::Actions),
        (WorkspacePanel::Playlist, RouteKey::Playlists),
        (WorkspacePanel::Trash, RouteKey::Files),
    ] {
        model.workspace.panel = panel;
        assert_eq!(RouteKey::of(&model), route, "{panel:?}");
    }

    let mut empty = ShellViewModel::for_page(ShellPage::EmptyRepository);
    empty.workspace.panel = WorkspacePanel::Search;
    assert_eq!(RouteKey::of(&empty), RouteKey::EmptySearch);
}

/// 路由容器夹在主区外框和分支之间，排版要和 `Stack::fill_column(0)` 一样，分支才和原来一样占满外框。
#[test]
fn the_route_container_lays_out_like_a_fill_column() {
    let harness = ShellHarness::mount(scene("live-files-plain"));
    let container = harness.keyed("primary-route").expect("主区路由容器");
    let style = harness.document().context().world().node_style(container).expect("路由容器的样式").clone();
    let expected = Stack::fill_column(0.0).node_style();
    assert_eq!(*style.layout, *expected.layout, "路由容器的排版和整列填充不同");
}

/// 从搜索结果换到播放集：只有路由容器里的分支换了，主区外框、路由容器、侧栏和标题栏的节点都不动，
/// 也没有浮层；换完和同一 ViewModel 新挂的一样。
#[test]
fn switching_routes_rebuilds_only_the_current_primary_branch() {
    let mut harness = ShellHarness::mount(scene("search-results"));
    assert_eq!(RouteKey::of(&harness.model), RouteKey::Search);
    let sidebar = harness.sidebar_root().expect("有侧栏");
    let (overlay, stage) = harness.content_roots();
    let container = harness.keyed("primary-route").expect("路由容器");
    let branch = harness.route_branch();
    let search = harness.input("全局搜索");
    let switcher = harness.keyed("repository-switcher").expect("仓库头");
    assert!(overlay.is_none(), "场景里不该有浮层");

    harness.apply(ShellMessage::Navigate(ShellPage::Playlists));
    assert_eq!(RouteKey::of(&harness.model), RouteKey::Playlists);
    harness.flush();

    assert_ne!(harness.route_branch(), branch, "换路由后分支应该换掉");
    assert_eq!(harness.content_roots(), (None, stage), "主区外框和浮层不该动");
    assert_eq!(harness.keyed("primary-route"), Some(container), "路由容器不该重建");
    assert_eq!(harness.sidebar_root(), Some(sidebar), "侧栏不该重挂");
    assert_eq!(harness.keyed("repository-switcher"), Some(switcher), "侧栏里的节点不该换");
    assert_eq!(harness.input("全局搜索"), search, "标题栏不该动");
    harness.assert_same_as_fresh_mount();

    harness.apply(ShellMessage::Navigate(ShellPage::FileList));
    harness.flush();
    assert_eq!(RouteKey::of(&harness.model), RouteKey::Search);
    assert_eq!(harness.sidebar_root(), Some(sidebar), "换回来侧栏也不该重挂");
    harness.assert_same_as_fresh_mount();
}

/// 收起侧栏换成主区独占：外框整块重挂，外面多一层壳层底色；再展开回到工作台。每步都和新挂的一样。
#[test]
fn a_layout_change_remounts_the_primary_frame() {
    let mut harness = ShellHarness::mount(scene("live-files-plain"));
    let (_, stage) = harness.content_roots();
    harness.apply(ShellMessage::ToggleSidebar);
    for _ in 0..30 {
        harness.frame();
    }
    assert!(harness.sidebar_root().is_none(), "收起后没有侧栏");
    assert_ne!(harness.content_roots().1, stage, "排法变了外框要重挂");
    harness.assert_same_as_fresh_mount();
    harness.apply(ShellMessage::ToggleSidebar);
    for _ in 0..30 {
        harness.frame();
    }
    assert!(harness.sidebar_root().is_some(), "展开后侧栏回来");
    harness.assert_same_as_fresh_mount();
}
