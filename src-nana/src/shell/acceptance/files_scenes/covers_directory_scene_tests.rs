//! 封面目录验收场景的测试。

use super::super::super::files::FileContext;
use super::super::super::view_harness::ShellHarness;
use super::super::super::view_part_sidebar::project::SidebarView;
use super::super::super::workspace::{LibraryCategory, WorkspacePanel};
use super::covers_directory_scene;

/// 进入 covers 后夹具这一层是空的。侧栏停在 covers，面包屑能看出根目录 / assets / covers。
#[test]
fn covers_directory_shows_the_empty_folder() {
    let model = covers_directory_scene();
    assert_eq!(model.workspace.panel, WorkspacePanel::Files);
    assert_eq!(model.workspace.library_category, LibraryCategory::All);
    assert_eq!(model.files.current_path, "assets/covers");
    assert_eq!(model.sidebar.current_directory, "assets/covers");
    assert!(
        model
            .sidebar
            .expanded_folders
            .iter()
            .any(|path| path == "assets"),
        "进入后展开 assets"
    );
    assert!(
        model
            .sidebar
            .expanded_folders
            .iter()
            .any(|path| path == "assets/covers"),
        "进入后展开 covers"
    );
    let paths = model
        .files
        .visible_rows(&FileContext::from_model(&model))
        .into_iter()
        .map(|row| row.path.clone())
        .collect::<Vec<_>>();
    assert!(
        paths.is_empty(),
        "夹具里 covers 没有子项，不编造文件：{paths:?}"
    );
    assert!(!model.files.loading, "空目录已经读回");
    assert!(model.staged_browses.is_empty(), "准备帧不应再标成读取中");
    assert!(model.player_surface_visible(), "播放条留在文件浏览面板下面");

    let sidebar = SidebarView::project(&model);
    let assets = sidebar
        .folder_rows
        .iter()
        .find(|row| row.path == "assets")
        .expect("侧栏有 assets");
    assert!(
        !assets.active && assets.expanded,
        "assets 展开，当前目录不是它"
    );
    let covers = sidebar
        .folder_rows
        .iter()
        .find(|row| row.path == "assets/covers")
        .expect("侧栏有 covers");
    assert!(covers.active, "当前目录是 covers");

    let harness = ShellHarness::mount(model);
    for label in [
        "当前目录",
        "根目录",
        "assets",
        "covers",
        "assets/covers",
        "直属文件",
        "直属子文件夹",
        "当前视图总条目",
        "0",
        "未选择播放内容",
    ] {
        assert!(harness.find(label).is_some(), "目录页缺少「{label}」");
    }
    for label in ["cover.png", "page.pdf", "选择一个文件或文件夹"] {
        assert!(harness.find(label).is_none(), "空目录不该出现「{label}」");
    }
}
