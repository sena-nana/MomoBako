//! 主区块的回归：路由键怎么取、路由容器和整列填充一样排、换路由只换主区当前分支，
//! 侧栏、浮层和主区外框都不动。

use nana_ui::runtime::{Entity, ScrollOffset, ScrollView, Stack};

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

/// 收起侧栏换成主区独占、再展开回到工作台：主区外框挪进壳层底色又挪回工作区，外框、分支、输入框和
/// 侧栏一个都不重建，滚动位置和输入框的焦点留着；每一步都和新挂的一样。
#[test]
fn a_layout_change_moves_the_frame_without_rebuilding() {
    let mut harness = ShellHarness::mount(ShellViewModel::for_page(ShellPage::Settings));
    let (_, stage) = harness.content_roots();
    let stage = stage.expect("主区外框");
    let sidebar = harness.sidebar_root().expect("工作台里有侧栏");
    let branch = harness.route_branch();
    let input = harness.input_within("admin-plugin-keyword");
    let scroll = Entity::<ScrollView>::from_stable_id(harness.keyed("settings-scroll").expect("设置页滚动区"));
    harness.focus(input);
    harness.window.document.context_mut().scroll_to(scroll, ScrollOffset { x: 0.0, y: 200.0 }).expect("滚动");
    harness.flush();
    let scrolled = |harness: &ShellHarness| harness.document().context().world().scroll_offset(scroll.stable_id()).map_or(0.0, |offset| offset.y);
    let offset = scrolled(&harness);
    assert!(offset > 100.0, "设置页要能滚：{offset}");
    let remounts = harness.view_stats().remounts;

    for expanded in [false, true] {
        harness.apply(ShellMessage::ToggleSidebar);
        for _ in 0..30 {
            harness.frame();
        }
        let solo = harness.content_roots().1.expect("主区槽位里的根");
        if expanded {
            assert_eq!(solo, stage, "展开后外框自己回到工作区的主区");
            assert_eq!(harness.sidebar_root(), Some(sidebar), "展开后侧栏原样回来，不重建");
            assert!(harness.keyed("repository-switcher").is_some());
        } else {
            assert_ne!(solo, stage, "主区独占时槽位里是壳层底色");
            let world = harness.document().context().world();
            assert_eq!(world.parent_id(stage), Some(solo), "外框挪进壳层底色");
            assert!(harness.keyed("repository-switcher").is_none(), "收起后侧栏不在文档里");
        }
        assert_eq!(harness.route_branch(), branch, "换排法不该重建路由分支");
        assert_eq!(harness.input_within("admin-plugin-keyword"), input, "换排法不该重建输入框");
        assert_eq!(harness.focused(), Some(input), "换排法以后焦点回到原来的输入框");
        assert!((scrolled(&harness) - offset).abs() < 0.5, "换排法以后滚动位置留着");
        assert_eq!(harness.view_stats().remounts, remounts, "换排法不该重挂任何内容");
        // 滚动位置是界面状态，新挂的文档在顶上：滚回顶上和新挂的比，再滚回去接着换排法。
        harness.window.document.context_mut().scroll_to(scroll, ScrollOffset { x: 0.0, y: 0.0 }).expect("滚回顶上");
        harness.assert_same_as_fresh_mount();
        harness.window.document.context_mut().scroll_to(scroll, ScrollOffset { x: 0.0, y: offset }).expect("滚回原处");
        harness.flush();
    }
}
