//! 弹层和右键菜单常驻的回归：打开期间无关更新不换节点；任务进度、仓库列表、分组展开和
//! 文件服务忙这些相关变化只改绑定的字段或变了的那一行；点外面只发一条关闭消息。浮层层常驻在
//! AppShell 的 overlay 槽位里，有块时挡点击、空了放开，增删块不用重新装配。

use nana_ui::runtime::{AppShell, Entity, Popover, StableNodeId, component_descriptors};

use super::OverlayKey;
use crate::backend::services::repository::TaskProgressSnapshot;
use crate::shell::files::{FileDialog, FilesMessage};
use crate::shell::player::PlayerMessage;
use crate::shell::sidebar::PopoverMode;
use crate::shell::view_harness::ShellHarness;
use crate::shell::workspace::WorkspaceRepository;
use crate::shell::{
    AdminMessage, GapMessage, InspectMessage, ShellMessage, ShellViewModel, SidebarMessage,
};

fn scene(name: &str) -> ShellViewModel {
    crate::shell::acceptance_gap_models()
        .into_iter()
        .find(|(scene, _)| *scene == name)
        .unwrap_or_else(|| panic!("没有场景 {name}"))
        .1
}

/// 和弹层无关的消息：播放音量和标题栏搜索。
fn unrelated() -> Vec<ShellMessage> {
    vec![
        ShellMessage::Player(PlayerMessage::SetVolume(0.4)),
        ShellMessage::Inspect(InspectMessage::SetQuery("封面".into())),
    ]
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
        .find(|id| {
            world
                .component_type(*id)
                .is_some_and(|kind| kind.as_str() == component_descriptors::APP_SHELL.type_id)
        })
        .expect("文档里有 AppShell");
    let layer = document
        .context()
        .read(Entity::<AppShell>::from_stable_id(shell), |shell| {
            shell.overlay
        })
        .expect("AppShell");
    (shell, layer.expect("AppShell 的 overlay 槽位是浮层层"))
}

/// 浮层层现在的子节点（各块的根）。
pub(super) fn blocks(harness: &ShellHarness) -> Vec<StableNodeId> {
    let (_, layer) = shell_and_layer(harness);
    harness
        .document()
        .context()
        .world()
        .node(layer)
        .map(|node| node.children.to_vec())
        .unwrap_or_default()
}

/// 浮层层挡不挡下面的点击（AppShell 按有没有没隐藏的块决定）。
pub(super) fn layer_takes_pointer(harness: &ShellHarness) -> bool {
    let (_, layer) = shell_and_layer(harness);
    harness
        .document()
        .context()
        .world()
        .interaction(layer)
        .is_some_and(|interaction| interaction.pointer_events)
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
    assert!(
        blocks(&harness).is_empty() && !layer_takes_pointer(&harness),
        "任务弹层是挂在「任务」上的 NanaUI Popover，不进浮层层"
    );
    harness.apply(ShellMessage::Admin(AdminMessage::CloseTaskPopover));
    harness.flush();

    harness.apply(ShellMessage::Sidebar(
        SidebarMessage::OpenRepositorySwitcher,
    ));
    harness.flush();
    let page = harness.content_roots().0.expect("仓库弹层的页");
    let slot = harness
        .keyed("repository-popover-slot")
        .expect("仓库头弹层里的一格");
    assert!(
        blocks(&harness).is_empty() && !layer_takes_pointer(&harness),
        "仓库弹层是挂在仓库头上的 NanaUI Popover，页放进它面板里的一格，不进浮层层"
    );
    assert_eq!(
        harness.document().context().world().parent_id(page),
        Some(slot)
    );
    harness.apply(ShellMessage::Sidebar(
        SidebarMessage::CloseRepositoryPopover,
    ));
    harness.flush();
    assert!(harness.content_roots().0.is_none());

    harness.apply(ShellMessage::Sidebar(SidebarMessage::Gap(
        GapMessage::OpenFolderMenu {
            path: "assets".into(),
            label: "assets".into(),
            x: 120.0,
            y: 200.0,
        },
    )));
    harness.flush();
    assert!(layer_takes_pointer(&harness), "菜单开着时浮层层挡点击");
    harness.apply(ShellMessage::Sidebar(SidebarMessage::Gap(
        GapMessage::CloseFolderMenu,
    )));
    harness.flush();
    assert!(!layer_takes_pointer(&harness));
    harness.take_messages();
    let trash = harness.node("回收站");
    let (x, y) = harness.center(trash);
    assert!(
        !harness.click_messages(x, y).is_empty(),
        "浮层层空着时点得到侧栏"
    );

    assert_eq!(
        shell_and_layer(&harness),
        (shell, layer),
        "浮层层一直是同一个节点，AppShell 没有换 overlay 槽位"
    );
    harness.assert_same_as_fresh_mount();
}

fn text(harness: &ShellHarness, key: &str) -> String {
    let id = harness.keyed(key).unwrap_or_else(|| panic!("没有 {key}"));
    harness
        .document()
        .context()
        .world()
        .text(id)
        .unwrap_or_default()
        .to_string()
}

#[test]
fn task_popover_patches_progress_in_place_and_closes_from_outside() {
    let mut harness = ShellHarness::mount(scene("live-files-plain"));
    harness.apply(ShellMessage::Admin(AdminMessage::ToggleTaskPopover));
    harness.apply(ShellMessage::TaskProgressLoaded(vec![task("task-1", 40.0)]));
    harness.flush();
    let root = harness.keyed("admin-task-popover").expect("任务弹层的面板");
    let row = harness.keyed("admin-task-task-1").expect("任务行");
    assert_eq!(text(&harness, "admin-task-percent-task-1"), "40%");

    harness.apply(ShellMessage::TaskProgressLoaded(vec![task("task-1", 65.0)]));
    harness.flush();
    assert_eq!(
        harness.keyed("admin-task-popover"),
        Some(root),
        "进度推进不该重挂弹层"
    );
    assert_eq!(
        harness.keyed("admin-task-task-1"),
        Some(row),
        "进度推进不该重建这一行"
    );
    assert_eq!(text(&harness, "admin-task-percent-task-1"), "65%");

    harness.apply(ShellMessage::TaskProgressLoaded(vec![
        task("task-1", 65.0),
        task("task-2", 10.0),
    ]));
    harness.flush();
    assert_eq!(
        harness.keyed("admin-task-task-1"),
        Some(row),
        "加一个任务不该重建已有的行"
    );
    assert!(
        harness.keyed("admin-task-task-2").is_some(),
        "新任务没有出现"
    );
    let before = subtree(&harness, root);
    for message in unrelated() {
        harness.apply(message);
        harness.flush();
    }
    assert_eq!(subtree(&harness, root), before, "无关更新换掉了弹层节点");
    harness.assert_same_as_fresh_mount();

    let messages = harness.click_messages(1000.0, 200.0);
    assert_eq!(messages.len(), 1, "点弹层外面应该只发一条消息");
    assert!(matches!(
        messages[0],
        ShellMessage::Admin(AdminMessage::TaskPopoverToggled(false))
    ));
    for message in messages {
        harness.apply(message);
    }
    harness.flush();
    assert!(!harness.model.admin.popover_open);
}

/// 任务弹层里一条进度条在读屏里的样子：是否忙碌（不确定进度）和报出的数值。
fn progress_state(harness: &ShellHarness, task_id: &str) -> (bool, Option<f64>) {
    let id = harness
        .keyed(&format!("admin-task-fill-{task_id}"))
        .unwrap_or_else(|| panic!("没有 {task_id} 的进度条"));
    let node = harness
        .nodes()
        .into_iter()
        .find(|node| node.id == id)
        .unwrap_or_else(|| panic!("{task_id} 的进度条不在读屏树里"));
    (node.busy, node.numeric_value)
}

/// 不确定进度是 NanaUI 的不确定进度条：读屏标成忙、不报数值，沿轨道扫的动画由组件自己跑；
/// 确定进度报出数值。换成不确定和换回来都不重建这一行；弹层关上以后壳层不再为进度要帧。
#[test]
fn indeterminate_progress_sweeps_in_the_task_popover() {
    let mut harness = ShellHarness::mount(scene("live-files-plain"));
    harness.apply(ShellMessage::Admin(AdminMessage::ToggleTaskPopover));
    let mut scanning = task("task-1", 0.0);
    scanning.percent = None;
    harness.apply(ShellMessage::TaskProgressLoaded(vec![
        scanning,
        task("task-2", 30.0),
    ]));
    harness.flush();
    let row = harness.keyed("admin-task-task-1").expect("任务行");
    assert_eq!(
        progress_state(&harness, "task-1"),
        (true, None),
        "不确定进度标成忙、不报数值"
    );
    assert_eq!(
        progress_state(&harness, "task-2"),
        (false, Some(30.0)),
        "确定进度报出数值"
    );
    for _ in 0..5 {
        harness.frame();
    }
    assert!(
        harness
            .document()
            .context()
            .next_animation_deadline()
            .is_some(),
        "不确定进度由组件自己扫，NanaUI 按它的动画时钟要帧"
    );
    assert!(!harness.model.fills.active(), "壳层不为扫动另外要帧");

    reduce_motion(&mut harness, true);
    harness.apply(ShellMessage::TaskProgressLoaded(vec![task("task-1", 50.0)]));
    harness.flush();
    assert_eq!(
        progress_state(&harness, "task-1"),
        (false, Some(50.0)),
        "有了百分比以后报出数值"
    );
    assert_eq!(
        harness.keyed("admin-task-task-1"),
        Some(row),
        "换成确定进度不重建这一行"
    );
    reduce_motion(&mut harness, false);
    harness.apply(ShellMessage::Admin(AdminMessage::CloseTaskPopover));
    for _ in 0..20 {
        harness.frame();
    }
    assert!(!harness.model.fills.active(), "弹层关着时不为进度要帧");
}

/// 在 NanaUI 的文档上下文上开关减少动态效果，跑一帧让进度条填充照它跟上（生产里系统设置也是这样传下来的）。
fn reduce_motion(harness: &mut ShellHarness, reduced: bool) {
    harness
        .window
        .document
        .context_mut()
        .set_reduced_motion(reduced);
    harness.frame();
}

/// 任务弹层里进度条的值（百分比），取读屏报出的数值。
fn fill_width(harness: &ShellHarness, task_id: &str) -> f32 {
    match progress_state(harness, task_id) {
        (false, Some(value)) => value as f32,
        other => panic!("进度条没有报出数值：{other:?}"),
    }
}

/// 进度条的值按主题动效过渡：新出现的行直接是目标值，进度推进时从原来的
/// 值过渡过去，走完过渡时长停在新值；过渡只改进度条的值，不重建这一行。减少动态效果时直接跳到新值。
#[test]
fn task_progress_widths_transition_smoothly() {
    let mut harness = ShellHarness::mount(scene("live-files-plain"));
    harness.apply(ShellMessage::Admin(AdminMessage::ToggleTaskPopover));
    harness.apply(ShellMessage::TaskProgressLoaded(vec![task("task-1", 40.0)]));
    harness.flush();
    let row = harness.keyed("admin-task-task-1").expect("任务行");
    assert_eq!(fill_width(&harness, "task-1"), 40.0, "新出现的行不过渡");

    harness.apply(ShellMessage::TaskProgressLoaded(vec![task("task-1", 80.0)]));
    harness.flush();
    assert_eq!(
        fill_width(&harness, "task-1"),
        40.0,
        "推进的这一刻还在原来的宽度"
    );
    assert_eq!(
        text(&harness, "admin-task-percent-task-1"),
        "80%",
        "百分比直接写新值"
    );
    harness.frame();
    let moving = fill_width(&harness, "task-1");
    assert!(moving > 40.0 && moving < 80.0, "过渡中：{moving}");
    assert!(
        matches!(harness.window.demand, nana_ui::FrameDemand::Continuous(_)),
        "过渡要帧"
    );
    for _ in 0..12 {
        harness.frame();
    }
    assert_eq!(fill_width(&harness, "task-1"), 80.0, "过渡走完以后停在新值");
    assert_eq!(
        harness.keyed("admin-task-task-1"),
        Some(row),
        "过渡不重建这一行"
    );

    reduce_motion(&mut harness, true);
    harness.apply(ShellMessage::TaskProgressLoaded(vec![task("task-1", 20.0)]));
    harness.flush();
    assert_eq!(
        fill_width(&harness, "task-1"),
        20.0,
        "减少动态效果时直接跳到新值"
    );
    harness.assert_same_as_fresh_mount();
}

/// 仓库操作行在准备帧里每 220ms 往上爬一级：宽度同样过渡过去，不是一格一格跳。
#[test]
fn a_climbing_repository_operation_transitions_its_width() {
    let mut harness = ShellHarness::mount(scene("live-files-plain"));
    harness.apply(ShellMessage::Admin(AdminMessage::ToggleTaskPopover));
    harness.apply(ShellMessage::Sidebar(SidebarMessage::RefreshFolderTree));
    harness.flush();
    let start = fill_width(&harness, "workspace-operation");
    assert_eq!(start, 12.0, "「刷新文件树」从 12% 起步");
    // 220ms 爬一级到 17%：爬的那一帧还在原来的宽度附近，之后几帧走到新值。
    for _ in 0..14 {
        harness.frame();
    }
    let climbed = harness
        .model
        .operation
        .shown()
        .map(|row| row.value as f32)
        .expect("仓库操作行");
    assert_eq!(climbed, 17.0);
    let moving = fill_width(&harness, "workspace-operation");
    assert!(moving > start && moving < climbed, "过渡中：{moving}");
    for _ in 0..12 {
        harness.frame();
    }
    assert_eq!(fill_width(&harness, "workspace-operation"), climbed);
}

#[test]
fn repository_switcher_adds_rows_without_rebuilding_the_rest() {
    let mut harness = ShellHarness::mount(scene("repo-switcher"));
    let root = harness.content_roots().0.expect("仓库弹层的页");
    let row = harness
        .keyed("repository-switch-acceptance-repo")
        .expect("当前仓库这一行");
    let before = subtree(&harness, root);
    for message in unrelated() {
        harness.apply(message);
        harness.flush();
    }
    assert_eq!(subtree(&harness, root), before, "无关更新换掉了弹层节点");

    harness
        .model
        .workspace
        .repositories
        .push(WorkspaceRepository {
            repo_id: "second".into(),
            name: "第二个资源库".into(),
            path: "D:/second".into(),
            status: "missing".into(),
            backend_plugin_id: "momobako.source.local-filesystem".into(),
            capabilities: Vec::new(),
            cache_required: false,
            cache_status: String::new(),
            cache_path: None,
            login_expired: false,
            logged_out: false,
        });
    changed(&mut harness);
    assert_eq!(
        harness.content_roots().0,
        Some(root),
        "列表变了不该重挂弹层"
    );
    assert_eq!(
        harness.keyed("repository-switch-acceptance-repo"),
        Some(row),
        "已有的行不该重建"
    );
    assert!(
        harness.find("切换资源库 第二个资源库").is_some(),
        "新仓库没有出现"
    );
    harness.assert_same_as_fresh_mount();

    let messages = harness.click_messages(1000.0, 600.0);
    assert_eq!(messages.len(), 1, "点弹层外面应该只发一条消息");
    assert!(matches!(
        messages[0],
        ShellMessage::Sidebar(SidebarMessage::CloseRepositoryPopover)
    ));
}

/// 仓库头上的 NanaUI `Popover` 现在开没开着。
fn repository_popover_open(harness: &ShellHarness) -> bool {
    let anchor = harness.keyed("repository-switcher").expect("仓库头");
    harness
        .document()
        .context()
        .read(Entity::<Popover>::from_stable_id(anchor), |popover| {
            popover.open
        })
        .expect("仓库头是 Popover")
}

/// 提交中仓库弹层关不掉：点外面和 Escape 都不收起，Escape 也不越过它去关别的层；点仓库头时框架先收起了
/// 弹层，ViewModel 不跟，下一次同步照 ViewModel 开回来。弹层关着时提交中点仓库头，框架打开了也照
/// ViewModel 收回去。
#[test]
fn a_submitting_repository_popover_follows_the_view_model() {
    let mut model = scene("repo-switcher");
    model.sidebar.submitting = true;
    let mut harness = ShellHarness::mount(model);
    assert!(repository_popover_open(&harness));

    let messages = harness.click_messages(1000.0, 600.0);
    harness.settle(messages);
    assert_eq!(harness.model.sidebar.popover, PopoverMode::Switcher);
    assert!(repository_popover_open(&harness), "点外面不收起");
    assert!(harness.press_escape(), "Escape 停在提交中的仓库弹层");
    assert_eq!(harness.model.sidebar.popover, PopoverMode::Switcher);
    assert!(repository_popover_open(&harness), "Escape 不收起");

    harness.click_label("资源库 · 默认资源库");
    assert_eq!(harness.model.sidebar.popover, PopoverMode::Switcher);
    assert!(
        repository_popover_open(&harness),
        "点仓库头收起了，照 ViewModel 开回来"
    );

    harness.model.sidebar.popover = PopoverMode::Closed;
    changed(&mut harness);
    assert!(!repository_popover_open(&harness));
    harness.click_label("资源库 · 默认资源库");
    assert_eq!(harness.model.sidebar.popover, PopoverMode::Closed);
    assert!(
        !repository_popover_open(&harness),
        "提交中打不开切换列表，框架打开的弹层收回去"
    );
}

#[test]
fn entry_menu_expands_a_group_without_rebuilding_rows() {
    let mut harness = ShellHarness::mount(scene("live-menu"));
    assert_eq!(OverlayKey::of(&harness.model), Some(OverlayKey::EntryMenu));
    let root = harness.content_roots().0.expect("右键菜单在浮层里");
    let rows = [
        "file-menu-preview",
        "file-menu-rename",
        "file-menu-thumbnail",
        "file-menu-delete",
    ]
    .map(|key| {
        harness
            .keyed(key)
            .unwrap_or_else(|| panic!("菜单缺少 {key}"))
    });
    assert!(harness.find("刷新缩略图").is_none(), "分组一开始收着");

    harness.apply(ShellMessage::Files(FilesMessage::MenuBranchChanged {
        branch: "thumbnail".into(),
        source: crate::shell::files::BranchSource::Pointer,
        inside: true,
    }));
    harness.flush();
    assert_eq!(
        harness.content_roots().0,
        Some(root),
        "展开分组不该重挂菜单"
    );
    assert!(harness.find("刷新缩略图").is_some(), "分组没有展开");
    for (key, id) in [
        "file-menu-preview",
        "file-menu-rename",
        "file-menu-thumbnail",
        "file-menu-delete",
    ]
    .iter()
    .zip(rows)
    {
        assert_eq!(harness.keyed(key), Some(id), "展开分组重建了 {key}");
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
    assert!(matches!(
        messages[0],
        ShellMessage::Files(FilesMessage::CloseEntryMenu)
    ));
}

/// 点菜单项发它自己的消息，框架随之收起菜单，只多一条关闭消息。
#[test]
fn entry_menu_items_take_their_own_clicks() {
    let mut harness = ShellHarness::mount(scene("live-menu"));
    harness.take_messages();
    let rename = harness.keyed("file-menu-rename").expect("重命名");
    let (x, y) = harness.center(rename);
    let messages = harness.click_messages(x, y);
    assert!(
        matches!(
            messages.as_slice(),
            [
                ShellMessage::Files(FilesMessage::OpenDialog(FileDialog::Rename)),
                ShellMessage::Files(FilesMessage::CloseEntryMenu)
            ]
        ),
        "点菜单项应该打开重命名并关菜单：{} 条消息",
        messages.len()
    );
    harness.settle(messages);
    assert_eq!(OverlayKey::of(&harness.model), Some(OverlayKey::FileDialog));
    assert!(harness.keyed("file-context-menu").is_none(), "菜单换下");
}

#[test]
fn folder_menu_follows_the_file_service_in_place() {
    let mut harness = ShellHarness::mount(scene("live-files-plain"));
    harness.apply(ShellMessage::Sidebar(SidebarMessage::Gap(
        GapMessage::OpenFolderMenu {
            path: "assets".into(),
            label: "assets".into(),
            x: 120.0,
            y: 200.0,
        },
    )));
    harness.flush();
    // 菜单层是浮层块的根：脱离树挂出的根不在键表里，按根找。
    let layer = harness.content_roots().0.expect("文件夹菜单在浮层里");
    let open = harness.keyed("folder-menu-open").expect("「打开」");
    let disabled = |harness: &ShellHarness| {
        let nodes = harness.nodes();
        ["open", "create", "rename", "delete"].map(|item| {
            let id = harness
                .keyed(&format!("folder-menu-{item}"))
                .unwrap_or_else(|| panic!("菜单里没有 {item}"));
            nodes
                .iter()
                .find(|node| node.id == id)
                .unwrap_or_else(|| panic!("{item} 不在无障碍树里"))
                .disabled
        })
    };
    assert_eq!(disabled(&harness), [false, false, false, false]);
    harness.model.files.mutating = true;
    changed(&mut harness);
    assert_eq!(
        harness.content_roots().0,
        Some(layer),
        "文件服务忙不该重挂菜单"
    );
    assert_eq!(
        harness.keyed("folder-menu-open"),
        Some(open),
        "没变的「打开」不重建"
    );
    assert_eq!(
        disabled(&harness),
        [false, true, true, true],
        "处理中时后三项应该禁用"
    );
    harness.assert_same_as_fresh_mount();
}
