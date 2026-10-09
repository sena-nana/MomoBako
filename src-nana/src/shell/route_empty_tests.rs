//! 常驻空库页的回归：投影取值；无关更新不换节点、不重挂；拖着文件夹经过时面板原地换底色和描边，
//! 附加失败的错误条原地出现；拖放消息带的是事件到达时的仓库条件，不是建视图那一刻的；每一步都和
//! 同一 ViewModel 新挂的一样。

use std::path::PathBuf;

use nana_ui::runtime::SemanticColorRole;
use nana_ui::{FileDragInput, FileDragKind, HeadlessInput, InputModifiers, InputPayload};

use super::EmptyView;
use crate::shell::input::{HostDragPhase, InputMessage};
use crate::shell::view_harness::ShellHarness;
use crate::shell::{ShellMessage, ShellPage, ShellViewModel, ThumbnailFrame, WorkspacePanel};

fn empty() -> ShellViewModel {
    ShellViewModel::for_page(ShellPage::EmptyRepository)
}

/// 宿主拖放消息：没有仓库时只看阶段和路径。
fn host_drag(phase: HostDragPhase) -> ShellMessage {
    ShellMessage::Input(InputMessage::HostDrag {
        phase,
        paths: vec!["C:\\library".into()],
        has_repository: false,
        missing_repository: false,
        writable: false,
        files_panel: false,
        has_snapshot: false,
        repo_root: String::new(),
    })
}

/// 面板的底色和描边色。
fn panel_paint(harness: &ShellHarness) -> (Option<SemanticColorRole>, Option<SemanticColorRole>) {
    let panel = harness.keyed("empty-panel").expect("空库面板");
    let style = harness.document().context().world().node_style(panel).expect("面板样式").clone();
    (style.background, style.border)
}

#[test]
fn projection_follows_the_drag_state() {
    let mut model = empty();
    let view = EmptyView::project(&model);
    assert!(!view.dragging);
    assert!(view.error.is_empty());
    assert_eq!(EmptyView::project(&model), view, "同一状态的投影相等");
    model.reduce(host_drag(HostDragPhase::Over));
    assert!(EmptyView::project(&model).dragging, "拖着文件夹经过");
    model.reduce(ShellMessage::SetWorkspacePanel(WorkspacePanel::Logs));
    assert_ne!(EmptyView::project(&model).drop, view.drop, "换面板改拖放条件");
}

/// 和空库页无关的更新：节点一个都不换，主区分支不重挂。
#[test]
fn unrelated_updates_keep_every_empty_node() {
    let mut harness = ShellHarness::mount(empty());
    let keys = ["workspace-empty-scroll", "empty-repository-page", "empty-panel", "empty-title", "empty-detail", "empty-error"];
    let nodes = keys.map(|key| harness.keyed(key).unwrap_or_else(|| panic!("空库页缺少 {key}")));
    let branch = harness.route_branch();
    let remounts = harness.view_stats().remounts;
    let unrelated = [
        ShellMessage::ThumbnailPixels(vec![ThumbnailFrame { path: "a.png".into(), natural_width: 1, natural_height: 1, width: 1, height: 1, rgba: vec![0; 4] }]),
        ShellMessage::Player(crate::shell::player::PlayerMessage::SetVolume(0.3)),
        ShellMessage::SetWorkspacePanel(WorkspacePanel::Logs),
    ];
    for message in unrelated {
        harness.apply(message);
        harness.flush();
        for (key, id) in keys.iter().zip(nodes) {
            assert_eq!(harness.keyed(key), Some(id), "无关更新换掉了空库页节点 {key}");
        }
    }
    assert_eq!(harness.route_branch(), branch, "常驻路由的分支不该重挂");
    assert_eq!(harness.view_stats().remounts, remounts, "常驻空库页不该重挂");
    harness.assert_same_as_fresh_mount();
}

/// 拖着文件夹经过时面板原地换样子，离开后恢复；附加失败的错误条原地出现。
#[test]
fn drag_highlight_and_error_patch_in_place() {
    let mut harness = ShellHarness::mount(empty());
    let panel = harness.keyed("empty-panel").expect("空库面板");
    assert_eq!(panel_paint(&harness), (None, None));

    harness.apply(host_drag(HostDragPhase::Over));
    harness.flush();
    assert_eq!(harness.keyed("empty-panel"), Some(panel));
    assert_eq!(panel_paint(&harness), (Some(SemanticColorRole::AccentSoft), Some(SemanticColorRole::Accent)));
    harness.assert_same_as_fresh_mount();

    harness.apply(host_drag(HostDragPhase::Leave));
    harness.flush();
    assert_eq!(panel_paint(&harness), (None, None));

    harness.model.input.empty_repository_error = "附加失败：目录不可读".into();
    harness.sync();
    harness.flush();
    assert!(harness.find("附加失败：目录不可读").is_some(), "错误条没有显示");
    assert_eq!(harness.keyed("empty-panel"), Some(panel));
    harness.assert_same_as_fresh_mount();
}

/// 建好页面以后换了面板：放下文件夹时发出的消息带的是现在的条件。
#[test]
fn drops_carry_the_flags_at_drop_time() {
    let mut harness = ShellHarness::mount(empty());
    harness.apply(ShellMessage::SetWorkspacePanel(WorkspacePanel::Logs));
    harness.flush();
    let target = harness.keyed("empty-repository-page").expect("拖放目标");
    let bounds = harness.document().context().world().layout_box(target).expect("拖放目标的布局");
    let point = (bounds.x + bounds.width / 2.0, bounds.y + bounds.height / 2.0);
    let document = harness.window.document.document();
    let mut input = HeadlessInput::bind(harness.window.document.context_mut(), document);
    input
        .route(
            harness.window.document.context_mut(),
            InputPayload::FileDrag(FileDragInput {
                kind: FileDragKind::Drop,
                paths: vec![PathBuf::from("C:\\library")],
                position: Some(point),
                modifiers: InputModifiers::default(),
            }),
        )
        .expect("文件拖放");
    let messages = harness.take_messages();
    let drop = messages
        .iter()
        .find_map(|message| match message {
            ShellMessage::Input(InputMessage::HostDrag { phase: HostDragPhase::Drop, files_panel, .. }) => Some(*files_panel),
            _ => None,
        })
        .expect("放下应该发出宿主拖放消息");
    assert!(!drop, "拖放条件应是放下时的（日志面板），不是建视图时的（文件面板）");
    for message in messages {
        harness.apply(message);
    }
    let effects = harness.model.sidebar.take_effects();
    assert!(
        effects.iter().any(|effect| matches!(effect, crate::shell::SidebarEffect::AttachRepository { path } if path == "C:\\library")),
        "空库放下应该附加文件夹：{effects:?}"
    );
}
