//! 侧栏投影的回归：取值对、同一状态两次投影相等；哪些消息只改哪一块，和侧栏无关的消息一块都不改；
//! 按键对照写 Store 列表时删、插、改、排都落到和新列表一样的结果。

use nana_ui::runtime::view::{store, StorePath};

use super::{sync_rows, FolderRow, FooterView, SidebarView};
use crate::shell::sidebar::SidebarTree;
use crate::shell::{ShellMessage, ShellPage, ShellViewModel, SidebarMessage, ThumbnailFrame};
use crate::backend::services::repository::FileTreeNode;

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

/// 场景仓库的目录树：assets 下面一个 covers，`counts` 是两个目录各自的文件数。
pub(crate) fn tree_loaded(model: &ShellViewModel, counts: (usize, usize)) -> ShellMessage {
    let tree = vec![FileTreeNode {
        path: "assets".into(),
        label: "assets".into(),
        file_count: counts.0,
        children: vec![FileTreeNode { path: "assets/covers".into(), label: "covers".into(), file_count: counts.1, children: Vec::new() }],
    }];
    let repo_id = model.workspace.active_repo_id.clone().expect("场景有仓库");
    sidebar(SidebarMessage::SidebarTreeLoaded { repo_id, result: Ok(SidebarTree::from_nodes(&tree)) })
}

fn folder(path: &str, label: &str, depth: u16, has_children: bool) -> FolderRow {
    FolderRow {
        path: path.into(),
        label: label.into(),
        depth,
        has_children,
        expanded: false,
        active: false,
        branch: false,
        count: 0,
    }
}

#[test]
fn projection_reads_the_sidebar_state() {
    let model = scene("live-files-plain");
    let view = SidebarView::project(&model);
    assert_eq!(view.head.name, "默认资源库");
    assert!(view.head.error.is_empty());
    assert!(view.head.folders_visible);
    assert!(!view.nav.locked);
    assert_eq!(view.nav.shortcuts[0], (2, true), "全部：两个文件，文件面板的全部分类是当前项");
    assert_eq!(view.nav.shortcuts[1], (1, false), "未分类：根目录下的 cover.png");
    assert_eq!(view.nav.actions, 0);
    assert!(view.quick.is_empty());
    assert!(!view.playlists.expanded);
    assert_eq!(view.playlists.hint, Some("还没有播放集。"));
    assert!(view.folders.tree);
    assert_eq!(view.folders.hint, None);
    assert_eq!(view.folder_rows, vec![folder("assets", "assets", 1, true)], "收起的 assets 不列出子目录");
    assert_eq!(view.smart.hint, Some("还没有智能文件夹。"));
    assert_eq!(view.footer, FooterView { settings: false, extensions: false, logs: false, tasks_open: false, tasks: 0 });
    assert_eq!(SidebarView::project(&model), view, "同一状态的投影相等");

    let missing = SidebarView::project(&scene("missing"));
    assert!(missing.nav.locked, "资源库丢失时导航锁住");
    assert_eq!(missing.folders.hint, Some("资源库文件夹丢失，请先在主视图修复。"));
    assert!(!missing.folders.tree);
}

/// 进设置页只改底部入口；展开目录只改文件夹树；换目录只改树行和新建文件夹的父目录；
/// 目录树读回新的计数只改计数。
#[test]
fn each_message_changes_only_its_part() {
    let model = scene("live-files-plain");
    let before = SidebarView::project(&model);

    let mut settings = model.clone();
    settings.reduce(ShellMessage::Navigate(ShellPage::Settings));
    let after = SidebarView::project(&settings);
    assert!(after.footer.settings);
    assert_eq!(SidebarView { footer: before.footer, ..after }, before, "进设置页只该改底部入口");

    let mut expanded = model.clone();
    expanded.reduce(sidebar(SidebarMessage::ToggleFolder("assets".into())));
    let after = SidebarView::project(&expanded);
    let mut assets = folder("assets", "assets", 1, true);
    assets.expanded = true;
    assert_eq!(after.folder_rows, vec![assets, folder("assets/covers", "covers", 2, false)], "展开后子目录跟在后面");
    assert_eq!(SidebarView { folder_rows: before.folder_rows.clone(), ..after }, before, "展开目录只该改树行");

    let mut opened = expanded.clone();
    let reference = SidebarView::project(&opened);
    opened.reduce(sidebar(SidebarMessage::OpenFolder("assets/covers".into())));
    let after = SidebarView::project(&opened);
    assert_eq!(after.folders.parent, "assets/covers");
    assert!(after.folder_rows[0].branch && !after.folder_rows[0].active, "父目录在当前这一支上");
    assert!(after.folder_rows[1].active, "covers 是当前目录");
    let mut folders = after.folders.clone();
    folders.parent = reference.folders.parent.clone();
    assert_eq!(
        SidebarView { folders, folder_rows: reference.folder_rows.clone(), ..after },
        reference,
        "换目录只该改树行和父目录"
    );

    let mut counted = expanded.clone();
    let reference = SidebarView::project(&counted);
    let message = tree_loaded(&counted, (3, 5));
    counted.reduce(message);
    let after = SidebarView::project(&counted);
    assert_eq!(after.folder_rows.iter().map(|row| row.count).collect::<Vec<_>>(), [3, 5]);
    let rows = after.folder_rows.iter().map(|row| FolderRow { count: 0, ..row.clone() }).collect::<Vec<_>>();
    assert_eq!(SidebarView { folder_rows: rows, ..after }, reference, "计数变了只该改计数");
}

/// 缩略图、播放音量、搜索词，以及只换主区的路由，一块都不改。
#[test]
fn unrelated_messages_leave_the_projection_alone() {
    let mut model = scene("search-results");
    let before = SidebarView::project(&model);
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
        ShellMessage::Navigate(ShellPage::Playlists),
        ShellMessage::Navigate(ShellPage::FileList),
    ];
    for message in unrelated {
        model.reduce(message);
        assert_eq!(SidebarView::project(&model), before);
    }
}

/// 删、插、改、排混在一起，写完的列表和新列表一样；新列表里重复的键只留第一行。
#[test]
fn sync_rows_matches_the_new_list() {
    fn key(row: &(u32, &'static str)) -> u32 {
        row.0
    }
    let list = store(vec![(1, "一"), (2, "二"), (3, "三"), (4, "四")]);
    let cases: [Vec<(u32, &'static str)>; 5] = [
        vec![(1, "一"), (5, "五"), (2, "二"), (3, "三"), (4, "四")],
        vec![(1, "一"), (2, "贰"), (4, "四")],
        vec![(4, "四"), (2, "贰"), (1, "一"), (6, "六")],
        vec![(6, "六"), (6, "重复"), (1, "壹")],
        Vec::new(),
    ];
    for rows in cases {
        let mut expected = rows.clone();
        let mut seen = std::collections::HashSet::new();
        expected.retain(|row| seen.insert(row.0));
        sync_rows(list, key, rows);
        assert_eq!(list.get_untracked(), expected);
    }
}
