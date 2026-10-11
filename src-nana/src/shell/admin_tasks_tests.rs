//! 任务弹层里的后台任务：每一行写明是什么任务（照任务协议写中文，不再是泛泛的「进度」），仓库操作那一行
//! 已经表示的同一件事不再单列一行，侧栏「任务」的计数也只算去重以后的行。

use crate::backend::services::mutsuki_runner::{
    PROTOCOL_ENTRY_COPY, PROTOCOL_PLAYBACK_PREPARE, PROTOCOL_REPOSITORY_SYNC, PROTOCOL_THUMBNAIL_REQUEST,
};
use crate::backend::services::repository::TaskProgressSnapshot;
use crate::shell::files::{FileDialog, FilesMessage};
use crate::shell::{ShellMessage, ShellViewModel, SidebarMessage};

fn scene(name: &str) -> ShellViewModel {
    crate::shell::acceptance_gap_models()
        .into_iter()
        .find(|(scene, _)| *scene == name)
        .unwrap_or_else(|| panic!("没有场景 {name}"))
        .1
}

/// 运行中、没有自己标题的任务快照（宿主起的任务跑完才交回进度事件，跑的时候都没有标题）。
fn task(task_id: &str, protocol_id: &str) -> TaskProgressSnapshot {
    TaskProgressSnapshot {
        task_id: task_id.into(),
        protocol_id: protocol_id.into(),
        status: "running".into(),
        phase: None,
        label: None,
        current: None,
        total: None,
        percent: None,
        error: None,
        updated_at: "2026-10-11T08:00:00Z".into(),
    }
}

/// 弹层各行的标题，按字排好。
fn labels(model: &ShellViewModel) -> Vec<String> {
    let mut labels = model.task_rows().into_iter().map(|row| row.label).collect::<Vec<_>>();
    labels.sort();
    labels
}

/// 没有标题的后台任务按协议写成用户看得懂的中文。
#[test]
fn every_background_task_says_what_it_is() {
    let mut model = ShellViewModel::default();
    model.reduce(ShellMessage::TaskProgressLoaded(vec![
        task("momobako.task.1", PROTOCOL_THUMBNAIL_REQUEST),
        task("momobako.task.2", PROTOCOL_PLAYBACK_PREPARE),
        task("momobako.task.3", PROTOCOL_REPOSITORY_SYNC),
    ]));
    assert_eq!(labels(&model), ["准备播放", "同步资源库", "生成缩略图"]);
}

/// 刷新文件夹树时的同步任务已经由「刷新文件树」这一行表示，不再单列；别的任务照常列出。
#[test]
fn a_sync_shown_as_the_repository_operation_is_not_listed_again() {
    let mut model = scene("live-files-plain");
    model.reduce(ShellMessage::Sidebar(SidebarMessage::RefreshFolderTree));
    assert!(
        model.task_rows().iter().any(|row| row.id == "workspace-operation" && row.label == "刷新文件树"),
        "刷新文件夹树应该出仓库操作行"
    );
    model.reduce(ShellMessage::TaskProgressLoaded(vec![
        task("momobako.task.7", PROTOCOL_REPOSITORY_SYNC),
        task("momobako.task.8", PROTOCOL_THUMBNAIL_REQUEST),
    ]));
    assert_eq!(labels(&model), ["刷新文件树", "生成缩略图"]);
}

/// 复制文件：「复制文件」这一行就是复制任务，任务弹层和侧栏「任务」的计数都只算一行。
#[test]
fn a_copy_shown_as_the_repository_operation_is_counted_once() {
    let mut model = scene("files-list-mode");
    let files = |model: &mut ShellViewModel, message: FilesMessage| model.reduce(ShellMessage::Files(message));
    files(&mut model, FilesMessage::ActivateRow("cover.png".into()));
    files(&mut model, FilesMessage::OpenDialog(FileDialog::Copy));
    files(&mut model, FilesMessage::DraftChanged("assets".into()));
    files(&mut model, FilesMessage::SubmitDialog);
    model.reduce(ShellMessage::TaskProgressLoaded(vec![task("momobako.task.9", PROTOCOL_ENTRY_COPY)]));
    assert_eq!(labels(&model), ["复制文件"]);
    assert_eq!(model.task_rows().len(), 1, "「任务」的计数只算去重以后的行");
}
