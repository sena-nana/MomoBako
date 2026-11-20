//! 资源分类验收场景的测试。

use super::super::super::ShortcutId;
use super::super::super::files::FileContext;
use super::super::super::workspace::{LibraryCategory, WorkspacePanel};
use super::category_scene;

fn paths(model: &super::super::super::ShellViewModel) -> Vec<String> {
    model
        .files
        .visible_rows(&FileContext::from_model(model))
        .into_iter()
        .map(|row| row.path)
        .collect()
}

/// 三个分类都还是文件页。列表只留摘要里对得上的素材，最近使用没有访问时间所以是空的。
#[test]
fn category_scenes_filter_the_snapshot() {
    let uncategorized = category_scene(ShortcutId::Uncategorized);
    assert_eq!(uncategorized.workspace.panel, WorkspacePanel::Files);
    assert_eq!(
        uncategorized.workspace.library_category,
        LibraryCategory::Uncategorized
    );
    assert_eq!(paths(&uncategorized), vec!["cover.png".to_string()]);
    assert_eq!(uncategorized.sidebar.counts.uncategorized, 1);
    assert!(!uncategorized.files.loading, "分类视图不读目录");
    assert!(
        uncategorized.player_surface_visible(),
        "分类视图仍是文件浏览面板，播放条留着"
    );
    assert!(uncategorized.staged_browses.is_empty());

    let untagged = category_scene(ShortcutId::Untagged);
    assert_eq!(
        untagged.workspace.library_category,
        LibraryCategory::Untagged
    );
    assert_eq!(
        paths(&untagged),
        vec!["cover.png".to_string(), "notes/page.pdf".to_string()]
    );
    assert_eq!(untagged.sidebar.counts.untagged, 2);
    assert!(
        untagged.files.rows.iter().any(|row| row.tags.is_empty()),
        "夹具文件本来就没有标签"
    );

    let recent = category_scene(ShortcutId::Recent);
    assert_eq!(recent.workspace.library_category, LibraryCategory::Recent);
    assert!(paths(&recent).is_empty(), "夹具没有访问时间，不编最近使用");
    assert_eq!(recent.sidebar.counts.recent, 0);
    assert!(recent.player_surface_visible());
    assert!(recent.staged_browses.is_empty());
}
