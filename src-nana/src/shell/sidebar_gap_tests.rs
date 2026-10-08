//! 侧栏对话框、右键菜单、悬停打开和弹层夹取的回归。

use super::super::super::{ShellMessage, ShellViewModel};
use super::{FolderDeleteMode, FolderMutation, GapMessage, FOLDER_CREATE_TITLE, FOLDER_RENAME_TITLE, SMART_EDIT_TITLE};
use crate::shell::workspace::WorkspaceRepository;
use crate::shell::SidebarMessage;

fn shell() -> ShellViewModel {
    let mut model = ShellViewModel::default();
    model.repository_id = Some("repo".into());
    model.workspace.active_repo_id = Some("repo".into());
    model.workspace.repositories.push(WorkspaceRepository {
        repo_id: "repo".into(),
        name: "库".into(),
        path: "C:/repo".into(),
        status: "ready".into(),
        backend_plugin_id: "filesystem".into(),
        capabilities: vec!["write".into()],
        cache_required: false,
        cache_status: String::new(),
    });
    model.sidebar.bind_repository(Some("repo"), false);
    model
}

fn gap(model: &mut ShellViewModel, message: GapMessage) {
    model.reduce(ShellMessage::Sidebar(SidebarMessage::Gap(message)));
}

#[test]
fn folder_dialogs_use_the_vue_copy_and_escape_closes_the_top_one() {
    let mut model = shell();
    gap(&mut model, GapMessage::OpenFolderCreate("photos".into()));
    let dialog = &model.sidebar.folder_dialog;
    assert_eq!(dialog.title(), FOLDER_CREATE_TITLE);
    assert_eq!(dialog.summary(), "将在 photos 下创建新文件夹。");
    assert_eq!(dialog.action_label(), "创建");
    assert_eq!(dialog.placeholder(), "输入文件夹名称");
    gap(&mut model, GapMessage::SetFolderValue("  ".into()));
    gap(&mut model, GapMessage::SubmitFolderDialog);
    assert!(model.sidebar.folder_dialog.open);
    assert!(!model.sidebar.folder_dialog.submitting, "空名称不提交");
    gap(&mut model, GapMessage::Escape);
    assert!(!model.sidebar.folder_dialog.open);
    gap(&mut model, GapMessage::OpenFolderRename { path: "photos".into(), label: "照片".into() });
    let dialog = &model.sidebar.folder_dialog;
    assert_eq!(dialog.title(), FOLDER_RENAME_TITLE);
    assert_eq!(dialog.value, "照片");
    assert_eq!(dialog.summary(), "正在重命名 照片。");
    assert_eq!(dialog.action_label(), "保存");
    gap(&mut model, GapMessage::OpenFolderCreate(String::new()));
    assert_eq!(model.sidebar.folder_dialog.summary(), "将在 根目录 下创建新文件夹。");
}

#[test]
fn folder_dialog_waits_for_the_file_service_and_keeps_errors() {
    let mut model = shell();
    gap(&mut model, GapMessage::OpenFolderCreate("photos".into()));
    gap(&mut model, GapMessage::SetFolderValue("trips".into()));
    gap(&mut model, GapMessage::SubmitFolderDialog);
    assert!(model.sidebar.folder_dialog.open, "交给文件服务后对话框保持打开");
    assert!(model.files.mutating, "建目录已经交给文件服务");
    gap(&mut model, GapMessage::CloseFolderDialog);
    assert!(model.sidebar.folder_dialog.open, "文件服务处理中不能关");
    // 文件服务失败：对话框留着并显示错误。
    model.files.mutating = false;
    model.files.error = "目录已存在".into();
    model.reduce(ShellMessage::Refresh);
    assert!(model.sidebar.folder_dialog.open);
    assert_eq!(model.sidebar.folder_dialog.error, "目录已存在");
    // 再提交一次，这次成功：关掉并展开父级。
    gap(&mut model, GapMessage::SubmitFolderDialog);
    assert!(model.files.error.is_empty(), "新的提交清掉上一次的错误");
    model.files.mutating = false;
    model.reduce(ShellMessage::Refresh);
    assert!(!model.sidebar.folder_dialog.open);
    assert!(model.sidebar.expanded_folders.iter().any(|path| path == "photos"));
}

#[test]
fn folder_delete_offers_move_to_parent_and_trash() {
    let mut model = shell();
    gap(&mut model, GapMessage::OpenFolderMenu { path: "photos".into(), label: "照片".into(), x: 40.0, y: 300.0 });
    assert!(model.sidebar.folder_menu.is_some());
    gap(&mut model, GapMessage::OpenFolderDelete { path: "photos".into(), label: "照片".into() });
    assert!(model.sidebar.folder_menu.is_none(), "打开对话框时收起右键菜单");
    assert!(model.sidebar.folder_delete_open());
    gap(&mut model, GapMessage::ConfirmFolderDelete(FolderDeleteMode::MoveToParent));
    assert!(model.sidebar.folder_delete_submitting);
    assert!(model.files.mutating);
    let effects = model.files.take_effects();
    assert!(
        effects.iter().any(|effect| matches!(effect, crate::shell::FilesEffect::Delete { mode: Some(mode), paths, .. } if mode == "moveToParent" && paths == &["photos".to_string()])),
        "处理文件夹走文件服务的 moveToParent：{effects:?}"
    );
    model.files.mutating = false;
    model.reduce(ShellMessage::Refresh);
    assert!(!model.sidebar.folder_delete_open());
    assert_eq!(FolderDeleteMode::Delete.as_str(), "delete");
    assert!(matches!(
        FolderMutation::Delete { repo_id: "r".into(), path: "p".into(), mode: FolderDeleteMode::Delete },
        FolderMutation::Delete { mode: FolderDeleteMode::Delete, .. }
    ));
}

#[test]
fn hover_opens_after_450ms_and_folders_follow_the_missing_lock() {
    let mut model = shell();
    let _ = model.sidebar.take_effects();
    gap(&mut model, GapMessage::FolderHover { path: "photos".into(), now_ms: 0, dragging: true });
    gap(&mut model, GapMessage::FolderHover { path: "photos".into(), now_ms: 449, dragging: true });
    assert_eq!(model.sidebar.current_directory, "");
    assert_eq!(model.current_directory, "");
    assert!(!model.sidebar.take_effects().iter().any(|effect| matches!(effect, crate::shell::SidebarEffect::Browse { path, .. } if path == "photos")));
    gap(&mut model, GapMessage::FolderHover { path: "photos".into(), now_ms: 450, dragging: true });
    assert_eq!(model.sidebar.current_directory, "photos");
    assert_eq!(model.current_directory, "photos");
    assert!(model.sidebar.expanded_folders.iter().any(|path| path == "photos"));
    assert!(model.sidebar.take_effects().iter().any(|effect| matches!(effect, crate::shell::SidebarEffect::Browse { path, .. } if path == "photos")));
    assert!(model.sidebar.playlists_visible(false));
    model.sidebar.bind_repository(Some("repo"), true);
    assert!(!model.sidebar.playlists_visible(true));
    gap(&mut model, GapMessage::OpenFolderMenu { path: "photos".into(), label: "照片".into(), x: 0.0, y: 0.0 });
    assert!(model.sidebar.folder_menu.is_some(), "可用仓库能打开文件夹右键菜单");
    gap(&mut model, GapMessage::CloseFolderMenu);
    model.workspace.repositories[0].status = "missing".into();
    gap(&mut model, GapMessage::OpenFolderMenu { path: "photos".into(), label: "照片".into(), x: 0.0, y: 0.0 });
    assert!(model.sidebar.folder_menu.is_none(), "缺失仓库锁住文件夹右键菜单");
}

#[test]
fn smart_edit_delete_and_playlist_play_match_vue() {
    let mut model = shell();
    model.sidebar.apply_smart_folders("repo", Ok(vec![super::super::SidebarSmartFolder {
        id: "sf".into(),
        parent_id: None,
        name: "高评分".into(),
        filter: Default::default(),
        children: Vec::new(),
    }]));
    gap(&mut model, GapMessage::OpenSmartEdit("sf".into()));
    assert!(model.sidebar.smart_draft.mode_edit);
    assert_eq!(model.sidebar.smart_draft.name, "高评分");
    assert_eq!(SMART_EDIT_TITLE, "编辑智能文件夹");
    gap(&mut model, GapMessage::OpenSmartDelete { id: "sf".into(), label: "高评分".into() });
    assert!(model.sidebar.smart_delete_open());
    assert_eq!(super::SMART_DELETE_TITLE, "删除智能文件夹");
    gap(&mut model, GapMessage::PlayPlaylist("pl-1".into()));
    assert_eq!(model.sidebar.pending_play.as_deref(), Some("pl-1"));
}
