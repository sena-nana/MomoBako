//! 回收站验收场景的测试：停在回收站面板，列表为空且不再显示读取中。

use super::super::super::workspace::WorkspacePanel;
use super::trash_scene;

/// 回收站场景停在回收站面板，列表是空的，不再挂着「正在读取」。
#[test]
fn trash_scene_shows_the_empty_bin() {
    let model = trash_scene();
    assert_eq!(model.workspace.panel, WorkspacePanel::Trash);
    assert!(model.sidebar.browsing_trash, "侧栏应停在回收站");
    assert!(model.files.rows.is_empty(), "夹具没有回收站条目，不编文件");
    assert!(!model.files.loading, "空目录已经读回");
    assert!(
        model.staged_browses.is_empty(),
        "准备帧不应再把回收站标成读取中"
    );
}
