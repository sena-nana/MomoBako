//! 弹层和右键菜单常驻的回归：打开期间无关更新不换节点；任务进度、仓库列表、子菜单展开和
//! 文件服务忙这些相关变化只改绑定的字段或变了的那一行；点外面只发一条关闭消息。浮层层常驻在
//! AppShell 的 overlay 槽位里，有块时挡点击、空了放开，增删块不用重新装配；右键菜单不靠显式层级。

use nana_ui::runtime::{component_descriptors, AppShell, ContextMenu, Entity, PositionSpec, StableNodeId};

use super::OverlayKey;
use crate::backend::services::repository::TaskProgressSnapshot;
use crate::shell::files::{FileDialog, FilesMessage};
use crate::shell::player::PlayerMessage;
use crate::shell::view_harness::ShellHarness;
use crate::shell::workspace::WorkspaceRepository;
use crate::shell::{AdminMessage, GapMessage, InspectMessage, ShellMessage, ShellViewModel, SidebarMessage};

fn scene(name: &str) -> ShellViewModel {
    crate::shell::acceptance_gap_models()
        .into_iter()
        .find(|(scene, _)| *scene == name)
        .unwrap_or_else(|| panic!("没有场景 {name}"))
        .1
}

/// 和弹层无关的消息：播放音量和标题栏搜索。
fn unrelated() -> Vec<ShellMessage> {
    vec![ShellMessage::Player(PlayerMessage::SetVolume(0.4)), ShellMessage::Inspect(InspectMessage::SetQuery("封面".into()))]
}

fn task(id: &str, percent: f32) -> TaskProgressSnapshot {
    TaskProgressSnapshot {
        task_id: id.into(),
        protocol_id: "momobako.sync".into(),
        status: "running".into(),
        phase: Some("scanning".into()),
        label: Some(format!("扫描 {id}")),
        current: None,
        total: None,
        percent: Some(percent),
        error: None,
        updated_at: "1".into(),
    }
}

/// 归约之外直接改了 ViewModel：照生产标脏（状态版本加一），同步并刷新一帧。
fn changed(harness: &mut ShellHarness) {
    harness.model.mark_surface_dirty();
    harness.sync();
    harness.flush();
}

/// 浮层根下面所有节点，按文档顺序。
fn subtree(harness: &ShellHarness, root: StableNodeId) -> Vec<StableNodeId> {
    let world = harness.document().context().world();
    let mut nodes = Vec::new();
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        nodes.push(id);
        if let Some(node) = world.node(id) {
            stack.extend(node.children.iter().rev().copied());
        }
    }
    nodes
}

/// 文档里的 AppShell 和它的 overlay 槽位（浮层层）。
pub(super) fn shell_and_layer(harness: &ShellHarness) -> (StableNodeId, StableNodeId) {
    let document = harness.document();
    let world = document.context().world();
    let shell = world
        .document_order(document.document())
        .into_iter()
        .find(|id| world.component_type(*id).is_some_and(|kind| kind.as_str() == component_descriptors::APP_SHELL.type_id))
        .expect("文档里有 AppShell");
    let layer = document.context().read(Entity::<AppShell>::from_stable_id(shell), |shell| shell.overlay).expect("AppShell");
    (shell, layer.expect("AppShell 的 overlay 槽位是浮层层"))
}

/// 浮层层现在的子节点（各块的根）。
pub(super) fn blocks(harness: &ShellHarness) -> Vec<StableNodeId> {
    let (_, layer) = shell_and_layer(harness);
    harness.document().context().world().node(layer).map(|node| node.children.to_vec()).unwrap_or_default()
}

/// 浮层层挡不挡下面的点击（AppShell 按有没有没隐藏的块决定）。
pub(super) fn layer_takes_pointer(harness: &ShellHarness) -> bool {
    let (_, layer) = shell_and_layer(harness);
    harness.document().context().world().interaction(layer).is_some_and(|interaction| interaction.pointer_events)
}

/// 浮层层一直是 AppShell 的 overlay 槽位：弹层和菜单开了又关，槽位不换、AppShell 不重新装配；
/// 有块时挡点击，块卸掉以后点击落到下面。
#[test]
fn the_overlay_layer_follows_its_blocks_without_reassembly() {
    let mut harness = ShellHarness::mount(scene("live-files-plain"));
    let (shell, layer) = shell_and_layer(&harness);
    assert!(blocks(&harness).is_empty());
    assert!(!layer_takes_pointer(&harness), "没有浮层时点击落到下面");

    harness.apply(ShellMessage::Admin(AdminMessage::ToggleTaskPopover));
    harness.flush();
    assert_eq!(blocks(&harness).len(), 1);
    assert!(layer_takes_pointer(&harness), "弹层开着时浮层层挡点击");
    harness.apply(ShellMessage::Admin(AdminMessage::CloseTaskPopover));
    harness.flush();
    assert!(blocks(&harness).is_empty(), "弹层没有退场，换下就卸");
    assert!(!layer_takes_pointer(&harness));

    harness.apply(ShellMessage::Sidebar(SidebarMessage::Gap(GapMessage::OpenFolderMenu {
        path: "assets".into(),
        label: "assets".into(),
        x: 120.0,
        y: 200.0,
    })));
    harness.flush();
    assert!(layer_takes_pointer(&harness), "菜单开着时浮层层挡点击");
    harness.apply(ShellMessage::Sidebar(SidebarMessage::Gap(GapMessage::CloseFolderMenu)));
    harness.flush();
    assert!(!layer_takes_pointer(&harness));
    harness.take_messages();
    let trash = harness.node("回收站");
    let (x, y) = harness.center(trash);
    assert!(!harness.click_messages(x, y).is_empty(), "浮层层空着时点得到侧栏");

    assert_eq!(shell_and_layer(&harness), (shell, layer), "浮层层一直是同一个节点，AppShell 没有换 overlay 槽位");
    harness.assert_same_as_fresh_mount();
}

fn text(harness: &ShellHarness, key: &str) -> String {
    let id = harness.keyed(key).unwrap_or_else(|| panic!("没有 {key}"));
    harness.document().context().world().text(id).unwrap_or_default().to_string()
}

#[test]
fn task_popover_patches_progress_in_place_and_closes_from_outside() {
    let mut harness = ShellHarness::mount(scene("live-files-plain"));
    harness.apply(ShellMessage::Admin(AdminMessage::ToggleTaskPopover));
    harness.apply(ShellMessage::TaskProgressLoaded(vec![task("task-1", 40.0)]));
    harness.flush();
    let root = harness.content_roots().0.expect("任务弹层在浮层里");
    let row = harness.keyed("admin-task-task-1").expect("任务行");
    assert_eq!(text(&harness, "admin-task-percent-task-1"), "40%");

    harness.apply(ShellMessage::TaskProgressLoaded(vec![task("task-1", 65.0)]));
    harness.flush();
    assert_eq!(harness.content_roots().0, Some(root), "进度推进不该重挂弹层");
    assert_eq!(harness.keyed("admin-task-task-1"), Some(row), "进度推进不该重建这一行");
    assert_eq!(text(&harness, "admin-task-percent-task-1"), "65%");

    harness.apply(ShellMessage::TaskProgressLoaded(vec![task("task-1", 65.0), task("task-2", 10.0)]));
    harness.flush();
    assert_eq!(harness.keyed("admin-task-task-1"), Some(row), "加一个任务不该重建已有的行");
    assert!(harness.keyed("admin-task-task-2").is_some(), "新任务没有出现");
    let before = subtree(&harness, root);
    for message in unrelated() {
        harness.apply(message);
        harness.flush();
    }
    assert_eq!(subtree(&harness, root), before, "无关更新换掉了弹层节点");
    harness.assert_same_as_fresh_mount();

    let messages = harness.click_messages(1000.0, 200.0);
    assert_eq!(messages.len(), 1, "点弹层外面应该只发一条消息");
    assert!(matches!(messages[0], ShellMessage::Admin(AdminMessage::CloseTaskPopover)));
    for message in messages {
        harness.apply(message);
    }
    harness.flush();
    assert!(!harness.model.admin.popover_open);
    assert!(harness.content_roots().0.is_none());
}

#[test]
fn repository_switcher_adds_rows_without_rebuilding_the_rest() {
    let mut harness = ShellHarness::mount(scene("repo-switcher"));
    let root = harness.content_roots().0.expect("仓库弹层在浮层里");
    let row = harness.keyed("repository-switch-acceptance-repo").expect("当前仓库这一行");
    let before = subtree(&harness, root);
    for message in unrelated() {
        harness.apply(message);
        harness.flush();
    }
    assert_eq!(subtree(&harness, root), before, "无关更新换掉了弹层节点");

    harness.model.workspace.repositories.push(WorkspaceRepository {
        repo_id: "second".into(),
        name: "第二个资源库".into(),
        path: "D:/second".into(),
        status: "missing".into(),
        backend_plugin_id: "momobako.source.local-filesystem".into(),
        capabilities: Vec::new(),
        cache_required: false,
        cache_status: String::new(),
    });
    changed(&mut harness);
    assert_eq!(harness.content_roots().0, Some(root), "列表变了不该重挂弹层");
    assert_eq!(harness.keyed("repository-switch-acceptance-repo"), Some(row), "已有的行不该重建");
    assert!(harness.find("切换资源库 第二个资源库").is_some(), "新仓库没有出现");
    harness.assert_same_as_fresh_mount();

    let messages = harness.click_messages(1000.0, 600.0);
    assert_eq!(messages.len(), 1, "点弹层外面应该只发一条消息");
    assert!(matches!(messages[0], ShellMessage::Sidebar(SidebarMessage::CloseRepositoryPopover)));
}

#[test]
fn entry_menu_toggles_its_submenu_without_rebuilding_rows() {
    let mut harness = ShellHarness::mount(scene("live-menu"));
    assert_eq!(OverlayKey::of(&harness.model), Some(OverlayKey::EntryMenu));
    let root = harness.content_roots().0.expect("右键菜单在浮层里");
    let rows = ["file-menu-preview", "file-menu-rename", "file-menu-thumbnail", "file-menu-delete"]
        .map(|key| harness.keyed(key).unwrap_or_else(|| panic!("菜单缺少 {key}")));
    assert!(harness.find("刷新缩略图").is_none(), "子菜单一开始收着");

    harness.apply(ShellMessage::Files(FilesMessage::ToggleMenuBranch("thumbnail".into())));
    harness.flush();
    assert_eq!(harness.content_roots().0, Some(root), "展开子菜单不该重挂菜单");
    assert!(harness.find("刷新缩略图").is_some(), "子菜单没有展开");
    for (key, id) in ["file-menu-preview", "file-menu-rename", "file-menu-thumbnail", "file-menu-delete"].iter().zip(rows) {
        assert_eq!(harness.keyed(key), Some(id), "展开子菜单重建了 {key}");
    }
    harness.assert_same_as_fresh_mount();

    let before = subtree(&harness, root);
    for message in unrelated() {
        harness.apply(message);
        harness.flush();
    }
    assert_eq!(subtree(&harness, root), before, "无关更新换掉了菜单节点");

    let messages = harness.click_messages(1100.0, 760.0);
    assert_eq!(messages.len(), 1, "点菜单外面应该只发一条消息");
    assert!(matches!(messages[0], ShellMessage::Files(FilesMessage::CloseEntryMenu)));
}

/// 右键菜单这一层相对浮层层绝对定位，不再靠固定定位加显式层级压过壳层浮层根：菜单层、透明底和
/// 菜单都不写层级，点菜单项照样发它的消息，点菜单外只关菜单。
#[test]
fn entry_menu_takes_clicks_without_a_z_index() {
    let mut harness = ShellHarness::mount(scene("live-menu"));
    harness.take_messages();
    // 菜单层是浮层块的根：脱离树挂出的根不在键表里，按根找。
    let layer = harness.content_roots().0.expect("右键菜单在浮层里");
    let parts = [("菜单层", Some(layer)), ("透明底", harness.keyed("file-context-backdrop")), ("菜单", harness.keyed("file-context-menu"))];
    for (name, id) in parts {
        let id = id.unwrap_or_else(|| panic!("缺少{name}"));
        let style = harness.document().context().world().node_style(id).unwrap_or_else(|| panic!("{name}没有样式"));
        assert_eq!(style.layout.z_index, None, "{name}不该再写层级");
        assert_ne!(style.layout.position, PositionSpec::Fixed, "{name}不该再固定定位");
    }
    let rename = harness.keyed("file-menu-rename").expect("重命名");
    let (x, y) = harness.center(rename);
    let messages = harness.click_messages(x, y);
    assert!(
        matches!(
            messages.as_slice(),
            [ShellMessage::Files(FilesMessage::CloseEntryMenu), ShellMessage::Files(FilesMessage::OpenDialog(FileDialog::Rename))]
        ),
        "点菜单项应该关菜单并打开重命名：{} 条消息",
        messages.len()
    );
    let messages = harness.click_messages(1100.0, 760.0);
    assert!(matches!(messages.as_slice(), [ShellMessage::Files(FilesMessage::CloseEntryMenu)]), "点菜单外只关菜单");
}

#[test]
fn folder_menu_follows_the_file_service_in_place() {
    let mut harness = ShellHarness::mount(scene("live-files-plain"));
    harness.apply(ShellMessage::Sidebar(SidebarMessage::Gap(GapMessage::OpenFolderMenu {
        path: "assets".into(),
        label: "assets".into(),
        x: 120.0,
        y: 200.0,
    })));
    harness.flush();
    // 菜单就是浮层块的根：脱离树挂出的根不在键表里，按根找。
    let menu = harness.content_roots().0.expect("文件夹菜单在浮层里");
    let disabled = |harness: &ShellHarness| {
        harness
            .document()
            .context()
            .read(Entity::<ContextMenu>::from_stable_id(menu), |menu| menu.items.iter().map(|item| item.disabled).collect::<Vec<_>>())
            .expect("菜单")
    };
    assert_eq!(disabled(&harness), [false, false, false, false]);
    harness.model.files.mutating = true;
    changed(&mut harness);
    assert_eq!(harness.content_roots().0, Some(menu), "文件服务忙不该重挂菜单");
    assert_eq!(disabled(&harness), [false, true, true, true], "处理中时后三项应该禁用");
}
