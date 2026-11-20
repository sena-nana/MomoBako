//! 复制、等待、侧栏读取、多选删除与回收站恢复清空时操作行的测试。

use super::*;

/// 复制开始是「复制文件 · 创建硬链接或复制文件」32%，协议完成、重读目录时走到「刷新文件索引」84%，
/// 目录读回来才收起；进行中「任务」的计数算上这一行。
#[test]
fn copying_shows_the_operation_until_the_directory_is_reloaded() {
    let mut model = files_page();
    assert!(operation_row(&model).is_none());
    let idle = model.task_rows().len();

    files(&mut model, FilesMessage::ActivateRow("cover.png".into()));
    files(&mut model, FilesMessage::OpenDialog(FileDialog::Copy));
    files(&mut model, FilesMessage::DraftChanged("assets".into()));
    files(&mut model, FilesMessage::SubmitDialog);
    assert_eq!(
        operation_row(&model),
        Some(("复制文件".into(), "创建硬链接或复制文件".into(), 32.0))
    );
    assert_eq!(model.task_rows().len(), idle + 1);

    files(
        &mut model,
        FilesMessage::ProtocolFinished {
            result: Ok(()),
            reload: true,
            hardlinks: false,
            folder_tree: None,
        },
    );
    assert_eq!(
        operation_row(&model),
        Some(("复制文件".into(), "刷新文件索引".into(), 84.0))
    );

    let message = reloaded_target(&model);
    model.reduce(message);
    assert_eq!(
        operation_row(&model),
        Some(("复制文件".into(), "刷新文件索引".into(), 100.0)),
        "目录读回来先停在 100%"
    );
    model.operation.tick(FINISH_LINGER_MS);
    assert!(operation_row(&model).is_none(), "停留 180 ms 以后收起");
    assert_eq!(model.task_rows().len(), idle);
}

/// 等待中的进度每 220 ms 往上爬：复制从 32% 起，下一拍 35%，再一拍过了 35 改加 3。
#[test]
fn waiting_operations_climb_like_vue() {
    let mut model = files_page();
    files(&mut model, FilesMessage::ActivateRow("cover.png".into()));
    files(&mut model, FilesMessage::OpenDialog(FileDialog::Copy));
    files(&mut model, FilesMessage::DraftChanged("assets".into()));
    files(&mut model, FilesMessage::SubmitDialog);
    model.operation.tick(CLIMB_INTERVAL_MS);
    assert_eq!(operation_row(&model).map(|row| row.2), Some(37.0));
    model.operation.tick(CLIMB_INTERVAL_MS);
    assert_eq!(operation_row(&model).map(|row| row.2), Some(40.0));
}

/// 侧栏的回收站快捷方式和快捷访问是非静默读取，任务弹层出「读取回收站」「读取目录」，读完走到
/// 「整理目录条目」再完成；点文件夹是静默读取，不出这一行。
#[test]
fn sidebar_reads_report_like_vue() {
    use super::super::sidebar::{ShortcutId, SidebarMessage};
    // 宿主的准备帧：交出侧栏排下的目录读取，记下文件列在读，再提交。
    fn prepare(model: &mut ShellViewModel) {
        model.stage_browses();
        crate::dispatch::sidebar::dispatch_prepared_browses(model);
    }
    let mut model = files_page();
    model.reduce(ShellMessage::Sidebar(SidebarMessage::SelectShortcut(
        ShortcutId::Trash,
    )));
    prepare(&mut model);
    assert_eq!(
        operation_row(&model),
        Some(("读取回收站".into(), "正在读取回收站".into(), 14.0))
    );
    let mut trash = match reloaded(&model) {
        ShellMessage::FileBrowserLoaded(Ok(snapshot)) => snapshot,
        _ => unreachable!(),
    };
    trash.special_location = Some("trash".into());
    model.reduce(ShellMessage::FileBrowserLoaded(Ok(trash)));
    assert_eq!(
        operation_row(&model),
        Some(("读取回收站".into(), "整理目录条目".into(), 100.0))
    );
    model.operation.tick(FINISH_LINGER_MS);

    assert!(operation_row(&model).is_none(), "停留以后收起");
    model.reduce(ShellMessage::Sidebar(SidebarMessage::OpenFolder(
        "assets".into(),
    )));
    prepare(&mut model);
    assert!(model.files.loading, "文件列在读 assets");
    assert!(operation_row(&model).is_none(), "点文件夹是静默读取");
}

/// 多选删除（和移动）成功以后非静默地重读目录：进度行换成「读取目录」，读回来走到「整理目录条目」再完成。
#[test]
fn multi_deletes_hand_the_row_to_the_directory_read() {
    let mut model = files_page();
    let both = vec!["cover.png".to_string(), "notes/page.pdf".to_string()];
    model
        .files
        .set_drag_selection(both, Some("cover.png".into()), Some("cover.png".into()));
    files(&mut model, FilesMessage::DeleteSelected);
    files(
        &mut model,
        FilesMessage::ProtocolFinished {
            result: Ok(()),
            reload: true,
            hardlinks: false,
            folder_tree: None,
        },
    );
    assert_eq!(
        operation_row(&model),
        Some(("读取目录".into(), "正在读取根目录".into(), 14.0))
    );
    let message = reloaded(&model);
    model.reduce(message);
    assert_eq!(
        operation_row(&model),
        Some(("读取目录".into(), "整理目录条目".into(), 100.0))
    );
}

/// 只有多选删除出进度行，单条删除不出；变更失败时收起。
#[test]
fn only_multi_delete_shows_the_operation_and_failures_clear_it() {
    let mut model = files_page();
    model.files.set_drag_selection(
        vec!["cover.png".into()],
        Some("cover.png".into()),
        Some("cover.png".into()),
    );
    files(&mut model, FilesMessage::DeleteSelected);
    assert!(model.files.mutating);
    assert!(operation_row(&model).is_none(), "单条删除不出进度行");
    files(
        &mut model,
        FilesMessage::ProtocolFinished {
            result: Err("删除失败".into()),
            reload: false,
            hardlinks: false,
            folder_tree: None,
        },
    );

    let both = vec!["cover.png".to_string(), "notes/page.pdf".to_string()];
    model
        .files
        .set_drag_selection(both, Some("cover.png".into()), Some("cover.png".into()));
    files(&mut model, FilesMessage::DeleteSelected);
    assert_eq!(
        operation_row(&model),
        Some(("删除文件".into(), "准备处理 2 个条目".into(), 10.0))
    );
    files(
        &mut model,
        FilesMessage::ProtocolFinished {
            result: Err("删除失败".into()),
            reload: false,
            hardlinks: false,
            folder_tree: None,
        },
    );
    assert!(operation_row(&model).is_none(), "失败时收起");
    assert_eq!(model.files.error, "删除失败");
}

/// 删除成功后静默重读仓库摘要，侧栏「回收站」换成摘要里的
/// `trashCount`，摘要没回来以前不自己加一。失败不读。清空回收站还静默重读仓库列表。
#[test]
fn trash_count_follows_the_snapshot_after_delete_and_empty() {
    let mut model = ShellViewModel::for_page(ShellPage::FileList);
    let repo_id = model.workspace.active_repo_id.clone().expect("活动仓库");
    assert_eq!(model.sidebar.counts.trash, 0, "夹具摘要里回收站是 0");
    assert!(model.workspace.effects.is_empty());

    model.files.set_drag_selection(
        vec!["cover.png".into()],
        Some("cover.png".into()),
        Some("cover.png".into()),
    );
    files(&mut model, FilesMessage::DeleteSelected);
    files(
        &mut model,
        FilesMessage::ProtocolFinished {
            result: Ok(()),
            reload: true,
            hardlinks: false,
            folder_tree: None,
        },
    );
    let effects = model.workspace.take_effects();
    assert!(
        effects.iter().any(|effect| matches!(effect, WorkspaceEffect::LoadSnapshotSilent { repo_id: id } if id == &repo_id)),
        "删除成功要静默重读摘要：{effects:?}"
    );
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, WorkspaceEffect::RefreshRepositoriesSilent)),
        "普通删除不重读仓库列表"
    );
    assert_eq!(model.sidebar.counts.trash, 0, "摘要还没回来，计数不动");

    model.reduce(reloaded(&model));
    model.reduce(ShellMessage::SilentWorkspace(SilentMessage::Snapshot(Ok(
        snapshot_with_trash(&repo_id, 4),
    ))));
    assert_eq!(
        model.sidebar.counts.trash, 4,
        "侧栏回收站换成摘要里的 trashCount"
    );

    model.files.set_drag_selection(
        vec!["notes/page.pdf".into()],
        Some("notes/page.pdf".into()),
        Some("notes/page.pdf".into()),
    );
    files(&mut model, FilesMessage::DeleteSelected);
    files(
        &mut model,
        FilesMessage::ProtocolFinished {
            result: Err("删除失败".into()),
            reload: false,
            hardlinks: false,
            folder_tree: None,
        },
    );
    assert!(model.workspace.effects.is_empty(), "删除失败不重读摘要");
    assert_eq!(model.sidebar.counts.trash, 4, "失败不改回收站计数");

    model.workspace.panel = WorkspacePanel::Trash;
    files(&mut model, FilesMessage::EmptyTrash);
    let ShellMessage::FileBrowserLoaded(Ok(mut browser)) = reloaded(&model) else {
        unreachable!("重读目录的消息");
    };
    browser.special_location = Some("trash".into());
    browser.entries.clear();
    files(
        &mut model,
        FilesMessage::MutationSnapshot {
            result: Ok(browser),
            created_name: None,
        },
    );
    let effects = model.workspace.take_effects();
    assert!(
        effects.iter().any(|effect| matches!(effect, WorkspaceEffect::LoadSnapshotSilent { repo_id: id } if id == &repo_id)),
        "清空成功要重读摘要：{effects:?}"
    );
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, WorkspaceEffect::RefreshRepositoriesSilent)),
        "清空回收站还要静默重读仓库列表"
    );
    model.reduce(ShellMessage::SilentWorkspace(SilentMessage::Snapshot(Ok(
        snapshot_with_trash(&repo_id, 0),
    ))));
    assert_eq!(
        model.sidebar.counts.trash, 0,
        "清空以后的摘要把回收站计数归零"
    );
}

/// 多选还原：成功后丢掉变更快照，清空选择，非静默重读回收站。
/// 任务弹层从「还原文件」换成「读取回收站」，文件列换成「正在读取目录」。
/// 失败不重读、选择留着。单条还原仍采用快照，不出「读取回收站」。
#[test]
fn multi_restore_reloads_the_trash_and_clears_the_selection() {
    let mut model = files_page();
    model.workspace.panel = WorkspacePanel::Trash;
    let both = vec!["cover.png".to_string(), "notes/page.pdf".to_string()];
    model.files.set_drag_selection(
        both.clone(),
        Some("cover.png".into()),
        Some("cover.png".into()),
    );
    let listed = model.files.entry_names();

    files(&mut model, FilesMessage::RestoreSelected);
    assert_eq!(
        operation_row(&model),
        Some(("还原文件".into(), "准备还原 2 个条目".into(), 10.0))
    );
    assert!(model.files.mutating, "还原进行中文件列是正在处理文件");
    assert!(!model.files.loading);

    files(
        &mut model,
        FilesMessage::MutationSnapshot {
            result: Err("还原失败".into()),
            created_name: None,
        },
    );
    assert_eq!(model.files.error, "还原失败");
    assert_eq!(model.files.selected, both, "失败不改选择");
    assert!(!model.files.loading, "失败不重读回收站");
    assert!(operation_row(&model).is_none(), "失败收起还原的进度行");
    assert!(model.workspace.effects.is_empty(), "失败不重读摘要");
    assert_eq!(model.files.entry_names(), listed);

    files(&mut model, FilesMessage::RestoreSelected);
    let mut discarded = match reloaded(&model) {
        ShellMessage::FileBrowserLoaded(Ok(browser)) => browser,
        _ => unreachable!("重读目录的消息"),
    };
    discarded.special_location = Some("trash".into());
    discarded.entries = vec![browser_file("not-applied.png")];
    discarded.total_entries = 1;
    discarded.loaded_count = 1;
    files(
        &mut model,
        FilesMessage::MutationSnapshot {
            result: Ok(discarded),
            created_name: None,
        },
    );
    assert_eq!(model.files.entry_names(), listed, "多选还原不采用变更快照");
    assert!(model.files.selected.is_empty(), "成功先清空选择");
    assert!(
        model.files.error.is_empty(),
        "重读前清掉失败文案，文件列才能写成正在读取目录"
    );
    assert!(model.files.loading, "文件列换成正在读取目录");
    assert!(!model.files.mutating);
    assert_eq!(
        operation_row(&model),
        Some(("读取回收站".into(), "正在读取回收站".into(), 14.0))
    );
    let effects = model.files.take_effects();
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, FilesEffect::Browse { trash: true, .. })),
        "要重读回收站：{effects:?}"
    );
    let workspace = model.workspace.take_effects();
    assert!(
        workspace
            .iter()
            .any(|effect| matches!(effect, WorkspaceEffect::LoadSnapshotSilent { .. })),
        "多选还原仍静默重读摘要：{workspace:?}"
    );
    assert!(
        workspace
            .iter()
            .any(|effect| matches!(effect, WorkspaceEffect::RefreshRepositoriesSilent)),
        "多选还原仍静默重读仓库列表"
    );

    let mut browser = match reloaded(&model) {
        ShellMessage::FileBrowserLoaded(Ok(browser)) => browser,
        _ => unreachable!("重读目录的消息"),
    };
    browser.special_location = Some("trash".into());
    browser.entries = vec![browser_file("cover.png")];
    browser.total_entries = 1;
    browser.loaded_count = 1;
    model.reduce(ShellMessage::FileBrowserLoaded(Ok(browser)));
    assert_eq!(model.files.entry_names(), vec!["cover.png".to_string()]);
    assert!(
        model.files.selected.is_empty(),
        "重读回来也不把还在列表里的条目选回去"
    );
    assert!(!model.files.loading);
    assert_eq!(
        operation_row(&model),
        Some(("读取回收站".into(), "整理目录条目".into(), 100.0))
    );

    let mut single = files_page();
    single.workspace.panel = WorkspacePanel::Trash;
    single.files.set_drag_selection(
        vec!["cover.png".into()],
        Some("cover.png".into()),
        Some("cover.png".into()),
    );
    files(&mut single, FilesMessage::RestoreSelected);
    assert!(operation_row(&single).is_none(), "单条还原不出进度行");
    let mut kept = match reloaded(&single) {
        ShellMessage::FileBrowserLoaded(Ok(browser)) => browser,
        _ => unreachable!("重读目录的消息"),
    };
    kept.special_location = Some("trash".into());
    kept.entries = vec![browser_file("notes/page.pdf")];
    kept.total_entries = 1;
    kept.loaded_count = 1;
    files(
        &mut single,
        FilesMessage::MutationSnapshot {
            result: Ok(kept),
            created_name: None,
        },
    );
    assert_eq!(
        single.files.entry_names(),
        vec!["notes/page.pdf".to_string()],
        "单条还原采用快照"
    );
    assert!(!single.files.loading, "单条还原不把列表换成正在读取目录");
    assert!(operation_row(&single).is_none());
    assert!(single.files.selected.is_empty(), "还原掉的条目不留在选择里");
}
