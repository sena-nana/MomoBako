//! 文件变更在任务弹层里的仓库操作行，对齐 Vue `operationProgress` 和 `TaskPopover.vue` 的
//! `workspace-operation`：从文件页的操作开始，看任务弹层的行和侧栏「任务」的计数。

use crate::backend::services::repository::{FileBrowserSnapshot, RepositoryStructureCacheState};

use super::files::{FileDialog, FilesMessage};
use super::workspace_dialogs::HardlinkView;
use super::repo_operation::{CLIMB_INTERVAL_MS, FINISH_LINGER_MS};
use super::{FilesEffect, HardlinkPrompt, ShellMessage, ShellViewModel, WorkspaceEffect};

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
    ShellMessage::FileBrowserLoaded(Ok(FileBrowserSnapshot {
        repo_id: model.workspace.active_repo_id.clone().expect("活动仓库"),
        root_path: "C:/repo".into(),
        backend_plugin_id: "local".into(),
        backend_kind: "local".into(),
        cache_state: RepositoryStructureCacheState::Ready,
        indexed_at: None,
        current_path: model.files.current_path.clone(),
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
    assert_eq!(operation_row(&model), Some(("复制文件".into(), "创建硬链接或复制文件".into(), 32.0)));
    assert_eq!(model.task_rows().len(), idle + 1);

    files(&mut model, FilesMessage::ProtocolFinished { result: Ok(()), reload: true, hardlinks: false });
    assert_eq!(operation_row(&model), Some(("复制文件".into(), "刷新文件索引".into(), 84.0)));

    let message = reloaded(&model);
    model.reduce(message);
    assert_eq!(operation_row(&model), Some(("复制文件".into(), "刷新文件索引".into(), 100.0)), "目录读回来先停在 100%");
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
    model.reduce(ShellMessage::Sidebar(SidebarMessage::SelectShortcut(ShortcutId::Trash)));
    prepare(&mut model);
    assert_eq!(operation_row(&model), Some(("读取回收站".into(), "正在读取回收站".into(), 14.0)));
    let mut trash = match reloaded(&model) {
        ShellMessage::FileBrowserLoaded(Ok(snapshot)) => snapshot,
        _ => unreachable!(),
    };
    trash.special_location = Some("trash".into());
    model.reduce(ShellMessage::FileBrowserLoaded(Ok(trash)));
    assert_eq!(operation_row(&model), Some(("读取回收站".into(), "整理目录条目".into(), 100.0)));
    model.operation.tick(FINISH_LINGER_MS);

    assert!(operation_row(&model).is_none(), "停留以后收起");
    model.reduce(ShellMessage::Sidebar(SidebarMessage::OpenFolder("assets".into())));
    prepare(&mut model);
    assert!(model.files.loading, "文件列在读 assets");
    assert!(operation_row(&model).is_none(), "点文件夹是静默读取");
}

/// 多选删除（和移动）在 Vue 里成功以后非静默地重读目录：进度行换成「读取目录」，读回来走到「整理目录条目」再完成。
#[test]
fn multi_deletes_hand_the_row_to_the_directory_read() {
    let mut model = files_page();
    let both = vec!["cover.png".to_string(), "notes/page.pdf".to_string()];
    model.files.set_drag_selection(both, Some("cover.png".into()), Some("cover.png".into()));
    files(&mut model, FilesMessage::DeleteSelected);
    files(&mut model, FilesMessage::ProtocolFinished { result: Ok(()), reload: true, hardlinks: false });
    assert_eq!(operation_row(&model), Some(("读取目录".into(), "正在读取根目录".into(), 14.0)));
    let message = reloaded(&model);
    model.reduce(message);
    assert_eq!(operation_row(&model), Some(("读取目录".into(), "整理目录条目".into(), 100.0)));
}

/// 和 Vue 一样只有多选删除出进度行，单条删除不出；变更失败时收起。
#[test]
fn only_multi_delete_shows_the_operation_and_failures_clear_it() {
    let mut model = files_page();
    model.files.set_drag_selection(vec!["cover.png".into()], Some("cover.png".into()), Some("cover.png".into()));
    files(&mut model, FilesMessage::DeleteSelected);
    assert!(model.files.mutating);
    assert!(operation_row(&model).is_none(), "单条删除不出进度行");
    files(&mut model, FilesMessage::ProtocolFinished { result: Err("删除失败".into()), reload: false, hardlinks: false });

    let both = vec!["cover.png".to_string(), "notes/page.pdf".to_string()];
    model.files.set_drag_selection(both, Some("cover.png".into()), Some("cover.png".into()));
    files(&mut model, FilesMessage::DeleteSelected);
    assert_eq!(operation_row(&model), Some(("删除文件".into(), "准备处理 2 个条目".into(), 10.0)));
    files(&mut model, FilesMessage::ProtocolFinished { result: Err("删除失败".into()), reload: false, hardlinks: false });
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
    assert_eq!(model.files.take_effects(), vec![FilesEffect::ConfirmHardlink { repo_id: repo_id.clone(), candidate_id: "link-1".into() }]);
    assert!(model.files.mutating, "确认在途时按钮禁用");
    assert!(operation_row(&model).is_none(), "确认本身不出进度行");

    let epoch = model.files.hardlink_epoch();
    files(&mut model, FilesMessage::HardlinkConfirmed { repo_id: repo_id.clone(), result: Ok("link-1".into()) });
    assert!(!model.files.mutating);
    assert_eq!(model.files.dialog, FileDialog::Closed);
    let effects = model.files.take_effects();
    assert!(effects.contains(&FilesEffect::CheckHardlinks { repo_id: repo_id.clone(), epoch: epoch + 1 }), "重读候选：{effects:?}");
    assert!(
        effects.iter().any(|effect| matches!(effect, FilesEffect::Browse { path, append: false, trash: false, .. } if path.is_empty())),
        "重读当前目录：{effects:?}"
    );
    assert_eq!(model.workspace.take_effects(), vec![WorkspaceEffect::LoadSnapshotSilent { repo_id }]);
    assert_eq!(operation_row(&model), Some(("读取目录".into(), "正在读取根目录".into(), 14.0)));
    let message = reloaded(&model);
    model.reduce(message);
    assert_eq!(operation_row(&model), Some(("读取目录".into(), "整理目录条目".into(), 100.0)));
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
    files(&mut model, FilesMessage::HardlinksChecked { epoch, result: Ok(vec![prompt("link-1", "inbox/cover.png"), prompt("link-2", "drafts/cover.png")]) });
    // 确认以前因为结构更新发出去、还没回来的静默重读。
    let host = crate::shell::host_events::HostMessage::StructureUpdated { repo_id: repo_id.clone(), reason: "watch".into() };
    model.reduce(ShellMessage::Host(host));
    let before = model.files.take_effects();
    assert!(before.contains(&FilesEffect::RefreshHardlinks { repo_id: repo_id.clone(), epoch }), "{before:?}");

    files(&mut model, FilesMessage::ConfirmHardlink);
    let _ = model.files.take_effects();
    files(&mut model, FilesMessage::HardlinkConfirmed { repo_id: repo_id.clone(), result: Ok("link-1".into()) });
    assert_eq!(model.files.dialog, FileDialog::Hardlink);
    let view = HardlinkView::project(&model).expect("确认框还开着");
    assert!(view.busy, "重读回来以前处理中");
    assert_eq!(view.confirm, "处理中...");
    assert_eq!(view.new_path, "drafts/cover.png");
    let _ = model.files.take_effects();
    files(&mut model, FilesMessage::ConfirmHardlink);
    files(&mut model, FilesMessage::SkipHardlink);
    assert!(model.files.take_effects().is_empty(), "处理中不能确认");
    assert_eq!(model.files.current_hardlink().map(|item| item.id.as_str()), Some("link-2"), "处理中不能跳过");
    assert!(model.files.escape_layer().is_none(), "处理中 Escape 不跳过");

    // 确认以前发出的静默重读晚到：里面还有 link-1，不认。
    files(&mut model, FilesMessage::HardlinksRefreshed { epoch, result: Ok(vec![prompt("link-1", "inbox/cover.png"), prompt("link-2", "drafts/cover.png")]) });
    assert!(HardlinkView::project(&model).expect("确认框").busy, "过期的结果不放开确认框");
    assert_eq!(model.files.current_hardlink().map(|item| item.id.as_str()), Some("link-2"), "确认掉的那一条不回来");

    // 确认以后的重读回来：按新列表显示下一条，放开按钮。
    files(&mut model, FilesMessage::HardlinksChecked { epoch: epoch + 1, result: Ok(vec![prompt("link-3", "more/cover.png")]) });
    let view = HardlinkView::project(&model).expect("确认框");
    assert!(!view.busy);
    assert_eq!(view.new_path, "more/cover.png", "显示新列表里的下一条");
    files(&mut model, FilesMessage::ConfirmHardlink);
    assert_eq!(model.files.take_effects(), vec![FilesEffect::ConfirmHardlink { repo_id, candidate_id: "link-3".into() }]);
}

/// 确认失败照 Vue 静默重读候选，确认框和这一条留着，错误写在文件列，不重读目录和摘要；重读回来以前确认框处理中，
/// 回来以后按新的列表走（候选已经失效、列表空了就收起）。确认回来时已经换了仓库就什么也不读。
#[test]
fn a_failed_or_stale_hardlink_confirmation_does_not_reload_the_workspace() {
    let (mut model, repo_id) = hardlink_page();
    files(&mut model, FilesMessage::ConfirmHardlink);
    let _ = model.files.take_effects();
    let epoch = model.files.hardlink_epoch();
    files(&mut model, FilesMessage::HardlinkConfirmed { repo_id: repo_id.clone(), result: Err("哈希不一致".into()) });
    assert_eq!(model.files.take_effects(), vec![FilesEffect::RefreshHardlinks { repo_id: repo_id.clone(), epoch: epoch + 1 }]);
    assert!(model.workspace.take_effects().is_empty());
    assert_eq!(model.files.dialog, FileDialog::Hardlink, "这一条留着");
    assert_eq!(model.files.error, "哈希不一致");
    assert!(operation_row(&model).is_none());
    assert!(HardlinkView::project(&model).expect("确认框").busy, "重读回来以前处理中");
    files(&mut model, FilesMessage::ConfirmHardlink);
    assert!(model.files.take_effects().is_empty(), "处理中不能再点");

    let still = HardlinkPrompt {
        id: "link-1".into(),
        new_path: "inbox/cover.png".into(),
        existing_path: "cover.png".into(),
        size_label: "2.3 MB".into(),
    };
    files(&mut model, FilesMessage::HardlinksRefreshed { epoch: epoch + 1, result: Ok(vec![still]) });
    assert!(!HardlinkView::project(&model).expect("确认框").busy, "重读回来放开");
    files(&mut model, FilesMessage::ConfirmHardlink);
    let _ = model.files.take_effects();
    files(&mut model, FilesMessage::HardlinkConfirmed { repo_id: "other-repo".into(), result: Ok("link-1".into()) });
    assert!(!model.files.mutating);
    assert!(!model.files.hardlink_busy(), "别的仓库的结果不等重读");
    assert!(model.files.take_effects().is_empty(), "别的仓库的结果不重读");
    assert!(model.workspace.take_effects().is_empty());
}

/// 确认失败以后的重读里，这一条已经失效（服务端删掉了）：列表空了，确认框收起，不留一个画不出来的打开状态。
#[test]
fn a_reread_without_candidates_closes_the_dialog() {
    let (mut model, repo_id) = hardlink_page();
    files(&mut model, FilesMessage::ConfirmHardlink);
    let epoch = model.files.hardlink_epoch();
    files(&mut model, FilesMessage::HardlinkConfirmed { repo_id, result: Err("hardlink candidate is no longer valid".into()) });
    files(&mut model, FilesMessage::HardlinksRefreshed { epoch: epoch + 1, result: Ok(Vec::new()) });
    assert_eq!(model.files.dialog, FileDialog::Closed);
    assert!(!model.files.hardlink_busy());
}
