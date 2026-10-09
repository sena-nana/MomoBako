//! 侧栏对话框投影的单测：取值、同一状态两次投影相等，以及哪些消息改投影、哪些不改。

use super::smart::SmartDialogView;
use super::{FolderDialogView, PlaylistCreateView, RepositoryDeleteView};
use crate::shell::workspace::DeleteMode;
use crate::shell::{GapMessage, ShellMessage, ShellViewModel, SidebarMessage};

fn scene(name: &str) -> ShellViewModel {
    crate::shell::acceptance_gap_models()
        .into_iter()
        .find(|(scene, _)| *scene == name)
        .unwrap_or_else(|| panic!("没有场景 {name}"))
        .1
}

/// 不改侧栏对话框的消息：播放音量和标题栏搜索。
fn unrelated(model: &mut ShellViewModel) {
    model.reduce(ShellMessage::Player(crate::shell::player::PlayerMessage::SetVolume(0.2)));
    model.reduce(ShellMessage::Inspect(crate::shell::InspectMessage::SetQuery("封面".into())));
}

#[test]
fn folder_dialog_projection_follows_the_draft_and_the_file_service() {
    let mut model = scene("folder-create-dialog");
    let view = FolderDialogView::project(&model).expect("对话框开着");
    assert_eq!((view.title, view.action, view.placeholder), ("新建文件夹", "创建", "输入文件夹名称"));
    assert_eq!(view.summary, "将在 根目录 下创建新文件夹。");
    assert!(view.blocked && !view.busy, "名称为空时主按钮不可用");
    assert_eq!(FolderDialogView::project(&model), Some(view.clone()), "同一状态的投影相等");
    unrelated(&mut model);
    assert_eq!(FolderDialogView::project(&model), Some(view.clone()), "无关消息不该改投影");

    model.reduce(ShellMessage::Sidebar(SidebarMessage::Gap(GapMessage::SetFolderValue("封面".into()))));
    let typed = FolderDialogView::project(&model).expect("对话框开着");
    assert!(!typed.blocked, "有名称后主按钮可用");
    model.files.mutating = true;
    let busy = FolderDialogView::project(&model).expect("对话框开着");
    assert!(busy.busy && busy.blocked, "文件服务忙时不能关也不能提交");

    model.files.mutating = false;
    model.reduce(ShellMessage::Sidebar(SidebarMessage::Gap(GapMessage::CloseFolderDialog)));
    assert_eq!(FolderDialogView::project(&model), None, "关掉以后没有投影");
}

#[test]
fn repository_delete_projection_lists_the_three_modes() {
    let mut model = scene("repo-delete-dialog");
    let view = RepositoryDeleteView::project(&model).expect("对话框开着");
    assert!(view.summary.contains("默认资源库"), "摘要写出资源库名：{}", view.summary);
    assert!(!view.deleting);
    assert!(!view.modes[0].1, "只删除记录总是可用");
    assert_eq!(RepositoryDeleteView::project(&model), Some(view.clone()));
    unrelated(&mut model);
    assert_eq!(RepositoryDeleteView::project(&model), Some(view));

    model.workspace.deleting_mode = Some(DeleteMode::RecordOnly);
    let deleting = RepositoryDeleteView::project(&model).expect("对话框开着");
    assert!(deleting.deleting && deleting.modes.iter().all(|(_, disabled)| *disabled), "删除中三个选项都禁用");
}

#[test]
fn playlist_and_smart_dialog_projections_follow_their_drafts() {
    let mut model = scene("playlist-create-dialog");
    let view = PlaylistCreateView::project(&model).expect("对话框开着");
    assert!(view.blocked, "名称为空时不能创建");
    assert!(!view.players.is_empty(), "播放类型来自内置播放器");
    model.reduce(ShellMessage::NewPlaylistNameChanged("通勤歌单".into()));
    let selected = view.players[0].0.clone();
    model.reduce(ShellMessage::SelectPlaylistPlayer(selected.clone()));
    let ready = PlaylistCreateView::project(&model).expect("对话框开着");
    assert_eq!(ready.selected.as_deref(), Some(selected.as_str()));
    assert!(!ready.blocked);

    let mut model = scene("smart-folder-dialog");
    let view = SmartDialogView::project(&model).expect("对话框开着");
    assert_eq!((view.title, view.action), ("新建智能文件夹", "创建"));
    assert_eq!(view.parents.first().map(|(_, name)| name.as_str()), Some("顶层智能文件夹"));
    assert_eq!((view.match_mode.as_str(), view.sort_direction.as_str()), ("and", "asc"));
    assert!(view.blocked);
    unrelated(&mut model);
    assert_eq!(SmartDialogView::project(&model), Some(view));
}
