//! 素材目录验收场景的测试。

use super::super::super::files::FileContext;
use super::super::super::view_harness::ShellHarness;
use super::super::super::view_part_sidebar::project::SidebarView;
use super::super::super::workspace::{LibraryCategory, WorkspacePanel};
use super::assets_directory_scene;

/// 进入 assets 后只列出夹具里的子文件夹 covers。侧栏停在这一层，播放条留着。
#[test]
fn assets_directory_lists_only_fixture_children() {
    let model = assets_directory_scene();
    assert_eq!(model.workspace.panel, WorkspacePanel::Files);
    assert_eq!(model.workspace.library_category, LibraryCategory::All);
    assert_eq!(model.files.current_path, "assets");
    assert_eq!(model.sidebar.current_directory, "assets");
    assert!(
        model
            .sidebar
            .expanded_folders
            .iter()
            .any(|path| path == "assets"),
        "进入后展开 assets"
    );
    let paths = model
        .files
        .visible_rows(&FileContext::from_model(&model))
        .into_iter()
        .map(|row| row.path.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        paths,
        vec!["assets/covers".to_string()],
        "夹具里 assets 只有子文件夹 covers"
    );
    assert!(
        model.files.rows.iter().all(|row| row.kind == "directory"),
        "这一层没有文件"
    );
    assert!(!model.files.loading, "目录已经读回");
    assert!(model.staged_browses.is_empty(), "准备帧不应再标成读取中");
    assert!(model.player_surface_visible(), "播放条留在文件浏览面板下面");

    let sidebar = SidebarView::project(&model);
    let assets = sidebar
        .folder_rows
        .iter()
        .find(|row| row.path == "assets")
        .expect("侧栏有 assets");
    assert!(
        assets.active && assets.expanded,
        "当前目录是 assets，不是根目录"
    );
    assert!(
        sidebar
            .folder_rows
            .iter()
            .any(|row| row.path == "assets/covers" && !row.active)
    );

    let harness = ShellHarness::mount(model);
    for label in [
        "当前目录",
        "根目录",
        "assets",
        "covers",
        "直属文件",
        "直属子文件夹",
        "当前视图总条目",
        "未选择播放内容",
    ] {
        assert!(harness.find(label).is_some(), "目录页缺少「{label}」");
    }
    for label in ["cover.png", "page.pdf", "选择一个文件或文件夹"] {
        assert!(harness.find(label).is_none(), "目录页不该出现「{label}」");
    }
}
