//! 侧栏投影的回归：和侧栏无关的更新不改投影、侧栏节点一个都不换；侧栏读到的状态一变，投影就变，
//! 侧栏重挂后和新挂的一样。

use super::SidebarProjection;
use crate::shell::view_harness::ShellHarness;
use crate::shell::{GapMessage, ShellMessage, ShellPage, ShellViewModel, SidebarMessage, ThumbnailFrame};

fn scene(name: &str) -> ShellViewModel {
    crate::shell::acceptance_gap_models()
        .into_iter()
        .find(|(scene, _)| *scene == name)
        .unwrap_or_else(|| panic!("没有场景 {name}"))
        .1
}

/// 和侧栏无关的消息：缩略图、播放音量、搜索词，以及只换主区的路由（搜索结果和播放集互换）。
fn unrelated() -> Vec<ShellMessage> {
    vec![
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
        ShellMessage::Navigate(ShellPage::Playlists),
        ShellMessage::Navigate(ShellPage::FileList),
    ]
}

#[test]
fn unrelated_messages_leave_the_projection_alone() {
    let mut model = scene("search-results");
    let before = SidebarProjection::project(&model);
    for message in unrelated() {
        model.reduce(message);
        assert_eq!(SidebarProjection::project(&model), before);
    }
}

#[test]
fn sidebar_inputs_change_the_projection() {
    let model = scene("live-files-plain");
    let before = SidebarProjection::project(&model);
    let changes = [
        ("设置页", ShellMessage::Navigate(ShellPage::Settings)),
        ("展开播放集", ShellMessage::Sidebar(SidebarMessage::TogglePlaylists)),
        ("任务弹层", ShellMessage::Admin(crate::shell::AdminMessage::ToggleTaskPopover)),
        ("回收站", ShellMessage::SetWorkspacePanel(crate::shell::WorkspacePanel::Trash)),
        ("最近分类", ShellMessage::SetLibraryCategory(crate::shell::LibraryCategory::Recent)),
    ];
    for (label, message) in changes {
        let mut changed = model.clone();
        changed.reduce(message);
        assert_ne!(SidebarProjection::project(&changed), before, "{label}应该改侧栏投影");
    }
    // 对话框在浮层里，不改侧栏。
    let mut dialog = model.clone();
    dialog.reduce(ShellMessage::Sidebar(SidebarMessage::Gap(GapMessage::OpenFolderCreate(String::new()))));
    assert!(dialog.sidebar.folder_dialog.open);
    assert_eq!(SidebarProjection::project(&dialog), before, "打开文件夹对话框不该改侧栏投影");
}

/// 一串无关更新之后侧栏的节点一个都不换；改了侧栏读到的状态后侧栏重挂，和新挂的一样。
#[test]
fn unrelated_updates_keep_the_sidebar_nodes() {
    let mut harness = ShellHarness::mount(scene("search-results"));
    let root = harness.sidebar_root().expect("有侧栏");
    let rows = ["repository-switcher", "shortcut-all", "sidebar-footer", "footer-settings"]
        .map(|key| harness.keyed(key).unwrap_or_else(|| panic!("侧栏缺少 {key}")));
    for message in unrelated() {
        harness.apply(message);
        harness.flush();
        assert_eq!(harness.sidebar_root(), Some(root), "无关更新重挂了侧栏");
        for (key, id) in ["repository-switcher", "shortcut-all", "sidebar-footer", "footer-settings"].iter().zip(rows) {
            assert_eq!(harness.keyed(key), Some(id), "无关更新换掉了侧栏节点 {key}");
        }
    }
    harness.assert_same_as_fresh_mount();

    harness.apply(ShellMessage::Navigate(ShellPage::Settings));
    harness.flush();
    assert_ne!(harness.sidebar_root(), Some(root), "设置入口亮起来，侧栏要重挂");
    harness.assert_same_as_fresh_mount();
}
