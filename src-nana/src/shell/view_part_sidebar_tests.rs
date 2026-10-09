//! 常驻侧栏的回归：进设置页、换目录、后台读回和无关更新之后，侧栏的节点一个都不换，只有当前态、
//! 计数这些绑定的字段原地改；展开目录只新增它的子级，折叠只删掉子级；播放集列表增删只动那一行。
//! 每一步都和同一 ViewModel 新挂的文档一样，侧栏不重挂。

use nana_ui::runtime::{SemanticColorRole, StableNodeId};

use super::project::tests::tree_loaded;
use crate::shell::sidebar::SidebarPlaylist;
use crate::shell::view_harness::ShellHarness;
use crate::shell::{ShellMessage, ShellPage, ShellViewModel, SidebarMessage, ThumbnailFrame};

fn scene(name: &str) -> ShellViewModel {
    crate::shell::acceptance_gap_models()
        .into_iter()
        .find(|(scene, _)| *scene == name)
        .unwrap_or_else(|| panic!("没有场景 {name}"))
        .1
}

fn sidebar(message: SidebarMessage) -> ShellMessage {
    ShellMessage::Sidebar(message)
}

/// 侧栏根下面的全部节点，按文档顺序。
fn sidebar_nodes(harness: &ShellHarness) -> Vec<StableNodeId> {
    let root = harness.sidebar_root().expect("有侧栏");
    let world = harness.document().context().world();
    world
        .document_order(harness.document().document())
        .into_iter()
        .filter(|id| world.is_descendant_or_self(*id, root))
        .collect()
}

/// 节点的底色。
fn background(harness: &ShellHarness, id: StableNodeId) -> Option<SemanticColorRole> {
    harness.document().context().world().node_style(id).and_then(|style| style.background)
}

/// 带 `key` 的节点下面所有非空文字，按文档顺序。
fn texts_under(harness: &ShellHarness, key: &str) -> Vec<String> {
    let row = harness.keyed(key).unwrap_or_else(|| panic!("侧栏缺少 {key}"));
    let world = harness.document().context().world();
    world
        .document_order(harness.document().document())
        .into_iter()
        .filter(|id| world.is_descendant_or_self(*id, row))
        .filter_map(|id| world.text(id).filter(|text| !text.is_empty()).map(str::to_string))
        .collect()
}

/// 列表项是不是当前项。
fn selected(harness: &ShellHarness, key: &str) -> bool {
    let id = harness.keyed(key).unwrap_or_else(|| panic!("侧栏缺少 {key}"));
    harness.document().context().world().accessibility(id).and_then(|state| state.selected).unwrap_or(false)
}

/// 应用一条消息并刷新，断言侧栏根和侧栏下的节点一个都没换，再和新挂的文档比一次。
/// 重挂计数不分块（主区旧视图路由的重挂也算在里面），所以这里看节点身份。
fn apply_keeping_nodes(harness: &mut ShellHarness, message: ShellMessage, what: &str) {
    let root = harness.sidebar_root();
    let nodes = sidebar_nodes(harness);
    harness.apply(message);
    harness.flush();
    assert_eq!(harness.sidebar_root(), root, "{what}重挂了侧栏");
    assert_eq!(sidebar_nodes(harness), nodes, "{what}换掉了侧栏节点");
    harness.assert_same_as_fresh_mount();
}

/// 进设置页、换目录、目录树读回新计数和一串无关的后台消息：侧栏节点不换，只有绑定的字段原地改。
#[test]
fn routine_updates_keep_every_sidebar_node() {
    let mut harness = ShellHarness::mount(scene("live-files-plain"));
    harness.apply(sidebar(SidebarMessage::ToggleFolder("assets".into())));
    harness.flush();
    let root = harness.sidebar_root();

    let settings = harness.keyed("footer-settings").expect("设置入口");
    assert_eq!(background(&harness, settings), None);
    apply_keeping_nodes(&mut harness, ShellMessage::Navigate(ShellPage::Settings), "进设置页");
    assert_eq!(background(&harness, settings), Some(SemanticColorRole::AccentSoft), "设置入口没有亮起来");

    assert!(!selected(&harness, "folder-row-assets%2Fcovers"));
    apply_keeping_nodes(&mut harness, sidebar(SidebarMessage::OpenFolder("assets/covers".into())), "换目录");
    assert!(selected(&harness, "folder-row-assets%2Fcovers"), "covers 没有成为当前目录");
    assert!(!selected(&harness, "folder-row-assets"));

    assert_eq!(texts_under(&harness, "folder-row-assets%2Fcovers"), ["covers", "0"]);
    let message = tree_loaded(&harness.model, (3, 5));
    apply_keeping_nodes(&mut harness, message, "目录树读回");
    assert_eq!(texts_under(&harness, "folder-row-assets%2Fcovers"), ["covers", "5"], "covers 的计数没有原地更新");

    let unrelated = [
        ShellMessage::ThumbnailPixels(vec![ThumbnailFrame {
            path: "cover.png".into(),
            natural_width: 2,
            natural_height: 2,
            width: 2,
            height: 2,
            rgba: vec![255; 16],
        }]),
        ShellMessage::Player(crate::shell::player::PlayerMessage::SetVolume(0.5)),
        ShellMessage::Inspect(crate::shell::InspectMessage::SetQuery("封面".into())),
        ShellMessage::Navigate(ShellPage::FileList),
    ];
    for message in unrelated {
        apply_keeping_nodes(&mut harness, message, "无关更新");
    }
    assert_eq!(background(&harness, settings), None, "离开设置页后设置入口应恢复");
    assert_eq!(harness.sidebar_root(), root, "侧栏根不该换");
}

/// 展开目录只新增子级那一行，开合三角原地换图标和说明；折叠只删掉子级。
#[test]
fn expanding_a_folder_only_adds_its_children() {
    let mut harness = ShellHarness::mount(scene("live-files-plain"));
    let caret = harness.node("展开文件夹");
    let before = sidebar_nodes(&harness);
    assert!(harness.keyed("folder-row-assets%2Fcovers").is_none(), "收起时没有子级");

    harness.apply(sidebar(SidebarMessage::ToggleFolder("assets".into())));
    harness.flush();
    let after = sidebar_nodes(&harness);
    assert_eq!(harness.node("收起文件夹"), caret, "开合三角应原地换说明");
    let added = after.iter().filter(|id| !before.contains(id)).copied().collect::<Vec<_>>();
    assert!(before.iter().all(|id| after.contains(id)), "展开目录换掉了已有节点");
    let row = harness.keyed("folder-row-assets%2Fcovers").expect("展开后有 covers 行");
    let world = harness.document().context().world();
    let row_root = world.parent_id(row).expect("covers 行外框");
    assert!(
        added.iter().all(|id| world.is_descendant_or_self(*id, row_root)),
        "新增的节点应该都是 covers 那一行"
    );
    harness.assert_same_as_fresh_mount();

    harness.apply(sidebar(SidebarMessage::ToggleFolder("assets".into())));
    harness.flush();
    assert_eq!(sidebar_nodes(&harness), before, "折叠后应只删掉子级");
    assert_eq!(harness.node("展开文件夹"), caret);
    harness.assert_same_as_fresh_mount();
}

/// 展开的播放集列表：读回多一个播放集只新增那一行，删掉一个只删那一行；当前播放集原地换底色。
#[test]
fn playlist_rows_follow_the_list_by_id() {
    let mut harness = ShellHarness::mount(scene("live-files-plain"));
    harness.apply(sidebar(SidebarMessage::TogglePlaylists));
    harness.flush();
    let repo_id = harness.model.workspace.active_repo_id.clone().expect("场景有仓库");
    let playlist = |id: &str, name: &str| SidebarPlaylist {
        id: id.into(),
        name: name.into(),
        player_label: "音频播放器".into(),
        player_type_id: "momobako.playlist.audio".into(),
        item_count: 2,
    };
    let loaded = |playlists: Vec<SidebarPlaylist>| {
        sidebar(SidebarMessage::SidebarPlaylistsLoaded { repo_id: repo_id.clone(), result: Ok(playlists) })
    };
    harness.apply(loaded(vec![playlist("a", "晨间"), playlist("b", "夜晚")]));
    harness.flush();
    harness.assert_same_as_fresh_mount();
    let first = harness.keyed("playlist-item-a").expect("第一个播放集");
    let second = harness.keyed("playlist-item-b").expect("第二个播放集");
    let others = sidebar_nodes(&harness);

    harness.apply(loaded(vec![playlist("a", "晨间"), playlist("c", "午后"), playlist("b", "夜晚")]));
    harness.flush();
    assert_eq!(harness.keyed("playlist-item-a"), Some(first));
    assert_eq!(harness.keyed("playlist-item-b"), Some(second));
    assert!(harness.keyed("playlist-item-c").is_some(), "新播放集没有建出来");
    let now = sidebar_nodes(&harness);
    assert!(others.iter().all(|id| now.contains(id)), "插入一个播放集换掉了已有节点");
    harness.assert_same_as_fresh_mount();

    harness.apply(sidebar(SidebarMessage::OpenSidebarPlaylist("b".into())));
    harness.flush();
    assert_eq!(harness.keyed("playlist-item-b"), Some(second), "点开播放集不该重建那一行");
    assert_eq!(background(&harness, second), Some(SemanticColorRole::AccentSoft), "当前播放集没有换底色");
    assert_eq!(background(&harness, first), None);
    harness.assert_same_as_fresh_mount();

    harness.apply(loaded(vec![playlist("b", "夜晚")]));
    harness.flush();
    assert!(harness.keyed("playlist-item-a").is_none() && harness.keyed("playlist-item-c").is_none());
    assert_eq!(harness.keyed("playlist-item-b"), Some(second));
    harness.assert_same_as_fresh_mount();
}

/// 收起侧栏再展开：侧栏整块重挂一次，按当前状态建，信号不重建，之后照常只改字段。
#[test]
fn collapsing_and_expanding_rebuilds_from_the_signals() {
    let mut harness = ShellHarness::mount(scene("live-files-plain"));
    harness.apply(ShellMessage::ToggleSidebar);
    for _ in 0..30 {
        harness.frame();
    }
    assert!(harness.sidebar_root().is_none(), "收起后没有侧栏");
    harness.apply(sidebar(SidebarMessage::ToggleFolder("assets".into())));
    harness.apply(ShellMessage::ToggleSidebar);
    for _ in 0..30 {
        harness.frame();
    }
    assert!(harness.sidebar_root().is_some(), "展开后侧栏回来");
    assert!(harness.keyed("folder-row-assets%2Fcovers").is_some(), "收起期间展开的目录要跟上");
    harness.assert_same_as_fresh_mount();
    apply_keeping_nodes(&mut harness, ShellMessage::Navigate(ShellPage::Settings), "展开后进设置页");
}
