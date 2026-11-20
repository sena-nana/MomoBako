//! 智能文件夹验收场景的测试。

use super::super::super::files::FileContext;
use super::super::super::workspace::WorkspacePanel;
use super::smart_folder_scene;

fn paths(model: &super::super::super::ShellViewModel) -> Vec<String> {
    model
        .files
        .visible_rows(&FileContext::from_model(model))
        .into_iter()
        .map(|row| row.path)
        .collect()
}

/// 挂上和离屏同一棵壳层，读出无障碍名。
fn labels(model: &super::super::super::ShellViewModel) -> Vec<String> {
    let document = crate::acceptance_document_for_model(model.clone()).expect("挂载壳层");
    document
        .context()
        .world()
        .project_accessibility_in_viewport(document.document())
        .into_iter()
        .filter_map(|node| node.label.map(|label| label.to_string()))
        .collect()
}

/// 夹具没有智能文件夹：结果页是空列表，空文案用只读虚拟视图的详情，不写命中数。
#[test]
fn smart_folder_results_stay_empty_without_a_fixture_folder() {
    let model = smart_folder_scene();
    assert!(
        model.sidebar.smart_folders.is_empty(),
        "夹具侧栏没有智能文件夹"
    );
    assert_eq!(model.workspace.panel, WorkspacePanel::SmartFolder);
    assert!(
        model.sidebar.active_smart_folder_id.is_none(),
        "没有文件夹就不编一个 id"
    );
    assert!(model.sidebar.smart_result.is_none(), "没有查询就不写命中数");
    assert!(model.sidebar.smart_summary().is_empty(), "摘要行留空");
    assert_eq!(model.sidebar.smart_title(), "智能文件夹");
    assert!(paths(&model).is_empty(), "结果列表不拿目录里的文件充数");
    assert!(
        model.files.rows.iter().any(|row| row.path == "cover.png"),
        "目录快照还在"
    );
    assert!(!model.files.loading, "没有在读目录");
    assert!(model.player_surface_visible(), "播放条留在列表下面");
    assert!(model.staged_browses.is_empty());

    let labels = labels(&model);
    for label in [
        "分类视图",
        "智能文件夹",
        "选择一个文件或文件夹",
        "在中间列表中选择目标查看详情。",
        "还没有智能文件夹。",
        "未选择播放内容",
    ] {
        assert!(
            labels.iter().any(|item| item == label),
            "结果页缺少「{label}」"
        );
    }
    for label in ["cover.png", "page.pdf", "0 条结果", "直属文件"] {
        assert!(
            !labels.iter().any(|item| item == label),
            "结果页不该出现「{label}」"
        );
    }
}
