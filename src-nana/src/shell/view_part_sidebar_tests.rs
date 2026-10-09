//! 常驻侧栏的回归：进设置页、换目录、后台读回和无关更新之后，侧栏的节点一个都不换，只有当前态、
//! 计数这些绑定的字段原地改；展开目录只新增它的子级，折叠只删掉子级；播放集列表增删只动那一行。
//! 每一步都和同一 ViewModel 新挂的文档一样，侧栏不重挂。

use nana_ui::runtime::{SemanticColorRole, StableNodeId};

use super::project::tests::tree_loaded;
use crate::backend::services::repository::PlaylistSummary;
use crate::shell::sidebar::{SidebarShortcut, SidebarSmartFolder};
use crate::shell::view_harness::ShellHarness;
use crate::shell::{ShellMessage, ShellViewModel, SidebarMessage, ThumbnailFrame};

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
    apply_keeping_nodes(&mut harness, ShellMessage::OpenSettings, "进设置页");
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
        // 侧栏点「全部」离开设置页，和 Vue 侧栏导航离开 `/settings` 一样。
        sidebar(SidebarMessage::SelectShortcut(crate::shell::ShortcutId::All)),
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
    let playlist = |id: &str, name: &str| PlaylistSummary {
        playlist_id: id.into(),
        repo_id: repo_id.clone(),
        name: name.into(),
        player_type_id: "momobako.playlist.audio".into(),
        player_plugin_id: "momobako.player.audio".into(),
        player_label: "音频播放器".into(),
        file_class: "audio".into(),
        item_count: 2,
        sort_order: 0,
        created_at: String::new(),
        updated_at: String::new(),
    };
    let loaded = |playlists: Vec<PlaylistSummary>| {
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

/// 收起侧栏再展开：侧栏不重建，收起时跟着工作区停放、照常写信号，展开后原样回来、跟上收起期间的变化。
#[test]
fn collapsing_and_expanding_keeps_the_sidebar() {
    let mut harness = ShellHarness::mount(scene("live-files-plain"));
    let root = harness.sidebar_root().expect("工作台里有侧栏");
    let switcher = harness.keyed("repository-switcher").expect("仓库头");
    harness.apply(ShellMessage::ToggleSidebar);
    for _ in 0..30 {
        harness.frame();
    }
    assert!(harness.keyed("repository-switcher").is_none(), "收起后侧栏不在文档里");
    harness.apply(sidebar(SidebarMessage::ToggleFolder("assets".into())));
    harness.apply(ShellMessage::ToggleSidebar);
    for _ in 0..30 {
        harness.frame();
    }
    assert_eq!(harness.sidebar_root(), Some(root), "展开后还是原来的侧栏");
    assert_eq!(harness.keyed("repository-switcher"), Some(switcher), "侧栏里的节点不该换");
    assert!(harness.keyed("folder-row-assets%2Fcovers").is_some(), "收起期间展开的目录要跟上");
    harness.assert_same_as_fresh_mount();
    apply_keeping_nodes(&mut harness, ShellMessage::OpenSettings, "展开后进设置页");
}

/// 智能文件夹树：读回两层，展开只新增子级；点开子级时它成为当前项、父级换成打开的图标，节点不换；
/// 提交中时行上的编辑原地禁用。
#[test]
fn smart_folder_rows_patch_in_place() {
    let mut harness = ShellHarness::mount(scene("live-files-plain"));
    let repo_id = harness.model.workspace.active_repo_id.clone().expect("场景有仓库");
    let folder = |id: &str, name: &str, children: Vec<SidebarSmartFolder>| SidebarSmartFolder {
        id: id.into(),
        parent_id: None,
        name: name.into(),
        filter: Default::default(),
        children,
    };
    let tree = vec![folder("root", "高评分", vec![folder("child", "本周", Vec::new())])];
    harness.apply(sidebar(SidebarMessage::SidebarSmartFoldersLoaded { repo_id, result: Ok(tree) }));
    harness.flush();
    harness.assert_same_as_fresh_mount();
    let root = harness.keyed("smart-row-root").expect("顶层智能文件夹");
    assert!(harness.keyed("smart-row-child").is_none(), "收起时没有子级");
    let caret = harness.node("展开智能文件夹");

    let before = sidebar_nodes(&harness);
    harness.apply(sidebar(SidebarMessage::ToggleSmartFolder("root".into())));
    harness.flush();
    assert_eq!(harness.node("收起智能文件夹"), caret, "开合三角应原地换说明");
    let after = sidebar_nodes(&harness);
    assert!(before.iter().all(|id| after.contains(id)), "展开换掉了已有节点");
    assert!(harness.keyed("smart-row-child").is_some(), "展开后有子级");
    harness.assert_same_as_fresh_mount();

    apply_keeping_nodes(&mut harness, sidebar(SidebarMessage::OpenSmartFolder("child".into())), "点开智能文件夹");
    assert!(selected(&harness, "smart-row-child"));
    assert!(!selected(&harness, "smart-row-root"));
    assert_eq!(harness.keyed("smart-row-root"), Some(root));

    harness.model.sidebar.smart_draft.busy = true;
    harness.sync();
    harness.flush();
    let world = harness.document().context().world();
    let edits = harness
        .nodes()
        .into_iter()
        .filter(|node| node.label.as_deref() == Some("编辑智能文件夹"))
        .map(|node| world.accessibility(node.id).is_some_and(|state| state.disabled))
        .collect::<Vec<_>>();
    assert_eq!(edits, [true, true], "提交中时每一行的编辑都该禁用");
    harness.assert_same_as_fresh_mount();
}

/// 快捷访问：有书签时整组出现，书签内容变了只换那一行。
#[test]
fn quick_access_rows_follow_the_snapshot() {
    let mut harness = ShellHarness::mount(scene("live-files-plain"));
    assert!(harness.find("快捷访问").is_none(), "没有书签时不显示快捷访问");
    let shortcut = |id: &str, label: &str, kind: &str| SidebarShortcut {
        id: id.into(),
        label: label.into(),
        target_kind: kind.into(),
        target_path: Some("assets".into()),
        target_id: None,
    };
    harness.model.sidebar.quick_access = vec![shortcut("a", "素材", "directory"), shortcut("b", "封面", "file")];
    harness.sync();
    harness.flush();
    assert!(harness.find("快捷访问").is_some(), "有书签时显示快捷访问");
    let first = harness.keyed("quick-access-a").expect("第一个书签");
    let second = harness.keyed("quick-access-b").expect("第二个书签");
    harness.assert_same_as_fresh_mount();

    harness.model.sidebar.quick_access[1].label = "封面图".into();
    harness.sync();
    harness.flush();
    assert_eq!(harness.keyed("quick-access-a"), Some(first), "没变的书签不该重建");
    assert_ne!(harness.keyed("quick-access-b"), Some(second), "改了名字的书签整行重建");
    assert!(harness.find("封面图").is_some());
    harness.assert_same_as_fresh_mount();
}
