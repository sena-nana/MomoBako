//! 快照目录树与协议目录树替换侧栏文件夹的测试。

use super::*;

/// 目录快照带着文件夹树时，侧栏目录换成这份树和每个目录的文件数。
/// 没有树时侧栏不动。分类视图路径对不上、过期的目录快照、多选还原丢掉的快照都不写。
#[test]
fn snapshot_tree_replaces_the_sidebar_folders() {
    let mut model = files_page();
    let original = folder_paths(&model);
    assert!(
        original.iter().any(|path| path == "assets"),
        "场景里本来有 assets"
    );

    let mut created = browser_of(&model);
    created.tree = Some(vec![
        tree_node("assets", 2, vec![tree_node("assets/covers", 1, vec![])]),
        tree_node("incoming", 4, vec![]),
    ]);
    created.entries = vec![browser_file("incoming")];
    created.total_entries = 1;
    created.loaded_count = 1;
    files(
        &mut model,
        FilesMessage::MutationSnapshot {
            result: Ok(created),
            created_name: None,
        },
    );
    assert_eq!(
        folder_paths(&model),
        vec![
            "assets".to_string(),
            "assets/covers".to_string(),
            "incoming".to_string()
        ],
        "新建文件夹的快照树写进侧栏"
    );
    assert_eq!(
        model
            .sidebar
            .folders
            .iter()
            .find(|folder| folder.path == "incoming")
            .map(|folder| folder.label.as_str()),
        Some("incoming")
    );
    assert_eq!(
        model.sidebar.folder_counts.get("incoming").copied(),
        Some(4),
        "目录行上的文件数跟着树走"
    );
    assert_eq!(
        model.sidebar.folder_counts.get("assets/covers").copied(),
        Some(1)
    );

    let mut plain = browser_of(&model);
    plain.tree = None;
    files(
        &mut model,
        FilesMessage::MutationSnapshot {
            result: Ok(plain),
            created_name: None,
        },
    );
    assert!(
        folder_paths(&model).iter().any(|path| path == "incoming"),
        "没有树的快照不改侧栏"
    );

    let mut emptied = browser_of(&model);
    emptied.tree = Some(Vec::new());
    files(
        &mut model,
        FilesMessage::MutationSnapshot {
            result: Ok(emptied),
            created_name: None,
        },
    );
    assert!(
        model.sidebar.folders.is_empty(),
        "空树也要换上，侧栏不再留着旧目录"
    );
    assert!(model.sidebar.folder_counts.is_empty());

    let mut category = files_page();
    let kept = folder_paths(&category);
    category.workspace.library_category = LibraryCategory::Uncategorized;
    let mut mismatch = browser_of(&category);
    mismatch.current_path = "elsewhere".into();
    mismatch.tree = Some(vec![tree_node("only-elsewhere", 1, vec![])]);
    files(
        &mut category,
        FilesMessage::MutationSnapshot {
            result: Ok(mismatch),
            created_name: None,
        },
    );
    assert_eq!(
        folder_paths(&category),
        kept,
        "分类视图路径对不上时不换侧栏树"
    );

    let mut trash = files_page();
    let kept = folder_paths(&trash);
    trash.workspace.panel = WorkspacePanel::Trash;
    trash.files.set_drag_selection(
        vec!["cover.png".into(), "notes/page.pdf".into()],
        Some("cover.png".into()),
        Some("cover.png".into()),
    );
    files(&mut trash, FilesMessage::RestoreSelected);
    let mut discarded = browser_of(&trash);
    discarded.special_location = Some("trash".into());
    discarded.tree = Some(vec![tree_node("from-restore", 9, vec![])]);
    files(
        &mut trash,
        FilesMessage::MutationSnapshot {
            result: Ok(discarded),
            created_name: None,
        },
    );
    assert_eq!(folder_paths(&trash), kept, "多选还原丢掉的快照不写侧栏");

    let mut stale_page = files_page();
    let kept = folder_paths(&stale_page);
    files(&mut stale_page, FilesMessage::OpenPath("assets".into()));
    let mut stale = browser_of(&stale_page);
    stale.tree = Some(vec![tree_node("stale-folder", 1, vec![])]);
    stale_page.reduce(ShellMessage::FileBrowserLoaded(Ok(stale)));
    assert_eq!(folder_paths(&stale_page), kept, "过期的目录快照不写侧栏");

    let mut browse = files_page();
    files(&mut browse, FilesMessage::OpenPath("assets".into()));
    let mut fresh = browser_of(&browse);
    fresh.current_path = "assets".into();
    fresh.tree = Some(vec![tree_node("from-browse", 3, vec![])]);
    browse.reduce(ShellMessage::FileBrowserLoaded(Ok(fresh)));
    assert_eq!(
        folder_paths(&browse),
        vec!["from-browse".to_string()],
        "对得上的目录快照带着树时换侧栏"
    );
    assert_eq!(
        browse.sidebar.folder_counts.get("from-browse").copied(),
        Some(3)
    );
}

/// 复制、移动、删除的协议结果带着文件夹树时，只把树和每个目录的文件数写进侧栏。
///
/// 选择仍走原来的规则：移到别的目录选还留着的第一条，移进当前目录等重读后按来源文件名，
/// 复制也等目标目录读回来再按来源文件名。没有树、失败、别的仓库、分类视图路径对不上、
/// 在途读取和这份快照对不上，侧栏都不动。重读回来的目录快照没有树时，侧栏留着协议写下的那份。
#[test]
fn protocol_tree_replaces_the_sidebar_folders() {
    use crate::backend::services::repository::{FileBrowserSnapshot, FileTreeNode};
    use crate::dispatch::files::protocol_folder_tree;
    let node = |path: &str, file_count: usize| FileTreeNode {
        path: path.into(),
        label: path.into(),
        file_count,
        children: Vec::new(),
    };
    let snapshot = |current_path: &str, tree: Option<Vec<FileTreeNode>>| FileBrowserSnapshot {
        current_path: current_path.into(),
        tree,
        ..super::super::data_load_tests::browser("acceptance-repo")
    };
    let parsed = protocol_folder_tree(&snapshot("albums", Some(vec![node("albums", 3)])))
        .expect("带树的协议结果");
    assert_eq!(parsed.current_path, "albums");
    assert!(!parsed.trash);
    assert_eq!(parsed.nodes[0].file_count, 3);
    assert!(protocol_folder_tree(&snapshot("", None)).is_none());
    let trash = FileBrowserSnapshot {
        special_location: Some("trash".into()),
        ..snapshot("", Some(vec![node("left", 1)]))
    };
    let parsed = protocol_folder_tree(&trash).expect("回收站里删除后的树");
    assert_eq!(parsed.nodes[0].path, "left");
    assert!(parsed.trash);
    let orphan = FileBrowserSnapshot {
        repo_id: String::new(),
        ..snapshot("", Some(Vec::new()))
    };
    assert!(protocol_folder_tree(&orphan).is_none(), "没有仓库的树不写");

    let mut away = files_page();
    away.files.set_drag_selection(
        vec!["cover.png".into()],
        Some("cover.png".into()),
        Some("cover.png".into()),
    );
    away.files.select_only = Some("cover.png".into());
    away.files.enqueue_move(
        "acceptance-repo".into(),
        vec!["cover.png".into()],
        "albums".into(),
    );
    files(
        &mut away,
        finished(protocol_tree(
            "albums",
            vec![
                tree_node("albums", 3, vec![]),
                tree_node("assets", 1, vec![tree_node("assets/covers", 2, vec![])]),
            ],
        )),
    );
    assert_eq!(
        away.files.entry_names(),
        vec![
            "assets".to_string(),
            "cover.png".to_string(),
            "page.pdf".to_string()
        ],
        "移走不整份采用快照"
    );
    assert_eq!(
        away.files.selected,
        vec!["assets".to_string()],
        "移到别的目录选还留着的第一条"
    );
    assert_eq!(away.files.primary.as_deref(), Some("assets"));
    assert!(
        away.files.select_only.is_none(),
        "新的主选是文件夹，不再只选中那个文件"
    );
    assert_eq!(
        folder_paths(&away),
        vec![
            "albums".to_string(),
            "assets".to_string(),
            "assets/covers".to_string()
        ]
    );
    assert_eq!(away.sidebar.folder_counts.get("albums").copied(), Some(3));
    assert_eq!(
        away.sidebar.folder_counts.get("assets/covers").copied(),
        Some(2)
    );
    let mut left = match reloaded(&away) {
        ShellMessage::FileBrowserLoaded(Ok(browser)) => browser,
        _ => unreachable!("重读目录的消息"),
    };
    left.entries = vec![
        named_file("assets", "assets", "directory"),
        named_file("notes/page.pdf", "page.pdf", "file"),
    ];
    left.total_entries = 2;
    left.loaded_count = 2;
    left.tree = None;
    away.reduce(ShellMessage::FileBrowserLoaded(Ok(left)));
    assert_eq!(
        away.files.selected,
        vec!["assets".to_string()],
        "没有树的重读不改已经按移动规则留下的选择"
    );
    assert_eq!(
        folder_paths(&away),
        vec![
            "albums".to_string(),
            "assets".to_string(),
            "assets/covers".to_string()
        ],
        "普通重读不带树，侧栏不重画"
    );

    let mut plain = files_page();
    let kept = folder_paths(&plain);
    plain.files.set_drag_selection(
        vec!["cover.png".into()],
        Some("cover.png".into()),
        Some("cover.png".into()),
    );
    plain.files.enqueue_move(
        "acceptance-repo".into(),
        vec!["cover.png".into()],
        "albums".into(),
    );
    files(&mut plain, finished(None));
    assert_eq!(plain.files.selected, vec!["assets".to_string()]);
    assert_eq!(folder_paths(&plain), kept, "协议结果没有树时不编侧栏");

    let mut here = files_page();
    here.files.current_path = "albums".into();
    here.files.set_drag_selection(
        vec!["albums/note.txt".into()],
        Some("albums/note.txt".into()),
        Some("albums/note.txt".into()),
    );
    here.files.enqueue_move(
        "acceptance-repo".into(),
        vec!["elsewhere/cover.png".into()],
        "albums".into(),
    );
    files(
        &mut here,
        finished(protocol_tree(
            "albums",
            vec![tree_node("from-move", 4, vec![])],
        )),
    );
    assert_eq!(
        here.files.primary.as_deref(),
        Some("albums/note.txt"),
        "同名条目还没出现时先留着原来的选择"
    );
    assert_eq!(folder_paths(&here), vec!["from-move".to_string()]);
    assert_eq!(
        here.sidebar.folder_counts.get("from-move").copied(),
        Some(4)
    );
    let mut arrived = match reloaded(&here) {
        ShellMessage::FileBrowserLoaded(Ok(browser)) => browser,
        _ => unreachable!("重读目录的消息"),
    };
    arrived.current_path = "albums".into();
    arrived.entries = vec![
        named_file("albums/note.txt", "note.txt", "file"),
        named_file("albums/cover.png", "cover.png", "file"),
    ];
    arrived.total_entries = 2;
    arrived.loaded_count = 2;
    arrived.tree = None;
    here.reduce(ShellMessage::FileBrowserLoaded(Ok(arrived)));
    assert_eq!(
        here.files.selected,
        vec!["albums/cover.png".to_string()],
        "移进当前目录后按来源文件名改选"
    );
    assert_eq!(folder_paths(&here), vec!["from-move".to_string()]);

    let mut copy = files_page();
    copy.files.set_drag_selection(
        vec!["cover.png".into()],
        Some("cover.png".into()),
        Some("cover.png".into()),
    );
    files(&mut copy, FilesMessage::OpenDialog(FileDialog::Copy));
    files(&mut copy, FilesMessage::DraftChanged("albums".into()));
    files(&mut copy, FilesMessage::SubmitDialog);
    assert!(copy.files.mutating, "复制已经提交");
    files(
        &mut copy,
        finished(protocol_tree(
            "albums",
            vec![tree_node("copied", 2, vec![])],
        )),
    );
    assert_eq!(
        copy.files.selected,
        vec!["cover.png".to_string()],
        "目标目录读回来以前不改选"
    );
    assert!(
        copy.files.current_path.is_empty(),
        "不因为协议快照改当前目录"
    );
    assert_eq!(folder_paths(&copy), vec!["copied".to_string()]);
    let mut target = match reloaded_target(&copy) {
        ShellMessage::FileBrowserLoaded(Ok(browser)) => browser,
        _ => unreachable!("重读目录的消息"),
    };
    target.current_path = "albums".into();
    target.entries = vec![
        named_file("albums/old.png", "old.png", "file"),
        named_file("albums/cover.png", "cover.png", "file"),
    ];
    target.total_entries = 2;
    target.loaded_count = 2;
    target.tree = None;
    copy.reduce(ShellMessage::FileBrowserLoaded(Ok(target)));
    assert_eq!(
        copy.files.selected,
        vec!["albums/cover.png".to_string()],
        "复制到别的目录按源文件名改选"
    );
    assert_eq!(copy.files.primary.as_deref(), Some("albums/cover.png"));
    assert_eq!(folder_paths(&copy), vec!["copied".to_string()]);

    let mut removed = files_page();
    removed.files.set_drag_selection(
        vec!["assets".into()],
        Some("assets".into()),
        Some("assets".into()),
    );
    files(&mut removed, FilesMessage::DeleteSelected);
    files(
        &mut removed,
        finished(protocol_tree("", vec![tree_node("kept", 5, vec![])])),
    );
    assert_eq!(
        removed.files.entry_names(),
        vec![
            "assets".to_string(),
            "cover.png".to_string(),
            "page.pdf".to_string()
        ],
        "删除不整份采用快照"
    );
    assert_eq!(
        removed.files.selected,
        vec!["assets".to_string()],
        "重读回来以前选择先留着"
    );
    assert_eq!(folder_paths(&removed), vec!["kept".to_string()]);
    assert_eq!(removed.sidebar.folder_counts.get("kept").copied(), Some(5));
    let mut after_delete = match reloaded(&removed) {
        ShellMessage::FileBrowserLoaded(Ok(browser)) => browser,
        _ => unreachable!("重读目录的消息"),
    };
    after_delete.entries = vec![
        named_file("cover.png", "cover.png", "file"),
        named_file("notes/page.pdf", "page.pdf", "file"),
    ];
    after_delete.total_entries = 2;
    after_delete.loaded_count = 2;
    after_delete.tree = None;
    removed.reduce(ShellMessage::FileBrowserLoaded(Ok(after_delete)));
    assert!(
        removed.files.selected.is_empty(),
        "删掉的文件夹在重读后从选择里修剪掉"
    );
    assert_eq!(folder_paths(&removed), vec!["kept".to_string()]);

    let mut failed = files_page();
    let kept = folder_paths(&failed);
    failed.files.set_drag_selection(
        vec!["cover.png".into()],
        Some("cover.png".into()),
        Some("cover.png".into()),
    );
    failed.files.enqueue_move(
        "acceptance-repo".into(),
        vec!["cover.png".into()],
        "albums".into(),
    );
    files(
        &mut failed,
        FilesMessage::ProtocolFinished {
            result: Err("移动失败".into()),
            reload: false,
            hardlinks: false,
            folder_tree: protocol_tree("", vec![tree_node("should-not", 1, vec![])]),
        },
    );
    assert_eq!(
        failed.files.selected,
        vec!["cover.png".to_string()],
        "失败不改选"
    );
    assert_eq!(folder_paths(&failed), kept, "失败不写侧栏");

    let mut category = files_page();
    let kept = folder_paths(&category);
    category.workspace.library_category = LibraryCategory::Uncategorized;
    files(
        &mut category,
        finished(protocol_tree(
            "elsewhere",
            vec![tree_node("only-elsewhere", 1, vec![])],
        )),
    );
    assert_eq!(
        folder_paths(&category),
        kept,
        "分类视图路径对不上时不换侧栏树"
    );
    assert!(category.files.selected.is_empty());

    let mut category_ok = files_page();
    category_ok.workspace.library_category = LibraryCategory::Uncategorized;
    files(
        &mut category_ok,
        finished(protocol_tree(
            "",
            vec![tree_node("from-category", 2, vec![])],
        )),
    );
    assert_eq!(
        folder_paths(&category_ok),
        vec!["from-category".to_string()]
    );
    assert_eq!(
        category_ok.files.entry_names(),
        vec![
            "assets".to_string(),
            "cover.png".to_string(),
            "page.pdf".to_string()
        ]
    );

    let mut other = files_page();
    let kept = folder_paths(&other);
    files(
        &mut other,
        finished(Some(ProtocolFolderTree {
            repo_id: "other-repo".into(),
            current_path: String::new(),
            trash: false,
            nodes: vec![tree_node("foreign", 1, vec![])],
        })),
    );
    assert_eq!(folder_paths(&other), kept, "别的仓库的树不写");

    let mut stale_page = files_page();
    let kept = folder_paths(&stale_page);
    files(&mut stale_page, FilesMessage::OpenPath("assets".into()));
    let mut old = browser_of(&stale_page);
    old.tree = Some(vec![tree_node("stale-folder", 1, vec![])]);
    stale_page.reduce(ShellMessage::FileBrowserLoaded(Ok(old)));
    assert_eq!(folder_paths(&stale_page), kept, "过期的目录快照不写侧栏");

    let mut late = files_page();
    let kept = folder_paths(&late);
    files(&mut late, FilesMessage::OpenPath("assets".into()));
    files(
        &mut late,
        FilesMessage::ProtocolFinished {
            result: Ok(()),
            reload: false,
            hardlinks: false,
            folder_tree: protocol_tree("", vec![tree_node("late-tree", 1, vec![])]),
        },
    );
    assert_eq!(
        folder_paths(&late),
        kept,
        "在途读取和协议快照对不上时不写侧栏"
    );

    let mut emptied = files_page();
    files(&mut emptied, finished(protocol_tree("", Vec::new())));
    assert!(emptied.sidebar.folders.is_empty(), "空树也要换上");
    assert!(emptied.sidebar.folder_counts.is_empty());
}
