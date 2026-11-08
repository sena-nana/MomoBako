//! 选中里有文件夹时，移到别的目录或多选删除成功后另读一次侧栏树。
//!
//! 对应 Vue `defaultDirectoryRefreshPlan` 得到 `currentWithTree` 时调用的 `refreshRepositoryTree`：
//! 回收站不读；移进当前目录、单条删除、复制、选中里没有文件夹，都不读。
//! 协议结果里的树仍按原规则写入。另读失败只留日志，不编一棵新树。选择仍是移走留第一条、删除等重读后修剪。

use crate::shell::{ShellMessage, SidebarEffect, SidebarMessage, SidebarTree, WorkspacePanel};

use super::{
    FileDialog, FilesMessage, LibraryCategory, files, files_page, finished, folder_paths,
    protocol_tree, reloaded, tree_node,
};

fn quiet(model: &mut crate::shell::ShellViewModel) {
    let _ = model.sidebar.take_effects();
}

fn requested_tree(model: &mut crate::shell::ShellViewModel) -> Vec<SidebarEffect> {
    model.sidebar.take_effects()
}

/// 移走文件夹、以及多选里带文件夹的删除，成功后各发一次目录树读取。
/// 读回来才换侧栏；失败留下原来的树。其余变更不发这次读取。
#[test]
fn folder_selection_rereads_sidebar_tree() {
    let mut away = files_page();
    let names = away.files.entry_names();
    away.files.set_drag_selection(
        vec!["assets".into()],
        Some("assets".into()),
        Some("assets".into()),
    );
    away.files.enqueue_move(
        "acceptance-repo".into(),
        vec!["assets".into()],
        "albums".into(),
    );
    quiet(&mut away);
    files(
        &mut away,
        finished(protocol_tree(
            "albums",
            vec![tree_node("from-protocol", 4, vec![])],
        )),
    );
    assert_eq!(away.files.entry_names(), names, "移走不整份采用协议快照");
    assert_eq!(
        away.files.selected,
        vec!["cover.png".to_string()],
        "移走后选还留着的第一条"
    );
    assert_eq!(away.files.primary.as_deref(), Some("cover.png"));
    assert_eq!(
        folder_paths(&away),
        vec!["from-protocol".to_string()],
        "协议树仍写入"
    );
    assert_eq!(
        away.sidebar.folder_counts.get("from-protocol").copied(),
        Some(4)
    );
    assert!(away.sidebar.tree_loading, "另读已经发出");
    assert_eq!(
        requested_tree(&mut away),
        vec![SidebarEffect::LoadTree {
            repo_id: "acceptance-repo".into()
        }],
        "只另读这一次目录树"
    );
    let read = SidebarTree::from_nodes(&[tree_node("fresh", 7, vec![])]);
    away.reduce(ShellMessage::Sidebar(SidebarMessage::SidebarTreeLoaded {
        repo_id: "acceptance-repo".into(),
        result: Ok(read),
    }));
    assert_eq!(
        folder_paths(&away),
        vec!["fresh".to_string()],
        "另读回来以后换上这棵树"
    );
    assert_eq!(away.sidebar.folder_counts.get("fresh").copied(), Some(7));
    assert_eq!(
        away.files.selected,
        vec!["cover.png".to_string()],
        "另读不改移动留下的选择"
    );
    let kept = folder_paths(&away);
    away.reduce(ShellMessage::Sidebar(SidebarMessage::SidebarTreeLoaded {
        repo_id: "acceptance-repo".into(),
        result: Err("拒绝访问".into()),
    }));
    assert_eq!(folder_paths(&away), kept, "另读失败不编树，侧栏留着上一份");
    assert!(!away.sidebar.tree_loading);

    let mut removed = files_page();
    let names = removed.files.entry_names();
    let both = vec!["assets".to_string(), "cover.png".to_string()];
    removed
        .files
        .set_drag_selection(both.clone(), Some("assets".into()), Some("assets".into()));
    files(&mut removed, FilesMessage::DeleteSelected);
    quiet(&mut removed);
    files(
        &mut removed,
        finished(protocol_tree("", vec![tree_node("kept", 5, vec![])])),
    );
    assert_eq!(
        removed.files.entry_names(),
        names,
        "多选删除不整份采用协议快照"
    );
    assert_eq!(removed.files.selected, both, "重读目录以前选择先留着");
    assert_eq!(folder_paths(&removed), vec!["kept".to_string()]);
    assert_eq!(
        requested_tree(&mut removed),
        vec![SidebarEffect::LoadTree {
            repo_id: "acceptance-repo".into()
        }]
    );
    let mut after = match reloaded(&removed) {
        ShellMessage::FileBrowserLoaded(Ok(browser)) => browser,
        _ => unreachable!("重读目录的消息"),
    };
    after.entries = vec![super::named_file("notes/page.pdf", "page.pdf", "file")];
    after.total_entries = 1;
    after.loaded_count = 1;
    after.tree = None;
    removed.reduce(ShellMessage::FileBrowserLoaded(Ok(after)));
    assert_eq!(
        removed.files.selected,
        Vec::<String>::new(),
        "删掉的条目在重读后从选择里修剪掉"
    );
    assert_eq!(
        folder_paths(&removed),
        vec!["kept".to_string()],
        "没有树的目录重读不改协议写下的侧栏"
    );

    let mut plain = files_page();
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
    quiet(&mut plain);
    files(
        &mut plain,
        finished(protocol_tree(
            "albums",
            vec![tree_node("file-move", 1, vec![])],
        )),
    );
    assert!(
        requested_tree(&mut plain).is_empty(),
        "移走的是文件，不另读侧栏树"
    );
    assert_eq!(folder_paths(&plain), vec!["file-move".to_string()]);

    let mut once = files_page();
    once.files.set_drag_selection(
        vec!["assets".into()],
        Some("assets".into()),
        Some("assets".into()),
    );
    files(&mut once, FilesMessage::DeleteSelected);
    quiet(&mut once);
    files(
        &mut once,
        finished(protocol_tree("", vec![tree_node("single", 2, vec![])])),
    );
    assert!(requested_tree(&mut once).is_empty(), "单条删除不另读侧栏树");
    assert_eq!(folder_paths(&once), vec!["single".to_string()]);

    let mut here = files_page();
    here.files.current_path = "albums".into();
    here.files.set_drag_selection(
        vec!["assets".into()],
        Some("assets".into()),
        Some("assets".into()),
    );
    here.files.enqueue_move(
        "acceptance-repo".into(),
        vec!["assets".into()],
        "albums".into(),
    );
    quiet(&mut here);
    files(
        &mut here,
        finished(protocol_tree(
            "albums",
            vec![tree_node("same-dir", 3, vec![])],
        )),
    );
    assert_eq!(
        here.files.selected,
        vec!["assets".to_string()],
        "同目录移动先留着原来的选择"
    );
    assert!(
        requested_tree(&mut here).is_empty(),
        "移进当前目录不另读侧栏树"
    );

    let mut failed = files_page();
    let kept = folder_paths(&failed);
    failed.files.set_drag_selection(
        vec!["assets".into()],
        Some("assets".into()),
        Some("assets".into()),
    );
    failed.files.enqueue_move(
        "acceptance-repo".into(),
        vec!["assets".into()],
        "albums".into(),
    );
    quiet(&mut failed);
    files(
        &mut failed,
        FilesMessage::ProtocolFinished {
            result: Err("移动失败".into()),
            reload: false,
            hardlinks: false,
            folder_tree: protocol_tree("", vec![tree_node("nope", 1, vec![])]),
        },
    );
    assert_eq!(
        failed.files.selected,
        vec!["assets".to_string()],
        "失败不改选"
    );
    assert_eq!(folder_paths(&failed), kept, "失败不写侧栏");
    assert!(requested_tree(&mut failed).is_empty(), "失败不另读侧栏树");

    let mut trash = files_page();
    trash.workspace.panel = WorkspacePanel::Trash;
    trash.files.set_drag_selection(
        vec!["assets".into(), "cover.png".into()],
        Some("assets".into()),
        Some("assets".into()),
    );
    files(&mut trash, FilesMessage::DeleteSelected);
    quiet(&mut trash);
    files(
        &mut trash,
        finished(protocol_tree("", vec![tree_node("trash-tree", 1, vec![])])),
    );
    assert!(
        requested_tree(&mut trash).is_empty(),
        "回收站里的多选删除不另读侧栏树"
    );
    assert_eq!(folder_paths(&trash), vec!["trash-tree".to_string()]);

    let mut files_only = files_page();
    files_only.files.set_drag_selection(
        vec!["cover.png".into(), "notes/page.pdf".into()],
        Some("cover.png".into()),
        Some("cover.png".into()),
    );
    files(&mut files_only, FilesMessage::DeleteSelected);
    quiet(&mut files_only);
    files(&mut files_only, finished(None));
    assert!(
        requested_tree(&mut files_only).is_empty(),
        "多选里没有文件夹，不另读侧栏树"
    );

    let mut copied = files_page();
    copied.files.set_drag_selection(
        vec!["assets".into()],
        Some("assets".into()),
        Some("assets".into()),
    );
    files(&mut copied, FilesMessage::OpenDialog(FileDialog::Copy));
    files(&mut copied, FilesMessage::DraftChanged("albums".into()));
    files(&mut copied, FilesMessage::SubmitDialog);
    quiet(&mut copied);
    files(
        &mut copied,
        finished(protocol_tree(
            "albums",
            vec![tree_node("copied-folder", 2, vec![])],
        )),
    );
    assert_eq!(
        copied.files.selected,
        vec!["assets".to_string()],
        "复制在目标目录读回来以前不改选"
    );
    assert!(requested_tree(&mut copied).is_empty(), "复制不另读侧栏树");
    assert_eq!(folder_paths(&copied), vec!["copied-folder".to_string()]);

    let mut category = files_page();
    category.workspace.library_category = LibraryCategory::Uncategorized;
    category.files.set_drag_selection(
        vec!["assets".into()],
        Some("assets".into()),
        Some("assets".into()),
    );
    category.files.enqueue_move(
        "acceptance-repo".into(),
        vec!["assets".into()],
        "albums".into(),
    );
    quiet(&mut category);
    files(
        &mut category,
        finished(protocol_tree("", vec![tree_node("hidden", 1, vec![])])),
    );
    assert!(
        requested_tree(&mut category).is_empty(),
        "分类视图里看不见这个文件夹，不另读"
    );

    let mut other = files_page();
    let kept = folder_paths(&other);
    quiet(&mut other);
    other.sidebar.reload_folder_tree("other-repo");
    assert_eq!(folder_paths(&other), kept, "仓库对不上时不改侧栏");
    assert!(
        requested_tree(&mut other).is_empty(),
        "仓库对不上时不发读取"
    );
}
