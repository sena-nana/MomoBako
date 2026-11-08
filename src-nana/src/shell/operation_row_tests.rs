//! 文件变更在任务弹层里的仓库操作行，对齐 Vue `operationProgress` 和 `TaskPopover.vue` 的
//! `workspace-operation`：从文件页的操作开始，看任务弹层的行和侧栏「任务」的计数。
//! 删除和回收站变更成功以后，侧栏「回收站」计数跟着静默重读的仓库摘要走。

use std::collections::BTreeMap;

use crate::backend::services::repository::{
    AssetSummary, FileBrowserEntry, FileBrowserSnapshot, FileTreeNode, RepositoryBackendSummary,
    RepositoryOverview, RepositorySnapshot, RepositoryStructureCacheState, RepositorySummary,
};

use super::files::{FileDialog, FilesMessage, ProtocolFolderTree};
use super::repo_operation::{CLIMB_INTERVAL_MS, FINISH_LINGER_MS};
use super::workspace_dialogs::HardlinkView;
use super::workspace_refresh::SilentMessage;
use super::{
    FilesEffect, HardlinkPrompt, LibraryCategory, ShellMessage, ShellPage, ShellViewModel,
    WorkspaceEffect, WorkspacePanel,
};

/// 列表模式的文件页：assets 文件夹、cover.png 和 notes/page.pdf，资源库可写。
fn files_page() -> ShellViewModel {
    super::acceptance_gap_models()
        .into_iter()
        .find(|(scene, _)| *scene == "files-list-mode")
        .map(|(_, model)| model)
        .expect("列表模式的文件页场景")
}

fn files(model: &mut ShellViewModel, message: FilesMessage) {
    model.reduce(ShellMessage::Files(message));
}

/// 任务弹层里的仓库操作行：(种类, 阶段, 百分比)。
fn operation_row(model: &ShellViewModel) -> Option<(String, String, f64)> {
    model
        .task_rows()
        .into_iter()
        .find(|row| row.id == "workspace-operation")
        .map(|row| (row.label, row.detail, row.value))
}

/// 当前目录重读回来的结果。
fn reloaded(model: &ShellViewModel) -> ShellMessage {
    reloaded_at(model, model.files.current_path.clone())
}

/// 复制后的宿主重读请求目标目录；ViewModel 在快照被接受前仍保留旧目录。
fn reloaded_target(model: &ShellViewModel) -> ShellMessage {
    let path = model
        .files
        .pending_path()
        .map(str::to_string)
        .unwrap_or_else(|| model.files.current_path.clone());
    reloaded_at(model, path)
}

fn reloaded_at(model: &ShellViewModel, current_path: String) -> ShellMessage {
    ShellMessage::FileBrowserLoaded(Ok(FileBrowserSnapshot {
        repo_id: model.workspace.active_repo_id.clone().expect("活动仓库"),
        root_path: "C:/repo".into(),
        backend_plugin_id: "local".into(),
        backend_kind: "local".into(),
        cache_state: RepositoryStructureCacheState::Ready,
        indexed_at: None,
        current_path,
        total_entries: 0,
        loaded_count: 0,
        next_offset: None,
        has_more: false,
        special_location: None,
        tree: None,
        entries: Vec::new(),
    }))
}

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

/// 等待中的进度照 Vue 每 220 ms 往上爬：复制从 32% 起，下一拍 35%，再一拍过了 35 改加 3。
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

/// 侧栏的回收站快捷方式和快捷访问是 Vue 的非静默读取，任务弹层出「读取回收站」「读取目录」，读完走到
/// 「整理目录条目」再完成；点文件夹是静默读取（Vue `openFolder` 带 `silent`），不出这一行。
#[test]
fn sidebar_reads_report_like_vue() {
    use super::sidebar::{ShortcutId, SidebarMessage};
    // 宿主的准备帧：交出侧栏排下的目录读取，记下文件列在读，再提交。
    fn prepare(model: &mut ShellViewModel) {
        model.stage_browses();
        crate::sidebar_dispatch::dispatch_prepared_browses(model);
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

/// 多选删除（和移动）在 Vue 里成功以后非静默地重读目录：进度行换成「读取目录」，读回来走到「整理目录条目」再完成。
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

/// 和 Vue 一样只有多选删除出进度行，单条删除不出；变更失败时收起。
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

/// 启动后读到一条硬链接候选的文件页（inbox/cover.png 与 cover.png 哈希一致），之前排下的请求清掉。
fn hardlink_page() -> (ShellViewModel, String) {
    let mut model = super::acceptance_gap_models()
        .into_iter()
        .find(|(scene, _)| *scene == "hardlink-dialog")
        .map(|(_, model)| model)
        .expect("硬链接确认场景");
    let repo_id = model.workspace.active_repo_id.clone().expect("活动仓库");
    let _ = model.files.take_effects();
    let _ = model.workspace.take_effects();
    (model, repo_id)
}

/// 确认硬链接照 Vue `refreshWorkspace(repoId, { directory, hardlinkCandidates, repositorySnapshot })`：确认本身不出
/// 进度行；成功以后重读候选、静默重读仓库摘要、非静默地重读当前目录，任务弹层是「读取目录」，目录读回来收起。
/// 只有这一条候选时确认框关掉。
#[test]
fn confirming_a_hardlink_reloads_candidates_summary_and_directory() {
    let (mut model, repo_id) = hardlink_page();
    assert_eq!(model.files.dialog, FileDialog::Hardlink);
    files(&mut model, FilesMessage::ConfirmHardlink);
    assert_eq!(
        model.files.take_effects(),
        vec![FilesEffect::ConfirmHardlink {
            repo_id: repo_id.clone(),
            candidate_id: "link-1".into()
        }]
    );
    assert!(model.files.mutating, "确认在途时按钮禁用");
    assert!(operation_row(&model).is_none(), "确认本身不出进度行");

    let epoch = model.files.hardlink_epoch();
    files(
        &mut model,
        FilesMessage::HardlinkConfirmed {
            repo_id: repo_id.clone(),
            result: Ok("link-1".into()),
        },
    );
    assert!(!model.files.mutating);
    assert_eq!(model.files.dialog, FileDialog::Closed);
    let effects = model.files.take_effects();
    assert!(
        effects.contains(&FilesEffect::CheckHardlinks {
            repo_id: repo_id.clone(),
            epoch: epoch + 1
        }),
        "重读候选：{effects:?}"
    );
    assert!(
        effects.iter().any(|effect| matches!(effect, FilesEffect::Browse { path, append: false, trash: false, .. } if path.is_empty())),
        "重读当前目录：{effects:?}"
    );
    assert_eq!(
        model.workspace.take_effects(),
        vec![WorkspaceEffect::LoadSnapshotSilent { repo_id }]
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

/// 两条候选，确认第一条成功：照 Vue `isMutatingFiles` 包着整轮刷新，重读的候选回来以前确认框一直处理中（按钮禁用、
/// 点「加入关联」「跳过」和 Escape 都不动），回来以后按新的列表显示下一条。确认以前发出的读取晚到，不会把确认掉的
/// 那一条写回来。
#[test]
fn the_dialog_waits_for_the_reread_and_ignores_reads_from_before_the_confirmation() {
    let (mut model, repo_id) = hardlink_page();
    let prompt = |id: &str, new_path: &str| HardlinkPrompt {
        id: id.into(),
        new_path: new_path.into(),
        existing_path: "cover.png".into(),
        size_label: "2.3 MB".into(),
    };
    let epoch = model.files.hardlink_epoch();
    files(
        &mut model,
        FilesMessage::HardlinksChecked {
            epoch,
            result: Ok(vec![
                prompt("link-1", "inbox/cover.png"),
                prompt("link-2", "drafts/cover.png"),
            ]),
        },
    );
    // 确认以前因为结构更新发出去、还没回来的静默重读。
    let host = crate::shell::host_events::HostMessage::StructureUpdated {
        repo_id: repo_id.clone(),
        reason: "watch".into(),
    };
    model.reduce(ShellMessage::Host(host));
    let before = model.files.take_effects();
    assert!(
        before.contains(&FilesEffect::RefreshHardlinks {
            repo_id: repo_id.clone(),
            epoch
        }),
        "{before:?}"
    );

    files(&mut model, FilesMessage::ConfirmHardlink);
    let _ = model.files.take_effects();
    files(
        &mut model,
        FilesMessage::HardlinkConfirmed {
            repo_id: repo_id.clone(),
            result: Ok("link-1".into()),
        },
    );
    assert_eq!(model.files.dialog, FileDialog::Hardlink);
    let view = HardlinkView::project(&model).expect("确认框还开着");
    assert!(view.busy, "重读回来以前处理中");
    assert_eq!(view.confirm, "处理中...");
    assert_eq!(view.new_path, "drafts/cover.png");
    let _ = model.files.take_effects();
    files(&mut model, FilesMessage::ConfirmHardlink);
    files(&mut model, FilesMessage::SkipHardlink);
    assert!(model.files.take_effects().is_empty(), "处理中不能确认");
    assert_eq!(
        model.files.current_hardlink().map(|item| item.id.as_str()),
        Some("link-2"),
        "处理中不能跳过"
    );
    assert!(model.files.escape_layer().is_none(), "处理中 Escape 不跳过");

    // 确认以前发出的静默重读晚到：里面还有 link-1，不认。
    files(
        &mut model,
        FilesMessage::HardlinksRefreshed {
            epoch,
            result: Ok(vec![
                prompt("link-1", "inbox/cover.png"),
                prompt("link-2", "drafts/cover.png"),
            ]),
        },
    );
    assert!(
        HardlinkView::project(&model).expect("确认框").busy,
        "过期的结果不放开确认框"
    );
    assert_eq!(
        model.files.current_hardlink().map(|item| item.id.as_str()),
        Some("link-2"),
        "确认掉的那一条不回来"
    );

    // 确认以后的重读回来：按新列表显示下一条，放开按钮。
    files(
        &mut model,
        FilesMessage::HardlinksChecked {
            epoch: epoch + 1,
            result: Ok(vec![prompt("link-3", "more/cover.png")]),
        },
    );
    let view = HardlinkView::project(&model).expect("确认框");
    assert!(!view.busy);
    assert_eq!(view.new_path, "more/cover.png", "显示新列表里的下一条");
    files(&mut model, FilesMessage::ConfirmHardlink);
    assert_eq!(
        model.files.take_effects(),
        vec![FilesEffect::ConfirmHardlink {
            repo_id,
            candidate_id: "link-3".into()
        }]
    );
}

/// 确认失败照 Vue 静默重读候选，确认框和这一条留着，错误写在文件列，不重读目录和摘要；重读回来以前确认框处理中，
/// 回来以后按新的列表走（候选已经失效、列表空了就收起）。确认回来时已经换了仓库就什么也不读。
#[test]
fn a_failed_or_stale_hardlink_confirmation_does_not_reload_the_workspace() {
    let (mut model, repo_id) = hardlink_page();
    files(&mut model, FilesMessage::ConfirmHardlink);
    let _ = model.files.take_effects();
    let epoch = model.files.hardlink_epoch();
    files(
        &mut model,
        FilesMessage::HardlinkConfirmed {
            repo_id: repo_id.clone(),
            result: Err("哈希不一致".into()),
        },
    );
    assert_eq!(
        model.files.take_effects(),
        vec![FilesEffect::RefreshHardlinks {
            repo_id: repo_id.clone(),
            epoch: epoch + 1
        }]
    );
    assert!(model.workspace.take_effects().is_empty());
    assert_eq!(model.files.dialog, FileDialog::Hardlink, "这一条留着");
    assert_eq!(model.files.error, "哈希不一致");
    assert!(operation_row(&model).is_none());
    assert!(
        HardlinkView::project(&model).expect("确认框").busy,
        "重读回来以前处理中"
    );
    files(&mut model, FilesMessage::ConfirmHardlink);
    assert!(model.files.take_effects().is_empty(), "处理中不能再点");

    let still = HardlinkPrompt {
        id: "link-1".into(),
        new_path: "inbox/cover.png".into(),
        existing_path: "cover.png".into(),
        size_label: "2.3 MB".into(),
    };
    files(
        &mut model,
        FilesMessage::HardlinksRefreshed {
            epoch: epoch + 1,
            result: Ok(vec![still]),
        },
    );
    assert!(
        !HardlinkView::project(&model).expect("确认框").busy,
        "重读回来放开"
    );
    files(&mut model, FilesMessage::ConfirmHardlink);
    let _ = model.files.take_effects();
    files(
        &mut model,
        FilesMessage::HardlinkConfirmed {
            repo_id: "other-repo".into(),
            result: Ok("link-1".into()),
        },
    );
    assert!(!model.files.mutating);
    assert!(!model.files.hardlink_busy(), "别的仓库的结果不等重读");
    assert!(
        model.files.take_effects().is_empty(),
        "别的仓库的结果不重读"
    );
    assert!(model.workspace.take_effects().is_empty());
}

/// 确认失败以后的重读里，这一条已经失效（服务端删掉了）：列表空了，确认框收起，不留一个画不出来的打开状态。
#[test]
fn a_reread_without_candidates_closes_the_dialog() {
    let (mut model, repo_id) = hardlink_page();
    files(&mut model, FilesMessage::ConfirmHardlink);
    let epoch = model.files.hardlink_epoch();
    files(
        &mut model,
        FilesMessage::HardlinkConfirmed {
            repo_id,
            result: Err("hardlink candidate is no longer valid".into()),
        },
    );
    files(
        &mut model,
        FilesMessage::HardlinksRefreshed {
            epoch: epoch + 1,
            result: Ok(Vec::new()),
        },
    );
    assert_eq!(model.files.dialog, FileDialog::Closed);
    assert!(!model.files.hardlink_busy());
}

/// 删除成功照 Vue `refreshAfterFileMutation` 静默重读仓库摘要，侧栏「回收站」换成摘要里的
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

/// 多选还原照 Vue `restoreTrashEntries`：成功后丢掉变更快照，清空选择，非静默重读回收站。
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

/// 目录快照带着文件夹树时，侧栏目录换成这份树和每个目录的文件数（Vue `applyFileBrowserSnapshot`）。
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
    let with_tree = serde_json::json!({
        "repoId": "acceptance-repo",
        "currentPath": "albums",
        "tree": [{ "path": "albums", "label": "albums", "fileCount": 3, "children": [] }],
    });
    let parsed = crate::files_dispatch::protocol_folder_tree(&with_tree).expect("带树的协议结果");
    assert_eq!(parsed.current_path, "albums");
    assert!(!parsed.trash);
    assert_eq!(parsed.nodes[0].file_count, 3);
    assert!(
        crate::files_dispatch::protocol_folder_tree(
            &serde_json::json!({ "repoId": "acceptance-repo", "currentPath": "" })
        )
        .is_none()
    );
    assert!(
        crate::files_dispatch::protocol_folder_tree(
            &serde_json::json!({ "repoId": "acceptance-repo", "tree": null })
        )
        .is_none()
    );
    let batch = serde_json::json!({
        "repoId": "acceptance-repo",
        "deletedCount": 1,
        "snapshot": {
            "repoId": "acceptance-repo",
            "currentPath": "",
            "specialLocation": "trash",
            "tree": [{ "path": "left", "label": "left", "fileCount": 1, "children": [] }],
        },
    });
    let parsed =
        crate::files_dispatch::protocol_folder_tree(&batch).expect("批量删除的 snapshot.tree");
    assert_eq!(parsed.nodes[0].path, "left");
    assert!(parsed.trash);
    assert!(
        crate::files_dispatch::protocol_folder_tree(&serde_json::json!({
            "repoId": "acceptance-repo",
            "currentPath": "",
            "tree": { "path": "nope" },
        }))
        .is_none()
    );
    assert!(
        crate::files_dispatch::protocol_folder_tree(&serde_json::json!({
            "currentPath": "",
            "tree": [],
        }))
        .is_none()
    );

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

fn protocol_tree(path: &str, nodes: Vec<FileTreeNode>) -> Option<ProtocolFolderTree> {
    Some(ProtocolFolderTree {
        repo_id: "acceptance-repo".into(),
        current_path: path.into(),
        trash: false,
        nodes,
    })
}

fn finished(folder_tree: Option<ProtocolFolderTree>) -> FilesMessage {
    FilesMessage::ProtocolFinished {
        result: Ok(()),
        reload: true,
        hardlinks: false,
        folder_tree,
    }
}

fn named_file(path: &str, name: &str, kind: &str) -> FileBrowserEntry {
    let mut entry = browser_file(path);
    entry.name = name.into();
    entry.kind = kind.into();
    entry
}

/// 侧栏目录树的路径，父级在子级前面。
fn folder_paths(model: &ShellViewModel) -> Vec<String> {
    fn walk(folders: &[super::SidebarFolder], out: &mut Vec<String>) {
        for folder in folders {
            out.push(folder.path.clone());
            walk(&folder.children, out);
        }
    }
    let mut out = Vec::new();
    walk(&model.sidebar.folders, &mut out);
    out
}

fn tree_node(path: &str, file_count: usize, children: Vec<FileTreeNode>) -> FileTreeNode {
    FileTreeNode {
        path: path.into(),
        label: path.rsplit('/').next().unwrap_or(path).into(),
        file_count,
        children,
    }
}

fn browser_of(model: &ShellViewModel) -> FileBrowserSnapshot {
    match reloaded(model) {
        ShellMessage::FileBrowserLoaded(Ok(browser)) => browser,
        _ => unreachable!("重读目录的消息"),
    }
}

/// 测试里的回收站文件行。只填路径和名字，其余保持空。
fn browser_file(path: &str) -> FileBrowserEntry {
    FileBrowserEntry {
        path: path.into(),
        name: path.into(),
        kind: "file".into(),
        extension: None,
        size_bytes: None,
        size_label: None,
        modified_at: None,
        asset_id: None,
        status: None,
        thumbnail_path: None,
        thumbnail_custom: false,
        hardlink_group_id: None,
        hardlink_state: None,
        tags: Vec::new(),
        alias_paths: Vec::new(),
        folder_metadata: None,
        metadata: BTreeMap::new(),
        is_virtual: false,
        provider_id: None,
        provider_item_id: None,
        source_payload: None,
        local_absolute_path: None,
    }
}

/// 测试用的仓库摘要：只改回收站计数，素材列表空着，分类计数跟着这份摘要走。
fn snapshot_with_trash(repo_id: &str, trash_count: i64) -> RepositorySnapshot {
    RepositorySnapshot {
        repository: RepositorySummary {
            repo_id: repo_id.into(),
            name: "默认资源库".into(),
            path: "C:/acceptance".into(),
            backend: RepositoryBackendSummary {
                plugin_id: "momobako.source.local-filesystem".into(),
                kind: "local-filesystem".into(),
                name: "本地文件系统".into(),
                capabilities: vec!["write".into()],
            },
            status: "ready".into(),
            asset_count: 0,
            updated_at: String::new(),
            local_cache: None,
            authentication: None,
        },
        folder_label: String::new(),
        folders: Vec::new(),
        assets: Vec::<AssetSummary>::new(),
        playlists: Vec::new(),
        quick_access: Vec::new(),
        tag_groups: Vec::new(),
        metadata_fields: Vec::new(),
        recent_revision_count: 0,
        overview: RepositoryOverview {
            total_size_bytes: 0,
            total_size_label: "0 B".into(),
            file_count: 0,
            folder_count: 0,
            trash_count,
            readme_content: None,
        },
    }
}

#[path = "folder_tree_reread_tests.rs"]
mod folder_tree_reread_tests;
